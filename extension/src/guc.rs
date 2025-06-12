use anyhow::Result;
use core::ffi::CStr;
use pgrx::*;

use crate::transformers::generic::env_interpolate_string;
use vectorize_core::guc::{ModelGucConfig, VectorizeGuc};
use vectorize_core::types::ModelSource;

pub static VECTORIZE_HOST: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static VECTORIZE_DATABASE_NAME: GucSetting<Option<&CStr>> =
    GucSetting::<Option<&CStr>>::new(None);
pub static OPENAI_BASE_URL: GucSetting<Option<&'static CStr>> =
    GucSetting::<Option<&'static CStr>>::new(Some(c"https://api.openai.com/v1"));
pub static OPENAI_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static BATCH_SIZE: GucSetting<i32> = GucSetting::<i32>::new(10000);
pub static NUM_BGW_PROC: GucSetting<i32> = GucSetting::<i32>::new(1);
pub static EMBEDDING_SERVICE_API_KEY: GucSetting<Option<&CStr>> =
    GucSetting::<Option<&CStr>>::new(None);
pub static EMBEDDING_SERVICE_HOST: GucSetting<Option<&CStr>> =
    GucSetting::<Option<&CStr>>::new(None);
pub static EMBEDDING_REQ_TIMEOUT_SEC: GucSetting<i32> = GucSetting::<i32>::new(120);
pub static OLLAMA_SERVICE_HOST: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static TEMBO_SERVICE_HOST: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static TEMBO_API_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static COHERE_API_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static PORTKEY_API_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static PORTKEY_VIRTUAL_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static PORTKEY_SERVICE_URL: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static VOYAGE_API_KEY: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static VOYAGE_SERVICE_URL: GucSetting<Option<&CStr>> = GucSetting::<Option<&CStr>>::new(None);
pub static SEMANTIC_WEIGHT: GucSetting<i32> = GucSetting::<i32>::new(50);
// EXPERIMENTAL
pub static FTS_INDEX_TYPE: GucSetting<Option<&'static CStr>> =
    GucSetting::<Option<&'static CStr>>::new(None);

// initialize GUCs
pub fn init_guc() {
    GucRegistry::define_string_guc(
        "vectorize.host",
        "unix socket url for Postgres",
        "unix socket path to the Postgres instance. Optional. Can also be set in environment variable.",
        &VECTORIZE_HOST,
        GucContext::Suset, GucFlags::default()
    );

    GucRegistry::define_string_guc(
        "vectorize.database_name",
        "Target database for vectorize operations",
        "Specifies the target database for vectorize operations.",
        &VECTORIZE_DATABASE_NAME,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.openai_service_url",
        "Base url to the OpenAI Server",
        "Url to any OpenAI compatible service.",
        &OPENAI_BASE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.openai_key",
        "API key from OpenAI",
        "API key from OpenAI. Optional. Overridden by any values provided in function calls.",
        &OPENAI_KEY,
        GucContext::Suset,
        GucFlags::SUPERUSER_ONLY,
    );

    GucRegistry::define_string_guc(
        "vectorize.ollama_service_url",
        "Ollama server url",
        "Scheme, host, and port of the Ollama server",
        &OLLAMA_SERVICE_HOST,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        "vectorize.batch_size",
        "Vectorize job batch size",
        "Number of records that can be included in a single vectorize job.",
        &BATCH_SIZE,
        1,
        100000,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.embedding_service_url",
        "Url for an OpenAI compatible embedding service",
        "Url to a service with request and response schema consistent with OpenAI's embeddings API.",
        &EMBEDDING_SERVICE_HOST,
        GucContext::Suset, GucFlags::default());

    GucRegistry::define_string_guc(
        "vectorize.embedding_service_api_key",
        "API key for vector-serve container",
        "Used for any models that require a Hugging Face API key in order to download into the vector-serve container. Not required.",
        &EMBEDDING_SERVICE_API_KEY,
        GucContext::Suset, GucFlags::default());

    GucRegistry::define_int_guc(
        "vectorize.num_bgw_proc",
        "Number of bgw processes",
        "Number of parallel background worker processes to run. Default is 1.",
        &NUM_BGW_PROC,
        1,
        10,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        "vectorize.embedding_req_timeout_sec",
        "Timeout, in seconds, for embedding transform requests",
        "Number of seconds to wait for an embedding http request to complete. Default is 120 seconds.",
        &EMBEDDING_REQ_TIMEOUT_SEC,
        1,
        1800,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.tembo_service_url",
        "Url for an Tembo AI service",
        "Url to Tembo's public AI hosting service",
        &TEMBO_SERVICE_HOST,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.tembo_jwt",
        "JWT for calling Tembo AI service",
        "JWT for calling Tembo AI service",
        &TEMBO_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.cohere_api_key",
        "API Key for calling Cohere Service",
        "API Key for calling Cohere Service",
        &COHERE_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.portkey_service_url",
        "Base url for the Portkey platform",
        "Base url for the Portkey platform",
        &PORTKEY_SERVICE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.portkey_api_key",
        "API Key for the Portkey platform",
        "API Key for the Portkey platform",
        &PORTKEY_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.portkey_virtual_key",
        "Virtual Key for the Portkey platform",
        "Virtual Key for the Portkey platform",
        &PORTKEY_VIRTUAL_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.voyage_service_url",
        "Base url for the Voyage AI platform",
        "Base url for the Voyage AI platform",
        &VOYAGE_SERVICE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.voyage_api_key",
        "API Key for the Voyage AI platform",
        "API Key for the Voyage AI platform",
        &VOYAGE_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        "vectorize.semantic_weight",
        "weight for semantic search",
        "weight for semantic search. default is 50",
        &SEMANTIC_WEIGHT,
        0,
        100,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        "vectorize.experimental_fts_index_type",
        "index type for hybrid search",
        "valid text index type. e.g. GIN",
        &FTS_INDEX_TYPE,
        GucContext::Suset,
        GucFlags::default(),
    );
}

