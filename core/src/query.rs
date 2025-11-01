use crate::transformers::types::Inputs;
use crate::types::{self, JobParams};
use anyhow::{Result, anyhow};
use serde::Serialize;
use sqlx::error::Error;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row};
use std::collections::BTreeMap;
use tiktoken_rs::cl100k_base;
pub const VECTORIZE_SCHEMA: &str = "vectorize";
static TRIGGER_FN_PREFIX: &str = "vectorize.handle_update_";

/// Filter operators supported by the search API
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FilterOperator {
    /// Equal to (=)
    Equal,
    /// Greater than (>)
    GreaterThan,
    /// Greater than or equal (>=)
    GreaterThanOrEqual,
    /// Less than (<)
    LessThan,
    /// Less than or equal (<=)
    LessThanOrEqual,
}

impl FilterOperator {
    /// Convert operator to SQL operator string
    pub fn to_sql(&self) -> &'static str {
        match self {
            FilterOperator::Equal => "=",
            FilterOperator::GreaterThan => ">",
            FilterOperator::GreaterThanOrEqual => ">=",
            FilterOperator::LessThan => "<",
            FilterOperator::LessThanOrEqual => "<=",
        }
    }
}

/// A filter value with an operator
#[derive(Debug, Clone, Serialize)]
pub struct FilterValue {
    pub operator: FilterOperator,
    pub value: FilterValueType,
}

/// The actual value stored in a filter
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FilterValueType {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl FilterValueType {
    /// Get the value as a string for SQL binding (TEST-ONLY - use parameterized queries in production)
    #[cfg(test)]
    pub fn as_sql_value(&self) -> String {
        match self {
            FilterValueType::String(s) => s.clone(),
            FilterValueType::Integer(i) => i.to_string(),
            FilterValueType::Float(f) => f.to_string(),
            FilterValueType::Boolean(b) => b.to_string(),
        }
    }

    /// Get the value for parameterized query binding
    /// Returns the value as a type that can be used with sqlx query parameters
    pub fn as_bind_value(&self) -> Box<dyn std::any::Any + Send> {
        match self {
            FilterValueType::String(s) => Box::new(s.clone()),
            FilterValueType::Integer(i) => Box::new(*i),
            FilterValueType::Float(f) => Box::new(*f),
            FilterValueType::Boolean(b) => Box::new(*b),
        }
    }
}

/// Custom deserializer for FilterValue that parses operator.value format
impl<'de> serde::Deserialize<'de> for FilterValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct FilterValueVisitor;

        impl<'de> Visitor<'de> for FilterValueVisitor {
            type Value = FilterValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string in format 'operator.value' or just 'value'.")
            }

            fn visit_str<E>(self, value: &str) -> Result<FilterValue, E>
            where
                E: de::Error,
            {
                if let Some(dot_pos) = value.find('.') {
                    let operator_str = &value[..dot_pos];
                    let val = &value[dot_pos + 1..];

                    let operator = match operator_str {
                        "eq" => FilterOperator::Equal,
                        "gt" => FilterOperator::GreaterThan,
                        "gte" => FilterOperator::GreaterThanOrEqual,
                        "lt" => FilterOperator::LessThan,
                        "lte" => FilterOperator::LessThanOrEqual,
                        _ => {
                            return Err(de::Error::custom(format!(
                                "Unknown operator: {}",
                                operator_str
                            )));
                        }
                    };

                    // Parse the value based on the operator
                    let parsed_value = match operator {
                        FilterOperator::Equal => {
                            // For equality, try to parse as boolean first, then number, fallback to string
                            if let Ok(bool_val) = val.parse::<bool>() {
                                FilterValueType::Boolean(bool_val)
                            } else if let Ok(int_val) = val.parse::<i64>() {
                                FilterValueType::Integer(int_val)
                            } else if let Ok(float_val) = val.parse::<f64>() {
                                FilterValueType::Float(float_val)
                            } else {
                                // No validation needed with parameterized queries
                                FilterValueType::String(val.to_string())
                            }
                        }
                        FilterOperator::GreaterThan
                        | FilterOperator::GreaterThanOrEqual
                        | FilterOperator::LessThan
                        | FilterOperator::LessThanOrEqual => {
                            // For comparison operators, require numeric values
                            if let Ok(int_val) = val.parse::<i64>() {
                                FilterValueType::Integer(int_val)
                            } else if let Ok(float_val) = val.parse::<f64>() {
                                FilterValueType::Float(float_val)
                            } else {
                                return Err(de::Error::custom(format!(
                                    "Comparison operators (gt, gte, lt, lte) require numeric values, got: '{}'",
                                    val
                                )));
                            }
                        }
                    };

                    Ok(FilterValue {
                        operator,
                        value: parsed_value,
                    })
                } else {
                    // Default to equality if no operator specified
                    // Try to parse as boolean first, then number, fallback to string
                    let parsed_value = if let Ok(bool_val) = value.parse::<bool>() {
                        FilterValueType::Boolean(bool_val)
                    } else if let Ok(int_val) = value.parse::<i64>() {
                        FilterValueType::Integer(int_val)
                    } else if let Ok(float_val) = value.parse::<f64>() {
                        FilterValueType::Float(float_val)
                    } else {
                        // No validation needed with parameterized queries
                        FilterValueType::String(value.to_string())
                    };

                    Ok(FilterValue {
                        operator: FilterOperator::Equal,
                        value: parsed_value,
                    })
                }
            }
        }

        deserializer.deserialize_str(FilterValueVisitor)
    }
}

