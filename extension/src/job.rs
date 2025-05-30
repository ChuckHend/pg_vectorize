use anyhow::Result;

use crate::executor::new_rows_query;
use crate::guc::BATCH_SIZE;
use crate::init::VECTORIZE_QUEUE;
use pgrx::prelude::*;
use tiktoken_rs::cl100k_base;
use vectorize_core::core::query::{create_batches, new_rows_query_join};
use vectorize_core::core::transformers::types::Inputs;
use vectorize_core::core::types::{JobMessage, JobParams, TableMethod};

// creates batches of embedding jobs
// typically used on table init
pub fn initalize_table_job(job_name: &str, job_params: &JobParams) -> Result<()> {
    // start with initial batch load
    let rows_need_update_query: String = match job_params.table_method {
        TableMethod::append => new_rows_query(job_name, job_params),
        TableMethod::join => new_rows_query_join(
            job_name,
            &job_params.columns,
            &job_params.schema,
            &job_params.relation,
            &job_params.primary_key,
            job_params.update_time_col.clone(),
        ),
    };
    let mut inputs: Vec<Inputs> = Vec::new();
    let bpe = cl100k_base().unwrap();
    let _: Result<_, spi::Error> = Spi::connect(|c| {
        let rows = c.select(&rows_need_update_query, None, &[])?;
        for row in rows {
            let ipt = row["input_text"]
                .value::<String>()?
                .expect("input_text is null");
            let token_estimate = bpe.encode_with_special_tokens(&ipt).len() as i32;
            inputs.push(Inputs {
                record_id: row["record_id"]
                    .value::<String>()?
                    .expect("record_id is null"),
                inputs: ipt.trim().to_owned(),
                token_estimate,
            });
        }
        Ok(())
    });

    let max_batch_size = BATCH_SIZE.get();
    let batches = create_batches(inputs, max_batch_size);

    for b in batches {
        let job_message = JobMessage {
            job_name: job_name.to_string(),
            record_ids: b.iter().map(|i| i.record_id.clone()).collect(),
        };
        let query = "select pgmq.send($1, $2::jsonb);";
        let _ran: Result<_, spi::Error> = Spi::connect_mut(|c| {
            let _r = c.update(
                query,
                None,
                &[
                    VECTORIZE_QUEUE.into(),
                    pgrx::JsonB(
                        serde_json::to_value(job_message).expect("failed parsing job message"),
                    )
                    .into(),
                ],
            )?;
            Ok(())
        });
    }
    Ok(())
}
