use anyhow::Result;
use pgrx::prelude::*;
use vectorize_core::core::transformers::http_handler::handle_response;

use crate::guc::EMBEDDING_REQ_TIMEOUT_SEC;

pub fn validate_api_key(key: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let timeout = EMBEDDING_REQ_TIMEOUT_SEC.get();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));
    runtime.block_on(async {
        let resp = client
            .get("https://api.openai.com/v1/models")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", key))
            .timeout(std::time::Duration::from_secs(timeout as u64))
            .send()
            .await
            .unwrap_or_else(|e| error!("failed to make Open AI key validation call: {}", e));
        let _ = handle_response::<serde_json::Value>(resp, "models")
            .await
            .unwrap_or_else(|e| error!("failed validate API key: {}", e));
    });
    Ok(())
}
