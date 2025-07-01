use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashSet;

#[derive(Clone)]
pub struct CleanupService {
    working_dir: PathBuf,
    // Track files currently being processed to prevent cleanup races
    active_files: Arc<Mutex<HashSet<String>>>,
}

impl CleanupService {
    pub fn new(working_dir: PathBuf) -> Self {
        Self { 
            working_dir,
            active_files: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Mark a file as actively being processed
    pub async fn mark_file_active(&self, file_path: &str) -> AppResult<()> {
        let mut active_files = self.active_files.lock().await;
        active_files.insert(file_path.to_string());
        Ok(())
    }

    /// Unmark a file as being processed
    pub async fn unmark_file_active(&self, file_path: &str) -> AppResult<()> {
        let mut active_files = self.active_files.lock().await;
        active_files.remove(file_path);
        Ok(())
    }

    /// Check if a file is currently being processed
    async fn is_file_active(&self, file_path: &str) -> bool {
        let active_files = self.active_files.lock().await;
        active_files.contains(file_path)
    }

    /// Clean up files associated with a job with race condition protection
    pub async fn cleanup_job_files(&self, job_id: &str) -> AppResult<()> {
        let mut cleaned_files = Vec::new();
        let mut errors = Vec::new();
        let mut skipped_files = Vec::new();

        // Clean up any files that start with the job ID
        if let Ok(mut entries) = fs::read_dir(&self.working_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename) = path.file_name() {
                        let filename_str = filename.to_string_lossy();
                        if filename_str.starts_with(job_id) {
                            let path_str = path.to_string_lossy().to_string();
                            
                            // Check if file is currently being processed
                            if self.is_file_active(&path_str).await {
                                skipped_files.push(path_str);
                                warn!("Skipping cleanup of active file: {}", path.display());
                                continue;
                            }

                            // Try to acquire a cleanup lock by temporarily marking the file
                            self.mark_file_active(&path_str).await?;
                            
                            // Double-check the file still exists (prevent TOCTOU)
                            if !path.exists() {
                                self.unmark_file_active(&path_str).await?;
                                continue;
                            }

                            match fs::remove_file(&path).await {
                                Ok(_) => {
                                    cleaned_files.push(path.clone());
                                    info!("Cleaned up file: {}", path.display());
                                }
                                Err(e) => {
                                    errors.push(format!("Failed to remove {}: {}", path.display(), e));
                                }
                            }
                            
                            // Always unmark the file after cleanup attempt
                            self.unmark_file_active(&path_str).await?;
                        }
                    }
                }
            }
        }

        if !skipped_files.is_empty() {
            info!("Skipped {} active files during cleanup for job {}", skipped_files.len(), job_id);
        }

        if !errors.is_empty() {
            return Err(AppError::Internal(format!(
                "Cleanup completed with errors: {}",
                errors.join(", ")
            )));
        }

        info!("Successfully cleaned up {} files for job {}", cleaned_files.len(), job_id);
        Ok(())
    }

    /// Clean up a specific file path
    pub async fn cleanup_file(&self, file_path: &PathBuf) -> AppResult<()> {
        if file_path.exists() {
            match fs::remove_file(file_path).await {
                Ok(_) => {
                    info!("Cleaned up file: {}", file_path.display());
                    Ok(())
                }
                Err(e) => Err(AppError::Internal(format!(
                    "Failed to remove file {}: {}",
                    file_path.display(),
                    e
                )))
            }
        } else {
            Ok(()) // File doesn't exist, nothing to clean
        }
    }

    /// Clean up old temporary files (older than specified hours)
    #[allow(dead_code)]
    pub async fn cleanup_old_files(&self, hours_old: u64) -> AppResult<()> {
        let cutoff_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - (hours_old * 3600);

        let mut cleaned_count = 0;
        let mut errors = Vec::new();

        if let Ok(mut entries) = fs::read_dir(&self.working_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(metadata) = fs::metadata(&path).await {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(modified_secs) = modified.duration_since(std::time::UNIX_EPOCH) {
                                if modified_secs.as_secs() < cutoff_time {
                                    match fs::remove_file(&path).await {
                                        Ok(_) => {
                                            cleaned_count += 1;
                                            info!("Cleaned up old file: {}", path.display());
                                        }
                                        Err(e) => {
                                            errors.push(format!("Failed to remove {}: {}", path.display(), e));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if !errors.is_empty() {
            return Err(AppError::Internal(format!(
                "Old file cleanup completed with errors: {}",
                errors.join(", ")
            )));
        }

        info!("Cleaned up {} old files", cleaned_count);
        Ok(())
    }
}