mod util;

use pgvector::Vector;
use serde_json::json;
use sqlx::Row;

use rand::prelude::*;
use util::common;
use vectorize_server::routes::table::JobResponse;
// these tests require the following main server, vector-serve, and Postgres to be running
// easiest way is to use the docker-compose file in the root of the project
#[tokio::test]
async fn test_search_server() {
    common::init_test_environment().await;

    let table = common::create_test_table().await;

    let job_name = format!("test_job_{table}");

    // Create a valid VectorizeJob payload
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "vectorize_test",
        "src_columns": ["content"],
        "primary_key": "id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/v1/table")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Response status: {:?}",
        resp.status()
    );

    let response: JobResponse = resp.json().await.expect("Failed to parse response");
    assert!(!response.id.is_nil(), "Job ID should not be nil");

    // sleep for 2 seconds
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // test searching the job
    let params = format!("job_name={job_name}&query=food");
    let search_results = common::search_with_retry(&params, 3).await.unwrap();
    // 3 results (number rows in table)
    assert_eq!(search_results.len(), 3);
    // First result should be pizza (highest similarity)
    assert_eq!(search_results[0]["content"].as_str().unwrap(), "pizza");
    assert!(
        search_results[0]["similarity_score"].as_f64().unwrap() > 0.5,
        "{} should be greater than 0.6",
        search_results[0]["similarity_score"]
    );

    // test limit parameter
    let params = format!("job_name={job_name}&query=writing%20utensil&limit=1");
    let search_results = common::search_with_retry(&params, 1).await.unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0]["content"].as_str().unwrap(), "pencil");

    let cfg = vectorize_core::config::Config::from_env();
    let pool = sqlx::PgPool::connect(&cfg.database_url).await.unwrap();

    // test insert
    common::insert_row(&pool, &table, "apples and apple trees").await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let params = format!("job_name={job_name}&query=apples&limit=1");
    let search_results = common::search_with_retry(&params, 1).await.unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(
        search_results[0]["content"].as_str().unwrap(),
        "apples and apple trees"
    );

    // test update
    common::update_row(
        &pool,
        &table,
        1,
        "a space shuttle is a device for storing and transporting astronauts",
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let params = format!("job_name={job_name}&query=astronauts&limit=1");
    let search_results = common::search_with_retry(&params, 1).await.unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(
        search_results[0]["content"].as_str().unwrap(),
        "a space shuttle is a device for storing and transporting astronauts"
    );
}

