use crate::core::query;
use crate::core::transformers::providers::prepare_generic_embedding_request;
use crate::{core::transformers::types::Inputs, errors::ServerError};
use actix_web::{HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, prelude::FromRow};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, FromRow)]
pub struct SearchRequest {
    pub job_name: String,
    pub query: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct SearchResponse {
    pub id: Uuid,
}

#[utoipa::path(
    context_path = "/api/v1",
    responses(
        (
            status = 200, description = "Search",
            body = SearchResponse,
        ),
    ),
)]
#[post("/search")]
pub async fn table(
    pool: web::Data<PgPool>,
    payload: web::Json<SearchRequest>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();

    let vectorizejob = crate::db::get_vectorize_job(&pool, &payload.job_name).await?;

    let provider = crate::core::transformers::providers::get_provider(
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
        3,
        None,
    );
    let results = sqlx::query(&q)
        .bind(&embeddings.embeddings[0])
        .fetch_all(&**pool)
        .await?;

    let json_results: Vec<serde_json::Value> = results
        .iter()
        .map(|row| row.get::<serde_json::Value, _>("row"))
        .collect();

    Ok(HttpResponse::Ok().json(json_results))
}
