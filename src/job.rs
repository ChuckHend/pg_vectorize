use anyhow::Result;

use crate::executor::{create_batches, new_rows_query, JobMessage, VectorizeMeta};
use crate::guc::BATCH_SIZE;
use crate::init::VECTORIZE_QUEUE;
use crate::transformers::types::Inputs;
use crate::types::{self, JobParams, JobType};
use crate::util;

use pgrx::prelude::*;
use tiktoken_rs::cl100k_base;

/// called by the trigger function when a table is updated
/// handles enqueueing the embedding transform jobs
#[pg_extern]
fn _handle_table_update(job_name: &str, record_ids: Vec<String>, inputs: Vec<String>) {
    // get the job metadata
    if record_ids.len() != inputs.len() {
        error!("record_ids and inputs must be the same length");
    }
    let project_meta: VectorizeMeta = if let Ok(Some(js)) = util::get_vectorize_meta_spi(job_name) {
        js
    } else {
        error!("failed to get project metadata");
    };

    // create Input objects
    let bpe = cl100k_base().unwrap();
    let mut new_inputs: Vec<Inputs> = Vec::new();
    for (record_id, input) in record_ids.into_iter().zip(inputs.into_iter()) {
        let token_estimate = bpe.encode_with_special_tokens(&input).len() as i32;
        new_inputs.push(Inputs {
            record_id,
            inputs: input,
            token_estimate,
        })
    }

    // create the job message
    let job_message = JobMessage {
        job_name: job_name.to_string(),
        job_meta: project_meta,
        inputs: new_inputs,
    };

    // send the job message to the queue
    let query = format!(
        "select pgmq.send('{VECTORIZE_QUEUE}', '{}');",
        serde_json::to_string(&job_message).unwrap()
    );
    let _ran: Result<_, spi::Error> = Spi::connect(|mut c| {
        let _r = c.update(&query, None, None)?;
        Ok(())
    });
}

static TRIGGER_FN_PREFIX: &str = "vectorize.handle_update_";

