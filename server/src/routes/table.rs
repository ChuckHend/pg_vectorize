use crate::errors::ServerError;
use crate::init;
use crate::init::get_column_datatype;
use actix_web::{HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, prelude::FromRow};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::core::types::{Model, model_to_string, string_to_model};

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, FromRow)]
pub struct VectorizeJob {
    pub job_name: String,
    pub src_table: String,
    pub src_schema: String,
    pub src_column: String,
    pub primary_key: String,
    pub update_time_col: String,
    #[serde(
        deserialize_with = "string_to_model",
        serialize_with = "model_to_string"
    )]
    pub model: Model,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct JobResponse {
    pub id: Uuid,
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
    dbclient: web::Data<PgPool>,
    payload: web::Json<VectorizeJob>,
) -> Result<HttpResponse, ServerError> {
    let payload = payload.into_inner();

    // validate update_time_col is timestamptz
    let datatype = get_column_datatype(
        &dbclient,
        &payload.src_schema,
        &payload.src_table,
        &payload.update_time_col,
    )
    .await?;
    if datatype != "timestamp with time zone" {
        return Err(ServerError::InvalidRequest(format!(
            "Column {} in table {}.{} must be of type 'timestamp with time zone'",
            payload.update_time_col, payload.src_schema, payload.src_table
        )));
    }

    let job_id = init::initialize_job(&dbclient, &payload).await?;
    let resp = JobResponse { id: job_id };
    Ok(HttpResponse::Ok().json(resp))
}
