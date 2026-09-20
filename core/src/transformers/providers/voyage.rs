use serde::{Deserialize, Serialize};

use super::{EmbeddingProvider, GenericEmbeddingRequest, GenericEmbeddingResponse};
use crate::errors::VectorizeError;
use crate::transformers::http_handler::{HTTP_CLIENT, handle_response};
use async_trait::async_trait;
use std::env;

pub const VOYAGE_BASE_URL: &str = "https://api.voyageai.com/v1";

pub struct VoyageProvider {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoyageEmbeddingBody {
    pub input: Vec<String>,
    pub model: String,
    pub input_type: String,
}

impl From<GenericEmbeddingRequest> for VoyageEmbeddingBody {
    fn from(request: GenericEmbeddingRequest) -> Self {
        VoyageEmbeddingBody {
            input: request.input,
            model: request.model,
            input_type: "document".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoyageEmbeddingResponse {
    pub data: Vec<EmbeddingObject>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EmbeddingObject {
    pub embedding: Vec<f64>,
}

impl From<VoyageEmbeddingResponse> for GenericEmbeddingResponse {
    fn from(response: VoyageEmbeddingResponse) -> Self {
        GenericEmbeddingResponse {
            embeddings: response.data.iter().map(|x| x.embedding.clone()).collect(),
        }
    }
}

impl VoyageProvider {
    pub fn new(url: Option<String>, api_key: Option<String>) -> Result<Self, VectorizeError> {
        let final_url = match url {
            Some(url) => url,
            None => VOYAGE_BASE_URL.to_string(),
        };
        let final_api_key = match api_key {
            Some(api_key) => api_key,
            None => {
                env::var("VOYAGE_API_KEY").map_err(|_| VectorizeError::ProviderNotConfigured {
                    provider: "voyage".to_string(),
                    env_var: "VOYAGE_API_KEY".to_string(),
                })?
            }
        };
        Ok(VoyageProvider {
            url: final_url,
            api_key: final_api_key,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for VoyageProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let req_body = VoyageEmbeddingBody::from(request.clone());
        let embedding_url = format!("{}/embeddings", self.url);

        let response = HTTP_CLIENT
            .post(&embedding_url)
            .timeout(crate::config::embedding_request_timeout())
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req_body)
            .send()
            .await?;

        let embeddings = handle_response::<VoyageEmbeddingResponse>(response, "embeddings").await?;
        Ok(GenericEmbeddingResponse {
            embeddings: embeddings
                .data
                .iter()
                .map(|x| x.embedding.clone())
                .collect(),
        })
    }

    async fn model_dim(&self, model_name: &str) -> Result<u32, VectorizeError> {
        // determine embedding dim by generating an embedding and getting length of array
        let req = GenericEmbeddingRequest {
            input: vec!["hello world".to_string()],
            model: model_name.to_string(),
        };
        let embedding = self.generate_embedding(&req).await?;
        let dim = embedding.embeddings[0].len();
        Ok(dim as u32)
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[ignore]
    #[tokio::test]
    async fn test_voyage_ai_embedding() {
        let provider = VoyageProvider::new(Some(VOYAGE_BASE_URL.to_string()), None).unwrap();

        let request = GenericEmbeddingRequest {
            input: vec!["hello world".to_string()],
            model: "voyage-3-lite".to_string(),
        };

        let embeddings = provider.generate_embedding(&request).await.unwrap();
        println!("{embeddings:?}");
        assert!(
            !embeddings.embeddings.is_empty(),
            "Embeddings should not be empty"
        );
        assert!(
            embeddings.embeddings.len() == 1,
            "Embeddings should have length 1"
        );
        assert!(
            embeddings.embeddings[0].len() == 512,
            "Embeddings should have dimension 512"
        );

        let dim = provider.model_dim("voyage-3-lite").await.unwrap();
        assert_eq!(dim, 512);
    }
}
