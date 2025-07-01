use crate::error::{AppError, AppResult};
use crate::models::job::{Job, JobStatus};
use crate::services::process::ProcessService;
use crate::services::{DownloadService, JobRepository, CleanupService, SecurityValidator, JobQueue, JobPriority};
use crate::services::retry::{retry_with_backoff, RetryConfig, is_retryable_error};
use actix_web::{get, post, delete, web, Responder};
use actix_web::http::header::{ContentDisposition, DispositionType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::path::Path;
use tracing::{info, warn, error, debug, instrument};

pub struct AppState {
    pub download_service: DownloadService,
    pub process_service: ProcessService,
    pub cleanup_service: CleanupService,
    pub job_repository: JobRepository,
    pub security_validator: SecurityValidator,
    pub job_queue: Arc<JobQueue>,
}

#[derive(Deserialize, Debug)]
pub struct DownloadRequest {
    pub url: String,
    pub priority: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct JobResponse {
    pub id: String,
    pub status: JobStatus,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
    pub error_message: Option<String>,
    pub processing_time: Option<String>,
}

impl From<&Job> for JobResponse {
    fn from(job: &Job) -> Self {
        let created_at = job.created_at.to_rfc3339();
        let updated_at = job.updated_at.to_rfc3339();
        let processing_time = job.get_processing_time().map(|d| format!("{d:?}"));

        Self {
            id: job.id.clone(),
            status: job.status.clone(),
            url: job.url.clone(),
            created_at,
            updated_at,
            error_message: job.error_message.clone(),
            processing_time,
        }
    }
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(start_job)
        .service(get_job_status)
        .service(get_processed_video)
        .service(stream_processed_video)
        .service(cancel_job)
        .service(list_jobs);
}

#[post("/process")]
#[instrument(skip(data), fields(url = %request.url))]
async fn start_job(
    data: web::Data<Arc<AppState>>,
    request: web::Json<DownloadRequest>,
) -> AppResult<impl Responder> {
    info!("Starting new job for URL: {}", request.url);
    
    // Enhanced input validation
    data.security_validator.validate_input(&request.url, "url", 2048)?;
    
    // Pre-validate URL before creating job
    let _validated_url = data.security_validator.validate_url(&request.url)?;
    
    // Check for existing pending/active jobs with the same URL
    match data.job_repository.find_active_job_by_url(&request.url).await? {
        Some(existing_job) => {
            info!("Found existing job {} for URL, returning existing job instead of creating duplicate", existing_job.id);
            return Ok(web::Json(JobResponse::from(&existing_job)));
        }
        None => {
            info!("No existing job found for URL, creating new job");
        }
    }
    
    let job = Job::new(request.url.clone());
    let job_id = job.id.clone();

    // Store the job in database
    data.job_repository.create_job(&job).await?;
    
    info!("Created job {} for URL: {}", job_id, request.url);

    // Parse priority
    let priority = match request.priority.as_deref() {
        Some("high") => JobPriority::High,
        Some("low") => JobPriority::Low,
        _ => JobPriority::Normal,
    };

    // Add job to queue
    if let Err(e) = data.job_queue.enqueue(job.clone(), priority).await {
        error!("Failed to enqueue job {}: {}", job_id, e);
        return Err(AppError::Internal(format!("Failed to queue job: {e}")));
    }
    
    info!("Enqueued job {} for processing", job_id);

    Ok(web::Json(JobResponse::from(&job)))
}

#[get("/status/{job_id}")]
#[instrument(skip(data), fields(job_id = %job_id))]
async fn get_job_status(
    data: web::Data<Arc<AppState>>,
    job_id: web::Path<String>,
) -> AppResult<impl Responder> {
    debug!("Getting status for job: {}", job_id);
    
    // Validate job_id input
    data.security_validator.validate_input(job_id.as_str(), "job_id", 100)?;
    
    let job = data.job_repository.get_job(job_id.as_str()).await?
        .ok_or_else(|| AppError::NotFound(format!("Job not found: {job_id}")))?;

    debug!("Job {} status: {:?}", job_id, job.status);
    Ok(web::Json(JobResponse::from(&job)))
}

#[get("/video/{job_id}")]
#[instrument(skip(data, req), fields(job_id = %job_id))]
async fn get_processed_video(
    data: web::Data<Arc<AppState>>,
    job_id: web::Path<String>,
    req: actix_web::HttpRequest,
) -> AppResult<impl Responder> {
    debug!("Streaming video for job: {}", job_id);
    
    // Validate job_id input
    data.security_validator.validate_input(job_id.as_str(), "job_id", 100)?;
    
    let job = data.job_repository.get_job(job_id.as_str()).await?
        .ok_or_else(|| AppError::NotFound(format!("Job not found: {job_id}")))?;

    if job.status != JobStatus::Completed {
        return Err(AppError::BadRequest("Job not completed yet".to_string()));
    }

    let processed_path = job.get_processed_path()
        .ok_or_else(|| AppError::NotFound("No processed file found".to_string()))?;

    // Check if file exists
    if !processed_path.exists() {
        error!("Processed file not found at path: {:?}", processed_path);
        return Err(AppError::NotFound("Processed file not found on disk".to_string()));
    }

    // Get file metadata
    let file_metadata = tokio::fs::metadata(&processed_path).await
        .map_err(|e| AppError::Internal(format!("Failed to get file metadata: {e}")))?;
    
    let file_size = file_metadata.len();
    info!("Streaming video file for job {}, size: {} bytes", job_id, file_size);

    // Create filename for download
    let filename = format!("video_{job_id}.mp4");
    
    // Create streaming response using actix-files NamedFile with optimized settings
    let file = actix_files::NamedFile::open(&processed_path)
        .map_err(|e| AppError::Internal(format!("Failed to open file for streaming: {e}")))?;

    // Enable range requests for better streaming support
    Ok(file
        .use_etag(true)
        .use_last_modified(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Attachment,
            parameters: vec![actix_web::http::header::DispositionParam::Filename(filename)],
        })
        .into_response(&req))
}

#[get("/stream/{job_id}")]
#[instrument(skip(data, req), fields(job_id = %job_id))]
async fn stream_processed_video(
    data: web::Data<Arc<AppState>>,
    job_id: web::Path<String>,
    req: actix_web::HttpRequest,
) -> AppResult<impl Responder> {
    debug!("Streaming video inline for job: {}", job_id);
    
    // Validate job_id input
    data.security_validator.validate_input(job_id.as_str(), "job_id", 100)?;
    
    let job = data.job_repository.get_job(job_id.as_str()).await?
        .ok_or_else(|| AppError::NotFound(format!("Job not found: {job_id}")))?;

    if job.status != JobStatus::Completed {
        return Err(AppError::BadRequest("Job not completed yet".to_string()));
    }

    let processed_path = job.get_processed_path()
        .ok_or_else(|| AppError::NotFound("No processed file found".to_string()))?;

    // Check if file exists
    if !processed_path.exists() {
        error!("Processed file not found at path: {:?}", processed_path);
        return Err(AppError::NotFound("Processed file not found on disk".to_string()));
    }

    // Get file metadata
    let file_metadata = tokio::fs::metadata(&processed_path).await
        .map_err(|e| AppError::Internal(format!("Failed to get file metadata: {e}")))?;
    
    let file_size = file_metadata.len();
    info!("Streaming video inline for job {}, size: {} bytes", job_id, file_size);

    // Create streaming response for inline viewing (no Content-Disposition header)
    let file = actix_files::NamedFile::open(&processed_path)
        .map_err(|e| AppError::Internal(format!("Failed to open file for streaming: {e}")))?;

    // Enable range requests and proper content type for video streaming
    Ok(file
        .use_etag(true)
        .use_last_modified(true)
        .set_content_type("video/mp4".parse::<mime::Mime>().unwrap())
        .into_response(&req))
}

#[delete("/jobs/{job_id}")]
#[instrument(skip(data), fields(job_id = %job_id))]
async fn cancel_job(
    data: web::Data<Arc<AppState>>,
    job_id: web::Path<String>,
) -> AppResult<impl Responder> {
    info!("Cancelling job: {}", job_id);
    
    // Validate job_id input
    data.security_validator.validate_input(job_id.as_str(), "job_id", 100)?;
    
    // Get the job from database
    let mut job = data.job_repository.get_job(job_id.as_str()).await?
        .ok_or_else(|| AppError::NotFound(format!("Job not found: {job_id}")))?;

    // Check if job can be cancelled
    match job.status {
        JobStatus::Completed => {
            return Err(AppError::BadRequest("Cannot cancel completed job".to_string()));
        }
        JobStatus::Cancelled => {
            return Err(AppError::BadRequest("Job already cancelled".to_string()));
        }
        JobStatus::Failed => {
            return Err(AppError::BadRequest("Cannot cancel failed job".to_string()));
        }
        _ => {} // Can cancel pending, downloading, or processing jobs
    }

    // Try to cancel the job in the queue/active jobs
    let cancelled = data.job_queue.cancel_job(job_id.as_str()).await
        .map_err(|e| AppError::Internal(format!("Failed to cancel job: {e}")))?;

    if cancelled {
        // Update job status in database
        job.update_status(JobStatus::Cancelled);
        job.set_error("Job cancelled by user".to_string());
        
        if let Err(e) = data.job_repository.update_job(&job).await {
            warn!("Failed to update cancelled job status in database: {}", e);
        }

        // Clean up any temporary files
        if let Err(e) = data.cleanup_service.cleanup_job_files(job_id.as_str()).await {
            warn!("Failed to cleanup files for cancelled job {}: {}", job_id, e);
        }

        info!("Successfully cancelled job: {}", job_id);
        Ok(web::Json(serde_json::json!({
            "message": "Job cancelled successfully",
            "job_id": job_id.as_str()
        })))
    } else {
        warn!("Job {} not found in queue or active jobs, may have already completed", job_id);
        Err(AppError::BadRequest("Job cannot be cancelled (may have already completed)".to_string()))
    }
}

#[derive(Deserialize, Debug)]
pub struct JobListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub status: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct JobListResponse {
    pub jobs: Vec<JobResponse>,
    pub pagination: PaginationInfo,
}

#[derive(Serialize, Debug)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub total_jobs: usize,
}

