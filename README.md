<h1 align="center">
 <b>pg_vectorize: Automated Vector Search on Postgres</b>
<br>
   
<br/>
  <a href="https://tembo.io"><img src="https://github.com/tembo-io/pg_vectorize/assets/15756360/34d65cba-065b-485f-84a4-76284e9def19" alt="pg_vectorize" width="368px"></a>

</h1>

<p align="center">
  
</p>

A Postgres extension that automates the transformation and orchestration of text to embeddings, allowing you to do vector and semantic search on existing data with as little as two function calls.

One function call to initialize your data. Another function call to search. Automated management of Postgres triggers and background jobs to keep your embeddings up to date.

---

[![Static Badge](https://img.shields.io/badge/%40tembo-community?logo=slack&label=slack)](https://join.slack.com/t/tembocommunity/shared_invite/zt-277pu7chi-NHtvHWvLhHwyK0Y5Y6vTPw)
[![PGXN version](https://badge.fury.io/pg/vectorize.svg)](https://pgxn.org/dist/vectorize/)

## Features

- Integrations with [OpenAI's embeddings](https://platform.openai.com/docs/guides/embeddings) endpoints and a self-hosted container for running [Hugging Face Sentence-Transformers](https://huggingface.co/sentence-transformers)
- Automated creation of Postgres triggers to keep your embeddings up to date
- High level API - one function to initialize embeddings transformations, and another function to search

## Table of Contents
- [Features](#features)
- [Table of Contents](#table-of-contents)
- [Installation](#installation)
- [API Overview](#api-overview)
  - [`vectorize.table()`](#vectorizetable)
  - [`vectorize.search()`](#vectorizesearch)
  - [`vectorize.transform_embeddings()`](#vectorizetransform_embeddings)
- [Hugging Face Example](#hugging-face-example)
- [OpenAI Example](#openai-example)
- [Trigger based updates](#trigger-based-updates)
- [Try it on Tembo Cloud](#try-it-on-tembo-cloud)

## Installation

The fastest way to get started is by running the Tembo docker container and the vector server with docker compose:

```bash
docker compose up -d
```

Then connect to Postgres:

```text
docker compose exec -it postgres psql
```

Enable the extension and its dependencies

```sql
CREATE EXTENSION vectorize CASCADE;
```

<details>

<summary>Install into an existing Postgres instance</summary>

If you're installing in an existing Postgres instance, you will need the following dependencies:

Rust:

- [pgrx toolchain](https://github.com/pgcentralfoundation/pgrx)

Postgres Extensions:

- [pg_cron](https://github.com/citusdata/pg_cron) ^1.5
- [pgmq](https://github.com/tembo-io/pgmq) ^1
- [pgvector](https://github.com/pgvector/pgvector) ^0.5.0

Then set the following either in postgresql.conf or as a configuration parameter:

```sql
-- requires restart of Postgres
alter system set shared_preload_libraries = 'vectorize,pg_cron';
alter system set cron.database_name = 'postgres'
```

And if you're running the vector-serve container, set the following url as a configuration parameter in Postgres.

```sql
alter system set vectorize.embedding_service_url = 'http://vector-serve:3000/v1/embeddings'

SELECT pg_reload_conf();
```

</details>

## API Overview

pg_vectorize is a high level API over pgvector and provides integrations into orcehstrating the transform of text to embeddings through three functions:

### `vectorize.table()`

Configures a vectorize job which handles transforming existing data into embeddings, and keeping the embeddings updated as new data is inserted or existing rows are updated.

```sql
SELECT vectorize.table(
    job_name => 'my_job',
    "table" => 'my_table',
    primary_key => 'record_id',
    columns => ARRAY['some_text_column'],
    transformer => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);
```

### `vectorize.search()`

An abstraction over a text-to-embedding transformation and pgvector's vector similarity search functionality. Used in conjuction with `vectorize.table()`.

Returns `ARRAY[json]`

```sql
SELECT * FROM vectorize.search(
    job_name => 'my_job',
    query => 'my raw text search query',
    return_columns => ARRAY['record_id', 'some_text_column'],
    num_results => 3
);
```

### `vectorize.transform_embeddings()`

A direct hook to a transformer model of your choice.

Returns `ARRAY[float]` (embeddings)

```sql
select vectorize.transform_embeddings(
    input => 'the quick brown fox jumped over the lazy dogs',
    model_name => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);

{-0.2556323707103729,-0.3213586211204529 ..., -0.0951206386089325}
```

## Hugging Face Example

Setup a products table. Copy from the example data provided by the extension.

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
SELECT vectorize.table(
    job_name => 'product_search_hf',
    "table" => 'products',
    primary_key => 'product_id',
    columns => ARRAY['product_name', 'description'],
    transformer => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);
```

This adds a new column to your table, in our case it is named `product_search_embeddings`, then populates that data with the transformed embeddings from the `product_name` and `description` columns.

Then search,

```sql
SELECT * FROM vectorize.search(
    job_name => 'product_search_hf',
    query => 'accessories for mobile devices',
    return_columns => ARRAY['product_id', 'product_name'],
    num_results => 3
);

                                       search_results                                        
---------------------------------------------------------------------------------------------
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8147814132322894}
 {"product_id": 6, "product_name": "Backpack", "similarity_score": 0.7743061352550308}
 {"product_id": 11, "product_name": "Stylus Pen", "similarity_score": 0.7709902653575383}
```

## OpenAI Example

pg_vectorize also works with using OpenAI's embeddings, but first you'll need an API key.

- [openai API key](https://platform.openai.com/docs/guides/embeddings)

Set your API key as a Postgres configuration parameter.

```sql
ALTER SYSTEM SET vectorize.openai_key TO '<your api key>';

SELECT pg_reload_conf();
```

Create an example table if it does not already exist.

```sql
CREATE TABLE products AS 
SELECT * FROM vectorize.example_products;
```

Then create the job:

```sql
SELECT vectorize.table(
    job_name => 'product_search_openai',
    "table" => 'products',
    primary_key => 'product_id',
    columns => ARRAY['product_name', 'description'],
    transformer => 'text-embedding-ada-002'
);
```

It may take some time to generate embeddings, depending on API latency.

```sql
SELECT * FROM vectorize.search(
    job_name => 'product_search_openai',
    query => 'accessories for mobile devices',
    return_columns => ARRAY['product_id', 'product_name'],
    num_results => 3
);

                                         search_results                                     
    
--------------------------------------------------------------------------------------------
----
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8564681325237845}
 {"product_id": 24, "product_name": "Tablet Holder", "similarity_score": 0.8295988934993099}
 {"product_id": 4, "product_name": "Bluetooth Speaker", "similarity_score": 0.8250355616233103}
(3 rows)
```

## Trigger based updates

When vectorize job is set up as `realtime` (the default behavior, via `vectorize.table(..., schedule => 'realtime')`), vectorize will create triggers on your table that will keep your embeddings up to date. When the text inputs are updated or if new rows are inserted, the triggers handle creating a background job that updates the embeddings. Since the transformation is executed in a background job and the transformer model is invoked in a separate container, there is minimal impact on the performance of the update or insert statement.

```sql
INSERT INTO products (product_id, product_name, description)
VALUES (12345, 'pizza', 'dish of Italian origin consisting of a flattened disk of bread');

UPDATE products
SET description = 'sling made of fabric, rope, or netting, suspended between two or more points, used for swinging, sleeping, or resting'
WHERE product_name = 'Hammock';
```

## Try it on Tembo Cloud

Try it for yourself! Install with a single click on a Vector DB Stack (or any other instance) in [Tembo Cloud](https://cloud.tembo.io/) today.
