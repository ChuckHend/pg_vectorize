use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{EmbeddingProvider, GenericEmbeddingRequest, GenericEmbeddingResponse};
use crate::errors::VectorizeError;
use crate::transformers::http_handler::handle_response;
use async_trait::async_trait;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::env;

pub const COHERE_BASE_URL: &str = "https://api.cohere.com/v1";

lazy_static! {
    static ref MODEL_DIMENSIONS: HashMap<&'static str, u32> = {
        let mut m = HashMap::new();
        m.insert("embed-english-v3.0", 1024);
        m.insert("embed-multilingual-v3.0", 1024);
        m.insert("embed-english-light-v3.0", 384);
        m.insert("embed-multilingual-light-v3.0", 384);
        m.insert("embed-english-v2.0", 4096);
        m.insert("embed-english-light-v2.0", 1024);
        m.insert("embed-multilingual-v2.0", 768);
        m
    };
}

pub struct CohereProvider {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CohereEmbeddingBody {
    model: String,
    texts: Vec<String>,
    input_type: String,
    truncate: String,
}

impl From<GenericEmbeddingRequest> for CohereEmbeddingBody {
    fn from(request: GenericEmbeddingRequest) -> Self {
        CohereEmbeddingBody {
            model: request.model,
            texts: request.input,
            input_type: "search_document".to_string(),
            truncate: "END".to_string(),
        }
    }
}

impl CohereProvider {
    pub fn new(url: Option<String>, api_key: Option<String>) -> Result<Self, VectorizeError> {
        let final_url = match url {
            Some(url) => url,
            None => COHERE_BASE_URL.to_string(),
        };
        let final_api_key = match api_key {
            Some(api_key) => api_key,
            None => match env::var("CO_API_KEY") {
                Ok(key) => key,
                Err(e) => {
                    log::error!("CO_API_KEY environment variable not set.");
                    Err(e)?
                }
            },
        };
        Ok(CohereProvider {
            url: final_url,
            api_key: final_api_key,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for CohereProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let client = Client::new();

        let payload = CohereEmbeddingBody::from(request.clone());
        let payload_val = serde_json::to_value(payload)?;
        let embeddings_url = format!("{}/embed", self.url);
        let response = client
            .post(&embeddings_url)
            .timeout(std::time::Duration::from_secs(120_u64))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&payload_val)
            .send()
            .await?;

        let embeddings =
            handle_response::<GenericEmbeddingResponse>(response, "embeddings").await?;
        Ok(embeddings)
    }

    async fn model_dim(&self, model_name: &str) -> Result<u32, VectorizeError> {
        match MODEL_DIMENSIONS.get(model_name) {
            Some(dim) => Ok(*dim),
            None => Err(VectorizeError::ModelNotFound(model_name.to_string())),
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio::test as async_test;

    #[ignore]
    #[async_test]
    async fn test_generate_embedding() {
        let provider = CohereProvider::new(Some(COHERE_BASE_URL.to_string()), None).unwrap();
        let request = GenericEmbeddingRequest {
            model: "embed-english-light-v3.0".to_string(),
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
            "Embeddings should have dimension 384"
        );
    }
}