#[get("/jobs")]
#[instrument(skip(data))]
async fn list_jobs(
    data: web::Data<Arc<AppState>>,
    query: web::Query<JobListQuery>,
) -> AppResult<impl Responder> {
    debug!("Listing jobs with query: {:?}", query);
    
    // Parse and validate parameters
    let page = query.page.unwrap_or(0);
    let page_size = query.page_size.unwrap_or(20).min(100); // Max 100 items per page
    
    // Parse status filter if provided
    let status_filter = if let Some(status_str) = &query.status {
        match status_str.to_lowercase().as_str() {
            "pending" => Some(JobStatus::Pending),
            "downloading" => Some(JobStatus::Downloading),
            "processing" => Some(JobStatus::Processing),
            "completed" => Some(JobStatus::Completed),
            "failed" => Some(JobStatus::Failed),
            "cancelled" => Some(JobStatus::Cancelled),
            _ => return Err(AppError::BadRequest(format!("Invalid status filter: {status_str}"))),
        }
    } else {
        None
    };
    
    // Get paginated jobs
    let (jobs, total_pages) = data.job_repository
        .list_jobs_paginated(page, page_size, status_filter)
        .await?;
    
    let job_responses: Vec<JobResponse> = jobs.iter().map(JobResponse::from).collect();
    
    let response = JobListResponse {
        jobs: job_responses,
        pagination: PaginationInfo {
            current_page: page,
            page_size,
            total_pages,
            total_jobs: jobs.len(),
        },
    };
    
    debug!("Returning {} jobs on page {} of {}", jobs.len(), page, total_pages);
    Ok(web::Json(response))
}

