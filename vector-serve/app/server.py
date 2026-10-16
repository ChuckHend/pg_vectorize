"""Production entrypoint.

Launches uvicorn directly instead of going through the `fastapi run` CLI, so
the worker count is a plain Python value read from application config
(app/threading_config.py) rather than a parameter threaded through the
Dockerfile CMD. `uvicorn.run`'s own defaults already match what `fastapi run`
uses (proxy_headers=True, reload=False), so behavior is unchanged.
"""

import uvicorn

from app.threading_config import resolve_worker_count


def main() -> None:
    uvicorn.run(
        "app.app:app",
        host="0.0.0.0",
        port=3000,
        workers=resolve_worker_count(),
    )


if __name__ == "__main__":
    main()
