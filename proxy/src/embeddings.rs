use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

use vectorize_core::errors::VectorizeError;
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
                "Project '{}' not found in jobmap cache",
                project_name
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

pub fn parse_embed_calls(sql: &str) -> Result<Vec<EmbedCall>> {
    let mut calls = Vec::new();

    // matches vectorize.embed('query', 'project_name')  string literals only
    let string_re = Regex::new(
        r"(?i)vectorize\.embed\s*\(\s*'([^']*(?:''[^']*)*)'\s*,\s*'([^']*(?:''[^']*)*)'\s*\)",
    )?;

    // matches vectorize.embed($1, $2) prepared statement parameters
    let param_re = Regex::new(r"(?i)vectorize\.embed\s*\(\s*\$(\d+)\s*,\s*\$(\d+)\s*\)")?;

    // Parse string literal calls
    for mat in string_re.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let start_pos = mat.start();
        let end_pos = mat.end();

        if let Some(captures) = string_re.captures(&full_match) {
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
    for mat in param_re.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let start_pos = mat.start();
        let end_pos = mat.end();

        if let Some(captures) = param_re.captures(&full_match) {
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
        if call.is_prepared {
            if let (Some(query_idx), Some(project_idx)) =
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
    }
    Ok(embed_calls)
}

/// rewrites SQL query by replacing vectorize.embed() calls with actual embeddings
pub async fn rewrite_query_with_embeddings(
    sql: &str,
    provider: &JobMapEmbeddingProvider,
) -> Result<String, VectorizeError> {
    let embed_calls = parse_embed_calls(sql).map_err(|e| {
        VectorizeError::EmbeddingGenerationFailed(format!("Failed to parse embed calls: {}", e))
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
}
