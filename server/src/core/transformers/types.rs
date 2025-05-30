use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingObject>,
    pub model: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct EmbeddingObject {
    pub index: usize,
    pub embedding: Vec<f64>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct EmbeddingRequest {
    pub url: String,
    pub payload: EmbeddingPayload,
    pub api_key: Option<String>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct EmbeddingPayload {
    pub input: Vec<String>,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Inputs {
    pub record_id: String,   // the value to join the record
    pub inputs: String,      // concatenation of input columns
    pub token_estimate: i32, // estimated token count
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PairedEmbeddings {
    pub primary_key: String,
    pub embeddings: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransformerMetadata {
    pub model: String,
    pub max_seq_len: i32,
    pub embedding_dimension: i32,
}
