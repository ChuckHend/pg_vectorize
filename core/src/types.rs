use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use thiserror::Error;
use utoipa::ToSchema;

pub const VECTORIZE_SCHEMA: &str = "vectorize";

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema, FromRow)]
pub struct VectorizeJob {
    pub job_name: String,
    pub src_table: String,
    pub src_schema: String,
    pub src_column: String,
    pub primary_key: String,
    pub update_time_col: String,
    #[serde(
        deserialize_with = "string_to_model",
        serialize_with = "model_to_string"
    )]
    pub model: Model,
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Serialize, Deserialize)]
// SimilarityAlg is now deprecated
pub enum SimilarityAlg {
    pgv_cosine_similarity,
}

impl Display for SimilarityAlg {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            SimilarityAlg::pgv_cosine_similarity => write!(f, "pgv_cosine_similarity"),
        }
    }
}

impl FromStr for SimilarityAlg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pgv_cosine_similarity" => Ok(SimilarityAlg::pgv_cosine_similarity),
            _ => Err(format!("Invalid value: {s}")),
        }
    }
}

impl From<String> for SimilarityAlg {
    fn from(s: String) -> Self {
        match s.as_str() {
            "pgv_cosine_similarity" => SimilarityAlg::pgv_cosine_similarity, // ... handle other variants ...
            _ => panic!("Invalid value for SimilarityAlg: {s}"), // or handle this case differently
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IndexDist {
    pgv_hnsw_l2,
    pgv_hnsw_ip,
    pgv_hnsw_cosine,
    vsc_diskann_cosine,
}

impl Display for IndexDist {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            IndexDist::pgv_hnsw_l2 => write!(f, "pgv_hnsw_l2"),
            IndexDist::pgv_hnsw_ip => write!(f, "pgv_hnsw_ip"),
            IndexDist::pgv_hnsw_cosine => write!(f, "pgv_hnsw_cosine"),
            IndexDist::vsc_diskann_cosine => write!(f, "vsc_diskann_cosine"),
        }
    }
}

impl FromStr for IndexDist {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pgv_hnsw_l2" => Ok(IndexDist::pgv_hnsw_l2),
            "pgv_hnsw_ip" => Ok(IndexDist::pgv_hnsw_ip),
            "pgv_hnsw_cosine" => Ok(IndexDist::pgv_hnsw_cosine),
            "vsc_diskann_cosine" => Ok(IndexDist::vsc_diskann_cosine),
            _ => Err(format!("Invalid value for IndexDist: {s}")),
        }
    }
}

impl From<String> for IndexDist {
    fn from(s: String) -> Self {
        match s.as_str() {
            "pgv_hnsw_l2" => IndexDist::pgv_hnsw_l2,
            "pgv_hnsw_ip" => IndexDist::pgv_hnsw_ip,
            "pgv_hnsw_cosine" => IndexDist::pgv_hnsw_cosine,
            "vsc_diskann_cosine" => IndexDist::vsc_diskann_cosine,
            _ => panic!("Invalid value for IndexDist: {s}"),
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum TableMethod {
    // append a new column to the existing table
    append,
    // join existing table to a new table with embeddings
    #[default]
    join,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, FromRow)]
pub struct JobParams {
    pub schema: String,
    pub relation: String,
    pub columns: Vec<String>,
    pub update_time_col: Option<String>,
    pub table_method: TableMethod,
    pub primary_key: String,
    pub pkey_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_schedule")]
    pub schedule: String,
    pub args: Option<serde_json::Value>,
}

fn default_schedule() -> String {
    "realtime".to_string()
}

// schema for all messages that hit pgmq
#[derive(Clone, Deserialize, Debug, Serialize)]
pub struct JobMessage {
    pub job_name: String,
    pub record_ids: Vec<String>,
}

// schema for every job
// also schema for the vectorize.vectorize_meta table
#[derive(Clone, Debug, Deserialize, FromRow, Serialize)]
pub struct VectorizeMeta {
    pub job_id: i64,
    pub name: String,
    pub index_dist_type: IndexDist,
    #[serde(
        deserialize_with = "string_to_model",
        serialize_with = "model_to_string"
    )]
    pub transformer: Model,
    pub params: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, FromRow, ToSchema)]
