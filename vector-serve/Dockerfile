FROM ubuntu:24.10

RUN apt-get update && \
    apt-get install -y \
    curl \
    ca-certificates \
    tzdata \
    git \
    gcc \
    libc6-dev \
    make && \
    rm -rf /var/lib/apt/lists/*

ARG PYTHON_VERSION=3.12.4
ARG PROJECT_NAME=vector-serve

ARG UV_PYTHON_INSTALL_DIR=/opt/python

ENV UV_PYTHON_INSTALL_DIR=${UV_PYTHON_INSTALL_DIR}
ENV PROJECT_HOME=/opt/${PROJECT_NAME}

ENV VIRTUAL_ENV=${PROJECT_HOME}/.venv
ENV PATH="${VIRTUAL_ENV}/bin:$PATH"

WORKDIR ${PROJECT_HOME}

COPY --from=ghcr.io/astral-sh/uv:latest /uv /bin/uv

ENV UV_LINK_MODE=copy

COPY vector-serve .

RUN uv venv --python "${PYTHON_VERSION}" && \
    uv sync --no-dev --frozen && \
    uv run python -m app.models && \
    uv cache clean

CMD ["fastapi", "run", "app/app.py", "--host", "0.0.0.0", "--port", "3000", "--workers", "1"]
