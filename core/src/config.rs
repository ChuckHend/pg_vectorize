use std::env;

use anyhow::{Result, anyhow};

// errors if input contains non-alphanumeric characters or underscore
// in other worse - valid column names only
pub fn check_input(input: &str) -> Result<()> {
    let valid = input
        .as_bytes()
        .iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'_');
    match valid {
        true => Ok(()),
        false => Err(anyhow!("Invalid Input: {}", input)),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub proxy_enabled: bool,
    pub vectorize_proxy_port: u16,
    pub database_url: String,
    pub queue_name: String,
    pub embedding_svc_url: String,
    pub openai_api_key: Option<String>,
    pub ollama_svc_url: String,
    pub embedding_request_timeout: i32,
    pub poll_interval: u64,
    pub poll_interval_error: u64,
    pub max_retries: i32,
    pub webserver_port: u16,
    pub num_server_workers: usize,
}

impl Config {
    pub fn from_env() -> Config {
        Config {
            proxy_enabled: env::var("VECTORIZE_PROXY_ENABLED")
                .map(|v| parse_bool_flexible(&v))
                .unwrap_or(false),
            vectorize_proxy_port: from_env_default("VECTORIZE_PROXY_PORT", "5433")
                .parse()
                .unwrap(),
            database_url: from_env_default(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/postgres",
            ),
            queue_name: from_env_default("VECTORIZE_QUEUE", "vectorize_jobs"),
            embedding_svc_url: from_env_default(
                "EMBEDDING_SVC_URL",
                "http://localhost:3000/v1/embeddings",
            ),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            ollama_svc_url: from_env_default("OLLAMA_SVC_URL", "http://localhost:3001"),
            embedding_request_timeout: from_env_default("EMBEDDING_REQUEST_TIMEOUT", "6")
                .parse()
                .unwrap(),
            // time to wait between polling for job when there are no messages in queue
            poll_interval: from_env_default("POLL_INTERVAL", "2").parse().unwrap(),
            // time to wait between polling for job when there has been an error in processing
            poll_interval_error: from_env_default("POLL_INTERVAL_ERROR", "10")
                .parse()
                .unwrap(),
            max_retries: from_env_default("MAX_RETRIES", "2").parse().unwrap(),
            webserver_port: from_env_default("WEBSERVER_PORT", "8080").parse().unwrap(),
            num_server_workers: from_env_default("NUM_SERVER_WORKERS", "8").parse().unwrap(),
        }
    }
}

/// source a variable from environment - use default if not exists
pub fn from_env_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn parse_bool_flexible(s: &str) -> bool {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => false, // default to false for unrecognized values
    }
}
