pub mod cohere;
pub mod ollama;
pub mod openai;
pub mod portkey;
pub mod vector_serve;
pub mod voyage;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::types::Inputs;
use crate::errors::VectorizeError;
use crate::transformers::providers;
use crate::types::Model;
use crate::types::ModelSource;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    #[allow(async_fn_in_trait)]
    async fn generate_embedding<'a>(
        &self,
        request: &'a GenericEmbeddingRequest,
    ) -> Result<GenericEmbeddingResponse, VectorizeError>;
    #[allow(async_fn_in_trait)]
    async fn model_dim(&self, model_name: &str) -> Result<u32, VectorizeError>;
}

#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct GenericEmbeddingRequest {
    pub input: Vec<String>,
    pub model: String,
}

#[derive(Deserialize, Debug)]
pub struct GenericEmbeddingResponse {
    pub embeddings: Vec<Vec<f64>>,
}

pub fn prepare_generic_embedding_request(
    model: &Model,
    inputs: &[Inputs],
) -> GenericEmbeddingRequest {
    let text_inputs = providers::openai::trim_inputs(inputs);

    GenericEmbeddingRequest {
        input: text_inputs,
        model: model.api_name(),
    }
}

pub fn get_provider(
    model_source: &ModelSource,
    api_key: Option<String>,
    url: Option<String>,
    virtual_key: Option<String>,
) -> Result<Box<dyn EmbeddingProvider + Send + Sync>, VectorizeError> {
    match model_source {
        ModelSource::OpenAI => Ok(Box::new(providers::openai::OpenAIProvider::new(
            url, api_key,
        )?)),
        ModelSource::Cohere => Ok(Box::new(providers::cohere::CohereProvider::new(
            url, api_key,
        )?)),
        ModelSource::Portkey => Ok(Box::new(providers::portkey::PortkeyProvider::new(
            url,
            api_key,
            virtual_key,
        ))),
        ModelSource::Voyage => Ok(Box::new(providers::voyage::VoyageProvider::new(
            url, api_key,
        )?)),
        ModelSource::SentenceTransformers => Ok(Box::new(
            providers::vector_serve::VectorServeProvider::new(url, api_key),
        )),
        ModelSource::Ollama => Ok(Box::new(providers::ollama::OllamaProvider::new(url))),
    }
}

fn split_vector(vec: Vec<String>, chunk_size: usize) -> Vec<Vec<String>> {
    vec.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageRequest {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize, Debug)]
struct ResponseMessage {
    content: String,
}
