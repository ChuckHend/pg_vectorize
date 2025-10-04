FROM rust:1.90.0-slim-bookworm AS builder

RUN apt-get update && \
    apt-get install -y curl pkg-config libssl-dev postgresql-client && apt-get clean && \
    rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

WORKDIR /build

COPY core core/
COPY worker worker/
COPY server server/
COPY proxy proxy

COPY Cargo.toml Cargo.lock ./

ENV SQLX_OFFLINE=1
RUN cargo build --bin vectorize-server --release

FROM rust:1.90.0-slim-bookworm

RUN apt-get update && \
    apt-get install -y postgresql-client && apt-get clean && \
    rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/*

COPY --from=builder /build/target/release/vectorize-server /usr/local/bin/vectorize-server

CMD ["vectorize-server"]