use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{EmbeddingProvider, GenericEmbeddingRequest, GenericEmbeddingResponse};
use crate::core::errors::VectorizeError;
use crate::core::transformers::http_handler::handle_response;
use crate::core::transformers::providers::openai;
use async_trait::async_trait;
use std::env;

pub const VECTOR_SERVE_BASE_URL: &str = "http://localhost:3000/v1";

pub struct VectorServeProvider {
    pub url: String,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ModelInfo {
    model: String,
    embedding_dimension: u32,
    max_seq_len: u32,
}

impl VectorServeProvider {
    pub fn new(url: Option<String>, api_key: Option<String>) -> Self {
        let final_url = match url {
            Some(url) => url,
            None => {
                env::var("EMBEDDING_SVC_URL").unwrap_or_else(|_| VECTOR_SERVE_BASE_URL.to_string())
            }
        };
        let final_api_key = match api_key {
            Some(api_key) => Some(api_key),
            // API key is optional for vector-serve
            None => env::var("EMBEDDING_SVC_API_KEY").ok(),
        };
        VectorServeProvider {
            url: final_url,
            api_key: final_api_key,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for VectorServeProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let client = Client::new();
        let req = openai::OpenAIEmbeddingBody::from(request.clone());
        let num_inputs = request.input.len();
        let todo_requests: Vec<openai::OpenAIEmbeddingBody> = if num_inputs > 2048 {
            split_vector(req.input, 2048)
                .iter()
                .map(|chunk| openai::OpenAIEmbeddingBody {
                    input: chunk.clone(),
                    model: request.model.clone(),
                })
                .collect()
        } else {
            vec![req]
        };

        let mut all_embeddings: Vec<Vec<f64>> = Vec::with_capacity(num_inputs);

        for request_payload in todo_requests.iter() {
            let payload_val = serde_json::to_value(request_payload)?;
            let embeddings_url = format!("{}/embeddings", self.url);
            let mut req = client
                .post(&embeddings_url)
                .timeout(std::time::Duration::from_secs(120_u64))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&payload_val);
            if let Some(key) = &self.api_key {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            let response = req.send().await?;
            let embeddings =
                handle_response::<openai::OpenAIEmbeddingResponse>(response, "embeddings").await?;
            all_embeddings.extend(embeddings.data.iter().map(|x| x.embedding.clone()));
        }
        Ok(GenericEmbeddingResponse {
            embeddings: all_embeddings,
        })
    }

    async fn model_dim(&self, model_name: &str) -> Result<u32, VectorizeError> {
        let client = Client::new();
        let mut req = client
            .get(format!("{}/info/?model_name={}", self.url, model_name))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let response = req.send().await?;
        let model_info = handle_response::<ModelInfo>(response, "model_info").await?;
        Ok(model_info.embedding_dimension)
    }
}

fn split_vector(vec: Vec<String>, chunk_size: usize) -> Vec<Vec<String>> {
    vec.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect()
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio::test as async_test;

    #[async_test]
    async fn test_vector_serve_embeddings() {
        let provider = VectorServeProvider::new(Some(VECTOR_SERVE_BASE_URL.to_string()), None);
        let request = GenericEmbeddingRequest {
            model: "sentence-transformers/all-MiniLM-L6-v2".to_string(),
            input: vec!["hello world".to_string()],
        };

        let embeddings = provider.generate_embedding(&request).await.unwrap();
        assert!(
            !embeddings.embeddings.is_empty(),
            "Embeddings should not be empty"
        );
        assert!(
            embeddings.embeddings.len() == 1,
            "Embeddings should have length 1"
        );
        assert!(
            embeddings.embeddings[0].len() == 384,
            "Embeddings should have dimension 1536"
        );

        let model_dim = provider
            .model_dim("sentence-transformers/all-MiniLM-L6-v2")
            .await
            .unwrap();
        assert_eq!(model_dim, 384);
    }
}
