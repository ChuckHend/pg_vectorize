pub mod common {
    use anyhow::Result;
    use log::LevelFilter;
    use serde::Serialize;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::{ConnectOptions, FromRow};
    use sqlx::{Pool, Postgres, Row};
    use url::{ParseError, Url};

    #[derive(FromRow, Debug, serde::Deserialize)]
    pub struct SearchResult {
        pub product_id: i32,
        pub product_name: String,
        pub description: String,
        pub similarity_score: f64,
    }

    #[derive(FromRow, Debug, Serialize)]
    pub struct SearchJSON {
        pub search_results: serde_json::Value,
    }

    pub async fn connect(url: &str) -> Pool<Postgres> {
        let options = conn_options(url).expect("failed to parse url");
        println!("URL: {}", url);
        PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(10))
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("failed to connect to pg")
    }

    pub async fn init_database() -> Pool<Postgres> {
        let username = whoami::username();
        let database_port = database_port();

        let conn = connect(&format!(
            "postgres://{username}:postgres@localhost:{database_port}/postgres"
        ))
        .await;

        let _ = sqlx::query("DROP EXTENSION IF EXISTS vectorize CASCADE")
            .execute(&conn)
            .await
            .expect("failed to create extension");

        // CREATE EXTENSION
        let _ = sqlx::query("CREATE EXTENSION IF NOT EXISTS vectorize CASCADE")
            .execute(&conn)
            .await
            .expect("failed to create extension");

        let _ = sqlx::query("CREATE EXTENSION IF NOT EXISTS vectorscale CASCADE")
            .execute(&conn)
            .await
            .expect("failed to create vectorscale extension");
        conn
    }

    pub fn database_port() -> usize {
        if cfg!(feature = "pg17") {
            28817
        } else if cfg!(feature = "pg16") {
            28816
        } else if cfg!(feature = "pg15") {
            28815
        } else if cfg!(feature = "pg14") {
            28814
        } else {
            5432
        }
    }

    pub fn conn_options(url: &str) -> Result<PgConnectOptions, ParseError> {
        // Parse url
        let parsed = Url::parse(url)?;
        let options = PgConnectOptions::new()
            .host(parsed.host_str().ok_or(ParseError::EmptyHost)?)
            .port(parsed.port().ok_or(ParseError::InvalidPort)?)
            .username(parsed.username())
            .password(parsed.password().ok_or(ParseError::IdnaError)?)
            .database(parsed.path().trim_start_matches('/'))
            .log_statements(LevelFilter::Debug);
        Ok(options)
    }

    pub async fn init_test_table(table: &str, conn: &Pool<Postgres>) {
        let create = format!(
            "CREATE TABLE IF NOT EXISTS {table} (LIKE vectorize.example_products INCLUDING ALL);"
        );
        let insert = format!(
            "
        DO $$
        BEGIN
            IF (SELECT COUNT(*) FROM {table}) = 0 THEN
                INSERT INTO {table} SELECT * FROM vectorize.example_products;
            END IF;
        END $$;
       "
        );
        for q in vec![create, insert] {
            sqlx::query(&q)
                .execute(conn)
                .await
                .expect("failed to create test table");
        }
    }

    pub async fn row_count(fq_table_name: &str, conn: &Pool<Postgres>) -> i64 {
        let q = format!(
            "SELECT COUNT(*) FROM {fq_table_name};",
            fq_table_name = fq_table_name
        );
        sqlx::query(&q)
            .fetch_one(conn)
            .await
            .expect("failed to select from test_table")
            .get::<i64, usize>(0)
    }

    pub async fn init_embedding_svc_url(conn: &Pool<Postgres>) {
        let set_guc = format!(
            "ALTER SYSTEM SET vectorize.embedding_service_url to 'http://0.0.0.0:3000/v1';"
        );
        let reload = "SELECT pg_reload_conf();".to_string();
        for q in vec![set_guc, reload] {
            sqlx::query(&q)
                .execute(conn)
                .await
                .expect("failed to init embedding svc url");
        }
    }

    pub async fn search_with_retry(
        conn: &Pool<Postgres>,
        query: &str,
        job_name: &str,
        retries: usize,
        delay_seconds: usize,
        num_results: i32,
        filter: Option<String>,
    ) -> Result<Vec<SearchJSON>> {
        let mut results: Vec<SearchJSON> = vec![];
        let filter_param = match filter {
            Some(f) => format!(",where_sql => $VECTDELIM${f}$VECTDELIM$"),
            None => "".to_string(),
        };
        let query = format!(
            "SELECT * from vectorize.search(
            job_name => '{job_name}',
            query => '{query}',
            return_columns => ARRAY['product_id', 'product_name', 'description'],
            num_results => {num_results}
            {filter_param}
        ) as search_results;"
        );
        for i in 0..retries {
            results = sqlx::query_as::<_, SearchJSON>(&query)
                .fetch_all(conn)
                .await?;
            let num_returned = results.len();
            if num_returned != num_results as usize {
                println!(
                    "job_name: {}, num_results: {}, retrying search query: {}/{}",
                    job_name,
                    num_returned,
                    i + 1,
                    retries
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds as u64)).await;
            } else {
                return Ok(results);
            }
        }
        let js_results = serde_json::to_value(&results).unwrap();
        println!("results: {:?}", js_results);
        Err(anyhow::anyhow!("timed out waiting for search query"))
    }

    pub async fn hybrid_search_with_retry(
        conn: &Pool<Postgres>,
        query: &str,
        job_name: &str,
        retries: usize,
        delay_seconds: usize,
        num_results: i32,
        filter: Option<String>,
    ) -> Result<Vec<SearchJSON>> {
        let mut results: Vec<SearchJSON> = vec![];
        let filter_param = match filter {
            Some(f) => format!(",where_sql => $VECTDELIM${f}$VECTDELIM$"),
            None => "".to_string(),
        };
        let query = format!(
            "SELECT * from vectorize.hybrid_search(
            job_name => '{job_name}',
            query => '{query}',
            return_columns => ARRAY['product_id', 'product_name', 'description'],
            num_results => {num_results}
            {filter_param}
        ) as hybrid_search_results;"
        );
        for i in 0..retries {
            results = sqlx::query_as::<_, SearchJSON>(&query)
                .fetch_all(conn)
                .await?;
            let num_returned = results.len();
            if num_returned != num_results as usize {
                println!(
                    "job_name: {}, num_results: {}, retrying hybrid search query: {}/{}",
                    job_name,
                    num_returned,
                    i + 1,
                    retries
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds as u64)).await;
            } else {
                return Ok(results);
            }
        }
        let js_results = serde_json::to_value(&results).unwrap();
        println!("results: {:?}", js_results);
        Err(anyhow::anyhow!("timed out waiting for hybrid search query"))
    }
}
