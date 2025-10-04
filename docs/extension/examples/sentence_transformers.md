# Sentence Transformers

Setup a products table. Copy from the example data provided by the extension.

Ensure `vectorize.embedding_svc_url` is set to the URL of the vector-serve container.

If you're running this example using the docker-compose.yaml file from this repo, it should look like this:


```sql
SHOW vectorize.embedding_service_url;
```

```text
    vectorize.embedding_service_url     
----------------------------------------
 http://vector-serve:3000/v1/embeddings
(1 row)
```

If you are not running in docker, then you will need to change the url to the appropriate location.
 If that is localhost, it would look like this;

```sql
ALTER SYSTEM SET vectorize.embedding_svc_url TO 'http://localhost:3000/v1/embeddings';
```

Then reload Postgres configurations:

```sql
SELECT pg_reload_conf();
```

Create an example table if it does not already exist.

```sql
CREATE TABLE products (LIKE vectorize.example_products INCLUDING ALL);
INSERT INTO products SELECT * FROM vectorize.example_products;
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

```sql
SELECT vectorize.table(
    job_name    => 'product_search_hf',
    relation    => 'products',
    primary_key => 'product_id',
    columns     => ARRAY['product_name', 'description'],
    transformer => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1',
    scheduler   => 'realtime'
);
```

This adds a new column to your table, in our case it is named `product_search_embeddings`, then populates that data with the transformed embeddings from the `product_name` and `description` columns.

Then search,

```sql
SELECT * FROM vectorize.search(
    job_name        => 'product_search_hf',
    query           => 'accessories for mobile devices',
    return_columns  => ARRAY['product_id', 'product_name'],
    num_results     => 3
);

                                       search_results                                        
---------------------------------------------------------------------------------------------
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8147814132322894}
 {"product_id": 6, "product_name": "Backpack", "similarity_score": 0.7743061352550308}
 {"product_id": 11, "product_name": "Stylus Pen", "similarity_score": 0.7709902653575383}
```
