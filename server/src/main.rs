use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use std::time::Duration;
use tracing::error;

use vectorize_core::config::Config;
use vectorize_proxy::start_postgres_proxy;
use vectorize_server::app_state::AppState;
use vectorize_worker::{WorkerHealthMonitor, start_vectorize_worker_with_monitoring};

#[actix_web::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let cfg = Config::from_env();

    let app_state = AppState::new(cfg)
        .await
        .expect("Failed to initialize application state");

    // start the PostgreSQL proxy if enabled
    if app_state.config.proxy_enabled {
        let proxy_port = app_state.config.vectorize_proxy_port;
        let database_url = app_state.config.database_url.clone();
        let job_cache = app_state.job_cache.clone();
        let db_pool = app_state.db_pool.clone();

        tokio::spawn(async move {
            if let Err(e) = start_postgres_proxy(proxy_port, database_url, job_cache, db_pool).await
            {
                error!("Failed to start PostgreSQL proxy: {e}");
            }
        });
    }

    // start the vectorize worker with health monitoring
    let worker_state = app_state.clone();
    let worker_health_monitor = WorkerHealthMonitor::new();

    tokio::spawn(async move {
        if let Err(e) = start_vectorize_worker_with_monitoring(
            worker_state.config.clone(),
            worker_state.db_pool.clone(),
            worker_health_monitor,
        )
        .await
        {
            error!("Failed to start vectorize worker: {e}");
        }
    });

    // store values before moving app_state
    let server_workers = app_state.config.num_server_workers;
    let server_port = app_state.config.webserver_port;

    let _ = HttpServer::new(move || {
        let cors = Cors::permissive();

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(app_state.clone()))
            .configure(vectorize_server::server::route_config)
            .configure(vectorize_server::routes::health::configure_health_routes)
    })
    .workers(server_workers)
    .keep_alive(Duration::from_secs(75))
    .bind(("0.0.0.0", server_port))
    .expect("Failed to bind server")
    .run()
    .await;
}
