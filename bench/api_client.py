"""Thin wrapper around the pg_vectorize HTTP server's endpoints."""

from __future__ import annotations

import httpx


def delete_job(base_url: str, job_name: str, timeout: float = 30.0) -> None:
    """Tear down a previous run's job (view, embeddings table, triggers, etc).

    Ignores 404s -- there may be no prior job to clean up.
    """
    resp = httpx.delete(f"{base_url}/api/v1/table/{job_name}", timeout=timeout)
    if resp.status_code != 404:
        resp.raise_for_status()


def create_job(
    base_url: str,
    job_name: str,
    src_table: str,
    model: str,
    primary_key: str = "doc_id",
    columns: list[str] | None = None,
    src_schema: str = "public",
    update_time_col: str = "updated_at",
    timeout: float = 30.0,
) -> dict:
    columns = columns or ["title", "body"]
    payload = {
        "job_name": job_name,
        "src_table": src_table,
        "src_schema": src_schema,
        "src_columns": columns,
        "primary_key": primary_key,
        "update_time_col": update_time_col,
        "model": model,
    }
    resp = httpx.post(f"{base_url}/api/v1/table", json=payload, timeout=timeout)
    resp.raise_for_status()
    return resp.json()


async def search(
    client: httpx.AsyncClient,
    base_url: str,
    job_name: str,
    query: str,
    limit: int,
) -> httpx.Response:
    return await client.get(
        f"{base_url}/api/v1/search",
        params={"job_name": job_name, "query": query, "limit": limit},
    )
