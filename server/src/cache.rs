use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use vectorize_core::types::VectorizeJob;

/// Cache sync functions for job change notifications
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
    db_pool: sqlx::PgPool,
    job_cache: Arc<RwLock<HashMap<String, VectorizeJob>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut retry_delay = std::time::Duration::from_secs(1);
    let max_retry_delay = std::time::Duration::from_secs(60);

    loop {
        match try_listen_for_changes(&db_pool, &job_cache).await {
            Ok(_) => retry_delay = std::time::Duration::from_secs(1),
            Err(e) => {
                error!("Cache sync listener error: {e}. Retrying in {retry_delay:?}");
                tokio::time::sleep(retry_delay).await;
                retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
            }
        }
    }
}

async fn try_listen_for_changes(
    db_pool: &sqlx::PgPool,
    job_cache: &Arc<RwLock<HashMap<String, VectorizeJob>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut listener = sqlx::postgres::PgListener::connect_with(db_pool).await?;
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

                if let Err(e) = refresh_job_cache(db_pool, job_cache).await {
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
    db_pool: &sqlx::PgPool,
    job_cache: &Arc<RwLock<HashMap<String, VectorizeJob>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let all_jobs: Vec<VectorizeJob> = sqlx::query_as(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model FROM vectorize.job",
    )
    .fetch_all(db_pool)
    .await?;

    let jobmap: HashMap<String, VectorizeJob> = all_jobs
        .into_iter()
        .map(|mut item| {
            let key = std::mem::take(&mut item.job_name);
            (key, item)
        })
        .collect();

    {
        let mut jobmap_write = job_cache.write().await;
        *jobmap_write = jobmap;
        info!("Updated job cache with {} jobs", jobmap_write.len());
    }

    Ok(())
}

pub async fn load_initial_job_cache(
    pool: &sqlx::PgPool,
) -> Result<HashMap<String, VectorizeJob>, crate::app_state::AppStateError> {
    let all_jobs: Vec<VectorizeJob> = sqlx::query_as(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model FROM vectorize.job",
    )
    .fetch_all(pool)
    .await
    .map_err(crate::app_state::AppStateError::Database)?;

    let jobmap: HashMap<String, VectorizeJob> = all_jobs
        .into_iter()
        .map(|mut item| {
            let key = std::mem::take(&mut item.job_name);
            (key, item)
        })
        .collect();

    Ok(jobmap)
}
