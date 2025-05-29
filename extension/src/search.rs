use crate::guc;
use crate::guc::get_guc_configs;
use crate::init;
use crate::job::{create_event_trigger, create_trigger_handler, initalize_table_job};
use crate::transformers::openai;
use crate::transformers::transform;
use crate::util;

use anyhow::{Context, Result};
use pgrx::prelude::*;
use pgrx::JsonB;
use serde_json::Value;
use std::collections::HashMap;
use vectorize_core::guc::VectorizeGuc;
use vectorize_core::query;
use vectorize_core::transformers::providers::get_provider;
use vectorize_core::transformers::providers::ollama::check_model_host;
use vectorize_core::types::{self, Model, ModelSource, TableMethod, VectorizeMeta};

#[allow(clippy::too_many_arguments)]
pub fn init_table(
    job_name: &str,
    schema: &str,
    table: &str,
    columns: Vec<String>,
    primary_key: &str,
    update_col: Option<String>,
    index_dist_type: types::IndexDist,
    transformer: &Model,
    table_method: TableMethod,
    // cron-like for a cron based update model, or 'realtime' for a trigger-based
    schedule: &str,
) -> Result<String> {
    // validate table method
    // realtime is only compatible with the join method
    if schedule == "realtime" && table_method != TableMethod::join {
        error!("realtime schedule is only compatible with the join table method");
    }

    if let Some(col) = &update_col {
        // validate update_col
        let update_time_dtype = init::get_column_datatype(schema, table, col)?;
        if update_time_dtype != "timestamp with time zone" {
            return Err(anyhow::anyhow!(
                "`update_col` must be of type `timestamp with time zone`. column: '{col}' is of type `{t}`", col=col, t=update_time_dtype
            ));
        }
    }

    // get prim key type
    let pkey_type = init::get_column_datatype(schema, table, primary_key)?;
    init::init_pgmq()?;

    let guc_configs = get_guc_configs(&transformer.source);
    // validate API key where necessary and collect any optional arguments
    // certain embedding services require an API key, e.g. openAI
    // key can be set in a GUC, so if its required but not provided in args, and not in GUC, error
    let optional_args = match transformer.source {
        ModelSource::OpenAI => {
            openai::validate_api_key(
                &guc_configs
                    .api_key
                    .clone()
                    .context("OpenAI key is required")?,
            )?;
            None
        }
        ModelSource::Tembo => {
            error!("Tembo not implemented for search yet");
        }
        ModelSource::Ollama => {
            let url = match guc::get_guc(VectorizeGuc::OllamaServiceUrl) {
                Some(k) => k,
                None => {
                    error!("failed to get Ollama url from GUC");
                }
            };
            let res = check_model_host(&url);
            match res {
                Ok(_) => {
                    info!("Model host active!");
                    None
                }
                Err(e) => {
                    error!("Error with model host: {:?}", e)
                }
            }
        }
        ModelSource::Portkey => Some(serde_json::json!({
            "virtual_key": guc_configs.virtual_key.clone().expect("Portkey virtual key is required")
        })),
        _ => None,
    };

    let provider = get_provider(
        &transformer.source,
        guc_configs.api_key.clone(),
        guc_configs.service_url.clone(),
        guc_configs.virtual_key.clone(),
    )?;

    // synchronous
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));
    let model_dim =
        match runtime.block_on(async { provider.model_dim(&transformer.api_name()).await }) {
            Ok(e) => e,
            Err(e) => {
                error!("error getting model dim: {}", e);
            }
        };

    let valid_params = types::JobParams {
        schema: schema.to_string(),
        relation: table.to_string(),
        columns: columns.clone(),
        update_time_col: update_col,
        table_method: table_method.clone(),
        primary_key: primary_key.to_string(),
        pkey_type,
        api_key: guc_configs.api_key.clone(),
        schedule: schedule.to_string(),
        args: optional_args,
    };
    let params =
        JsonB(serde_json::to_value(valid_params.clone()).expect("error serializing params"));

    // write job to table
    let init_job_q = query::init_job_query();
    // using SPI here because it is unlikely that this code will be run anywhere but inside the extension.
    // background worker will likely be moved to an external container or service in near future
    let ran: Result<_, spi::Error> = Spi::connect_mut(|c| {
        match c.update(
            &init_job_q,
            None,
            &[
                job_name.into(),
                index_dist_type.to_string().into(),
                transformer.to_string().into(),
                params.into(),
            ],
        ) {
            Ok(_) => (),
            Err(e) => {
                error!("error creating job: {}", e);
            }
        }
        Ok(())
    });
    ran?;

    let init_embed_q =
        init::init_embedding_table_query(job_name, &valid_params, &index_dist_type, model_dim);
    let ran_semantic: Result<_, spi::Error> = Spi::connect_mut(|c| {
        for q in init_embed_q {
            let _r = c.update(&q, None, &[])?;
        }
        Ok(())
    });
    if let Err(e) = ran_semantic {
        error!("error creating embedding table: {}", e);
    }

    match schedule {
        "realtime" => {
            // setup triggers
            // create the trigger if not exists
            let trigger_handler = create_trigger_handler(job_name, primary_key);
            let insert_trigger = create_event_trigger(job_name, schema, table, "INSERT");
            let update_trigger = create_event_trigger(job_name, schema, table, "UPDATE");
            let _: Result<_, spi::Error> = Spi::connect_mut(|c| {
                let _r = c.update(&trigger_handler, None, &[])?;
                let _r = c.update(&insert_trigger, None, &[])?;
                let _r = c.update(&update_trigger, None, &[])?;
                Ok(())
            });
        }
        _ => {
            // initialize cron
            init::init_cron(schedule, job_name)?;
            log!("Initialized cron job");
        }
    }
    // start with initial batch load
    initalize_table_job(job_name, &valid_params)?;
    Ok(format!("Successfully created job: {job_name}"))
}

