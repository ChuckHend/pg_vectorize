"""Owns the worker-process count and per-worker CPU thread budget.

This is the single source of truth for VECTOR_SERVE_WORKERS -- both
app/server.py (which launches the worker processes) and this module's own
thread-limit math read the default from here, so there's one place to look
instead of a number buried in a Dockerfile comment.

torch/MKL/OpenMP and the Rust `tokenizers` crate (via Rayon) each default to
sizing their own thread pool off the total visible CPU count. That's fine for
a single process, but running multiple worker processes in the same container
means each one independently makes that same assumption -- so under
concurrent load they oversubscribe the pod's actual core count several times
over, causing context-switch thrashing that hurts both latency and throughput
instead of the intended per-process parallelism.

The OMP/MKL/RAYON env vars are read at first use by their respective native
libraries, so this module must be imported before torch or tokenizers are
imported anywhere in the process -- it is the first import in app.py.
"""

import os

# Conservative production default: bounds the memory cost of one model copy
# per worker. Override with the VECTOR_SERVE_WORKERS env var (e.g. sized from
# a Kubernetes pod's CPU request/limit) to match the CPU actually allocated
# to this container.
DEFAULT_WORKERS = 4


def resolve_worker_count() -> int:
    raw = os.getenv("VECTOR_SERVE_WORKERS")
    return max(1, int(raw)) if raw is not None else DEFAULT_WORKERS


def _available_cores() -> int:
    """Cores actually usable by this process.

    `sched_getaffinity` reflects a Kubernetes static-CPU-Manager cpuset when
    one is pinned; `os.cpu_count()` is the best available fallback elsewhere
    (e.g. under CFS-quota-only limits, or on macOS/Windows for local dev).
    """
    if hasattr(os, "sched_getaffinity"):
        return len(os.sched_getaffinity(0))
    return os.cpu_count() or 1


def configure_thread_limits() -> int:
    threads_per_worker = max(1, _available_cores() // resolve_worker_count())

    for var in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "RAYON_NUM_THREADS"):
        os.environ.setdefault(var, str(threads_per_worker))

    return threads_per_worker


THREADS_PER_WORKER = configure_thread_limits()
