FROM pytorch/pytorch:2.1.0-cuda12.1-cudnn8-runtime

WORKDIR /usr/src/app

RUN apt-get update && \
    apt-get install -y curl

RUN pip install \
    fastapi==0.104.0 \
    uvicorn[standard]==0.23.2 \
    sentence-transformers==2.2.2

# Download models, save to image cache
COPY app/models.py .
RUN python models.py

COPY . .

CMD ["uvicorn", "app.app:app", "--host", "0.0.0.0", "--port", "3000", "--workers", "2"]
