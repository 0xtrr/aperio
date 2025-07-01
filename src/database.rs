use crate::error::{AppError, AppResult};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use std::path::Path;
use std::os::unix::fs::PermissionsExt;

pub async fn create_database_pool(database_url: &str) -> AppResult<SqlitePool> {
    let db_path = database_url.trim_start_matches("sqlite://");
    tracing::info!("Database file path: {}", db_path);
    
    // Ensure the database directory exists
    if let Some(parent) = Path::new(db_path).parent() {
        tracing::info!("Creating database directory: {:?}", parent);
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("Failed to create database directory: {e}")))?;
        
        // Check directory permissions
        let metadata = std::fs::metadata(parent)
            .map_err(|e| AppError::Internal(format!("Failed to read directory metadata: {e}")))?;
        tracing::info!("Directory permissions: {:o}", metadata.permissions().mode() & 0o777);
        
        // Try to create a test file
        let test_file = parent.join("test_write");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                std::fs::remove_file(&test_file).ok();
                tracing::info!("Directory is writable");
            }
            Err(e) => {
                tracing::error!("Directory is not writable: {}", e);
                return Err(AppError::Internal(format!("Directory not writable: {e}")));
            }
        }
    }

    // Use connection options that create the database if it doesn't exist
    let connection_url = if database_url.contains('?') {
        format!("{database_url}&create-if-missing=true")
    } else {
        format!("{database_url}?mode=rwc")
    };
    
    tracing::info!("Connecting with URL: {}", connection_url);
    
    // Configure connection pool based on environment
    let max_connections = std::env::var("APERIO_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            // Default to 4x CPU cores, min 10, max 100
            let cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            (cpus * 4).clamp(10, 100)
        });
    
    tracing::info!("Configuring database pool with {} max connections", max_connections);
    
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections as u32)
        .connect(&connection_url)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create database pool: {e}")))?;

    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> AppResult<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run migrations: {e}")))?;

    Ok(())
}
