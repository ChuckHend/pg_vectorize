"""
Two distinct passes over /api/v1/search, kept separate on purpose:

  run_accuracy_pass  -- one request per query, sequential-ish, just to
                        collect *which documents came back*, for scoring.
  run_load_test      -- a concurrency sweep whose only job is measuring
                        latency distribution and realized QPS. Query
                        content doesn't matter much here beyond being
                        realistic and varied (fixed-string benchmarks
                        hide query-embedding-time variance).
"""

from __future__ import annotations

import asyncio
import itertools
import logging
import time
from dataclasses import dataclass, field

import httpx
import numpy as np

from api_client import search

logger = logging.getLogger(__name__)


# ---------- accuracy pass ----------

async def run_accuracy_pass(
    base_url: str,
    job_name: str,
    queries: dict[str, str],
    limit: int,
    concurrency: int = 8,
    timeout: float = 30.0,
) -> dict[str, dict]:
    """Returns {query_id: parsed_json_response}. One search call per query."""
    results: dict[str, dict] = {}
    sem = asyncio.Semaphore(concurrency)

    async def one(qid: str, text: str, client: httpx.AsyncClient):
        async with sem:
            resp = await search(client, base_url, job_name, text, limit)
            resp.raise_for_status()
            results[qid] = resp.json()

    async with httpx.AsyncClient(timeout=timeout) as client:
        await asyncio.gather(*(one(qid, text, client) for qid, text in queries.items()))
    return results


# ---------- load test ----------

@dataclass
class LoadTestResult:
    concurrency: int
    n_requests: int
    n_errors: int
    wall_time_s: float
    qps: float
    latencies_ms: list[float] = field(repr=False)

    def percentile(self, p: float) -> float:
        return float(np.percentile(self.latencies_ms, p))

    def summary(self) -> dict:
        return {
            "concurrency": self.concurrency,
            "n_requests": self.n_requests,
            "n_errors": self.n_errors,
            "qps": round(self.qps, 2),
            "p50_ms": round(self.percentile(50), 2),
            "p95_ms": round(self.percentile(95), 2),
            "p99_ms": round(self.percentile(99), 2),
            "max_ms": round(max(self.latencies_ms), 2) if self.latencies_ms else None,
        }


async def _worker(
    client: httpx.AsyncClient,
    base_url: str,
    job_name: str,
    query_text: str,
    limit: int,
    sem: asyncio.Semaphore,
    latencies: list[float],
    errors: list[int],
) -> None:
    async with sem:
        start = time.perf_counter()
        try:
            resp = await search(client, base_url, job_name, query_text, limit)
            resp.raise_for_status()
        except Exception as exc:  # noqa: BLE001 - we want to count *any* failure
            errors.append(1)
            logger.warning("request failed: %s", exc)
        else:
            latencies.append((time.perf_counter() - start) * 1000.0)


async def run_load_test(
    base_url: str,
    job_name: str,
    query_pool: list[str],
    limit: int,
    concurrency: int,
    n_requests: int,
    timeout: float = 30.0,
) -> LoadTestResult:
    latencies: list[float] = []
    errors: list[int] = []
    query_cycle = itertools.cycle(query_pool)
    sem = asyncio.Semaphore(concurrency)

    async with httpx.AsyncClient(timeout=timeout) as client:
        start = time.perf_counter()
        tasks = [
            _worker(client, base_url, job_name, next(query_cycle), limit, sem, latencies, errors)
            for _ in range(n_requests)
        ]
        await asyncio.gather(*tasks)
        wall_time = time.perf_counter() - start

    n_ok = len(latencies)
    return LoadTestResult(
        concurrency=concurrency,
        n_requests=n_requests,
        n_errors=len(errors),
        wall_time_s=wall_time,
        qps=n_ok / wall_time if wall_time > 0 else 0.0,
        latencies_ms=latencies,
    )


async def run_concurrency_sweep(
    base_url: str,
    job_name: str,
    query_pool: list[str],
    limit: int,
    concurrency_levels: list[int],
    requests_per_level: int,
) -> list[LoadTestResult]:
    out = []
    for c in concurrency_levels:
        logger.info("load test: concurrency=%d, n_requests=%d", c, requests_per_level)
        result = await run_load_test(base_url, job_name, query_pool, limit, c, requests_per_level)
        out.append(result)
        logger.info("  -> %s", result.summary())
    return out
