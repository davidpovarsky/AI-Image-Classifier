from __future__ import annotations

import math
from dataclasses import asdict, dataclass, field
from enum import StrEnum
from typing import Any


class PipelineStatus(StrEnum):
    SUCCESS = "success"
    PARTIAL_SUCCESS = "partialSuccess"
    FAILED = "failed"
    SKIPPED = "skipped"


class FilterAction(StrEnum):
    ALLOW = "allow"
    BLUR = "blur"
    REPLACE = "replace"
    ERROR = "error"


@dataclass(frozen=True, slots=True)
class BoundingBox:
    """Normalized top-left box."""

    x: float
    y: float
    width: float
    height: float

    def __post_init__(self) -> None:
        values = (self.x, self.y, self.width, self.height)
        if not all(math.isfinite(value) for value in values):
            raise ValueError("Bounding-box coordinates must be finite")
        if self.width < 0 or self.height < 0:
            raise ValueError("Bounding-box width and height must not be negative")

    def clamped(self) -> BoundingBox:
        x = min(1.0, max(0.0, self.x))
        y = min(1.0, max(0.0, self.y))
        right = min(1.0, max(x, self.x + self.width))
        bottom = min(1.0, max(y, self.y + self.height))
        return BoundingBox(x=x, y=y, width=right - x, height=bottom - y)

    @property
    def area(self) -> float:
        return max(0.0, self.width) * max(0.0, self.height)

    @property
    def center(self) -> tuple[float, float]:
        return self.x + self.width / 2.0, self.y + self.height / 2.0

    def to_dict(self) -> dict[str, float]:
        return asdict(self)


@dataclass(frozen=True, slots=True)
class PersonDetection:
    person_id: str
    source: str
    confidence: float
    bounding_box: BoundingBox

    def __post_init__(self) -> None:
        if not math.isfinite(self.confidence) or not 0 <= self.confidence <= 1:
            raise ValueError("Person confidence must be finite and between 0 and 1")

    def to_dict(self) -> dict[str, Any]:
        return {
            "personId": self.person_id,
            "source": self.source,
            "confidence": self.confidence,
            "boundingBox": self.bounding_box.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class PersonClassification:
    person_id: str
    crop_id: str
    predicted_class: str
    confidence: float
    scores: dict[str, float]
    embedding_dimension: int
    embedding_norm: float
    embedding_finite: bool

    def __post_init__(self) -> None:
        if not math.isfinite(self.confidence) or not 0 <= self.confidence <= 1:
            raise ValueError("Classification confidence must be finite and between 0 and 1")
        if any(not math.isfinite(value) or not 0 <= value <= 1 for value in self.scores.values()):
            raise ValueError("Classification scores must be finite and between 0 and 1")

    def to_dict(self) -> dict[str, Any]:
        return {
            "personId": self.person_id,
            "cropId": self.crop_id,
            "predictedClass": self.predicted_class,
            "confidence": self.confidence,
            "scores": self.scores,
            "embedding": {
                "included": False,
                "dimension": self.embedding_dimension,
                "norm": self.embedding_norm,
                "finite": self.embedding_finite,
            },
        }


@dataclass(frozen=True, slots=True)
class RawNudityDetection:
    detection_id: str
    raw_label: str
    confidence: float
    source: str
    bounding_box: BoundingBox
    raw_model_index: int
    person_id: str | None = None
    crop_id: str | None = None
    bounding_box_in_crop: BoundingBox | None = None

    def __post_init__(self) -> None:
        if not math.isfinite(self.confidence) or not 0 <= self.confidence <= 1:
            raise ValueError("Nudity confidence must be finite and between 0 and 1")
        if self.raw_model_index < 0:
            raise ValueError("Nudity model index must not be negative")

    def to_dict(self) -> dict[str, Any]:
        return {
            "detectionId": self.detection_id,
            "rawLabel": self.raw_label,
            "confidence": self.confidence,
            "source": self.source,
            "boundingBox": self.bounding_box.to_dict(),
            "coordinateSystem": "normalized-top-left",
            "rawModelIndex": self.raw_model_index,
            "personId": self.person_id,
            "cropId": self.crop_id,
            "boundingBoxInCrop": (
                self.bounding_box_in_crop.to_dict() if self.bounding_box_in_crop else None
            ),
            "boundingBoxInOriginalImage": self.bounding_box.to_dict(),
        }


@dataclass(frozen=True, slots=True)
class MergedNudityDetection:
    merged_detection_id: str
    raw_label: str
    confidence: float
    bounding_box: BoundingBox
    sources: tuple[str, ...]
    source_detection_ids: tuple[str, ...]
    source_confidences: tuple[float, ...]
    person_ids: tuple[str, ...]
    maximum_iou: float
    merge_threshold: float

    def to_dict(self) -> dict[str, Any]:
        return {
            "mergedDetectionId": self.merged_detection_id,
            "rawLabel": self.raw_label,
            "confidence": self.confidence,
            "boundingBox": self.bounding_box.to_dict(),
            "sources": list(self.sources),
            "sourceDetectionIds": list(self.source_detection_ids),
            "sourceConfidences": list(self.source_confidences),
            "personIds": list(self.person_ids),
            "merge": {
                "algorithm": "sameLabelIoU",
                "threshold": self.merge_threshold,
                "maximumIoU": self.maximum_iou,
            },
        }


@dataclass(slots=True)
class ModuleReport:
    status: PipelineStatus = PipelineStatus.SUCCESS
    duration_ms: int = 0
    warnings: list[dict[str, Any]] = field(default_factory=list)
    error: dict[str, Any] | None = None
    details: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        result = {
            "status": self.status.value,
            "durationMs": self.duration_ms,
            "warnings": self.warnings,
            "error": self.error,
        }
        result.update(self.details)
        return result


@dataclass(frozen=True, slots=True)
class FilterDecision:
    action: FilterAction
    reasons: tuple[str, ...]
    evidence_ids: tuple[str, ...] = ()
    model_evidence_complete: bool = True

    def to_dict(self) -> dict[str, Any]:
        return {
            "action": self.action.value,
            "reasons": list(self.reasons),
            "evidenceIds": list(self.evidence_ids),
            "modelEvidenceComplete": self.model_evidence_complete,
        }


@dataclass(slots=True)
class FilterOutcome:
    content_hash: str
    response: dict[str, Any]
    decision: FilterDecision
    cache_hit: bool = False
    replacement_bytes: bytes | None = None
    replacement_mime_type: str | None = None
