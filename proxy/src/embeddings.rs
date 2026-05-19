use anyhow::Result;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

static SEARCH_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)vectorize\.search\s*\(((?:'(?:[^']|'')*'|[^)])*)\)").unwrap()
});
static SEARCH_JOB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)job\s*=>\s*'((?:[^']|'')*)'").unwrap());
static SEARCH_QUERY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)query\s*=>\s*'((?:[^']|'')*)'").unwrap());
static SEARCH_NUM_RESULTS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:num_results|limit)\s*=>\s*(\d+)").unwrap());
static EMBED_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)vectorize\.embed\s*\(\s*'([^']*(?:''[^']*)*)'\s*,\s*'([^']*(?:''[^']*)*)'\s*\)",
    )
    .unwrap()
});
static EMBED_PARAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)vectorize\.embed\s*\(\s*\$(\d+)\s*,\s*\$(\d+)\s*\)").unwrap()
});
use std::sync::Arc;

use vectorize_core::errors::VectorizeError;
use vectorize_core::query::hybrid_search_query_rows;
use vectorize_core::transformers::providers::{self, prepare_generic_embedding_request};
use vectorize_core::transformers::types::Inputs;
use vectorize_core::types::VectorizeJob;

/// Represents a parsed vectorize.embed() function call
#[derive(Debug, Clone)]
pub struct EmbedCall {
    pub query: String,
    pub project_name: String,
    pub full_match: String,
    pub start_pos: usize,
    pub end_pos: usize,
    pub is_prepared: bool,
    pub query_param_index: Option<usize>,
    pub project_param_index: Option<usize>,
}

/// Embedding provider that uses the jobmap cache to determine the appropriate embedding provider
/// based on the project name from vectorize.embed() calls
#[derive(Clone)]
pub struct JobMapEmbeddingProvider {
    pub jobmap: Arc<HashMap<String, VectorizeJob>>,
}

impl JobMapEmbeddingProvider {
    pub fn new(jobmap: Arc<HashMap<String, VectorizeJob>>) -> Self {
        Self { jobmap }
    }

    pub async fn generate_embeddings(
        &self,
        query: &str,
        project_name: &str,
    ) -> Result<Vec<f64>, VectorizeError> {
        // Look up the project in the jobmap cache
        let vectorize_job = self.jobmap.get(project_name).ok_or_else(|| {
            VectorizeError::JobNotFound(format!(
                "Project '{project_name}' not found in jobmap cache"
            ))
        })?;

        // Get the provider based on the model source using the proper get_provider function
        let provider = providers::get_provider(&vectorize_job.model.source, None, None, None)?;

        // Create input for embedding generation
        let input = Inputs {
            record_id: String::new(),
            inputs: query.to_string(),
            token_estimate: 0,
        };

        let embedding_request = prepare_generic_embedding_request(&vectorize_job.model, &[input]);
        let response = provider.generate_embedding(&embedding_request).await?;
        response.embeddings.into_iter().next().ok_or_else(|| {
            VectorizeError::EmbeddingGenerationFailed("No embeddings returned".to_string())
        })
    }
}

/// Represents a parsed vectorize.search() named-argument function call
#[derive(Debug, Clone)]
pub struct SearchCall {
    pub job_name: String,
    pub query: String,
    pub num_results: i32,
    pub full_match: String,
    pub start_pos: usize,
    pub end_pos: usize,
}

/// Parses `vectorize.search(job=>'...', query=>'...')` calls from SQL.
/// Only named-argument syntax is supported.
pub fn parse_search_calls(sql: &str) -> Result<Vec<SearchCall>> {
    let mut calls = Vec::new();

    for mat in SEARCH_CALL_RE.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let args_str = SEARCH_CALL_RE
            .captures(mat.as_str())
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        let job_name = SEARCH_JOB_RE
            .captures(args_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().replace("''", "'"))
            .ok_or_else(|| anyhow::anyhow!("Missing 'job' parameter in vectorize.search()"))?;

        let query = SEARCH_QUERY_RE
            .captures(args_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().replace("''", "'"))
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter in vectorize.search()"))?;

        let num_results = SEARCH_NUM_RESULTS_RE
            .captures(args_str)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(10i32);

        calls.push(SearchCall {
            job_name,
            query,
            num_results,
            full_match,
            start_pos: mat.start(),
            end_pos: mat.end(),
        });
    }

    Ok(calls)
}

