use crate::errors::ServerError;
use actix_web::{HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
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
    dbclient: web::Data<PgPool>,
    jobmap: web::Data<Arc<RwLock<HashMap<String, VectorizeJob>>>>,
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

    let job_id = init::initialize_job(&dbclient, &payload).await?;

    // Update the job cache with the new job information
    {
        let mut job_cache = jobmap.write().await;
        job_cache.insert(payload.job_name.clone(), payload.clone());
    }

    let resp = JobResponse { id: job_id };
    Ok(HttpResponse::Ok().json(resp))
}
