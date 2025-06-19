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
    #[serde(default = "default_limit")]
    pub limit: i32,
}

fn default_limit() -> i32 {
    100
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
        inputs: payload.query,
        token_estimate: 0,
    };

    let embedding_request = prepare_generic_embedding_request(&vectorizejob.model, &[input]);
    let embeddings = provider.generate_embedding(&embedding_request).await?;

    let q = query::join_table_cosine_similarity(
        &payload.job_name,
        &vectorizejob.src_schema,
        &vectorizejob.src_table,
        &vectorizejob.primary_key,
        &["*".to_string()],
        payload.limit,
        None,
    );
    let results = sqlx::query(&q)
        .bind(&embeddings.embeddings[0])
        .fetch_all(&**pool)
        .await?;

    let json_results: Vec<serde_json::Value> = results
        .iter()
        .map(|row| row.get::<serde_json::Value, _>("results"))
        .collect();

    Ok(HttpResponse::Ok().json(json_results))
}
