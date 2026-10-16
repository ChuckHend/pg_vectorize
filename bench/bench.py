"""
End-to-end pg_vectorize HTTP-server benchmark.

Usage:
    python bench.py \
        --dsn "postgresql://postgres:postgres@localhost:5432/postgres" \
        --base-url http://localhost:8080 \
        --dataset scifact \
        --job-name bench_scifact \
        --model sentence-transformers/all-MiniLM-L6-v2 \
        --concurrency 1,5,10,25,50 \
        --requests-per-level 200

What it does, in order:
  1. downloads the BEIR dataset and loads its corpus into a fresh Postgres table
  2. calls POST /api/v1/table to kick off pg_vectorize's embedding job
  3. polls Postgres until every row has an embedding
  4. runs one search per BEIR query -> scores IR quality (NDCG/Recall/MAP
     vs. qrels) and ANN-recall (vs. an exact brute-force baseline)
  5. runs a concurrency sweep against /api/v1/search -> latency percentiles + QPS
  6. writes everything to --output-dir as JSON, and prints a summary table
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import random
import time
from pathlib import Path

from tabulate import tabulate

from accuracy import ann_recall_at_k, brute_force_top_k, ir_metrics
from api_client import create_job, delete_job
from dataset import load_beir_dataset
from load_test import run_accuracy_pass, run_concurrency_sweep
from pg_ops import fetch_all_embeddings, load_corpus, wait_for_embeddings

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
logger = logging.getLogger("bench")


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--dsn", required=True, help="Postgres connection string")
    p.add_argument("--base-url", required=True, help="pg_vectorize HTTP server base URL")
    p.add_argument("--dataset", default="scifact", help="BEIR dataset name")
    p.add_argument("--table-name", default="bench_corpus")
    p.add_argument("--job-name", default="bench_job")
    p.add_argument("--model", default="sentence-transformers/all-MiniLM-L6-v2")
    p.add_argument("--search-limit", type=int, default=10, help="`limit` param for /api/v1/search")
    p.add_argument("--k-values", default="1,5,10,20", help="cutoffs for IR/ANN metrics")
    p.add_argument("--concurrency", default="1,5,10,25,50", help="comma-separated concurrency levels")
    p.add_argument("--requests-per-level", type=int, default=200)
    p.add_argument("--max-accuracy-queries", type=int, default=None,
                    help="subsample queries for the accuracy pass (useful for large datasets)")
    p.add_argument("--output-dir", default="./bench_results")
    p.add_argument("--skip-ingest", action="store_true",
                    help="skip steps 1-3, assume table+job already populated")
    return p.parse_args()


async def main() -> None:
    args = parse_args()
    out_dir = Path(args.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    k_values = tuple(int(k) for k in args.k_values.split(","))
    concurrency_levels = [int(c) for c in args.concurrency.split(",")]

    data = load_beir_dataset(args.dataset)

    if not args.skip_ingest:
        delete_job(args.base_url, args.job_name)
        n_docs = load_corpus(args.dsn, args.table_name, data.corpus)
        create_job(args.base_url, args.job_name, args.table_name, args.model)
        wait_for_embeddings(args.dsn, args.job_name, n_docs)
    else:
        n_docs = len(data.corpus)

    # ---- accuracy pass ----
    queries = data.queries
    if args.max_accuracy_queries and len(queries) > args.max_accuracy_queries:
        sample_ids = random.sample(list(queries.keys()), args.max_accuracy_queries)
        queries = {qid: queries[qid] for qid in sample_ids}
    logger.info("running accuracy pass over %d queries", len(queries))
    api_results = await run_accuracy_pass(args.base_url, args.job_name, queries, args.search_limit)

    ir_scores = ir_metrics(data.qrels, api_results, k_values=k_values)

    logger.info("pulling embeddings for brute-force ANN-recall baseline")
    doc_ids, doc_matrix = fetch_all_embeddings(args.dsn, args.job_name)
    # ground truth needs query embeddings from the *same* model pg_vectorize used
    from sentence_transformers import SentenceTransformer  # local import: heavy, only needed here

    st_model = SentenceTransformer(args.model)
    ground_truth = {}
    for qid, text in queries.items():
        qvec = st_model.encode(text, normalize_embeddings=False)
        ground_truth[qid] = brute_force_top_k(qvec, doc_ids, doc_matrix, k=max(k_values))

    ann_recall = {k: ann_recall_at_k(api_results, ground_truth, k) for k in k_values}

    # ---- latency / throughput pass ----
    query_pool = list(data.queries.values())
    load_results = await run_concurrency_sweep(
        args.base_url, args.job_name, query_pool, args.search_limit,
        concurrency_levels, args.requests_per_level,
    )

    # ---- report ----
    timestamp = int(time.time())
    report = {
        "dataset": args.dataset,
        "n_docs": n_docs,
        "n_queries_scored": len(queries),
        "model": args.model,
        "search_limit": args.search_limit,
        "ir_metrics": ir_scores,
        "ann_recall_at_k": ann_recall,
        "load_test": [r.summary() for r in load_results],
    }
    out_path = out_dir / f"report_{args.dataset}_{timestamp}.json"
    out_path.write_text(json.dumps(report, indent=2))

    print("\n=== IR quality (vs. BEIR qrels) ===")
    print(tabulate(sorted(ir_scores.items()), headers=["metric", "value"]))

    print("\n=== ANN recall@k (vs. exact brute-force) ===")
    print(tabulate(sorted(ann_recall.items()), headers=["k", "recall"]))

    print("\n=== Latency / throughput ===")
    print(tabulate([r.summary() for r in load_results], headers="keys"))

    print(f"\nFull report written to {out_path}")


if __name__ == "__main__":
    asyncio.run(main())
