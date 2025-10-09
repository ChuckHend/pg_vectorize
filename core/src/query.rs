use crate::transformers::types::Inputs;
use crate::types::{self, JobParams};
use anyhow::{Result, anyhow};
use serde::Serialize;
use sqlx::error::Error;
use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row};
use std::collections::BTreeMap;
use tiktoken_rs::cl100k_base;

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

/// The actual value stored in a filter - can be string or numeric
#[derive(Debug, Clone, Serialize)]
pub enum FilterValueType {
    String(String),
    Integer(i64),
    Float(f64),
}

impl FilterValueType {
    /// Get the value as a string for SQL binding
    pub fn as_sql_value(&self) -> String {
        match self {
            FilterValueType::String(s) => s.clone(),
            FilterValueType::Integer(i) => i.to_string(),
            FilterValueType::Float(f) => f.to_string(),
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
                formatter.write_str("a string in format 'operator.value' or just 'value'")
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
                            // For equality, try to parse as number first, fallback to string
                            if let Ok(int_val) = val.parse::<i64>() {
                                FilterValueType::Integer(int_val)
                            } else if let Ok(float_val) = val.parse::<f64>() {
                                FilterValueType::Float(float_val)
                            } else {
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
                    // Try to parse as number first, fallback to string
                    let parsed_value = if let Ok(int_val) = value.parse::<i64>() {
                        FilterValueType::Integer(int_val)
                    } else if let Ok(float_val) = value.parse::<f64>() {
                        FilterValueType::Float(float_val)
                    } else {
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
pub const VECTORIZE_SCHEMA: &str = "vectorize";
static TRIGGER_FN_PREFIX: &str = "vectorize.handle_update_";

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
    where_clause: Option<String>,
) -> String {
    let cols = &return_columns
        .iter()
        .map(|s| format!("t0.{s}"))
        .collect::<Vec<_>>()
        .join(",");

    let where_str = if let Some(w) = where_clause {
        prepare_filter(&w, join_key)
    } else {
        "".to_string()
    };
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
        {where_str}
    ) t
    ORDER BY t.similarity_score DESC
    LIMIT {num_results};
    "
    )
}

// transform user's where_sql into the format search query expects
fn prepare_filter(filter: &str, pkey: &str) -> String {
    let wc = filter.replace(pkey, &format!("t0.{pkey}"));
    format!("AND {wc}")
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
}
