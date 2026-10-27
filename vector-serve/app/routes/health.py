import logging

from fastapi import APIRouter
from pydantic import BaseModel

logger = logging.getLogger(__name__)

router = APIRouter(tags=["health"])


class ReadyResponse(BaseModel):
    ready: bool


class AliveResponse(BaseModel):
    alive: bool


@router.get("/ready", response_model=ReadyResponse)
def ready() -> ReadyResponse:
    logger.debug("Health check")
    return ReadyResponse(
        ready=True,
    )


@router.get("/alive", response_model=AliveResponse)
def alive() -> AliveResponse:
    logger.debug("Health check")
    return AliveResponse(
        alive=True,
    )
