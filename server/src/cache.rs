use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;
use vectorize_core::types::VectorizeJob;

pub use vectorize_core::cache::{refresh_job_cache, setup_job_change_notifications};

pub async fn load_initial_job_cache(
    pool: &sqlx::PgPool,
) -> Result<HashMap<String, VectorizeJob>, crate::app_state::AppStateError> {
    vectorize_core::cache::load_initial_job_cache(pool)
        .await
        .map_err(crate::app_state::AppStateError::Database)
}

pub async fn start_cache_sync_listener(
    db_pool: sqlx::PgPool,
    job_cache: Arc<RwLock<HashMap<String, VectorizeJob>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut retry_delay = std::time::Duration::from_secs(1);
    let max_retry_delay = std::time::Duration::from_secs(60);

    loop {
        if let Err(e) = vectorize_core::cache::try_listen_for_changes(&db_pool, &job_cache).await {
            error!("Cache sync listener error: {e}. Retrying in {retry_delay:?}");
            tokio::time::sleep(retry_delay).await;
            retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
        }
    }
}
