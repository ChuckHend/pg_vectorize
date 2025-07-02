pub mod common {
    use actix_http::Request;
    use actix_service::Service;
    use actix_web::test;
    use actix_web::{App, Error, dev::ServiceResponse, web};
    use rand::prelude::*;
    use vectorize_core::init;

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn get_test_app() -> impl Service<Request, Response = ServiceResponse, Error = Error>
    {
        let cfg = vectorize_core::config::Config::from_env();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&cfg.database_url)
            .await
            .expect("unable to connect to postgres");
        init::init_project(&pool, Some(&cfg.database_url))
            .await
            .expect("Failed to initialize project");
        test::init_service(
            App::new()
                .app_data(web::Data::new(pool))
                .configure(vectorize_server::server::route_config),
        )
        .await
    }

    // Initialize test environment without creating Actix test service
    // For use with reqwest-based tests that hit a running server
    #[cfg(test)]
    pub async fn init_test_environment() {
        let cfg = vectorize_core::config::Config::from_env();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&cfg.database_url)
            .await
            .expect("unable to connect to postgres");
        init::init_project(&pool, Some(&cfg.database_url))
            .await
            .expect("Failed to initialize project");
    }

    // Helper function to perform search requests with retry logic
    #[cfg(test)]
    pub async fn search_with_retry(
        params: &str,
        num_expected: usize,
    ) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let search_url = format!("http://0.0.0.0:8080/api/v1/search?{params}");
        println!("Search URL: {search_url}");

        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(10);
        let retry_interval = std::time::Duration::from_secs(1);

        loop {
            let resp = client.get(&search_url).send().await?;

            if resp.status().is_success() {
                let search_results: Vec<serde_json::Value> = resp.json().await?;
                // Check if we have the expected number of results
                if search_results.len() == num_expected {
                    return Ok(search_results);
                } else {
                    // Log a warning if the number of results is not as expected
                    println!(
                        "Expected {} results, but got {}. Retrying...",
                        num_expected,
                        search_results.len()
                    );
                }
            }

            // Check if we've exceeded the timeout
            if start_time.elapsed() >= timeout_duration {
                // Return empty results if timeout is reached
                panic!("Search request timed out after 10 seconds");
            }

            // Wait before retrying
            tokio::time::sleep(retry_interval).await;
        }
    }

    // creates a table in the vectorize_test schema
    pub async fn create_test_table() -> String {
        let cfg = vectorize_core::config::Config::from_env();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&cfg.database_url)
            .await
            .expect("unable to connect to postgres");

        sqlx::query("create schema if not exists vectorize_test;")
            .execute(&pool)
            .await
            .expect("unable to create vectorize_test schema");

        let mut rng = rand::rng();
        let test_num = rng.random_range(1..1000);

        let table = format!("test_table_{test_num}");
        sqlx::query(
            format!(
                "create table if not exists vectorize_test.{table} (id serial primary key, content text, updated_at timestamptz);"
            )
            .as_str(),
        )
        .execute(&pool)
        .await
        .expect("unable to create test table");

        for record in ["pizza", "pencil", "airplane"] {
            sqlx::query(
                format!(
                    "insert into vectorize_test.{table} (content, updated_at) values ('{record}', now());"
                )
                .as_str(),
            )
            .execute(&pool)
            .await
            .expect("unable to insert test data");
        }

        table
    }
}
