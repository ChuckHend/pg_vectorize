use std::sync::Arc;

use crate::app_state::AppState;
use crate::bm25::BM25Index;
use crate::errors::ServerError;
use actix_web::{HttpResponse, delete, post, web};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use utoipa::ToSchema;
use uuid::Uuid;
use vectorize_core::init::{self, get_column_datatype};

use vectorize_core::types::VectorizeJob;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct JobResponse {
    pub id: Uuid,
}

#[utoipa::path(
    context_path = "/api/v1",
    responses(
        (
            status = 200, description = "Initialize a vectorize job",
            body = JobResponse,
        ),
    ),
)]
#[post("/table")]
pub async fn table(
    app_state: web::Data<AppState>,
    payload: web::Json<VectorizeJob>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();

    // validate update_time_col is timestamptz
    let datatype = get_column_datatype(
        &app_state.db_pool,
        &payload.src_schema,
        &payload.src_table,
        &payload.update_time_col,
    )
    .await
    .map_err(|e| match e {
        vectorize_core::errors::VectorizeError::NotFound(msg) => ServerError::NotFoundError(msg),
        _ => ServerError::from(e),
    })?;
    if datatype != "timestamp with time zone" {
        return Err(ServerError::InvalidRequest(format!(
            "Column {} in table {}.{} must be of type 'timestamp with time zone'",
            payload.update_time_col, payload.src_schema, payload.src_table
        )));
    }

    let job_id = init::initialize_job(&app_state.db_pool, &payload).await?;

    // Update the job cache with the new job information
    {
        let mut job_cache = app_state.job_cache.write().await;
        job_cache.insert(payload.job_name.clone(), payload.clone());
    }

    // BM25 indexing is opt-in per job via `bm25_enabled`.
    if payload.bm25_enabled {
        // Create a BM25 index for this job and populate it in the background.
        match BM25Index::new() {
            Ok(idx) => {
                let idx = Arc::new(Mutex::new(idx));
                app_state
                    .bm25_indexes
                    .write()
                    .await
                    .insert(payload.job_name.clone(), idx.clone());
                let pool = app_state.db_pool.clone();
                let job = payload.clone();
                tokio::spawn(async move {
                    crate::bm25::populate_bm25_index(&pool, &job, idx).await;
                });
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to create BM25 index for job {}: {e}",
                    payload.job_name
                );
            }
        }
    } else {
        // Job was re-created/updated with BM25 disabled; drop any stale index
        // so it stops consuming memory and the background sync loop skips it.
        app_state
            .bm25_indexes
            .write()
            .await
            .remove(&payload.job_name);
    }

    let resp = JobResponse { id: job_id };
    Ok(HttpResponse::Ok().json(resp))
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeleteJobResponse {
    pub job_name: String,
    pub message: String,
}

#[utoipa::path(
    context_path = "/api/v1",
    responses(
        (
            status = 200, description = "Successfully deleted vectorize job",
            body = DeleteJobResponse,
        ),
        (
            status = 404, description = "Job not found",
        ),
    ),
)]
#[delete("/table/{job_name}")]
pub async fn delete_table(
    app_state: web::Data<AppState>,
    job_name: web::Path<String>,
) -> Result<HttpResponse, ServerError> {
    let job_name = job_name.into_inner();

    // Cleanup the job resources
    init::cleanup_job(&app_state.db_pool, &job_name)
        .await
        .map_err(|e| match e {
            vectorize_core::errors::VectorizeError::NotFound(msg) => {
                ServerError::NotFoundError(msg)
            }
            _ => ServerError::from(e),
        })?;

    // Remove from job cache and BM25 index map
    {
        let mut job_cache = app_state.job_cache.write().await;
        job_cache.remove(&job_name);
    }
    {
        let mut indexes = app_state.bm25_indexes.write().await;
        indexes.remove(&job_name);
    }

    let resp = DeleteJobResponse {
        job_name: job_name.clone(),
        message: format!("Successfully deleted job '{}'", job_name),
    };
    Ok(HttpResponse::Ok().json(resp))
}
