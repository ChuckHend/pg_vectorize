pub mod generic;
pub mod http_handler;
pub mod openai;

use crate::guc;
use pgrx::prelude::*;

use vectorize_core::guc::ModelGucConfig;
use vectorize_core::transformers::providers::{self, prepare_generic_embedding_request};
use vectorize_core::transformers::types::Inputs;
use vectorize_core::types::Model;

pub fn transform(input: &str, transformer: &Model, api_key: Option<String>) -> Vec<Vec<f64>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));

    let guc_configs: ModelGucConfig = guc::get_guc_configs(&transformer.source);
    let api_key = if let Some(k) = api_key {
        Some(k)
    } else {
        guc_configs.api_key
    };

    let provider = providers::get_provider(
        &transformer.source,
        api_key,
        guc_configs.service_url,
        guc_configs.virtual_key,
    )
    .expect("failed to get provider");
    let input = Inputs {
        record_id: "".to_string(),
        inputs: input.to_string(),
        token_estimate: 0,
    };
    let embedding_request = prepare_generic_embedding_request(transformer, &[input]);
    match runtime.block_on(async { provider.generate_embedding(&embedding_request).await }) {
        Ok(e) => e.embeddings,
        Err(e) => {
            error!("error getting embeddings: {}", e);
        }
    }
}
