use crate::errors::VectorizeError;
use crate::transformers::types::{Inputs, PairedEmbeddings};
use std::sync::LazyLock;

/// Shared client for all embedding-provider HTTP calls. `reqwest::Client` holds
/// its own connection pool, so constructing a new one per request (as opposed to
/// reusing this) throws away keep-alive and adds real per-request overhead.
pub static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

pub async fn handle_response<T: for<'de> serde::Deserialize<'de>>(
    resp: reqwest::Response,
    method: &'static str,
) -> Result<T, VectorizeError> {
    if !resp.status().is_success() {
        let errmsg = format!(
            "Failed to call method '{}', received response with status code:{} and body: {}",
            method,
            resp.status(),
            resp.text().await?
        );
        return Err(anyhow::anyhow!(errmsg)).map_err(VectorizeError::from);
    }
    let value = resp.json::<T>().await?;
    Ok(value)
}

// merges the vec of inputs with the embedding responses
pub fn merge_input_output(inputs: Vec<Inputs>, values: Vec<Vec<f64>>) -> Vec<PairedEmbeddings> {
    inputs
        .into_iter()
        .zip(values)
        .map(|(input, value)| PairedEmbeddings {
            primary_key: input.record_id,
            embeddings: value,
        })
        .collect()
}
