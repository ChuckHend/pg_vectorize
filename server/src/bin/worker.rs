use actix_web::{App, HttpResponse, HttpServer, web};
use serde_json::json;
use std::time::SystemTime;
use tracing::{error, info};
use vectorize_core::config::Config;
use vectorize_core::init;
use vectorize_worker::{WorkerHealthMonitor, start_vectorize_worker_with_monitoring};

async fn health(monitor: web::Data<WorkerHealthMonitor>) -> HttpResponse {
    let health = monitor.get_health().await;
    let is_up = health.is_up();

    let response = json!({
        "status": if is_up { "healthy" } else { "unhealthy" },
        "worker": health.report(),
        "timestamp": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    if is_up {
        HttpResponse::Ok().json(response)
    } else {
        HttpResponse::ServiceUnavailable().json(response)
    }
}

#[actix_web::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    info!("starting pg-vectorize worker");

    let cfg = Config::from_env();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");

    init::init_project(&pool)
        .await
        .expect("Failed to initialize project");

    vectorize_server::db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    let monitor = WorkerHealthMonitor::new();

    let worker = tokio::spawn(start_vectorize_worker_with_monitoring(
        cfg.clone(),
        pool,
        monitor.clone(),
    ));

    let health_server = HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(monitor.clone()))
            .route("/health", web::get().to(health))
    })
    .workers(1)
    .bind(("0.0.0.0", cfg.worker_health_port))
    .expect("Failed to bind health server")
    .run();

    // exit when either stops (a signal stops the health server; the worker returns only
    // after giving up on restarts) so the container restarts instead of idling
    tokio::select! {
        _ = health_server => info!("shutting down"),
        res = worker => {
            error!("worker stopped: {res:?}");
            std::process::exit(1);
        }
    }
}
