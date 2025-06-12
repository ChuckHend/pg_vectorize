use actix_web::{HttpResponse, Result, web};
use serde_json::json;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

use vectorize_worker::WorkerHealth;

pub async fn health_check(
    worker_health: web::Data<Arc<RwLock<WorkerHealth>>>,
) -> Result<HttpResponse> {
    let health = worker_health.read().await;
    let is_healthy = match &health.status {
        vectorize_worker::WorkerStatus::Healthy => true,
        vectorize_worker::WorkerStatus::Starting => {
            health
                .last_heartbeat
                .elapsed()
                .unwrap_or_default()
                .as_secs()
                < 120
        }
        _ => false,
    };

    let response = json!({
        "status": if is_healthy { "healthy" } else { "unhealthy" },
        "worker": {
            "status": format!("{:?}", health.status),
            "last_heartbeat": health.last_heartbeat
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "jobs_processed": health.jobs_processed,
            "uptime_seconds": health.uptime.as_secs(),
            "restart_count": health.restart_count,
            "last_error": health.last_error
        },
        "timestamp": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    if is_healthy {
        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(response))
    }
}

pub async fn liveness_check() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "alive",
        "timestamp": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    })))
}

pub async fn readiness_check(
    worker_health: web::Data<Arc<RwLock<WorkerHealth>>>,
) -> Result<HttpResponse> {
    let health = worker_health.read().await;
    let is_ready = matches!(health.status, vectorize_worker::WorkerStatus::Healthy);

    let response = json!({
        "status": if is_ready { "ready" } else { "not_ready" },
        "worker_status": format!("{:?}", health.status),
        "timestamp": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    if is_ready {
        Ok(HttpResponse::Ok().json(response))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(response))
    }
}

pub fn configure_health_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/health")
            .route("", web::get().to(health_check))
            .route("/live", web::get().to(liveness_check))
            .route("/ready", web::get().to(readiness_check)),
    );
}
