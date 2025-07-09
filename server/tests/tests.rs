mod util;

use pgvector::Vector;
use serde_json::json;
use sqlx::Row;

use rand::prelude::*;
use util::common;
use vectorize_core::init::exec_psql;
use vectorize_server::routes::table::JobResponse;
// these tests require the following main server, vector-serve, and Postgres to be running
// easiest way is to use the docker-compose file in the root of the project
#[ignore]
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
}

#[ignore]
#[tokio::test]
async fn test_search_filters() {
    let mut rng = rand::rng();
    let test_num = rng.random_range(1..100000);
    let cfg = vectorize_core::config::Config::from_env();
    let sql = std::fs::read_to_string("sql/example.sql").unwrap();
    let _sql_exec = exec_psql(&cfg.database_url, &sql);

    let pool = sqlx::PgPool::connect(&cfg.database_url).await.unwrap();
    // test table
    let table = format!("test_filter_{test_num}");
    let create_sql =
        format!("CREATE TABLE public.{table} (LIKE public.my_products INCLUDING ALL);");
    let insert_sql = format!("INSERT INTO public.{table} SELECT * FROM public.my_products;");

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

    // filter a query by product_category
    let params = format!("job_name={job_name}&query=pen&product_category=electronics",);
    let search_results = common::search_with_retry(&params, 9).await.unwrap();

    assert_eq!(search_results.len(), 9);
    assert!(search_results[0]["fts_rank"].is_null());
    for result in search_results {
        assert_eq!(result["product_category"].as_str().unwrap(), "electronics");
    }

    // filter by price
    let params = format!("job_name={job_name}&query=electronics&price=25");
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
}

#[ignore]
#[tokio::test]
async fn test_lifecycle() {
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
    // let row = sqlx::query("SELECT vectorize.embed('food'::text, $1);")
    //     .bind(&job_name)
    //     .fetch_one(&pool)
    //     .await
    //     .unwrap();
    // let result_str: String = row.get(0);
    // let result_str = result_str.trim_start_matches('[').trim_end_matches(']');
    // let values: Vec<f64> = result_str
    //     .split(',')
    //     .map(|s| s.trim().parse::<f64>().unwrap())
    //     .collect();
    // assert_eq!(values.len(), 384); // sentence-transformers/all-MiniLM-L6-v2 has 384 dimensions
}

#[ignore]
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
