
## GET /api/v1/search


Perform a hybrid semantic + full-text search against a previously initialized vectorize job.

URL

 /api/v1/search

Method

 GET

Query parameters

| Parameter    | Type    | Required | Default     | Description |
|--------------|:-------:|:--------:|:-----------:|------------|
| job_name     | string  | yes      | —           | Name of the vectorize job to search. This identifies the table, schema, model and other job configuration.
| query        | string  | yes      | —           | The user's search query string.
| limit        | int     | no       | 10          | Maximum number of results to return.
| window_size  | int     | no       | 5 * limit   | Internal window size used by the hybrid search algorithm.
| rrf_k        | float   | no       | 60.0        | Reciprocal Rank Fusion parameter used by the hybrid ranking.
| semantic_wt  | float   | no       | 1.0         | Weight applied to the semantic score.
| fts_wt       | float   | no       | 1.0         | Weight applied to the full-text-search score.
| filters      | object  | no       | —           | Additional filters passed as separate query parameters. The server parses values into typed filter values and validates keys/values for safety.

Notes on filters

Filters are supplied as individual URL query parameters (not as a single JSON payload). The server will parse them into a map (BTreeMap) of filter keys to typed values. The server validates raw string inputs to avoid SQL injection; only the job configuration may define allowed table and column names when the job was created. See the source for details about accepted filter types and how values are interpreted.

Example: passing filters in a curl request

```bash
curl -G "http://localhost:8080/api/v1/search" \
  --data-urlencode "job_name=my_job" \
  --data-urlencode "query=camping gear" \
  --data-urlencode "limit=2" \
  --data-urlencode "product_category=outdoor" \
  --data-urlencode "price<10"
```

In the example above the server will receive two filter parameters: `product_category=outdoor` and `min_price=10`. The server attempts to convert values to the appropriate column types based on the job's schema.

Example request

```bash
curl -G "http://localhost:8080/api/v1/search" \
  --data-urlencode "job_name=my_job" \
  --data-urlencode "query=camping gear" \

  # /api/v1/search (GET & POST)

  Perform a hybrid semantic + full-text search against a previously initialized vectorize job.

  This endpoint supports both GET and POST methods:

  - **GET**: Accepts parameters as URL query parameters.
  - **POST**: Accepts parameters as a JSON object in the request body.

  ---

  ## Parameters

  The following parameters are accepted by both GET and POST requests:

  | Parameter    | Type    | Required | Default     | Description |
  |--------------|:-------:|:--------:|:-----------:|------------|
  | job_name     | string  | yes      | —           | Name of the vectorize job to search. This identifies the table, schema, model and other job configuration. |
  | query        | string  | yes      | —           | The user's search query string. |
  | limit        | int     | no       | 10          | Maximum number of results to return. |
  | window_size  | int     | no       | 5 * limit   | Internal window size used by the hybrid search algorithm. |
  | rrf_k        | float   | no       | 60.0        | Reciprocal Rank Fusion parameter used by the hybrid ranking. |
  | semantic_wt  | float   | no       | 1.0         | Weight applied to the semantic score. |
  | fts_wt       | float   | no       | 1.0         | Weight applied to the full-text-search score. |
  | filters      | object  | no       | —           | Additional filters to restrict results. See below for details. |

  ---

  ### Notes on filters

  - **GET**: Filters are supplied as individual URL query parameters (e.g., `product_category=outdoor`, `price<10`).
  - **POST**: Filters are supplied as a JSON object in the `filters` field (e.g., `{ "product_category": "outdoor", "price": { "$lt": 10 } }`).

  The server parses and validates filter values according to the job's schema and allowed columns. Only columns defined in the job configuration may be filtered. See the source for details about accepted filter types and how values are interpreted.

  ---

  ## GET /api/v1/search

  **URL**

      /api/v1/search

  **Method**

      GET

  **How to use**

  Pass parameters as URL query parameters. Example:

  ```bash
  curl -G "http://localhost:8080/api/v1/search" \
    --data-urlencode "job_name=my_job" \
    --data-urlencode "query=camping gear" \
    --data-urlencode "limit=2" \
    --data-urlencode "product_category=outdoor" \
    --data-urlencode "price<10"
  ```

  ---

  ## POST /api/v1/search

  **URL**

      /api/v1/search

  **Method**

      POST

  **How to use**

  Pass parameters as a JSON object in the request body. Example:

  ```bash
  curl -X POST "http://localhost:8080/api/v1/search" \
    -H "Content-Type: application/json" \
    -d '{
      "job_name": "my_job",
      "query": "camping gear",
      "limit": 2,
      "filters": {"product_category": "outdoor", "price": {"$lt": 10}}
    }'
  ```

  ---

  ## Example response (200)

  The endpoint returns an array of JSON objects. The exact shape depends on the columns selected by the job (server uses `SELECT *` for results), plus additional ranking fields. Example returned item:

  ```json
  [
    {
      "product_id": 39,
      "product_name": "Hammock",
      "description": "Sling made of fabric or netting, suspended between two points for relaxation",
      "product_category": "outdoor",
      "price": 40.0,
      "updated_at": "2025-06-25T19:57:22.410561+00:00",
      "semantic_rank": 1,
      "similarity_score": 0.3192296909597241,
      "rrf_score": 0.01639344262295082,
      "fts_rank": null
    }
  ]
  ```

  ---

  ## Errors

  - 400 / InvalidRequest - missing or invalid parameters
  - 404 / NotFound - job not found
  - 500 / InternalServerError - other server-side errors
    "limit": 2,
    "filters": {"product_category": "outdoor", "price": {"$lt": 10}}
  }'
```

**Example response (200)**

The response format is identical to the GET endpoint (see above).

**Errors**

 - 400 / InvalidRequest - missing or invalid parameters
 - 404 / NotFound - job not found
 - 500 / InternalServerError - other server-side errors
