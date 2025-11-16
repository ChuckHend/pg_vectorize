
## Hybrid Search

Perform a hybrid semantic + full-text search against a previously initialized vectorize job.

### /api/v1/search

The following query parameters are available on both the GET and POST methods.

- **GET**: Accepts parameters as URL query parameters.
- **POST**: Accepts parameters as a JSON object in the request body.

Query parameters:

| Parameter   |  Type  | Required |  Default  | Description                                                                                                                                     |
| ----------- | :----: | :------: | :-------: | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| job_name    | string |   yes    |     —     | Name of the vectorize job to search. This identifies the table, schema, model and other job configuration.                                      |
| query       | string |   yes    |     —     | The user's search query string.                                                                                                                 |
| limit       |  int   |    no    |    10     | Maximum number of results to return.                                                                                                            |
| window_size |  int   |    no    | 5 * limit | Internal window size used by the hybrid search algorithm.                                                                                       |
| rrf_k       | float  |    no    |   60.0    | Reciprocal Rank Fusion parameter used by the hybrid ranking.                                                                                    |
| semantic_wt | float  |    no    |    1.0    | Weight applied to the semantic score.                                                                                                           |
| fts_wt      | float  |    no    |    1.0    | Weight applied to the full-text-search score.                                                                                                   |
| filters     | object |    no    |     —     | Additional filters passed as separate query parameters. The server parses values into typed filter values and validates keys/values for safety. |


### Notes on filters

- **GET**: Filters are supplied as individual URL query parameters (e.g., `product_category=outdoor`, `price=lt.10`).
- **POST**: Filters are supplied as a JSON object in the `filters` field (e.g., `{ "product_category": "outdoor", "price": "lt.10"}`).

The Operator will default to `equal` if one is not provided.
 Therefore, `product_category=outdoor` and `product_category=eq.outdoor` are equivalent.

Supported operators:

| Operator | Full Name |
|----------|-----------|
| `eq` | Equal |
| `gt` | Greater Than |
| `gte` | Greater Than or Equal |
| `lt` | Less Than |
| `lte` | Less Than or Equal |

The server parses and validates filter values according to the job's schema and allowed columns.

### GET /api/v1/search

Example with multiple `filter` values

```bash
curl -G "http://localhost:8080/api/v1/search" \
  --data-urlencode "job_name=my_job" \
  --data-urlencode "query=camping gear" \
  --data-urlencode "limit=2" \
  --data-urlencode "product_category=outdoor" \
  --data-urlencode "price=gt.10"
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
    "rrf_score": 0.015873015873015872,
    "semantic_rank": 3,
    "similarity_score": 0.3863893266436258,
    "updated_at": "2025-11-01T16:30:42.501294+00:00"
  }
]
```

## POST /api/v1/search

Pass parameters as a JSON object in the request body. Example:

```bash
curl -X POST "http://localhost:8080/api/v1/search" \
  -H "Content-Type: application/json" \
  -d '{
    "job_name": "my_job",
    "query": "camping gear",
    "limit": 2,
    "filters": {"product_category": "outdoor", "price": "gt.10"}
  }'
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
    "rrf_score": 0.015873015873015872,
    "semantic_rank": 3,
    "similarity_score": 0.3863893266436258,
    "updated_at": "2025-11-01T16:30:42.501294+00:00"
  }
]
```
