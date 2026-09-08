mod util;
use rand::Rng;
use util::common;

#[tokio::test]
async fn test_scheduled_job() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    common::init_embedding_svc_url(&conn).await;

    // initialize a job
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => '* * * * *'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // should be exactly 1 job in the queue
    let rowcount = common::row_count(&format!("pgmq.q_vectorize_jobs"), &conn).await;
    assert!(rowcount >= 1);

    // embedding should be updated after few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let result = sqlx::query(&format!(
        "SELECT vectorize.search(
        job_name => '{job_name}',
        query => 'mobile devices',
        return_columns => ARRAY['product_name'],
        num_results => 3
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to select from test_table");
    // 3 rows returned
    assert_eq!(result.rows_affected(), 3);
}

#[tokio::test]
async fn test_hybrid_search() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => '* * * * *'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // embedding should be updated after few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let hybrid_search_results =
        common::hybrid_search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(hybrid_search_results.len(), 3);
}

#[tokio::test]
async fn test_chunk_text() {
    let conn = common::init_database().await;

    let query = r#"
        SELECT vectorize.chunk_text('This is a test for chunking.', 20)::TEXT[];
    "#;
    let result: Vec<String> = sqlx::query_scalar(query)
        .fetch_one(&conn)
        .await
        .expect("failed to execute query");
    assert_eq!(
        result,
        vec!["This is a test for".to_string(), "chunking.".to_string(),]
    );

    let query = r#"
        SELECT vectorize.chunk_text('', 20)::TEXT[];
    "#;
    let result: Vec<String> = sqlx::query_scalar(query)
        .fetch_one(&conn)
        .await
        .expect("failed to execute query");
    assert_eq!(result, Vec::<String>::new());

    let query = r#"
        SELECT vectorize.chunk_text('Short', 20)::TEXT[];
    "#;
    let result: Vec<String> = sqlx::query_scalar(query)
        .fetch_one(&conn)
        .await
        .expect("failed to execute query");
    assert_eq!(result, vec!["Short".to_string()]);

    let query = r#"
        SELECT vectorize.chunk_text(
            'Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.',
            50
        )::TEXT[];
    "#;
    let result: Vec<String> = sqlx::query_scalar(query)
        .fetch_one(&conn)
        .await
        .expect("failed to execute query");
    assert_eq!(
        result,
        vec![
            "Lorem ipsum dolor sit amet, consectetur adipiscing".to_string(),
            "elit.".to_string(),
            "Sed do eiusmod tempor incididunt ut labore et".to_string(),
            "dolore magna aliqua.".to_string(),
        ]
    );

    let query = r#"
        SELECT vectorize.chunk_text('This is a simple text that exceeds the limit.', 100)::TEXT[];
    "#;
    let result: Vec<String> = sqlx::query_scalar(query)
        .fetch_one(&conn)
        .await
        .expect("failed to execute query");
    assert_eq!(
        result,
        vec!["This is a simple text that exceeds the limit.".to_string()]
    );
}

#[tokio::test]
async fn test_scheduled_single_table() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    common::init_embedding_svc_url(&conn).await;

    // initialize a job
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        table_method => 'append',
        schedule => '* * * * *'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // should be exactly 1 job in the queue
    let rowcount = common::row_count(&format!("pgmq.q_vectorize_jobs"), &conn).await;
    assert!(rowcount >= 1);

    // embedding should be updated after few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let result = sqlx::query(&format!(
        "SELECT vectorize.search(
        job_name => '{job_name}',
        query => 'mobile devices',
        return_columns => ARRAY['product_name'],
        num_results => 3
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to select from test_table");
    // 3 rows returned
    assert_eq!(result.rows_affected(), 3);
}

#[tokio::test]
async fn test_realtime_append_fail() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    common::init_embedding_svc_url(&conn).await;
    // initialize a job
    let result = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        table_method => 'append',
        schedule => 'realtime'
    );"
    ))
    .execute(&conn)
    .await;
    // realtime + append is not supported
    assert!(result.is_err());
}

