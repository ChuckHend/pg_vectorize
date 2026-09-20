# Server configuration

`vectorize-server` and `vectorize-worker` are configured with environment variables. This page covers the ones that decide where Postgres is and which embedding provider is used. For sizing, health checks and deployment, see [Production](../production.md).

## Postgres

| variable | default | |
|---|---|---|
| `DATABASE_URL` | `postgres://postgres:postgres@localhost:5432/postgres` | Any Postgres with [pgvector](https://github.com/pgvector/pgvector), including a managed one (RDS, Neon, and others). |

## Embedding providers

The provider for a job is chosen by the prefix of its `model`: `openai/text-embedding-3-small` uses OpenAI, `voyage/voyage-3-lite` uses Voyage, and so on. The server reads that provider's credentials from its own environment. There is no per-request or per-job API key, and the extension's `vectorize.*` settings do not apply to the server.

| model prefix | variables | notes |
|---|---|---|
| `openai/` | `OPENAI_API_KEY` (required), `OPENAI_BASE_URL` | `OPENAI_BASE_URL` points at an OpenAI-compatible gateway or proxy. It is the base, ending in `/v1`; the server appends `/embeddings`. Unset or empty means `https://api.openai.com/v1`. |
| `voyage/` | `VOYAGE_API_KEY` | |
| `cohere/` | `CO_API_KEY` | |
| `portkey/` | `PORTKEY_API_KEY`, `PORTKEY_VIRTUAL_KEY` | Both are required. |
| `ollama/` | | Uses `http://localhost:3001`. It cannot be changed through the environment. |
| anything else | `EMBEDDING_SVC_URL`, `EMBEDDING_SVC_API_KEY` (optional) | Self-hosted: [TEI](https://github.com/huggingface/text-embeddings-inference) or `vector-serve`. `EMBEDDING_SVC_URL` is the base, ending in `/v1`. `sentence-transformers/...` and Hugging Face names such as `BAAI/bge-base-en-v1.5` land here. |

Things to know:

- **Set the provider variables on both `vectorize-server` and `vectorize-worker`.** The worker embeds your rows. The server embeds each search query, and it also embeds a sample input when a job is created, to learn the model's vector dimension. A key set on only one of them fails on the other, at job creation or at search time.
- **Creating a job without the credentials returns a 500 that names the fix.** `POST /api/v1/table` (and search) on a server missing the key answers `500` with, for example, `{"error": "embedding provider 'openai' is not configured: OPENAI_API_KEY is not set"}`. Nothing is created. The server cannot see the worker's environment, so this does not catch a key missing only on the worker.
- **A worker without the credentials drops jobs.** If the worker cannot build the provider (say `OPENAI_API_KEY` is set on the server only), the job fails, is retried after its 300 s visibility timeout, and after `MAX_RETRIES` (default 2) retries the queue message is deleted without the rows being embedded. The worker's health endpoint turns 503 with the error while this happens; check its logs.
- **An unrecognized prefix does not fail.** `opnai/text-embedding-3-small` is sent to `EMBEDDING_SVC_URL`.
- **A missing key is an error, not an empty value.** In Docker Compose, `OPENAI_API_KEY: ${OPENAI_API_KEY}` passes an empty string when the variable is unset, and the provider then rejects the request with a 401. Leave the variable out, or use the key-only form (`OPENAI_API_KEY:`), so it is passed only when set. `deploy/docker-compose.yml` does this.
- **The vector dimension comes from the model.** For `openai/` on `api.openai.com` it comes from a built-in table of OpenAI's models. With a custom `OPENAI_BASE_URL`, and for every other provider, the server embeds a sample input and reads the length of the result, so the model must be reachable when a job is created.

## Worker

| variable | default | |
|---|---|---|
| `VECTORIZE_WORKER_ENABLED` | `true` | Read by `vectorize-server`. `false` stops it starting its own worker, for when the `vectorize-worker` binary runs as a separate service. See [Production](../production.md#the-background-worker). |
| `WORKER_HEALTH_PORT` | `8081` | Port of the standalone `vectorize-worker`'s `GET /health`. |

## Timeouts

| variable | default | |
|---|---|---|
| `EMBEDDING_REQUEST_TIMEOUT` | `120` | Seconds allowed for each HTTP request to an embedding provider. |
