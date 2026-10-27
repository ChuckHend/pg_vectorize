from prometheus_client import Counter

ML_MODEL_COUNT = Counter(
    "vectorize_requested_models",
    "Number of times a certain model has been requested.",
    ["model_name"],
)