pub struct Model {
    pub source: ModelSource,
    // the model's namespace + model name
    pub fullname: String,
    pub name: String,
}

use serde::Serializer;
use serde::de::Deserializer;

pub fn string_to_model<'de, D>(deserializer: D) -> Result<Model, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Model::from(s))
}

// New serialization function
pub fn model_to_string<S>(model: &Model, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&model.fullname)
}

use sqlx::{
    Decode, Type,
    postgres::{PgTypeInfo, PgValueRef},
};

impl Type<sqlx::Postgres> for Model {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> Decode<'r, sqlx::Postgres> for Model {
    fn decode(value: PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let s = <String as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(Model::from(s))
    }
}

impl Model {
    // the name to use when calling an API
    pub fn api_name(&self) -> String {
        match self.source {
            ModelSource::OpenAI => self.name.clone(),
            ModelSource::SentenceTransformers => self.fullname.clone(),
            ModelSource::Ollama => self.name.clone(),
            ModelSource::Cohere => self.name.clone(),
            ModelSource::Portkey => self.name.clone(),
            ModelSource::Voyage => self.name.clone(),
        }
    }
}

impl From<String> for Model {
    fn from(input: String) -> Self {
        let errmsg = format!("Invalid input string for Model: {input}");
        Model::new(&input).expect(&errmsg)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum ModelError {
    #[error("Invalid model source: {0}")]
    InvalidSource(String),
    #[error("Invalid model format: {0}")]
    InvalidFormat(String),
}

impl Model {
    pub fn new(input: &str) -> Result<Self, ModelError> {
        let mut parts: Vec<&str> = input.split('/').collect();

        let missing_source = parts.len() < 2;
        if parts.len() > 3 {
            return Err(ModelError::InvalidFormat(input.to_string()));
        }

        if missing_source && parts[0] == "text-embedding-ada-002" {
            // for backwards compatibility, prepend "openai" to text-embedding-ada-2
            parts.insert(0, "openai");
        } else if missing_source && parts[0] == "all-MiniLM-L12-v2" {
            parts.insert(0, "sentence-transformers");
        } else if missing_source {
            return Err(ModelError::InvalidFormat(input.to_string()));
        }

        let source = parts[0]
            .parse::<ModelSource>()
            .map_err(|_| ModelError::InvalidSource(parts[0].to_string()))?;

        let name = parts
            .last()
            .expect("expected non-empty model name")
            .to_string();

        Ok(Self {
            source,
            fullname: parts.join("/"),
            name,
        })
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.fullname)
    }
}

// model sources are places that serve models
// each source can have its own API schema
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub enum ModelSource {
    OpenAI,
    SentenceTransformers,
    Ollama,
    Cohere,
    Portkey,
    Voyage,
}

impl FromStr for ModelSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ollama" => Ok(ModelSource::Ollama),
            "openai" => Ok(ModelSource::OpenAI),
            "sentence-transformers" => Ok(ModelSource::SentenceTransformers),
            "cohere" => Ok(ModelSource::Cohere),
            "portkey" => Ok(ModelSource::Portkey),
            "voyage" => Ok(ModelSource::Voyage),
            _ => Ok(ModelSource::SentenceTransformers),
        }
    }
}

impl Display for ModelSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            ModelSource::Ollama => write!(f, "ollama"),
            ModelSource::OpenAI => write!(f, "openai"),
            ModelSource::SentenceTransformers => write!(f, "sentence-transformers"),
            ModelSource::Cohere => write!(f, "cohere"),
            ModelSource::Portkey => write!(f, "portkey"),
            ModelSource::Voyage => write!(f, "voyage"),
        }
    }
}

