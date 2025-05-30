FROM rust:1.87.0-slim-bookworm AS builder

RUN apt-get update && \
    apt-get install -y curl pkg-config libssl-dev postgresql-client && apt-get clean && \
    rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

WORKDIR /build

COPY .sqlx .sqlx/
COPY src src/
COPY Cargo.toml Cargo.lock ./

ENV SQLX_OFFLINE=1
RUN cargo build --bin vectorize-worker --release
RUN cargo build --bin vectorize-server --release

FROM rust:1.87.0-slim-bookworm

RUN apt-get update && \
    apt-get install -y postgresql-client && apt-get clean && \
    rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

COPY --from=builder /build/target/release/vectorize-worker /usr/local/bin/vectorize-worker
COPY --from=builder /build/target/release/vectorize-server /usr/local/bin/vectorize-server
