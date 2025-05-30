RUST_LOG:=info

include .env

run-server:
	RUST_LOG=$(RUST_LOG) DATABASE_URL=$(DATABASE_URL) cargo run --bin vectorize-server

run-worker:
	RUST_LOG=$(RUST_LOG) cargo run --bin vectorize-worker
