# pg_vectorize HTTP-server benchmark

Benchmarks `/api/v1/table` (ingest) and `/api/v1/search` (query) against a
BEIR dataset, measuring latency, throughput, and IR/ANN accuracy.

## Setup

```bash
uv sync
```

Requires:
- pg_vectorize HTTP server + Postgres running (`docker compose up -d`), reachable at `--base-url`.
- Direct Postgres access via `--dsn` (loads the corpus, pulls raw embeddings for the brute-force baseline).

## Run

```bash
uv run bench.py \
  --dsn "postgresql://postgres:postgres@localhost:5432/postgres" \
  --base-url http://localhost:8080 \
  --dataset scifact \
  --job-name bench_scifact \
  --model sentence-transformers/all-MiniLM-L6-v2 \
  --concurrency 1,5,10,25,50 \
  --requests-per-level 200
```

Start with `scifact` (~5K docs) to verify wiring, then switch to `fiqa`/`msmarco`
for real throughput numbers.

### Scaling past a throughput ceiling

If QPS plateaus as `--concurrency` increases while latency grows linearly,
check in order:

1. **Server HTTP client** — fixed in `core/src/transformers/http_handler.rs`
   (shared `HTTP_CLIENT`). A fresh `reqwest::Client` per request used to pin
   the server at 600%+ CPU and cap throughput regardless of concurrency or
   replica count. Make sure your image includes this fix.
2. **`vector-serve`** — single CPU-bound worker per container
   (`--workers 1`); becomes the next ceiling once (1) is fixed.
   `NUM_SERVER_WORKERS`/`DATABASE_POOL_MAX` tune the DB pool but are rarely
   the actual bottleneck.

To test with 2 `vector-serve` replicas behind an nginx LB:

```bash
docker compose -f docker-compose.yml -f bench/docker-compose.bench.yml up -d
```

Tear down with the same command, swapping `up -d` for `down`.

## What gets measured

- **IR quality** (`NDCG@k`, `Recall@k`, `MAP@k`) — `/search` results scored
  against BEIR qrels via `pytrec_eval`.
- **ANN recall@k** — `/search`'s semantic ranking vs. exact brute-force
  cosine search; isolates index approximation from embedding-model quality.
- **Latency/throughput** — concurrency sweep against `/api/v1/search`,
  p50/p95/p99 + QPS per level.

## Assumptions

1. **Embedding table**: polls `vectorize._embeddings_{job_name}` (column
   `embeddings`), not a `{job_name}_embeddings` source-table column (the
   older pgrx extension's convention). Edit `EMBEDDING_TABLE_FMT` in
   `pg_ops.py` if yours differs.
2. **Hybrid-only search**: ANN-recall is reconstructed from `semantic_rank`
   in the hybrid response. Prefer a semantic-only mode if one's added later.
3. **No job-status endpoint**: `wait_for_embeddings` polls the DB directly;
   swap for a status endpoint if one exists.

## Extending

- Ingest throughput (docs/sec via `/api/v1/table`) isn't measured;
  `load_corpus`/`wait_for_embeddings` timing is where to add it.
- To sweep HNSW params (`m`/`ef_construction`/`ef_search`), rebuild the
  index between runs and rerun with `--skip-ingest`.
