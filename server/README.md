# Instant Search on any Postgres

An HTTP server that sits in between your application and Postgres.

## Features
- Quickly sets up semantic and full text search on any Postgres table.
- Generate embeddings from OpenAI, Hugging Face, and many other embedding model providers.
- Updates embeddings and full text search token indices whenever data changes
- Compatible with any Postgres that has [pgvector](https://github.com/pgvector/pgvector) installed (RDS, CloudSQL, etc)

## Getting started

Run Postgres and the HTTP servers in separate containers locally:

```bash
# docker-compose.server.yml is located in the root of this repository
docker compose -f docker-compose.server.yml up -d
```

There are three contains; postgres, a local embedding server, and the HTTP search service.

```plaintext
docker ps --format "table {{.Image}}\t{{.Names}}"

IMAGE                                   NAMES
pg_vectorize-server                     pg_vectorize-server-1
pgvector/pgvector:0.8.0-pg17            pg_vectorize-postgres-1
ghcr.io/chuckhend/vector-serve:latest   pg_vectorize-vector-serve-1
```

## Create a table and insert some data

`sql/example.sql` contains an example products data set.

```bash
psql postgres://postgres:postgres@localhost:5432/postgres -f sql/example.sql
```

## Generating embeddings

We'll use the API to create a job that will  generate embeddings for the `description` column in the `my_products` table. Anytime we insert or update a row in this table, the embeddings will automatically be updated.

```bash
curl -X POST http://localhost:8080/api/v1/table -d '{
        "job_name": "my_job",
        "src_table": "my_products",
        "src_schema": "public",
        "src_column": "description",
        "primary_key": "product_id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    }' \
    -H "Content-Type: application/json"
```

## Search with HTTP API

```bash
curl -X GET "http://localhost:8080/api/v1/search?job_name=my_job&query=camping%20grear&limit=2" | jq .
```

```json
[
  {
    "description": "Sling made of fabric or netting, suspended between two points for relaxation",
    "fts_rank": null,
    "price": 40.0,
    "product_category": "outdoor",
    "product_id": 39,
    "product_name": "Hammock",
    "rrf_score": 0.01639344262295082,
    "semantic_rank": 1,
    "similarity_score": 0.3192296909597241,
    "updated_at": "2025-06-25T19:57:22.410561+00:00"
  },
  {
    "description": "Container for holding plants, often with drainage",
    "fts_rank": null,
    "price": 12.0,
    "product_category": "garden",
    "product_id": 8,
    "product_name": "Plant Pot",
    "rrf_score": 0.016129032258064516,
    "semantic_rank": 2,
    "similarity_score": 0.3032694847366062,
    "updated_at": "2025-06-25T19:57:22.410561+00:00"
  }
]
```

## SQL proxy example

We can also use the SQL proxy to perform the same search query, but using SQL instead of the HTTP API. This is useful if you have additional joins are advanced SQL queries that you want to perform.

Note that this query routes through the proxy on port 5433.

```sql
psql postgres://postgres:postgres@localhost:5433/postgres -c \
"SELECT * FROM (
    SELECT t0.*, t1.similarity_score
    FROM (
        SELECT
            product_id,
            1 - (embeddings <=> vectorize.embed('plants', 'my_job')) as similarity_score
        FROM vectorize._embeddings_my_job
        ) t1
    INNER JOIN public.my_products t0 on t0.product_id = t1.product_id
) t
ORDER BY t.similarity_score DESC
LIMIT 2;"
```

```plaintext
 product_id |   product_name   |                    description                    | product_category | price |          updated_at           |  similarity_score   
------------+------------------+---------------------------------------------------+------------------+-------+-------------------------------+---------------------
          8 | Plant Pot        | Container for holding plants, often with drainage | garden           | 12.00 | 2025-06-25 20:27:07.725765+00 | 0.46105278002586925
         35 | Gardening Gloves | Handwear for protection during gardening tasks    | garden           |  8.00 | 2025-06-25 20:27:07.725765+00 |  0.2909192990160845
(2 rows)
```