use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware, web};
use log::{error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use url::Url;

use vectorize_core::config::Config;
use vectorize_core::init;
use vectorize_core::types::VectorizeJob;
use vectorize_proxy::{
    ProxyConfig, handle_connection_with_timeout, load_initial_job_cache,
    setup_job_change_notifications, start_cache_sync_listener,
};
use vectorize_worker::{WorkerHealthMonitor, start_vectorize_worker_with_monitoring};

#[actix_web::main]
async fn main() {
    env_logger::init();

    let cfg = Config::from_env();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(cfg.database_pool_max)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");

    // Create a separate connection pool for cache refresher
    let cache_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(cfg.database_cache_pool_max)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres for cache refresher");
    let server_port = cfg.webserver_port;
    let server_workers = cfg.num_server_workers;
    init::init_project(&pool)
        .await
        .expect("Failed to initialize project");

    // Load initial job cache and setup job change notifications
    let jobmap = load_initial_job_cache(&pool)
        .await
        .expect("Failed to load initial job cache");
    let jobmap = Arc::new(RwLock::new(jobmap));

    if let Err(e) = setup_job_change_notifications(&pool).await {
        warn!("Failed to setup job change notifications: {e}");
    }

    // Start the PostgreSQL proxy if enabled
    if cfg.proxy_enabled {
        let proxy_pool = pool.clone();
        let proxy_cfg = cfg.clone();
        let proxy_jobmap = Arc::clone(&jobmap);
        let proxy_cache_pool = cache_pool.clone();
        tokio::spawn(async move {
            if let Err(e) =
                start_postgres_proxy(proxy_cfg, proxy_pool, proxy_jobmap, proxy_cache_pool).await
            {
                error!("Failed to start PostgreSQL proxy: {e}");
            }
        });
    }

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
            error!("Failed to start vectorize worker: {e}");
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
            .app_data(web::Data::new(jobmap.clone()))
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

async fn start_postgres_proxy(
    cfg: Config,
    pool: sqlx::PgPool,
    jobmap: Arc<RwLock<HashMap<String, VectorizeJob>>>,
    cache_pool: sqlx::PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let bind_address = "0.0.0.0";
    let timeout = 30;

    let listen_addr: SocketAddr =
        format!("{}:{}", bind_address, cfg.vectorize_proxy_port).parse()?;

    let url = Url::parse(&cfg.database_url)?;
    let postgres_host = url.host_str().unwrap();
    let postgres_port = url.port().unwrap();

    let postgres_addr: SocketAddr = format!("{postgres_host}:{postgres_port}")
        .to_socket_addrs()?
        .next()
        .ok_or("Failed to resolve PostgreSQL host address")?;

    let config = Arc::new(ProxyConfig {
        postgres_addr,
        timeout: Duration::from_secs(timeout),
        jobmap: Arc::clone(&jobmap),
        db_pool: pool.clone(),
        prepared_statements: Arc::new(RwLock::new(HashMap::new())),
    });

    info!("Proxy listening on: {listen_addr}");
    info!("Forwarding to PostgreSQL at: {postgres_addr}");

    // Start cache sync listener with its own connection pool
    let cache_pool_for_sync = cache_pool.clone();
    let jobmap_for_sync = Arc::clone(&jobmap);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let sync_config = Arc::new(ProxyConfig {
            postgres_addr,
            timeout: Duration::from_secs(timeout),
            jobmap: jobmap_for_sync,
            db_pool: cache_pool_for_sync,
            prepared_statements: Arc::new(RwLock::new(HashMap::new())),
        });
        if let Err(e) = start_cache_sync_listener(sync_config).await {
            error!("Cache synchronization error: {e}");
        }
    });

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
