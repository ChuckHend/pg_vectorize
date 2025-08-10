import os
import logging

from fastapi import FastAPI, HTTPException
from sentence_transformers import SentenceTransformer

from app.metrics import ML_MODEL_COUNT

LOCAL_FILES_ONLY = os.getenv("LOCAL_FILES_ONLY", "true").lower() in [
    "true",
    "1",
    "t",
    True,
]

_HF_ORG = "sentence-transformers"

MODELS_TO_CACHE = [f"{_HF_ORG}/all-MiniLM-L6-v2"]

cache_dir = "./models"

try:
    MULTI_MODEL = int(os.getenv("MULTI_MODEL", 1))
except Exception:
    logging.exception("Failed to parse MULTI_MODEL env var")
    MULTI_MODEL = 1


def parse_header(authorization: str | None) -> str | None:
    """parses hugging face token from the authorization header
    Returns None if the token is not a hugging face token"""
    if authorization is not None:
        token_value = authorization.split()[-1]
        is_hf_token = bool(token_value and token_value.startswith("hf_"))
        if is_hf_token:
            return token_value
    return None


def load_model_cache(app: FastAPI) -> dict[str, SentenceTransformer]:
    model_cache = {}
    for m in MODELS_TO_CACHE:
        model_cache[m] = SentenceTransformer(
            m, cache_folder=cache_dir, local_files_only=LOCAL_FILES_ONLY
        )
    app.state.model_cache = model_cache


def save_model_cache() -> None:
    """caches models to local storage"""
    for mod in MODELS_TO_CACHE:
        logging.debug(f"Caching model: {mod}")
        SentenceTransformer(mod, cache_folder=cache_dir)


def model_org_name(model_name: str) -> str:
    """prepends with the HF if the org is not specified"""
    if "/" not in model_name:
        return f"{_HF_ORG}/{model_name}"
    else:
        return model_name


def get_model(
    model_name: str, model_cache: dict[str, SentenceTransformer], api_key: str = None
) -> SentenceTransformer:
    model = model_cache.get(model_name)
    if model is None:
        if not MULTI_MODEL:
            raise HTTPException(
                status_code=400,
                detail="Must enable multi-model via MULTI_MODEL env var",
            )
        # try to download from HF when MULTI_MODEL enabled
        # and model not in cache
        logging.debug(f"Model: {model_name} not in cache.")
        # fall back to env var for Hugging Face API key, if it exists
        token = os.getenv("HF_API_KEY") or api_key
        try:
            model = SentenceTransformer(model_name, token=token, trust_remote_code=True)
            # add model to cache
            model_cache[model_name] = model
            logging.debug(f"Added model: {model_name} to cache.")
        except Exception:
            if api_key is None:
                logging.warning("No api_key provided for model: %s", model_name)
            logging.exception("Failed to load model %s", model_name)
            raise
    ML_MODEL_COUNT.labels(model_name=model_name).inc()
    return model


if __name__ == "__main__":
    save_model_cache()
