use pgrx::prelude::*;

pub fn cosine_similarity_search(
    project: &str,
    schema: &str,
    table: &str,
    return_columns: &[String],
    num_results: i32,
    embeddings: &[f64],
) -> Result<Vec<(pgrx::JsonB,)>, spi::Error> {
    let query = format!(
        "
    SELECT to_jsonb(t)
    as results FROM (
        SELECT 
        1 - ({project}_embeddings <=> $1::vector) AS similarity_score,
        {cols}
    FROM {schema}.{table}
    WHERE {project}_updated_at is NOT NULL
    ORDER BY similarity_score DESC
    LIMIT {num_results}
    ) t
    ",
        cols = return_columns.join(", "),
    );
    Spi::connect(|client| {
        let mut results: Vec<(pgrx::JsonB,)> = Vec::new();
        let tup_table = client.select(
            &query,
            None,
            Some(vec![(
                PgBuiltInOids::FLOAT8ARRAYOID.oid(),
                embeddings.into_datum(),
            )]),
        )?;
        for row in tup_table {
            match row["results"].value()? {
                Some(r) => results.push((r,)),
                None => error!("failed to get results"),
            }
        }
        Ok(results)
    })
}
