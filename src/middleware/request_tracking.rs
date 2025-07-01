use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage,
};
use std::future::{ready, Ready, Future};
use std::pin::Pin;
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

pub struct RequestTracking;

impl<S, B> Transform<S, ServiceRequest> for RequestTracking
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RequestTrackingMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestTrackingMiddleware { service }))
    }
}

pub struct RequestTrackingMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for RequestTrackingMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let start_time = Instant::now();
        let correlation_id = Uuid::new_v4().to_string();
        let method = req.method().to_string();
        let path = req.path().to_string();
        let user_agent = req
            .headers()
            .get("user-agent")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown")
            .to_string();

        // Add correlation ID to request extensions
        req.extensions_mut().insert(correlation_id.clone());

        // Create a span for this request
        let span = tracing::info_span!(
            "http_request",
            correlation_id = %correlation_id,
            method = %method,
            path = %path,
            user_agent = %user_agent
        );

        let fut = self.service.call(req);

        Box::pin(async move {
            let _guard = span.enter();

            info!(
                correlation_id = %correlation_id,
                method = %method,
                path = %path,
                "Request started"
            );

            let result = fut.await;

            let duration = start_time.elapsed();
            let duration_ms = duration.as_millis() as f64;

            match &result {
                Ok(response) => {
                    let status = response.status().as_u16();

                    if status >= 400 {
                        warn!(
                            correlation_id = %correlation_id,
                            method = %method,
                            path = %path,
                            status = status,
                            duration_ms = duration_ms,
                            "Request completed with error"
                        );
                    } else {
                        info!(
                            correlation_id = %correlation_id,
                            method = %method,
                            path = %path,
                            status = status,
                            duration_ms = duration_ms,
                            "Request completed successfully"
                        );
                    }

                    // Store metrics for collection
                    REQUEST_METRICS.record_request(duration_ms, status >= 400);
                }
                Err(error) => {
                    warn!(
                        correlation_id = %correlation_id,
                        method = %method,
                        path = %path,
                        error = %error,
                        duration_ms = duration_ms,
                        "Request failed with error"
                    );

                    REQUEST_METRICS.record_request(duration_ms, true);
                }
            }

            result
        })
    }
}

// Simple metrics collector
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::RwLock;

pub struct RequestMetrics {
    total_requests: AtomicUsize,
    error_requests: AtomicUsize,
    total_duration_ms: AtomicU64,
    recent_requests: RwLock<Vec<RequestData>>,
}

#[derive(Clone)]
struct RequestData {
    timestamp: std::time::SystemTime,
    #[allow(dead_code)]
    duration_ms: f64,
    #[allow(dead_code)]
    is_error: bool,
}

impl RequestMetrics {
    const fn new() -> Self {
        Self {
            total_requests: AtomicUsize::new(0),
            error_requests: AtomicUsize::new(0),
            total_duration_ms: AtomicU64::new(0),
            recent_requests: RwLock::new(Vec::new()),
        }
    }

    fn record_request(&self, duration_ms: f64, is_error: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms.fetch_add(duration_ms as u64, Ordering::Relaxed);

        if is_error {
            self.error_requests.fetch_add(1, Ordering::Relaxed);
        }

        // Keep recent requests for rate calculation (last 1000)
        if let Ok(mut recent) = self.recent_requests.write() {
            recent.push(RequestData {
                timestamp: std::time::SystemTime::now(),
                duration_ms,
                is_error,
            });

            // Keep only last 1000 requests
            if recent.len() > 1000 {
                let excess = recent.len() - 1000;
                recent.drain(0..excess);
            }
        }
    }

    pub fn get_stats(&self) -> (f64, f64, f64) {
        let total = self.total_requests.load(Ordering::Relaxed);
        let errors = self.error_requests.load(Ordering::Relaxed);
        let total_duration = self.total_duration_ms.load(Ordering::Relaxed);

        let error_rate = if total > 0 {
            (errors as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let avg_response_time = if total > 0 {
            total_duration as f64 / total as f64
        } else {
            0.0
        };

        // Calculate requests per minute from recent data
        let requests_per_minute = if let Ok(recent) = self.recent_requests.read() {
            let now = std::time::SystemTime::now();
            let one_minute_ago = now - std::time::Duration::from_secs(60);

            let recent_count = recent
                .iter()
                .filter(|req| req.timestamp > one_minute_ago)
                .count();

            recent_count as f64
        } else {
            0.0
        };

        (requests_per_minute, avg_response_time, error_rate)
    }
}

static REQUEST_METRICS: RequestMetrics = RequestMetrics::new();

pub fn get_request_metrics() -> (f64, f64, f64) {
    REQUEST_METRICS.get_stats()
}