fn generate_column_concat(src_columns: &[String], prefix: &str) -> String {
    src_columns
        .iter()
        .map(|col| format!("COALESCE({prefix}.{col}, '')"))
        .collect::<Vec<String>>()
        .join(" || ' ' || ")
}

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
            src_columns TEXT[] NOT NULL,
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
        _ => panic!("Expected 'GIN' or 'GIST', got '{idx_type}' index type"),
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

pub fn create_fts_index_query(job_name: &str, idx_type: &str) -> String {
    check_input(job_name).expect("invalid job name");
    match idx_type.to_uppercase().as_str() {
        "GIN" | "GIST" => {} // Do nothing, it's valid
        _ => panic!("Expected 'GIN' or 'GIST', got '{idx_type}' index type"),
    }
    format!(
        "CREATE INDEX IF NOT EXISTS {job_name}_{idx_type}_idx ON vectorize._search_tokens_{job_name}
        USING {idx_type} (search_tokens);"
    )
}

pub fn update_search_tokens_trigger_queries(
    job_name: &str,
    join_key: &str,
    src_schema: &str,
    src_table: &str,
    src_columns: &[String],
) -> Vec<String> {
    let trigger_fn_name = format!("update_{job_name}_search_tokens");

    let new_cols = generate_column_concat(src_columns, "NEW");
    let old_cols = generate_column_concat(src_columns, "OLD");

    let trigger_dev = format!(
        "
CREATE OR REPLACE FUNCTION {trigger_fn_name}()
RETURNS TRIGGER AS $$
BEGIN
-- Handle INSERT and UPDATE operations
IF TG_OP = 'INSERT' THEN
    INSERT INTO vectorize._search_tokens_{job_name} ({join_key}, search_tokens)
    VALUES (
        NEW.{join_key},
        to_tsvector('english', {new_cols})
    )
    ON CONFLICT ({join_key}) DO UPDATE SET
        search_tokens = to_tsvector('english', {new_cols}),
        updated_at = CLOCK_TIMESTAMP()
    ;
    RETURN NEW;
END IF;

IF TG_OP = 'UPDATE' THEN
    IF {old_cols} IS DISTINCT FROM {new_cols} THEN
        INSERT INTO vectorize._search_tokens_{job_name} ({join_key}, search_tokens)
        VALUES (NEW.{join_key}, to_tsvector('english', {new_cols}))
        ON CONFLICT ({join_key}) DO UPDATE SET
            search_tokens = to_tsvector('english', {new_cols}),
            updated_at = CLOCK_TIMESTAMP();
    END IF;
    RETURN NEW;
END IF;

RETURN NULL;
END;
$$ LANGUAGE plpgsql;",
    );
    let apply_trigger = format!(
        "
        CREATE OR REPLACE TRIGGER {job_name}_search_tokens_trigger
        AFTER INSERT OR UPDATE OR DELETE ON {src_schema}.{src_table}
        FOR EACH ROW
        EXECUTE FUNCTION {trigger_fn_name}();"
    );
    vec![trigger_dev, apply_trigger]
}

/// creates a project view over a source table and the embeddings table
pub fn create_project_view(job_name: &str, schema: &str, relation: &str, pkey: &str) -> String {
    format!(
        "CREATE OR REPLACE VIEW vectorize.{job_name}_view as 
        SELECT t0.*, t1.embeddings, t1.updated_at as embeddings_updated_at
        FROM {schema}.{relation} t0
        INNER JOIN vectorize._embeddings_{job_name} t1
            ON t0.{pkey} = t1.{pkey};
        "
    )
}

pub fn create_search_tokens_table(
    job_name: &str,
    join_key: &str,
    join_key_type: &str,
    src_schema: &str,
    src_table: &str,
) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS vectorize._search_tokens_{job_name} (
            {join_key} {join_key_type} UNIQUE NOT NULL,
            search_tokens TSVECTOR NOT NULL,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
            FOREIGN KEY ({join_key}) REFERENCES {src_schema}.{src_table} ({join_key}) ON DELETE CASCADE
        );
        ",
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
    format!("DROP VIEW IF EXISTS vectorize.{job_name}_view;")
}

pub fn drop_embeddings_table(job_name: &str) -> String {
    format!("DROP TABLE IF EXISTS vectorize._embeddings_{job_name} CASCADE;")
}

