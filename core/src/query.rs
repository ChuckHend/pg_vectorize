use crate::types::{self, JobParams};
use anyhow::{anyhow, Result};
pub const VECTORIZE_SCHEMA: &str = "vectorize";

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
pub fn create_project_view(job_name: &str, job_params: &JobParams) -> String {
    format!(
        "CREATE VIEW vectorize.{job_name}_view as 
        SELECT t0.*, t1.embeddings, t1.updated_at as embeddings_updated_at
        FROM {schema}.{table} t0
        INNER JOIN vectorize._embeddings_{job_name} t1
            ON t0.{primary_key} = t1.{primary_key};
        ",
        job_name = job_name,
        schema = job_params.schema,
        table = job_params.relation,
        primary_key = job_params.primary_key,
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
