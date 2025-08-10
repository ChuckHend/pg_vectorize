import os

def test_ready_endpoint(test_client):
    response = test_client.get("/ready")
    assert response.status_code == 200
    assert response.json() == {"ready": True}


def test_alive_endpoint(test_client):
    response = test_client.get("/alive")
    assert response.status_code == 200
    assert response.json() == {"alive": True}


def test_model_info(test_client):
    response = test_client.get(
        "/v1/info", params={"model_name": "sentence-transformers/all-MiniLM-L6-v2"}
    )
    assert response.status_code == 200


def test_metrics_endpoint(test_client):
    response = test_client.get("/metrics")
    assert response.status_code == 200
    assert "all-MiniLM-L6-v2" in response.text

def test_private_hf_model(test_client):
    response = test_client.post(
        "/v1/embeddings",
        headers={"authorization": f"bearer {os.environ['HF_API_KEY']}"},
        json={
            "input": ["string"],
            "model": "chuckhend/private-model",
            "normalize": False
        }
    )
    assert response.status_code == 200
    assert "data" in response.json()
    embed = response.json()["data"][0]["embedding"]
    assert isinstance(embed, list)
    assert len(embed) == 384