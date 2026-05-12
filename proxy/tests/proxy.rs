//! Integration tests for the vectorize-proxy.
//!
//! These tests assume all services are already running on their default ports:
//!   - Postgres:             localhost:5432
//!   - vector-serve:         localhost:3000
//!   - vectorize-server:     localhost:8080
//!   - vectorize-proxy:      localhost:5433
//!
//! Load the example dataset and create the job before running:
//!
//!   psql postgres://postgres:postgres@localhost:5432/postgres \
//!       -f server/sql/example.sql
//!
//!   curl -s -X POST http://localhost:8080/api/v1/table \
//!     -H "Content-Type: application/json" \
//!     -d '{"job_name":"my_job","src_table":"my_products","src_schema":"public",
//!           "src_columns":["product_name","description"],"primary_key":"product_id",
//!           "update_time_col":"updated_at","model":"sentence-transformers/all-MiniLM-L6-v2"}'
//!
//! Run with: cargo test --test proxy

use sqlx::{Column, Row};

const PROXY_URL: &str = "postgresql://postgres:postgres@localhost:5433/postgres";

async fn connect() -> sqlx::PgPool {
    sqlx::PgPool::connect(PROXY_URL)
        .await
        .expect("Failed to connect to vectorize-proxy at localhost:5433 — is the proxy running?")
}

/// Non-vectorize queries should pass through unchanged.
#[tokio::test]
async fn test_passthrough() {
    let pool = connect().await;

    let row = sqlx::query("SELECT 1 + 1 AS result")
        .fetch_one(&pool)
        .await
        .expect("simple passthrough query failed");

    let result: i32 = row.get("result");
    assert_eq!(result, 2);
}

/// `SELECT *` returns real table columns, not JSON.
#[tokio::test]
async fn test_search_returns_table_rows() {
    let pool = connect().await;

    let rows = sqlx::query(
        "SELECT * FROM vectorize.search(job=>'my_job', query=>'camping backpack', num_results=>3)",
    )
    .fetch_all(&pool)
    .await
    .expect("SELECT * search query failed");

    assert!(!rows.is_empty(), "expected search results, got none");
    assert!(
        rows.len() <= 3,
        "expected at most 3 rows, got {}",
        rows.len()
    );

    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(
        col_names.contains(&"product_id"),
        "expected product_id column in results, got: {col_names:?}"
    );
    assert!(
        col_names.contains(&"product_name"),
        "expected product_name column in results, got: {col_names:?}"
    );
    assert!(
        col_names.contains(&"rrf_score"),
        "expected rrf_score column in results, got: {col_names:?}"
    );
}

/// `SELECT product_name FROM vectorize.search(...)` returns only the requested column.
#[tokio::test]
async fn test_search_column_projection() {
    let pool = connect().await;

    let rows = sqlx::query(
        "SELECT product_name FROM vectorize.search(job=>'my_job', query=>'camping backpack', num_results=>3)",
    )
    .fetch_all(&pool)
    .await
    .expect("column-projected search query failed");

    assert!(!rows.is_empty(), "expected at least one result");

    let col_names: Vec<&str> = rows[0].columns().iter().map(|c| c.name()).collect();
    assert!(
        col_names.contains(&"product_name"),
        "expected product_name column"
    );
    assert!(
        !col_names.contains(&"product_id"),
        "product_id should not appear when only product_name is selected, got: {col_names:?}"
    );
    assert!(
        !col_names.contains(&"rrf_score"),
        "rrf_score should not appear when only product_name is selected"
    );
}

/// `num_results` limits the number of returned rows.
#[tokio::test]
async fn test_search_num_results_limit() {
    let pool = connect().await;

    let rows_1 = sqlx::query(
        "SELECT * FROM vectorize.search(job=>'my_job', query=>'backpack', num_results=>1)",
    )
    .fetch_all(&pool)
    .await
    .expect("search with num_results=>1 failed");

    assert_eq!(rows_1.len(), 1, "expected exactly 1 result");

    let rows_5 = sqlx::query(
        "SELECT * FROM vectorize.search(job=>'my_job', query=>'backpack', num_results=>5)",
    )
    .fetch_all(&pool)
    .await
    .expect("search with num_results=>5 failed");

    assert!(
        rows_5.len() <= 5,
        "expected at most 5 results, got {}",
        rows_5.len()
    );
    assert!(
        rows_5.len() > 1,
        "expected more than 1 result with num_results=>5"
    );
}

