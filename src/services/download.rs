use crate::config::DownloadConfig;
use crate::error::{AppError, AppResult};
use crate::models::job::Job;
use crate::services::{SecurityValidator, ConnectionPoolManager};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};

pub struct DownloadService {
    config: DownloadConfig,
    working_dir: PathBuf,
    security_validator: SecurityValidator,
    pool_manager: Arc<ConnectionPoolManager>,
}

impl DownloadService {
    pub fn new(config: DownloadConfig, working_dir: PathBuf, security_config: &crate::config::SecurityConfig, pool_manager: Arc<ConnectionPoolManager>) -> Self {
        let security_validator = SecurityValidator::new(
            config.allowed_domains.clone(),
            security_config.max_file_size_mb as u32,
            security_config.max_url_length as u32,
        );
        Self {
            config,
            working_dir,
            security_validator,
            pool_manager,
        }
    }
    
    pub async fn download(&self, job: &mut Job) -> AppResult<PathBuf> {
        // Acquire download permit before starting
        info!("Waiting for download permit for job {}", job.id);
        let _permit = self.pool_manager.acquire_download_permit().await
            .map_err(|e| AppError::Internal(format!("Failed to acquire download permit: {e}")))?;
        
        info!("Download permit acquired for job {}", job.id);
        // Note: Job status is updated to Downloading at the higher level
        
        // Enhanced security validation
        let validated_url = self.security_validator.validate_url(&job.url)?;
        
        // Check available disk space before download
        self.check_disk_space(&self.working_dir)?;
        
        // Validate job ID for security (prevent path traversal)
        self.security_validator.validate_input(&job.id, "job_id", 100)?;
        
        // Create output path with secure path construction
        let safe_output_template = self.security_validator.safe_job_file_path(
            &self.working_dir, 
            &job.id, 
            "original.%(ext)s"
        )?;
        
        // Execute download with timeout and file size limits, optimized format selection
        let download_result = timeout(
            self.config.download_timeout,
            Command::new(&self.config.download_command)
                .arg("-o")
                .arg(&safe_output_template)
                .arg("-f")
                .arg("bestvideo[height<=1080][vcodec^=avc1]+bestaudio[acodec^=mp4a]/best[height<=1080]/best")
                .arg("--merge-output-format")
                .arg("mp4")
                .arg("--max-filesize")
                .arg(format!("{}", self.security_validator.get_max_file_size()))
                .arg(validated_url.as_str())
                .output(),
        ).await;
        
        match download_result {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    // Clean up any partial files on download failure
                    if let Some(partial_file) = self.find_downloaded_file(&job.id).await {
                        let _ = tokio::fs::remove_file(&partial_file).await;
                    }
                    let error_message = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(AppError::Download(error_message));
                }
                
                let downloaded_file = self
                    .find_downloaded_file(&job.id).await
                    .ok_or_else(|| AppError::Download("No downloaded file found".to_string()))?;

                // Mark file as active to prevent cleanup races
                // Note: This would require passing CleanupService reference, which we'll add later

                // Validate the downloaded file size
                if let Ok(metadata) = tokio::fs::metadata(&downloaded_file).await {
                    if metadata.len() > self.security_validator.get_max_file_size() {
                        // Remove the oversized file
                        let _ = tokio::fs::remove_file(&downloaded_file).await;
                        return Err(AppError::Download(format!(
                            "Downloaded file exceeds maximum size limit of {} bytes",
                            self.security_validator.get_max_file_size()
                        )));
                    }
                }

                Ok(downloaded_file)
            }
            Ok(Err(error)) => Err(AppError::Download(format!("Download command failed: {error}"))),
            Err(_) => {
                // Clean up any partial files on timeout
                if let Some(partial_file) = self.find_downloaded_file(&job.id).await {
                    let _ = tokio::fs::remove_file(&partial_file).await;
                }
                Err(AppError::Timeout(format!(
                    "Download timed out after {} seconds",
                    self.config.download_timeout.as_secs()
                )))
            }
        }
    }
    

    async fn find_downloaded_file(&self, job_id: &str) -> Option<PathBuf> {
        // Direct path construction is much more efficient than directory scanning
        let common_extensions = ["mp4", "mkv", "avi", "mov", "webm", "m4v"];
        let prefixes = [format!("{job_id}_original"), job_id.to_string()];
        
        // Try direct path construction first (O(1) vs O(n) directory scan)
        for prefix in &prefixes {
            for ext in &common_extensions {
                let candidate = self.working_dir.join(format!("{prefix}.{ext}"));
                if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                    return Some(candidate);
                }
                // Try with underscores too
                let candidate = self.working_dir.join(format!("{prefix}_.{ext}"));
                if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                    return Some(candidate);
                }
            }
        }
        
        // Fallback to async directory scan only if direct construction fails
        // This should be rare if yt-dlp naming is consistent
        if let Ok(mut entries) = tokio::fs::read_dir(&self.working_dir).await {
            let prefix = format!("{job_id}_original");
            
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Ok(metadata) = tokio::fs::metadata(&path).await {
                    if metadata.is_file() {
                        if let Some(filename) = path.file_name() {
                            let filename_str = filename.to_string_lossy();
                            if filename_str.starts_with(&prefix) {
                                let after_prefix = &filename_str[prefix.len()..];
                                if after_prefix.starts_with('.') || after_prefix.starts_with('_') {
                                    return Some(path);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }

    /// Check available disk space before download
    fn check_disk_space(&self, dir: &std::path::Path) -> AppResult<()> {
        match fs2::available_space(dir) {
            Ok(available_bytes) => {
                // Require at least 2x the max file size plus 1GB buffer
                let required_space = (self.security_validator.get_max_file_size() * 2) + (1024 * 1024 * 1024);
                
                if available_bytes < required_space {
                    return Err(AppError::Internal(format!(
                        "Insufficient disk space. Available: {available_bytes} bytes, Required: {required_space} bytes"
                    )));
                }
                
                info!("Disk space check passed. Available: {} GB", available_bytes / (1024 * 1024 * 1024));
                Ok(())
            }
            Err(e) => {
                warn!("Failed to check disk space: {}", e);
                // Don't fail the download if we can't check disk space
                Ok(())
            }
        }
    }
}