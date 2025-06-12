use pgrx::prelude::*;

use crate::guc::BATCH_SIZE;
use crate::init::VECTORIZE_QUEUE;
use crate::util::get_pg_conn;
use crate::workers::get_vectorize_meta;
use sqlx::error::Error;
use sqlx::postgres::PgRow;
use sqlx::{Pool, Postgres, Row};
use tiktoken_rs::cl100k_base;
use vectorize_core::errors::DatabaseError;
use vectorize_core::query::{check_input, create_batches, new_rows_query_join};
use vectorize_core::transformers::types::Inputs;
use vectorize_core::types::{JobMessage, JobParams, TableMethod};

#[pg_extern]
pub fn batch_texts(
    record_ids: Vec<String>,
    batch_size: i32,
) -> TableIterator<'static, (name!(batch, Vec<String>),)> {
    let batch_size = batch_size as usize;

    let total_records = record_ids.len();
    if batch_size == 0 || total_records <= batch_size {
        return TableIterator::new(vec![record_ids].into_iter().map(|arr| (arr,)));
    }

    let num_batches = (total_records + batch_size - 1) / batch_size;

    let mut batches = Vec::with_capacity(num_batches);

    for i in 0..num_batches {
        let start = i * batch_size;
        let end = std::cmp::min(start + batch_size, total_records);

        batches.push(record_ids[start..end].to_vec());
    }
    TableIterator::new(batches.into_iter().map(|arr| (arr,)))
}

// called by pg_cron on schedule
// identifiers new inputs and enqueues them
#[pg_extern]
#[pg_guard]
fn job_execute(job_name: String) {
    log!("pg-vectorize: refresh job: {}", job_name);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));

    let max_batch_size = BATCH_SIZE.get();

    runtime.block_on(async {
        let conn = get_pg_conn()
            .await
            .unwrap_or_else(|e| error!("pg-vectorize: failed to establish db connection: {}", e));
        let queue = pgmq::PGMQueueExt::new_with_pool(conn.clone()).await;
        let meta = get_vectorize_meta(&job_name, &conn)
            .await
            .unwrap_or_else(|e| error!("failed to get job metadata: {}", e));
        let job_params = serde_json::from_value::<JobParams>(meta.params.clone())
            .unwrap_or_else(|e| error!("failed to deserialize job params: {}", e));

        let new_or_updated_rows = get_new_updates(&conn, &job_name, job_params)
            .await
            .unwrap_or_else(|e| error!("failed to get new updates: {}", e));

        match new_or_updated_rows {
            Some(rows) => {
                info!("num new records: {}", rows.len());
                let batches = create_batches(rows, max_batch_size);
                info!(
                    "total batches: {}, max_batch_size: {}",
                    batches.len(),
                    max_batch_size
                );
                for b in batches {
                    let record_ids = b.iter().map(|i| i.record_id.clone()).collect::<Vec<_>>();
                    let msg = JobMessage {
                        job_name: job_name.clone(),
                        record_ids,
                    };
                    let msg_id = queue
                        .send(VECTORIZE_QUEUE, &msg)
                        .await
                        .unwrap_or_else(|e| error!("failed to send message updates: {}", e));
                    log!("message sent: {}", msg_id);
                }
            }
            None => {
                log!("pg-vectorize: job: {}, no new records", job_name);
            }
        };
    })
}

pub fn new_rows_query(job_name: &str, job_params: &JobParams) -> String {
    let cols = collapse_to_csv(&job_params.columns);

    // query source and return any new rows that need transformation
    // return any row where last updated embedding is also null (never populated)
    let base_query = format!(
        "
        SELECT 
        {record_id}::text as record_id,
        {cols} as input_text
        FROM {schema}.{table}
        ",
        record_id = job_params.primary_key,
        schema = job_params.schema,
        table = job_params.relation,
    );
    if let Some(updated_at_col) = &job_params.update_time_col {
        // updated_at_column is not required when `schedule` is realtime
        let where_clause = format!(
            "
            WHERE {updated_at_col} > COALESCE
            (
                {job_name}_updated_at::timestamp,
                '0001-01-01 00:00:00'::timestamp
            )",
        );
        format!(
            "
            {base_query}
            {where_clause}
        "
        )
    } else {
        base_query
    }
}

// queries a table and returns rows that need new embeddings
// used for the TableMethod::append, which has source and embedding on the same table
pub async fn get_new_updates(
    pool: &Pool<Postgres>,
    job_name: &str,
    job_params: JobParams,
) -> Result<Option<Vec<Inputs>>, DatabaseError> {
    let query = match job_params.table_method {
        TableMethod::append => new_rows_query(job_name, &job_params),
        TableMethod::join => new_rows_query_join(
            job_name,
            &job_params.columns,
            &job_params.schema,
            &job_params.relation,
            &job_params.primary_key,
            job_params.update_time_col.clone(),
        ),
    };
    let rows: Result<Vec<PgRow>, Error> = sqlx::query(&query).fetch_all(pool).await;
    match rows {
        Ok(rows) => {
            if !rows.is_empty() {
                let bpe = cl100k_base().unwrap();
                let mut new_inputs: Vec<Inputs> = Vec::new();
                for r in rows {
                    let ipt: String = r.get("input_text");
                    let token_estimate = bpe.encode_with_special_tokens(&ipt).len() as i32;
                    new_inputs.push(Inputs {
                        record_id: r.get("record_id"),
                        inputs: ipt.trim().to_owned(),
                        token_estimate,
                    })
                }
                log!("pg-vectorize: num new inputs: {}", new_inputs.len());
                Ok(Some(new_inputs))
            } else {
                Ok(None)
            }
        }
        Err(sqlx::error::Error::RowNotFound) => Ok(None),
        Err(e) => Err(e)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_batches_normal() {
        let data = vec![
            Inputs {
                record_id: "1".to_string(),
                inputs: "Test 1.".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "2".to_string(),
                inputs: "Test 2.".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "3".to_string(),
                inputs: "Test 3.".to_string(),
                token_estimate: 3,
            },
        ];

        let batches = create_batches(data, 4);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn test_create_batches_empty() {
        let data: Vec<Inputs> = Vec::new();
        let batches = create_batches(data, 4);
        assert_eq!(batches.len(), 0);
    }

    #[test]
    fn test_create_batches_large() {
        let data = vec![
            Inputs {
                record_id: "1".to_string(),
                inputs: "Test 1.".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "2".to_string(),
                inputs: "Test 2.".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "3".to_string(),
                inputs: "Test 3.".to_string(),
                token_estimate: 100,
            },
        ];
        let batches = create_batches(data, 5);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].len(), 1);
        assert_eq!(batches[1][0].token_estimate, 100);
    }
}
