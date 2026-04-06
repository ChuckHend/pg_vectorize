SOURCE_OBJECTS=app

format:
	uv run ruff check --silent --fix --exit-zero ${SOURCE_OBJECTS}

download.models:
	uv run python -m app.models

run: download.models
	uv run fastapi run app/app.py --host 0.0.0.0 --port 3000 --workers 1

run.docker:
	docker build -t vector-serve .
	docker run -p 3000:3000 vector-serve

test:
	uv run pytest

setup:
	curl -LsSf https://astral.sh/uv/install.sh | sh
	uv sync

check:
	uv run ruff check ${SOURCE_OBJECTS}