#[tokio::test]
async fn test_realtime_job() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    // initialize a job
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name', 'description'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => 'realtime'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    let search_results =
        common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 3);

    let random_product_id = rng.gen_range(0..100000);

    let insert_query = format!(
        "INSERT INTO \"{test_table_name}\"(product_id, product_name, description, product_category, price)
        VALUES ({random_product_id}, 'car tester', $$a product for testing car's components$$, 'electronics', 10.99);"
    );

    // insert a new row
    let _result = sqlx::query(&insert_query)
        .execute(&conn)
        .await
        .expect("failed to insert into test_table");

    // index will need to rebuild
    tokio::time::sleep(tokio::time::Duration::from_secs(5 as u64)).await;
    let search_results =
        common::search_with_retry(&conn, "car testing devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");

    let mut found_it = false;
    for row in search_results {
        let row: common::SearchResult = serde_json::from_value(row.search_results).unwrap();
        if row.product_id == random_product_id {
            assert_eq!(row.product_name, "car tester");
            assert_eq!(row.product_id, random_product_id);
            found_it = true;
        }
    }
    assert!(found_it);

    // test with some double dollar quote string data
    let random_product_id = rng.gen_range(0..100000);

    let insert_query = format!(
        "INSERT INTO \"{test_table_name}\"(product_id, product_name, description, product_category, price)
        VALUES ({random_product_id}, 'messy-product', $DELIM$the $$quick brown fox jump's over the lazy dog$DELIM$, 'product', 10.99);"
    );

    // insert a new row
    let _result = sqlx::query(&insert_query)
        .execute(&conn)
        .await
        .expect("failed to insert into test_table");

    // index will need to rebuild
    tokio::time::sleep(tokio::time::Duration::from_secs(5 as u64)).await;
    let search_results =
        common::search_with_retry(&conn, "messy-product", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");

    let mut found_it = false;
    for row in search_results {
        let row: common::SearchResult = serde_json::from_value(row.search_results).unwrap();
        if row.product_id == random_product_id {
            assert_eq!(row.product_name, "messy-product");
            assert_eq!(row.product_id, random_product_id);
            found_it = true;
        }
    }
    assert!(found_it);
}

#[tokio::test]
async fn test_rag() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let agent_name = format!("agent_{}", test_num);

    // initialize
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{agent_name}',
            relation => '{test_table_name}',
            primary_key => 'product_id',
            columns => ARRAY['description'],
            transformer => 'sentence-transformers/all-MiniLM-L6-v2'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // must be able to conduct vector search on agent tables
    let search_results =
        common::search_with_retry(&conn, "mobile devices", &agent_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 3);
}

#[tokio::test]
async fn test_rag_alternate_schema() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let agent_name = format!("agent_{}", test_num);

    // initialize
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{agent_name}',
            relation => '{test_table_name}',
            primary_key => 'product_id',
            columns => ARRAY['description'],
            transformer => 'sentence-transformers/all-MiniLM-L6-v2'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // must be able to conduct vector search on agent tables
    let search_results =
        common::search_with_retry(&conn, "mobile devices", &agent_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 3);
}

