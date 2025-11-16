use std::ffi::CString;

use pgrx::*;

use crate::transformers::generic::env_interpolate_string;
use vectorize_core::guc::{ModelGucConfig, VectorizeGuc};
use vectorize_core::types::ModelSource;

pub static VECTORIZE_HOST: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);
pub static VECTORIZE_DATABASE_NAME: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static OPENAI_BASE_URL: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(Some(c"https://api.openai.com/v1"));
pub static OPENAI_KEY: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);
pub static BATCH_SIZE: GucSetting<i32> = GucSetting::<i32>::new(10000);
pub static NUM_BGW_PROC: GucSetting<i32> = GucSetting::<i32>::new(1);
pub static EMBEDDING_SERVICE_API_KEY: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static EMBEDDING_SERVICE_HOST: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static EMBEDDING_REQ_TIMEOUT_SEC: GucSetting<i32> = GucSetting::<i32>::new(120);
pub static OLLAMA_SERVICE_HOST: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static COHERE_API_KEY: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);
pub static PORTKEY_API_KEY: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);
pub static PORTKEY_VIRTUAL_KEY: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static PORTKEY_SERVICE_URL: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static VOYAGE_API_KEY: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);
pub static VOYAGE_SERVICE_URL: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(None);
pub static SEMANTIC_WEIGHT: GucSetting<i32> = GucSetting::<i32>::new(50);
// EXPERIMENTAL
pub static FTS_INDEX_TYPE: GucSetting<Option<CString>> = GucSetting::<Option<CString>>::new(None);

// initialize GUCs
pub fn init_guc() {
    GucRegistry::define_string_guc(
        c"vectorize.host",
        c"unix socket url for Postgres",
        c"unix socket path to the Postgres instance. Optional. Can also be set in environment variable.",
        &VECTORIZE_HOST,
        GucContext::Suset, GucFlags::default()
    );

    GucRegistry::define_string_guc(
        c"vectorize.database_name",
        c"Target database for vectorize operations",
        c"Specifies the target database for vectorize operations.",
        &VECTORIZE_DATABASE_NAME,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.openai_service_url",
        c"Base url to the OpenAI Server",
        c"Url to any OpenAI compatible service.",
        &OPENAI_BASE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.openai_key",
        c"API key from OpenAI",
        c"API key from OpenAI. Optional. Overridden by any values provided in function calls.",
        &OPENAI_KEY,
        GucContext::Suset,
        GucFlags::SUPERUSER_ONLY,
    );

    GucRegistry::define_string_guc(
        c"vectorize.ollama_service_url",
        c"Ollama server url",
        c"Scheme, host, and port of the Ollama server",
        &OLLAMA_SERVICE_HOST,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"vectorize.batch_size",
        c"Vectorize job batch size",
        c"Number of records that can be included in a single vectorize job.",
        &BATCH_SIZE,
        1,
        100000,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.embedding_service_url",
        c"Url for an OpenAI compatible embedding service",
        c"Url to a service with request and response schema consistent with OpenAI's embeddings API.",
        &EMBEDDING_SERVICE_HOST,
        GucContext::Suset, GucFlags::default());

    GucRegistry::define_string_guc(
        c"vectorize.embedding_service_api_key",
        c"API key for vector-serve container",
        c"Used for any models that require a Hugging Face API key in order to download into the vector-serve container. Not required.",
        &EMBEDDING_SERVICE_API_KEY,
        GucContext::Suset, GucFlags::default());

    GucRegistry::define_int_guc(
        c"vectorize.num_bgw_proc",
        c"Number of bgw processes",
        c"Number of parallel background worker processes to run. Default is 1.",
        &NUM_BGW_PROC,
        1,
        10,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"vectorize.embedding_req_timeout_sec",
        c"Timeout, in seconds, for embedding transform requests",
        c"Number of seconds to wait for an embedding http request to complete. Default is 120 seconds.",
        &EMBEDDING_REQ_TIMEOUT_SEC,
        1,
        1800,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.cohere_api_key",
        c"API Key for calling Cohere Service",
        c"API Key for calling Cohere Service",
        &COHERE_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.portkey_service_url",
        c"Base url for the Portkey platform",
        c"Base url for the Portkey platform",
        &PORTKEY_SERVICE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.portkey_api_key",
        c"API Key for the Portkey platform",
        c"API Key for the Portkey platform",
        &PORTKEY_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.portkey_virtual_key",
        c"Virtual Key for the Portkey platform",
        c"Virtual Key for the Portkey platform",
        &PORTKEY_VIRTUAL_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.voyage_service_url",
        c"Base url for the Voyage AI platform",
        c"Base url for the Voyage AI platform",
        &VOYAGE_SERVICE_URL,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.voyage_api_key",
        c"API Key for the Voyage AI platform",
        c"API Key for the Voyage AI platform",
        &VOYAGE_API_KEY,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"vectorize.semantic_weight",
        c"weight for semantic search",
        c"weight for semantic search. default is 50",
        &SEMANTIC_WEIGHT,
        0,
        100,
        GucContext::Suset,
        GucFlags::default(),
    );

    GucRegistry::define_string_guc(
        c"vectorize.experimental_fts_index_type",
        c"index type for hybrid search",
        c"valid text index type. e.g. GIN",
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
    if let Some(cstring) = val {
        let s = cstring.to_str().expect("failed to convert CString to str");
        let interpolated = env_interpolate_string(s).unwrap();
        Some(interpolated)
    } else {
        debug1!("no value set for GUC: {:?}", guc);
        None
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
