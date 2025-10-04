# Instant Search on any Postgres

An HTTP server that sits in between your application and Postgres.

## See also

- Top-level project overview: `../README.md`

## Features
- Quickly sets up semantic and full text search on any Postgres table.
- Generate embeddings from OpenAI, Hugging Face, and many other embedding model providers.
- Updates embeddings and full text search token indices whenever data changes
- Compatible with any Postgres that has [pgvector](https://github.com/pgvector/pgvector) installed (RDS, CloudSQL, etc)

## Getting started

Run Postgres and the HTTP servers in separate containers locally:

```bash
# docker-compose.yml is located in the root of this repository
docker compose -f docker-compose.yml up -d
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

We'll use the API to create a job that will  generate embeddings for the `product_name` and `description` columns in the `my_products` table. Anytime we insert or update a row in this table, the embeddings will automatically be updated.

```bash
curl -X POST http://localhost:8080/api/v1/table -d '{
        "job_name": "my_job",
        "src_table": "my_products",
        "src_schema": "public",
        "src_columns": ["product_name", "description"],
        "primary_key": "product_id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    }' \
    -H "Content-Type: application/json"
```

## Search with HTTP API

```bash
curl -X GET "http://localhost:8080/api/v1/search?job_name=my_job&query=camping%20gear&limit=2" | jq .
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

## Running on an existing Postgres instance

Assuming you have an existing Postgres instance with `pgvector` installed, you can run the HTTP servers using Docker and get started quickly.

Set the following env vars in a `.env` file:

If your embedding model is gated or private on Hugging Face, you will also need to set the `HF_API_KEY` environment variable. Otherwise you can ignore it.

```dotenv
DATABASE_URL=postgresql://user:password@your-postgres-host:5432/postgres
HF_API_KEY=your_huggingface_api_key
```

Then start the search and embedding servers:

```bash
docker compose up -d
```

Then generate embeddings and indices as describe [above](#generating-embeddings).

Finally, search using the [HTTP API](#search-with-http-api).
