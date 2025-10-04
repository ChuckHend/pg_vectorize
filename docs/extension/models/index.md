# Supported Transformers and Generative Models

pg_vectorize provides hooks into two types of models; `text-to-embedding` transformer models and `text-generation` models.
 Whether a model is a text-to-embedding transformer or a text generation model, the models are always referenced from SQL using the following syntax:

`${provider}/${model-name}`

A few illustrative examples:

- `openai/text-embedding-ada-002` is one of OpenAI's earliest [embedding](https://platform.openai.com/docs/models/embeddings) models
- `openai/gpt-3.5-turbo-instruct` is a [text generation](https://platform.openai.com/docs/models/gpt-3-5-turbo) model from OpenAI.
- `ollama/wizardlm2:7b` is a language model hosted in [Ollama](https://ollama.com/library/wizardlm2:7b) and developed by MicrosoftAI.
- `sentence-transformers/all-MiniLM-L12-v2` is a text-to-embedding model from [SentenceTransformers](https://huggingface.co/sentence-transformers/all-MiniLM-L12-v2).

## Text-to-Embedding Models

pg_vectorize provides hooks into the following tex-to-embedding models:

- OpenAI (public API)
- SentenceTransformers (self-hosted)

The transformer model that you want to be used is specified in a parameter in various functions in this project,

For example, the `sentence-transformers` provider has a model named `all-MiniLM-L12-v2`.
 The model name is `sentence-transformers/all-MiniLM-L12-v2`. To use openai's `text-embedding-ada-002`,
 the model name is `openai/text-embedding-ada-002`.

### SentenceTransformers

[SentenceTransformers](https://sbert.net/) is a Python library for computing text embeddings.
 pg_vectorize provides a container image that implements the SentenceTransformer library beyind a REST API.
 The container image is pre-built with `sentence-transformers/all-MiniLM-L12-v2` pre-cached.
 Models that are not pre-cached will be downloaded on first use and cached for subsequent use.

When calling the model server from Postgres, the url to the model server must first be set in the `vectorize.embedding_service_url` configuration parameter.
 Assuming the model server is running on the same host as Postgres, you would set the following:

```sql
ALTER SYSTEM SET vectorize.embedding_service_url TO 'http://localhost:3000/v1/embeddings';
SELECT pg_reload_conf();
```

#### Running the model server

You can run this model server locally by executing

```bash
docker compose up vector-serve -d
```

Then call it with simple curl commands:

#### Calling with curl

```bash
curl -X POST http://localhost:3000/v1/embeddings \
  -H 'Content-Type: application/json' \
  -d '{"input": ["solar powered mobile electronics accessories without screens"],
   "model": "sentence-transformers/all-MiniLM-L12-v2"}'
```

```plaintext
{
  "data": [
    {
      "embedding": [
        -0.07903402298688889,
        0.028912536799907684,
        -0.018827738240361214,
        -0.013423092663288116,
        -0.06503172218799591,
          ....384 total elements
      ],
      "index": 0
    }
  ],
  "model": "all-MiniLM-L12-v2"
}
```

We can change the model name to any of the models supported by SentenceTransformers, and it will be downloaded on-the-fly.

```bash
curl -X POST http://localhost:3000/v1/embeddings \
  -H 'Content-Type: application/json' \
  -d '{"input": ["solar powered mobile electronics accessories without screens"],
   "model": "sentence-transformers/sentence-t5-base"}'
```

```plaintext
{
  "data": [
    {
      "embedding": [
        -0.07903402298688889,
        0.028912536799907684,
        -0.018827738240361214,
        -0.013423092663288116,
        -0.06503172218799591,
          ....384 total elements
      ],
      "index": 0
    }
  ],
  "model": "sentence-transformers/sentence-t5-base"
}
```

#### Calling with SQL

We can also call the model server from SQL using the `pg_vectorize.transform_embeddings` function.

Model name support rules apply the same.

```sql
select vectorize.transform_embeddings(
    input       => 'the quick brown fox jumped over the lazy dogs',
    model_name  => 'sentence-transformers/multi-qa-MiniLM-L6-dot-v1'
);
```

```plaintext
{-0.2556323707103729,-0.3213586211204529 ..., -0.0951206386089325}
```

### OpenAI

OpenAI embedding models are hosted by OpenAI's public API.
 You just need to have an API key of your own, and it can be set with:

```sql
ALTER SYSTEM SET vectorize.openai_key TO '<your api key>';

SELECT pg_reload_conf();
```

To call the `text-embedding-ada-002` from OpenAI:

```sql
select vectorize.transform_embeddings(
    input       => 'the quick brown fox jumped over the lazy dogs',
    model_name  => 'openai/text-embedding-ada-002'
);
```

To call `text-embedding-3-large`

```sql
select vectorize.transform_embeddings(
    input => 'the quick brown fox jumped over the lazy dogs',
    model_name => 'openai/text-embedding-3-large'
);
```

## Text Generation Models

pg_vectorize provides hooks into the following text generation models:

- OpenAI (public API)
- Ollama (self-hosted)

### Ollama Generative Models

To run the self-hosted Ollama models, you must first start the model server:

```bash
docker compose up ollama-serve -d
```

This starts an Ollama server pre-loaded with the `wizardlm2:7b` model.

#### Calling with `curl`

Once the Ollama server is running, you can call it directly with `curl`:

```bash
curl http://localhost:3001/api/generate -d '{
  "model": "wizardlm2:7b",
  "prompt": "What is Postgres?"
}'
```

#### Calling with SQL

First set the url to the Ollama server:

```sql
ALTER SYSTEM set vectorize.ollama_service_url TO 'http://localhost:3001`;
SELECT pg_reload_conf();
```

The text-generation models are available as part of the [RAG](../api/rag.md) API.
 To call the models provided by the self-hosted Ollama container,
 pass the model name into the `chat_model` parameter.

```sql
SELECT vectorize.rag(
    job_name    => 'product_chat',
    query       => 'What is a pencil?',
    chat_model  => 'ollama/wizardlm2:7b'
);
```

#### Loading new Ollama models

While Ollama server comes preloaded with `wizardlm2:7b`, we can load and model supported by Ollama by calling the `/api/pull` endpoint.
 The service is compatible with all models available in the [Ollama library](https://ollama.com/library).

To pull Llama 3:

```bash
curl http://localhost:3001/api/pull -d '{
  "name": "llama3"
}'
```

Then use that model in your RAG application:

```sql
SELECT vectorize.rag(
    job_name    => 'product_chat',
    query       => 'What is a pencil?'
    chat_model  => 'ollama/llama3'
);
```
