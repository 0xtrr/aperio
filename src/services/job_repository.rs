use crate::error::{AppError, AppResult};
use crate::models::job::{Job, JobStatus};
use sqlx::{SqlitePool, Row};
use std::collections::HashMap;

#[derive(Clone)]
pub struct JobRepository {
    pool: SqlitePool,
}

impl JobRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_job(&self, job: &Job) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO jobs (id, url, status, created_at, updated_at, downloaded_path, processed_path, error_message, processing_time_seconds)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&job.id)
        .bind(&job.url)
        .bind(job.status.to_string())
        .bind(job.created_at)
        .bind(job.updated_at)
        .bind(&job.downloaded_path)
        .bind(&job.processed_path)
        .bind(&job.error_message)
        .bind(job.processing_time_seconds)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create job: {e}")))?;

        Ok(())
    }

    pub async fn get_job(&self, job_id: &str) -> AppResult<Option<Job>> {
        let row = sqlx::query(
            r#"
            SELECT id, url, status, created_at, updated_at,
                   downloaded_path, processed_path, error_message, processing_time_seconds
            FROM jobs
            WHERE id = ?
            "#
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to get job: {e}")))?;

        if let Some(row) = row {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                _ => JobStatus::Failed,
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    pub async fn update_job(&self, job: &Job) -> AppResult<()> {
        // Use transaction for atomic update
        let mut tx = self.pool.begin().await
            .map_err(|e| AppError::Internal(format!("Failed to start transaction: {e}")))?;

        let updated_at = chrono::Utc::now();
        
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, updated_at = ?, downloaded_path = ?, processed_path = ?,
                error_message = ?, processing_time_seconds = ?
            WHERE id = ?
            "#
        )
        .bind(job.status.to_string())
        .bind(updated_at)
        .bind(&job.downloaded_path)
        .bind(&job.processed_path)
        .bind(&job.error_message)
        .bind(job.processing_time_seconds)
        .bind(&job.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to update job: {e}")))?;

        if result.rows_affected() == 0 {
            tx.rollback().await
                .map_err(|e| AppError::Internal(format!("Failed to rollback transaction: {e}")))?;
            return Err(AppError::NotFound(format!("Job not found: {}", job.id)));
        }

        tx.commit().await
            .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {e}")))?;

        Ok(())
    }

    /// Atomically update job status with validation
    #[allow(dead_code)]
    pub async fn update_job_status(&self, job_id: &str, new_status: JobStatus, from_status: Option<JobStatus>) -> AppResult<bool> {
        let mut tx = self.pool.begin().await
            .map_err(|e| AppError::Internal(format!("Failed to start transaction: {e}")))?;

        let updated_at = chrono::Utc::now();
        
        let result = if let Some(expected_status) = from_status {
            // Conditional update: only update if current status matches expected
            sqlx::query(
                "UPDATE jobs SET status = ?, updated_at = ? WHERE id = ? AND status = ?"
            )
            .bind(new_status.to_string())
            .bind(updated_at)
            .bind(job_id)
            .bind(expected_status.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to update job status: {e}")))?
        } else {
            // Unconditional update
            sqlx::query(
                "UPDATE jobs SET status = ?, updated_at = ? WHERE id = ?"
            )
            .bind(new_status.to_string())
            .bind(updated_at)
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to update job status: {e}")))?
        };

        let success = result.rows_affected() > 0;
        
        if success {
            tx.commit().await
                .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {e}")))?;
        } else {
            tx.rollback().await
                .map_err(|e| AppError::Internal(format!("Failed to rollback transaction: {e}")))?;
        }

        Ok(success)
    }

    pub async fn get_job_stats(&self) -> AppResult<HashMap<String, i64>> {
        let rows = sqlx::query(
            r#"
            SELECT status, COUNT(*) as count
            FROM jobs
            GROUP BY status
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to get job stats: {e}")))?;

        let mut stats = HashMap::new();
        for row in rows {
            let status: String = row.get("status");
            let count: i64 = row.get("count");
            stats.insert(status, count);
        }

        Ok(stats)
    }

    #[allow(dead_code)]
    pub async fn list_jobs_by_status(&self, status: JobStatus) -> AppResult<Vec<Job>> {
        let rows = sqlx::query(
            r#"
            SELECT id, url, status, created_at, updated_at,
                   downloaded_path, processed_path, error_message, processing_time_seconds
            FROM jobs
            WHERE status = ?
            ORDER BY created_at DESC
            "#
        )
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list jobs: {e}")))?;

        let mut jobs = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let job_status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                _ => JobStatus::Failed,
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status: job_status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };
            jobs.push(job);
        }

        Ok(jobs)
    }

    #[allow(dead_code)]
    pub async fn delete_job(&self, job_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM jobs WHERE id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete job: {e}")))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn list_all_jobs(&self) -> AppResult<Vec<Job>> {
        let rows = sqlx::query(
            r#"
            SELECT id, url, status, created_at, updated_at,
                   downloaded_path, processed_path, error_message, processing_time_seconds
            FROM jobs
            ORDER BY created_at DESC
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list all jobs: {e}")))?;

        let mut jobs = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let job_status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                _ => JobStatus::Failed,
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status: job_status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };
            jobs.push(job);
        }

        Ok(jobs)
    }

    pub async fn list_jobs_paginated(
        &self, 
        page: u32, 
        page_size: u32, 
        status_filter: Option<JobStatus>
    ) -> AppResult<(Vec<Job>, u32)> {
        let offset = page * page_size;
        
        // Build query based on whether we have a status filter
        let (query, count_query) = if let Some(ref _status) = status_filter {
            (
                r#"
                SELECT id, url, status, created_at, updated_at,
                       downloaded_path, processed_path, error_message, processing_time_seconds
                FROM jobs
                WHERE status = ?
                ORDER BY created_at DESC
                LIMIT ? OFFSET ?
                "#,
                r#"
                SELECT COUNT(*) as total
                FROM jobs
                WHERE status = ?
                "#
            )
        } else {
            (
                r#"
                SELECT id, url, status, created_at, updated_at,
                       downloaded_path, processed_path, error_message, processing_time_seconds
                FROM jobs
                ORDER BY created_at DESC
                LIMIT ? OFFSET ?
                "#,
                r#"
                SELECT COUNT(*) as total
                FROM jobs
                "#
            )
        };

        // Get total count
        let total_count: i64 = if let Some(status) = &status_filter {
            sqlx::query(count_query)
                .bind(status.to_string())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to count jobs: {e}")))?
                .get("total")
        } else {
            sqlx::query(count_query)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to count jobs: {e}")))?
                .get("total")
        };

        // Get jobs
        let rows = if let Some(status) = status_filter {
            sqlx::query(query)
                .bind(status.to_string())
                .bind(page_size as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to list jobs: {e}")))?
        } else {
            sqlx::query(query)
                .bind(page_size as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to list jobs: {e}")))?
        };

        let mut jobs = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let job_status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                _ => JobStatus::Failed,
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status: job_status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };
            jobs.push(job);
        }

        let total_pages = ((total_count as f64) / (page_size as f64)).ceil() as u32;
        Ok((jobs, total_pages))
    }

    /// Get all pending jobs for queue restoration on startup
    pub async fn get_pending_jobs(&self) -> AppResult<Vec<Job>> {
        let rows = sqlx::query("SELECT * FROM jobs WHERE status = 'Pending' ORDER BY created_at ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get pending jobs: {e}")))?;

        let mut jobs = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                _ => return Err(AppError::Internal(format!("Unknown job status: {status_str}"))),
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };
            jobs.push(job);
        }

        Ok(jobs)
    }

    /// Atomically claim a pending job for processing (prevents race conditions)
    pub async fn try_claim_pending_job(&self, job_id: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE jobs SET status = 'Claimed' WHERE id = ? AND status = 'Pending'"
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to claim job: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    /// Unclaim a job (set back to pending) if processing failed to start
    pub async fn unclaim_job(&self, job_id: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE jobs SET status = 'Pending' WHERE id = ? AND status = 'Claimed'"
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to unclaim job: {e}")))?;

        Ok(())
    }

    /// Get job with row-level locking for atomic updates
    #[allow(dead_code)]
    pub async fn get_job_for_update(&self, job_id: &str) -> AppResult<Option<Job>> {
        // SQLite doesn't have SELECT FOR UPDATE, so we use a transaction
        let mut tx = self.pool.begin().await
            .map_err(|e| AppError::Internal(format!("Failed to start transaction: {e}")))?;

        let row = sqlx::query(
            "SELECT id, url, status, created_at, updated_at, downloaded_path, processed_path, error_message, processing_time_seconds 
             FROM jobs WHERE id = ?"
        )
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to get job: {e}")))?;

        if let Some(row) = row {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                "Claimed" => JobStatus::Pending, // Treat claimed as pending for now
                _ => return Err(AppError::Internal(format!("Unknown job status: {status_str}"))),
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };

            tx.commit().await
                .map_err(|e| AppError::Internal(format!("Failed to commit transaction: {e}")))?;

            Ok(Some(job))
        } else {
            tx.rollback().await
                .map_err(|e| AppError::Internal(format!("Failed to rollback transaction: {e}")))?;
            Ok(None)
        }
    }

    /// Find an active job (pending, downloading, processing) by URL for deduplication
    pub async fn find_active_job_by_url(&self, url: &str) -> AppResult<Option<Job>> {
        let row = sqlx::query(
            "SELECT id, url, status, created_at, updated_at, downloaded_path, processed_path, error_message, processing_time_seconds 
             FROM jobs WHERE url = ? AND status IN ('Pending', 'Downloading', 'Processing', 'Claimed') 
             ORDER BY created_at DESC LIMIT 1"
        )
        .bind(url)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to find job by URL: {e}")))?;

        if let Some(row) = row {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "Pending" => JobStatus::Pending,
                "Downloading" => JobStatus::Downloading,
                "Processing" => JobStatus::Processing,
                "Completed" => JobStatus::Completed,
                "Failed" => JobStatus::Failed,
                "Cancelled" => JobStatus::Cancelled,
                "Claimed" => JobStatus::Pending, // Treat claimed as pending
                _ => return Err(AppError::Internal(format!("Unknown job status: {status_str}"))),
            };

            let job = Job {
                id: row.get("id"),
                url: row.get("url"),
                status,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                downloaded_path: row.get("downloaded_path"),
                processed_path: row.get("processed_path"),
                error_message: row.get("error_message"),
                processing_time_seconds: row.get("processing_time_seconds"),
            };

            Ok(Some(job))
        } else {
            Ok(None)
        }
    }
}
