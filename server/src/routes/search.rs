use crate::app_state::AppState;
use crate::errors::ServerError;
use actix_web::{HttpResponse, get, web};
use serde::{Deserialize, Serialize};
use sqlx::{Row, prelude::FromRow};
use std::collections::BTreeMap;

use utoipa::ToSchema;
use uuid::Uuid;
use vectorize_core::query;
use vectorize_core::transformers::providers::prepare_generic_embedding_request;
use vectorize_core::transformers::types::Inputs;
use vectorize_core::types::VectorizeJob;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, FromRow)]
pub struct SearchRequest {
    pub job_name: String,
    pub query: String,
    #[serde(default = "default_window_size")]
    pub window_size: i32,
    #[serde(default = "default_limit")]
    pub limit: i32,
    #[serde(default = "default_rrf_k")]
    pub rrf_k: f32,
    #[serde(default = "default_semantic_wt")]
    pub semantic_wt: f32,
    #[serde(default = "default_fts_wt")]
    pub fts_wt: f32,
    #[serde(flatten, default)]
    pub filters: BTreeMap<String, query::FilterValue>,
}

fn default_semantic_wt() -> f32 {
    1.0
}

fn default_fts_wt() -> f32 {
    1.0
}

fn default_limit() -> i32 {
    10
}

fn default_window_size() -> i32 {
    5 * default_limit()
}

fn default_rrf_k() -> f32 {
    60.0
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SearchResponse {
    pub id: Uuid,
}

#[utoipa::path(
    context_path = "/api/v1",
    params(
        ("job_name" = String, Query, description = "Name of the vectorize job"),
        ("query" = String, Query, description = "Search query string"),
        ("limit" = Option<i64>, Query, description = "Optional limit on the number of results"),
        ("window_size" = Option<i64>, Query, description = "Optional window size (inner limits) for hybrid search"),
        ("rrf_k" = Option<i64>, Query, description = "Optional RRF k parameter for hybrid search"),
        ("semantic_wt" = Option<f32>, Query, description = "Optional weight for semantic search (default: 1.0)"),
        ("fts_wt" = Option<f32>, Query, description = "Optional weight for full-text search (default: 1.0)"),
        ("filters" = Option<HashMap<String, String>>, Query, description = "Optional filters for the search"),
    ),
    responses(
        (
            status = 200, description = "Search results",
            body = Vec<serde_json::Value>,
        ),
    ),
)]
#[get("/search")]
pub async fn search(
    app_state: web::Data<AppState>,
    payload: web::Query<SearchRequest>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();

    // check inputs and filters are valid if they exist and create a SQL string for them
    query::check_input(&payload.job_name)?;
    if !payload.filters.is_empty() {
        for (key, value) in &payload.filters {
            // validate key and value
            query::check_input(key)?;
            if let query::FilterValue::String(value) = value {
                // only need to check the value if it is a raw string
                query::check_input(value)?;
            }
        }
    }

    // Try to get job info from cache first, fallback to database with write-through on miss
    let vectorizejob = {
        if let Some(job_info) = {
            let job_cache = app_state.job_cache.read().await;
            job_cache.get(&payload.job_name).cloned()
        } {
            job_info
        } else {
            tracing::warn!(
                "Job not found in cache, querying database for job: {}",
                payload.job_name
            );
            let job = get_vectorize_job(&app_state.db_pool, &payload.job_name).await?;
            let mut job_cache = app_state.job_cache.write().await;
            job_cache.insert(payload.job_name.clone(), job.clone());
            job
        }
    };

    let provider = vectorize_core::transformers::providers::get_provider(
        &vectorizejob.model.source,
        None,
        None,
        None,
    )?;

    let input = Inputs {
        record_id: "".to_string(),
        inputs: payload.query.clone(),
        token_estimate: 0,
    };

    let embedding_request = prepare_generic_embedding_request(&vectorizejob.model, &[input]);
    let embeddings = provider.generate_embedding(&embedding_request).await?;

    let q = query::hybrid_search_query(
        &payload.job_name,
        &vectorizejob.src_schema,
        &vectorizejob.src_table,
        &vectorizejob.primary_key,
        &["*".to_string()],
        payload.window_size,
        payload.limit,
        payload.rrf_k,
        payload.semantic_wt,
        payload.fts_wt,
        &payload.filters,
    );

    let mut prepared_query = sqlx::query(&q)
        .bind(&embeddings.embeddings[0])
        .bind(&payload.query);

    // Bind filter values using the same BTreeMap instance used by the query builder
    for value in payload.filters.values() {
        prepared_query = value.bind_to_query(prepared_query);
    }

    let results = prepared_query.fetch_all(&app_state.db_pool).await?;

    let json_results: Vec<serde_json::Value> = results
        .iter()
        .map(|row| row.get::<serde_json::Value, _>("results"))
        .collect();

    Ok(HttpResponse::Ok().json(json_results))
}

async fn get_vectorize_job(
    pool: &sqlx::PgPool,
    job_name: &str,
) -> Result<VectorizeJob, ServerError> {
    // Changed return type
    match sqlx::query(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model 
         FROM vectorize.job 
         WHERE job_name = $1",
    )
    .bind(job_name)
    .fetch_optional(pool)
    .await?
    {
        Some(row) => Ok(VectorizeJob::from_row(&row)?),
        None => Err(ServerError::NotFoundError(format!(
            "Job not found: {}",
            job_name
        ))),
    }
}