/// The `limit` alias for `num_results` works the same way.
#[tokio::test]
async fn test_search_limit_alias() {
    let pool = connect().await;

    let rows =
        sqlx::query("SELECT * FROM vectorize.search(job=>'my_job', query=>'backpack', limit=>2)")
            .fetch_all(&pool)
            .await
            .expect("search with limit=>2 failed");

    assert!(
        rows.len() <= 2,
        "expected at most 2 results, got {}",
        rows.len()
    );
}

/// Semantic relevance: "writing utensil" should rank "Pencil" first.
#[tokio::test]
async fn test_search_relevance_ordering() {
    let pool = connect().await;

    let rows = sqlx::query(
        "SELECT product_name FROM vectorize.search(job=>'my_job', query=>'writing utensil', num_results=>5)",
    )
    .fetch_all(&pool)
    .await
    .expect("relevance ordering search failed");

    assert!(!rows.is_empty(), "expected at least one result");

    let top_name: String = rows[0].get("product_name");
    assert_eq!(
        top_name.to_lowercase(),
        "pencil",
        "expected 'Pencil' as the top result for 'writing utensil', got '{top_name}'"
    );
}

/// Named arguments may appear in any order.
#[tokio::test]
async fn test_search_argument_order_independence() {
    let pool = connect().await;

    let rows_a = sqlx::query(
        "SELECT product_name FROM vectorize.search(job=>'my_job', query=>'backpack', num_results=>3)",
    )
    .fetch_all(&pool)
    .await
    .expect("query-first ordering failed");

    let rows_b = sqlx::query(
        "SELECT product_name FROM vectorize.search(query=>'backpack', job=>'my_job', num_results=>3)",
    )
    .fetch_all(&pool)
    .await
    .expect("job-first ordering failed");

    assert_eq!(
        rows_a.len(),
        rows_b.len(),
        "result count should be the same regardless of argument order"
    );

    let names_a: Vec<String> = rows_a.iter().map(|r| r.get("product_name")).collect();
    let names_b: Vec<String> = rows_b.iter().map(|r| r.get("product_name")).collect();
    assert_eq!(
        names_a, names_b,
        "results should be identical regardless of named-argument order"
    );
}

/// An outer WHERE clause filters the search subquery results.
#[tokio::test]
async fn test_search_outer_where_clause() {
    let pool = connect().await;

    // Search for "speaker" but filter to only electronics
    let rows = sqlx::query(
        "SELECT product_name, product_category \
         FROM vectorize.search(job=>'my_job', query=>'audio speaker', num_results=>5) \
         WHERE product_category = 'electronics'",
    )
    .fetch_all(&pool)
    .await
    .expect("search with outer WHERE failed");

    for row in &rows {
        let category: String = row.get("product_category");
        assert_eq!(
            category, "electronics",
            "expected only electronics, got '{category}'"
        );
    }
}

/// An outer ORDER BY can override the default relevance ordering.
#[tokio::test]
async fn test_search_outer_order_by_price() {
    let pool = connect().await;

    let rows = sqlx::query(
        "SELECT product_name, price::float8 AS price \
         FROM vectorize.search(job=>'my_job', query=>'electronics gadget', num_results=>5) \
         ORDER BY price ASC",
    )
    .fetch_all(&pool)
    .await
    .expect("search with outer ORDER BY failed");

    assert!(rows.len() >= 2, "need at least 2 rows to verify ordering");

    let prices: Vec<f64> = rows.iter().map(|r| r.get::<f64, _>("price")).collect();

    for window in prices.windows(2) {
        assert!(
            window[0] <= window[1],
            "prices should be ascending: {} > {}",
            window[0],
            window[1]
        );
    }
}
