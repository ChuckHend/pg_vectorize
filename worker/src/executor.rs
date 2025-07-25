use pgmq::Message;
use sqlx::PgPool;
use vectorize_core::errors::VectorizeError;
use vectorize_core::types::JobMessage;

use crate::ops;
use anyhow::Result;
use pgmq::PGMQueueExt;
use tiktoken_rs::cl100k_base;
use vectorize_core::config::Config;
use vectorize_core::db;
use vectorize_core::init;
use vectorize_core::transformers::{http_handler, providers, types::Inputs};

pub async fn poll_job(
    conn: &PgPool,
    queue: &PGMQueueExt,
    config: &Config,
) -> Result<Option<()>, VectorizeError> {
    let msg: Message<JobMessage> = match queue.read::<JobMessage>(&config.queue_name, 300_i32).await
    {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            log::debug!("No message found in queue: {}", config.queue_name);
            return Ok(None);
        }
        Err(e) => {
            return Err(anyhow::anyhow!("failed reading message: {}", e).into());
        }
    };

    let job_name = msg.message.job_name.clone();

    let read_ct: i32 = msg.read_ct;
    let msg_id: i64 = msg.msg_id;
    if read_ct <= config.max_retries {
        match execute_job(conn, msg).await {
            Ok(_) => {
                log::info!("Successfully processed job: {job_name}");
            }
            Err(e) => {
                log::error!("Error processing job: {job_name}, msg_id: {msg_id}, error: {e}");
                Err(e)?;
            }
        }
    } else {
        log::error!(
            "message exceeds max retry of {}, archiving msg_id: {}",
            config.max_retries,
            msg_id
        );
    }

    queue.delete(&config.queue_name, msg_id).await?;

    Ok(Some(()))
}

/// processes a single job from the queue
async fn execute_job(pool: &PgPool, msg: Message<JobMessage>) -> Result<(), VectorizeError> {
    let bpe = cl100k_base().unwrap();

    let job_name = msg.message.job_name.clone();
    let vectorizejob = db::get_vectorize_job(pool, &job_name).await?;
    log::debug!("Retrieved vectorize job: {vectorizejob:?}");
    let provider = providers::get_provider(&vectorizejob.model.source, None, None, None)?;

    log::info!("processing job: {vectorizejob:?}");

    let pkey_type = init::get_column_datatype(
        pool,
        &vectorizejob.src_schema,
        &vectorizejob.src_table,
        &vectorizejob.primary_key,
    )
    .await?;

    let select_cols = vectorizejob
        .src_columns
        .iter()
        .map(|col| format!("'{col}: ' || COALESCE({col}, '') || ' '"))
        .collect::<Vec<String>>()
        .join(" || ' ' || ");
    let job_records_query = format!(
        "
    SELECT
        {primary_key}::text as record_id,
        {select_cols} as input_text
    FROM {schema}.{relation}
    WHERE {primary_key} = ANY ($1::{pk_type}[])",
        primary_key = vectorizejob.primary_key,
        schema = vectorizejob.src_schema,
        relation = vectorizejob.src_table,
        pk_type = pkey_type
    );

    #[derive(sqlx::FromRow)]
    struct Res {
        record_id: String,
        input_text: String,
    }

    let job_records: Vec<Res> = sqlx::query_as(&job_records_query)
        .bind(&msg.message.record_ids)
        .fetch_all(pool)
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
        providers::prepare_generic_embedding_request(&vectorizejob.model, &inputs);

    let embeddings = provider.generate_embedding(&embedding_request).await?;

    let paired_embeddings = http_handler::merge_input_output(inputs, embeddings.embeddings);

    ops::upsert_embedding_table(
        pool,
        &vectorizejob.job_name,
        paired_embeddings,
        "vectorize", // embeddings always live in vectorize schema
        &vectorizejob.primary_key,
        &pkey_type,
    )
    .await?;

    Ok(())
}
