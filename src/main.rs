mod api;
mod services;
mod config;
mod error;
mod models;
mod database;
mod middleware;
mod monitoring;

use crate::api::routes::{configure_routes, AppState};
use crate::api::monitoring::{configure_monitoring_routes, MonitoringState};
use crate::config::load_config;
use crate::services::{ProcessService, DownloadService, JobRepository, CleanupService, SecurityValidator, ConnectionPoolManager, JobQueue, RetentionService};
use crate::database::{create_database_pool, run_migrations};
use crate::middleware::{SecurityHeaders, Cors, RequestTracking, AuthMiddleware};
use crate::monitoring::HealthChecker;
use actix_web::{web, App, HttpServer};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_actix_web::TracingLogger;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize structured logging
    init_logging();

    info!("Starting Aperio Video Processing API v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = load_config();
    let server_config = config.server.clone();
    
    // Create working directory
    let working_dir_path = std::env::var("APERIO_WORKING_DIR").unwrap_or_else(|_| "/app/working".to_string());
    let working_dir = PathBuf::from(&working_dir_path);
    tokio::fs::create_dir_all(&working_dir).await.expect("Failed to create working directory");
    info!("Working directory initialized: {}", working_dir_path);

    // Create storage directory
    let storage_dir_path = std::env::var("APERIO_STORAGE_PATH").unwrap_or_else(|_| "/app/storage".to_string());
    let storage_dir = PathBuf::from(&storage_dir_path);
    tokio::fs::create_dir_all(&storage_dir).await.expect("Failed to create storage directory");
    info!("Storage directory initialized: {}", storage_dir_path);

    // Initialize database
    let database_url = std::env::var("APERIO_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:///app/storage/aperio.db".to_string());

    info!("Connecting to database: {}", database_url);
    let pool = create_database_pool(&database_url)
        .await
        .expect("Failed to create database pool");

    // Run database migrations
    info!("Running database migrations");
    run_migrations(&pool)
        .await
        .expect("Failed to run database migrations");

    // Initialize connection pool manager
    let pool_manager = Arc::new(ConnectionPoolManager::new(
        config.download.max_concurrent_downloads,
        config.processing.max_concurrent_processing,
    ));

    // Initialize services
    info!("Initializing services");
    let download_service = DownloadService::new(config.download.clone(), working_dir.clone(), &config.security, pool_manager.clone());
    let process_service = ProcessService::new(config.processing.clone(), working_dir.clone(), pool_manager.clone());
    let cleanup_service = Arc::new(CleanupService::new(working_dir.clone()));
    let job_repository = Arc::new(JobRepository::new(pool.clone()));
    let security_validator = SecurityValidator::new(
        config.download.allowed_domains.clone(),
        config.security.max_file_size_mb as u32,
        config.security.max_url_length as u32,
    );

    // Initialize job queue (simplified - no TaskManager overhead)
    let job_queue = Arc::new(JobQueue::new(config.queue.max_concurrent_jobs));

    // Initialize monitoring
    let health_checker = HealthChecker::new(
        pool.clone(),
        working_dir.clone(),
    );

    let app_state = Arc::new(AppState {
        download_service,
        process_service,
        cleanup_service: (*cleanup_service).clone(),
        job_repository: (*job_repository).clone(),
        security_validator,
        job_queue: job_queue.clone(),
    });

    // Restore pending jobs from database to queue on startup with race condition protection
    info!("Restoring pending jobs from database to queue");
    match job_repository.get_pending_jobs().await {
        Ok(pending_jobs) => {
            info!("Found {} pending jobs to restore", pending_jobs.len());
            for job in pending_jobs {
                // Atomic check: only restore if still pending and not being processed
                let job_id = job.id.clone();
                match job_repository.try_claim_pending_job(&job_id).await {
                    Ok(true) => {
                        info!("Successfully claimed and restoring job {} to queue", job_id);
                        if let Err(e) = job_queue.enqueue(job, crate::services::job_queue::JobPriority::Normal).await {
                            warn!("Failed to restore job to queue: {}", e);
                            // Unclaim the job if queueing failed
                            if let Err(unclaim_err) = job_repository.unclaim_job(&job_id).await {
                                warn!("Failed to unclaim job {} after queue failure: {}", job_id, unclaim_err);
                            }
                        }
                    }
                    Ok(false) => {
                        info!("Job {} already claimed or no longer pending, skipping restoration", job_id);
                    }
                    Err(e) => {
                        warn!("Failed to claim job {} for restoration: {}", job_id, e);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to restore pending jobs from database: {}", e);
        }
    }

    // Start job queue worker
    job_queue.start_worker(app_state.clone()).await;

    // Start retention service if enabled
    if config.retention.enabled {
        info!("Starting retention service with {} day retention", config.retention.retention_days);
        let retention_service = RetentionService::new(
            job_repository.clone(),
            cleanup_service.clone(),
            config.retention.retention_days,
            config.retention.cleanup_interval_hours,
        );
        
        let retention_service_clone = retention_service.clone();
        tokio::spawn(async move {
            retention_service_clone.start_background_cleanup().await;
        });
    } else {
        info!("Retention service disabled");
    }

    let monitoring_state = Arc::new(MonitoringState {
        health_checker,
    });

    // Configure CORS
    let cors_config = std::env::var("APERIO_CORS_ORIGINS")
        .map(|origins| Cors::new(origins.split(',').map(|s| s.trim().to_string()).collect()))
        .unwrap_or_else(|_| Cors::restrictive());

    info!("Starting Aperio server on {}:{}", server_config.host, server_config.port);
    info!("Security: File size limit: {}MB, URL length limit: {} chars",
           config.security.max_file_size_mb, config.security.max_url_length);

    // Start HTTP server with monitoring and security middleware
    HttpServer::new(move || {
        App::new()
            .wrap(RequestTracking) // Add request correlation IDs and performance tracking
            .wrap(TracingLogger::default()) // Add request tracing
            .wrap(SecurityHeaders) // Add security headers to all responses
            .wrap(cors_config.clone()) // Add CORS support
            .wrap(AuthMiddleware::new(config.clone())) // Add authentication middleware
            .app_data(web::Data::new(app_state.clone()))
            .app_data(web::Data::new(monitoring_state.clone()))
            .app_data(web::PayloadConfig::new(server_config.max_payload_size))
            .app_data(web::JsonConfig::default().limit(4096))
            .configure(configure_routes)
            .configure(configure_monitoring_routes)
    })
        .client_request_timeout(server_config.client_timeout)
        .keep_alive(server_config.keep_alive)
        .bind((server_config.host, server_config.port))?
        .run()
        .await
}

fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "aperio=info,actix_web=info".to_string());
    let log_format = std::env::var("APERIO_LOG_FORMAT").unwrap_or_else(|_| "json".to_string());

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&log_level));

    if log_format == "json" {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty())
            .init();
    }
}
