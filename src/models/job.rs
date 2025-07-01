use std::path::PathBuf;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum JobStatus {
    Pending,
    Claimed,
    Downloading,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "Pending"),
            JobStatus::Claimed => write!(f, "Claimed"),
            JobStatus::Downloading => write!(f, "Downloading"),
            JobStatus::Processing => write!(f, "Processing"),
            JobStatus::Completed => write!(f, "Completed"),
            JobStatus::Failed => write!(f, "Failed"),
            JobStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: String,
    pub url: String,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub downloaded_path: Option<String>,
    pub processed_path: Option<String>,
    pub error_message: Option<String>,
    pub processing_time_seconds: Option<i64>,
}

impl Job {
    pub fn new(url: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            url,
            status: JobStatus::Pending,
            created_at: now,
            updated_at: now,
            downloaded_path: None,
            processed_path: None,
            error_message: None,
            processing_time_seconds: None,
        }
    }
    
    pub fn update_status(&mut self, status: JobStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }
    
    pub fn set_error(&mut self, error: String) {
        self.status = JobStatus::Failed;
        self.error_message = Some(error);
        self.updated_at = Utc::now();
    }

    #[allow(dead_code)]
    pub fn mark_completed(&mut self, output_path: String, processing_time: i64) {
        self.status = JobStatus::Completed;
        self.processed_path = Some(output_path);
        self.processing_time_seconds = Some(processing_time);
        self.updated_at = Utc::now();
    }
    
    pub fn set_downloaded_path(&mut self, path: PathBuf) {
        self.downloaded_path = Some(path.to_string_lossy().to_string());
        self.updated_at = Utc::now();
    }
    
    pub fn set_processed_path(&mut self, path: PathBuf) {
        self.processed_path = Some(path.to_string_lossy().to_string());
        self.updated_at = Utc::now();
    }

    pub fn set_processing_time(&mut self, duration: Duration) {
        self.processing_time_seconds = Some(duration.as_secs() as i64);
        self.updated_at = Utc::now();
    }
    
    // Helper methods for PathBuf conversion
    pub fn get_downloaded_path(&self) -> Option<PathBuf> {
        self.downloaded_path.as_ref().map(PathBuf::from)
    }

    pub fn get_processed_path(&self) -> Option<PathBuf> {
        self.processed_path.as_ref().map(PathBuf::from)
    }

    pub fn get_processing_time(&self) -> Option<Duration> {
        self.processing_time_seconds.map(|s| Duration::from_secs(s as u64))
    }
}