/// Detects `vectorize.search()` calls in SQL and rewrites the entire query to the
/// underlying hybrid search SQL with the embedding vector inlined.
/// Returns `Ok(None)` if no search calls are found.
pub async fn rewrite_search_query(
    sql: &str,
    provider: &JobMapEmbeddingProvider,
) -> Result<Option<String>, VectorizeError> {
    let search_calls = parse_search_calls(sql).map_err(|e| {
        VectorizeError::EmbeddingGenerationFailed(format!("Failed to parse search calls: {e}"))
    })?;

    if search_calls.is_empty() {
        return Ok(None);
    }

    // Handle the first call (the common case; multiple search calls in one query are unusual)
    let call = &search_calls[0];

    let vectorize_job = provider.jobmap.get(&call.job_name).ok_or_else(|| {
        VectorizeError::JobNotFound(format!("Job '{}' not found in proxy cache", call.job_name))
    })?;

    let embeddings = provider
        .generate_embeddings(&call.query, &call.job_name)
        .await?;
    let embedding_literal = format_embeddings_as_vector(&embeddings);

    let window_size = 5 * call.num_results;
    let template_sql = hybrid_search_query_rows(
        &call.job_name, // vectorize_job.job_name was cleared by mem::take in cache load
        &vectorize_job.src_schema,
        &vectorize_job.src_table,
        &vectorize_job.primary_key,
        &["*".to_string()],
        window_size,
        call.num_results,
        60.0,
        1.0,
        1.0,
        &BTreeMap::new(),
    );

    // Inline the sqlx bind parameter placeholders with their actual values.
    // $1::vector is the embedding; $2 is the raw text for the FTS plainto_tsquery.
    // With no filters, these are the only two bind params in the generated SQL.
    let escaped_query = call.query.replace('\'', "''");
    let query_literal = format!("'{escaped_query}'");
    let inlined_sql = template_sql
        .replace("$1::vector", &embedding_literal)
        .replace("$2", &query_literal);

    // Splice the subquery in place of `vectorize.search(...)`, keeping any outer
    // SELECT column list, WHERE, ORDER BY, or LIMIT the caller wrote.
    let subquery = format!("({inlined_sql}\n    ) AS _vectorize_search");
    let mut rewritten = sql.to_string();
    rewritten.replace_range(call.start_pos..call.end_pos, &subquery);
    Ok(Some(rewritten))
}

pub fn parse_embed_calls(sql: &str) -> Result<Vec<EmbedCall>> {
    let mut calls = Vec::new();

    // Parse string literal calls
    for mat in EMBED_STRING_RE.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let start_pos = mat.start();
        let end_pos = mat.end();

        if let Some(captures) = EMBED_STRING_RE.captures(&full_match) {
            let query = captures.get(1).unwrap().as_str().replace("''", "'");
            let project_name = captures.get(2).unwrap().as_str().replace("''", "'");

            calls.push(EmbedCall {
                query,
                project_name,
                full_match,
                start_pos,
                end_pos,
                is_prepared: false,
                query_param_index: None,
                project_param_index: None,
            });
        }
    }

    // parse prepared statement parameter calls
    for mat in EMBED_PARAM_RE.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let start_pos = mat.start();
        let end_pos = mat.end();

        if let Some(captures) = EMBED_PARAM_RE.captures(&full_match) {
            // convert 1-based indices to 0-based (e.g. bind parameters from $1 -> 0)
            let query_param_index = captures.get(1).unwrap().as_str().parse::<usize>()? - 1;
            let project_param_index = captures.get(2).unwrap().as_str().parse::<usize>()? - 1;

            calls.push(EmbedCall {
                query: String::new(),        // filled from bind parameters
                project_name: String::new(), // filled from bind parameters
                full_match,
                start_pos,
                end_pos,
                is_prepared: true,
                query_param_index: Some(query_param_index),
                project_param_index: Some(project_param_index),
            });
        }
    }

    Ok(calls)
}

/// resolves prepared statement parameters in embed calls
pub fn resolve_prepared_embed_calls(
    mut embed_calls: Vec<EmbedCall>,
    parameters: &[String],
) -> Result<Vec<EmbedCall>, VectorizeError> {
    for call in &mut embed_calls {
        if call.is_prepared
            && let (Some(query_idx), Some(project_idx)) =
                (call.query_param_index, call.project_param_index)
        {
            if query_idx >= parameters.len() || project_idx >= parameters.len() {
                return Err(VectorizeError::EmbeddingGenerationFailed(format!(
                    "Parameter index out of bounds: query_idx={}, project_idx={}, params_len={}",
                    query_idx,
                    project_idx,
                    parameters.len()
                )));
            }
            call.query = parameters[query_idx].clone();
            call.project_name = parameters[project_idx].clone();
        }
    }
    Ok(embed_calls)
}