#[instrument(skip(app_state), fields(job_id = %job_id))]
pub async fn process_job(job_id: &str, app_state: Arc<AppState>) {
    info!("Starting processing for job: {}", job_id);
    
    let cleanup_on_exit = {
        let job_id = job_id.to_string();
        let app_state = app_state.clone();
        move || async move {
            if let Err(e) = app_state.cleanup_service.cleanup_job_files(&job_id).await {
                warn!("Failed to cleanup files for job {}: {}", job_id, e);
            }
        }
    };

    // Get the job from database with retry
    let mut job = match retry_with_backoff(
        || app_state.job_repository.get_job(job_id),
        &RetryConfig::default(),
        "database_get_job"
    ).await {
        Ok(Some(job)) => job,
        Ok(None) => {
            error!("Job not found: {}", job_id);
            return;
        }
        Err(e) => {
            error!("Failed to get job {} after retries: {}", job_id, e);
            return;
        }
    };

    let start_time = std::time::Instant::now();

    // Download phase with retry and cleanup
    info!("Starting download phase for job: {}", job_id);
    
    // Update status to Downloading and save to database
    job.update_status(JobStatus::Downloading);
    if let Err(e) = update_job_with_retry(&job, &app_state).await {
        warn!("Failed to update job status to Downloading: {}", e);
    }
    
    let downloaded_path = match download_with_retry(&mut job, &app_state).await {
        Ok(path) => {
            info!("Download completed for job {}: {:?}", job_id, path);
            path
        }
        Err(e) => {
            error!("Download failed for job {}: {}", job_id, e);
            job.set_error(e.to_string());
            let _ = update_job_with_retry(&job, &app_state).await;
            cleanup_on_exit().await;
            return;
        }
    };

    // Processing phase with retry and cleanup
    info!("Starting processing phase for job: {}", job_id);
    
    // Update status to Processing and save to database
    job.update_status(JobStatus::Processing);
    if let Err(e) = update_job_with_retry(&job, &app_state).await {
        warn!("Failed to update job status to Processing: {}", e);
    }
    
    let _processed_path = match process_with_retry(&mut job, &downloaded_path, &app_state).await {
        Ok(path) => {
            info!("Processing completed for job {}: {:?}", job_id, path);
            path
        }
        Err(e) => {
            error!("Processing failed for job {}: {}", job_id, e);
            job.set_error(e.to_string());
            let _ = update_job_with_retry(&job, &app_state).await;
            cleanup_on_exit().await;
            return;
        }
    };

    // Mark as completed and cleanup temporary files
    job.update_status(JobStatus::Completed);
    job.set_processing_time(start_time.elapsed());

    if let Err(e) = update_job_with_retry(&job, &app_state).await {
        error!("Failed to update job completion status: {}", e);
    } else {
        info!("Job {} completed successfully in {:?}", job_id, start_time.elapsed());
    }

    // Clean up temporary download files (keep processed files)
    if let Some(downloaded_path) = job.get_downloaded_path() {
        if let Err(e) = app_state.cleanup_service.cleanup_file(&downloaded_path).await {
            warn!("Failed to cleanup downloaded file: {}", e);
        }
    }
}

