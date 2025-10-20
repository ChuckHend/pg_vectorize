pub mod pg_bgw;

use crate::executor::collapse_to_csv;
use anyhow::Result;
use pgmq::{Message, PGMQueueExt};
use pgrx::*;
use sqlx::{Pool, Postgres};
use tiktoken_rs::cl100k_base;
use vectorize_core::errors::DatabaseError;
use vectorize_core::guc;
use vectorize_core::transformers::http_handler;
use vectorize_core::transformers::providers;
use vectorize_core::transformers::types::Inputs;
use vectorize_core::types;
use vectorize_core::types::{JobMessage, JobParams, VectorizeMeta};
use vectorize_worker::ops;

pub async fn run_worker(
    queue: PGMQueueExt,
    conn: &Pool<Postgres>,
    queue_name: &str,
) -> Result<Option<()>> {
    let msg: Message<types::JobMessage> =
        match queue.read::<types::JobMessage>(queue_name, 180_i32).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                info!("pg-vectorize: No messages in queue");
                return Ok(None);
            }
            Err(e) => {
                warning!("pg-vectorize: Error reading message: {e}");
                return Err(anyhow::anyhow!("failed to read message"));
            }
        };

    let msg_id: i64 = msg.msg_id;
    let read_ct: i32 = msg.read_ct;
    info!(
        "pg-vectorize: received message for job: {:?}",
        msg.message.job_name
    );
    let job_success = execute_job(&conn.clone(), msg).await;
    let delete_it = match job_success {
        Ok(_) => {
            info!("pg-vectorize: job success");
            true
        }
        Err(e) => {
            warning!("pg-vectorize: job failed: {:?}", e);
            read_ct > 2
        }
    };

    // delete message from queue
    if delete_it {
        match queue.delete(queue_name, msg_id).await {
            Ok(_) => {
                info!("pg-vectorize: deleted message: {}", msg_id);
            }
            Err(e) => {
                warning!("pg-vectorize: Error deleting message: {}", e);
            }
        }
    }
    // return Some(), indicating that worker consumed some message
    // any possibly more messages on queue
    Ok(Some(()))
}

// get job meta
pub async fn get_vectorize_meta(
    job_name: &str,
    conn: &Pool<Postgres>,
) -> Result<VectorizeMeta, DatabaseError> {
    let row = sqlx::query_as!(
        VectorizeMeta,
        "
        SELECT
            job_id, name, index_dist_type, transformer, params
        FROM vectorize.job
        WHERE name = $1
        ",
        job_name.to_string(),
    )
    .fetch_one(conn)
    .await?;
    Ok(row)
}

/// processes a single job from the queue
pub async fn execute_job(dbclient: &Pool<Postgres>, msg: Message<JobMessage>) -> Result<()> {
    // Check if the job still exists - it may have been deleted
    let job_meta = match get_vectorize_meta(&msg.message.job_name, dbclient).await {
        Ok(meta) => meta,
        Err(DatabaseError::Db(sqlx::Error::RowNotFound)) => {
            warning!(
                "pg-vectorize: Job '{}' not found - it may have been deleted. Skipping message.",
                msg.message.job_name
            );
            // Return Ok to allow the message to be deleted from queue
            return Ok(());
        }
        Err(e) => return Err(anyhow::anyhow!("Failed to get job meta: {}", e)),
    };
    let mut job_params: JobParams = serde_json::from_value(job_meta.params.clone())?;
    let bpe = cl100k_base().unwrap();

    let guc_configs = guc::get_guc_configs(&job_meta.transformer.source, dbclient).await;
    // if api_key found in GUC, then use that and re-assign
    if let Some(k) = guc_configs.api_key {
        job_params.api_key = Some(k);
    }

    let provider = providers::get_provider(
        &job_meta.transformer.source,
        job_params.api_key.clone(),
        guc_configs.service_url,
        guc_configs.virtual_key,
    )?;

    let cols = collapse_to_csv(&job_params.columns);

    let job_records_query = format!(
        "
    SELECT
        {primary_key}::text as record_id,
        {cols} as input_text
    FROM {schema}.{relation}
    WHERE {primary_key} = ANY ($1::{pk_type}[])",
        primary_key = job_params.primary_key,
        cols = cols,
        schema = job_params.schema,
        relation = job_params.relation,
        pk_type = job_params.pkey_type
    );

    #[derive(sqlx::FromRow)]
    struct Res {
        record_id: String,
        input_text: String,
    }

    let job_records: Vec<Res> = sqlx::query_as(&job_records_query)
        .bind(&msg.message.record_ids)
        .fetch_all(dbclient)
        .await?;

    let inputs: Vec<Inputs> = job_records
        .iter()
        .map(|row| {
            let token_estimate = bpe.encode_with_special_tokens(&row.input_text).len() as i32;
            Inputs {
                record_id: row.record_id.clone(),
                inputs: row.input_text.trim().to_owned(),
                token_estimate,
            }
        })
        .collect();

    let embedding_request =
        providers::prepare_generic_embedding_request(&job_meta.transformer, &inputs);

    let embeddings = provider.generate_embedding(&embedding_request).await?;

    let paired_embeddings = http_handler::merge_input_output(inputs, embeddings.embeddings);
    match job_params.clone().table_method {
        vectorize_core::types::TableMethod::append => {
            ops::update_embeddings(
                dbclient,
                &job_params.schema,
                &job_params.relation,
                &job_meta.clone().name,
                &job_params.primary_key,
                &job_params.pkey_type,
                paired_embeddings,
            )
            .await?;
        }
        vectorize_core::types::TableMethod::join => {
            ops::upsert_embedding_table(
                dbclient,
                &job_meta.name,
                paired_embeddings,
                "vectorize", // embeddings always live in vectorize schema in JOIN method
                &job_params.primary_key,
                &job_params.pkey_type,
            )
            .await?
        }
    }
    Ok(())
}
