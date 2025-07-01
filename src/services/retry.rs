use crate::error::{AppError, AppResult};
use std::time::Duration;
use tokio::time::sleep;

pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

pub async fn retry_with_backoff<F, Fut, T>(
    mut operation: F,
    config: &RetryConfig,
    operation_name: &str,
) -> AppResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    let mut last_error = None;

    for attempt in 1..=config.max_attempts {
        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    println!("{operation_name} succeeded on attempt {attempt}");
                }
                return Ok(result);
            }
            Err(e) => {
                last_error = Some(e);

                if attempt < config.max_attempts {
                    let delay = calculate_backoff_delay(attempt, config);
                    println!(
                        "{} failed on attempt {} ({}), retrying in {:?}",
                        operation_name, attempt, last_error.as_ref().unwrap(), delay
                    );
                    sleep(delay).await;
                } else {
                    println!(
                        "{} failed on final attempt {} ({})",
                        operation_name, attempt, last_error.as_ref().unwrap()
                    );
                }
            }
        }
    }

    Err(last_error.unwrap())
}

fn calculate_backoff_delay(attempt: u32, config: &RetryConfig) -> Duration {
    let delay_secs = config.base_delay.as_secs_f64()
        * config.backoff_multiplier.powi((attempt - 1) as i32);

    
    Duration::from_secs_f64(delay_secs.min(config.max_delay.as_secs_f64()))
}

pub fn is_retryable_error(error: &AppError) -> bool {
    match error {
        AppError::Timeout(_) => true,
        AppError::Download(msg) => {
            let msg_lower = msg.to_lowercase();
            // Retry on network-related download errors
            msg_lower.contains("timeout")
                || msg_lower.contains("connection")
                || msg_lower.contains("network")
                || msg_lower.contains("temporary")
                || msg_lower.contains("unavailable")
                || msg_lower.contains("reset")
                || msg_lower.contains("refused")
                // HTTP status codes that indicate temporary issues
                || msg_lower.contains("502")
                || msg_lower.contains("503")
                || msg_lower.contains("504")
                || msg_lower.contains("429") // Rate limited
        }
        AppError::Processing(msg) => {
            let msg_lower = msg.to_lowercase();
            // Retry on temporary processing errors
            msg_lower.contains("resource temporarily unavailable")
                || msg_lower.contains("device busy")
                || msg_lower.contains("temporary failure")
                || msg_lower.contains("disk full") // Could be temporary
        }
        AppError::Internal(msg) => {
            let msg_lower = msg.to_lowercase();
            // Retry on database connection issues and temporary internal errors
            msg_lower.contains("database") && (
                msg_lower.contains("busy") 
                || msg_lower.contains("locked")
                || msg_lower.contains("connection")
            )
        }
        AppError::Storage(_) => false, // Don't retry storage errors
        AppError::BadRequest(_) => false, // Don't retry client errors
        AppError::NotFound(_) => false, // Don't retry not found errors
    }
}
