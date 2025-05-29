use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{
    ChatMessageRequest, ChatResponse, EmbeddingProvider, GenericEmbeddingRequest,
    GenericEmbeddingResponse,
};
use crate::errors::VectorizeError;
use crate::transformers::http_handler::handle_response;
use crate::transformers::providers;
use crate::transformers::types::Inputs;
use async_trait::async_trait;
use std::env;

pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const MAX_TOKEN_LEN: usize = 8192;

pub struct OpenAIProvider {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAIEmbeddingBody {
    pub model: String,
    pub input: Vec<String>,
}

impl From<GenericEmbeddingRequest> for OpenAIEmbeddingBody {
    fn from(request: GenericEmbeddingRequest) -> Self {
        OpenAIEmbeddingBody {
            model: request.model,
            input: request.input,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenAIEmbeddingResponse {
    pub model: String,
    pub data: Vec<EmbeddingObject>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EmbeddingObject {
    pub index: usize,
    pub embedding: Vec<f64>,
}

impl From<OpenAIEmbeddingResponse> for GenericEmbeddingResponse {
    fn from(response: OpenAIEmbeddingResponse) -> Self {
        GenericEmbeddingResponse {
            embeddings: response.data.iter().map(|x| x.embedding.clone()).collect(),
        }
    }
}

impl OpenAIProvider {
    pub fn new(url: Option<String>, api_key: Option<String>) -> Self {
        let final_url = match url {
            Some(url) => url,
            None => OPENAI_BASE_URL.to_string(),
        };
        let final_api_key = match api_key {
            Some(api_key) => api_key,
            None => env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        };
        OpenAIProvider {
            url: final_url,
            api_key: final_api_key,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIProvider {
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError> {
        let client = Client::new();
        let req = OpenAIEmbeddingBody::from(request.clone());
        let num_inputs = request.input.len();
        let todo_requests: Vec<OpenAIEmbeddingBody> = if num_inputs > 2048 {
            providers::split_vector(req.input, 2048)
                .iter()
                .map(|chunk| OpenAIEmbeddingBody {
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
                handle_response::<OpenAIEmbeddingResponse>(response, "embeddings").await?;
            all_embeddings.extend(embeddings.data.iter().map(|x| x.embedding.clone()));
        }
        Ok(GenericEmbeddingResponse {
            embeddings: all_embeddings,
        })
    }

    async fn model_dim(&self, model_name: &str) -> Result<u32, VectorizeError> {
        Ok(openai_embedding_dim(model_name) as u32)
    }
}

pub fn openai_embedding_dim(model_name: &str) -> i32 {
    match model_name {
        "text-embedding-3-large" => 3072,
        "text-embedding-3-small" => 1536,
        "text-embedding-ada-002" => 1536,
        _ => 1536,
    }
}

impl OpenAIProvider {
    pub async fn generate_response(
        &self,
        model_name: String,
        messages: &[ChatMessageRequest],
    ) -> Result<String, VectorizeError> {
        let client = Client::new();
        let chat_url = format!("{}/chat/completions", self.url);
        let message = serde_json::json!({
            "model": model_name,
            "messages": messages,
        });
        let response = client
            .post(&chat_url)
            .timeout(std::time::Duration::from_secs(120_u64))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .json(&message)
            .send()
            .await?;
        let chat_response = handle_response::<ChatResponse>(response, "embeddings").await?;
        Ok(chat_response.choices[0].message.content.clone())
    }
}

// OpenAI embedding model has a limit of 8192 tokens per input
// there can be a number of ways condense the inputs
pub fn trim_inputs(inputs: &[Inputs]) -> Vec<String> {
    // TODO: need to dynamically calculate max input window
    inputs
        .iter()
        .map(|input| {
            if input.token_estimate as usize > MAX_TOKEN_LEN {
                // not example taking tokens, but naive way to trim input
                let tokens: Vec<&str> = input.inputs.split_whitespace().collect();
                tokens
                    .into_iter()
                    .take(MAX_TOKEN_LEN)
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                input.inputs.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tokio::test as async_test;

    #[ignore]
    #[async_test]
    async fn test_generate_embedding() {
        let provider = OpenAIProvider::new(Some(OPENAI_BASE_URL.to_string()), None);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_inputs_no_trimming_required() {
        let data = vec![
            Inputs {
                record_id: "1".to_string(),
                inputs: "token1 token2".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "2".to_string(),
                inputs: "token3 token4".to_string(),
                token_estimate: 2,
            },
        ];

        let trimmed = trim_inputs(&data);
        assert_eq!(trimmed, vec!["token1 token2", "token3 token4"]);
    }

    #[test]
    fn test_trim_inputs_trimming_required() {
        let token_len = 1000000;
        let long_input = (0..token_len)
            .map(|i| format!("token{}", i))
            .collect::<Vec<_>>()
            .join(" ");

        let num_tokens = long_input.split_whitespace().count();
        assert_eq!(num_tokens, token_len);

        let data = vec![Inputs {
            record_id: "1".to_string(),
            inputs: long_input.clone(),
            token_estimate: token_len as i32,
        }];

        let trimmed = trim_inputs(&data);
        let trimmed_input = trimmed[0].clone();
        let trimmed_length = trimmed_input.split_whitespace().count();
        assert_eq!(trimmed_length, MAX_TOKEN_LEN);
    }

    #[test]
    fn test_trim_inputs_mixed_cases() {
        let num_tokens_in = 1000000;
        let long_input = (0..num_tokens_in)
            .map(|i| format!("token{}", i))
            .collect::<Vec<_>>()
            .join(" ");
        let data = vec![
            Inputs {
                record_id: "1".to_string(),
                inputs: "token1 token2".to_string(),
                token_estimate: 2,
            },
            Inputs {
                record_id: "2".to_string(),
                inputs: long_input.clone(),
                token_estimate: num_tokens_in,
            },
        ];

        let trimmed = trim_inputs(&data);
        assert_eq!(trimmed[0].split_whitespace().count(), 2);
        assert_eq!(trimmed[1].split_whitespace().count(), MAX_TOKEN_LEN);
    }
}
