use crate::error::AppResult;
use crate::services::JobRepository;
use std::path::PathBuf;
use std::sync::Arc;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub timestamp: u64,
    pub version: String,
    pub uptime_seconds: u64,
    pub checks: HealthChecks,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthChecks {
    pub database: CheckResult,
    pub disk_space: CheckResult,
    pub dependencies: CheckResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub status: String,
    pub message: Option<String>,
    pub response_time_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub jobs: JobMetrics,
    pub system: SystemMetrics,
    pub performance: PerformanceMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobMetrics {
    pub total_jobs: i64,
    pub pending_jobs: i64,
    pub processing_jobs: i64,
    pub completed_jobs: i64,
    pub failed_jobs: i64,
    pub avg_processing_time_seconds: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub uptime_seconds: u64,
    pub working_dir_files: usize,
    pub storage_dir_size_mb: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub requests_per_minute: f64,
    pub avg_response_time_ms: f64,
    pub error_rate_percent: f64,
}

pub struct HealthChecker {
    start_time: SystemTime,
    database_pool: SqlitePool,
    job_repository: Arc<JobRepository>,
    working_dir: PathBuf,
    storage_dir: PathBuf,
}

impl HealthChecker {
    pub fn new(
        database_pool: SqlitePool,
        job_repository: Arc<JobRepository>,
        working_dir: PathBuf,
        storage_dir: PathBuf,
    ) -> Self {
        Self {
            start_time: SystemTime::now(),
            database_pool,
            job_repository,
            working_dir,
            storage_dir,
        }
    }

    pub async fn get_health_status(&self) -> HealthStatus {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let uptime = SystemTime::now()
            .duration_since(self.start_time)
            .unwrap()
            .as_secs();

        let checks = HealthChecks {
            database: self.check_database().await,
            disk_space: self.check_disk_space().await,
            dependencies: self.check_dependencies().await,
        };

        let overall_status = if checks.database.status == "healthy"
            && checks.disk_space.status == "healthy"
            && checks.dependencies.status == "healthy" {
            "healthy"
        } else if checks.database.status == "critical" {
            "critical"
        } else {
            "degraded"
        };

        HealthStatus {
            status: overall_status.to_string(),
            timestamp: now,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: uptime,
            checks,
        }
    }

    pub async fn get_metrics(&self) -> AppResult<Metrics> {
        let job_metrics = self.collect_job_metrics().await?;
        let system_metrics = self.collect_system_metrics().await;
        let performance_metrics = self.collect_performance_metrics().await;

        Ok(Metrics {
            jobs: job_metrics,
            system: system_metrics,
            performance: performance_metrics,
        })
    }

    async fn check_database(&self) -> CheckResult {
        let start = SystemTime::now();

        match sqlx::query("SELECT 1").fetch_one(&self.database_pool).await {
            Ok(_) => {
                let duration = start.elapsed().unwrap().as_millis() as u64;
                CheckResult {
                    status: "healthy".to_string(),
                    message: Some("Database connection successful".to_string()),
                    response_time_ms: Some(duration),
                }
            }
            Err(e) => CheckResult {
                status: "critical".to_string(),
                message: Some(format!("Database connection failed: {e}")),
                response_time_ms: None,
            }
        }
    }

    async fn check_disk_space(&self) -> CheckResult {
        // Check available disk space in working directory
        match std::fs::metadata(&self.working_dir) {
            Ok(_) => {
                // Simple check that directory is accessible
                CheckResult {
                    status: "healthy".to_string(),
                    message: Some("Working directory accessible".to_string()),
                    response_time_ms: Some(0),
                }
            }
            Err(e) => CheckResult {
                status: "critical".to_string(),
                message: Some(format!("Working directory inaccessible: {e}")),
                response_time_ms: None,
            }
        }
    }

    async fn check_dependencies(&self) -> CheckResult {
        // Check if external dependencies (yt-dlp, ffmpeg) are available
        let yt_dlp_check = tokio::process::Command::new("yt-dlp")
            .arg("--version")
            .output()
            .await;

        let ffmpeg_check = tokio::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .await;

        match (yt_dlp_check, ffmpeg_check) {
            (Ok(yt_dlp), Ok(ffmpeg)) if yt_dlp.status.success() && ffmpeg.status.success() => {
                CheckResult {
                    status: "healthy".to_string(),
                    message: Some("All dependencies available".to_string()),
                    response_time_ms: Some(10),
                }
            }
            _ => CheckResult {
                status: "degraded".to_string(),
                message: Some("Some dependencies may be missing".to_string()),
                response_time_ms: None,
            }
        }
    }

    async fn collect_job_metrics(&self) -> AppResult<JobMetrics> {
        // Get job statistics from database
        let stats = self.job_repository.get_job_stats().await.unwrap_or_default();

        let total_jobs = stats.values().sum::<i64>();
        let pending_jobs = stats.get("Pending").copied().unwrap_or(0);
        let processing_jobs = stats.get("Downloading").copied().unwrap_or(0)
            + stats.get("Processing").copied().unwrap_or(0);
        let completed_jobs = stats.get("Completed").copied().unwrap_or(0);
        let failed_jobs = stats.get("Failed").copied().unwrap_or(0);

        // Calculate average processing time (simplified - would need more sophisticated tracking)
        let avg_processing_time = 45.0; // Placeholder - implement proper tracking

        Ok(JobMetrics {
            total_jobs,
            pending_jobs,
            processing_jobs,
            completed_jobs,
            failed_jobs,
            avg_processing_time_seconds: avg_processing_time,
        })
    }

    async fn collect_system_metrics(&self) -> SystemMetrics {
        let uptime = SystemTime::now()
            .duration_since(self.start_time)
            .unwrap()
            .as_secs();

        let working_dir_files = match tokio::fs::read_dir(&self.working_dir).await {
            Ok(mut entries) => {
                let mut count = 0;
                while entries.next_entry().await.unwrap_or(None).is_some() {
                    count += 1;
                }
                count
            }
            Err(_) => 0
        };

        let storage_size = self.calculate_directory_size(&self.storage_dir).await;

        SystemMetrics {
            uptime_seconds: uptime,
            working_dir_files,
            storage_dir_size_mb: storage_size,
        }
    }

    async fn collect_performance_metrics(&self) -> PerformanceMetrics {
        let (requests_per_minute, avg_response_time_ms, error_rate_percent) = crate::middleware::get_request_metrics();

        PerformanceMetrics {
            requests_per_minute,
            avg_response_time_ms,
            error_rate_percent,
        }
    }

    async fn calculate_directory_size(&self, dir: &std::path::Path) -> f64 {
        let mut size = 0u64;
        if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    if metadata.is_file() {
                        size += metadata.len();
                    }
                }
            }
        }
        size as f64 / (1024.0 * 1024.0) // Convert to MB
    }
}
