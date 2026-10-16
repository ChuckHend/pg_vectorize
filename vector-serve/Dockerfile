FROM ubuntu:24.04 AS builder

RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    gcc \
    libc6-dev \
    make && \
    rm -rf /var/lib/apt/lists/*

ARG PYTHON_VERSION=3.13.13
ARG PROJECT_NAME=vector-serve
ARG TORCH_VARIANT=cpu

ARG UV_PYTHON_INSTALL_DIR=/opt/python

ENV UV_PYTHON_INSTALL_DIR=${UV_PYTHON_INSTALL_DIR}
ENV PROJECT_HOME=/opt/${PROJECT_NAME}

ENV VIRTUAL_ENV=${PROJECT_HOME}/.venv
ENV PATH="${VIRTUAL_ENV}/bin:$PATH"

WORKDIR ${PROJECT_HOME}

COPY --from=ghcr.io/astral-sh/uv:latest /uv /bin/uv

ENV UV_LINK_MODE=copy

COPY vector-serve/pyproject.toml .
COPY vector-serve/uv.lock .
COPY vector-serve/app ./app

RUN uv venv --python "${PYTHON_VERSION}" && \
    uv sync --no-dev --frozen --extra "${TORCH_VARIANT}" && \
    uv run --no-sync python -m app.models && \
    uv cache clean

FROM ubuntu:24.04 AS runtime

RUN apt-get update && \
    apt-get install -y \
    ca-certificates \
    tzdata && \
    rm -rf /var/lib/apt/lists/*

ARG PROJECT_NAME=vector-serve
ARG UV_PYTHON_INSTALL_DIR=/opt/python

ENV PROJECT_HOME=/opt/${PROJECT_NAME}
ENV VIRTUAL_ENV=${PROJECT_HOME}/.venv
ENV PATH="${VIRTUAL_ENV}/bin:$PATH"

WORKDIR ${PROJECT_HOME}

COPY --from=builder ${UV_PYTHON_INSTALL_DIR} ${UV_PYTHON_INSTALL_DIR}
COPY --from=builder ${PROJECT_HOME} ${PROJECT_HOME}

CMD ["python", "-m", "app.server"]
