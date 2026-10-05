use serde::{Deserialize, Serialize};

use super::{EmbeddingProvider, GenericEmbeddingRequest, GenericEmbeddingResponse};
use crate::errors::VectorizeError;
use crate::transformers::http_handler::{HTTP_CLIENT, handle_response};
use async_trait::async_trait;
use std::env;

pub const MISTRAL_BASE_URL: &str = "https://api.mistral.ai/v1";

pub struct MistralProvider {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MistralEmbeddingBody {
    pub input: Vec<String>,
    pub model: String,
}

impl From<GenericEmbeddingRequest> for MistralEmbeddingBody {
    fn from(request: GenericEmbeddingRequest) -> Self {
        MistralEmbeddingBody {
            input: request.input,
            model: request.model,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MistralEmbeddingResponse {
    pub data: Vec<EmbeddingObject>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EmbeddingObject {
    pub index: Option<usize>,
    pub embedding: Vec<f64>,
}

impl From<MistralEmbeddingResponse> for GenericEmbeddingResponse {
    fn from(response: MistralEmbeddingResponse) -> Self {
        GenericEmbeddingResponse {
            embeddings: response.data.iter().map(|x| x.embedding.clone()).collect(),
        }
    }
}

impl MistralProvider {
    pub fn new(url: Option<String>, api_key: Option<String>) -> Result<Self, VectorizeError> {
        let final_url = match url {
            Some(url) => url,
            None => MISTRAL_BASE_URL.to_string(),
        };
        let final_api_key = match api_key {
            Some(api_key) => api_key,
            None => {
                env::var("MISTRAL_API_KEY").map_err(|_| VectorizeError::ProviderNotConfigured {
                    provider: "mistral".to_string(),
                    env_var: "MISTRAL_API_KEY".to_string(),
                })?
            }
        };
        Ok(MistralProvider {
            url: final_url,
            api_key: final_api_key,
        })
    }
}

#[async_trait]
impl EmbeddingProvider for MistralProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let req_body = MistralEmbeddingBody::from(request.clone());
        let embedding_url = format!("{}/embeddings", self.url);

        let response = HTTP_CLIENT
            .post(&embedding_url)
            .timeout(crate::config::embedding_request_timeout())
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&req_body)
            .send()
            .await?;

        let embeddings =
            handle_response::<MistralEmbeddingResponse>(response, "embeddings").await?;
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
mod tests {
    use super::*;

    #[test]
    fn test_mistral_embedding_body_from_request() {
        let req = GenericEmbeddingRequest {
            input: vec!["test text".to_string()],
            model: "mistral-embed".to_string(),
        };
        let body = MistralEmbeddingBody::from(req);
        assert_eq!(body.model, "mistral-embed");
        assert_eq!(body.input, vec!["test text".to_string()]);
    }

    #[test]
    fn test_mistral_response_conversion() {
        let response = MistralEmbeddingResponse {
            data: vec![EmbeddingObject {
                index: Some(0),
                embedding: vec![0.1, 0.2, 0.3],
            }],
        };
        let generic: GenericEmbeddingResponse = response.into();
        assert_eq!(generic.embeddings.len(), 1);
        assert_eq!(generic.embeddings[0], vec![0.1, 0.2, 0.3]);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[ignore]
    #[tokio::test]
    async fn test_mistral_ai_embedding() {
        let provider = MistralProvider::new(Some(MISTRAL_BASE_URL.to_string()), None).unwrap();

        let request = GenericEmbeddingRequest {
            input: vec!["hello world".to_string()],
            model: "mistral-embed".to_string(),
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
            embeddings.embeddings[0].len() == 1024,
            "Embeddings should have dimension 1024"
        );

        let dim = provider.model_dim("mistral-embed").await.unwrap();
        assert_eq!(dim, 1024);
    }
}