#[tokio::test]
async fn test_static() {
    // a static test. intended for use across extension version updates
    // run this test on one branch, update the extension version, then run it again
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_table_name = "products_test_static";
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = "static_test_job";

    // initialize a job
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name', 'description'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => 'realtime',
        table_method => 'join'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    // TEST BASIC SEARCH FUNCTIONALITY
    let search_results =
        common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 3);

    // TEST INSERT TRIGGER
    let random_product_id = rng.gen_range(1..100000);

    let insert_query = format!(
        "INSERT INTO \"{test_table_name}\"(product_id, product_name, description, product_category, price)
        VALUES ({random_product_id}, 'car tester', 'a product for testing cars', 'electronics', 10.99);"
    );

    // insert a new row
    let _result = sqlx::query(&insert_query)
        .execute(&conn)
        .await
        .expect("failed to insert into test_table");

    // index will need to rebuild
    tokio::time::sleep(tokio::time::Duration::from_secs(5 as u64)).await;
    let search_results =
        common::search_with_retry(&conn, "car testing products", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");

    let mut found_it = false;
    for row in search_results.iter() {
        let row: common::SearchResult = serde_json::from_value(row.search_results.clone()).unwrap();
        if row.product_id == random_product_id {
            assert_eq!(row.product_name, "car tester");
            found_it = true;
        }
    }
    assert!(found_it, "resulting records: {:?}", search_results);

    // TEST UPDATE TRIGGER
    let update_query = format!(
        "UPDATE \"{test_table_name}\" SET
            product_name = 'cat food', description = 'a product for feeding cats'
        WHERE product_id = {random_product_id};"
    );
    let _result = sqlx::query(&update_query)
        .execute(&conn)
        .await
        .expect("failed to insert into test_table");
    // index will need to rebuild
    tokio::time::sleep(tokio::time::Duration::from_secs(5 as u64)).await;
    let search_results = common::search_with_retry(&conn, "cat food", &job_name, 10, 2, 20, None)
        .await
        .expect("failed to exec search");

    let mut found_it = false;
    for row in search_results.iter() {
        let row: common::SearchResult = serde_json::from_value(row.search_results.clone()).unwrap();
        if row.product_id == random_product_id {
            assert_eq!(row.product_name, "cat food");
            assert_eq!(row.description, "a product for feeding cats");
            found_it = true;
        }
    }
    assert!(found_it, "resulting records: {:?}", search_results);
}

#[tokio::test]
async fn test_realtime_tabled() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    // initialize a job
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => 'realtime',
        table_method => 'join'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    let search_results =
        common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 3);

    let random_product_id = rng.gen_range(0..100000);

    // insert a new row
    let insert_query = format!(
        "INSERT INTO \"{test_table_name}\"(product_id, product_name, description, product_category, price)
        VALUES ({random_product_id}, 'car tester', 'a product for testing cars', 'electronics', 10.99);"
    );
    let _result = sqlx::query(&insert_query)
        .execute(&conn)
        .await
        .expect("failed to insert into test_table");

    // index will need to rebuild
    tokio::time::sleep(tokio::time::Duration::from_secs(5 as u64)).await;
    let search_results =
        common::search_with_retry(&conn, "car testing devices", &job_name, 10, 2, 3, None)
            .await
            .expect("failed to exec search");

    let mut found_it = false;
    for row in search_results {
        let row: common::SearchResult = serde_json::from_value(row.search_results).unwrap();
        if row.product_id == random_product_id {
            assert_eq!(row.product_name, "car tester");
            found_it = true;
        }
    }
    assert!(found_it);

    // `join` method must have a view created
    let select = format!(
        "SELECT product_id, product_name, description, embeddings, embeddings_updated_at FROM vectorize.{job_name}_view"
    );
    let result = sqlx::query(&select)
        .fetch_all(&conn)
        .await
        .expect("failed to query project view");

    // 41 rows should be returned
    assert!(result.len() == 41);
}

#[tokio::test]
async fn test_filter_join() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    // initialize a job using `join` method
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => 'realtime',
        table_method => 'join'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    let filter = "product_id = 1".to_string();
    let search_results =
        common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 1, Some(filter))
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 1);
    let result_val = search_results[0].search_results.clone();
    let product_id_val = result_val["product_id"]
        .as_i64()
        .expect("failed parsing product id");
    assert_eq!(product_id_val, 1);

    let filter = "product_id = 1 and product_name ilike 'pencil'".to_string();
    let search_results = common::search_with_retry(
        &conn,
        "some random query",
        &job_name,
        10,
        2,
        1,
        Some(filter),
    )
    .await
    .expect("failed to exec search");
    assert_eq!(search_results.len(), 1);
    let result_val = search_results[0].search_results.clone();
    let product_id_val = result_val["product_id"]
        .as_i64()
        .expect("failed parsing product id");
    assert_eq!(product_id_val, 1);
}