impl From<String> for ModelSource {
    fn from(s: String) -> Self {
        match s.as_str() {
            "ollama" => ModelSource::Ollama,
            "openai" => ModelSource::OpenAI,
            "sentence-transformers" => ModelSource::SentenceTransformers,
            "cohere" => ModelSource::Cohere,
            "portkey" => ModelSource::Portkey,
            "voyage" => ModelSource::Voyage,
            // other cases are assumed to be private sentence-transformer compatible model
            // and can be hot-loaded
            _ => ModelSource::SentenceTransformers,
        }
    }
}

// test
#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn test_portkey_parsing() {
        let model = Model::new("portkey/openai/text-embedding-ada-002").unwrap();
        assert_eq!(model.source, ModelSource::Portkey);
        assert_eq!(model.fullname, "portkey/openai/text-embedding-ada-002");
        assert_eq!(model.name, "text-embedding-ada-002");
        assert_eq!(model.api_name(), "text-embedding-ada-002");
    }

    #[test]
    fn test_voyage_parsing() {
        let model = Model::new("voyage/voyage-3-lite").unwrap();
        assert_eq!(model.source, ModelSource::Voyage);
        assert_eq!(model.fullname, "voyage/voyage-3-lite");
        assert_eq!(model.name, "voyage-3-lite");
        assert_eq!(model.api_name(), "voyage-3-lite");
    }

    #[test]
    fn test_ollama_parsing() {
        let model = Model::new("ollama/wizardlm2:7b").unwrap();
        assert_eq!(model.source, ModelSource::Ollama);
        assert_eq!(model.fullname, "ollama/wizardlm2:7b");
        assert_eq!(model.name, "wizardlm2:7b");
    }

    #[test]
    fn test_legacy_fullname() {
        let model = Model::new("text-embedding-ada-002").unwrap();
        assert_eq!(model.source, ModelSource::OpenAI);
        assert_eq!(model.name, "text-embedding-ada-002");
        assert_eq!(model.fullname, "openai/text-embedding-ada-002");
        let model_string = model.to_string();
        assert_eq!(model_string, "openai/text-embedding-ada-002");

        let model = Model::new("all-MiniLM-L12-v2").unwrap();
        assert_eq!(model.source, ModelSource::SentenceTransformers);
        assert_eq!(model.name, "all-MiniLM-L12-v2");
        assert_eq!(model.fullname, "sentence-transformers/all-MiniLM-L12-v2");

        let model_string = model.to_string();
        assert_eq!(model_string, "sentence-transformers/all-MiniLM-L12-v2");
    }

    #[test]
    fn test_valid_model_openai() {
        let model = Model::new("openai/model-name").unwrap();
        assert_eq!(model.source, ModelSource::OpenAI);
        assert_eq!(model.name, "model-name");
    }

    #[test]
    fn test_valid_model_sentencetransformers() {
        let model = Model::new("sentence-transformers/model-name").unwrap();
        assert_eq!(model.source, ModelSource::SentenceTransformers);
        assert_eq!(model.name, "model-name");
    }

    #[test]
    fn test_unknown_namespace() {
        // unknown namespace should default to sentence-transformers
        let model = Model::new("unknown/model-name").unwrap();
        assert!(model.source == ModelSource::SentenceTransformers);
        assert!(model.name == "model-name");
        assert!(model.fullname == "unknown/model-name");
    }

    #[test]
    fn test_invalid_format_no_slash() {
        assert!(Model::new("openaimodel-name").is_err());
    }

    #[test]
    fn test_backwards_compatibility() {
        let model = Model::new("text-embedding-ada-002").unwrap();
        assert_eq!(model.source, ModelSource::OpenAI);
        assert_eq!(model.name, "text-embedding-ada-002");
    }

    #[test]
    fn test_private_hf_sentence_transformer() {
        let model = Model::new("chuckhend/private-model").unwrap();
        assert_eq!(model.source, ModelSource::SentenceTransformers);
        assert_eq!(model.name, "private-model");
        assert_eq!(model.fullname, "chuckhend/private-model");
        let model_string = model.to_string();
        assert_eq!(model_string, "chuckhend/private-model");
    }
}
