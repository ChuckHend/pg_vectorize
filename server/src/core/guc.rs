// for handling of GUCs that can be error prone
#[derive(Clone, Debug)]
pub enum VectorizeGuc {
    Host,
    DatabaseName,
    OpenAIServiceUrl,
    OpenAIKey,
    TemboAIKey,
    EmbeddingServiceUrl,
    EmbeddingServiceApiKey,
    OllamaServiceUrl,
    TemboServiceUrl,
    CohereApiKey,
    PortkeyApiKey,
    PortkeyVirtualKey,
    PortkeyServiceUrl,
    VoyageApiKey,
    VoyageServiceUrl,
    TextIndexType,
}

#[derive(Clone, Debug)]
pub struct ModelGucConfig {
    pub api_key: Option<String>,
    pub service_url: Option<String>,
    pub virtual_key: Option<String>,
}

use sqlx::PgPool;

use crate::core::types::ModelSource;
pub async fn get_guc(guc: VectorizeGuc, pool: &PgPool) -> Option<String> {
    let guc_name = match guc {
        VectorizeGuc::Host => "host",
        VectorizeGuc::DatabaseName => "database_name",
        VectorizeGuc::OpenAIServiceUrl => "openai_service_url",
        VectorizeGuc::OpenAIKey => "openai_key",
        VectorizeGuc::TemboAIKey => "tembo_jwt",
        VectorizeGuc::EmbeddingServiceUrl => "embedding_service_url",
        VectorizeGuc::EmbeddingServiceApiKey => "embedding_service_api_key",
        VectorizeGuc::OllamaServiceUrl => "ollama_service_url",
        VectorizeGuc::TemboServiceUrl => "tembo_service_url",
        VectorizeGuc::CohereApiKey => "cohere_api_key",
        VectorizeGuc::PortkeyApiKey => "portkey_api_key",
        VectorizeGuc::PortkeyVirtualKey => "portkey_virtual_key",
        VectorizeGuc::PortkeyServiceUrl => "portkey_service_url",
        VectorizeGuc::VoyageApiKey => "voyage_api_key",
        VectorizeGuc::VoyageServiceUrl => "voyage_service_url",
        VectorizeGuc::TextIndexType => "experimental_fts_index_type",
    };
    let query = format!("SHOW vectorize.{}", guc_name);
    let row: (String,) = sqlx::query_as(&query)
        .fetch_one(pool)
        .await
        .expect("failed to fetch GUC value");

    Some(row.0)
}

pub async fn get_guc_configs(model_source: &ModelSource, pool: &PgPool) -> ModelGucConfig {
    match model_source {
        ModelSource::OpenAI => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::OpenAIKey, pool).await,
            service_url: get_guc(VectorizeGuc::OpenAIServiceUrl, pool).await,
            virtual_key: None,
        },
        ModelSource::Tembo => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::TemboAIKey, pool).await,
            service_url: get_guc(VectorizeGuc::TemboServiceUrl, pool).await,
            virtual_key: None,
        },
        ModelSource::SentenceTransformers => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::EmbeddingServiceApiKey, pool).await,
            service_url: get_guc(VectorizeGuc::EmbeddingServiceUrl, pool).await,
            virtual_key: None,
        },
        ModelSource::Cohere => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::CohereApiKey, pool).await,
            service_url: None,
            virtual_key: None,
        },
        ModelSource::Ollama => ModelGucConfig {
            api_key: None,
            service_url: get_guc(VectorizeGuc::OllamaServiceUrl, pool).await,
            virtual_key: None,
        },
        ModelSource::Portkey => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::PortkeyApiKey, pool).await,
            service_url: get_guc(VectorizeGuc::PortkeyServiceUrl, pool).await,
            virtual_key: get_guc(VectorizeGuc::PortkeyVirtualKey, pool).await,
        },
        ModelSource::Voyage => ModelGucConfig {
            api_key: get_guc(VectorizeGuc::VoyageApiKey, pool).await,
            service_url: get_guc(VectorizeGuc::VoyageServiceUrl, pool).await,
            virtual_key: None,
        },
    }
}