pub fn full_text_search(
    job_name: &str,
    query: &str,
    return_columns: Vec<String>,
    num_results: i32,
) -> Result<Vec<JsonB>> {
    let project_meta: VectorizeMeta = util::get_vectorize_meta_spi(job_name)?;
    let proj_params: types::JobParams = serde_json::from_value(
        serde_json::to_value(project_meta.params).unwrap_or_else(|e| {
            error!("failed to serialize metadata: {}", e);
        }),
    )?;

    let search_columns = proj_params
        .columns
        .iter()
        .map(|col| format!("COALESCE({}, '')", col))
        .collect::<Vec<String>>()
        .join(" || ' ' || ");

    let query = format!(
        "SELECT {return_columns} FROM {schema}.{table}
             WHERE to_tsvector('english', {search_columns})
             @@ to_tsquery('english', '{query}')
             LIMIT {limit};",
        schema = proj_params.schema,
        table = proj_params.relation,
        return_columns = return_columns.join(", "),
        search_columns = search_columns, // Dynamically concatenate columns
        query = query
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" | ")
            .replace("'", "''"),
        limit = num_results
    );

    Spi::connect(|client| {
        let mut results: Vec<JsonB> = Vec::new();
        let tup_table = client.select(&query, None, &[])?;

        for row in tup_table {
            let mut row_result = serde_json::json!({});

            for col in &return_columns {
                let col_value: Option<Value> = row
                    .get_by_name::<JsonB, _>(col)
                    .ok()
                    .flatten()
                    .map(|v| v.0) // JSONB values
                    .or_else(|| {
                        row.get_by_name::<String, _>(col)
                            .ok()
                            .flatten()
                            .map(|v| serde_json::json!(v))
                    }) // TEXT values
                    .or_else(|| {
                        row.get_by_name::<i32, _>(col)
                            .ok()
                            .flatten()
                            .map(|v| serde_json::json!(v))
                    }); // INT values

                row_result[col] = col_value.unwrap_or(Value::Null);
            }

            results.push(JsonB(row_result));
        }

        Ok(results)
    })
}

