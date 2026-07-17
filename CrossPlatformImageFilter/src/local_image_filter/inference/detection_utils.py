from __future__ import annotations

from collections.abc import Iterable

from .base import DetectorBox


def intersection_over_union(a: DetectorBox, b: DetectorBox) -> float:
    left = max(a.x, b.x)
    top = max(a.y, b.y)
    right = min(a.x + a.width, b.x + b.width)
    bottom = min(a.y + a.height, b.y + b.height)
    intersection = max(0.0, right - left) * max(0.0, bottom - top)
    union = a.width * a.height + b.width * b.height - intersection
    return intersection / union if union > 0 else 0.0


def non_maximum_suppression(boxes: Iterable[DetectorBox], threshold: float) -> list[DetectorBox]:
    kept: list[DetectorBox] = []
    for candidate in sorted(boxes, key=lambda item: item.confidence, reverse=True):
        if all(
            candidate.class_index != previous.class_index
            or intersection_over_union(candidate, previous) < threshold
            for previous in kept
        ):
            kept.append(candidate)
    return kept