/// creates a function that can be called by trigger
pub fn create_trigger_handler(job_name: &str, input_columns: &[String], pkey: &str) -> String {
    let input_concat = generate_input_concat(input_columns);
    format!(
        "
CREATE OR REPLACE FUNCTION {TRIGGER_FN_PREFIX}{job_name}()
RETURNS trigger AS $$
BEGIN
    PERFORM vectorize._handle_table_update(
        '{job_name}',
        ARRAY[NEW.{pkey}::text],
        ARRAY[{input_concat}]
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;    
"
    )
}

// creates the trigger for a row update
pub fn create_update_trigger(
    job_name: &str,
    schema: &str,
    table_name: &str,
    input_columns: &[String],
) -> String {
    let trigger_condition = generate_trigger_condition(input_columns);
    format!(
        "
CREATE OR REPLACE TRIGGER vectorize_update_trigger_{job_name}
AFTER UPDATE ON {schema}.{table_name}
FOR EACH ROW
WHEN ( {trigger_condition} )
EXECUTE FUNCTION vectorize.handle_update_{job_name}();"
    )
}

pub fn create_insert_trigger(job_name: &str, schema: &str, table_name: &str) -> String {
    format!(
        "
CREATE OR REPLACE TRIGGER vectorize_insert_trigger_{job_name}
AFTER INSERT ON {schema}.{table_name}
FOR EACH ROW
EXECUTE FUNCTION vectorize.handle_update_{job_name}();"
    )
}

// takes in arbitrary number of columns to evaluate for changes and returns the trigger condition
fn generate_trigger_condition(inputs: &[String]) -> String {
    inputs
        .iter()
        .map(|item| format!("OLD.{item} IS DISTINCT FROM NEW.{item}"))
        .collect::<Vec<String>>()
        .join(" OR ")
}

// concatenates the input columns into a single string
fn generate_input_concat(inputs: &[String]) -> String {
    inputs
        .iter()
        .map(|item| format!("NEW.{item}"))
        .collect::<Vec<String>>()
        .join(" || ' ' || ")
}

// creates batches of embedding jobs
// typically used on table init
pub fn initalize_table_job(
    job_name: &str,
    job_params: &JobParams,
    job_type: &JobType,
    transformer: &str,
    search_alg: types::SimilarityAlg,
) -> Result<()> {
    // start with initial batch load
    let rows_need_update_query: String = new_rows_query(job_name, job_params);
    let mut inputs: Vec<Inputs> = Vec::new();
    let bpe = cl100k_base().unwrap();
    let _: Result<_, spi::Error> = Spi::connect(|c| {
        let rows = c.select(&rows_need_update_query, None, None)?;
        for row in rows {
            let ipt = row["input_text"]
                .value::<String>()?
                .expect("input_text is null");
            let token_estimate = bpe.encode_with_special_tokens(&ipt).len() as i32;
            inputs.push(Inputs {
                record_id: row["record_id"]
                    .value::<String>()?
                    .expect("record_id is null"),
                inputs: ipt,
                token_estimate,
            });
        }
        Ok(())
    });

    let max_batch_size = BATCH_SIZE.get();
    let batches = create_batches(inputs, max_batch_size);
    let vectorize_meta = VectorizeMeta {
        name: job_name.to_string(),
        // TODO: in future, lookup job id once this gets put into use
        // job_id is currently not used, job_name is unique
        job_id: 0,
        job_type: job_type.clone(),
        params: serde_json::to_value(job_params.clone()).unwrap(),
        transformer: transformer.to_string(),
        search_alg: search_alg.clone(),
        last_completion: None,
    };
    for b in batches {
        let job_message = JobMessage {
            job_name: job_name.to_string(),
            job_meta: vectorize_meta.clone(),
            inputs: b,
        };
        let query = format!(
            "select pgmq.send('{VECTORIZE_QUEUE}', '{}');",
            serde_json::to_string(&job_message)
                .unwrap()
                .replace('\'', "''")
        );
        let _ran: Result<_, spi::Error> = Spi::connect(|mut c| {
            let _r = c.update(&query, None, None)?;
            Ok(())
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_input_concat_multi() {
        let input = vec!["column1".to_string(), "column2".to_string()];
        let result = generate_input_concat(&input);
        let expected = "NEW.column1 || ' ' || NEW.column2";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_generate_input_concat_single() {
        let input = vec!["column1".to_string()];
        let result = generate_input_concat(&input);
        let expected = "NEW.column1";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_generate_trigger_condition_multi() {
        let input = vec!["column1".to_string(), "column2".to_string()];
        let result = generate_trigger_condition(&input);
        let expected =
            "OLD.column1 IS DISTINCT FROM NEW.column1 OR OLD.column2 IS DISTINCT FROM NEW.column2";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_generate_trigger_condition_single() {
        let input = vec!["column1".to_string()];
        let result = generate_trigger_condition(&input);
        let expected = "OLD.column1 IS DISTINCT FROM NEW.column1";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_create_update_trigger_multi() {
        let job_name = "example_job";
        let table_name = "example_table";
        let input_columns = vec!["column1".to_string(), "column2".to_string()];

        let expected = format!(
            "
CREATE OR REPLACE TRIGGER vectorize_update_trigger_example_job
AFTER UPDATE ON myschema.example_table
FOR EACH ROW
WHEN ( OLD.column1 IS DISTINCT FROM NEW.column1 OR OLD.column2 IS DISTINCT FROM NEW.column2 )
EXECUTE FUNCTION vectorize.handle_update_example_job();"
        );
        let result = create_update_trigger(job_name, "myschema", table_name, &input_columns);
        assert_eq!(expected, result);
    }

    #[test]
    fn test_create_update_trigger_single() {
        let job_name = "another_job";
        let table_name = "another_table";
        let input_columns = vec!["column1".to_string()];

        let expected = format!(
            "
CREATE OR REPLACE TRIGGER vectorize_update_trigger_another_job
AFTER UPDATE ON myschema.another_table
FOR EACH ROW
WHEN ( OLD.column1 IS DISTINCT FROM NEW.column1 )
EXECUTE FUNCTION vectorize.handle_update_another_job();"
        );
        let result = create_update_trigger(job_name, "myschema", table_name, &input_columns);
        assert_eq!(expected, result);
    }

    #[test]
    fn test_create_insert_trigger_single() {
        let job_name = "another_job";
        let table_name = "another_table";

        let expected = format!(
            "
CREATE OR REPLACE TRIGGER vectorize_insert_trigger_another_job
AFTER INSERT ON myschema.another_table
FOR EACH ROW
EXECUTE FUNCTION vectorize.handle_update_another_job();"
        );
        let result = create_insert_trigger(job_name, "myschema", table_name);
        assert_eq!(expected, result);
    }
}
