use crate::guc;
use anyhow::{Context, Result};

use pgrx::prelude::*;

use vectorize_core::guc::ModelGucConfig;
use vectorize_core::transformers::providers::get_provider;
use vectorize_core::transformers::types::TransformerMetadata;
use vectorize_core::types::Model;

#[pg_extern]
pub fn mod_info(model_name: &str, api_key: default!(Option<String>, "NULL")) -> pgrx::JsonB {
    let transformer_model = Model::new(model_name)
        .context("Invalid model name")
        .unwrap();
    let mut guc_configs = guc::get_guc_configs(&transformer_model.source);
    if let Some(key) = api_key {
        guc_configs.api_key = Some(key);
    }
    let meta = sync_get_model_info(&transformer_model, &guc_configs).unwrap();
    pgrx::JsonB(serde_json::to_value(meta).unwrap())
}

pub fn sync_get_model_info(
    model: &Model,
    guc_configs: &ModelGucConfig,
) -> Result<TransformerMetadata> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));
    let meta = match runtime.block_on(async { get_model_info(model, guc_configs).await }) {
        Ok(e) => e,
        Err(e) => {
            error!("error getting model info: {}", e);
        }
    };
    Ok(meta)
}

pub async fn get_model_info(
    model: &Model,
    guc_configs: &ModelGucConfig,
) -> Result<TransformerMetadata> {
    let provider = get_provider(
        &model.source,
        guc_configs.api_key.clone(),
        guc_configs.service_url.clone(),
        guc_configs.virtual_key.clone(),
    )?;
    let dim = provider.model_dim(&model.api_name()).await?;
    Ok(TransformerMetadata {
        model: model.api_name(),
        max_seq_len: 0,
        embedding_dimension: dim as i32,
    })
}
