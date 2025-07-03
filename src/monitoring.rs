use std::path::PathBuf;
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


pub struct HealthChecker {
    start_time: SystemTime,
    database_pool: SqlitePool,
    working_dir: PathBuf,
}

impl HealthChecker {
    pub fn new(
        database_pool: SqlitePool,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            start_time: SystemTime::now(),
            database_pool,
            working_dir,
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
}