async fn download_with_retry(job: &mut Job, app_state: &Arc<AppState>) -> AppResult<std::path::PathBuf> {
    let retry_config = RetryConfig {
        max_attempts: 2, // Reduce retry attempts
        base_delay: std::time::Duration::from_secs(1),
        max_delay: std::time::Duration::from_secs(10),
        backoff_multiplier: 2.0,
    };

    let download_result = retry_with_backoff(
        || {
            let app_state = app_state.clone();
            let mut job_clone = job.clone();
            async move {
                app_state.download_service.download(&mut job_clone).await
            }
        },
        &retry_config,
        "video_download"
    ).await;

    match download_result {
        Ok(path) => {
            job.set_downloaded_path(path.clone());
            let _ = update_job_with_retry(job, app_state).await;
            Ok(path)
        }
        Err(e) if is_retryable_error(&e) => {
            Err(AppError::Download(format!("Download failed after retries: {e}")))
        }
        Err(e) => Err(e),
    }
}

async fn process_with_retry(
    job: &mut Job,
    input_path: &Path,
    app_state: &Arc<AppState>
) -> AppResult<std::path::PathBuf> {
    let retry_config = RetryConfig {
        max_attempts: 1, // No retries for processing - either works or fails
        base_delay: std::time::Duration::from_secs(1),
        max_delay: std::time::Duration::from_secs(5),
        backoff_multiplier: 1.0,
    };

    let process_result = retry_with_backoff(
        || {
            let app_state = app_state.clone();
            let mut job_clone = job.clone();
            let input_path = input_path.to_path_buf();
            async move {
                app_state.process_service.process(&mut job_clone, &input_path).await
            }
        },
        &retry_config,
        "video_processing"
    ).await;

    match process_result {
        Ok(path) => {
            job.set_processed_path(path.clone());
            let _ = update_job_with_retry(job, app_state).await;
            Ok(path)
        }
        Err(e) if is_retryable_error(&e) => {
            Err(AppError::Processing(format!("Processing failed after retries: {e}")))
        }
        Err(e) => Err(e),
    }
}

async fn update_job_with_retry(job: &Job, app_state: &Arc<AppState>) -> AppResult<()> {
    let retry_config = RetryConfig {
        max_attempts: 3, // Reduce database retry attempts
        base_delay: std::time::Duration::from_millis(50),
        max_delay: std::time::Duration::from_secs(2),
        backoff_multiplier: 2.0,
    };

    retry_with_backoff(
        || app_state.job_repository.update_job(job),
        &retry_config,
        "database_update"
    ).await
}
