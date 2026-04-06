from typing import Callable
import logging

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from app.routes.transform import router as transform_router
from app.routes.info import router as info_router
from app.routes.health import router as health_router

from app.models import load_model_cache

from prometheus_fastapi_instrumentator import Instrumentator


logging.basicConfig(level=logging.DEBUG)

app = FastAPI(title="Embedding-Service")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(transform_router)
app.include_router(info_router)
app.include_router(health_router)


def start_app_handler(app: FastAPI) -> Callable:
    def startup() -> None:
        logging.info("Running app start handler.")
        load_model_cache(app)

    return startup


app.add_event_handler("startup", start_app_handler(app))

instrumentator = Instrumentator().instrument(app)
instrumentator.expose(app)

if __name__ == "__main__":
    import uvicorn  # type: ignore

    uvicorn.run("app.app:app", host="0.0.0.0", port=3000, reload=True)
