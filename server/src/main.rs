use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use log::error;

use std::time::Duration;
use vectorize_core::config::Config;
use vectorize_core::init;
use vectorize_worker::{WorkerHealthMonitor, start_vectorize_worker_with_monitoring};

#[actix_web::main]
async fn main() {
    env_logger::init();

    let cfg = Config::from_env();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");
    let server_port = cfg.webserver_port;
    let server_workers = cfg.num_server_workers;
    init::init_project(&pool, Some(&cfg.database_url))
        .await
        .expect("Failed to initialize project");

    // Start the vectorize worker with health monitoring
    let worker_pool = pool.clone();
    let worker_cfg = cfg.clone();
    let worker_health_monitor = WorkerHealthMonitor::new();
    let worker_health_for_routes = worker_health_monitor.get_arc_clone();

    tokio::spawn(async move {
        if let Err(e) =
            start_vectorize_worker_with_monitoring(worker_cfg, worker_pool, worker_health_monitor)
                .await
        {
            error!("Failed to start vectorize worker: {}", e);
        }
    });

    let _ = HttpServer::new(move || {
        let cors = Cors::permissive();

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(cfg.clone()))
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(worker_health_for_routes.clone()))
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
