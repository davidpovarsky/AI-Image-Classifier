from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from PIL import Image

from ..domain.models import PersonClassification, PersonDetection, RawNudityDetection


class InferenceError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class DetectorBox:
    class_index: int
    confidence: float
    x: float
    y: float
    width: float
    height: float


class PersonDetectorProtocol(Protocol):
    def detect(self, image: Image.Image) -> list[PersonDetection]: ...


class NudeNetProtocol(Protocol):
    def detect(
        self,
        image: Image.Image,
        *,
        source: str,
        id_prefix: str,
        person_id: str | None = None,
        crop_id: str | None = None,
    ) -> list[RawNudityDetection]: ...


class MobileCLIPProtocol(Protocol):
    def classify(
        self, image: Image.Image, person_id: str, crop_id: str
    ) -> PersonClassification: ...
