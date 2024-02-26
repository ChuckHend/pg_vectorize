from fastapi import APIRouter
from pydantic import BaseModel
import logging

router = APIRouter(tags=["health"])


class ReadyResponse(BaseModel):
    ready: bool


class AliveResponse(BaseModel):
    alive: bool


@router.get("/ready", response_model=ReadyResponse)
def ready() -> ReadyResponse:
    logging.debug("Health check")
    return ReadyResponse(
        ready=True,
    )


@router.get("/alive", response_model=AliveResponse)
def alive() -> AliveResponse:
    logging.debug("Health check")
    return AliveResponse(
        alive=True,
    )
