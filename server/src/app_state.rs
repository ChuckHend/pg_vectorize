use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info};
use url::Url;
use vectorize_core::config::Config;
use vectorize_core::types::VectorizeJob;
use vectorize_worker::WorkerHealth;

#[derive(Debug, thiserror::Error)]
pub enum AppStateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Connection timeout")]
    Timeout,
}

#[derive(Clone)]
pub struct CacheSyncConfig {
    pub postgres_addr: std::net::SocketAddr,
    pub timeout: Duration,
    pub jobmap: Arc<RwLock<HashMap<String, VectorizeJob>>>,
    pub db_pool: sqlx::PgPool,
    pub prepared_statements: Arc<RwLock<HashMap<String, PreparedStatement>>>,
}

#[derive(Debug, Clone)]
pub struct PreparedStatement {
    pub statement_name: String,
    pub sql: String,
    pub embed_calls: Vec<EmbedCall>,
}

#[derive(Debug, Clone)]
pub struct EmbedCall {
    pub column_name: String,
    pub model_name: String,
}

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
        let job_cache = load_initial_job_cache(&db_pool)
            .await
            .map_err(|e| format!("Failed to load initial job cache: {e}"))?;
        let job_cache = Arc::new(RwLock::new(job_cache));

        if let Err(e) = setup_job_change_notifications(&db_pool).await {
            tracing::warn!("Failed to setup job change notifications: {e}");
        }
        Self::start_cache_sync_listener_task(&config, &cache_pool, &job_cache).await;

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

    async fn start_cache_sync_listener_task(
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

            let sync_config = Arc::new(CacheSyncConfig {
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

// Cache sync functions copied from proxy module
pub async fn setup_job_change_notifications(
    pool: &sqlx::PgPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut tx = pool.begin().await?;

    let create_notify_function = r#"
        CREATE OR REPLACE FUNCTION vectorize.notify_job_change()
        RETURNS TRIGGER AS $$
        BEGIN
            IF TG_OP = 'DELETE' THEN
                PERFORM pg_notify('vectorize_job_changes', 
                    json_build_object(
                        'operation', TG_OP,
                        'job_name', OLD.job_name
                    )::text
                );
                RETURN OLD;
            ELSE
                PERFORM pg_notify('vectorize_job_changes', 
                    json_build_object(
                        'operation', TG_OP,
                        'job_name', NEW.job_name
                    )::text
                );
                RETURN NEW;
            END IF;
        END;
        $$ LANGUAGE plpgsql;
    "#;

    sqlx::query("DROP TRIGGER IF EXISTS job_change_trigger ON vectorize.job;")
        .execute(&mut *tx)
        .await?;

    let create_trigger = r#"
        CREATE TRIGGER job_change_trigger
            AFTER INSERT OR UPDATE OR DELETE ON vectorize.job
            FOR EACH ROW EXECUTE FUNCTION vectorize.notify_job_change();
    "#;

    sqlx::query(create_notify_function)
        .execute(&mut *tx)
        .await?;
    sqlx::query(create_trigger).execute(&mut *tx).await?;

    tx.commit().await?;
    info!("Database trigger for job changes setup successfully");
    Ok(())
}

pub async fn start_cache_sync_listener(
    config: Arc<CacheSyncConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(60);

    loop {
        match try_listen_for_changes(&config).await {
            Ok(_) => retry_delay = Duration::from_secs(1),
            Err(e) => {
                error!("Cache sync listener error: {e}. Retrying in {retry_delay:?}");
                tokio::time::sleep(retry_delay).await;
                retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
            }
        }
    }
}

async fn try_listen_for_changes(
    config: &CacheSyncConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&config.db_pool).await?;
    listener.listen("vectorize_job_changes").await?;

    info!("Connected and listening for vectorize job changes");

    loop {
        match listener.recv().await {
            Ok(notification) => {
                info!(
                    "Received job change notification: {}",
                    notification.payload()
                );

                if let Ok(payload) =
                    serde_json::from_str::<serde_json::Value>(notification.payload())
                {
                    let operation = payload.get("operation").and_then(|v| v.as_str());
                    let job_name = payload.get("job_name").and_then(|v| v.as_str());
                    info!(
                        "Job change detected - Operation: {}, Job: {}",
                        operation.unwrap_or("unknown"),
                        job_name.unwrap_or("unknown")
                    );
                }

                if let Err(e) = refresh_job_cache(config).await {
                    error!("Failed to refresh job cache: {e}");
                } else {
                    info!("Job cache refreshed successfully");
                }
            }
            Err(e) => {
                error!("Error receiving notification: {e}");
                return Err(e.into());
            }
        }
    }
}

pub async fn refresh_job_cache(
    config: &CacheSyncConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let all_jobs: Vec<VectorizeJob> = sqlx::query_as(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model FROM vectorize.job",
    )
    .fetch_all(&config.db_pool)
    .await?;

    let jobmap: HashMap<String, VectorizeJob> = all_jobs
        .into_iter()
        .map(|mut item| {
            let key = std::mem::take(&mut item.job_name);
            (key, item)
        })
        .collect();

    {
        let mut jobmap_write = config.jobmap.write().await;
        *jobmap_write = jobmap;
        info!("Updated job cache with {} jobs", jobmap_write.len());
    }

    Ok(())
}

pub async fn load_initial_job_cache(
    pool: &sqlx::PgPool,
) -> Result<HashMap<String, VectorizeJob>, AppStateError> {
    let all_jobs: Vec<VectorizeJob> = sqlx::query_as(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model FROM vectorize.job",
    )
    .fetch_all(pool)
    .await
    .map_err(AppStateError::Database)?;

    let jobmap: HashMap<String, VectorizeJob> = all_jobs
        .into_iter()
        .map(|mut item| {
            let key = std::mem::take(&mut item.job_name);
            (key, item)
        })
        .collect();

    Ok(jobmap)
}
