use crate::executor::{create_batches, new_rows_query, JobMessage, VectorizeMeta};
use crate::guc::{self, BATCH_SIZE};
use crate::init::{self, VECTORIZE_QUEUE};
use crate::job::{create_insert_trigger, create_trigger_handler, create_update_trigger};
use crate::search::cosine_similarity_search;
use crate::transformers::http_handler::sync_get_model_info;
use crate::transformers::types::Inputs;
use crate::transformers::{openai, transform};
use crate::types;
use crate::util;
use anyhow::Result;
use pgrx::prelude::*;
use tiktoken_rs::cl100k_base;

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
    transformer: default!(String, "'text-embedding-ada-002'"),
    search_alg: default!(types::SimilarityAlg, "'pgv_cosine_similarity'"),
    table_method: default!(types::TableMethod, "'append'"),
    // cron-like for a cron based update model, or 'realtime' for a trigger-based
    schedule: default!(String, "'realtime'"),
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
    init::init_pgmq()?;

    // certain embedding services require an API key, e.g. openAI
    // key can be set in a GUC, so if its required but not provided in args, and not in GUC, error
    match transformer.as_ref() {
        "text-embedding-ada-002" => {
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
        t => {
            // make sure transformer exists
            let _ = sync_get_model_info(t).expect("transformer does not exist");
        }
    }

    let valid_params = types::JobParams {
        schema: schema.clone(),
        table: table.to_string(),
        columns: columns.clone(),
        update_time_col: update_col,
        table_method: table_method.clone(),
        primary_key: primary_key.clone(),
        pkey_type,
        api_key: api_key
            .map(|k| serde_json::from_value::<String>(k.clone()).expect("error parsing api key")),
    };
    let params =
        pgrx::JsonB(serde_json::to_value(valid_params.clone()).expect("error serializing params"));

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
    let init_embed_q =
        init::init_embedding_table_query(&job_name, &schema, table, &transformer, &table_method);

    let ran: Result<_, spi::Error> = Spi::connect(|mut c| {
        for q in init_embed_q {
            let _r = c.update(&q, None, None)?;
        }
        Ok(())
    });
    if let Err(e) = ran {
        error!("error creating embedding table: {}", e);
    }
    match schedule.as_ref() {
        "realtime" => {
            // setup triggers
            // create the trigger if not exists
            let trigger_handler = create_trigger_handler(&job_name, &columns, &primary_key);
            let insert_trigger = create_insert_trigger(&job_name, table);
            let update_trigger = create_update_trigger(&job_name, table, &columns);

            let _: Result<_, spi::Error> = Spi::connect(|mut c| {
                let _r = c.update(&trigger_handler, None, None)?;
                let _r = c.update(&insert_trigger, None, None)?;
                let _r = c.update(&update_trigger, None, None)?;
                Ok(())
            });

            // start with initial batch load
            let rows_need_update_query: String = new_rows_query(&job_name, &valid_params);
            let mut inputs: Vec<Inputs> = Vec::new();
            let bpe = cl100k_base().unwrap();
            let _: Result<_, spi::Error> = Spi::connect(|c| {
                let rows = c.select(&rows_need_update_query, None, None)?;
                for row in rows {
                    let ipt = row["input_text"]
                        .value::<String>()?
                        .expect("input_text is null");
                    let token_estimate = bpe.encode_with_special_tokens(&ipt).len() as i32;
                    inputs.push(Inputs {
                        record_id: row["record_id"]
                            .value::<String>()?
                            .expect("record_id is null"),
                        inputs: ipt,
                        token_estimate,
                    });
                }
                Ok(())
            });
            let max_batch_size = BATCH_SIZE.get();
            let batches = create_batches(inputs, max_batch_size);
            let vectorize_meta = VectorizeMeta {
                name: job_name.clone(),
                // TODO: in future, lookup job id once this gets put into use
                // job_id is currently not used, job_name is unique
                job_id: 0,
                job_type: job_type.clone(),
                params: serde_json::to_value(valid_params.clone()).unwrap(),
                transformer: transformer.clone(),
                search_alg: search_alg.clone(),
                last_completion: None,
            };
            for b in batches {
                let job_message = JobMessage {
                    job_name: job_name.clone(),
                    job_meta: vectorize_meta.clone(),
                    inputs: b,
                };
                let query = format!(
                    "select pgmq.send('{VECTORIZE_QUEUE}', '{}');",
                    serde_json::to_string(&job_message).unwrap()
                );
                let _ran: Result<_, spi::Error> = Spi::connect(|mut c| {
                    let _r = c.update(&query, None, None)?;
                    Ok(())
                });
            }
        }
        _ => {
            // initialize cron
            init::init_cron(&schedule, &job_name)?;
            log!("Initialized cron job");
        }
    }
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
    let proj_params: types::JobParams = serde_json::from_value(
        serde_json::to_value(project_meta.params).unwrap_or_else(|e| {
            error!("failed to serialize metadata: {}", e);
        }),
    )
    .unwrap_or_else(|e| error!("failed to deserialize metadata: {}", e));

    let schema = proj_params.schema;
    let table = proj_params.table;

    let embeddings = transform(query, &project_meta.transformer, api_key);

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

#[pg_extern]
fn transform_embeddings(
    input: &str,
    model_name: default!(String, "'text-embedding-ada-002'"),
    api_key: default!(Option<String>, "NULL"),
) -> Result<Vec<f64>, spi::Error> {
    Ok(transform(input, &model_name, api_key).remove(0))
}