#[tokio::test]
async fn test_search_filters() {
    let mut rng = rand::rng();
    let test_num = rng.random_range(1..100000);
    let cfg = vectorize_core::config::Config::from_env();
    let sql = std::fs::read_to_string("sql/example.sql").unwrap();
    if let Err(e) = common::exec_psql(&cfg.database_url, &sql) {
        // installation of example.sql could fail due to race conditions
        // so we can continue
        log::warn!("failed to execute example.sql: {}", e);
    }

    let pool = sqlx::PgPool::connect(&cfg.database_url).await.unwrap();
    // test table
    let table = format!("test_filter_{test_num}");
    let drop_sql = format!("DROP TABLE IF EXISTS public.{table};");
    let create_sql =
        format!("CREATE TABLE public.{table} (LIKE public.my_products INCLUDING ALL);");
    let insert_sql = format!("INSERT INTO public.{table} SELECT * FROM public.my_products;");

    sqlx::query(&drop_sql).execute(&pool).await.unwrap();
    sqlx::query(&create_sql).execute(&pool).await.unwrap();
    sqlx::query(&insert_sql).execute(&pool).await.unwrap();

    // initialize search job
    let job_name = format!("test_filter_{test_num}");
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "public",
        "src_columns": ["description"],
        "primary_key": "product_id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/v1/table")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Response status: {:?}",
        resp.status()
    );

    // filter a query by product_category (using eq operator)
    let params = format!("job_name={job_name}&query=pen&product_category=eq.electronics",);
    let search_results = common::search_with_retry(&params, 9).await.unwrap();

    assert_eq!(search_results.len(), 9);
    assert!(search_results[0]["fts_rank"].is_null());
    for result in search_results {
        assert_eq!(result["product_category"].as_str().unwrap(), "electronics");
    }

    let params = format!("job_name={job_name}&query=electronics&price=eq.25");
    let search_results = common::search_with_retry(&params, 2).await.unwrap();
    assert_eq!(search_results.len(), 2);
    assert_eq!(
        search_results[0]["product_name"].as_str().unwrap(),
        "Wireless Mouse"
    );
    assert_eq!(
        search_results[1]["product_name"].as_str().unwrap(),
        "Alarm Clock"
    );

    // test greater than or equal operator
    let params = format!("job_name={job_name}&query=electronics&price=gte.25&limit=5");
    let search_results = common::search_with_retry(&params, 5).await.unwrap();
    assert_eq!(search_results.len(), 5);

    // test backward compatibility - no operator should default to equality
    let params = format!("job_name={job_name}&query=pen&product_category=electronics",);
    let search_results = common::search_with_retry(&params, 9).await.unwrap();
    assert_eq!(search_results.len(), 9);
    for result in search_results {
        assert_eq!(result["product_category"].as_str().unwrap(), "electronics");
    }

    // test multiple filters - category first, then price
    let params = format!(
        "job_name={job_name}&query=electronics&product_category=eq.electronics&price=gte.25"
    );
    let search_results_category_first = common::search_with_retry(&params, 5).await.unwrap();
    assert_eq!(search_results_category_first.len(), 5);
    for result in &search_results_category_first {
        assert_eq!(result["product_category"].as_str().unwrap(), "electronics");
        assert!(result["price"].as_f64().unwrap() >= 25.0);
    }

    // test multiple filters - price first, then category (different order)
    let params = format!(
        "job_name={job_name}&query=electronics&price=gte.25&product_category=eq.electronics"
    );
    let search_results_price_first = common::search_with_retry(&params, 5).await.unwrap();
    assert_eq!(search_results_price_first.len(), 5);
    for result in &search_results_price_first {
        assert_eq!(result["product_category"].as_str().unwrap(), "electronics");
        assert!(result["price"].as_f64().unwrap() >= 25.0);
    }

    // verify that both filter orders produce the same results
    assert_eq!(
        search_results_category_first.len(),
        search_results_price_first.len()
    );
    // Sort both results by product_id to ensure consistent comparison
    let mut category_first_sorted = search_results_category_first.clone();
    let mut price_first_sorted = search_results_price_first.clone();
    category_first_sorted.sort_by(|a, b| {
        a["product_id"]
            .as_i64()
            .unwrap()
            .cmp(&b["product_id"].as_i64().unwrap())
    });
    price_first_sorted.sort_by(|a, b| {
        a["product_id"]
            .as_i64()
            .unwrap()
            .cmp(&b["product_id"].as_i64().unwrap())
    });

    for (i, (result1, result2)) in category_first_sorted
        .iter()
        .zip(price_first_sorted.iter())
        .enumerate()
    {
        assert_eq!(
            result1["product_id"], result2["product_id"],
            "Product IDs should match at index {}",
            i
        );
        assert_eq!(
            result1["product_name"], result2["product_name"],
            "Product names should match at index {}",
            i
        );
    }
}

#[tokio::test]
async fn test_search_filter_operators() {
    let mut rng = rand::rng();
    let test_num = rng.random_range(1..100000);
    let cfg = vectorize_core::config::Config::from_env();
    // install raw SQL
    let sql = std::fs::read_to_string("sql/example.sql").unwrap();
    if let Err(e) = common::exec_psql(&cfg.database_url, &sql) {
        // installation of example.sql could fail due to race conditions
        // so we can continue
        log::warn!("failed to execute example.sql: {}", e);
    }

    let pool = sqlx::PgPool::connect(&cfg.database_url).await.unwrap();
    // test table
    let table = format!("test_filter_ops_{test_num}");
    let drop_sql = format!("DROP TABLE IF EXISTS public.{table};");
    let create_sql =
        format!("CREATE TABLE public.{table} (LIKE public.my_products INCLUDING ALL);");
    let insert_sql = format!("INSERT INTO public.{table} SELECT * FROM public.my_products;");

    sqlx::query(&drop_sql).execute(&pool).await.unwrap();
    sqlx::query(&create_sql).execute(&pool).await.unwrap();
    sqlx::query(&insert_sql).execute(&pool).await.unwrap();

    // initialize search job
    let job_name = format!("test_filter_ops_{test_num}");
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "public",
        "src_columns": ["description"],
        "primary_key": "product_id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/v1/table")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Response status: {:?}",
        resp.status()
    );

    // Test different operators
    // Greater than
    let params = format!("job_name={job_name}&query=electronics&price=gt.20&limit=100");
    let search_results = common::search_with_retry(&params, 14).await.unwrap();
    assert_eq!(search_results.len(), 14);

    // Less than or equal
    let params = format!("job_name={job_name}&query=electronics&price=lte.25&limit=100");
    let search_results = common::search_with_retry(&params, 30).await.unwrap();
    assert_eq!(search_results.len(), 30);

    // Test float values
    let params = format!("job_name={job_name}&query=electronics&price=gte.24.5&limit=1000");
    let search_results = common::search_with_retry(&params, 12).await.unwrap();
    assert_eq!(search_results.len(), 12);

    // Test invalid operator (should return error)
    let params = format!("job_name={job_name}&query=electronics&price=invalid.25");
    let response = client
        .get(&format!("http://localhost:8080/api/v1/search?{}", params))
        .send()
        .await
        .expect("Failed to send request");

    // Should return an error for invalid operator
    assert!(response.status().is_client_error() || response.status().is_server_error());

    // Test non-numeric value with comparison operator (should return error)
    let params = format!("job_name={job_name}&query=electronics&price=gt.abc");
    let response = client
        .get(&format!("http://localhost:8080/api/v1/search?{}", params))
        .send()
        .await
        .expect("Failed to send request");

    // Should return an error for non-numeric value with comparison operator
    assert!(response.status().is_client_error() || response.status().is_server_error());
}

