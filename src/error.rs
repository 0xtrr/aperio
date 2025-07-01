use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;
use std::fmt::Debug;

#[derive(Debug, Clone, PartialEq)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
    #[allow(dead_code)]
    Storage(String),
    Download(String),
    Processing(String),
    Timeout(String),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    error_type: String,
    message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Download(msg) => write!(f, "Download error: {msg}"),
            AppError::Processing(msg) => write!(f, "Processing error: {msg}"),
            AppError::Storage(msg) => write!(f, "Storage error: {msg}"),
            AppError::Timeout(msg) => write!(f, "Timeout error: {msg}"),
            AppError::Internal(msg) => write!(f, "Internal error: {msg}"),
            AppError::BadRequest(msg) => write!(f, "Bad Request error: {msg}"),
            AppError::NotFound(msg) => write!(f, "Not Found error: {msg}"),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (error_type, message) = match self {
            AppError::Download(msg) => ("download_error", msg),
            AppError::Processing(msg) => ("processing_error", msg),
            AppError::Storage(msg) => ("storage_error", msg),
            AppError::Timeout(msg) => ("timeout_error", msg),
            AppError::Internal(msg) => ("internal_error", msg),
            AppError::BadRequest(msg) => ("bad_request", msg),
            AppError::NotFound(msg) => ("not_found", msg),
        };

        let error_response = ErrorResponse {
            error: "request_failed".to_string(),
            error_type: error_type.to_string(),
            message: message.clone(),
        };

        match self {
            AppError::Download(_) => HttpResponse::BadRequest().json(error_response),
            AppError::Processing(_) => HttpResponse::InternalServerError().json(error_response),
            AppError::Storage(_) => HttpResponse::InternalServerError().json(error_response),
            AppError::Timeout(_) => HttpResponse::RequestTimeout().json(error_response),
            AppError::Internal(_) => HttpResponse::InternalServerError().json(error_response),
            AppError::BadRequest(_) => HttpResponse::BadRequest().json(error_response),
            AppError::NotFound(_) => HttpResponse::NotFound().json(error_response),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;