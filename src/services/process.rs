use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::info;
use crate::config::ProcessingConfig;
use crate::error::{AppError, AppResult};
use crate::models::job::Job;
use crate::services::ConnectionPoolManager;

pub struct ProcessService {
    config: ProcessingConfig,
    working_dir: PathBuf,
    pool_manager: Arc<ConnectionPoolManager>,
}

impl ProcessService {
    pub fn new(config: ProcessingConfig, working_dir: PathBuf, pool_manager: Arc<ConnectionPoolManager>) -> Self {
        Self {
            config,
            working_dir,
            pool_manager,
        }
    }

    pub async fn process(&self, job: &mut Job, input_path: &Path) -> AppResult<PathBuf> {
        // Acquire processing permit before starting
        info!("Waiting for processing permit for job {}", job.id);
        let _permit = self.pool_manager.acquire_processing_permit().await
            .map_err(|e| AppError::Internal(format!("Failed to acquire processing permit: {e}")))?;
        
        info!("Processing permit acquired for job {}", job.id);
        // Note: Job status is updated to Processing at the higher level

        // Create output path
        let output_filename = format!("{}_processed.mp4", job.id);
        let output_path = self.working_dir.join(&output_filename);

        // Build optimized ffmpeg command with better compatibility and compression
        let process_result = timeout(
            self.config.processing_timeout,
            Command::new(&self.config.ffmpeg_command)
                .args([
                    "-i", input_path.to_str().ok_or_else(|| 
                        AppError::Processing("Invalid input path".to_string()))?,
                    "-c:v", &self.config.video_codec,
                    "-preset", &self.config.preset,
                    "-crf", &self.config.crf.to_string(),
                    "-profile:v", "high",
                    "-level", "4.0",
                    "-pix_fmt", "yuv420p",
                    "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                    "-c:a", &self.config.audio_codec,
                    "-b:a", &self.config.audio_bitrate,
                    "-ac", "2", // Force stereo for compatibility
                    "-threads", "0", // Use all available cores since we limit concurrent processing
                    "-movflags", "+faststart",
                    "-max_muxing_queue_size", "1024",
                    output_path.to_str().ok_or_else(|| 
                        AppError::Processing("Invalid output path".to_string()))?,
])
                .output(),
        ).await;

        match process_result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    // Clean up partial output file on processing failure
                    if output_path.exists() {
                        let _ = tokio::fs::remove_file(&output_path).await;
                    }
                    let error_message = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(AppError::Processing(error_message));
                }

                if !output_path.exists() {
                    return Err(AppError::Processing(format!(
                        "Output file not created: {}",
                        output_path.display()
                    )));
                }

                Ok(output_path)
            }
            Ok(Err(error)) => Err(AppError::Processing(format!("FFmpeg command failed: {error}"))),
            Err(_) => {
                // Clean up partial output file on timeout
                if output_path.exists() {
                    let _ = tokio::fs::remove_file(&output_path).await;
                }
                Err(AppError::Timeout(format!(
                    "Processing timed out after {} seconds",
                    self.config.processing_timeout.as_secs()
                )))
            }
        }
    }
}