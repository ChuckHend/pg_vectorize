<h1 align="center">
 <b>pg_vectorize: a VectorDB for Postgres</b>
</h1>

A Postgres extension that automates the transformation and orchestration of text to embeddings and provides hooks into the most popular LLMs. This allows you to do get up and running and automate maintenance for vector search, full text search, and hybrid search very quickly, which enables you to quickly build RAG and search engines on Postgres.

This project relies heavily on the work by [pgvector](https://github.com/pgvector/pgvector) for vector similarity search, [pgmq](https://github.com/pgmq/pgmq) for orchestration in background workers, and [SentenceTransformers](https://huggingface.co/sentence-transformers).

---

[![PGXN version](https://badge.fury.io/pg/vectorize.svg)](https://pgxn.org/dist/vectorize/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13%20%7C%2014%20%7C%2015%20%7C%2016%20%7C%2017%20%7C%2018-336791?logo=postgresql&logoColor=white)](https://www.postgresql.org/)


**API Documentation**: https://chuckhend.github.io/pg_vectorize/

```markdown
# pg_vectorize — Vector search and RAG for Postgres

pg_vectorize provides two ways to add semantic search and retrieval-augmented generation (RAG) to a Postgres-backed app:

- A Postgres extension (native SQL API) that runs inside Postgres and exposes functions like `vectorize.table()`, `vectorize.search()` and `vectorize.rag()`.
- A standalone HTTP server that connects to an existing Postgres (with `pgvector` installed) and exposes a REST API (for example `POST /api/v1/table` and `GET /api/v1/search`).

Which mode you pick depends on your deployment constraints:

- Extension (SQL): requires file-system access to the Postgres instance because the extension is installed into Postgres itself. Best when you self-host Postgres and want to run everything close to the database and use SQL directly.
- HTTP server (service): works with any Postgres that already has `pgvector` installed (including managed DBs like RDS or Cloud SQL). You run the HTTP server separately (for example in Docker on an EC2 instance) and it connects to Postgres over the network.

## Which should I pick?

Quick guidance to choose a mode:

- Use the HTTP server if:
	- Your Postgres is managed (RDS, Cloud SQL, etc.) or you cannot install extensions.
	- You prefer running a separate service (Docker, Kubernetes, or VM) that connects to Postgres over the network.
	- You want a REST API for embedding jobs and search (POST /api/v1/table, GET /api/v1/search, etc.).

- Use the Postgres extension if:
	- You self-host Postgres and can install extensions (need filesystem access).
	- You want to run transformations and searches inside the database and call everything from SQL.
	- In-database processing is a priority.

If you're still unsure: prefer the mode that requires the least operational changes for your environment — managed DBs usually go with the HTTP server, self-hosted DBs often benefit from the extension.

This repository contains both projects. See the detailed docs below:

- [Extension (SQL)](./extension/README.md): — installation, SQL examples, RAG workflows, and advanced usage.
- [HTTP server](./server/README.md): — how to run the standalone server, API examples, and deployment notes.

Badges, docs and source

[![PGXN version](https://badge.fury.io/pg/vectorize.svg)](https://pgxn.org/dist/vectorize/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13%20%7C%2014%20%7C%2015%20%7C%2016%20%7C%2017%20%7C%2018-336791?logo=postgresql&logoColor=white)](https://www.postgresql.org/)

API Documentation: https://chuckhend.github.io/pg_vectorize/

Source: https://github.com/tembo-io/pg_vectorize

For contribution guidelines see `CONTRIBUTING.md` in the repo root.

```
```text
