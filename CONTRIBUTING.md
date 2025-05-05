# Contributing to pg_vectorize

If you encounter any issues or have questions, feel free to join the [Tembo Community Slack](https://join.slack.com/t/tembocommunity/shared_invite/zt-2u3ctm86u-XzcyL76T7o~7Mpnt6KUx1g) for support.

## Prerequisites

- [Rust](https://www.rust-lang.org/learn/get-started) - Toolchain including `rustc`, `cargo`, and `rustfmt`
- [PGRX](https://github.com/pgcentralfoundation/pgrx) - Rust-based PostgreSQL extension development framework
- [Docker Engine](https://docs.docker.com/engine/install/) - For running local containers
- [psql](https://www.postgresql.org/docs/current/app-psql.html) - Terminal-based front-end to PostgreSQL
- [pgmq](https://github.com/tembo-io/pgmq) - PostgreSQL extension for message queues
- [pg_cron](https://github.com/citusdata/pg_cron) - PostgreSQL extension for cron-based job scheduling
- [pgvector](https://github.com/pgvector/pgvector) - PostgreSQL extension for vector similarity search

## Building from source

This process is more involved, but can easily be distilled down into a handful of steps.

### 1. Set up pgrx

```bash
cargo pgrx init
```

### 2. Set up Docker container

```bash
docker run -d -p 3000:3000 ghcr.io/chuckhend/vector-serve:latest
```

Confirm a successful set up by running the following:

```bash
docker ps
```

### 3. Clone and compile `pg_vectorize` and extension dependencies

#### 3.1. Clone and enter directory

```bash
git clone https://github.com/tembo-io/pg_vectorize.git

cd pg_vectorize/extension
```

#### 3.2. Install dependencies

From within the pg_vectorize/extension directory, run the following, which will install `pg_cron`, `pgmq`, and `pgvector`:

```bash
make setup
```

#### 3.3. Compile and run `pg_vectorize`

```bash
make run
```

### 4. Confirm successful build

#### 4.1. Check extension presence

Once the above command is run, you will be brought into Postgres via `psql`.

Run the following command inside the `psql` console to enable the extensions:

```sql
create extension vectorize cascade
```

To list out the enabled extensions, run:

```sql
\dx
```
```text
                                      List of installed extensions
    Name    | Version |   Schema   |                             Description
------------+---------+------------+---------------------------------------------------------------------
 pg_cron    | 1.6     | pg_catalog | Job scheduler for PostgreSQL
 pgmq       | 1.1.1   | pgmq       | A lightweight message queue. Like AWS SQS and RSMQ but on Postgres.
 plpgsql    | 1.0     | pg_catalog | PL/pgSQL procedural language
 vector     | 0.6.0   | public     | vector data type and ivfflat and hnsw access methods
 vectorize  | 0.19.0  | vectorize  | The simplest way to do vector search on Postgres
(6 rows)
```

#### 4.2 Confirm embedding service url is set to localhost

Run the following SHOW command to confirm that the url is set to `localhost`:

```sql
SHOW vectorize.embedding_service_url;
```
```text
   vectorize.embedding_service_url
-------------------------------------
 http://localhost:3000/v1
(1 row)
```

#### 4.3. Load example data

The following can be found within the this project's README, under [Vector Search Example](https://github.com/tembo-io/pg_vectorize/blob/main/README.md#vector-search-example).

Begin by creating a `products` table with the dataset that comes included with `pg_vectorize`.

```sql
CREATE TABLE products (LIKE vectorize.example_products INCLUDING ALL);
INSERT INTO products SELECT * FROM vectorize.example_products;
```

You can then confirm everything is correct by running the following:

```sql
SELECT * FROM products limit 2;
```

```text
 product_id | product_name |                      description                       |        last_updated_at        
------------+--------------+--------------------------------------------------------+-------------------------------
          1 | Pencil       | Utensil used for writing and often works best on paper | 2023-07-26 17:20:43.639351-05
          2 | Laptop Stand | Elevated platform for laptops, enhancing ergonomics    | 2023-07-26 17:20:43.639351-05
```

#### 4.4. Sample queries

```sql
SELECT vectorize.table(
job_name => 'product_search_hf',
relation => 'products',
primary_key => 'product_id',
columns => ARRAY['product_name', 'description'],
transformer => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);
```

```text
                    table
---------------------------------------------
 Successfully created job: product_search_hf
(1 row)
```

```sql
SELECT * FROM vectorize.search(
job_name => 'product_search_hf',
query => 'accessories for mobile devices',
return_columns => ARRAY['product_id', 'product_name'],
num_results => 3
);
```
```text
                                       search_results
---------------------------------------------------------------------------------------------
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8147812194590133}
 {"product_id": 6, "product_name": "Backpack", "similarity_score": 0.774306211384604}
 {"product_id": 11, "product_name": "Stylus Pen", "similarity_score": 0.7709903789778251}
(3 rows)
```

### 5. Local URL

Once all of the following is complete, you should be able to access Swagger UI for `Tembo-Embedding-Service` at [http://localhost:3000/docs](http://localhost:3000/docs) and explore.
This is a platform that allows, for example, the input of [different sentence-transformers models](https://huggingface.co/models?sort=trending&search=sentence-transformers) from Hugging Face.

## TroubleShooting

To check `pgrx` logs for debugging:

```bash
cat ~/.pgrx/17.log
```

# Releases

`pg_vectorize` releases are automated through a [Github workflow](https://github.com/tembo-io/pg_vectorize/blob/main/.github/workflows/extension_ci.yml).
The compiled binaries are publish to and hosted at [pgt.dev](https://pgt.dev).
To create a release, create a new tag follow a valid [semver](https://semver.org/), then create a release with the same name.
Auto-generate the release notes and/or add more relevant details as needed.