/// rewrites SQL query by replacing vectorize.embed() calls with actual embeddings
pub async fn rewrite_query_with_embeddings(
    sql: &str,
    provider: &JobMapEmbeddingProvider,
) -> Result<String, VectorizeError> {
    let embed_calls = parse_embed_calls(sql).map_err(|e| {
        VectorizeError::EmbeddingGenerationFailed(format!("Failed to parse embed calls: {e}"))
    })?;

    if embed_calls.is_empty() {
        return Ok(sql.to_string());
    }

    let mut rewritten = sql.to_string();

    // process calls in reverse order to maintain correct positions
    for call in embed_calls.iter().rev() {
        let embeddings = provider
            .generate_embeddings(&call.query, &call.project_name)
            .await?;
        let embedding_str = format_embeddings_as_vector(&embeddings);
        rewritten.replace_range(call.start_pos..call.end_pos, &embedding_str);
    }

    Ok(rewritten)
}

fn format_embeddings_as_vector(embeddings: &[f64]) -> String {
    let values: Vec<String> = embeddings.iter().map(|x| x.to_string()).collect();
    format!("'[{}]'::vector", values.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_embed_calls() {
        let sql = "SELECT * FROM vectorize.embed('hello world', 'my_project')";
        let calls = parse_embed_calls(sql).unwrap();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].query, "hello world");
        assert_eq!(calls[0].project_name, "my_project");
        assert!(!calls[0].is_prepared);
    }

    #[test]
    fn test_parse_multiple_embed_calls() {
        let sql =
            "SELECT vectorize.embed('query1', 'project1'), vectorize.embed('query2', 'project2')";
        let calls = parse_embed_calls(sql).unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].query, "query1");
        assert_eq!(calls[0].project_name, "project1");
        assert!(!calls[0].is_prepared);
        assert_eq!(calls[1].query, "query2");
        assert_eq!(calls[1].project_name, "project2");
        assert!(!calls[1].is_prepared);
    }

    #[test]
    fn test_parse_prepared_embed_calls() {
        let sql = "SELECT vectorize.embed($1, $2)";
        let calls = parse_embed_calls(sql).unwrap();

        assert_eq!(calls.len(), 1);
        assert!(calls[0].is_prepared);
        assert_eq!(calls[0].query_param_index, Some(0));
        assert_eq!(calls[0].project_param_index, Some(1));
    }

    #[test]
    fn test_resolve_prepared_embed_calls() {
        let sql = "SELECT vectorize.embed($1, $2)";
        let mut calls = parse_embed_calls(sql).unwrap();
        let parameters = vec!["hello world".to_string(), "my_project".to_string()];

        calls = resolve_prepared_embed_calls(calls, &parameters).unwrap();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].query, "hello world");
        assert_eq!(calls[0].project_name, "my_project");
    }

    #[test]
    fn test_no_embed_calls() {
        let sql = "SELECT * FROM documents WHERE id = 1";
        let calls = parse_embed_calls(sql).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_search_calls_basic() {
        let sql = "SELECT * FROM vectorize.search(job=>'my_job', query=>'camping backpack')";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].job_name, "my_job");
        assert_eq!(calls[0].query, "camping backpack");
        assert_eq!(calls[0].num_results, 10);
    }

    #[test]
    fn test_parse_search_calls_with_num_results() {
        let sql = "SELECT * FROM vectorize.search(job=>'my_job', query=>'camping backpack', num_results=>5)";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].num_results, 5);
    }

    #[test]
    fn test_parse_search_calls_with_limit_alias() {
        let sql =
            "SELECT * FROM vectorize.search(job=>'my_job', query=>'camping backpack', limit=>3)";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].num_results, 3);
    }

    #[test]
    fn test_parse_search_calls_query_first() {
        let sql = "SELECT * FROM vectorize.search(query=>'camping backpack', job=>'my_job')";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].job_name, "my_job");
        assert_eq!(calls[0].query, "camping backpack");
    }

    #[test]
    fn test_parse_search_calls_none() {
        let sql = "SELECT * FROM products WHERE id = 1";
        let calls = parse_search_calls(sql).unwrap();
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_search_calls_escaped_quotes() {
        let sql = "SELECT * FROM vectorize.search(job=>'it''s a job', query=>'o''malley''s bar')";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].job_name, "it's a job");
        assert_eq!(calls[0].query, "o'malley's bar");
    }

    #[test]
    fn test_parse_search_calls_paren_in_query() {
        let sql = "SELECT * FROM vectorize.search(job=>'my_job', query=>'find func(arg)')";
        let calls = parse_search_calls(sql).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].job_name, "my_job");
        assert_eq!(calls[0].query, "find func(arg)");
    }
}