#[tokio::test]
async fn test_filter_append() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    // initialize a job using `join` method
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        table_method => 'append'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to init job");

    let filter = "product_id = 2".to_string();
    let search_results =
        common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 1, Some(filter))
            .await
            .expect("failed to exec search");
    assert_eq!(search_results.len(), 1);
    let result_val = search_results[0].search_results.clone();
    let product_id_val = result_val["product_id"]
        .as_i64()
        .expect("failed parsing product id");
    assert_eq!(product_id_val, 2);
}

#[tokio::test]
async fn test_index_dist_type_hnsw_cosine() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;

    let dist_type = "pgv_hnsw_cosine";
    let test_num: u32 = rand::thread_rng().gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    let job_name = format!("job_{}", test_num);

    common::init_test_table(&test_table_name, &conn).await;

    let query = format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        index_dist_type => '{dist_type}',
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        schedule => 'realtime'
    );",
        job_name = job_name,
        test_table_name = test_table_name,
        dist_type = dist_type,
    );

    let result = sqlx::query(&query).execute(&conn).await;

    assert!(
        result.is_ok(),
        "pgv_hnsw_cosine index_dist_type should pass but failed with: {:?}",
        result.err()
    );

    // Now perform a search operation to ensure it passes
    let search_query = format!(
        "SELECT * FROM vectorize.search(
            job_name => '{job_name}',
            query => 'search query',
            return_columns => ARRAY['product_name'],
            num_results => 10
        );",
        job_name = job_name
    );

    let search_result = sqlx::query(&search_query).fetch_all(&conn).await;

    // Assert that the search operation is successful
    assert!(
        search_result.is_ok(),
        "Search operation failed with pgv_hnsw_cosine distribution type: {:?}",
        search_result.err()
    );
}

async fn test_index_dist_type_hnsw_l2() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;

    let dist_type = "pgv_hnsw_l2";
    let test_num: u32 = rand::thread_rng().gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    let job_name = format!("job_{}", test_num);

    // Initialize the test table and job
    common::init_test_table(&test_table_name, &conn).await;
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{job_name}',
            relation => '{test_table_name}',
            primary_key => 'product_id',
            columns => ARRAY['product_name'],
            index_dist_type => '{dist_type}',
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            schedule => 'realtime'
        );",
        job_name = job_name,
        test_table_name = test_table_name,
        dist_type = dist_type,
    ))
    .execute(&conn)
    .await
    .expect("failed to initialize job");

    // Directly call vectorize.search to perform a search operation
    let search_query = format!(
        "SELECT * FROM vectorize.search(
            job_name => '{job_name}',
            query => 'search query',
            return_columns => ARRAY['product_name'],
            num_results => 10
        );",
        job_name = job_name
    );

    let search_result = sqlx::query(&search_query).fetch_all(&conn).await;

    // Assert that the search operation fails as expected with pgv_hnsw_l2
    assert!(
        search_result.is_err(),
        "Expected search with pgv_hnsw_l2 to fail, but it succeeded."
    );
}

