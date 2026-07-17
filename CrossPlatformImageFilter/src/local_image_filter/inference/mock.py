from __future__ import annotations

from PIL import Image

from ..domain.models import PersonClassification, PersonDetection, RawNudityDetection


class MockPersonDetector:
    def detect(self, image: Image.Image) -> list[PersonDetection]:
        return []


class MockNudeNetDetector:
    labels: list[str] = []

    def detect(
        self,
        image: Image.Image,
        *,
        source: str,
        id_prefix: str,
        person_id: str | None = None,
        crop_id: str | None = None,
    ) -> list[RawNudityDetection]:
        return []


class MockMobileCLIPClassifier:
    def classify(self, image: Image.Image, person_id: str, crop_id: str) -> PersonClassification:
        return PersonClassification(
            person_id=person_id,
            crop_id=crop_id,
            predicted_class="notPerson",
            confidence=1.0,
            scores={"woman": 0.0, "man": 0.0, "uncertain": 0.0, "notPerson": 1.0},
            embedding_dimension=512,
            embedding_norm=1.0,
            embedding_finite=True,
        )
