use crate::errors::VectorizeError;
use crate::query;
use crate::transformers::providers::get_provider;
use crate::types::JobMessage;
use crate::types::VectorizeJob;
use sqlx::PgPool;

use uuid::Uuid;

pub async fn init_project(pool: &PgPool) -> Result<(), VectorizeError> {
    // Initialize the pgmq extension
    init_pgmq(pool).await?;

    let statements = vec![
        "CREATE EXTENSION IF NOT EXISTS vector;".to_string(),
        "SELECT pgmq.create('vectorize_jobs');".to_string(),
    ];
    for s in statements {
        sqlx::query(&s).execute(pool).await?;
    }
    init_vectorize(pool).await?;

    Ok(())
}

pub async fn get_column_datatype(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<String, VectorizeError> {
    let row: String = sqlx::query_scalar(
        "
        SELECT data_type
        FROM information_schema.columns
        WHERE
            table_schema = $1
            AND table_name = $2
            AND column_name = $3    
        ",
    )
    .bind(schema)
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        VectorizeError::NotFound(format!(
            "schema, table or column NOT FOUND for {schema}.{table}.{column}: {e}"
        ))
    })?;

    Ok(row)
}

async fn pgmq_schema_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'pgmq')",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

async fn vectorize_schema_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'vectorize')",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn init_vectorize(pool: &PgPool) -> Result<(), VectorizeError> {
    if vectorize_schema_exists(pool).await? {
        log::info!("vectorize schema already exists, skipping initialization.");
        return Ok(());
    } else {
        // these statements are critical, so we fail if they error
        let statements_nofail = vec![
            "CREATE SCHEMA IF NOT EXISTS vectorize;".to_string(),
            query::create_vectorize_table(),
            query::handle_table_update(),
            query::create_batch_texts_fn(),
        ];
        // these statements are not critical, so we log warnings and continue
        let statements_failable = vec![
            "ALTER SYSTEM SET vectorize.batch_size = 10000;".to_string(),
            "SELECT pg_reload_conf();".to_string(),
        ];
        for s in statements_nofail {
            sqlx::query(&s).execute(pool).await?;
        }
        for s in statements_failable.into_iter() {
            match sqlx::query(&s).execute(pool).await {
                Ok(_) => {}
                Err(e) => {
                    let errmsg = format!("Warning: failed to execute statement: {s}, error: {e}");
                    log::warn!("{errmsg}");
                }
            }
        }
        log::info!("Installing vectorize...")
    }
    Ok(())
}

pub async fn init_pgmq(pool: &PgPool) -> Result<(), VectorizeError> {
    // Check if the pgmq schema already exists
    if pgmq_schema_exists(pool).await? {
        log::info!("pgmq schema already exists, skipping initialization.");
        return Ok(());
    } else {
        log::info!("Installing pgmq...")
    }

    let queue = pgmq::PGMQueueExt::new_with_pool(pool.clone()).await;
    queue.install_sql(None).await?;
    Ok(())
}

