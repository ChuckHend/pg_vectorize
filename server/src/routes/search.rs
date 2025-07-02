use crate::errors::ServerError;
use actix_web::{HttpResponse, get, web};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, prelude::FromRow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
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
    pub filters: HashMap<String, query::FilterValue>,
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
    pool: web::Data<PgPool>,
    jobmap: web::Data<Arc<RwLock<HashMap<String, VectorizeJob>>>>,
    payload: web::Query<SearchRequest>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();
    query::check_input(&payload.job_name)?;

    // check the filters are valid if they exist and create a SQL string for them
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

    // Try to get job info from cache first, fallback to database
    let vectorizejob = {
        let job_cache = jobmap.read().await;
        if let Some(job_info) = job_cache.get(&payload.job_name) {
            job_info.clone()
        } else {
            // cache miss is going to either be an invalid job name, or there is an issue with the cache
            log::warn!(
                "Job not found in cache, querying database for job: {}",
                payload.job_name
            );
            vectorize_core::db::get_vectorize_job(&pool, &payload.job_name).await?
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

    log::info!("Query: {}", q);

    let mut prepared_query = sqlx::query(&q)
        .bind(&embeddings.embeddings[0])
        .bind(&payload.query);

    // bind filter values in the same order they were processed in hybrid_search_query
    for (_column, value) in &payload.filters {
        prepared_query = value.bind_to_query(prepared_query);
    }

    let results = prepared_query.fetch_all(&**pool).await?;

    let json_results: Vec<serde_json::Value> = results
        .iter()
        .map(|row| row.get::<serde_json::Value, _>("results"))
        .collect();

    Ok(HttpResponse::Ok().json(json_results))
}
