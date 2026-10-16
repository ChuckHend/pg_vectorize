use crate::app_state::AppState;
use actix_web::{HttpResponse, Result, web};
use serde_json::json;
use std::time::SystemTime;

pub async fn health_check(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    // with the in-process worker off there is nothing to monitor here
    if !app_state.config.worker_enabled {
        return Ok(HttpResponse::Ok().json(json!({
            "status": "healthy",
            "worker": { "status": "Disabled" },
            "timestamp": SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })));
    }

    let health = app_state.worker_health.read().await;
    let is_healthy = health.is_up();

    let response = json!({
        "status": if is_healthy { "healthy" } else { "unhealthy" },
        "worker": health.report(),
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

pub async fn readiness_check(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    if !app_state.config.worker_enabled {
        return Ok(HttpResponse::Ok().json(json!({
            "status": "ready",
            "worker_status": "Disabled",
            "timestamp": SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })));
    }

    let health = app_state.worker_health.read().await;
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
