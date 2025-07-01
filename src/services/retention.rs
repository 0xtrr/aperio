use crate::error::AppResult;
use crate::services::{JobRepository, CleanupService};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, warn, error};

#[derive(Clone)]
pub struct RetentionService {
    job_repository: Arc<JobRepository>,
    cleanup_service: Arc<CleanupService>,
    retention_days: u32,
    cleanup_interval_hours: u64,
}

impl RetentionService {
    pub fn new(
        job_repository: Arc<JobRepository>,
        cleanup_service: Arc<CleanupService>,
        retention_days: u32,
        cleanup_interval_hours: u64,
    ) -> Self {
        Self {
            job_repository,
            cleanup_service,
            retention_days,
            cleanup_interval_hours,
        }
    }

    /// Start the background retention cleanup task
    pub async fn start_background_cleanup(&self) {
        let mut interval = interval(Duration::from_secs(self.cleanup_interval_hours * 3600));
        
        info!(
            "Starting retention cleanup service: {} day retention, {} hour intervals",
            self.retention_days, self.cleanup_interval_hours
        );

        // Initial delay to avoid startup conflicts
        sleep(Duration::from_secs(60)).await;

        loop {
            interval.tick().await;
            
            if let Err(e) = self.run_cleanup().await {
                error!("Retention cleanup failed: {}", e);
            }
        }
    }

    /// Run a single cleanup cycle
    pub async fn run_cleanup(&self) -> AppResult<()> {
        info!("Starting retention cleanup cycle");

        // Get statistics before cleanup
        let (completed_before, failed_before, cancelled_before) = self.job_repository.get_cleanup_stats().await?;
        info!(
            "Jobs before cleanup - Completed: {}, Failed: {}, Cancelled: {}",
            completed_before, failed_before, cancelled_before
        );

        // Get old job IDs and delete from database
        let old_job_ids = self.job_repository.cleanup_old_jobs(self.retention_days).await?;
        
        if old_job_ids.is_empty() {
            info!("No old jobs found for cleanup");
            return Ok(());
        }

        info!("Found {} old jobs to clean up", old_job_ids.len());

        // Clean up associated files
        let mut file_cleanup_errors = Vec::new();
        let mut successful_file_cleanups = 0;

        for job_id in &old_job_ids {
            match self.cleanup_service.cleanup_job_files(job_id).await {
                Ok(_) => {
                    successful_file_cleanups += 1;
                }
                Err(e) => {
                    warn!("Failed to clean up files for job {}: {}", job_id, e);
                    file_cleanup_errors.push(format!("Job {job_id}: {e}"));
                }
            }
        }

        // Get statistics after cleanup
        let (completed_after, failed_after, cancelled_after) = self.job_repository.get_cleanup_stats().await?;

        info!(
            "Retention cleanup completed - Removed {} database records, cleaned {} file sets",
            old_job_ids.len(), successful_file_cleanups
        );
        info!(
            "Jobs after cleanup - Completed: {}, Failed: {}, Cancelled: {}",
            completed_after, failed_after, cancelled_after
        );

        if !file_cleanup_errors.is_empty() {
            warn!(
                "File cleanup had {} errors: {}",
                file_cleanup_errors.len(),
                file_cleanup_errors.join("; ")
            );
        }

        Ok(())
    }

    /// Run immediate cleanup (for manual triggering)
    pub async fn cleanup_now(&self) -> AppResult<()> {
        info!("Running immediate retention cleanup");
        self.run_cleanup().await
    }
}