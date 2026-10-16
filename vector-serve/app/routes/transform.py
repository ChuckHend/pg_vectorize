import logging
import os
import threading
from typing import TYPE_CHECKING, List

from app.models import model_org_name, get_model, parse_header
from fastapi import APIRouter, Header, HTTPException, Request
from pydantic import BaseModel, conlist

router = APIRouter(tags=["transform"])

LOG_LEVEL = os.getenv("LOG_LEVEL", "INFO").upper()
logging.basicConfig(level=LOG_LEVEL)

# Inference batch size passed straight through to SentenceTransformer.encode(),
# which does its own internal chunking (and sorts inputs by length first to
# minimize padding waste across the whole request) -- so this controls the
# actual model batch size, not an outer Python-level loop.
BATCH_SIZE = int(os.getenv("BATCH_SIZE", 32))

# Serialize encode() per worker process. encode() is CPU-bound, so async/threads
# don't add throughput, and concurrent calls each start their own torch/OpenMP
# thread team. Those threads spin while waiting, so a few overlapping requests
# oversubscribe the CPU and latency collapses.
_ENCODE_LOCK = threading.Lock()


if TYPE_CHECKING:
    Vector = List[str]
else:
    Vector = conlist(str, min_length=1)


class Batch(BaseModel):
    input: Vector
    model: str = "all-MiniLM-L6-v2"
    normalize: bool = False


class Embedding(BaseModel):
    embedding: list[float]
    index: int


class ResponseModel(BaseModel):
    data: list[Embedding]
    model: str


@router.post("/v1/embeddings", response_model=ResponseModel)
def batch_transform(
    request: Request, payload: Batch, authorization: str = Header(None)
) -> ResponseModel:
    logging.info({"batch-predict-len": len(payload.input)})

    requested_model = model_org_name(payload.model)

    api_key = parse_header(authorization)
    try:
        model = get_model(
            model_name=requested_model,
            model_cache=request.app.state.model_cache,
            api_key=api_key,
        )
    except Exception as e:
        raise HTTPException(
            status_code=400,
            detail=f"Unable to load {payload.model} -- {e}",
        )

    with _ENCODE_LOCK:
        vectors = model.encode(
            sentences=payload.input,
            batch_size=BATCH_SIZE,
            normalize_embeddings=payload.normalize,
            show_progress_bar=False,
        ).tolist()
    embeds = [
        Embedding(embedding=embedding, index=i) for i, embedding in enumerate(vectors)
    ]
    return ResponseModel(
        data=embeds,
        model=requested_model,
    )
