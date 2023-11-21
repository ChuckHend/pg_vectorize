use crate::executor::VectorizeMeta;
use crate::guc;
use crate::guc::get_guc;
use crate::init;
use crate::search::cosine_similarity_search;
use crate::transformers::{
    http_handler::openai_embedding_request, openai, openai::OPENAI_EMBEDDING_MODEL,
    openai::OPENAI_EMBEDDING_URL, types::EmbeddingPayload, types::EmbeddingRequest,
};
use crate::types;
use crate::types::JobParams;
use crate::util;
use anyhow::Result;
use pgrx::prelude::*;

#[allow(clippy::too_many_arguments)]
#[pg_extern]
fn table(
    table: &str,
    columns: Vec<String>,
    job_name: String,
    primary_key: String,
    args: default!(pgrx::Json, "'{}'"),
    schema: default!(String, "'public'"),
    update_col: default!(String, "'last_updated_at'"),
    transformer: default!(types::Transformer, "'openai'"),
    search_alg: default!(types::SimilarityAlg, "'pgv_cosine_similarity'"),
    table_method: default!(types::TableMethod, "'append'"),
    schedule: default!(String, "'* * * * *'"),
) -> Result<String> {
    let job_type = types::JobType::Columns;

    // write job to table
    let init_job_q = init::init_job_query();
    let arguments = match serde_json::to_value(args) {
        Ok(a) => a,
        Err(e) => {
            error!("invalid json for argument `args`: {}", e);
        }
    };
    let api_key = arguments.get("api_key");

    // get prim key type
    let pkey_type = init::get_column_datatype(&schema, table, &primary_key);

    // certain embedding services require an API key, e.g. openAI
    // key can be set in a GUC, so if its required but not provided in args, and not in GUC, error
    init::init_pgmq(&transformer)?;
    match transformer {
        types::Transformer::openai => {
            let openai_key = match api_key {
                Some(k) => serde_json::from_value::<String>(k.clone())?,
                None => match guc::get_guc(guc::VectorizeGuc::OpenAIKey) {
                    Some(k) => k,
                    None => {
                        error!("failed to get API key from GUC");
                    }
                },
            };
            openai::validate_api_key(&openai_key)?;
        }
        // no-op
        types::Transformer::all_MiniLM_L12_v2 => (),
    }

    let valid_params = types::JobParams {
        schema: schema.clone(),
        table: table.to_string(),
        columns: columns.clone(),
        update_time_col: update_col,
        table_method: table_method.clone(),
        primary_key,
        pkey_type,
        api_key: api_key
            .map(|k| serde_json::from_value::<String>(k.clone()).expect("error parsing api key")),
    };
    let params = pgrx::JsonB(serde_json::to_value(valid_params).expect("error serializing params"));

    // using SPI here because it is unlikely that this code will be run anywhere but inside the extension.
    // background worker will likely be moved to an external container or service in near future
    let ran: Result<_, spi::Error> = Spi::connect(|mut c| {
        match c.update(
            &init_job_q,
            None,
            Some(vec![
                (PgBuiltInOids::TEXTOID.oid(), job_name.clone().into_datum()),
                (
                    PgBuiltInOids::TEXTOID.oid(),
                    job_type.to_string().into_datum(),
                ),
                (
                    PgBuiltInOids::TEXTOID.oid(),
                    transformer.to_string().into_datum(),
                ),
                (
                    PgBuiltInOids::TEXTOID.oid(),
                    search_alg.to_string().into_datum(),
                ),
                (PgBuiltInOids::JSONBOID.oid(), params.into_datum()),
            ]),
        ) {
            Ok(_) => (),
            Err(e) => {
                error!("error creating job: {}", e);
            }
        }
        Ok(())
    });
    if ran.is_err() {
        error!("error creating job");
    }
    let init_embed_q = init::init_embedding_table_query(
        &job_name,
        &schema,
        table,
        &transformer,
        &search_alg,
        &table_method,
    );

    let ran: Result<_, spi::Error> = Spi::connect(|mut c| {
        for q in init_embed_q {
            let _r = c.update(&q, None, None)?;
        }
        Ok(())
    });
    if let Err(e) = ran {
        error!("error creating embedding table: {}", e);
    }
    // TODO: first batch update
    // initialize cron
    let _ = init::init_cron(&schedule, &job_name); // handle this error
    Ok(format!("Successfully created job: {job_name}"))
}

#[pg_extern]
fn search(
    job_name: &str,
    query: &str,
    api_key: default!(Option<String>, "NULL"),
    return_columns: default!(Vec<String>, "ARRAY['*']::text[]"),
    num_results: default!(i32, 10),
) -> Result<TableIterator<'static, (name!(search_results, pgrx::JsonB),)>, spi::Error> {
    let project_meta: VectorizeMeta = if let Ok(Some(js)) = util::get_vectorize_meta_spi(job_name) {
        js
    } else {
        error!("failed to get project metadata");
    };
    let proj_params: JobParams = serde_json::from_value(
        serde_json::to_value(project_meta.params).unwrap_or_else(|e| {
            error!("failed to serialize metadata: {}", e);
        }),
    )
    .unwrap_or_else(|e| error!("failed to deserialize metadata: {}", e));

    // get embeddings
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));

    let schema = proj_params.schema;
    let table = proj_params.table;

    let embedding_request = match project_meta.transformer {
        types::Transformer::openai => {
            let openai_key = match api_key {
                Some(k) => k,
                None => match guc::get_guc(guc::VectorizeGuc::OpenAIKey) {
                    Some(k) => k,
                    None => {
                        error!("failed to get API key from GUC");
                    }
                },
            };

            let embedding_request = EmbeddingPayload {
                input: vec![query.to_string()],
                model: OPENAI_EMBEDDING_MODEL.to_string(),
            };
            EmbeddingRequest {
                url: OPENAI_EMBEDDING_URL.to_owned(),
                payload: embedding_request,
                api_key: Some(openai_key),
            }
        }
        types::Transformer::all_MiniLM_L12_v2 => {
            let url: String = get_guc(guc::VectorizeGuc::EmbeddingServiceUrl)
                .expect("failed to get embedding service url from GUC");
            let embedding_request = EmbeddingPayload {
                input: vec![query.to_string()],
                model: project_meta.transformer.to_string(),
            };
            EmbeddingRequest {
                url,
                payload: embedding_request,
                api_key: None,
            }
        }
    };

    let embeddings =
        match runtime.block_on(async { openai_embedding_request(embedding_request).await }) {
            Ok(e) => e,
            Err(e) => {
                error!("error getting embeddings: {}", e);
            }
        };

    let search_results = match project_meta.search_alg {
        types::SimilarityAlg::pgv_cosine_similarity => cosine_similarity_search(
            job_name,
            &schema,
            &table,
            &return_columns,
            num_results,
            &embeddings[0],
        )?,
    };

    Ok(TableIterator::new(search_results))
}
