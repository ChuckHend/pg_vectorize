use crate::core::errors::DatabaseError;
use crate::core::guc;
use crate::core::transformers::types::Inputs;
use crate::core::transformers::{http_handler, providers};
use crate::core::types::{JobMessage, JobParams};
use crate::core::worker::ops;

use log::error;
use pgmq::{Message, PGMQueueExt};
use sqlx::{Pool, Postgres};
use std::env;
use tiktoken_rs::cl100k_base;

use crate::core::types::VectorizeMeta;

use anyhow::{Result, anyhow};

// errors if input contains non-alphanumeric characters or underscore
// in other worse - valid column names only
pub fn check_input(input: &str) -> Result<()> {
    let valid = input
        .as_bytes()
        .iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'_');
    match valid {
        true => Ok(()),
        false => Err(anyhow!("Invalid Input: {}", input)),
    }
}

pub fn collapse_to_csv(strings: &[String]) -> String {
    strings
        .iter()
        .map(|s| {
            check_input(s).expect("Failed to validate input");
            s.as_str()
        })
        .collect::<Vec<_>>()
        .join("|| ', ' ||")
}

pub async fn poll_job(
    conn: &Pool<Postgres>,
    queue: &PGMQueueExt,
    config: &Config,
) -> Result<Option<()>> {
    let msg: Message<JobMessage> = match queue.read::<JobMessage>(&config.queue_name, 300_i32).await
    {
        Ok(Some(msg)) => msg,
        Ok(None) => {
            log::debug!("No message found in queue: {}", config.queue_name);
            return Ok(None);
        }
        Err(e) => {
            return Err(anyhow::anyhow!("failed reading message: {}", e));
        }
    };

    let read_ct: i32 = msg.read_ct;
    let msg_id: i64 = msg.msg_id;
    if read_ct <= config.max_retries {
        execute_job(conn, msg).await?;
    } else {
        error!(
            "message exceeds max retry of {}, archiving msg_id: {}",
            config.max_retries, msg_id
        );
    }

    queue.archive(&config.queue_name, msg_id).await?;

    Ok(Some(()))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub queue_name: String,
    pub embedding_svc_url: String,
    pub openai_api_key: Option<String>,
    pub ollama_svc_url: String,
    pub embedding_request_timeout: i32,
    pub poll_interval: u64,
    pub poll_interval_error: u64,
    pub max_retries: i32,
    pub webserver_port: u16,
    pub num_server_workers: usize,
}

impl Config {
    pub fn from_env() -> Config {
        Config {
            database_url: from_env_default(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/postgres",
            ),
            queue_name: from_env_default("VECTORIZE_QUEUE", "vectorize_jobs"),
            embedding_svc_url: from_env_default(
                "EMBEDDING_SVC_URL",
                "http://localhost:3000/v1/embeddings",
            ),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            ollama_svc_url: from_env_default("OLLAMA_SVC_URL", "http://localhost:3001"),
            embedding_request_timeout: from_env_default("EMBEDDING_REQUEST_TIMEOUT", "6")
                .parse()
                .unwrap(),
            // time to wait between polling for job when there are no messages in queue
            poll_interval: from_env_default("POLL_INTERVAL", "2").parse().unwrap(),
            // time to wait between polling for job when there has been an error in processing
            poll_interval_error: from_env_default("POLL_INTERVAL_ERROR", "10")
                .parse()
                .unwrap(),
            max_retries: from_env_default("MAX_RETRIES", "2").parse().unwrap(),
            webserver_port: from_env_default("WEBSERVER_PORT", "8080").parse().unwrap(),
            num_server_workers: from_env_default("NUM_SERVER_WORKERS", "8").parse().unwrap(),
        }
    }
}

/// source a variable from environment - use default if not exists
pub fn from_env_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
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
    let job_meta = get_vectorize_meta(&msg.message.job_name, dbclient).await?;
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
        crate::core::types::TableMethod::append => {
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
        crate::core::types::TableMethod::join => {
            ops::upsert_embedding_table(
                dbclient,
                &job_meta.name,
                paired_embeddings,
                &job_params.schema,
                &job_params.primary_key,
                &job_params.pkey_type,
            )
            .await?
        }
    }
    Ok(())
}
