# pg_vectorize

*under development*
 
The simplest implementation of LLM-backed vector search on Postgres.

Dependencies:
- [pgrx toolchain](https://github.com/pgcentralfoundation/pgrx)
- [pg_cron](https://github.com/citusdata/pg_cron)
- [pgmq](https://github.com/tembo-io/pgmq)
- [pgvector](https://github.com/pgvector/pgvector)
- [openai API key](https://platform.openai.com/docs/guides/embeddings)

# Example

Setup a products table. Copy from example data from the extension.

```sql
CREATE TABLE products AS 
SELECT * FROM vectorize.example_products;
```

```sql
SELECT * FROM products limit 2;
```

```text
 product_id | product_name |                      description                       |        last_updated_at        
------------+--------------+--------------------------------------------------------+-------------------------------
          1 | Pencil       | Utensil used for writing and often works best on paper | 2023-07-26 17:20:43.639351-05
          2 | Laptop Stand | Elevated platform for laptops, enhancing ergonomics    | 2023-07-26 17:20:43.639351-05
```

Create a job to vectorize the products table. We'll specify the tables primary key (product_id) and the columns that we want to search (product_name and description).

Provide the OpenAI API key for the job.

```sql
SELECT vectorize.table(
    job_name => 'product_search',
    "table" => 'products',
    primary_key => 'product_id',
    columns => ARRAY['product_name', 'description'],
    args => '{"api_key": "my-openai-key"}'
);
```

Trigger the job. This will update embeddings for all records which do not have them, or for records whos embeddings are out of date. By default, pg_cron will run this job every minute.

```sql
SELECT vectorize.job_execute('product_search');
```


Finally, search.

```sql
SELECT * FROM vectorize.search(
    job_name => 'product_search',
    query => 'accessories for mobile devices',
    api_key => 'my-openai-key',
    return_columns => ARRAY['product_id', 'product_name'],
    num_results => 3
);
```

```text
                                         search_results                                         
------------------------------------------------------------------------------------------------
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8564774308489237}
 {"product_id": 24, "product_name": "Tablet Holder", "similarity_score": 0.8295404213393001}
 {"product_id": 4, "product_name": "Bluetooth Speaker", "similarity_score": 0.8248579643539758}
```
