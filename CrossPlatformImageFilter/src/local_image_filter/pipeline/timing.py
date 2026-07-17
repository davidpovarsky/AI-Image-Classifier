from __future__ import annotations

from collections.abc import Iterator
from contextlib import contextmanager
from time import perf_counter


@contextmanager
def measured(target: dict[str, int], key: str) -> Iterator[None]:
    started = perf_counter()
    try:
        yield
    finally:
        target[key] = round((perf_counter() - started) * 1000)