/// proxy is an incomplete feature
#[ignore]
#[tokio::test]
async fn test_proxy() {
    // Initialize the project (database setup, etc.) without creating test app
    common::init_test_environment().await;

    // Create test table with required columns
    let cfg = vectorize_core::config::Config::from_env();

    // parse the host and port from the cfg.database_url using url crate
    use url::Url;
    let mut url = Url::parse(&cfg.database_url).unwrap();
    url.set_port(Some(5433)).unwrap();

    let database_url = url.to_string();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .expect("unable to connect to postgres");

    let table = common::create_test_table().await;

    let job_name = format!("test_job_{table}");

    // Create a valid VectorizeJob payload
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "vectorize_test",
        "src_columns": ["content"],
        "primary_key": "id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    });

    // Use reqwest to make HTTP request to running server
    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/v1/table")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Response status: {:?}",
        resp.status()
    );

    let response: JobResponse = resp.json().await.expect("Failed to parse response");
    assert!(!response.id.is_nil(), "Job ID should not be nil");

    // request a job that does not exist should be a 404
    let resp = client
        .get("http://localhost:8080/api/v1/search?job_name=does_not_exist")
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // sleep for 2 seconds
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // test searching the job
    let params = format!("job_name={job_name}&query=food");
    let search_results = common::search_with_retry(&params, 3).await.unwrap();

    // Should return 3 results
    assert_eq!(search_results.len(), 3);

    // First result should be pizza (highest similarity)
    assert_eq!(search_results[0]["content"].as_str().unwrap(), "pizza");
    assert!(search_results[0]["similarity_score"].as_f64().unwrap() > 0.5);
    let q = format!("SELECT (vectorize.embed('food', '{job_name}'));");

    let row = sqlx::query(&q).fetch_one(&pool).await.unwrap();
    let v: Vector = row.get(0);
    assert_eq!(v.to_vec().len(), 384); // sentence-transformers/all-MiniLM-L6-v2 has 384 dimensions

    // execute search SQL
    let q = format!(
        "
    SELECT *
    FROM (
        SELECT t0.*, t1.similarity_score
        FROM (
            SELECT
                id,
                1 - (embeddings <=> vectorize.embed('food', '{job_name}')) as similarity_score
            FROM vectorize._embeddings_{job_name}
            ) t1
        INNER JOIN vectorize_test.{table} t0 on t0.id = t1.id
    ) t
    ORDER BY t.similarity_score DESC;"
    );
    let row = sqlx::query(&q).fetch_all(&pool).await.unwrap();
    assert_eq!(row.len(), 3);
    // assert first row is pizza
    assert_eq!(row[0].get::<String, usize>(1), "pizza");

    // test prepared statements
    // Use parameter binding instead of string formatting
    let row = sqlx::query("SELECT vectorize.embed('food'::text, $1);")
        .bind(&job_name)
        .fetch_one(&pool)
        .await
        .unwrap();
    let result_str: String = row.get(0);
    let result_str = result_str.trim_start_matches('[').trim_end_matches(']');
    let values: Vec<f64> = result_str
        .split(',')
        .map(|s| s.trim().parse::<f64>().unwrap())
        .collect();
    assert_eq!(values.len(), 384); // sentence-transformers/all-MiniLM-L6-v2 has 384 dimensions
}

