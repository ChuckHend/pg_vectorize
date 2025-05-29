use crate::transformers::types::Inputs;
use crate::types::{self, JobParams};
use anyhow::{anyhow, Result};
use sqlx::error::Error;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row};
use tiktoken_rs::cl100k_base;

pub const VECTORIZE_SCHEMA: &str = "vectorize";
static TRIGGER_FN_PREFIX: &str = "vectorize.handle_update_";

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

pub fn create_vectorize_table() -> String {
    "CREATE TABLE IF NOT EXISTS vectorize.job
        (
            id UUID DEFAULT gen_random_uuid() PRIMARY KEY,
            job_name TEXT NOT NULL UNIQUE,
            src_schema TEXT NOT NULL,
            src_table TEXT NOT NULL,
            src_column TEXT NOT NULL,
            primary_key TEXT NOT NULL,
            update_time_col TEXT NOT NULL,
            model TEXT NOT NULL,
            params JSONB
        );
        "
    .to_string()
}

pub fn init_index_query(job_name: &str, idx_type: &str, job_params: &JobParams) -> String {
    check_input(job_name).expect("invalid job name");
    let src_schema = job_params.schema.clone();
    let src_table = job_params.relation.clone();
    match idx_type.to_uppercase().as_str() {
        "GIN" | "GIST" => {} // Do nothing, it's valid
        _ => panic!("Expected 'GIN' or 'GIST', got '{}' index type", idx_type),
    }

    format!(
        "
        CREATE INDEX IF NOT EXISTS {job_name}_idx on {schema}.{table} using {idx_type} (to_tsvector('english', {columns}));
        ",
        job_name = job_name,
        schema = src_schema,
        table = src_table,
        columns = job_params.columns.join(" || ' ' || "),
    )
}

/// creates a project view over a source table and the embeddings table
pub fn create_project_view(job_name: &str, schema: &str, relation: &str, pkey: &str) -> String {
    format!(
        "CREATE VIEW vectorize.{job_name}_view as 
        SELECT t0.*, t1.embeddings, t1.updated_at as embeddings_updated_at
        FROM {schema}.{table} t0
        INNER JOIN vectorize._embeddings_{job_name} t1
            ON t0.{primary_key} = t1.{primary_key};
        ",
        job_name = job_name,
        schema = schema,
        table = relation,
        primary_key = pkey
    )
}

pub fn create_embedding_table(
    job_name: &str,
    join_key: &str,
    join_key_type: &str,
    col_type: &str,
    src_schema: &str,
    src_table: &str,
) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS vectorize._embeddings_{job_name} (
            {join_key} {join_key_type} UNIQUE NOT NULL,
            embeddings {col_type} NOT NULL,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
            FOREIGN KEY ({join_key}) REFERENCES {src_schema}.{src_table} ({join_key}) ON DELETE CASCADE
        );
        ",
        job_name = job_name,
        join_key = join_key,
        join_key_type = join_key_type,
        col_type = col_type,
        src_schema = src_schema,
        src_table = src_table,
    )
}

pub fn create_hnsw_l2_index(
    job_name: &str,
    schema: &str,
    table: &str,
    embedding_col: &str,
) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {job_name}_hnsw_l2_idx ON {schema}.{table}
        USING hnsw ({embedding_col} vector_l2_ops);
        ",
    )
}

pub fn create_hnsw_ip_index(
    job_name: &str,
    schema: &str,
    table: &str,
    embedding_col: &str,
) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {job_name}_hnsw_ip_idx ON {schema}.{table}
        USING hnsw ({embedding_col} vector_ip_ops);
        ",
    )
}

pub fn create_hnsw_cosine_index(
    job_name: &str,
    schema: &str,
    table: &str,
    embedding_col: &str,
) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {job_name}_hnsw_cos_idx ON {schema}.{table}
        USING hnsw ({embedding_col} vector_cosine_ops);
        ",
    )
}

pub fn init_job_query() -> String {
    format!(
        "
        INSERT INTO {schema}.job (name, index_dist_type, transformer, params)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (name) DO UPDATE SET
            index_dist_type = EXCLUDED.index_dist_type,
            params = job.params || EXCLUDED.params;
        ",
        schema = types::VECTORIZE_SCHEMA
    )
}

