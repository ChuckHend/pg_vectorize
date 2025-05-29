use crate::errors::ServerError;
use sqlx::PgPool;

pub async fn get_column_datatype(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<String, ServerError> {
    sqlx::query_scalar!(
        "
        SELECT data_type
        FROM information_schema.columns
        WHERE
            table_schema = $1
            AND table_name = $2
            AND column_name = $3    
        ",
        schema,
        table,
        column
    )
    .fetch_one(pool)
    .await?
    .ok_or_else(|| {
        ServerError::NotFoundError(format!(
            "Column {} not found in table {}.{}",
            column, schema, table
        ))
    })
}

async fn pgmq_schema_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let row: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name = 'pgmq')",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn init_pgmq(pool: &PgPool) -> Result<(), ServerError> {
    // Check if the pgmq schema already exists
    if pgmq_schema_exists(pool).await? {
        log::info!("pgmq schema already exists, skipping initialization.");
        return Ok(());
    }

    // URL to the raw SQL file
    let sql_url = "https://raw.githubusercontent.com/pgmq/pgmq/main/pgmq-extension/sql/pgmq.sql";

    let client = reqwest::Client::new();
    let response = client.get(sql_url).send().await?;
    let sql_content = response.text().await?;

    // Split the SQL into individual statements
    // This handles basic semicolon separation - you might need more sophisticated parsing
    // for complex SQL files with semicolons in strings, etc.
    let statements = split_sql_statements(&sql_content);

    println!("Found {} SQL statements", statements.len());

    // Execute each statement
    for (i, statement) in statements.iter().enumerate() {
        let trimmed = statement.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue; // Skip empty lines and comments
        }

        println!("Executing statement {} of {}", i + 1, statements.len());

        match sqlx::query(trimmed).execute(pool).await {
            Ok(_) => {
                println!("✓ Statement {} executed successfully", i + 1);
            }
            Err(e) => {
                eprintln!("✗ Error executing statement {}: {}", i + 1, e);
                eprintln!("Statement: {}", trimmed);
                // Depending on your needs, you might want to continue or return the error
                // return Err(e.into());
            }
        }
    }

    Ok(())
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    // Simple SQL statement splitter - splits on semicolons
    // This is basic and might need enhancement for complex SQL
    sql.split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with("--"))
        .collect()
}

// Alternative approach: Execute as a single batch (if supported by your SQL)
async fn execute_sql_batch(pool: &PgPool, sql_content: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sql_content).execute(pool).await?;
    Ok(())
}
