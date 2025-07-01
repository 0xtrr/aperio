use crate::error::{AppError, AppResult};
use crate::monitoring::HealthChecker;
use crate::services::metrics;
use actix_web::{get, web, Responder, HttpResponse};
use std::sync::Arc;

pub struct MonitoringState {
    pub health_checker: HealthChecker,
}

pub fn configure_monitoring_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(health_check)
        .service(health_check_detailed)
        .service(metrics_endpoint)
        .service(metrics_prometheus)
        .service(metrics_history)
        .service(readiness_check)
        .service(liveness_check);
}

#[get("/health")]
async fn health_check(data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let health_status = data.health_checker.get_health_status().await;
    
    match health_status.status.as_str() {
        "healthy" => Ok(web::Json(health_status)),
        "degraded" => Ok(web::Json(health_status)), // 200 but with warnings
        _ => Err(AppError::Internal("Service unhealthy".to_string())), // 500 for critical
    }
}

#[get("/health/detailed")]
async fn health_check_detailed(data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let health_status = data.health_checker.get_health_status().await;
    Ok(web::Json(health_status))
}

#[get("/metrics")]
async fn metrics_endpoint(_data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let metrics_registry = metrics::get_metrics();
    let metrics = metrics_registry.get_json_format().await;
    Ok(web::Json(metrics))
}

#[get("/metrics/prometheus")]
async fn metrics_prometheus(_data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let metrics_registry = metrics::get_metrics();
    let prometheus_format = metrics_registry.get_prometheus_format().await;
    Ok(HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4; charset=utf-8")
        .body(prometheus_format))
}

#[get("/metrics/history")]
async fn metrics_history(_data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let metrics_registry = metrics::get_metrics();
    let history = metrics_registry.get_metrics_history(Some(50)).await;
    Ok(web::Json(history))
}

#[get("/health/ready")]
async fn readiness_check(data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    let health_status = data.health_checker.get_health_status().await;
    
    // Ready if database is healthy (can serve requests)
    if health_status.checks.database.status == "healthy" {
        Ok(web::Json(serde_json::json!({
            "status": "ready",
            "timestamp": health_status.timestamp
        })))
    } else {
        Err(AppError::Internal("Service not ready".to_string()))
    }
}

#[get("/health/live")]
async fn liveness_check(_data: web::Data<Arc<MonitoringState>>) -> AppResult<impl Responder> {
    // Simple liveness check - if we can respond, we're alive
    Ok(web::Json(serde_json::json!({
        "status": "alive",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    })))
}
