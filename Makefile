.PHONY: docs

compose-server-up:
	docker compose -f server/docker-compose.yml up --build -d

docs:
	uv sync
	uv run mkdocs serve