pub fn drop_search_tokens_table(job_name: &str) -> String {
    format!("DROP TABLE IF EXISTS vectorize._search_tokens_{job_name} CASCADE;")
}

pub fn drop_trigger_handler(job_name: &str) -> String {
    format!("DROP FUNCTION IF EXISTS {TRIGGER_FN_PREFIX}{job_name}() CASCADE;")
}

pub fn drop_event_trigger(
    job_name: &str,
    src_schema: &str,
    src_table: &str,
    event: &str,
) -> String {
    format!(
        "DROP TRIGGER IF EXISTS vectorize_{event_name}_trigger_{job_name} ON {src_schema}.{src_table};",
        event_name = event.to_lowercase()
    )
}

pub fn drop_search_tokens_trigger(job_name: &str, src_schema: &str, src_table: &str) -> String {
    format!("DROP TRIGGER IF EXISTS {job_name}_search_tokens_trigger ON {src_schema}.{src_table};")
}

pub fn delete_job_record(job_name: &str) -> String {
    format!("DELETE FROM vectorize.job WHERE job_name = '{job_name}';")
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

pub fn handle_table_update() -> String {
    "CREATE OR REPLACE FUNCTION vectorize._handle_table_update(
    job_name text,
    record_ids text[]
) RETURNS void AS $$
DECLARE
    batch_size integer;
    batch_result RECORD;
    job_messages jsonb[] := '{}';
BEGIN
    -- create jobs of size batch_size
    batch_size := coalesce(
        current_setting('vectorize.batch_size', true)::integer,
        1000 -- default batch size
    );
    FOR batch_result IN SELECT batch FROM vectorize.batch_texts(record_ids, batch_size) LOOP
        -- only append non-null, non-empty batches
        IF array_length(batch_result.batch, 1) > 0 THEN
            job_messages := array_append(
                job_messages,
                jsonb_build_object(
                    'job_name', job_name,
                    'record_ids', batch_result.batch
                )
            );
        END IF;
    END LOOP;

    PERFORM pgmq.send_batch(
        queue_name=>'vectorize_jobs'::text,
        msgs=>job_messages::jsonb[])
    ;

END;
$$ LANGUAGE plpgsql;"
        .to_string()
}

pub fn create_batch_texts_fn() -> String {
    "CREATE OR REPLACE FUNCTION vectorize.batch_texts(
    record_ids text[],
    batch_size integer
) RETURNS TABLE(batch text[]) AS $$
DECLARE
    total_records integer;
    num_batches integer;
    i integer;
    start_idx integer;
    end_idx integer;
BEGIN
    total_records := array_length(record_ids, 1);
    
    -- Handle edge cases
    IF batch_size <= 0 OR total_records IS NULL OR total_records <= batch_size THEN
        RETURN QUERY SELECT record_ids;
        RETURN;
    END IF;
    
    -- Calculate number of batches needed
    num_batches := (total_records + batch_size - 1) / batch_size;
    
    -- Create batches
    FOR i IN 0..(num_batches - 1) LOOP
        start_idx := i * batch_size + 1; -- PostgreSQL arrays are 1-indexed
        end_idx := LEAST(start_idx + batch_size - 1, total_records);
        
        RETURN QUERY SELECT record_ids[start_idx:end_idx];
    END LOOP;
END;
$$ LANGUAGE plpgsql;"
        .to_string()
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

