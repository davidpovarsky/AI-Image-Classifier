from __future__ import annotations

import math

import numpy as np
from PIL import Image

from .base import DetectorBox, InferenceError
from .detection_utils import non_maximum_suppression
from .onnx_session import OnnxSession


class NudeNetYoloV8Decoder:
    """Adapter for NudeNet320n's channels-first YOLOv8 output."""

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
            raise InferenceError("NudeNet requires exactly one input and one output")
        if session.inputs[0].type != "tensor(float)":
            raise InferenceError(f"NudeNet input must be float32, got {session.inputs[0].type}")
        self.session = session
        self.input_size = input_size
        self.class_count = class_count
        self.confidence_threshold = confidence_threshold
        self.iou_threshold = iou_threshold
        self.input_name = session.inputs[0].name

    def _tensor(self, image: Image.Image) -> tuple[np.ndarray, int]:
        rgb = image.convert("RGB")
        square_size = max(rgb.width, rgb.height)
        square = Image.new("RGB", (square_size, square_size), (0, 0, 0))
        square.paste(rgb, (0, 0))
        resized = square.resize((self.input_size, self.input_size), Image.Resampling.BILINEAR)
        values = np.asarray(resized, dtype=np.float32) / 255.0
        return np.ascontiguousarray(values.transpose(2, 0, 1))[None], square_size

    def detect(self, image: Image.Image) -> list[DetectorBox]:
        tensor, square_size = self._tensor(image)
        output = np.asarray(self.session.run({self.input_name: tensor})[0])
        expected_channels = 4 + self.class_count
        if output.ndim != 3 or output.shape[0] != 1 or output.shape[1] != expected_channels:
            raise InferenceError(
                f"NudeNet expected [1,{expected_channels},boxes], received {tuple(output.shape)}"
            )
        rows = output[0].T
        if not np.isfinite(rows).all():
            raise InferenceError("NudeNet output contains NaN or Infinity")

        scale = square_size / self.input_size
        results: list[DetectorBox] = []
        for row in rows:
            class_scores = row[4:]
            class_index = int(np.argmax(class_scores))
            confidence = float(class_scores[class_index])
            if not math.isfinite(confidence) or not 0 <= confidence <= 1:
                raise InferenceError(f"NudeNet produced invalid confidence {confidence}")
            if confidence < self.confidence_threshold:
                continue
            cx, cy, width, height = (float(value) * scale for value in row[:4])
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