/// a convenience function to get this project's GUCs
pub fn get_guc(guc: VectorizeGuc) -> Option<String> {
    let val = match guc {
        VectorizeGuc::Host => VECTORIZE_HOST.get(),
        VectorizeGuc::DatabaseName => VECTORIZE_DATABASE_NAME.get(),
        VectorizeGuc::OpenAIKey => OPENAI_KEY.get(),
        VectorizeGuc::EmbeddingServiceUrl => EMBEDDING_SERVICE_HOST.get(),
        VectorizeGuc::OllamaServiceUrl => OLLAMA_SERVICE_HOST.get(),
        VectorizeGuc::OpenAIServiceUrl => OPENAI_BASE_URL.get(),
        VectorizeGuc::EmbeddingServiceApiKey => EMBEDDING_SERVICE_API_KEY.get(),
        VectorizeGuc::CohereApiKey => COHERE_API_KEY.get(),
        VectorizeGuc::PortkeyApiKey => PORTKEY_API_KEY.get(),
        VectorizeGuc::PortkeyVirtualKey => PORTKEY_VIRTUAL_KEY.get(),
        VectorizeGuc::PortkeyServiceUrl => PORTKEY_SERVICE_URL.get(),
        VectorizeGuc::VoyageApiKey => VOYAGE_API_KEY.get(),
        VectorizeGuc::VoyageServiceUrl => VOYAGE_SERVICE_URL.get(),
        VectorizeGuc::TextIndexType => FTS_INDEX_TYPE.get(),
    };
    if let Some(cstr) = val {
        if let Ok(s) = handle_cstr(cstr) {
            let interpolated = env_interpolate_string(&s).unwrap();
            Some(interpolated)
        } else {
            error!("failed to convert CStr to str");
        }
    } else {
        debug1!("no value set for GUC: {:?}", guc);
        None
    }
}

#[allow(dead_code)]
fn handle_cstr(cstr: &CStr) -> Result<String> {
    if let Ok(s) = cstr.to_str() {
        Ok(s.to_owned())
    } else {
        Err(anyhow::anyhow!("failed to convert CStr to str"))
    }
}

pub fn get_guc_configs(model_source: &ModelSource) -> ModelGucConfig {
    match model_source {
        ModelSource::OpenAI => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::OpenAIKey),
            service_url: get_guc(VectorizeGuc::OpenAIServiceUrl),
            virtual_key: None,
        },
        ModelSource::SentenceTransformers => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::EmbeddingServiceApiKey),
            service_url: get_guc(VectorizeGuc::EmbeddingServiceUrl),
            virtual_key: None,
        },
        ModelSource::Cohere => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::CohereApiKey),
            service_url: None,
            virtual_key: None,
        },
        ModelSource::Ollama => ModelGucConfig {
            api_key: None,
            service_url: get_guc(VectorizeGuc::OllamaServiceUrl),
            virtual_key: None,
        },
        ModelSource::Portkey => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::PortkeyApiKey),
            service_url: get_guc(VectorizeGuc::PortkeyServiceUrl),
            virtual_key: get_guc(VectorizeGuc::PortkeyVirtualKey),
        },
        ModelSource::Voyage => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::VoyageApiKey),
            service_url: get_guc(VectorizeGuc::VoyageServiceUrl),
            virtual_key: None,
        },
    }
}
