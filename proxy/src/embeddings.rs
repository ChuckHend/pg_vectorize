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

    // Regex to match vectorize.embed('query', 'project_name')
    let re = Regex::new(
        r"(?i)vectorize\.embed\s*\(\s*'([^']*(?:''[^']*)*)'\s*,\s*'([^']*(?:''[^']*)*)'\s*\)",
    )?;

    let param_re = Regex::new(
        r"(?i)vectorize\.embed\s*\(\s*'([^']*(?:''[^']*)*)'\s*,\s*'([^']*(?:''[^']*)*)'\s*\)",
    )?;
    for mat in re.find_iter(sql) {
        let full_match = mat.as_str().to_string();
        let start_pos = mat.start();
        let end_pos = mat.end();

        if let Some(captures) = param_re.captures(&full_match) {
            let query = captures.get(1).unwrap().as_str().replace("''", "'");
            let project_name = captures.get(2).unwrap().as_str().replace("''", "'");

            calls.push(EmbedCall {
                query,
                project_name,
                full_match,
                start_pos,
                end_pos,
            });
        }
    }

    Ok(calls)
}

/// Rewrites SQL query by replacing vectorize.embed() calls with actual embeddings
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

    // Process calls in reverse order to maintain correct positions
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
    }

    #[test]
    fn test_parse_multiple_embed_calls() {
        let sql =
            "SELECT vectorize.embed('query1', 'project1'), vectorize.embed('query2', 'project2')";
        let calls = parse_embed_calls(sql).unwrap();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].query, "query1");
        assert_eq!(calls[0].project_name, "project1");
        assert_eq!(calls[1].query, "query2");
        assert_eq!(calls[1].project_name, "project2");
    }

    #[test]
    fn test_no_embed_calls() {
        let sql = "SELECT * FROM documents WHERE id = 1";
        let calls = parse_embed_calls(sql).unwrap();
        assert!(calls.is_empty());
    }
}
