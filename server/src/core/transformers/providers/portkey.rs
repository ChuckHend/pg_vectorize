use reqwest::Client;

use super::{
    ChatMessageRequest, ChatResponse, EmbeddingProvider, GenericEmbeddingRequest,
    GenericEmbeddingResponse,
};
use crate::core::errors::VectorizeError;
use crate::core::transformers::http_handler::handle_response;
use crate::core::transformers::providers;
use crate::core::transformers::providers::openai;
use async_trait::async_trait;
use std::env;

pub const PORTKEY_BASE_URL: &str = "https://api.portkey.ai/v1";
pub const MAX_TOKEN_LEN: usize = 8192;

pub struct PortkeyProvider {
    pub url: String,
    pub api_key: String,
    pub virtual_key: String,
}

impl PortkeyProvider {
    pub fn new(url: Option<String>, api_key: Option<String>, virtual_key: Option<String>) -> Self {
        let final_url = match url {
            Some(url) => url,
            None => PORTKEY_BASE_URL.to_string(),
        };
        let final_api_key = match api_key {
            Some(api_key) => api_key,
            None => env::var("PORTKEY_API_KEY").expect("PORTKEY_API_KEY not set"),
        };
        let final_virtual_key = match virtual_key {
            Some(vkey) => vkey,
            None => env::var("PORTKEY_VIRTUAL_KEY").expect("PORTKEY_VIRTUAL_KEY not set"),
        };
        PortkeyProvider {
            url: final_url,
            api_key: final_api_key,
            virtual_key: final_virtual_key,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for PortkeyProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let client = Client::new();

        let req = openai::OpenAIEmbeddingBody::from(request.clone());
        let num_inputs = request.input.len();
        let todo_requests: Vec<openai::OpenAIEmbeddingBody> = if num_inputs > 2048 {
            providers::split_vector(req.input, 2048)
                .iter()
                .map(|chunk| openai::OpenAIEmbeddingBody {
                    input: chunk.clone(),
                    model: request.model.clone(),
                })
                .collect()
        } else {
            vec![req]
        };
        let embeddings_url = format!("{}/embeddings", self.url);

        let mut all_embeddings: Vec<Vec<f64>> = Vec::with_capacity(num_inputs);
        for request_payload in todo_requests.iter() {
            let payload_val = serde_json::to_value(request_payload)?;
            let response = client
                .post(&embeddings_url)
                .timeout(std::time::Duration::from_secs(120_u64))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("x-portkey-virtual-key", self.virtual_key.clone())
                .header("x-portkey-api-key", &self.api_key)
                .json(&payload_val)
                .send()
                .await?;

            let embeddings =
                handle_response::<openai::OpenAIEmbeddingResponse>(response, "embeddings").await?;
            all_embeddings.extend(embeddings.data.iter().map(|x| x.embedding.clone()));
        }
        Ok(GenericEmbeddingResponse {
            embeddings: all_embeddings,
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

impl PortkeyProvider {
    pub async fn generate_response(
        &self,
        model_name: String,
        messages: &[ChatMessageRequest],
    ) -> Result<String, VectorizeError> {
        let client = Client::new();
        let message = serde_json::json!({
            "model": model_name,
            "messages": messages,
        });
        let chat_url = format!("{}/chat/completions", self.url);
        let response = client
            .post(&chat_url)
            .timeout(std::time::Duration::from_secs(120_u64))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("x-portkey-virtual-key", self.virtual_key.clone())
            .header("x-portkey-api-key", &self.api_key)
            .json(&message)
            .send()
            .await?;
        let chat_response = handle_response::<ChatResponse>(response, "embeddings").await?;
        Ok(chat_response.choices[0].message.content.clone())
    }
}

#[cfg(test)]
mod portkey_integration_tests {
    use super::*;
    use tokio::test as async_test;

    #[ignore]
    #[async_test]
    async fn test_portkey_openai() {
        let portkey_api_key = env::var("PORTKEY_API_KEY").expect("PORTKEY_API_KEY not set");
        let portkey_virtual_key =
            env::var("PORTKEY_VIRTUAL_KEY_OPENAI").expect("PORTKEY_VIRTUAL_KEY_OPENAI not set");
        let provider = PortkeyProvider::new(None, Some(portkey_api_key), Some(portkey_virtual_key));
        let request = GenericEmbeddingRequest {
            model: "text-embedding-ada-002".to_string(),
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
            embeddings.embeddings[0].len() == 1536,
            "Embeddings should have dimension 1536"
        );

        let dim = provider.model_dim("text-embedding-ada-002").await.unwrap();
        assert_eq!(dim, 1536);

        let chatmessage = ChatMessageRequest {
            role: "user".to_string(),
            content: "hello world".to_string(),
        };
        let response = provider
            .generate_response("gpt-3.5-turbo".to_string(), &[chatmessage])
            .await
            .unwrap();
        assert!(!response.is_empty(), "Response should not be empty");
    }
}
