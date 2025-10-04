## GET /api/v1/search

Perform a hybrid semantic + full-text search against a previously initialized vectorize job.

URL

 /api/v1/search

Method

 GET

Query parameters

 - job_name (string) - required
   - Name of the vectorize job to search. This identifies the table, schema, model and other job configuration.
 - query (string) - required
   - The user's search query string.
 - limit (int) - optional, default: 10
   - Maximum number of results to return.
 - window_size (int) - optional, default: 5 * limit
   - Internal window size used by the hybrid search algorithm.
 - rrf_k (float) - optional, default: 60.0
   - Reciprocal Rank Fusion param used by the hybrid ranking.
 - semantic_wt (float) - optional, default: 1.0
   - Weight applied to the semantic score.
 - fts_wt (float) - optional, default: 1.0
   - Weight applied to the full-text-search score.
 - filters (object) - optional
   - Additional filters are accepted as query params and are passed as typed filter values to the query builder. Filters are provided as URL query parameters and will be parsed into a map of keys to values. The server validates keys and raw string values for safety.

Notes on filters

 Filters are supplied as query parameters and the server will parse them into a BTreeMap of filter keys and typed values. The server validates string inputs to avoid SQL injection; only the job is allowed to specify table/column names on job creation. See the source for details about accepted filter types.

Example request

```bash
curl -G "http://localhost:8080/api/v1/search" \
  --data-urlencode "job_name=my_job" \
  --data-urlencode "query=camping gear" \
  --data-urlencode "limit=2"
```

Example response (200)

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

Errors

 - 400 / InvalidRequest - missing or invalid parameters
 - 404 / NotFound - job not found
 - 500 / InternalServerError - other server-side errors
