# pg_vectorize

The simplest way to do vector search in Postgres. Vectorize is a Postgres extension that automates that the transformation and orchestration of text to embeddings, allowing you to do vector and semantic search on existing data with as little as two function calls.

One function call to initialize your data. Another function call to search.

[![Static Badge](https://img.shields.io/badge/%40tembo-community?logo=slack&label=slack)](https://join.slack.com/t/tembocommunity/shared_invite/zt-20dtnhcmo-pLNV7_Aobi50TdTLpfQ~EQ)

## Installation

The fastest way to get started is by running the Tembo docker image, where Vectorize and all its dependencies come pre-installed.

```bash
docker run -d --name postgres -e POSTGRES_PASSWORD=postgres -p 5432:5432 quay.io/tembo/vectorize-pg:latest
```

Connect to Postgres

```text
psql postgres://postgres:postgres@0.0.0.0:5432/postgres
```

Enable the extension and its dependencies

```sql
CREATE EXTENSION vectorize CASCADE;
```

If you're installing in an existing Postgres instance, you will need the following depdencies:

Rust:

- [pgrx toolchain](https://github.com/pgcentralfoundation/pgrx)

Postgres Extensions:

- [pg_cron](https://github.com/citusdata/pg_cron) == 1.5
- [pgmq](https://github.com/tembo-io/pgmq) >= 0.30.0
- [pgvector](https://github.com/pgvector/pgvector) >= 1.5.0

And you'll need an OpenAI key:

- [openai API key](https://platform.openai.com/docs/guides/embeddings)

## Example

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

```sql
ALTER SYSTEM SET vectorize.openai_key TO '<your api key>';
```

```sql
SELECT vectorize.table(
    job_name => 'product_search',
    "table" => 'products',
    primary_key => 'product_id',
    columns => ARRAY['product_name', 'description']
);
```

Trigger the job. This will update embeddings for all records which do not have them, or for records where embeddings are out of date. By default, pg_cron will run this job every minute.

```sql
SELECT vectorize.job_execute('product_search');
```

Finally, search.

```sql
SELECT * FROM vectorize.search(
    job_name => 'product_search',
    query => 'accessories for mobile devices',
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
