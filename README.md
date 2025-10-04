<h1 align="center">
 <b>pg_vectorize: a VectorDB for Postgres</b>
</h1>

A Postgres server extension that automates the transformation and orchestration of text to embeddings and provides hooks into the most popular LLMs. This allows you to do get up and running and automate maintenance for vector search, full text search, and hybrid search very quickly, which enables you to quickly build RAG and search engines on Postgres.

This project relies heavily on the work by [pgvector](https://github.com/pgvector/pgvector) for vector similarity search, [pgmq](https://github.com/pgmq/pgmq) for orchestration in background workers, and [SentenceTransformers](https://huggingface.co/sentence-transformers).

---

[![PGXN version](https://badge.fury.io/pg/vectorize.svg)](https://pgxn.org/dist/vectorize/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13%20%7C%2014%20%7C%2015%20%7C%2016%20%7C%2017%20%7C%2018-336791?logo=postgresql&logoColor=white)](https://www.postgresql.org/)


**API Documentation**: https://chuckhend.github.io/pg_vectorize/

**Source**: https://github.com/tembo-io/pg_vectorize

## Overview

pg_vectorize provides two ways to add semantic, full text, and hybrid to any Postgres making it easy to build retrieval-augmented generation (RAG) on Postgres. This top-level README focuses on the HTTP/server usage (the mode most suitable for managed Postgres instances), while still pointing to the Postgres extension docs for users who prefer an in-database SQL experience.

Modes at a glance:

- HTTP server (recommended for managed DBs): run a standalone service that connects to Postgres and exposes a REST API (POST /api/v1/table, GET /api/v1/search).
- Postgres extension (SQL): install the extension into Postgres and use SQL functions like `vectorize.table()` and `vectorize.search()` (requires filesystem access to Postgres; see [./extension/README.md](./extension/README.md)).

## Quick start — HTTP server

Run Postgres and the HTTP servers locally using docker compose:

```bash
# runs Postgres, the embeddings server, and the management API
docker compose up -d
```

Load the example dataset into Postgres (optional):

```bash
psql postgres://postgres:postgres@localhost:5432/postgres -f server/sql/example.sql
```

```text
CREATE TABLE
INSERT 0 40
```

Create an embedding job via the HTTP API. This generates embeddings for the existing data and continuously watches for updates or new data:

```bash
curl -X POST http://localhost:8080/api/v1/table -d '{
		"job_name": "my_job",
		"src_table": "my_products",
		"src_schema": "public",
		"src_columns": ["product_name", "description"],
		"primary_key": "product_id",
		"update_time_col": "updated_at",
		"model": "sentence-transformers/all-MiniLM-L6-v2"
	}' -H "Content-Type: application/json"
```

```json
{"id":"16b80184-2e8e-4ee6-b7e2-1a068ff4b314"}
```

Search using the HTTP API:

```bash
curl -X GET "http://localhost:8080/api/v1/search?job_name=my_job&query=camping%20backpack&limit=1" | jq .
```

```json
[
  {
    "description": "Storage solution for carrying personal items on ones back",
    "fts_rank": 1,
    "price": 45.0,
    "product_category": "accessories",
    "product_id": 6,
    "product_name": "Backpack",
    "rrf_score": 0.03278688524590164,
    "semantic_rank": 1,
    "similarity_score": 0.6296013593673706,
    "updated_at": "2025-10-04T14:45:16.152526+00:00"
  }
]
```

## Which should I pick?

- Use the HTTP server when your Postgres is managed (RDS, Cloud SQL, etc.) or you cannot install extensions. It requires only that `pgvector` is available in the database. You the HTTP services separately.
- Use Postgres extension when you self-host Postgres and can install extensions. This provides an in-database experience and direct SQL APIs for vectorization and RAG.

If you want hands-on SQL examples or to install the extension into Postgres, see `./extension/README.md`. For full HTTP API docs and deployment notes, see `./server/README.md`.


For contribution guidelines see `CONTRIBUTING.md` in the repo root.