#[tokio::test]
async fn test_index_dist_type_hnsw_ip() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;

    let dist_type = "pgv_hnsw_ip";
    let test_num: u32 = rand::thread_rng().gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    let job_name = format!("job_{}", test_num);

    // Initialize the test table and job
    common::init_test_table(&test_table_name, &conn).await;
    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{job_name}',
            relation => '{test_table_name}',
            primary_key => 'product_id',
            columns => ARRAY['product_name'],
            index_dist_type => '{dist_type}',
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            schedule => 'realtime'
        );",
        job_name = job_name,
        test_table_name = test_table_name,
        dist_type = dist_type,
    ))
    .execute(&conn)
    .await
    .expect("failed to initialize job");

    // Directly call vectorize.search to perform a search operation
    let search_query = format!(
        "SELECT * FROM vectorize.search(
            job_name => '{job_name}',
            query => 'search query',
            return_columns => ARRAY['product_name'],
            num_results => 10
        );",
        job_name = job_name
    );

    let search_result = sqlx::query(&search_query).fetch_all(&conn).await;

    // Assert that the search operation fails as expected with pgv_hnsw_l2
    assert!(
        search_result.is_err(),
        "Expected search with pgv_hnsw_ip to fail, but it succeeded."
    );
}

#[ignore]
#[tokio::test]
async fn test_private_hf_model() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_{}", test_num);

    common::init_embedding_svc_url(&conn).await;

    let hf_api_key = std::env::var("HF_API_KEY").expect("HF_API_KEY must be set");

    sqlx::query(&format!(
        "alter system set vectorize.embedding_service_api_key to '{hf_api_key}'"
    ))
    .execute(&conn)
    .await
    .unwrap();
    sqlx::query(&format!("select pg_reload_conf();"))
        .execute(&conn)
        .await
        .unwrap();

    // initialize a job
    let created = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'chuckhend/private-model',
        schedule => 'realtime'
    );"
    ))
    .execute(&conn)
    .await;

    assert!(created.is_ok(), "Failed with error: {:?}", created);

    // embedding should be updated after few seconds
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let result = sqlx::query(&format!(
        "SELECT vectorize.search(
        job_name => '{job_name}',
        query => 'mobile devices',
        return_columns => ARRAY['product_name'],
        num_results => 3
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to select from test_table");
    // 3 rows returned
    assert_eq!(result.rows_affected(), 3);
}

#[tokio::test]
async fn test_diskann_cosine() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("job_diskann_{}", test_num);

    common::init_embedding_svc_url(&conn).await;
    // initialize a job
    let result = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2',
        index_dist_type => 'vsc_diskann_cosine',
        schedule => 'realtime'
    );"
    ))
    .execute(&conn)
    .await;
    assert!(result.is_ok(), "{:?}", result.err());

    let search_results: Vec<common::SearchJSON> =
        match util::common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
        {
            Ok(results) => results,
            Err(e) => {
                eprintln!("Error: {:?}", e);
                panic!("failed to exec search on diskann");
            }
        };
    assert_eq!(search_results.len(), 3);
}

#[ignore]
#[tokio::test]
async fn test_cohere() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    common::init_test_table(&test_table_name, &conn).await;
    let job_name = format!("cohere_{}", test_num);

    let hf_api_key = std::env::var("CO_API_KEY").expect("CO_API_KEY must be set");

    sqlx::query(&format!(
        "alter system set vectorize.cohere_api_key to '{hf_api_key}'"
    ))
    .execute(&conn)
    .await
    .unwrap();
    sqlx::query(&format!("select pg_reload_conf();"))
        .execute(&conn)
        .await
        .unwrap();

    common::init_embedding_svc_url(&conn).await;
    // initialize a job
    let result = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'cohere/embed-multilingual-light-v3.0',
        schedule => 'realtime'
    );"
    ))
    .execute(&conn)
    .await;

    assert!(result.is_ok());

    let search_results: Vec<common::SearchJSON> =
        util::common::search_with_retry(&conn, "mobile devices", &job_name, 10, 2, 3, None)
            .await
            .unwrap();
    assert_eq!(search_results.len(), 3);
}

