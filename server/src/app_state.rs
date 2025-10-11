use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;
use vectorize_core::config::Config;
use vectorize_core::types::VectorizeJob;
use vectorize_worker::WorkerHealth;

use crate::cache;

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
        let job_cache = cache::load_initial_job_cache(&db_pool)
            .await
            .map_err(|e| format!("Failed to load initial job cache: {e}"))?;
        let job_cache = Arc::new(RwLock::new(job_cache));

        // listen for job change notifications
        if let Err(e) = cache::setup_job_change_notifications(&db_pool).await {
            tracing::warn!("Failed to setup job change notifications: {e}");
        }
        Self::start_cache_sync_listener_task(&cache_pool, &job_cache).await;

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
        cache_pool: &sqlx::PgPool,
        job_cache: &Arc<RwLock<HashMap<String, VectorizeJob>>>,
    ) {
        let cache_pool_for_sync = cache_pool.clone();
        let jobmap_for_sync = job_cache.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            if let Err(e) =
                cache::start_cache_sync_listener(cache_pool_for_sync, jobmap_for_sync).await
            {
                error!("Cache synchronization error: {e}");
            }
        });
    }
}
