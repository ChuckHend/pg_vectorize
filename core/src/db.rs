use crate::{errors::VectorizeError, types::VectorizeJob};
use sqlx::{FromRow, PgPool};

pub async fn get_vectorize_job(
    pool: &PgPool,
    job_name: &str,
) -> Result<Option<VectorizeJob>, VectorizeError> {
    // Changed return type to Option<VectorizeJob>
    let row = sqlx::query(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model 
         FROM vectorize.job 
         WHERE job_name = $1",
    )
    .bind(job_name)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => Ok(Some(VectorizeJob::from_row(&row)?)),
        None => Ok(None),
    }
}
