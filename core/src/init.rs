use crate::errors::VectorizeError;
use crate::query;
use crate::transformers::providers::get_provider;
use crate::types::JobMessage;
use crate::types::{JobUpdate, MAX_BATCH_SIZE, VectorizeJob};
use sqlx::{FromRow, PgConnection, PgPool};

use uuid::Uuid;

/// Rejects any identifier that is not strictly alphanumeric/underscore.
/// These values are interpolated directly into DDL and PL/pgSQL, so they must be validated
/// before any SQL is built from them.
fn validate_job_identifiers(job_request: &VectorizeJob) -> Result<(), VectorizeError> {
    let identifiers = [
        ("job_name", &job_request.job_name),
        ("src_schema", &job_request.src_schema),
        ("src_table", &job_request.src_table),
        ("primary_key", &job_request.primary_key),
        ("update_time_col", &job_request.update_time_col),
    ]
    .into_iter()
    .chain(
        job_request
            .src_columns
            .iter()
            .map(|col| ("src_columns", col)),
    );
    for (field, value) in identifiers {
        if value.is_empty() || query::check_input(value).is_err() {
            return Err(VectorizeError::InvalidInput(format!(
                "{field} must contain only alphanumeric characters or underscores, got: '{value}'"
            )));
        }
    }
    if job_request.src_columns.is_empty() {
        return Err(VectorizeError::InvalidInput(
            "src_columns must not be empty".to_string(),
        ));
    }
    validate_batch_size(job_request.batch_size)
}

