"""
Direct Postgres access, used for:
  1. loading the BEIR corpus into the source table pg_vectorize will read from
  2. polling for embedding-generation completion (the HTTP API has no
     job-status endpoint, so we watch the generated embeddings table instead)
  3. pulling raw embeddings back out, to compute an exact brute-force
     nearest-neighbor baseline (for ANN-recall, see accuracy.py)

The HTTP server (server/src/routes/table.rs -> core/src/init.rs) stores
embeddings in a separate table "vectorize._embeddings_{job_name}" (column
"embeddings"), not as a column on the source table -- that's the older
pgrx SQL-extension's convention instead.
"""

from __future__ import annotations

import logging
import time

import numpy as np
import psycopg

logger = logging.getLogger(__name__)

EMBEDDING_TABLE_FMT = "vectorize._embeddings_{job_name}"


def load_corpus(dsn: str, table_name: str, corpus: dict) -> int:
    """(Re)create the source table and bulk-load the BEIR corpus into it."""
    with psycopg.connect(dsn, autocommit=True) as conn:
        with conn.cursor() as cur:
            cur.execute(f"DROP TABLE IF EXISTS {table_name}")
            cur.execute(
                f"""
                CREATE TABLE {table_name} (
                    doc_id TEXT PRIMARY KEY,
                    title TEXT,
                    body TEXT,
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
                )
                """
            )
            with cur.copy(f"COPY {table_name} (doc_id, title, body) FROM STDIN") as copy:
                for doc_id, doc in corpus.items():
                    copy.write_row((doc_id, doc.get("title", ""), doc.get("text", "")))
    logger.info("loaded %d rows into %s", len(corpus), table_name)
    return len(corpus)


def wait_for_embeddings(
    dsn: str,
    job_name: str,
    total_docs: int,
    poll_interval_s: float = 5.0,
    timeout_s: float = 3600.0,
) -> None:
    """Block until every row has an embedding, or raise on timeout."""
    embeddings_table = EMBEDDING_TABLE_FMT.format(job_name=job_name)
    start = time.monotonic()
    with psycopg.connect(dsn, autocommit=True) as conn:
        while True:
            with conn.cursor() as cur:
                cur.execute(f"SELECT count(*) FROM {embeddings_table}")
                (done,) = cur.fetchone()
            logger.info("embeddings: %d/%d", done, total_docs)
            if done >= total_docs:
                return
            if time.monotonic() - start > timeout_s:
                raise TimeoutError(
                    f"only {done}/{total_docs} embeddings after {timeout_s}s -- "
                    "is the worker running / job_name correct?"
                )
            time.sleep(poll_interval_s)


def fetch_all_embeddings(dsn: str, job_name: str) -> tuple[list[str], np.ndarray]:
    """Pull every (doc_id, embedding) pair back out for an exact brute-force baseline."""
    embeddings_table = EMBEDDING_TABLE_FMT.format(job_name=job_name)
    doc_ids: list[str] = []
    vectors: list[list[float]] = []
    with psycopg.connect(dsn) as conn:
        with conn.cursor() as cur:
            cur.execute(f"SELECT doc_id, embeddings::text FROM {embeddings_table}")
            for doc_id, vec_text in cur:
                doc_ids.append(doc_id)
                vectors.append([float(x) for x in vec_text.strip("[]").split(",")])
    matrix = np.array(vectors, dtype=np.float32)
    # L2-normalize so a dot product == cosine similarity
    norms = np.linalg.norm(matrix, axis=1, keepdims=True)
    norms[norms == 0] = 1.0
    matrix = matrix / norms
    return doc_ids, matrix