pub fn rrf_score(rank: Option<i32>) -> f32 {
    let k = 60.0;
    match rank {
        Some(rank) => 1.0 / (rank as f32 + k),
        None => 0.0,
    }
}

#[derive(Debug)]
pub struct AllResults {
    data: JsonB,
    full_text_rank: Option<i32>,
    semantic_rank: Option<i32>,
    rrf_score: f32,
}

pub fn hybrid_search(
    job_name: &str,
    query: &str,
    api_key: Option<String>,
    return_columns: Vec<String>,
    num_results: i32,
    where_clause: Option<String>,
) -> Result<Vec<JsonB>> {
    let semantic_weight: i32 = guc::SEMANTIC_WEIGHT.get();

    // Getting the results from both full-text and semantic search
    // num_results * 2 to get a larger pool of results to rank
    let full_text_results =
        full_text_search(job_name, query, return_columns.clone(), num_results * 2)?;
    let semantic_results = search(
        job_name,
        query,
        api_key,
        return_columns,
        num_results * 2,
        where_clause,
    )?;

    // Use a HashMap with serde_json::Value as the key
    let mut combined_results_map: HashMap<Value, AllResults> = HashMap::new();

    // Process full-text search results to combine with semantic search results
    for (i, result) in full_text_results.iter().enumerate() {
        let json_value = result.0.clone(); // Extract serde_json::Value from JsonB

        combined_results_map
            .entry(json_value.clone())
            .and_modify(|entry| entry.full_text_rank = Some(i as i32))
            .or_insert(AllResults {
                data: JsonB(json_value),
                full_text_rank: Some(i as i32),
                semantic_rank: None,
                rrf_score: 0.0,
            });
    }

    // Process semantic search results
    for (i, result) in semantic_results.iter().enumerate() {
        let json_value = result.0.clone(); // Extract serde_json::Value from JsonB

        combined_results_map
            .entry(json_value.clone())
            .and_modify(|entry| entry.semantic_rank = Some(i as i32))
            .or_insert(AllResults {
                data: JsonB(json_value),
                full_text_rank: None,
                semantic_rank: Some(i as i32),
                rrf_score: 0.0,
            });
    }

    // Convert HashMap to Vec
    let mut all_results: Vec<AllResults> = combined_results_map.into_values().collect();

    // Calculate RRF score for each result, sum it and store in AllResults
    for result in all_results.iter_mut() {
        // Iterate mutably to update results
        let ft_score = (1.0 - semantic_weight as f32 / 100.0) * rrf_score(result.full_text_rank);
        let s_score = (semantic_weight as f32 / 100.0) * rrf_score(result.semantic_rank);
        let final_rrf_score = ft_score + s_score;
        result.rrf_score = final_rrf_score; // Store RRF score in AllResults
    }

    // Sort by RRF score
    all_results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap());

    let final_results: Vec<JsonB> = all_results
        .into_iter()
        .map(|res| {
            let mut result_json = res.data.0.clone(); // Extract original JSON structure

            // Inject score values at the top level
            result_json["full_text_rank"] = serde_json::Value::from(res.full_text_rank);
            result_json["semantic_rank"] = serde_json::Value::from(res.semantic_rank);
            result_json["rrf_score"] = serde_json::Value::from(res.rrf_score);

            JsonB(result_json) // Wrap back into JsonB
        })
        .collect();

    // Return only the top num_results
    Ok(final_results
        .into_iter()
        .take(num_results as usize)
        .collect())
}

