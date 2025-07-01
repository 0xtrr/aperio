use std::fs;
use std::path::{Path, PathBuf};
use crate::config::{StorageConfig, StorageType};
use crate::error::{AppError, AppResult};
use crate::models::job::Job;

pub struct StorageService {
    config: StorageConfig,
}

impl StorageService {
    pub fn new(config: StorageConfig) -> AppResult<Self> {
        match &config.storage_type {
            StorageType::Local => {
                if let Some(local_path) = &config.local_path {
                    fs::create_dir_all(local_path).map_err(|e| {
                        AppError::Storage(format!("Failed to create storage directory: {}", e))
                    })?;
                } else {
                    return Err(AppError::Storage(
                        "Local storage path not configured".to_string(),
                    ));
                }
            }
        }
        Ok(Self { config })
    }

    pub async fn store(&self, job: &Job, source_path: &Path) -> AppResult<PathBuf> {
        match &self.config.storage_type {
            StorageType::Local => self.store_local(job, source_path).await,
        }
    }

    pub async fn get(&self, job_id: &str) -> AppResult<Option<PathBuf>> {
        match &self.config.storage_type {
            StorageType::Local => self.get_local(job_id), 
        }
    }
    
    pub async fn read(&self, path: &Path) -> AppResult<Vec<u8>> {
        tokio::fs::read(path)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to read file: {}", e)))
    }
    
    async fn store_local(&self, job: &Job, source_path: &Path) -> AppResult<PathBuf> {
        let local_path = self.config.local_path.as_ref().unwrap();
        let job_dir = Path::new(local_path).join(&job.id);

        // Create job directory
        tokio::fs::create_dir_all(&job_dir)
            .await
            .map_err(|e| { AppError::Storage(format!("Failed to create job directory: {}", e)) })?;

        // Get filename from source path
        let filename = source_path
            .file_name()
            .ok_or_else(|| AppError::Storage("Invalid source filename".to_string()))?;

        // Create destination path
        let dest_path = job_dir.join(filename);

        // Copy file
        tokio::fs::copy(source_path, &dest_path)
            .await
            .map_err(|e| AppError::Storage(format!("Failed to copy file: {}", e)))?;

        Ok(dest_path)
    }

    fn get_local(&self, job_id: &str) -> AppResult<Option<PathBuf>> {
        let local_path = self.config.local_path.as_ref().unwrap();
        let job_dir = Path::new(local_path).join(job_id);

        if !job_dir.exists() {
            return Ok(None);
        }

        // Find processed file
        if let Ok(entries) = fs::read_dir(&job_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename) = path.file_name() {
                        let filename_str = filename.to_string_lossy();
                        if filename_str.contains("_processed") {
                            return Ok(Some(path));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}