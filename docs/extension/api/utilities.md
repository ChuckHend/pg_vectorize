# Utilities

## Text to Embeddings

Transforms a block of text to embeddings using the specified transformer.

Requires the `vector-serve` container to be set via `vectorize.embedding_service_url`, or an OpenAI key to be set if using OpenAI embedding models.

```sql
vectorize."encode"(
    "input" TEXT,
    "model_name" TEXT DEFAULT 'sentence-transformers/all-MiniLM-L6-v2',
    "api_key" TEXT DEFAULT NULL
) RETURNS double precision[]
```

**Parameters:**

| Parameter      | Type | Description     |
| :---        |    :----   |          :--- |
| input | text | Raw text to be transformed to an embedding |
| model_name | text | Name of the sentence-transformer or OpenAI model to use.  |
| api_key | text | API key for the transformer. Defaults to NULL. |

### Example

```sql
select vectorize.encode(
    input       => 'the quick brown fox jumped over the lazy dogs',
    model_name  => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);

{-0.2556323707103729,-0.3213586211204529 ..., -0.0951206386089325}
```

## Updating the Database

Configure `vectorize` to run on a database other than the default `postgres`.

Note that when making this change, it's also required to update `pg_cron` such that its corresponding background workers also connect to the appropriate database.

### Example

```sql
CREATE DATABASE my_new_db;
```

```sql
ALTER SYSTEM SET cron.database_name TO 'my_new_db';
ALTER SYSTEM SET vectorize.database_name TO 'my_new_db';
```

Then, restart postgres to apply the changes and, if you haven't already, enable `vectorize` in your new database.

```sql
\c my_new_db
```

```sql
CREATE EXTENSION vectorize CASCADE;
```

```sql
SHOW cron.database_name;
SHOW vectorize.database_name;
```

```text
 cron.database_name 
--------------------
 my_new_db
(1 row)

 vectorize.database_name 
-------------------------
 my_new_db
(1 row)
```
