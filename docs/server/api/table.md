## POST /api/v1/table

Create (initialize) a vectorize job that will generate embeddings and keep them in sync for a Postgres table.

URL

 /api/v1/table

Method

 POST

Request body (JSON)

 The request expects a `VectorizeJob` JSON object. Required fields below are inferred from the server code and core types used by the server.

 - job_name: string
   - A unique name for this vectorize job.
 - src_table: string
   - Table name that contains the data to index.
 - src_schema: string
   - Schema name where the table lives (e.g., `public`).
 - src_columns: array[string]
   - List of columns to include when building the embeddings (for example: `["product_name", "description"]`).
 - primary_key: string
   - Column name of the primary key for the source table.
 - update_time_col: string
   - Column name that contains last-updated timestamps for rows. NOTE: the server enforces this column is of type `timestamp with time zone`.
 - model: string
   - Embedding model identifier (e.g. `sentence-transformers/all-MiniLM-L6-v2` or other provider model string supported by the transformers/provider layer).

Example request

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

Validation and behavior

 - The server validates that `update_time_col` exists on the table and its data type is `timestamp with time zone`. If not, the server will return an error.
 - On success the server initializes job metadata in Postgres and returns a JSON object with the job id.

Success response (200)

```json
{
  "id": "<uuid>"
}
```

Errors

 - 400 / InvalidRequest - malformed payload or validation failed (e.g., wrong timestamp type)
 - 404 / NotFound - referenced table/column or objects not found
 - 500 / InternalServerError - other server-side errors
