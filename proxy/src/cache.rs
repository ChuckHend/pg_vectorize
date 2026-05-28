use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::error;
use vectorize_core::types::VectorizeJob;

use super::protocol::{ProxyConfig, WireProxyError};

pub use vectorize_core::cache::setup_job_change_notifications;

pub async fn load_initial_job_cache(
    pool: &sqlx::PgPool,
) -> Result<HashMap<String, VectorizeJob>, WireProxyError> {
    vectorize_core::cache::load_initial_job_cache(pool)
        .await
        .map_err(WireProxyError::Database)
}

pub async fn start_cache_sync_listener(
    config: Arc<ProxyConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(60);

    loop {
        if let Err(e) =
            vectorize_core::cache::try_listen_for_changes(&config.db_pool, &config.jobmap).await
        {
            error!("Cache sync listener error: {e}. Retrying in {retry_delay:?}");
            tokio::time::sleep(retry_delay).await;
            retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
        }
    }
}
