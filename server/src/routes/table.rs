use crate::errors::ServerError;
use crate::init::{get_column_datatype, init_pgmq};
use actix_web::{HttpRequest, HttpResponse, get, post, web};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use vectorize_core::query;
use vectorize_core::transformers::providers::{EmbeddingProvider, get_provider};
use vectorize_core::types::{IndexDist, JobParams, Model, ModelSource, TableMethod};

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct VectorizeJob {
    job_name: String,
    table: String,
    schema: String,
    column: String,
    primary_key: String,
    update_time_col: String,
    model: Model,
}

#[utoipa::path(
    context_path = "/api/v1",
    responses(
        (
            status = 200, description = "Initialize a vectorize job",
            body = Option<Vec<Conversation>>,
        ),
    ),
)]
#[post("/table")]
pub async fn table(
    req: HttpRequest,
    dbclient: web::Data<PgPool>,
    payload: web::Json<VectorizeJob>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();

    // validate update_time_col is timestamptz
    let datatype = get_column_datatype(
        &dbclient,
        &payload.schema,
        &payload.table,
        &payload.update_time_col,
    )
    .await?;
    if datatype != "timestamp with time zone" {
        return Err(ServerError::InvalidRequest(format!(
            "Column {} in table {}.{} must be of type 'timestamp with time zone'",
            payload.update_time_col, payload.schema, payload.table
        )));
    }

    let pkey_type = get_column_datatype(
        &dbclient,
        &payload.schema,
        &payload.table,
        &payload.primary_key,
    )
    .await?;

    // TODO: move this to webserver startup along with vectorize.table
    init_pgmq(&dbclient).await?;

    let provider = get_provider(&payload.model.source, None, None, None)?;

    // get model dimension
    let model_dim = provider.model_dim(&payload.model.api_name()).await?;

    let init_job_q = query::init_job_query();
    let valid_params = JobParams {
        schema: payload.schema,
        relation: payload.table,
        columns: vec![payload.column.clone()],
        update_time_col: Some(payload.update_time_col),
        table_method: TableMethod::join,
        primary_key: payload.primary_key,
        pkey_type,
        api_key: None,
        schedule: "realtime".to_string(),
        args: None,
    };
    sqlx::query(init_job_q.as_str())
        .bind(payload.job_name)
        .bind(IndexDist::pgv_hnsw_cosine.to_string())
        .bind(payload.model.to_string())
        .bind(serde_json::to_value(&valid_params)?)
        .execute(dbclient.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json("response"))
}
