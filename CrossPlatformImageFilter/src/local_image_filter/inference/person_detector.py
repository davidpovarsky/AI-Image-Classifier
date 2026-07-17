from __future__ import annotations

from typing import Any

from PIL import Image

from ..domain.models import BoundingBox, PersonDetection
from .onnx_session import OnnxSession
from .yolox_decoder import YoloXDecoder


class PersonDetector:
    def __init__(self, session: OnnxSession, config: dict[str, Any]) -> None:
        self.class_index = int(config.get("class_index", 0))
        class_count = int(config.get("class_count", 80))
        if config.get("adapter") != "yolox":
            raise ValueError("Person detector requires adapter='yolox'")
        self.detector = YoloXDecoder(
            session=session,
            input_size=int(config.get("input_size", 640)),
            class_count=class_count,
            confidence_threshold=float(config.get("confidence_threshold", 0.30)),
            iou_threshold=float(config.get("iou_threshold", 0.50)),
        )

    def detect(self, image: Image.Image) -> list[PersonDetection]:
        results = [
            item for item in self.detector.detect(image) if item.class_index == self.class_index
        ]
        results.sort(key=lambda item: item.width * item.height, reverse=True)
        return [
            PersonDetection(
                person_id=f"person-{index}",
                source="yoloPersonDetector",
                confidence=item.confidence,
                bounding_box=BoundingBox(item.x, item.y, item.width, item.height).clamped(),
            )
            for index, item in enumerate(results, start=1)
        ]
