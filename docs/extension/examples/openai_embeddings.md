# Vector Search with OpenAI

First you'll need an [OpenAI API key](https://platform.openai.com/docs/guides/embeddings).

Set your API key as a Postgres configuration parameter.

```sql
ALTER SYSTEM SET vectorize.openai_key TO '<your api key>';

SELECT pg_reload_conf();
```

Create an example table if it does not already exist.

```sql
CREATE TABLE products (LIKE vectorize.example_products INCLUDING ALL);
INSERT INTO products SELECT * FROM vectorize.example_products;
```

Then create the job.
 It may take some time to generate embeddings, depending on API latency.

```sql
SELECT vectorize.table(
    job_name    => 'product_search_openai',
    relation    => 'products',
    primary_key => 'product_id',
    columns     => ARRAY['product_name', 'description'],
    transformer => 'openai/text-embedding-ada-002'
);
```

To search the table, use the `vectorize.search` function.

```sql
SELECT * FROM vectorize.search(
    job_name        => 'product_search_openai',
    query           => 'accessories for mobile devices',
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
