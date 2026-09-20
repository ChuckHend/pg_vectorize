# Scaling for production

A ready-to-run production-oriented stack lives in `deploy/` in the repository. It always runs the pg_vectorize server (the API) and a separate embedding worker. Postgres and a self-hosted embedding model are optional [compose profiles](https://docs.docker.com/compose/how-tos/profiles/), because most production deployments use a managed Postgres and a hosted embedding provider instead. Only the API port is published, and only on loopback. The rest of this page explains the choices in that file and what it leaves out.

## Where Postgres and embeddings run

The stack does not assume where either lives. Each is one of two choices, and `.env` selects it.

**Postgres** is either your own (RDS, Neon, or any Postgres with [pgvector](https://github.com/pgvector/pgvector)) or the bundled container:

| | set in `.env` |
|---|---|
| Your own | `DATABASE_URL=postgresql://user:password@host:5432/postgres` |
| Bundled | `COMPOSE_PROFILES=postgres` and `POSTGRES_PASSWORD` |

**Embeddings** come from a hosted provider or from a model you run. The provider is chosen per job, by the prefix of its `model` (`openai/text-embedding-3-small`), and its credentials are read from the environment of both the server and the worker:

| | set in `.env` |
|---|---|
| OpenAI | `OPENAI_API_KEY`, and `OPENAI_BASE_URL` for a compatible gateway |
| Voyage, Cohere | `VOYAGE_API_KEY`, `CO_API_KEY` |
| Portkey | `PORTKEY_API_KEY` and `PORTKEY_VIRTUAL_KEY` |
| Your own endpoint | `EMBEDDING_SVC_URL` and, if it needs one, `EMBEDDING_SVC_API_KEY` |
| Bundled [TEI](https://github.com/huggingface/text-embeddings-inference) | `COMPOSE_PROFILES=tei` and `EMBEDDING_MODEL` |

`COMPOSE_PROFILES` takes both: `COMPOSE_PROFILES=postgres,tei`. The full list of variables, and what each provider needs, is in [Server configuration](server/configuration.md). For example, a managed Postgres with OpenAI, and nothing else to run:

```bash
cd deploy
cp .env.example .env
# in .env: delete the COMPOSE_PROFILES line, then set
#   DATABASE_URL=postgresql://user:password@your-host:5432/postgres
#   OPENAI_API_KEY=sk-...
docker compose up -d
```

To run everything yourself, keep `COMPOSE_PROFILES=postgres,tei` from `.env.example`, set `POSTGRES_PASSWORD`, pick an `EMBEDDING_MODEL`, and run `docker compose up -d`.

## Choosing an embedding model

The best model depends on your language, domain, document length, latency and cost, and on the hardware it runs on. Plan to spend time on this: embed a sample of your own data with a few candidates and check that search returns what your users expect. Trying candidates first is cheap (`vector-serve` or a hosted provider makes it easy) and changing later is expensive.

If you want somewhere to start, `deploy/` uses `BAAI/bge-base-en-v1.5`. These three work with the bundled TEI:

| model | dimensions | max tokens |
|---|---|---|
| `BAAI/bge-small-en-v1.5` | 384 | 512 |
| `BAAI/bge-base-en-v1.5` | 768 | 512 |
| `BAAI/bge-large-en-v1.5` | 1024 | 512 |

Larger models usually retrieve better and run slower. All three are English-language and MIT-licensed. TEI runs any embedding model it supports (see its documentation for the list), so for other languages pick a multilingual one.

Two things to know before you choose, whichever provider you use:

- **The model is fixed per table.** The embeddings column is typed `vector(N)`, so changing to a model with a different dimension means re-embedding everything. Vectors from different models are not comparable even at the same dimension. Decide before loading data.
- **Inputs longer than the model's max tokens are truncated** (the bundled TEI runs with `--auto-truncate`), so the tail of a long document does not affect its embedding. Check the limit against your document or chunk size.

## Self-hosting embeddings with TEI

This section applies when you run the embedding model yourself (the `tei` profile). With a hosted provider, skip to [Postgres](#postgres).

### Use TEI instead of vector-serve

`vector-serve` is convenient: it downloads any Hugging Face model on first use and can serve several at once. That flexibility is what makes it good for demos and for comparing models. It is also a Python process running one `sentence-transformers` model call at a time per worker, and it is much slower than a dedicated inference server.

[Text Embeddings Inference](https://github.com/huggingface/text-embeddings-inference) (TEI) serves one model chosen at startup, batches requests by token count, and exposes the same `/v1/embeddings` API. Measured on one 24-vCPU WSL2 Docker host with `all-MiniLM-L6-v2` and short inputs, requests per second straight to `/v1/embeddings`:

| concurrent requests | vector-serve | TEI |
|---|---|---|
| 1 | 236 | 268 |
| 8 | 121 | 463 |
| 32 | 201 | 647 |
| 64 | 197 | 721 |

This is embedding throughput only. On the end-to-end search benchmark (`bench/`), TEI and vector-serve came out about even: the hybrid SQL query and CPU sharing with Postgres dominate there. Expect the gain to show up when you ingest or re-embed a lot of data, not in single-query search latency.

### TEI settings that matter for pg_vectorize

| flag | why |
|---|---|
| `--max-client-batch-size 2048` | pg_vectorize sends up to 2048 inputs per request. TEI's default of 32 rejects them. |
| `--max-concurrent-requests 4096` | TEI counts inputs, not requests, against this limit and answers `429` past it. The default of 512 is below one full batch. |
| `--payload-limit 50000000` | Default request body cap is 2 MB, which a batch of real documents exceeds (`413`). |
| `--auto-truncate` | Cut inputs longer than the model's limit instead of failing the whole batch because of one long row. |

### Setting the model

Set `EMBEDDING_MODEL` in `.env` (see [Choosing an embedding model](#choosing-an-embedding-model)). TEI serves that one model and **ignores the `model` field in a request**. A job created with a different `model` string is still embedded by TEI's model, without any error, so keep the job's `model` and `EMBEDDING_MODEL` the same.

### CPU sizing and batch size

Larger models are slower than small ones, and CPU is much slower than GPU (TEI publishes GPU images; see its documentation for the right one for your GPU). How slow depends on the model, your hardware and your document length, so time a batch of your own documents before settling on settings.

This interacts with two pg_vectorize settings:

- Rows inserted or updated after a job exists are queued in jobs of up to `vectorize.batch_size` rows, and the worker embeds each job's rows together. This is a Postgres setting, read by the trigger function in the session that writes the rows, not a server environment variable. The built-in default is **1000**, but on first start the server tries to `ALTER SYSTEM` it to **10000**. That needs superuser, so the bundled Postgres ends up at 10000 and most managed Postgres services stay at 1000. The rows already in the table when you create a job are queued differently: in batches of about 10,000 tokens, which is not configurable.
- Embedding requests time out after **120 s** by default. Set the `EMBEDDING_REQUEST_TIMEOUT` environment variable (in seconds) on the worker to change it: the `worker` service in the compose file, or the server if you run the worker in-process. It applies to each HTTP request to the embedding provider.

On CPU, a job of 1000 or more long documents can take longer than 120 s. And when a client gives up, TEI does not cancel the batch it already accepted: in testing it kept all CPUs busy for minutes after the proxy timed out. If your documents are long and you are on CPU, lower the batch size for the database that holds your tables:

```sql
ALTER DATABASE postgres SET vectorize.batch_size = 100;
```

A database-level setting overrides the server's `ALTER SYSTEM` value, and applies to new connections, so recycle your application's connection pool afterwards.

## Postgres

- **Prefer a managed Postgres** that offers pgvector (RDS, Cloud SQL, Neon, and others): leave the `postgres` profile off and set `DATABASE_URL`. If you use the bundled one, back up the `pgdata` volume.
- **Size memory.** Postgres defaults are tiny. `maintenance_work_mem` bounds HNSW index builds; `shared_buffers` and `effective_cache_size` should reflect the host. The compose file exposes these as `PG_SHARED_BUFFERS`, `PG_EFFECTIVE_CACHE_SIZE` and `PG_MAINTENANCE_WORK_MEM`.
- **Keep the embedder off the database host** if you can. On the benchmark host, Postgres and the embedding service competed for the same CPUs. We did not isolate how much of the search latency that explains, so treat this as a precaution.
- **Connection pool.** `DATABASE_POOL_MAX` (default `2 * NUM_SERVER_WORKERS + 2`, so 18) and `NUM_SERVER_WORKERS` (default 8) set how many connections the server holds. Keep the total under Postgres's `max_connections`.

## The background worker

The worker that turns queued rows into embeddings can run in two ways:

- **Inside the server** (the default). `vectorize-server` starts the worker as a task, so one process serves the API and does the embedding. The repository's demo `docker-compose.yml` does this.
- **As its own service.** Set `VECTORIZE_WORKER_ENABLED=false` on the server and run the `vectorize-worker` binary, which is in the same image, as a separate container. `deploy/docker-compose.yml` does this.

With the split, ingest load no longer competes with search traffic for a process or a connection pool, and you scale each side on its own:

```bash
docker compose up -d --scale worker=4    # more embedding throughput
```

Notes:

- Several workers can poll the same queue. Each job is claimed from the queue with a 300 s visibility timeout, so two workers do not take the same job. Embedding throughput is still capped by TEI, so more workers only help if TEI is not already saturated.
- Each `vectorize-worker` holds 5 Postgres connections. Add that to the server's pool when you size `max_connections`.
- With the in-process worker off, the server's `/health` and `/health/ready` return 200 and report the worker as `Disabled`. They say nothing about whether a worker is running.
- The standalone worker serves its own `GET /health` on `WORKER_HEALTH_PORT` (default 8081; the compose file does not publish it) and the compose service uses it as its healthcheck. It returns 200 with `status: healthy`, or 503 while the worker is failing to read the queue or process a job (for example when Postgres or TEI is down), with the last error in the body. It recovers on the next successful poll. It does not detect a worker stuck inside one slow batch, because a busy worker does not heartbeat; watch the queue depth for that (`SELECT * FROM pgmq.metrics('vectorize_jobs')`).
- If you turn the in-process worker off and start no worker, rows are queued and never embedded.

## Security and networking

- The server has **no authentication** and allows **any CORS origin**. Do not expose port 8080 directly. Put a reverse proxy in front that terminates TLS and authenticates requests. The compose file binds the port to `127.0.0.1` for this reason.
- Postgres and TEI publish no ports. To get a SQL shell, use `docker compose exec postgres psql -U postgres`.
- If you use the bundled Postgres, set a real `POSTGRES_PASSWORD`. The container will not initialize without one. Keep provider API keys in `.env` (or your secret store), not in the compose file.

## Operations

- **Pin images.** `deploy/` pins TEI and pgvector to versions. Set `VECTORIZE_SERVER_IMAGE` to a specific tag or digest instead of `latest`. Changing the TEI version or model changes the embeddings, which matters for an existing index.
- **Model cache.** TEI stores downloaded weights in the `tei-cache` volume. The first start downloads the model, and the healthcheck allows two minutes for that.
- **Logs** are rotated by the compose logging options. Lower `RUST_LOG` from `debug` (the demo default) to `info`.
