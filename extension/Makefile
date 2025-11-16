SQLX_OFFLINE:=true
DISTNAME = $(shell grep -m 1 '^name' Cargo.toml | sed -e 's/[^"]*"\([^"]*\)",\{0,1\}/\1/')
DISTVERSION  = $(shell grep -m 1 '^version' Cargo.toml | sed -e 's/[^"]*"\([^"]*\)",\{0,1\}/\1/')
PG_VERSION:=18
DATABASE_URL:=postgres://${USER}:${USER}@localhost:288${PG_VERSION}/postgres
PGRX_PG_CONFIG:=$(shell cargo pgrx info pg-config pg${PG_VERSION})
PGRX_VERSION := $(shell grep -E "^pgrx\s*=" Cargo.toml | cut -d'"' -f2 | tr -d '=')
UPGRADE_FROM_VER:=0.16.0
BRANCH:=$(git rev-parse --abbrev-ref HEAD)
RUST_LOG:=debug
ARCH := $(shell uname -m)

.PHONY: install-pg_cron install-pg_vector install-pgmq run setup test-integration test-unit test-version test-branch test-upgrade cat-logs docs

sqlx-cache:
	cargo sqlx prepare --database-url=${DATABASE_URL}

bench:
	DATABASE_URL=${DATABASE_URL} cargo bench -- --nocapture

format:
	SQLX_OFFLINE=${SQLX_OFFLINE} cargo fmt --all
	SQLX_OFFLINE=${SQLX_OFFLINE} cargo clippy

# ensure the DATABASE_URL is not used, since pgrx will stop postgres during compile
run:
	SQLX_OFFLINE=${SQLX_OFFLINE} DATABASE_URL=${DATABASE_URL} cargo pgrx run pg${PG_VERSION} postgres

META.json: META.json.in
	@sed "s/@CARGO_VERSION@/$(DISTVERSION)/g" META.json.in > META.json

# `git archive` only archives committed stuff, so use `git stash create` to
# create a temporary commit to archive.
$(DISTNAME)-$(DISTVERSION).zip: META.json
	git archive --format zip --prefix $(DISTNAME)-$(DISTVERSION)/ --add-file META.json -o $(DISTNAME)-$(DISTVERSION).zip HEAD

pgxn-zip: $(DISTNAME)-$(DISTVERSION).zip

clean:
	@rm -rf META.json $(DISTNAME)-$(DISTVERSION).zip

setup.dependencies: install-pg_cron install-pgvector install-pgmq install-vectorscale
setup.shared_preload_libraries:
	echo "shared_preload_libraries = 'pg_cron, vectorize'" >> ~/.pgrx/data-${PG_VERSION}/postgresql.conf
	echo "cron.database_name = 'postgres'" >> ~/.pgrx/data-${PG_VERSION}/postgresql.conf
setup.urls:
	echo "vectorize.embedding_service_url = 'http://localhost:3000/v1'" >> ~/.pgrx/data-${PG_VERSION}/postgresql.conf
	echo "vectorize.ollama_service_url = 'http://localhost:3001'" >> ~/.pgrx/data-${PG_VERSION}/postgresql.conf
setup: setup.dependencies setup.shared_preload_libraries setup.urls

cat-logs:
	cat ~/.pgrx/${PG_VERSION}.log

install-pg_cron:
	git clone https://github.com/citusdata/pg_cron.git && \
	cd pg_cron && \
	sed -i.bak 's/-Werror//g' Makefile && \
	PG_CONFIG=${PGRX_PG_CONFIG} make && \
	PG_CONFIG=${PGRX_PG_CONFIG} make install && \
	cd .. && rm -rf pg_cron

install-pgvector:
	git clone https://github.com/pgvector/pgvector.git && \
	cd pgvector && \
	PG_CONFIG=${PGRX_PG_CONFIG} make clean && \
	PG_CONFIG=${PGRX_PG_CONFIG} make && \
	PG_CONFIG=${PGRX_PG_CONFIG} make install && \
	cd .. && rm -rf pgvector

install-pgmq:
	git clone https://github.com/pgmq/pgmq.git && \
	cd pgmq/pgmq-extension && \
	PG_CONFIG=${PGRX_PG_CONFIG} make clean && \
	PG_CONFIG=${PGRX_PG_CONFIG} make && \
	PG_CONFIG=${PGRX_PG_CONFIG} make install && \
	cd ../.. && rm -rf pgmq

install-vectorscale:
	@ARCH=$$(uname -m); \
	if [ "$$ARCH" != "aarch64" ] && [ "$$ARCH" != "arm64" ]; then \
		export RUSTFLAGS="-C target-feature=+avx2,+fma"; \
	else \
		echo "Running on arm64, no compiler flags required."; \
	fi; \
	git clone https://github.com/timescale/pgvectorscale.git && \
	cd pgvectorscale/pgvectorscale && \
	cargo install cargo-pgrx --locked --version $$(grep -E "^pgrx\s*=" Cargo.toml | cut -d'"' -f2 | tr -d '=') && \
	cargo pgrx install --pg-config=${PGRX_PG_CONFIG} && \
	cd ../.. && rm -rf pgvectorscale
	cargo install cargo-pgrx --locked --version ${PGRX_VERSION}

test-integration:
	echo "\q" | make run
	cargo test ${TEST_NAME} -- --ignored --test-threads=1 --nocapture

test-unit:
	cargo test ${TEST_NAME} -- --test-threads=1 --nocapture

test-version:
	git fetch --tags
	git checkout tags/v${UPGRADE_FROM_VER}
	echo "\q" | make run
	psql ${DATABASE_URL} -c "DROP EXTENSION IF EXISTS vectorize"
	cargo test -- --ignored --test-threads=1

test-update:
	cargo install cargo-pgrx --locked --version ${PGRX_VERSION}
	echo "\q" | make run
	psql ${DATABASE_URL} -c "CREATE EXTENSION IF NOT EXISTS vectorscale"
	psql ${DATABASE_URL} -c "ALTER EXTENSION vectorize UPDATE"
	make test-unit
