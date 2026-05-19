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
  "SELECT product_id, product_name, semantic_rank, fts_rank, similarity_score FROM vectorize.search(job=>'my_job', query=>'camping backpack', num_results=>3);"
```

```text
 product_id | product_name | semantic_rank | fts_rank |  similarity_score   
------------+--------------+---------------+----------+---------------------
          6 | Backpack     |             1 |        1 |  0.6296013593673706
         39 | Hammock      |             2 |          | 0.37895236548639444
         12 | Travel Mug   |             3 |          |  0.3591853487248824
(3 rows)
```