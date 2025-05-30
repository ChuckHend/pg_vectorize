mod util;

use actix_web::{http::StatusCode, http::header, test};
use env_logger;
use serde_json::json;

use util::common;
use vectorize_server::routes::table::JobResponse;

// these tests require:
// 1. Postgres to be running
// 2. job-worker binary to be running
// easiest way is to run these with docker-compose.yml
#[ignore]
#[actix_web::test]
async fn test_table() {
    env_logger::init();
    let app = common::get_test_app().await;

    // Create test table with required columns
    let cfg = vectorize_server::config::Config::default();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");

    let table = common::create_test_table().await;

    let job_name = format!("test_job_{}", table);

    // Create a valid VectorizeJob payload
    let payload = json!({
        "job_name": job_name,
        "src_table": table,
        "src_schema": "vectorize_test",
        "src_column": "content",
        "primary_key": "id",
        "update_time_col": "updated_at",
        "model": "openai/text-embedding-3-small"
    });

    let req = test::TestRequest::post()
        .uri("/api/v1/table")
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .set_json(&payload)
        .to_request();

    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK, "{:?}", resp);

    // deserialize the response body
    let body = test::read_body(resp).await;
    let response: JobResponse = serde_json::from_slice(&body).unwrap();
    assert!(!response.id.is_nil(), "Job ID should not be nil");
}