fn validate_batch_size(batch_size: i32) -> Result<(), VectorizeError> {
    if !(1..=MAX_BATCH_SIZE).contains(&batch_size) {
        return Err(VectorizeError::InvalidInput(format!(
            "batch_size must be between 1 and {MAX_BATCH_SIZE}, got: {batch_size}"
        )));
    }
    Ok(())
}

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
        // Note: the vectorize.job table itself is created/evolved via the
        // vectorize-server crate's sqlx migrations (server/migrations), not here.
        let statements_nofail = vec![
            "CREATE SCHEMA IF NOT EXISTS vectorize;".to_string(),
            query::handle_table_update(),
            query::create_batch_texts_fn(),
        ];
        for s in statements_nofail {
            sqlx::query(&s).execute(pool).await?;
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
    validate_job_identifiers(job_request)?;

    // create the job record
    // everything below runs in this single transaction so a failure at any step
    // leaves no job record, tables, triggers or queued messages behind
    let mut tx = pool.begin().await?;
    let job_id: Uuid = sqlx::query_scalar("
        INSERT INTO vectorize.job (job_name, src_schema, src_table, src_columns, primary_key, update_time_col, model, bm25_enabled, batch_size)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (job_name) DO UPDATE SET
            src_schema = EXCLUDED.src_schema,
            src_table = EXCLUDED.src_table,
            src_columns = EXCLUDED.src_columns,
            primary_key = EXCLUDED.primary_key,
            update_time_col = EXCLUDED.update_time_col,
            model = EXCLUDED.model,
            bm25_enabled = EXCLUDED.bm25_enabled,
            batch_size = EXCLUDED.batch_size
        RETURNING id")
        .bind(job_request.job_name.clone())
        .bind(job_request.src_schema.clone())
        .bind(job_request.src_table.clone())
        .bind(job_request.src_columns.clone())
        .bind(job_request.primary_key.clone())
        .bind(job_request.update_time_col.clone())
        .bind(job_request.model.to_string())
        .bind(job_request.bm25_enabled)
        .bind(job_request.batch_size)
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
    let trigger_handler = query::create_trigger_handler_with_batch_size(
        &job_request.job_name,
        &job_request.primary_key,
        job_request.batch_size,
    );
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

    // enqueue pgmq jobs; messages only become visible to workers once the tx commits,
    // at which point the embeddings table they write to already exists
    scan_job(&mut tx, job_request).await?;

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
    sqlx::query(&initial_update_query).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(job_id)
}

// enqueues jobs where records need embeddings computed
pub async fn scan_job(
    conn: &mut PgConnection,
    job_request: &VectorizeJob,
) -> Result<(), VectorizeError> {
    let rows_for_update_query = query::new_rows_query_join(
        &job_request.job_name,
        &job_request.src_columns,
        &job_request.src_schema,
        &job_request.src_table,
        &job_request.primary_key,
        Some(job_request.update_time_col.clone()),
    );

    let new_or_updated_rows = query::get_new_updates(&mut *conn, &rows_for_update_query).await?;

    match new_or_updated_rows {
        Some(rows) => {
            // cap each message at ~10k tokens and at the job's batch_size rows
            let batches = query::create_batches(rows, 10000);
            let batch_size = job_request.batch_size as usize;
            for b in batches.iter().flat_map(|b| b.chunks(batch_size)) {
                let record_ids = b.iter().map(|i| i.record_id.clone()).collect::<Vec<_>>();

                let msg = JobMessage {
                    job_name: job_request.job_name.clone(),
                    record_ids,
                };
                let msg_id: i64 = sqlx::query_scalar(
                    "SELECT * FROM pgmq.send(queue_name=>'vectorize_jobs', msg=>$1)",
                )
                .bind(serde_json::to_value(msg)?)
                .fetch_one(&mut *conn)
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

/// Applies the settings that can change on an existing job and returns the updated job.
/// A new batch_size is written into the job's trigger function, so it applies to rows
/// written after this commits; messages already queued keep their size.
pub async fn update_job(
    pool: &PgPool,
    job_name: &str,
    update: &JobUpdate,
) -> Result<VectorizeJob, VectorizeError> {
    if query::check_input(job_name).is_err() {
        return Err(VectorizeError::InvalidInput(format!(
            "job_name must contain only alphanumeric characters or underscores, got: '{job_name}'"
        )));
    }
    if update.batch_size.is_none() && update.bm25_enabled.is_none() {
        return Err(VectorizeError::InvalidInput(
            "no settings to update: set batch_size and/or bm25_enabled".to_string(),
        ));
    }
    if let Some(batch_size) = update.batch_size {
        validate_batch_size(batch_size)?;
    }

    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT job_name, src_table, src_schema, src_columns, primary_key, update_time_col, model, bm25_enabled, batch_size
         FROM vectorize.job
         WHERE job_name = $1
         FOR UPDATE",
    )
    .bind(job_name)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| VectorizeError::NotFound(format!("Job '{job_name}' not found")))?;
    let mut job = VectorizeJob::from_row(&row)?;

    sqlx::query(
        "UPDATE vectorize.job
         SET batch_size = COALESCE($2, batch_size),
             bm25_enabled = COALESCE($3, bm25_enabled)
         WHERE job_name = $1",
    )
    .bind(job_name)
    .bind(update.batch_size)
    .bind(update.bm25_enabled)
    .execute(&mut *tx)
    .await?;

    if let Some(batch_size) = update.batch_size {
        // primary_key is interpolated into the function body; rows written before
        // identifier validation existed are not guaranteed to be safe
        query::check_input(&job.primary_key)?;
        let trigger_handler =
            query::create_trigger_handler_with_batch_size(job_name, &job.primary_key, batch_size);
        sqlx::query(&trigger_handler).execute(&mut *tx).await?;
        job.batch_size = batch_size;
    }
    if let Some(bm25_enabled) = update.bm25_enabled {
        job.bm25_enabled = bm25_enabled;
    }
    tx.commit().await?;

    Ok(job)
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
        "DELETE FROM pgmq.q_vectorize_jobs WHERE message->>'job_name' = $1".to_string();
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

    fn job(src_columns: &[&str]) -> VectorizeJob {
        VectorizeJob {
            job_name: "my_job".to_string(),
            src_table: "products".to_string(),
            src_schema: "public".to_string(),
            src_columns: src_columns.iter().map(|s| s.to_string()).collect(),
            primary_key: "product_id".to_string(),
            update_time_col: "updated_at".to_string(),
            model: crate::types::Model::new("openai/text-embedding-3-small").unwrap(),
            bm25_enabled: false,
            batch_size: crate::types::DEFAULT_BATCH_SIZE,
        }
    }

    #[test]
    fn test_validate_job_identifiers_accepts_valid() {
        assert!(validate_job_identifiers(&job(&["product_name", "description"])).is_ok());
    }

    #[test]
    fn test_validate_job_identifiers_rejects_injection() {
        let bad = [
            "description, '')) FROM pg_shadow --",
            "a\"b",
            "a;b",
            "a b",
            "",
        ];
        for col in bad {
            let err = validate_job_identifiers(&job(&["ok", col])).unwrap_err();
            assert!(matches!(err, VectorizeError::InvalidInput(_)), "{col}");
        }

        let mut j = job(&["ok"]);
        j.job_name = "x'; drop".to_string();
        assert!(matches!(
            validate_job_identifiers(&j),
            Err(VectorizeError::InvalidInput(_))
        ));

        let mut j = job(&["ok"]);
        j.update_time_col = "updated_at::text".to_string();
        assert!(matches!(
            validate_job_identifiers(&j),
            Err(VectorizeError::InvalidInput(_))
        ));
    }

    #[test]
    fn test_validate_job_identifiers_rejects_empty_columns() {
        assert!(matches!(
            validate_job_identifiers(&job(&[])),
            Err(VectorizeError::InvalidInput(_))
        ));
    }

    #[test]
    fn test_validate_batch_size() {
        assert!(validate_batch_size(1).is_ok());
        assert!(validate_batch_size(MAX_BATCH_SIZE).is_ok());
        for bad in [0, -1, MAX_BATCH_SIZE + 1] {
            assert!(matches!(
                validate_batch_size(bad),
                Err(VectorizeError::InvalidInput(_))
            ));
        }
        let mut j = job(&["ok"]);
        j.batch_size = 0;
        assert!(validate_job_identifiers(&j).is_err());
    }

    #[test]
    fn test_job_batch_size_defaults_when_omitted() {
        let j: VectorizeJob = serde_json::from_value(serde_json::json!({
            "job_name": "my_job",
            "src_table": "products",
            "src_schema": "public",
            "src_columns": ["description"],
            "primary_key": "product_id",
            "update_time_col": "updated_at",
            "model": "openai/text-embedding-3-small"
        }))
        .unwrap();
        assert_eq!(j.batch_size, crate::types::DEFAULT_BATCH_SIZE);
    }

    #[test]
    fn test_job_update_rejects_unknown_fields() {
        let u: JobUpdate = serde_json::from_str(r#"{"batch_size": 50}"#).unwrap();
        assert_eq!(u.batch_size, Some(50));
        assert_eq!(u.bm25_enabled, None);
        assert!(serde_json::from_str::<JobUpdate>(r#"{"model": "openai/x"}"#).is_err());
    }

    #[test]
    fn test_create_trigger_handler_with_batch_size() {
        let sql = query::create_trigger_handler_with_batch_size("my_job", "product_id", 50);
        assert!(sql.contains("CREATE OR REPLACE FUNCTION vectorize.handle_update_my_job()"));
        assert!(sql.contains("'my_job'::text"));
        assert!(sql.contains("array_agg(product_id::text)"));
        assert!(sql.contains("50::integer"));
    }
}
