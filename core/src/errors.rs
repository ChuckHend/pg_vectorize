use anyhow::Error as AnyhowError;
use ollama_rs::error::OllamaError;
use pgmq::PgmqError;
use sqlx::error::Error as DbError;
use std::env::VarError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum VectorizeError {
    // invalid input
    #[error("Invalid input: {0}")]
    InputError(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    #[error("SQL error: {0}")]
    SqlError(#[from] DbError),
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("An internal error occurred: {0}")]
    InternalError(#[from] AnyhowError),
    #[error("data not found: {0}")]
    NotFound(String),
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("ollama error: {0}")]
    OllamaError(#[from] OllamaError),
    // env var missing
    #[error("Environment variable missing: {0}")]
    EnvError(#[from] VarError),
    #[error("job not found: {0}")]
    JobNotFound(String),
    #[error("embedding generation failed: {0}")]
    EmbeddingGenerationFailed(String),
    #[error("pgmq error: {0}")]
    PgmqError(#[from] PgmqError),
}