pub fn drop_project_view(job_name: &str) -> String {
    format!(
        "DROP VIEW IF EXISTS vectorize.{job_name}_view;",
        job_name = job_name
    )
}

/// creates a function that can be called by trigger
pub fn create_trigger_handler(job_name: &str, pkey: &str) -> String {
    format!(
        "
CREATE OR REPLACE FUNCTION {TRIGGER_FN_PREFIX}{job_name}()
RETURNS TRIGGER AS $$
DECLARE
BEGIN
    PERFORM vectorize._handle_table_update(
        '{job_name}'::text,
       (SELECT array_agg({pkey}::text) FROM new_table)::TEXT[]
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;    
"
    )
}

// creates the trigger for a row update
// these triggers use transition tables
// transition tables cannot be specified for triggers with more than one event
// so we create two triggers instead
pub fn create_event_trigger(job_name: &str, schema: &str, table_name: &str, event: &str) -> String {
    format!(
        "
CREATE OR REPLACE TRIGGER vectorize_{event_name}_trigger_{job_name}
AFTER {event} ON {schema}.{table_name}
REFERENCING NEW TABLE AS new_table
FOR EACH STATEMENT
EXECUTE FUNCTION vectorize.handle_update_{job_name}();",
        event_name = event.to_lowercase()
    )
}

pub fn new_rows_query_join(
    job_name: &str,
    columns: &[String],
    schema: &str,
    table: &str,
    pkey: &str,
    update_time_col: Option<String>,
) -> String {
    let cols = columns
        .iter()
        .map(|s| format!("t0.{}", s))
        .collect::<Vec<_>>()
        .join(",");

    let base_query = format!(
        "
    SELECT t0.{join_key}::text as record_id, {cols} as input_text
    FROM {schema}.{table} t0
    LEFT JOIN vectorize._embeddings_{job_name} t1 ON t0.{join_key} = t1.{join_key}
    WHERE t1.{join_key} IS NULL",
        join_key = pkey,
        cols = cols,
        schema = schema,
        table = table,
        job_name = job_name
    );
    if let Some(updated_at_col) = update_time_col {
        // updated_at_column is not required when `schedule` is realtime
        let where_clause = format!(
            "
            OR t0.{updated_at_col} > COALESCE
            (
                t1.updated_at::timestamp,
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

pub async fn get_new_updates<'c, E: sqlx::Executor<'c, Database = Postgres>>(
    pool: E,
    query: &str,
) -> Result<Option<Vec<Inputs>>, Error> {
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
                log::info!("pg-vectorize: num new inputs: {}", new_inputs.len());
                Ok(Some(new_inputs))
            } else {
                Ok(None)
            }
        }
        Err(sqlx::error::Error::RowNotFound) => Ok(None),
        Err(e) => Err(e)?,
    }
}

// creates batches based on total token count
// batch_size is the max token count per batch
pub fn create_batches(data: Vec<Inputs>, batch_size: i32) -> Vec<Vec<Inputs>> {
    let mut groups: Vec<Vec<Inputs>> = Vec::new();
    let mut current_group: Vec<Inputs> = Vec::new();
    let mut current_token_count = 0;

    for input in data {
        if current_token_count + input.token_estimate > batch_size {
            // Create a new group
            groups.push(current_group);
            current_group = Vec::new();
            current_token_count = 0;
        }
        current_token_count += input.token_estimate;
        current_group.push(input);
    }

    // Add any remaining inputs to the groups
    if !current_group.is_empty() {
        groups.push(current_group);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_update_trigger_single() {
        let job_name = "another_job";
        let table_name = "another_table";

        let expected = format!(
            "
CREATE OR REPLACE TRIGGER vectorize_update_trigger_another_job
AFTER UPDATE ON myschema.another_table
REFERENCING NEW TABLE AS new_table
FOR EACH STATEMENT
EXECUTE FUNCTION vectorize.handle_update_another_job();"
        );
        let result = create_event_trigger(job_name, "myschema", table_name, "UPDATE");
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
REFERENCING NEW TABLE AS new_table
FOR EACH STATEMENT
EXECUTE FUNCTION vectorize.handle_update_another_job();"
        );
        let result = create_event_trigger(job_name, "myschema", table_name, "INSERT");
        assert_eq!(expected, result);
    }
}
