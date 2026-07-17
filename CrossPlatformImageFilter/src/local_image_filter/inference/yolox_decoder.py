from __future__ import annotations

import math

import numpy as np
from PIL import Image

from .base import DetectorBox, InferenceError
from .detection_utils import non_maximum_suppression
from .onnx_session import OnnxSession


class YoloXDecoder:
    """Adapter for YOLOX exports with decoded cx/cy/w/h and objectness."""

    def __init__(
        self,
        session: OnnxSession,
        *,
        input_size: int,
        class_count: int,
        confidence_threshold: float,
        iou_threshold: float,
    ) -> None:
        if len(session.inputs) != 1 or len(session.outputs) != 1:
            raise InferenceError("YOLOX requires exactly one input and one output")
        if session.inputs[0].type != "tensor(float)":
            raise InferenceError(f"YOLOX input must be float32, got {session.inputs[0].type}")
        self.session = session
        self.input_size = input_size
        self.class_count = class_count
        self.confidence_threshold = confidence_threshold
        self.iou_threshold = iou_threshold
        self.input_name = session.inputs[0].name

    def _tensor(self, image: Image.Image) -> tuple[np.ndarray, float]:
        rgb = image.convert("RGB")
        scale = min(self.input_size / max(rgb.width, 1), self.input_size / max(rgb.height, 1))
        target = (max(1, int(rgb.width * scale)), max(1, int(rgb.height * scale)))
        resized = rgb.resize(target, Image.Resampling.BILINEAR)
        canvas = np.full((self.input_size, self.input_size, 3), 114, dtype=np.uint8)
        canvas[: resized.height, : resized.width] = np.asarray(resized)[:, :, ::-1]
        tensor = np.ascontiguousarray(canvas.transpose(2, 0, 1), dtype=np.float32)[None]
        return tensor, scale

    def detect(self, image: Image.Image) -> list[DetectorBox]:
        tensor, scale = self._tensor(image)
        output = np.asarray(self.session.run({self.input_name: tensor})[0])
        if output.ndim == 3 and output.shape[0] == 1:
            output = output[0]
        expected_attributes = 5 + self.class_count
        if output.ndim != 2 or output.shape[1] != expected_attributes:
            raise InferenceError(
                f"YOLOX expected [boxes,{expected_attributes}], received {tuple(output.shape)}"
            )
        if not np.isfinite(output).all():
            raise InferenceError("YOLOX output contains NaN or Infinity")

        results: list[DetectorBox] = []
        for row in output:
            objectness = float(row[4])
            class_scores = row[5:]
            class_index = int(np.argmax(class_scores))
            confidence = objectness * float(class_scores[class_index])
            if not math.isfinite(confidence) or not 0 <= confidence <= 1:
                raise InferenceError(f"YOLOX produced invalid confidence {confidence}")
            if confidence < self.confidence_threshold:
                continue
            cx, cy, width, height = (float(value) / max(scale, 1e-12) for value in row[:4])
            left = max(0.0, min(float(image.width), cx - width / 2))
            top = max(0.0, min(float(image.height), cy - height / 2))
            right = max(left, min(float(image.width), cx + width / 2))
            bottom = max(top, min(float(image.height), cy + height / 2))
            if right - left < 1 or bottom - top < 1:
                continue
            results.append(
                DetectorBox(
                    class_index=class_index,
                    confidence=confidence,
                    x=left / image.width,
                    y=top / image.height,
                    width=(right - left) / image.width,
                    height=(bottom - top) / image.height,
                )
            )
        return non_maximum_suppression(results, self.iou_threshold)