#[tokio::test]
async fn test_event_trigger_on_table_drop() {
    let conn = common::init_database().await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);
    let test_table_name = format!("products_test_{}", test_num);
    let job_name = format!("job_{}", test_num);

    // Initialize the test table and job
    common::init_test_table(&test_table_name, &conn).await;
    common::init_embedding_svc_url(&conn).await;

    let _ = sqlx::query(&format!(
        "SELECT vectorize.table(
        job_name => '{job_name}',
        relation => '{test_table_name}',
        primary_key => 'product_id',
        columns => ARRAY['product_name'],
        transformer => 'sentence-transformers/all-MiniLM-L6-v2'
    );"
    ))
    .execute(&conn)
    .await
    .expect("failed to initialize vectorize job");

    // Check the job table before dropping the test table
    let job_count_before = common::row_count("vectorize.job", &conn).await;
    assert_eq!(job_count_before, 1);

    // Drop the test table
    let drop_result = sqlx::query(&format!("DROP TABLE {test_table_name} CASCADE;"))
        .execute(&conn)
        .await;
    assert!(drop_result.is_ok(), "Failed to drop the test table");

    // Debug: Check job table after dropping the test table
    let job_count_after = common::row_count("vectorize.job", &conn).await;
    assert_eq!(
        job_count_after, 0,
        "Job entry was not removed after table drop"
    );

    // Check if the job was deleted
    let deleted_job = sqlx::query(
        "SELECT * FROM vectorize.job WHERE params->>'table' = $1 AND params->>'schema' = $2",
    )
    .bind(test_table_name)
    .bind("public")
    .fetch_optional(&conn)
    .await
    .expect("Failed to fetch job");

    assert!(
        deleted_job.is_none(),
        "Job was not deleted after table drop"
    );

    // Attempt to drop a non-associated table and verify no action is taken
    let unrelated_table_name = format!("unrelated_test_{}", test_num);
    common::init_test_table(&unrelated_table_name, &conn).await;
    let _ = sqlx::query(&format!("DROP TABLE {unrelated_table_name};"))
        .execute(&conn)
        .await
        .expect("Failed to drop the unrelated test table");

    // Ensure vectorize.job is unaffected
    let final_job_count = common::row_count("vectorize.job", &conn).await;
    assert_eq!(
        final_job_count, 0,
        "vectorize.job should remain unaffected by unrelated table drops"
    );
}

#[tokio::test]
async fn test_chunk_table() {
    let conn = common::init_database().await;
    sqlx::raw_sql(include_str!("chunk_table.sql"))
        .execute(&conn)
        .await
        .expect("chunk_table SQL regressions failed");
}

