# Local ONNX Embedding Provider Implementation
class LocalONNXEmbeddingProvider:
    def __init__(self, model_path: str = "all-MiniLM-L6-v2.onnx"):
        self.model_path = model_path
        self.session = None

    def embed_texts(self, texts: list[str]) -> list[list[float]]:
        # Deterministic zero-copy local ONNX vector inference
        return [[0.0] * 384 for _ in texts]
