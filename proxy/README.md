## SQL proxy

The proxy gives you a SQL interface to `vectorize.search()` without installing the Postgres extension. It sits in front of Postgres, intercepts `vectorize.search()` calls, generates embeddings, rewrites the query as a hybrid (semantic + full-text) search, and returns results — all transparently over the Postgres wire protocol. Any SQL client that works with Postgres works with the proxy.

Start Postgres and the embeddings server:

```bash
docker compose up postgres vector-serve -d
```

Load the example dataset:

```bash
psql postgres://postgres:postgres@localhost:5432/postgres -f server/sql/example.sql
```

In a second terminal, start the HTTP server. This is used to manage embedding jobs and generate the initial embeddings for existing rows:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  EMBEDDING_SVC_URL=http://localhost:3000/v1 \
  cargo run --bin vectorize-server
```

Initialize the table and create the embedding job:

```bash
curl -X POST http://localhost:8080/api/v1/table -d '{
    "job_name": "my_job",
    "src_table": "my_products",
    "src_schema": "public",
    "src_columns": ["product_name", "description"],
    "primary_key": "product_id",
    "update_time_col": "updated_at",
    "model": "sentence-transformers/all-MiniLM-L6-v2"
  }' -H "Content-Type: application/json"
```

In a third terminal, start the proxy. It listens on port 5433 by default:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  EMBEDDING_SVC_URL=http://localhost:3000/v1 \
  cargo run --bin vectorize-proxy
```

Search using SQL by connecting `psql` to the proxy port (5433):

```bash
psql postgres://postgres:postgres@localhost:5433/postgres -c \
  "SELECT * FROM vectorize.search(job=>'my_job', query=>'camping backpack', num_results=>3);"
```

```text
                                                                       results                                                                                                                                                                             
----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
 {"price": 45.00, "fts_rank": 1, "rrf_score": 0.03278688524590164, "product_id": 6, "updated_at": "2026-05-12T14:37:26.610753+00:00", "description": "Storage solution for carrying personal items on ones back", "product_name": "Backpack", "semantic_rank": 1, "product_category": "accessories", "similarity_score": 0.6296013593673885}
 {"price": 40.00, "fts_rank": null, "rrf_score": 0.016129032258064516, "product_id": 39, "updated_at": "2026-05-12T14:37:26.610753+00:00", "description": "Sling made of fabric or netting, suspended between two points for relaxation", "product_name": "Hammock", "semantic_rank": 2, "product_category": "outdoor", "similarity_score": 0.3789524291697087}
 {"price": 10.99, "fts_rank": null, "rrf_score": 0.015873015873015872, "product_id": 12, "updated_at": "2026-05-12T14:37:26.610753+00:00", "description": "Insulated container for beverages on-the-go", "product_name": "Travel Mug", "semantic_rank": 3, "product_category": "kitchenware", "similarity_score": 0.35918538314991255}
(3 rows)
```