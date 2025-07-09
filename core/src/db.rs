use crate::{errors::VectorizeError, types::VectorizeJob};
use sqlx::{FromRow, PgPool};

pub async fn get_vectorize_job(
    pool: &PgPool,
    job_name: &str,
) -> Result<VectorizeJob, VectorizeError> {
    // Changed return type
    let row = sqlx::query(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model 
         FROM vectorize.job 
         WHERE job_name = $1",
    )
    .bind(job_name)
    .fetch_one(pool)
    .await?;

    Ok(VectorizeJob::from_row(&row)?) // Handle the Result from from_row
}
