# Embedding and Vector Search on any Postgres

Quick start -- just run the contains locally:

```bash
docker compose up -d
```

Create a table and insert some data:

```bash
psql postgres://postgres:postgres@0.0.0.0:5432/postgres -c "CREATE TABLE my_table (id SERIAL PRIMARY KEY, content TEXT, updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP);"

psql postgres://postgres:postgres@0.0.0.0:5432/postgres -c "INSERT INTO my_table (content) VALUES ('pizza'), ('pencil'), ('airplane');"
```

Generating embeddings:

```bash
curl -X POST http://0.0.0.0:8080/api/v1/table -d '{
        "job_name": "my_job",
        "src_table": "my_table",
        "src_schema": "public",
        "src_column": "content",
        "primary_key": "id",
        "update_time_col": "updated_at",
        "model": "sentence-transformers/all-MiniLM-L6-v2"
    }' \
    -H "Content-Type: application/json"
```

Search for similar content:

```bash
curl -X GET http://0.0.0.0:8080/api/v1/search -d '{
        "job_name": "my_job",
        "query": "food"
    }'     -H "Content-Type: application/json" | jq .
```

```json
[
  {
    "content": "pizza",
    "id": 1,
    "similarity_score": 0.637525878100046,
    "updated_at": "2025-05-31T01:13:09.349983+00:00"
  },
  {
    "content": "airplane",
    "id": 3,
    "similarity_score": 0.31890476700379766,
    "updated_at": "2025-05-31T01:13:09.349983+00:00"
  },
  {
    "content": "pencil",
    "id": 2,
    "similarity_score": 0.25519378452338115,
    "updated_at": "2025-05-31T01:13:09.349983+00:00"
  }
]
```
