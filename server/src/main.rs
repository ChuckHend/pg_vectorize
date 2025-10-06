use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{error, info};
use url::Url;

use vectorize_core::config::Config;
use vectorize_proxy::{ProxyConfig, handle_connection_with_timeout};
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
        let proxy_state = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = start_postgres_proxy(proxy_state).await {
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

async fn start_postgres_proxy(app_state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let bind_address = "0.0.0.0";
    let timeout = 30;

    let listen_addr: SocketAddr =
        format!("{}:{}", bind_address, app_state.config.vectorize_proxy_port).parse()?;

    let url = Url::parse(&app_state.config.database_url)?;
    let postgres_host = url.host_str().unwrap();
    let postgres_port = url.port().unwrap();

    let postgres_addr: SocketAddr = format!("{postgres_host}:{postgres_port}")
        .to_socket_addrs()?
        .next()
        .ok_or("Failed to resolve PostgreSQL host address")?;

    let config = Arc::new(ProxyConfig {
        postgres_addr,
        timeout: Duration::from_secs(timeout),
        jobmap: app_state.job_cache.clone(),
        db_pool: app_state.db_pool.clone(),
        prepared_statements: Arc::new(RwLock::new(HashMap::new())),
    });

    info!("Proxy listening on: {listen_addr}");
    info!("Forwarding to PostgreSQL at: {postgres_addr}");

    let listener = TcpListener::bind(listen_addr).await?;

    loop {
        match listener.accept().await {
            Ok((client_stream, client_addr)) => {
                info!("New proxy connection from: {client_addr}");

                let config = Arc::clone(&config);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection_with_timeout(client_stream, config).await {
                        error!("Proxy connection error from {client_addr}: {e}");
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept proxy connection: {e}");
            }
        }
    }
}
