# Vector Search

The vector-search flow is two part; first initialize a table using `vectorize.table()`, then search the table with `vectorize.search()`.

## Initialize a table

Initialize a table for vector search. Generates embeddings and index. Creates triggers to keep embeddings up-to-date.

```sql
vectorize."table"(
    "relation" TEXT,
    "columns" TEXT[],
    "job_name" TEXT,
    "primary_key" TEXT,
    "schema" TEXT DEFAULT 'public',
    "update_col" TEXT DEFAULT 'last_updated_at',
    "transformer" TEXT DEFAULT 'sentence-transformers/all-MiniLM-L6-v2',
    "index_dist_type" vectorize.IndexDist DEFAULT 'pgv_hnsw_cosine',
    "table_method" vectorize.TableMethod DEFAULT 'join',
    "schedule" TEXT DEFAULT '* * * * *'
) RETURNS TEXT
```

| Parameter      | Type | Description     |
| :---        |    :----   |          :--- |
| relation | text | The name of the table to be initialized. |
| columns | text | The name of the columns that contains the content that is used for context for RAG. Multiple columns are concatenated. |
| job_name | text | A unique name for the project. |
| primary_key | text | The name of the column that contains the unique record id. |
| args | json | Additional arguments for the transformer. Defaults to '{}'. |
| schema | text | The name of the schema where the table is located. Defaults to 'public'. |
| update_col | text | Column specifying the last time the record was updated. Required for cron-like schedule. Defaults to `last_updated_at` |
| transformer | text | The name of the transformer to use for the embeddings. Defaults to 'text-embedding-ada-002'. |
| index_dist_type | IndexDist | The name of index type to build. Defaults to 'pgv_hnsw_cosine'. |
| table_method | TableMethod | `join` to store embeddings in a new table in the vectorize schema. `append` to create columns for embeddings on the source table. Defaults to `join`. |
| schedule | text | Accepts a cron-like input for a cron based updates. Or `realtime` to set up a trigger. |

### Sentence-Transformer Examples

### OpenAI Examples

To use embedding model provided by OpenAI's public embedding endpoints, provide the model name into the `transformer` parameter,
 and provide the OpenAI API key.

Pass the API key into the function call via `args`.

```sql
select vectorize.table(
    job_name    => 'product_search',
    relation    => 'products',
    primary_key => 'product_id',
    columns     => ARRAY['product_name', 'description'],
    transformer =>  'openai/text-embedding-ada-002',
    args        => '{"api_key": "my-openai-key"}'
);
```

The API key can also be set via GUC.

```sql
ALTER SYSTEM SET vectorize.openai_key TO 'my-openai-key';
SELECT pg_reload_conf();
```

Then call `vectorize.table()` without providing the API key.

```sql
select vectorize.table(
    job_name    => 'product_search',
    relation    => 'products',
    primary_key => 'product_id',
    columns     => ARRAY['product_name', 'description'],
    transformer =>  'openai/text-embedding-ada-002'
);
```

## Search a table

Search a table initialized with `vectorize.table`. The search results are sorted in descending order according to similarity. 

The `query` is transformed to embeddings using the same `transformer` configured during `vectorize.table`.

The `where_sql` parameter is used to apply additional filtering to the search results based on SQL conditions. 

```sql
vectorize."search"(
    "job_name" TEXT,
    "query" TEXT,
    "api_key" TEXT DEFAULT NULL,
    "return_columns" TEXT[] DEFAULT ARRAY['*']::text[],
    "num_results" INT DEFAULT 10
    "where_sql" TEXT DEFAULT NULL
) RETURNS TABLE (
    "search_results" jsonb
)
```

**Parameters:**

| Parameter      | Type | Description     |
| :---        |    :----   |          :--- |
| job_name | text | A unique name for the project. |
| query | text | The user provided query or command provided to the chat completion model. |
| api_key | text | API key for the specified chat model. If OpenAI, this value overrides the config `vectorize.openai_key` |
| return_columns | text[] | The columns to return in the search results. Defaults to all columns. |
| num_results | int | The number of results to return. Sorted in descending order according to similarity. Defaults to 10. |
| where_sql | text | An optional SQL condition to filter the search results. This condition is applied after the similarity search. |

### Example

```sql
SELECT * FROM vectorize.search(
    job_name        => 'product_search',
    query           => 'mobile electronic devices',
    return_columns  => ARRAY['product_id', 'product_name'],
    num_results     => 3
);
```

```text
                                         search_results                                     
    
--------------------------------------------------------------------------------------------
----
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8564681325237845}
 {"product_id": 24, "product_name": "Tablet Holder", "similarity_score": 0.8295988934993099}
 {"product_id": 4, "product_name": "Bluetooth Speaker", "similarity_score": 0.8250355616233103}
(3 rows)
```

## Filtering Search Results

The `where_sql` parameter allows to apply SQL-based filtering after performing the vector similarity search. This feature is useful when you want to narrow down the search results based on certain conditions such as `product category` or `price`.

### Example

```sql
SELECT * FROM vectorize.search(
    job_name        => 'product_search',
    query           => 'mobile electronic devices',
    return_columns  => ARRAY['product_id', 'product_name'],
    num_results     => 3,
    where_sql       => 'product_category = ''electronics'' AND price > 100'
);
```

In the above example, the results are filtered where the `product_category` is `electronics` and the `price` is greater than 100.

## Optimizing Searches with Partial Indices

For improving performance when using filters, you can create partial indices. This will speed up the execution of queries with frequent conditions in the `where_sql` parameter.

### Example

```sql
CREATE INDEX idx_product_price ON products (product_name) WHERE price > 100;
```

This index optimizes queries that search for products where the `price` is greater than 100.

> **Note:** Partial indices improve performance by only indexing rows that meet the specified condition. This reduces the amount of data the database needs to scan, making queries with the same filter more efficient since only relevant rows are included in the index.

By combining the `where_sql` filtering feature with partial indices, you can efficiently narrow down search results and improve query performance.
