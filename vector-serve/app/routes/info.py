import logging

from fastapi import APIRouter, Header, HTTPException, Query, Request
from pydantic import BaseModel
from sentence_transformers import SentenceTransformer

from app.models import get_model, model_org_name, parse_header

logger = logging.getLogger(__name__)

router = APIRouter(tags=["info"])


class InfoResponse(BaseModel):
    model: str
    max_seq_len: int
    embedding_dimension: int


@router.get("/v1/info/", response_model=InfoResponse)
def model_info(
    request: Request, model_name: str = Query(...), authorization: str = Header(None)
) -> InfoResponse:
    requested_model = model_org_name(model_name)
    try:
        api_key = parse_header(authorization)

        model: SentenceTransformer = get_model(
            model_name=requested_model,
            model_cache=request.app.state.model_cache,
            api_key=api_key,
        )
    except Exception as e:  # noqa: BLE001 -- any model load failure maps to a 400
        raise HTTPException(
            status_code=400,
            detail=f"Unable to load {requested_model} -- {e}",
        )
    logger.debug(requested_model)
    logger.debug(model)
    return InfoResponse(
        model=requested_model,
        max_seq_len=model.get_max_seq_length(),
        embedding_dimension=model.get_sentence_embedding_dimension(),
    )
