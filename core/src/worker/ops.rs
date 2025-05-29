use crate::transformers::types::PairedEmbeddings;
use crate::types;
use anyhow::Result;
use serde_json::to_string;
use sqlx::{Pool, Postgres};
use std::fmt::Write;

pub async fn upsert_embedding_table(
    conn: &Pool<Postgres>,
    project: &str,
    job_params: &types::JobParams,
    embeddings: Vec<PairedEmbeddings>,
) -> Result<()> {
    let (query, bindings) = build_upsert_query(project, job_params, embeddings);
    let mut q = sqlx::query(&query);
    for (record_id, embeddings) in bindings {
        q = q.bind(record_id).bind(embeddings);
    }
    match q.execute(conn).await {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("failed to execute query: {}", e)),
    }
}

// returns query and bindings
// only compatible with pg-vector data types
fn build_upsert_query(
    project: &str,
    job_params: &types::JobParams,
    embeddings: Vec<PairedEmbeddings>,
) -> (String, Vec<(String, String)>) {
    let join_key = &job_params.primary_key;
    let schema = match &job_params.table_method {
        types::TableMethod::append => job_params.schema.clone(),
        types::TableMethod::join => "vectorize".to_string(),
    };
    let mut query = format!(
        "
        INSERT INTO {schema}._embeddings_{project} ({join_key}, embeddings) VALUES",
        schema = schema,
        join_key = join_key,
    );
    let mut bindings: Vec<(String, String)> = Vec::new();

    for (index, pair) in embeddings.into_iter().enumerate() {
        if index > 0 {
            query.push(',');
        }
        query.push_str(&format!(
            " (${}::{}, ${}::vector)",
            2 * index + 1,
            job_params.pkey_type,
            2 * index + 2
        ));

        let embedding =
            serde_json::to_string(&pair.embeddings).expect("failed to serialize embedding");
        bindings.push((pair.primary_key, embedding));
    }
    let upsert = format!(
        " ON CONFLICT ({join_key})
        DO UPDATE SET embeddings = EXCLUDED.embeddings, updated_at = NOW();",
        join_key = join_key
    );
    query.push_str(&upsert);
    (query, bindings)
}

pub async fn update_embeddings(
    pool: &Pool<Postgres>,
    schema: &str,
    table: &str,
    project: &str,
    pkey: &str,
    pkey_type: &str,
    embeddings: Vec<PairedEmbeddings>,
) -> anyhow::Result<()> {
    if embeddings.len() > 10 {
        bulk_update_embeddings(pool, schema, table, project, pkey, pkey_type, embeddings).await
    } else {
        update_append_table(pool, embeddings, schema, table, project, pkey, pkey_type).await
    }
}

// creates a temporary table, inserts all new values into the temporary table, and then performs an update by join
async fn bulk_update_embeddings(
    pool: &Pool<Postgres>,
    schema: &str,
    table: &str,
    project: &str,
    pkey: &str,
    pkey_type: &str,
    embeddings: Vec<PairedEmbeddings>,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    let tmp_table = format!("temp_embeddings_{project}");

    let temp_table_query = format!(
        "CREATE TEMP TABLE IF NOT EXISTS {tmp_table} (
            pkey {pkey_type} PRIMARY KEY,
            embeddings vector
        ) ON COMMIT DROP;", // note, dropping on commit
    );

    sqlx::query(&temp_table_query).execute(&mut *tx).await?;

    // insert all new values into the temporary table
    let mut insert_query = format!("INSERT INTO {tmp_table} (pkey, embeddings) VALUES ");
    let mut params: Vec<(String, String)> = Vec::new();

    for embed in &embeddings {
        let embedding_json = to_string(&embed.embeddings).expect("failed to serialize embedding");
        params.push((embed.primary_key.to_string(), embedding_json));
    }

    // Constructing query values part and collecting bind parameters
    for (i, (_pkey, _embedding)) in params.iter().enumerate() {
        if i > 0 {
            insert_query.push_str(", ");
        }
        write!(
            &mut insert_query,
            "(${}::{}, ${}::vector)",
            i * 2 + 1,
            pkey_type,
            i * 2 + 2
        )
        .expect("Failed to write to query string");
    }

    let mut insert_statement = sqlx::query(&insert_query);

    for (pkey, embedding) in params {
        insert_statement = insert_statement.bind(pkey).bind(embedding);
    }
    // insert to the temp table
    insert_statement.execute(&mut *tx).await?;

    let update_query = format!(
        "UPDATE {schema}.{table} SET
            {project}_embeddings = temp.embeddings,
            {project}_updated_at = (NOW())
        FROM {tmp_table} temp
        WHERE {schema}.{table}.{pkey}::{pkey_type} = temp.pkey::{pkey_type};"
    );

    sqlx::query(&update_query).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok(())
}

async fn update_append_table(
    pool: &Pool<Postgres>,
    embeddings: Vec<PairedEmbeddings>,
    schema: &str,
    table: &str,
    project: &str,
    pkey: &str,
    pkey_type: &str,
) -> anyhow::Result<()> {
    for embed in embeddings {
        // Serialize the Vec<f64> to a JSON string
        let embedding = to_string(&embed.embeddings).expect("failed to serialize embedding");

        let update_query = format!(
            "
            UPDATE {schema}.{table}
            SET 
                {project}_embeddings = $1::vector,
                {project}_updated_at = (NOW())
            WHERE {pkey} = $2::{pkey_type}
        "
        );
        // Prepare and execute the update statement for this pair within the transaction
        sqlx::query(&update_query)
            .bind(embedding)
            .bind(embed.primary_key)
            .execute(pool)
            .await?;
    }
    Ok(())
}
