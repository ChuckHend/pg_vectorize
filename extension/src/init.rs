use crate::guc;
use pgrx::prelude::*;

use anyhow::{anyhow, Context, Result};
use vectorize_core::guc::VectorizeGuc;
use vectorize_core::query::{self, check_input};
use vectorize_core::types::IndexDist;
use vectorize_core::types::{JobParams, TableMethod, VECTORIZE_SCHEMA};
pub static VECTORIZE_QUEUE: &str = "vectorize_jobs";

pub fn init_pgmq() -> Result<()> {
    // check if queue already created:
    let queue_exists: bool = Spi::get_one(&format!(
        "SELECT EXISTS (SELECT 1 FROM pgmq.meta WHERE queue_name = '{VECTORIZE_QUEUE}');",
    ))?
    .context("error checking if queue exists")?;
    if queue_exists {
        debug1!("queue already exists");
        return Ok(());
    } else {
        debug1!("creating queue;");
        let ran: Result<_, spi::Error> = Spi::connect_mut(|c| {
            let _r = c.update(
                &format!("SELECT pgmq.create('{VECTORIZE_QUEUE}');"),
                None,
                &[],
            )?;
            Ok(())
        });
        if let Err(e) = ran {
            error!("error creating job queue: {}", e);
        }
    }
    Ok(())
}

pub fn init_cron(cron: &str, job_name: &str) -> Result<Option<i64>, spi::Error> {
    // Skip scheduling for manual schedule
    if cron == "manual" {
        return Ok(None);
    }

    let cronjob = format!(
        "
        SELECT cron.schedule(
            '{job_name}',
            '{cron}',
            $$select vectorize.job_execute('{job_name}')$$
        )
        ;"
    );
    Spi::get_one(&cronjob)
}

pub fn init_embedding_table_query(
    job_name: &str,
    job_params: &JobParams,
    index_type: &IndexDist,
    model_dim: u32,
) -> Vec<String> {
    query::check_input(job_name).expect("invalid job name");
    let src_schema = job_params.schema.clone();
    let src_table = job_params.relation.clone();

    let col_type = format!("vector({model_dim})");

    let (index_schema, table_name, embeddings_col) = match job_params.table_method {
        TableMethod::append => {
            let embeddings_col = format!("{job_name}_embeddings");
            (src_schema.clone(), src_table.clone(), embeddings_col)
        }
        TableMethod::join => {
            let table_name = format!("_embeddings_{}", job_name);

            (
                VECTORIZE_SCHEMA.to_string(),
                table_name.to_string(),
                "embeddings".to_string(),
            )
        }
    };

    let index_stmt = match index_type {
        IndexDist::pgv_hnsw_cosine => {
            query::create_hnsw_cosine_index(job_name, &index_schema, &table_name, &embeddings_col)
        }
        IndexDist::vsc_diskann_cosine => {
            create_diskann_index(job_name, &index_schema, &table_name, &embeddings_col)
        }
        IndexDist::pgv_hnsw_ip => {
            query::create_hnsw_ip_index(job_name, &index_schema, &table_name, &embeddings_col)
        }
        IndexDist::pgv_hnsw_l2 => {
            query::create_hnsw_l2_index(job_name, &index_schema, &table_name, &embeddings_col)
        }
    };

    match job_params.table_method {
        TableMethod::append => {
            vec![
                append_embedding_column(job_name, &src_schema, &src_table, &col_type),
                index_stmt,
            ]
        }
        TableMethod::join => {
            let mut stmts = vec![
                query::create_embedding_table(
                    job_name,
                    &job_params.primary_key,
                    &job_params.pkey_type,
                    &col_type,
                    &src_schema,
                    &src_table,
                ),
                index_stmt,
                // also create a view over the source table and the embedding table, for this project
                query::drop_project_view(job_name),
                query::create_project_view(
                    job_name,
                    &job_params.schema,
                    &job_params.relation,
                    &job_params.primary_key,
                ),
            ];

            // Currently creating a GIN index within this function
            // TODO: Find a long term solution for this
            if let Some(indx_type_guc) = guc::get_guc(VectorizeGuc::TextIndexType) {
                stmts.push(query::init_index_query(
                    job_name,
                    &indx_type_guc,
                    job_params,
                ))
            }
            stmts
        }
    }
}

fn create_diskann_index(job_name: &str, schema: &str, table: &str, embedding_col: &str) -> String {
    format!(
        "CREATE INDEX IF NOT EXISTS {job_name}_diskann_idx ON {schema}.{table}
        USING diskann ({embedding_col});
        ",
    )
}

fn append_embedding_column(job_name: &str, schema: &str, table: &str, col_type: &str) -> String {
    check_input(job_name).expect("invalid job name");
    format!(
        "
        DO $$
        BEGIN
           IF NOT EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_name = '{table}'
                AND table_schema = '{schema}'
                AND column_name = '{job_name}_embeddings'
            )
            THEN ALTER TABLE {schema}.{table}
            ADD COLUMN {job_name}_embeddings {col_type},
            ADD COLUMN {job_name}_updated_at TIMESTAMP WITH TIME ZONE;
           END IF;
        END
        $$;
        ",
    )
}

pub fn get_column_datatype(schema: &str, table: &str, column: &str) -> Result<String> {
    Spi::get_one_with_args(
        "
        SELECT data_type
        FROM information_schema.columns
        WHERE
            table_schema = $1
            AND table_name = $2
            AND column_name = $3    
        ",
        &[schema.into(), table.into(), column.into()],
    )
    .map_err(|_| {
        anyhow!(
            "One of schema:`{}`, table:`{}`, column:`{}` does not exist.",
            schema,
            table,
            column
        )
    })?
    .ok_or_else(|| {
        anyhow!(
            "An unknown error occurred while fetching the data type for column `{}` in `{}.{}`.",
            schema,
            table,
            column
        )
    })
}
