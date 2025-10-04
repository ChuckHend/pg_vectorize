# Configuring pg_vectorize

## Changing the database

To change the database that pg_vectorize background worker is connected to, you can use the following SQL command:

```sql
ALTER SYSTEM SET vectorize.database_name TO 'my_new_db';
```

Then, restart Postgres.

## Changing Embedding and LLM base URLs

All Embedding model and LLM providers can have their base URLs changed.

For example, if you have an OpenAI compliant embedding or LLM server (such as [vLLM](https://github.com/vllm-project/vllm)), running at `https://api.myserver.com/v1`, you can change the base URL with the following SQL command:

```sql
ALTER SYSTEM SET vectorize.openai_service_url TO 'https://api.myserver.com/v1';
SELECT pg_reload_conf();
```

## Changing the batch job size

Text data stored in Postgres is transformed into embeddings via HTTP requests made from the pg_vectorize background worker. Requests are made to the specified embedding service in batch (multiple inputs per request). The number of inputs per request is determined by the `vectorize.batch_size` GUC. This has no impact on transformations that occur during `vectorize.search()`, `vectorize.encode()` and `vectorize.rag()` which are always batch size 1 since those APIs accept only a single input (the raw text query).

```sql
ALTER SYSTEM SET vectorize.batch_size to 100;
```

## Available GUCs

The complete list of GUCs available for pg_vectorize are defined in [extension/src/guc.rs](https://github.com/tembo-io/pg_vectorize/blob/638b12887f14d47de0793b16d535b226d8f371b9/extension/src/guc.rs#L33).