pub async fn initialize_job(
    pool: &PgPool,
    job_request: &VectorizeJob,
) -> Result<Uuid, VectorizeError> {
    // create the job record
    let mut tx = pool.begin().await?;
    let job_id: Uuid = sqlx::query_scalar("
        INSERT INTO vectorize.job (job_name, src_schema, src_table, src_columns, primary_key, update_time_col, model)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (job_name) DO UPDATE SET
            src_schema = EXCLUDED.src_schema,
            src_table = EXCLUDED.src_table,
            src_columns = EXCLUDED.src_columns,
            primary_key = EXCLUDED.primary_key,
            update_time_col = EXCLUDED.update_time_col,
            model = EXCLUDED.model
        RETURNING id")
        .bind(job_request.job_name.clone())
        .bind(job_request.src_schema.clone())
        .bind(job_request.src_table.clone())
        .bind(job_request.src_columns.clone())
        .bind(job_request.primary_key.clone())
        .bind(job_request.update_time_col.clone())
        .bind(job_request.model.to_string())
        .fetch_one(&mut *tx)
        .await?;

    // get model dimension
    let provider = get_provider(&job_request.model.source, None, None, None)?;
    let model_dim = provider.model_dim(&job_request.model.api_name()).await?;

    let pkey_dtype = get_column_datatype(
        pool,
        &job_request.src_schema,
        &job_request.src_table,
        &job_request.primary_key,
    )
    .await?;

    // create embeddings table and views
    let col_type = format!("vector({model_dim})");
    let create_embedding_table_query = query::create_embedding_table(
        job_request.job_name.as_str(),
        &job_request.primary_key,
        &pkey_dtype,
        &col_type,
        &job_request.src_schema,
        &job_request.src_table,
    );

    // create search tokens table
    let create_search_tokens_table_query = query::create_search_tokens_table(
        job_request.job_name.as_str(),
        &job_request.primary_key,
        &pkey_dtype,
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
    let embedding_index_query = query::create_hnsw_cosine_index(
        &job_request.job_name,
        "vectorize",
        &embeddings_table,
        "embeddings",
    );

    let fts_index_query = query::create_fts_index_query(&job_request.job_name, "GIN");

    sqlx::query(&create_embedding_table_query)
        .execute(&mut *tx)
        .await?;
    sqlx::query(&create_search_tokens_table_query)
        .execute(&mut *tx)
        .await?;
    sqlx::query(&view_query).execute(&mut *tx).await?;
    sqlx::query(&embedding_index_query)
        .execute(&mut *tx)
        .await?;
    sqlx::query(&fts_index_query).execute(&mut *tx).await?;

    // create triggers on the source table
    let trigger_handler =
        query::create_trigger_handler(&job_request.job_name, &job_request.primary_key);
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
    let search_token_trigger_queries = query::update_search_tokens_trigger_queries(
        &job_request.job_name,
        &job_request.primary_key,
        &job_request.src_schema,
        &job_request.src_table,
        &job_request.src_columns,
    );
    for q in search_token_trigger_queries {
        sqlx::query(&q).execute(&mut *tx).await?;
    }
    sqlx::query(&trigger_handler).execute(&mut *tx).await?;
    sqlx::query(&insert_trigger).execute(&mut *tx).await?;
    sqlx::query(&update_trigger).execute(&mut *tx).await?;
    tx.commit().await?;

    // finally, enqueue pgmq job
    // previous tx needs to be committed before we can enqueue the job
    scan_job(pool, job_request).await?;

    let search_cols = job_request
        .src_columns
        .iter()
        .map(|col| format!("COALESCE({col}, '')"))
        .collect::<Vec<String>>()
        .join(" || ' ' || ");
    let initial_update_query = format!(
        "
        INSERT INTO vectorize._search_tokens_{job_name} ({join_key}, search_tokens)
        SELECT 
            {join_key}, 
            to_tsvector('english', {search_cols})
        FROM {src_schema}.{src_table}
        ON CONFLICT ({join_key}) DO UPDATE SET
            search_tokens = EXCLUDED.search_tokens,
            updated_at = NOW();
    ",
        src_schema = job_request.src_schema,
        src_table = job_request.src_table,
        join_key = job_request.primary_key,
        job_name = job_request.job_name
    );
    sqlx::query(&initial_update_query).execute(pool).await?;

    Ok(job_id)
}

// enqueues jobs where records need embeddings computed
pub async fn scan_job(pool: &PgPool, job_request: &VectorizeJob) -> Result<(), VectorizeError> {
    let rows_for_update_query = query::new_rows_query_join(
        &job_request.job_name,
        &job_request.src_columns,
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

pub async fn cleanup_job(pool: &PgPool, job_name: &str) -> Result<(), VectorizeError> {
    // First, fetch the job details to get src_schema and src_table
    let job = crate::db::get_vectorize_job(pool, job_name)
        .await
        .map_err(|e| match e {
            VectorizeError::SqlError(sqlx::Error::RowNotFound) => {
                VectorizeError::NotFound(format!("Job '{}' not found", job_name))
            }
            _ => e,
        })?;

    log::info!("Cleaning up job: {}", job_name);

    // Delete pending PGMQ messages for this job
    // We search for messages where the job_name matches
    let delete_messages_query =
        "DELETE FROM pgmq.vectorize_jobs WHERE message->>'job_name' = $1".to_string();
    match sqlx::query(&delete_messages_query)
        .bind(job_name)
        .execute(pool)
        .await
    {
        Ok(result) => {
            log::info!(
                "Deleted {} pending PGMQ messages for job: {}",
                result.rows_affected(),
                job_name
            );
        }
        Err(e) => {
            log::warn!("Failed to delete PGMQ messages for job {}: {}", job_name, e);
            // Continue with cleanup even if PGMQ deletion fails
        }
    }

    // Begin transaction for database resource cleanup
    let mut tx = pool.begin().await?;

    // Generate cleanup SQL statements
    let cleanup_statements = [
        // Drop triggers first (they depend on the function and table)
        query::drop_event_trigger(job_name, &job.src_schema, &job.src_table, "INSERT"),
        query::drop_event_trigger(job_name, &job.src_schema, &job.src_table, "UPDATE"),
        query::drop_search_tokens_trigger(job_name, &job.src_schema, &job.src_table),
        // Drop trigger handler function
        query::drop_trigger_handler(job_name),
        // Drop view (depends on tables)
        query::drop_project_view(job_name),
        // Drop tables (CASCADE will handle indexes)
        query::drop_embeddings_table(job_name),
        query::drop_search_tokens_table(job_name),
        // Delete job record
        query::delete_job_record(job_name),
    ];

    // Execute cleanup statements
    for (idx, statement) in cleanup_statements.iter().enumerate() {
        match sqlx::query(statement).execute(&mut *tx).await {
            Ok(_) => {
                log::debug!("Executed cleanup statement {}: {}", idx + 1, statement);
            }
            Err(e) => {
                log::warn!(
                    "Warning: cleanup statement {} failed (continuing): {} - Error: {}",
                    idx + 1,
                    statement,
                    e
                );
                // Continue with other cleanup steps even if one fails
            }
        }
    }

    // Commit transaction
    tx.commit().await?;

    log::info!("Successfully cleaned up job: {}", job_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ignore]
    #[tokio::test]
    async fn test_init_pgmq() {
        env_logger::init();
        let conn_string = "postgresql://postgres:postgres@localhost:5432/postgres";
        let pool = PgPool::connect(conn_string).await.unwrap();
        init_pgmq(&pool).await.unwrap();
    }
}
