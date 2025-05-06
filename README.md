<h1 align="center">
 <b>pg_vectorize: a VectorDB for Postgres</b>
</h1>

A Postgres extension that automates the transformation and orchestration of text to embeddings and provides hooks into the most popular LLMs. This allows you to do vector search and build LLM applications on existing data with as little as two function calls.

This project relies heavily on the work by [pgvector](https://github.com/pgvector/pgvector) for vector similarity search, [pgmq](https://github.com/pgmq/pgmq) for orchestration in background workers, and [SentenceTransformers](https://huggingface.co/sentence-transformers).

---

[![PGXN version](https://badge.fury.io/pg/vectorize.svg)](https://pgxn.org/dist/vectorize/)
[![OSSRank](https://shields.io/endpoint?url=https://ossrank.com/shield/3815)](https://ossrank.com/p/3815)


**API Documentation**: https://chuckhend.github.io/pg_vectorize/

**Source**: https://github.com/ChuckHend/pg_vectorize

## Features

- Workflows for both vector search and RAG
- Integrations with OpenAI's [embeddings](https://platform.openai.com/docs/guides/embeddings) and [Text-Generation](https://platform.openai.com/docs/guides/text-generation) endpoints and a self-hosted container for running [Hugging Face Sentence-Transformers](https://huggingface.co/sentence-transformers)
- Automated creation of Postgres triggers to keep your embeddings up to date
- High level API - one function to initialize embeddings transformations, and another function to search

## Table of Contents
- [Features](#features)
- [Table of Contents](#table-of-contents)
- [Installation](#installation)
- [Vector Search Example](#vector-search-example)
- [RAG Example](#rag-example)
- [Updating Embeddings](#updating-embeddings)
- [Directly Interact with LLMs](#directly-interact-with-llms)
- [Importing Pre-existing Embeddings](#importing-pre-existing-embeddings)
- [Creating a Table from Existing Embeddings](#creating-a-table-from-existing-embeddings)

## Installation

The fastest way to get started is by using [docker compose](https://docs.docker.com/compose/).

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
- [pgmq](https://github.com/pgmq/pgmq) ^1
- [pgvector](https://github.com/pgvector/pgvector) ^0.5.0

Then set the following either in postgresql.conf or as a configuration parameter:

```sql
-- requires restart of Postgres
alter system set shared_preload_libraries = 'vectorize,pg_cron';
alter system set cron.database_name = 'postgres';
```

And if you're running the vector-serve container, set the following url as a configuration parameter in Postgres.
 The host may need to change from `localhost` to something else depending on where you are running the container.

```sql
alter system set vectorize.embedding_service_url = 'http://localhost:3000/v1';

SELECT pg_reload_conf();
```

</details>

## Vector Search Example

Text-to-embedding transformation can be done with either Hugging Face's Sentence-Transformers or OpenAI's embeddings. The following examples use Hugging Face's Sentence-Transformers. See the project [documentation](https://chuckhend.github.io/pg_vectorize/examples/openai_embeddings/) for OpenAI examples.

Follow the [installation](#installation) steps if you haven't already.

Setup a products table. Copy from the example data provided by the extension.

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
    transformer => 'sentence-transformers/all-MiniLM-L6-v2',
    schedule    => 'realtime'
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
```

```text
                                       search_results                                        
---------------------------------------------------------------------------------------------
 {"product_id": 13, "product_name": "Phone Charger", "similarity_score": 0.8147814132322894}
 {"product_id": 6, "product_name": "Backpack", "similarity_score": 0.7743061352550308}
 {"product_id": 11, "product_name": "Stylus Pen", "similarity_score": 0.7709902653575383}
```

## RAG Example

Ask raw text questions of the example  `products` dataset and get chat responses from an OpenAI LLM.

Follow the [installation](#installation) steps if you haven't already.

Set the [OpenAI API key](https://platform.openai.com/docs/guides/embeddings), this is required to for use with OpenAI's chat-completion models.

```sql
ALTER SYSTEM SET vectorize.openai_key TO '<your api key>';
SELECT pg_reload_conf();
```

Create an example table if it does not already exist.

```sql
CREATE TABLE products (LIKE vectorize.example_products INCLUDING ALL);
INSERT INTO products SELECT * FROM vectorize.example_products;
```

Initialize a table for RAG. We'll use an open source Sentence Transformer to generate embeddings.

Create a new column that we want to use as the context. In this case, we'll concatenate both `product_name` and `description`.

```sql
ALTER TABLE products
ADD COLUMN context TEXT GENERATED ALWAYS AS (product_name || ': ' || description) STORED;
```

Initialize the RAG project.
 We'll use the `openai/text-embedding-3-small` model to generate embeddings on our source documents.

```sql
SELECT vectorize.table(
    job_name    => 'product_chat',
    relation    => 'products',
    primary_key => 'product_id',
    columns     => ARRAY['context'],
    transformer => 'openai/text-embedding-3-small',
    schedule    => 'realtime'
);
```

Now we can ask questions of the `products` table and get responses from the `product_chat` agent using the `openai/gpt-3.5-turbo` generative model.

```sql
SELECT vectorize.rag(
    job_name    => 'product_chat',
    query       => 'What is a pencil?',
    chat_model  => 'openai/gpt-3.5-turbo'
) -> 'chat_response';
```

```text
"A pencil is an item that is commonly used for writing and is known to be most effective on paper."
```

And to use a locally hosted Ollama service, change the `chat_model` parameter:

```sql
SELECT vectorize.rag(
    job_name    => 'product_chat',
    query       => 'What is a pencil?',
    chat_model  => 'ollama/wizardlm2:7b'
) -> 'chat_response';
```

```text
" A pencil is a writing instrument that consists of a solid or gelignola wood core, known as the \"lead,\" encased in a cylindrical piece of breakable material (traditionally wood or plastic), which serves as the body of the pencil. The tip of the body is tapered to a point for writing, and it can mark paper with the imprint of the lead. When used on a sheet of paper, the combination of the pencil's lead and the paper creates a visible mark that is distinct from unmarked areas of the paper. Pencils are particularly well-suited for writing on paper, as they allow for precise control over the marks made."
```


:bulb: Note that the `-> 'chat_response'` addition selects for that field of the JSON object output. Removing it will show the full JSON object, including information on which documents were included in the contextual prompt.

## Updating Embeddings

When the source text data is updated, how and when the embeddings are updated is determined by the value set to the `schedule` parameter in `vectorize.table`.

The default behavior is `schedule => '* * * * *'`, which means the background worker process checks for changes every minute, and updates the embeddings accordingly. This method requires setting the `updated_at_col` value to point to a colum on the table indicating the time that the input text columns were last changed. `schedule` can be set to any cron-like value.

Alternatively, `schedule => 'realtime` creates triggers on the source table and updates embeddings anytime new records are inserted to the source table or existing records are updated.

Statements below would will result in new embeddings being generated either immediately (`schedule => 'realtime'`) or within the cron schedule set in the `schedule` parameter.

```sql
INSERT INTO products (product_id, product_name, description, product_category, price)
VALUES (12345, 'pizza', 'dish of Italian origin consisting of a flattened disk of bread', 'food', 5.99);

UPDATE products
SET description = 'sling made of fabric, rope, or netting, suspended between two or more points, used for swinging, sleeping, or resting'
WHERE product_name = 'Hammock';
```

## Directly Interact with LLMs

Sometimes you want more control over the handling of embeddings.
 For those situations you can directly call various LLM providers using SQL:

For text generation:

```sql
select vectorize.generate(
  input => 'Tell me the difference between a cat and a dog in 1 sentence',
  model => 'openai/gpt-4o'
);
```

```text
                                                 generate                                                  
-----------------------------------------------------------------------------------------------------------
 Cats are generally more independent and solitary, while dogs tend to be more social and loyal companions.
(1 row)
```

And for embedding generation:

```sql
select vectorize.encode(
  input => 'Tell me the difference between a cat and a dog in 1 sentence',
  model => 'openai/text-embedding-3-large'
);
```

```text
{0.0028769304,-0.005826319,-0.0035932811, ...}
```

## Importing Pre-existing Embeddings

If you have already computed embeddings using a compatible model (e.g., using Sentence-Transformers directly), you can import these into pg_vectorize without recomputation:

```sql
-- First create the vectorize project
SELECT vectorize.table(
    job_name    => 'my_search',
    relation    => 'my_table',
    primary_key => 'id',
    columns     => ARRAY['content'],
    transformer => 'sentence-transformers/all-MiniLM-L6-v2'
);

-- Then import your pre-computed embeddings
SELECT vectorize.import_embeddings(
    job_name            => 'my_search',
    src_table           => 'my_embeddings_table',
    src_primary_key     => 'id',
    src_embeddings_col  => 'embedding'
);
```

The embeddings must match the dimensions of the specified transformer model. For example, 'sentence-transformers/all-MiniLM-L6-v2' expects 384-dimensional vectors.

## Creating a Table from Existing Embeddings

If you have already computed embeddings using a compatible model, you can create a new vectorize table directly from them:

```sql
-- Create a vectorize table from existing embeddings
SELECT vectorize.table_from(
    relation => 'my_table',
    columns => ARRAY['content'],
    job_name => 'my_search',
    primary_key => 'id',
    src_table => 'my_embeddings_table',
    src_primary_key => 'id',
    src_embeddings_col => 'embedding',
    transformer => 'sentence-transformers/all-MiniLM-L6-v2'
);
```

The embeddings must match the dimensions of the specified transformer model. This approach ensures your pre-computed embeddings are properly imported before any automatic updates are enabled.

## Contributing

We welcome contributions from the community! If you're interested in contributing to `pg_vectorize`, please check out our [Contributing Guide](CONTRIBUTING.md). You can also open an issue.