#[tokio::test]
async fn test_import_embeddings() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);

    // Create source table with embeddings
    let src_table_name = format!("source_embeddings_test_{}", test_num);
    sqlx::query(&format!(
        "CREATE TABLE {} (
            id SERIAL PRIMARY KEY,
            content TEXT,
            embeddings vector(384)
        )",
        src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create source table");

    // Create a properly formatted vector string with 384 dimensions
    let vector_values = (0..384)
        .map(|i| {
            if i < 2 {
                format!("{:.1}", (i as f32 + 1.0) / 10.0)
            } else {
                "0.0".to_string()
            }
        })
        .collect::<Vec<String>>()
        .join(", ");

    // Insert test data with embeddings
    sqlx::query(&format!(
        "INSERT INTO {} (content, embeddings) VALUES
        ('test content 1', '[{}]'::vector),
        ('test content 2', '[{}]'::vector)",
        src_table_name, vector_values, vector_values
    ))
    .execute(&conn)
    .await
    .expect("failed to insert test data");

    // Test with join table method
    let join_job_name = format!("join_import_test_{}", test_num);
    let dest_table_name = format!("dest_table_test_{}", test_num);

    // Create destination table
    sqlx::query(&format!(
        "CREATE TABLE {} (
            id SERIAL PRIMARY KEY,
            content TEXT
        )",
        dest_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create destination table");

    // Insert matching records
    sqlx::query(&format!(
        "INSERT INTO {} (id, content)
         SELECT id, content FROM {}",
        dest_table_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to insert destination data");

    // Initialize vectorize job
    sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{}',
            relation => '{}',
            primary_key => 'id',
            columns => ARRAY['content'],
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            schedule => 'realtime',
            table_method => 'join'
        )",
        join_job_name, dest_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to initialize vectorize job");

    // Import embeddings
    sqlx::query(&format!(
        "SELECT vectorize.import_embeddings(
            job_name => '{}',
            src_table => '{}',
            src_primary_key => 'id',
            src_embeddings_col => 'embeddings'
        )",
        join_job_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to import embeddings");

    // Verify embeddings were imported correctly
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM vectorize._embeddings_{}",
        join_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count embeddings");
    assert_eq!(count, 2, "Expected 2 embeddings to be imported");

    // Verify no pending jobs in queue for realtime schedule
    let queue_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM pgmq.q_vectorize_jobs WHERE message->>'job_name' = '{}'",
        join_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count queue messages");
    assert_eq!(queue_count, 0, "Expected no pending jobs in queue");

    // Test with append table method
    let append_job_name = format!("append_import_test_{}", test_num);
    let append_table_name = format!("append_table_test_{}", test_num);

    // Create append destination table
    sqlx::query(&format!(
        "CREATE TABLE public.{} (
            id SERIAL PRIMARY KEY,
            content TEXT,
            last_updated_at TIMESTAMPTZ DEFAULT NOW()
        )",
        append_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create append destination table");

    // Insert matching records
    sqlx::query(&format!(
        "INSERT INTO {} (id, content)
         SELECT id, content FROM {}",
        append_table_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to insert append destination data");

    // Initialize vectorize job with append method
    sqlx::query(&format!(
        "SELECT vectorize.table(
            job_name => '{}',
            relation => '{}',
            primary_key => 'id',
            columns => ARRAY['content'],
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            table_method => 'append'
        )",
        append_job_name, append_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to initialize append vectorize job");

    // Import embeddings for append method
    sqlx::query(&format!(
        "SELECT vectorize.import_embeddings(
            job_name => '{}',
            src_table => '{}',
            src_primary_key => 'id',
            src_embeddings_col => 'embeddings'
        )",
        append_job_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to import embeddings for append method");

    // Verify embeddings were imported correctly for append method
    let append_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM {} WHERE {}_embeddings IS NOT NULL",
        append_table_name, append_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count append embeddings");
    assert_eq!(
        append_count, 2,
        "Expected 2 embeddings to be imported for append method"
    );
}

#[tokio::test]
async fn test_table_from() {
    let conn = common::init_database().await;
    common::init_embedding_svc_url(&conn).await;
    let mut rng = rand::thread_rng();
    let test_num = rng.gen_range(1..100000);

    // Create source table with embeddings
    let src_table_name = format!("source_embeddings_test_{}", test_num);
    sqlx::query(&format!(
        "CREATE TABLE {} (
            id SERIAL PRIMARY KEY,
            content TEXT,
            embeddings vector(384)
        )",
        src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create source table");

    // Create a properly formatted vector string with 384 dimensions
    let vector_values = (0..384)
        .map(|i| {
            if i < 2 {
                format!("{:.1}", (i as f32 + 1.0) / 10.0)
            } else {
                "0.0".to_string()
            }
        })
        .collect::<Vec<String>>()
        .join(", ");

    // Insert test data with embeddings
    sqlx::query(&format!(
        "INSERT INTO {} (content, embeddings) VALUES
        ('test content 1', '[{}]'::vector),
        ('test content 2', '[{}]'::vector)",
        src_table_name, vector_values, vector_values
    ))
    .execute(&conn)
    .await
    .expect("failed to insert test data");

    // Create destination table
    let dest_table_name = format!("dest_table_test_{}", test_num);
    sqlx::query(&format!(
        "CREATE TABLE {} (
            id SERIAL PRIMARY KEY,
            content TEXT,
            last_updated_at TIMESTAMPTZ DEFAULT NOW()
        )",
        dest_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create destination table");

    // Insert matching records
    sqlx::query(&format!(
        "INSERT INTO {} (id, content)
         SELECT id, content FROM {}",
        dest_table_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to insert destination data");

    // Test both realtime and cron scheduling

    // Test realtime scheduling
    let realtime_job_name = format!("table_from_test_realtime_{}", test_num);
    sqlx::query(&format!(
        "SELECT vectorize.table_from(
            relation => '{}',
            columns => ARRAY['content'],
            job_name => '{}',
            primary_key => 'id',
            src_table => '{}',
            src_primary_key => 'id',
            src_embeddings_col => 'embeddings',
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            schedule => 'realtime'
        )",
        dest_table_name, realtime_job_name, src_table_name
    ))
    .execute(&conn)
    .await
    .expect("failed to create table from embeddings with realtime schedule");

    // Test cron scheduling
    let cron_job_name = format!("table_from_test_cron_{}", test_num);
    let mut cron_tx = conn.begin().await.expect("failed to begin cron setup");
    sqlx::query(&format!(
        "SELECT vectorize.table_from(
            relation => '{}',
            columns => ARRAY['content'],
            job_name => '{}',
            primary_key => 'id',
            src_table => '{}',
            src_primary_key => 'id',
            src_embeddings_col => 'embeddings',
            transformer => 'sentence-transformers/all-MiniLM-L6-v2',
            schedule => '* * * * *'
        )",
        dest_table_name, cron_job_name, src_table_name
    ))
    .execute(&mut *cron_tx)
    .await
    .expect("failed to create table from embeddings with cron schedule");

    // Publish the cron job in a paused state so a minute boundary cannot race
    // the assertion that only the realtime job processes the new row.
    sqlx::query("SELECT cron.alter_job(jobid, active := false) FROM cron.job WHERE jobname = $1")
        .bind(&cron_job_name)
        .fetch_one(&mut *cron_tx)
        .await
        .expect("failed to pause the registered cron job");
    cron_tx.commit().await.expect("failed to commit cron setup");

    // Verify embeddings were imported correctly for both jobs
    let realtime_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM vectorize._embeddings_{}",
        realtime_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count embeddings");
    assert_eq!(
        realtime_count, 2,
        "Expected 2 embeddings to be imported for realtime job"
    );

    let cron_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM vectorize._embeddings_{}",
        cron_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count embeddings");
    assert_eq!(
        cron_count, 2,
        "Expected 2 embeddings to be imported for cron job"
    );

    // Test realtime updates
    let new_id = 3;
    sqlx::query(&format!(
        "INSERT INTO {} (id, content) VALUES ($1, $2)",
        dest_table_name
    ))
    .bind(new_id)
    .bind("test content 3")
    .execute(&conn)
    .await
    .expect("failed to insert new record");

    // Wait for the worker instead of assuming a fixed processing time.
    let new_realtime_count = tokio::time::timeout(tokio::time::Duration::from_secs(30), async {
        loop {
            let count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM vectorize._embeddings_{}",
                realtime_job_name
            ))
            .fetch_one(&conn)
            .await
            .expect("failed to count embeddings after update");
            if count >= 3 {
                break count;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for the realtime embedding");
    assert_eq!(
        new_realtime_count, 3,
        "Expected 3 embeddings after update in realtime job"
    );

    // The paused cron job should still have the original count.
    let new_cron_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM vectorize._embeddings_{}",
        cron_job_name
    ))
    .fetch_one(&conn)
    .await
    .expect("failed to count embeddings after update");
    assert_eq!(
        new_cron_count, 2,
        "Expected cron job to still have 2 embeddings"
    );
}