// generates query to fetch new rows have had data changed since last embedding generation
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
        .map(|s| format!("t0.{s}"))
        .collect::<Vec<_>>()
        .join(",");

    let base_query = format!(
        "
    SELECT t0.{pkey}::text as record_id, {cols} as input_text
    FROM {schema}.{table} t0
    LEFT JOIN vectorize._embeddings_{job_name} t1 ON t0.{pkey} = t1.{pkey}
    WHERE t1.{pkey} IS NULL"
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
    let rows: Result<Vec<PgRow>, Error> = sqlx::query(query).fetch_all(pool).await;
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

pub fn join_table_cosine_similarity(
    project: &str,
    schema: &str,
    table: &str,
    join_key: &str,
    return_columns: &[String],
    num_results: i32,
    filters: &BTreeMap<String, FilterValue>,
) -> String {
    let cols = &return_columns
        .iter()
        .map(|s| format!("t0.{s}"))
        .collect::<Vec<_>>()
        .join(",");

    let mut bind_value_counter: i16 = 2; // Start at $2 since $1 is the vector
    let mut where_filter = "WHERE 1=1".to_string();
    for (column, filter_value) in filters.iter() {
        let operator = filter_value.operator.to_sql();
        let filt = format!(" AND t0.\"{column}\" {operator} ${bind_value_counter}");
        where_filter.push_str(&filt);
        bind_value_counter += 1;
    }

    let inner_query = format!(
        "
    SELECT
        {join_key},
        1 - (embeddings <=> $1::vector) AS similarity_score
    FROM vectorize._embeddings_{project}
    ORDER BY similarity_score DESC
    "
    );
    format!(
        "
    SELECT to_jsonb(t) as results
    FROM (
        SELECT {cols}, t1.similarity_score
        FROM
            (
                {inner_query}
            ) t1
        INNER JOIN {schema}.{table} t0 on t0.{join_key} = t1.{join_key}
        {where_filter}
    ) t
    ORDER BY t.similarity_score DESC
    LIMIT {num_results};
    "
    )
}

#[allow(clippy::too_many_arguments)]
pub fn hybrid_search_query(
    job_name: &str,
    src_schema: &str,
    src_table: &str,
    join_key: &str,
    return_columns: &[String],
    window_size: i32,
    limit: i32,
    rrf_k: f32,
    semantic_weight: f32,
    fts_weight: f32,
    filters: &BTreeMap<String, FilterValue>,
) -> String {
    let cols = &return_columns
        .iter()
        .map(|s| format!("t0.{s}"))
        .collect::<Vec<_>>()
        .join(",");

    let mut bind_value_counter: i16 = 3;
    let mut where_filter = "WHERE 1=1".to_string();
    for (column, filter_value) in filters.iter() {
        let operator = filter_value.operator.to_sql();
        let filt = format!(" AND t0.\"{column}\" {operator} ${bind_value_counter}");
        where_filter.push_str(&filt);
        bind_value_counter += 1;
    }

    format!(
    "
    SELECT to_jsonb(t) as results 
    FROM (
        SELECT {cols}, t.rrf_score, t.semantic_rank, t.fts_rank, t.similarity_score
        FROM (
            SELECT
                COALESCE(s.{join_key}, f.{join_key}) as {join_key},
                s.semantic_rank,
                s.similarity_score,
                f.fts_rank,
                (
                    CASE
                        WHEN s.semantic_rank IS NOT NULL THEN {semantic_weight}::float/({rrf_k} + s.semantic_rank)
                        ELSE 0
                    END +
                    CASE
                        WHEN f.fts_rank IS NOT NULL THEN {fts_weight}::float/({rrf_k} + f.fts_rank)
                        ELSE 0
                    END
                ) as rrf_score
            FROM (
                SELECT
                    {join_key},
                    distance,
                    ROW_NUMBER() OVER (ORDER BY distance) as semantic_rank,
                    COUNT(*) OVER () as max_semantic_rank,
                    1 - distance as similarity_score
                FROM (
                    SELECT
                        {join_key},
                        embeddings <=> $1::vector as distance
                    FROM vectorize._embeddings_{job_name}
                ) sub
                ORDER BY distance
                LIMIT {window_size}
            ) s
            FULL OUTER JOIN (
                SELECT
                    {join_key},
                    ROW_NUMBER() OVER (ORDER BY ts_rank_cd(search_tokens, query) DESC) as fts_rank,
                    COUNT(*) OVER () as max_fts_rank
                FROM vectorize._search_tokens_{job_name}, 
                     to_tsquery('english', 
                         NULLIF(
                             replace(plainto_tsquery('english', $2)::text, ' & ', ' | '),
                             ''
                         )
                     ) as query
                WHERE search_tokens @@ query
                ORDER BY ts_rank_cd(search_tokens, query) DESC 
                LIMIT {window_size}
            ) f ON s.{join_key} = f.{join_key}
        ) t
        INNER JOIN {src_schema}.{src_table} t0 ON t0.{join_key} = t.{join_key}
        {where_filter}
        ORDER BY t.rrf_score DESC
        LIMIT {limit}
    ) t"
)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_create_update_trigger_single() {
        let job_name = "another_job";
        let table_name = "another_table";

        let expected = "
CREATE OR REPLACE TRIGGER vectorize_update_trigger_another_job
AFTER UPDATE ON myschema.another_table
REFERENCING NEW TABLE AS new_table
FOR EACH STATEMENT
EXECUTE FUNCTION vectorize.handle_update_another_job();"
            .to_string();
        let result = create_event_trigger(job_name, "myschema", table_name, "UPDATE");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_create_insert_trigger_single() {
        let job_name = "another_job";
        let table_name = "another_table";

        let expected = "
CREATE OR REPLACE TRIGGER vectorize_insert_trigger_another_job
AFTER INSERT ON myschema.another_table
REFERENCING NEW TABLE AS new_table
FOR EACH STATEMENT
EXECUTE FUNCTION vectorize.handle_update_another_job();"
            .to_string();
        let result = create_event_trigger(job_name, "myschema", table_name, "INSERT");
        assert_eq!(expected, result);
    }

    // ===== FilterValue Deserialization Tests =====

    #[test]
    fn test_filter_value_deserialize_equality_string() {
        let json = "\"eq.hello\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "hello");
    }

    #[test]
    fn test_filter_value_deserialize_equality_integer() {
        let json = "\"eq.42\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "42");
    }

    #[test]
    fn test_filter_value_deserialize_equality_float() {
        let json = "\"eq.3.14\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "3.14");
    }

    #[test]
    fn test_filter_value_deserialize_greater_than() {
        let json = "\"gt.100\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::GreaterThan);
        assert_eq!(filter.value.as_sql_value(), "100");
    }

    #[test]
    fn test_filter_value_deserialize_greater_than_or_equal() {
        let json = "\"gte.50.5\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::GreaterThanOrEqual);
        assert_eq!(filter.value.as_sql_value(), "50.5");
    }

    #[test]
    fn test_filter_value_deserialize_less_than() {
        let json = "\"lt.25\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::LessThan);
        assert_eq!(filter.value.as_sql_value(), "25");
    }

    #[test]
    fn test_filter_value_deserialize_less_than_or_equal() {
        let json = "\"lte.10.0\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::LessThanOrEqual);
        assert_eq!(filter.value.as_sql_value(), "10");
    }

    #[test]
    fn test_filter_value_deserialize_default_equality() {
        let json = "\"hello\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "hello");
    }

    #[test]
    fn test_filter_value_deserialize_default_equality_numeric() {
        let json = "\"42\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "42");
    }

    // ===== Edge Case Tests =====

    #[test]
    fn test_filter_value_deserialize_empty_string() {
        let json = "\"eq.\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "");
    }

    #[test]
    fn test_filter_value_deserialize_zero_values() {
        let json_int = "\"eq.0\"";
        let filter_int: FilterValue = serde_json::from_str(json_int).unwrap();
        assert_eq!(filter_int.operator, FilterOperator::Equal);
        assert_eq!(filter_int.value.as_sql_value(), "0");

        let json_float = "\"eq.0.0\"";
        let filter_float: FilterValue = serde_json::from_str(json_float).unwrap();
        assert_eq!(filter_float.operator, FilterOperator::Equal);
        assert_eq!(filter_float.value.as_sql_value(), "0");
    }

    #[test]
    fn test_filter_value_deserialize_negative_values() {
        let json_int = "\"eq.-42\"";
        let filter_int: FilterValue = serde_json::from_str(json_int).unwrap();
        assert_eq!(filter_int.operator, FilterOperator::Equal);
        assert_eq!(filter_int.value.as_sql_value(), "-42");

        let json_float = "\"eq.-3.14\"";
        let filter_float: FilterValue = serde_json::from_str(json_float).unwrap();
        assert_eq!(filter_float.operator, FilterOperator::Equal);
        assert_eq!(filter_float.value.as_sql_value(), "-3.14");
    }

    #[test]
    fn test_filter_value_deserialize_special_characters() {
        let json = "\"eq.hello-world_123\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "hello-world_123");
    }

    #[test]
    fn test_filter_value_deserialize_unicode_characters() {
        let json = "\"eq.测试\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "测试");
    }

    #[test]
    fn test_filter_value_deserialize_whitespace_values() {
        let json = "\"eq. hello \"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), " hello ");
    }

    #[test]
    fn test_filter_value_deserialize_scientific_notation() {
        let json = "\"eq.1e5\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "100000");
    }

    #[test]
    fn test_filter_value_deserialize_large_numbers() {
        let json = "\"eq.9223372036854775807\""; // i64::MAX
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "9223372036854775807");
    }

    #[test]
    fn test_filter_value_deserialize_precision_float() {
        let json = "\"eq.3.141592653589793\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "3.141592653589793");
    }

    // ===== Error Handling Tests =====

    #[test]
    fn test_filter_value_deserialize_invalid_operator() {
        let json = "\"invalid.42\"";
        let result: Result<FilterValue, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Should fail for invalid operator");
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Unknown operator"));
    }

    #[test]
    fn test_filter_value_deserialize_comparison_with_string() {
        // Test that comparison operators fail with non-numeric values
        let test_cases = vec![
            ("gt", "hello"),
            ("gte", "world"),
            ("lt", "test"),
            ("lte", "string"),
        ];

        for (op, value) in test_cases {
            let json = format!("\"{}.{}\"", op, value);
            let result: Result<FilterValue, _> = serde_json::from_str(&json);
            assert!(
                result.is_err(),
                "Should fail for non-numeric value with {} operator",
                op
            );
            let error = result.unwrap_err();
            assert!(error.to_string().contains("require numeric values"));
        }
    }

    #[test]
    fn test_filter_value_deserialize_malformed_json() {
        // Test various malformed JSON inputs
        let malformed_inputs = vec![
            ("\"eq.42", false),        // Missing closing quote
            ("eq.42\"", false),        // Missing opening quote
            ("\"eq.42\"", true),       // This should work
            ("\"eq.42.extra\"", true), // Extra dot should work as string
            ("\"eq.\"", true),         // Empty value should work
            ("\".42\"", false),        // Missing operator should fail
            ("\"eq\"", true),          // Missing dot and value should work as string
        ];

        for (input, should_succeed) in malformed_inputs {
            let result: Result<FilterValue, _> = serde_json::from_str(input);
            if should_succeed {
                assert!(result.is_ok(), "Should succeed for input: {}", input);
            } else {
                assert!(
                    result.is_err(),
                    "Should fail for malformed input: {}",
                    input
                );
            }
        }
    }

    #[test]
    fn test_filter_value_deserialize_empty_input() {
        let json = "\"\"";
        let result: Result<FilterValue, _> = serde_json::from_str(json);
        // Empty input should succeed and default to equality with empty string
        assert!(result.is_ok(), "Empty input should succeed");
        let filter = result.unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "");
    }

    #[test]
    fn test_filter_value_deserialize_just_dot() {
        let json = "\".\"";
        let result: Result<FilterValue, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Should fail for input with just a dot");
    }

    #[test]
    fn test_filter_value_deserialize_multiple_dots() {
        let json = "\"eq.42.extra\"";
        let result: Result<FilterValue, _> = serde_json::from_str(json);
        // Multiple dots should succeed and treat the whole thing as a string
        assert!(
            result.is_ok(),
            "Should succeed for input with multiple dots"
        );
        let filter = result.unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value.as_sql_value(), "42.extra");
    }

    #[test]
    fn test_filter_value_deserialize_case_sensitive_operators() {
        // Test that operators are case sensitive
        let case_variations = vec!["EQ.42", "GT.42", "GTE.42", "LT.42", "LTE.42"];

        for input in case_variations {
            let json = format!("\"{}\"", input);
            let result: Result<FilterValue, _> = serde_json::from_str(&json);
            assert!(
                result.is_err(),
                "Should fail for uppercase operator: {}",
                input
            );
        }
    }

    #[test]
    fn test_filter_value_deserialize_whitespace_in_operator() {
        // Test operators with whitespace - these actually succeed as they're treated as strings
        let whitespace_inputs = vec![
            ("\" eq.42\"", false), // Leading space - fails
            ("\"eq .42\"", false), // Space before dot - fails
            ("\"eq. 42\"", true),  // Space after dot - succeeds as string
            ("\"eq.42 \"", true),  // Trailing space - succeeds as string
        ];

        for (input, should_succeed) in whitespace_inputs {
            let result: Result<FilterValue, _> = serde_json::from_str(input);
            if should_succeed {
                assert!(result.is_ok(), "Should succeed for input: {}", input);
            } else {
                assert!(result.is_err(), "Should fail for input: {}", input);
            }
        }
    }

    // ===== Numeric Parsing Edge Cases =====

    #[test]
    fn test_filter_value_deserialize_numeric_boundaries() {
        // Test integer boundaries
        let json_max_i64 = "\"eq.9223372036854775807\""; // i64::MAX
        let filter_max: FilterValue = serde_json::from_str(json_max_i64).unwrap();
        assert_eq!(filter_max.value.as_sql_value(), "9223372036854775807");

        let json_min_i64 = "\"eq.-9223372036854775808\""; // i64::MIN
        let filter_min: FilterValue = serde_json::from_str(json_min_i64).unwrap();
        assert_eq!(filter_min.value.as_sql_value(), "-9223372036854775808");
    }

    #[test]
    fn test_filter_value_deserialize_float_precision() {
        // Test various float precision cases
        let test_cases = vec![
            ("0.0", "0"),
            ("0.1", "0.1"),
            ("0.01", "0.01"),
            ("0.001", "0.001"),
            ("1.0", "1"),
            ("1.1", "1.1"),
            ("1.11", "1.11"),
            ("1.111", "1.111"),
        ];

        for (input, expected) in test_cases {
            let json = format!("\"eq.{}\"", input);
            let filter: FilterValue = serde_json::from_str(&json).unwrap();
            assert_eq!(
                filter.value.as_sql_value(),
                expected,
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_filter_value_deserialize_scientific_notation_edge_cases() {
        // Test scientific notation edge cases
        let test_cases = vec![
            ("1e0", "1"),
            ("1e1", "10"),
            ("1e-1", "0.1"),
            ("1e-10", "0.0000000001"),
            ("1e10", "10000000000"),
            ("1.5e2", "150"),
            ("1.5e-2", "0.015"),
        ];

        for (input, expected) in test_cases {
            let json = format!("\"eq.{}\"", input);
            let filter: FilterValue = serde_json::from_str(&json).unwrap();
            assert_eq!(
                filter.value.as_sql_value(),
                expected,
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_filter_value_deserialize_hex_numbers() {
        // Test that hex numbers are treated as strings (not parsed as integers)
        let json = "\"eq.0xFF\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "0xFF");
    }

    #[test]
    fn test_filter_value_deserialize_octal_numbers() {
        // Test that octal numbers are parsed as integers
        let json = "\"eq.0777\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "777");
    }

    #[test]
    fn test_filter_value_deserialize_binary_numbers() {
        // Test that binary numbers are treated as strings
        let json = "\"eq.0b1010\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "0b1010");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_with_leading_zeros() {
        // Test numbers with leading zeros
        let json = "\"eq.007\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "7");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_with_trailing_zeros() {
        // Test numbers with trailing zeros
        let json = "\"eq.42.000\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "42");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_with_plus_sign() {
        // Test numbers with explicit plus sign
        let json = "\"+42\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "42");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_with_plus_sign_float() {
        // Test floats with explicit plus sign (should work as default equality)
        let json = "\"+3.14\"";
        let result: Result<FilterValue, _> = serde_json::from_str(json);
        // This should fail because "+3" is not a valid operator
        assert!(
            result.is_err(),
            "Should fail for input with plus sign as operator"
        );
    }

    #[test]
    fn test_filter_value_deserialize_numeric_infinity() {
        // Test infinity values (should be parsed as float infinity)
        let json = "\"eq.infinity\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.value.as_sql_value(), "inf");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_nan() {
        // Test NaN values (should be parsed as float NaN)
        let json = "\"eq.nan\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        // NaN comparison requires special handling
        match filter.value {
            FilterValueType::Float(f) => assert!(f.is_nan(), "Expected NaN"),
            _ => panic!("Expected Float(NaN)"),
        }
        assert_eq!(filter.value.as_sql_value(), "NaN");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_very_small() {
        // Test very small numbers
        let json = "\"eq.0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        // Very small numbers get parsed as floats and converted to "0" when using to_string()
        assert_eq!(filter.value.as_sql_value(), "0");
    }

    #[test]
    fn test_filter_value_deserialize_numeric_very_large() {
        // Test very large numbers (using f64::MAX as a reasonable upper bound)
        let json = "\"eq.1.7976931348623157e308\""; // f64::MAX
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        // Very large numbers get parsed as floats and converted to a long decimal string when using to_string()
        assert_eq!(
            filter.value.as_sql_value(),
            "179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    // ===== Boolean Filter Value Tests =====

    #[test]
    fn test_filter_value_deserialize_boolean_true() {
        let json = "\"eq.true\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValueType::Boolean(true));
        assert_eq!(filter.value.as_sql_value(), "true");
    }

    #[test]
    fn test_filter_value_deserialize_boolean_false() {
        let json = "\"eq.false\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValueType::Boolean(false));
        assert_eq!(filter.value.as_sql_value(), "false");
    }

    #[test]
    fn test_filter_value_deserialize_boolean_default_true() {
        let json = "\"true\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValueType::Boolean(true));
        assert_eq!(filter.value.as_sql_value(), "true");
    }

    #[test]
    fn test_filter_value_deserialize_boolean_default_false() {
        let json = "\"false\"";
        let filter: FilterValue = serde_json::from_str(json).unwrap();
        assert_eq!(filter.operator, FilterOperator::Equal);
        assert_eq!(filter.value, FilterValueType::Boolean(false));
        assert_eq!(filter.value.as_sql_value(), "false");
    }

    #[test]
    fn test_filter_value_deserialize_boolean_case_sensitive() {
        // Test that boolean parsing is case sensitive - uppercase values become strings
        let test_cases = vec![
            ("\"eq.True\"", FilterValueType::String("True".to_string())),
            ("\"eq.False\"", FilterValueType::String("False".to_string())),
            ("\"eq.TRUE\"", FilterValueType::String("TRUE".to_string())),
            ("\"eq.FALSE\"", FilterValueType::String("FALSE".to_string())),
            ("\"eq.true\"", FilterValueType::Boolean(true)),
            ("\"eq.false\"", FilterValueType::Boolean(false)),
        ];

        for (input, expected_value) in test_cases {
            let filter: FilterValue = serde_json::from_str(input).unwrap();
            assert_eq!(filter.value, expected_value);
        }
    }

    #[test]
    fn test_filter_value_deserialize_boolean_with_whitespace() {
        // Test boolean parsing with whitespace
        let test_cases = vec![
            ("\"eq. true\"", true),  // Space before true - should succeed as string
            ("\"eq.false \"", true), // Space after false - should succeed as string
            ("\"eq. true \"", true), // Spaces around true - should succeed as string
        ];

        for (input, should_succeed) in test_cases {
            let result: Result<FilterValue, _> = serde_json::from_str(input);
            if should_succeed {
                assert!(result.is_ok(), "Should succeed for input: {}", input);
                let filter = result.unwrap();
                // With whitespace, it should be parsed as a string, not boolean
                assert!(matches!(filter.value, FilterValueType::String(_)));
            } else {
                assert!(result.is_err(), "Should fail for input: {}", input);
            }
        }
    }

    #[test]
    fn test_filter_value_deserialize_boolean_vs_string() {
        // Test that "true" and "false" strings are not parsed as booleans
        let test_cases = vec![
            (
                "\"eq.true_string\"",
                FilterValueType::String("true_string".to_string()),
            ),
            (
                "\"eq.false_string\"",
                FilterValueType::String("false_string".to_string()),
            ),
            (
                "\"eq.true123\"",
                FilterValueType::String("true123".to_string()),
            ),
            (
                "\"eq.false456\"",
                FilterValueType::String("false456".to_string()),
            ),
        ];

        for (input, expected_value) in test_cases {
            let filter: FilterValue = serde_json::from_str(input).unwrap();
            assert_eq!(filter.value, expected_value);
        }
    }

    #[test]
    fn test_filter_value_deserialize_boolean_vs_numeric() {
        // Test that numeric values are still parsed as numbers, not booleans
        let test_cases = vec![
            ("\"eq.1\"", FilterValueType::Integer(1)),
            ("\"eq.0\"", FilterValueType::Integer(0)),
            ("\"eq.1.0\"", FilterValueType::Float(1.0)),
            ("\"eq.0.0\"", FilterValueType::Float(0.0)),
        ];

        for (input, expected_value) in test_cases {
            let filter: FilterValue = serde_json::from_str(input).unwrap();
            assert_eq!(filter.value, expected_value);
        }
    }

    #[test]
    fn test_filter_value_deserialize_boolean_comparison_operators() {
        // Test that comparison operators with boolean values fail (as they should require numeric values)
        let test_cases = vec!["gt.true", "gte.false", "lt.true", "lte.false"];

        for input in test_cases {
            let json = format!("\"{}\"", input);
            let result: Result<FilterValue, _> = serde_json::from_str(&json);
            assert!(
                result.is_err(),
                "Should fail for boolean value with comparison operator: {}",
                input
            );
            let error = result.unwrap_err();
            assert!(error.to_string().contains("require numeric values"));
        }
    }

    #[test]
    fn test_filter_value_deserialize_boolean_edge_cases() {
        // Test edge cases for boolean parsing
        let test_cases = vec![
            ("\"eq.true\"", FilterValueType::Boolean(true)),
            ("\"eq.false\"", FilterValueType::Boolean(false)),
            ("\"true\"", FilterValueType::Boolean(true)),
            ("\"false\"", FilterValueType::Boolean(false)),
        ];

        for (input, expected_value) in test_cases {
            let filter: FilterValue = serde_json::from_str(input).unwrap();
            assert_eq!(filter.value, expected_value);
            assert_eq!(filter.operator, FilterOperator::Equal);
        }
    }

    #[test]
    fn test_drop_embeddings_table() {
        let job_name = "test_job";
        let result = drop_embeddings_table(job_name);
        assert_eq!(
            result,
            "DROP TABLE IF EXISTS vectorize._embeddings_test_job CASCADE;"
        );
    }

    #[test]
    fn test_drop_search_tokens_table() {
        let job_name = "test_job";
        let result = drop_search_tokens_table(job_name);
        assert_eq!(
            result,
            "DROP TABLE IF EXISTS vectorize._search_tokens_test_job CASCADE;"
        );
    }

    #[test]
    fn test_drop_trigger_handler() {
        let job_name = "test_job";
        let result = drop_trigger_handler(job_name);
        assert!(result.contains("DROP FUNCTION IF EXISTS"));
        assert!(result.contains("vectorize.handle_update_test_job()"));
        assert!(result.contains("CASCADE"));
    }

    #[test]
    fn test_drop_event_trigger() {
        let job_name = "test_job";
        let src_schema = "public";
        let src_table = "my_table";

        let insert_trigger = drop_event_trigger(job_name, src_schema, src_table, "INSERT");
        assert_eq!(
            insert_trigger,
            "DROP TRIGGER IF EXISTS vectorize_insert_trigger_test_job ON public.my_table;"
        );

        let update_trigger = drop_event_trigger(job_name, src_schema, src_table, "UPDATE");
        assert_eq!(
            update_trigger,
            "DROP TRIGGER IF EXISTS vectorize_update_trigger_test_job ON public.my_table;"
        );
    }

    #[test]
    fn test_drop_search_tokens_trigger() {
        let job_name = "test_job";
        let src_schema = "public";
        let src_table = "my_table";

        let result = drop_search_tokens_trigger(job_name, src_schema, src_table);
        assert_eq!(
            result,
            "DROP TRIGGER IF EXISTS test_job_search_tokens_trigger ON public.my_table;"
        );
    }

    #[test]
    fn test_delete_job_record() {
        let job_name = "test_job";
        let result = delete_job_record(job_name);
        assert_eq!(
            result,
            "DELETE FROM vectorize.job WHERE job_name = 'test_job';"
        );
    }

    #[test]
    fn test_drop_project_view() {
        let job_name = "test_job";
        let result = drop_project_view(job_name);
        assert_eq!(result, "DROP VIEW IF EXISTS vectorize.test_job_view;");
    }

    #[test]
    fn test_cleanup_sql_with_special_chars() {
        // Test that job names with underscores work correctly
        let job_name = "my_test_job_123";

        let embeddings = drop_embeddings_table(job_name);
        assert!(embeddings.contains("_embeddings_my_test_job_123"));

        let tokens = drop_search_tokens_table(job_name);
        assert!(tokens.contains("_search_tokens_my_test_job_123"));

        let view = drop_project_view(job_name);
        assert!(view.contains("my_test_job_123_view"));
    }
}