pub fn search(
    job_name: &str,
    query: &str,
    api_key: Option<String>,
    return_columns: Vec<String>,
    num_results: i32,
    where_clause: Option<String>,
) -> Result<Vec<JsonB>> {
    let project_meta: VectorizeMeta = util::get_vectorize_meta_spi(job_name)?;
    let proj_params: types::JobParams = serde_json::from_value(
        serde_json::to_value(project_meta.params).unwrap_or_else(|e| {
            error!("failed to serialize metadata: {}", e);
        }),
    )
    .unwrap_or_else(|e| error!("failed to deserialize metadata: {}", e));

    let proj_api_key = match api_key {
        // if api passed in the function call, use that
        Some(k) => Some(k),
        // if not, use the one from the project metadata
        None => proj_params.api_key.clone(),
    };
    let embeddings = transform(query, &project_meta.transformer, proj_api_key);

    match project_meta.index_dist_type {
        types::IndexDist::pgv_hnsw_l2 => error!("Not implemented."),
        types::IndexDist::pgv_hnsw_ip => error!("Not implemented."),
        types::IndexDist::pgv_hnsw_cosine | types::IndexDist::vsc_diskann_cosine => {
            cosine_similarity_search(
                job_name,
                &proj_params,
                &return_columns,
                num_results,
                &embeddings[0],
                where_clause,
            )
        }
    }
}

pub fn cosine_similarity_search(
    project: &str,
    job_params: &types::JobParams,
    return_columns: &[String],
    num_results: i32,
    embeddings: &[f64],
    where_clause: Option<String>,
) -> Result<Vec<JsonB>> {
    let schema = job_params.schema.clone();
    let table = job_params.relation.clone();

    // switch on table method
    let query = match job_params.table_method {
        TableMethod::append => single_table_cosine_similarity(
            project,
            &schema,
            &table,
            return_columns,
            num_results,
            where_clause,
        ),
        TableMethod::join => join_table_cosine_similarity(
            project,
            job_params,
            return_columns,
            num_results,
            where_clause,
        ),
    };
    Spi::connect(|client| {
        let mut results: Vec<JsonB> = Vec::new();
        let tup_table = client.select(&query, None, &[embeddings.into()])?;
        for row in tup_table {
            match row["results"].value()? {
                Some(r) => results.push(r),
                None => error!("failed to get results"),
            }
        }
        Ok(results)
    })
}

fn join_table_cosine_similarity(
    project: &str,
    job_params: &types::JobParams,
    return_columns: &[String],
    num_results: i32,
    where_clause: Option<String>,
) -> String {
    let schema = job_params.schema.clone();
    let table = job_params.relation.clone();
    let join_key = &job_params.primary_key;
    let cols = &return_columns
        .iter()
        .map(|s| format!("t0.{}", s))
        .collect::<Vec<_>>()
        .join(",");

    let where_str = if let Some(w) = where_clause {
        prepare_filter(&w, join_key)
    } else {
        "".to_string()
    };
    let inner_query = format!(
        "
    SELECT
        {join_key},
        1 - (embeddings <=> $1::vector) AS similarity_score
    FROM vectorize._embeddings_{project}
    ORDER BY similarity_score DESC
    "
    );
    format!(
        "
    SELECT to_jsonb(t) as results
    FROM (
        SELECT {cols}, t1.similarity_score
        FROM
            (
                {inner_query}
            ) t1
        INNER JOIN {schema}.{table} t0 on t0.{join_key} = t1.{join_key}
        {where_str}
    ) t
    ORDER BY t.similarity_score DESC
    LIMIT {num_results};
    "
    )
}

fn single_table_cosine_similarity(
    project: &str,
    schema: &str,
    table: &str,
    return_columns: &[String],
    num_results: i32,
    where_clause: Option<String>,
) -> String {
    let where_str = if let Some(w) = where_clause {
        format!("AND {}", w)
    } else {
        "".to_string()
    };
    format!(
        "
    SELECT to_jsonb(t) as results
    FROM (
        SELECT 
        1 - ({project}_embeddings <=> $1::vector) AS similarity_score,
        {cols}
    FROM {schema}.{table}
    WHERE {project}_updated_at is NOT NULL
    {where_str}
    ORDER BY similarity_score DESC
    LIMIT {num_results}
    ) t
    ",
        cols = return_columns.join(", "),
    )
}

// transform user's where_sql into the format search query expects
fn prepare_filter(filter: &str, pkey: &str) -> String {
    let wc = filter.replace(pkey, &format!("t0.{}", pkey));
    format!("AND {wc}")
}