#[tokio::test]
async fn test_lifecycle() {
    // Initialize the project (database setup, etc.) without creating test app
    common::init_test_environment().await;

    // Create test table with required columns
    let table = common::create_test_table().await;

    let job_name = format!("test_job_{table}");

    // Create a valid VectorizeJob payload
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "vectorize_test",
        "src_columns": ["content"],
        "primary_key": "id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    });

    // Use reqwest to make HTTP request to running server
    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:8080/api/v1/table")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Response status: {:?}",
        resp.status()
    );

    let response: JobResponse = resp.json().await.expect("Failed to parse response");
    assert!(!response.id.is_nil(), "Job ID should not be nil");

    // request a job that does not exist should be a 404
    let resp = client
        .get("http://localhost:8080/api/v1/search?job_name=does_not_exist")
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // sleep for 2 seconds
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // test searching the job
    let params = format!("job_name={job_name}&query=food");
    let search_results = common::search_with_retry(&params, 3).await.unwrap();

    // Should return 3 results
    assert_eq!(search_results.len(), 3);

    // First result should be pizza (highest similarity)
    assert_eq!(search_results[0]["content"].as_str().unwrap(), "pizza");
    assert!(search_results[0]["similarity_score"].as_f64().unwrap() > 0.5);
}

#[tokio::test]
async fn test_health_monitoring() {
    // Initialize the test environment without creating test app
    common::init_test_environment().await;

    // Use reqwest to make HTTP requests to running server
    let client = reqwest::Client::new();

    // Test liveness endpoint
    let resp = client
        .get("http://localhost:8080/health/live")
        .send()
        .await
        .expect("Failed to send liveness request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "Liveness check should always return OK"
    );

    let liveness_response: serde_json::Value = resp
        .json()
        .await
        .expect("Failed to parse liveness response");

    assert_eq!(liveness_response["status"], "alive");
    assert!(liveness_response["timestamp"].is_number());

    // Test main health endpoint
    let resp = client
        .get("http://localhost:8080/health")
        .send()
        .await
        .expect("Failed to send health request");

    // Health endpoint might return 200 (healthy) or 503 (unhealthy) depending on worker state
    assert!(
        resp.status() == reqwest::StatusCode::OK
            || resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "Health check should return 200 or 503, got: {:?}",
        resp.status()
    );

    let health_response: serde_json::Value =
        resp.json().await.expect("Failed to parse health response");

    // Verify response structure
    assert!(health_response["status"].is_string());
    assert!(health_response["worker"].is_object());
    assert!(health_response["worker"]["status"].is_string());
    assert!(health_response["worker"]["last_heartbeat"].is_number());
    assert!(health_response["worker"]["jobs_processed"].is_number());
    assert!(health_response["worker"]["uptime_seconds"].is_number());
    assert!(health_response["worker"]["restart_count"].is_number());
    assert!(health_response["timestamp"].is_number());

    println!(
        "Health response: {}",
        serde_json::to_string_pretty(&health_response).unwrap()
    );

    // Test readiness endpoint
    let resp = client
        .get("http://localhost:8080/health/ready")
        .send()
        .await
        .expect("Failed to send readiness request");

    // Readiness endpoint might return 200 (ready) or 503 (not ready) depending on worker state
    assert!(
        resp.status() == reqwest::StatusCode::OK
            || resp.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "Readiness check should return 200 or 503, got: {:?}",
        resp.status()
    );

    let readiness_response: serde_json::Value = resp
        .json()
        .await
        .expect("Failed to parse readiness response");

    assert!(readiness_response["status"].is_string());
    assert!(readiness_response["worker_status"].is_string());
    assert!(readiness_response["timestamp"].is_number());

    // Wait a moment and check that worker metrics change over time
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let resp2 = client
        .get("http://localhost:8080/health")
        .send()
        .await
        .expect("Failed to send second health request");

    let health_response2: serde_json::Value = resp2
        .json()
        .await
        .expect("Failed to parse second health response");

    // The uptime should have increased
    let uptime1 = health_response["worker"]["uptime_seconds"]
        .as_u64()
        .unwrap();
    let uptime2 = health_response2["worker"]["uptime_seconds"]
        .as_u64()
        .unwrap();
    assert!(uptime2 >= uptime1, "Uptime should increase over time");

    // The heartbeat timestamp should have updated
    let heartbeat1 = health_response["worker"]["last_heartbeat"]
        .as_u64()
        .unwrap();
    let heartbeat2 = health_response2["worker"]["last_heartbeat"]
        .as_u64()
        .unwrap();
    assert!(
        heartbeat2 >= heartbeat1,
        "Heartbeat should update over time"
    );

    println!("Health monitoring test completed successfully");
}
