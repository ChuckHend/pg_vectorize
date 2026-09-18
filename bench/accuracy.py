"""
Two different notions of "accuracy", answering two different questions:

  ir_metrics()   "Did the pipeline (embedding model + hybrid search +
                  RRF fusion) surface documents a human said were
                  relevant?" Scored with pytrec_eval against BEIR's
                  qrels -- the standard IR eval approach, same one
                  MTEB/BEIR leaderboards use, so your numbers are
                  comparable to published embedding-model results.

  ann_recall_at_k()  "Did pgvector's index return the same neighbors
                      as an exact, brute-force cosine search?" This
                      isolates *index* approximation quality from
                      *embedding model* quality. It reads each result's
                      `semantic_rank` field to recover the semantic-only
                      ordering out of the hybrid response.

Caveat worth checking against your running version: the /search endpoint
in the README example always returns fts_rank/semantic_rank/rrf_score,
implying hybrid search runs unconditionally. If a newer version exposes
a way to request semantic-only results, prefer that for ANN-recall
instead of reconstructing it from semantic_rank -- it removes any doubt
about whether RRF fusion truncated the semantic candidate list before
you got to see it.
"""

from __future__ import annotations

import logging

import numpy as np
import pytrec_eval

logger = logging.getLogger(__name__)


def ir_metrics(
    qrels: dict[str, dict[str, int]],
    api_results: dict[str, dict],
    id_field: str = "doc_id",
    score_field: str = "rrf_score",
    k_values: tuple[int, ...] = (1, 5, 10, 20),
) -> dict:
    """
    api_results: {query_id: raw_json_response_from_search_endpoint}
    Returns pytrec_eval's aggregate measures (NDCG@k, MAP@k, Recall@k, P@k).
    """
    run: dict[str, dict[str, float]] = {}
    for qid, rows in api_results.items():
        scored = {}
        for row in rows:
            score = row.get(score_field, row.get("similarity_score", 0.0))
            scored[str(row[id_field])] = float(score)
        run[qid] = scored

    # only evaluate queries we actually have both a run and a qrel for
    judged_qrels = {qid: rels for qid, rels in qrels.items() if qid in run}

    measures = {
        f"ndcg_cut.{','.join(map(str, k_values))}",
        f"recall.{','.join(map(str, k_values))}",
        f"map_cut.{','.join(map(str, k_values))}",
        f"P.{','.join(map(str, k_values))}",
    }
    evaluator = pytrec_eval.RelevanceEvaluator(judged_qrels, measures)
    per_query = evaluator.evaluate(run)

    aggregated: dict[str, float] = {}
    for metric_scores in per_query.values():
        for metric, value in metric_scores.items():
            aggregated.setdefault(metric, []).append(value)
    return {metric: float(np.mean(values)) for metric, values in aggregated.items()}


def brute_force_top_k(
    query_vector: np.ndarray, doc_ids: list[str], doc_matrix: np.ndarray, k: int
) -> list[str]:
    """Exact cosine top-k (doc_matrix rows assumed already L2-normalized)."""
    q = query_vector / (np.linalg.norm(query_vector) or 1.0)
    sims = doc_matrix @ q
    top_idx = np.argpartition(-sims, min(k, len(sims) - 1))[:k]
    top_idx = top_idx[np.argsort(-sims[top_idx])]
    return [doc_ids[i] for i in top_idx]


def ann_recall_at_k(
    api_results: dict[str, dict],
    ground_truth: dict[str, list[str]],
    k: int,
    id_field: str = "doc_id",
) -> float:
    """
    api_results: {query_id: raw_json_response}, each row expected to carry
                 a `semantic_rank` field (1 = best semantic match).
    ground_truth: {query_id: [doc_id, ...]} exact top-k from brute_force_top_k.
    """
    per_query_recall = []
    for qid, rows in api_results.items():
        if qid not in ground_truth:
            continue
        ranked = sorted(
            (r for r in rows if r.get("semantic_rank") is not None),
            key=lambda r: r["semantic_rank"],
        )
        returned_ids = {str(r[id_field]) for r in ranked[:k]}
        truth_ids = set(ground_truth[qid][:k])
        if not truth_ids:
            continue
        per_query_recall.append(len(returned_ids & truth_ids) / len(truth_ids))
    if not per_query_recall:
        logger.warning("no comparable queries for ANN recall@%d", k)
        return float("nan")
    return float(np.mean(per_query_recall))
