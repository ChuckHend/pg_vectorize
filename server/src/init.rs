use crate::errors::ServerError;
use crate::routes::table::VectorizeJob;
use sqlx::PgPool;
use std::process::Command;
use uuid::Uuid;
use vectorize_core::query;
use vectorize_core::transformers::providers::get_provider;
use vectorize_core::types::JobMessage;

pub async fn init_project(pool: &PgPool, conn_string: Option<&str>) -> Result<(), ServerError> {
    // Initialize the pgmq extension
    init_pgmq(pool, conn_string).await?;

    let statements = vec![
        "CREATE SCHEMA IF NOT EXISTS vectorize;".to_string(),
        "CREATE EXTENSION IF NOT EXISTS vector;".to_string(),
        query::create_vectorize_table(),
        "SELECT pgmq.create('vectorize_jobs');".to_string(),
    ];
    for s in statements {
        sqlx::query(&s).execute(pool).await?;
    }

    Ok(())
}

pub async fn get_column_datatype(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<String, ServerError> {
    sqlx::query_scalar!(
        "
        SELECT data_type
        FROM information_schema.columns
        WHERE
            table_schema = $1
            AND table_name = $2
            AND column_name = $3    
        ",
        schema,
        table,
        column,
    )
    .fetch_one(pool)
    .await?
    .ok_or_else(|| {
        ServerError::NotFoundError(format!(
            "Column {} not found in table {}.{}",
            column, schema, table
        ))
    })
}

async fn pgmq_schema_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'pgmq')",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn init_pgmq(pool: &PgPool, conn_string: Option<&str>) -> Result<(), ServerError> {
    // Check if the pgmq schema already exists
    if pgmq_schema_exists(pool).await? {
        log::info!("pgmq schema already exists, skipping initialization.");
        return Ok(());
    }

    // URL to the raw SQL file
    let sql_url = "https://raw.githubusercontent.com/pgmq/pgmq/main/pgmq-extension/sql/pgmq.sql";

    let client = reqwest::Client::new();
    let response = client.get(sql_url).send().await?;
    let sql_content = response.text().await?;

    if let Some(url) = conn_string {
        let output = Command::new("psql")
            .arg(url)
            .arg("-c")
            .arg(sql_content)
            .output()
            .unwrap();
        log::info!("{}", String::from_utf8_lossy(&output.stdout));
    }

    Ok(())
}

pub async fn initialize_job(
    pool: &PgPool,
    job_request: &VectorizeJob,
) -> Result<Uuid, ServerError> {
    // create the job record
    let mut tx = pool.begin().await?;
    let job_id: Uuid = sqlx::query_scalar("
        INSERT INTO vectorize.job (job_name, src_schema, src_table, src_column, primary_key, update_time_col, model)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id")
        .bind(job_request.job_name.clone())
        .bind(job_request.src_schema.clone())
        .bind(job_request.src_table.clone())
        .bind(job_request.src_column.clone())
        .bind(job_request.primary_key.clone())
        .bind(job_request.update_time_col.clone())
        .bind(job_request.model.to_string())
        .fetch_one(&mut *tx)
        .await?;

    // get model dimension
    let provider = get_provider(&job_request.model.source, None, None, None)?;
    let model_dim = provider.model_dim(&job_request.model.api_name()).await?;

    // create embeddings table and views
    let col_type = format!("vector({})", model_dim);
    let create_query = query::create_embedding_table(
        job_request.job_name.as_str(),
        &job_request.primary_key,
        &get_column_datatype(
            pool,
            &job_request.src_schema,
            &job_request.src_table,
            &job_request.primary_key,
        )
        .await?,
        &col_type,
        &job_request.src_schema,
        &job_request.src_table,
    );
    let view_query = query::create_project_view(
        &job_request.job_name,
        job_request.src_schema.as_str(),
        job_request.src_table.as_str(),
        &job_request.primary_key,
    );

    let embeddings_table = format!("_embeddings_{}", job_request.job_name);
    let index_query = query::create_hnsw_cosine_index(
        &job_request.job_name,
        "vectorize",
        &embeddings_table,
        "embeddings",
    );
    sqlx::query(&create_query).execute(&mut *tx).await?;
    sqlx::query(&view_query).execute(&mut *tx).await?;
    sqlx::query(&index_query).execute(&mut *tx).await?;

    // create triggers on the source table
    let trigger_handler =
        query::create_trigger_handler(&job_request.job_name, &job_request.job_name);
    let insert_trigger = query::create_event_trigger(
        &job_request.job_name,
        &job_request.src_schema,
        &job_request.src_table,
        "INSERT",
    );
    let update_trigger = query::create_event_trigger(
        &job_request.job_name,
        &job_request.src_schema,
        &job_request.src_table,
        "UPDATE",
    );
    sqlx::query(&trigger_handler).execute(&mut *tx).await?;
    sqlx::query(&insert_trigger).execute(&mut *tx).await?;
    sqlx::query(&update_trigger).execute(&mut *tx).await?;
    tx.commit().await?;

    // finally, enqueue pgmq job
    // previous tx needs to be committed before we can enqueue the job
    scan_job(pool, job_request).await?;

    Ok(job_id)
}

// enqueues jobs where records need embeddings computed
pub async fn scan_job(pool: &PgPool, job_request: &VectorizeJob) -> Result<(), ServerError> {
    let rows_for_update_query = query::new_rows_query_join(
        &job_request.job_name,
        &[job_request.src_column.clone()],
        &job_request.src_schema,
        &job_request.src_table,
        &job_request.primary_key,
        Some(job_request.update_time_col.clone()),
    );

    let new_or_updated_rows = query::get_new_updates(pool, &rows_for_update_query).await?;

    match new_or_updated_rows {
        Some(rows) => {
            let batches = query::create_batches(rows, 10000);
            for b in batches {
                let record_ids = b.iter().map(|i| i.record_id.clone()).collect::<Vec<_>>();

                let msg = JobMessage {
                    job_name: job_request.job_name.clone(),
                    record_ids,
                };
                let msg_id: i64 = sqlx::query_scalar(
                    "SELECT * FROM pgmq.send(queue_name=>'vectorize_jobs', msg=>$1)",
                )
                .bind(serde_json::to_value(msg)?)
                .fetch_one(pool)
                .await?;
                log::info!(
                    "enqueued job_name: {}, msg_id: {}",
                    job_request.job_name,
                    msg_id,
                );
            }
        }
        None => {
            log::warn!(
                "No new or updated rows found for job: {}",
                job_request.job_name
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_pgmq() {
        env_logger::init();
        let conn_string = "postgresql://postgres:postgres@localhost:5432/postgres";
        let pool = PgPool::connect(conn_string).await.unwrap();
        init_pgmq(&pool, Some(conn_string)).await.unwrap();
    }
}
