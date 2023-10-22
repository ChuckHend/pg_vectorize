use crate::executor::ColumnJobParams;
use crate::init;
use crate::openai::get_embeddings;
use crate::search::cosine_similarity_search;
use crate::types;
use crate::util;
use anyhow::Result;
use pgrx::prelude::*;

#[pg_extern]
fn table(
    table: &str,
    columns: Vec<String>,
    job_name: Option<String>,
    args: pgrx::Json,
    primary_key: String,
    schema: default!(String, "'public'"),
    update_col: default!(String, "'last_updated_at'"),
    transformer: default!(types::Transformer, "'openai'"),
    search_alg: default!(types::SimilarityAlg, "'pgv_cosine_similarity'"),
    table_method: default!(init::TableMethod, "'append'"),
    schedule: default!(String, "'* * * * *'"),
) -> Result<String> {
    // initialize pgmq
    init::init_pgmq()?;
    let job_type = types::JobType::Columns;

    // write job to table
    let init_job_q = init::init_job_query();
    let job_name = match job_name {
        Some(a) => a,
        None => format!("{}_{}_{}", schema, table, columns.join("_")),
    };
    let arguments = match serde_json::to_value(args) {
        Ok(a) => a,
        Err(e) => {
            error!("invalid json for argument `args`: {}", e);
        }
    };
    let api_key = arguments.get("api_key");

    // get prim key type
    let pkey_type = init::get_column_datatype(&schema, table, &primary_key);
    // TODO: implement a struct for these params
    let params = pgrx::JsonB(serde_json::json!({
        "schema": schema,
        "table": table,
        "columns": columns,
        "update_time_col": update_col,
        "table_method": table_method,
        "primary_key": primary_key,
        "pkey_type": pkey_type,
        "api_key": api_key
    }));

    // using SPI here because it is unlikely that this code will be run anywhere but inside the extension
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
        let _r = c.update(&init_embed_q, None, None)?;
        Ok(())
    });
    if let Err(e) = ran {
        error!("error creating embedding table: {}", e);
    }
    // TODO: first batch update
    // initialize cron
    let _ = init::init_cron(&schedule, &job_name); // handle this error
    Ok(format!(
        "{schema}.{table}.{columns:?}.{transformer}.{search_alg}"
    ))
}

#[pg_extern]
fn search(
    job_name: &str,
    return_col: &str,
    query: &str,
    api_key: &str,
    num_results: default!(i32, 10),
) -> Result<TableIterator<'static, (name!(search_results, pgrx::JsonB),)>, spi::Error> {
    // note: this is not the most performant implementation
    // this requires a query to metadata table to get the projects schema and table, which has a cost
    // this does ensure consistency between the model used to generate the stored embeddings and the query embeddings, which is crucial

    // get project metadata
    let _project_meta = if let Some(js) = util::get_vectorize_meta_spi(job_name) {
        js
    } else {
        error!("failed to get project metadata");
    };
    let project_meta: ColumnJobParams =
        serde_json::from_value(serde_json::to_value(_project_meta).unwrap_or_else(|e| {
            error!("failed to serialize metadata: {}", e);
        }))
        .unwrap_or_else(|e| error!("failed to serialize metadata: {}", e));
    // assuming default openai API for now
    // get embeddings
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));

    let schema = project_meta.schema;
    let table = project_meta.table;

    let embeddings =
        match runtime.block_on(async { get_embeddings(&vec![query.to_string()], api_key).await }) {
            Ok(e) => e,
            Err(e) => {
                error!("error getting embeddings: {}", e);
            }
        };
    let search_results = cosine_similarity_search(
        job_name,
        &schema,
        &table,
        return_col,
        num_results,
        &embeddings[0],
    )?;
    Ok(TableIterator::new(search_results.into_iter()))
}
