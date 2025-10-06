use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::error;
use url::Url;
use vectorize_core::config::Config;
use vectorize_core::types::VectorizeJob;
use vectorize_proxy::{ProxyConfig, start_cache_sync_listener};
use vectorize_worker::WorkerHealth;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db_pool: sqlx::PgPool,
    pub cache_pool: sqlx::PgPool,
    /// in-memory cache of existing vectorize jobs and their metadata
    pub job_cache: Arc<RwLock<HashMap<String, VectorizeJob>>>,
    /// worker health monitoring data
    pub worker_health: Arc<RwLock<WorkerHealth>>,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(config.database_pool_max)
            .connect(&config.database_url)
            .await?;

        let cache_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(config.database_cache_pool_max)
            .connect(&config.database_url)
            .await?;

        vectorize_core::init::init_project(&db_pool)
            .await
            .map_err(|e| format!("Failed to initialize project: {e}"))?;

        // load initial job cache
        let job_cache = vectorize_proxy::load_initial_job_cache(&db_pool)
            .await
            .map_err(|e| format!("Failed to load initial job cache: {e}"))?;
        let job_cache = Arc::new(RwLock::new(job_cache));

        // setup job change notifications
        if let Err(e) = vectorize_proxy::setup_job_change_notifications(&db_pool).await {
            tracing::warn!("Failed to setup job change notifications: {e}");
        }
        // start cache sync listener for job changes
        Self::start_cache_sync_listener(&config, &cache_pool, &job_cache).await;

        // Initialize worker health monitoring
        let worker_health = Arc::new(RwLock::new(WorkerHealth {
            status: vectorize_worker::WorkerStatus::Starting,
            last_heartbeat: std::time::SystemTime::now(),
            jobs_processed: 0,
            uptime: std::time::Duration::from_secs(0),
            restart_count: 0,
            last_error: None,
        }));

        Ok(AppState {
            config,
            db_pool,
            cache_pool,
            job_cache,
            worker_health,
        })
    }

    /// Start the cache sync listener for job changes
    async fn start_cache_sync_listener(
        config: &Config,
        cache_pool: &sqlx::PgPool,
        job_cache: &Arc<RwLock<HashMap<String, VectorizeJob>>>,
    ) {
        let cache_pool_for_sync = cache_pool.clone();
        let jobmap_for_sync = job_cache.clone();
        let database_url = config.database_url.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;

            let url = match Url::parse(&database_url) {
                Ok(url) => url,
                Err(e) => {
                    error!("Failed to parse database URL: {e}");
                    return;
                }
            };

            let postgres_host = match url.host_str() {
                Some(host) => host,
                None => {
                    error!("No host in database URL");
                    return;
                }
            };

            let postgres_port = match url.port() {
                Some(port) => port,
                None => {
                    error!("No port in database URL");
                    return;
                }
            };

            let postgres_addr: SocketAddr =
                match format!("{postgres_host}:{postgres_port}").to_socket_addrs() {
                    Ok(mut addrs) => match addrs.next() {
                        Some(addr) => addr,
                        None => {
                            error!("Failed to resolve PostgreSQL host address");
                            return;
                        }
                    },
                    Err(e) => {
                        error!("Failed to resolve PostgreSQL host address: {e}");
                        return;
                    }
                };

            let sync_config = Arc::new(ProxyConfig {
                postgres_addr,
                timeout: Duration::from_secs(30),
                jobmap: jobmap_for_sync,
                db_pool: cache_pool_for_sync,
                prepared_statements: Arc::new(RwLock::new(HashMap::new())),
            });

            if let Err(e) = start_cache_sync_listener(sync_config).await {
                error!("Cache synchronization error: {e}");
            }
        });
    }
}
