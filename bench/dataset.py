"""
Download and load a BEIR benchmark dataset.

BEIR (https://github.com/beir-cellar/beir) ships public, pre-packaged
IR datasets with a corpus, a query set, and human-judged relevance
labels (qrels). That last part is the thing you can't get from
ANN-Benchmarks or VectorDBBench -- it's what lets us score "recall of
relevant documents", not just "recall vs. an exact-search baseline".

Good starter datasets (small -> fast iteration):
    scifact   ~5.2K docs,  300 queries   (scientific claim verification)
    nfcorpus  ~3.6K docs,  323 queries   (medical/nutrition IR)
Bigger, for scale/throughput runs:
    fiqa      ~57K docs,   648 queries   (financial QA)
    msmarco   ~8.8M docs,  ~6.9K queries (large-scale passage ranking)
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

from beir import util
from beir.datasets.data_loader import GenericDataLoader

logger = logging.getLogger(__name__)

BEIR_URL_TEMPLATE = (
    "https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/{name}.zip"
)


@dataclass
class BeirData:
    corpus: dict  # doc_id -> {"title": str, "text": str}
    queries: dict  # query_id -> str
    qrels: dict  # query_id -> {doc_id: relevance_int}


def load_beir_dataset(name: str, out_dir: str = "./datasets", split: str = "test") -> BeirData:
    url = BEIR_URL_TEMPLATE.format(name=name)
    data_path = util.download_and_unzip(url, out_dir)
    corpus, queries, qrels = GenericDataLoader(data_folder=data_path).load(split=split)
    logger.info(
        "loaded %s: %d docs, %d queries, %d judged queries",
        name,
        len(corpus),
        len(queries),
        len(qrels),
    )
    return BeirData(corpus=corpus, queries=queries, qrels=qrels)
