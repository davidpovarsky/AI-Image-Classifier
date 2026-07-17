from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from PIL import Image

from ..domain.models import BoundingBox, RawNudityDetection
from .base import InferenceError
from .nudenet_decoder import NudeNetYoloV8Decoder
from .onnx_session import OnnxSession


class NudeNetDetector:
    def __init__(self, session: OnnxSession, config: dict[str, Any], labels_path: Path) -> None:
        if not labels_path.is_file():
            raise InferenceError(f"NudeNet labels not found: {labels_path}")
        labels = json.loads(labels_path.read_text(encoding="utf-8"))
        if not isinstance(labels, list) or not all(isinstance(item, str) for item in labels):
            raise InferenceError("NudeNet labels must be a JSON string array")
        self.labels: list[str] = labels
        if config.get("adapter") != "nudenet-yolov8":
            raise InferenceError("NudeNet requires adapter='nudenet-yolov8'")
        if len(self.labels) != 18:
            raise InferenceError(f"NudeNet requires exactly 18 labels, got {len(self.labels)}")
        self.detector = NudeNetYoloV8Decoder(
            session=session,
            input_size=int(config.get("input_size", 320)),
            class_count=len(self.labels),
            confidence_threshold=float(config.get("confidence_threshold", 0.20)),
            iou_threshold=float(config.get("iou_threshold", 0.45)),
        )

    def detect(
        self,
        image: Image.Image,
        *,
        source: str,
        id_prefix: str,
        person_id: str | None = None,
        crop_id: str | None = None,
    ) -> list[RawNudityDetection]:
        detections = self.detector.detect(image)
        results = [
            RawNudityDetection(
                detection_id=f"{id_prefix}-{index}",
                raw_label=self.labels[item.class_index],
                confidence=item.confidence,
                source=source,
                bounding_box=BoundingBox(item.x, item.y, item.width, item.height).clamped(),
                raw_model_index=item.class_index,
                person_id=person_id,
                crop_id=crop_id,
                bounding_box_in_crop=(
                    BoundingBox(item.x, item.y, item.width, item.height).clamped()
                    if source == "personCrop"
                    else None
                ),
            )
            for index, item in enumerate(detections, start=1)
        ]
        if any(not 0 <= item.raw_model_index < len(self.labels) for item in results):
            raise InferenceError("NudeNet produced an invalid class index")
        return results
