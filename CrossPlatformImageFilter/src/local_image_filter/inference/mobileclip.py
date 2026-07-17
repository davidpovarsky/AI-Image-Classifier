from __future__ import annotations

from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageOps

from ..domain.models import PersonClassification
from .base import InferenceError
from .onnx_session import OnnxSession


class MobileCLIPClassifier:
    def __init__(
        self,
        session: OnnxSession,
        config: dict[str, Any],
        prompt_embeddings_path: Path,
    ) -> None:
        if config.get("adapter") != "mobileclip2-image":
            raise InferenceError("MobileCLIP requires adapter='mobileclip2-image'")
        if len(session.inputs) != 1 or len(session.outputs) != 1:
            raise InferenceError("MobileCLIP requires exactly one input and one output")
        if session.inputs[0].type != "tensor(float)":
            raise InferenceError(f"MobileCLIP input must be float32, got {session.inputs[0].type}")
        if not prompt_embeddings_path.is_file():
            raise InferenceError(f"Prompt embeddings not found: {prompt_embeddings_path}")
        bundle = np.load(prompt_embeddings_path, allow_pickle=False)
        self.class_names = [str(item) for item in bundle["class_names"].tolist()]
        self.embeddings = np.asarray(bundle["embeddings"], dtype=np.float32)
        if self.embeddings.ndim != 2 or self.embeddings.shape[0] != len(self.class_names):
            raise InferenceError("Prompt embeddings must be [classCount, embeddingDimension]")
        if self.embeddings.shape[1] != 512 or not np.isfinite(self.embeddings).all():
            raise InferenceError("Prompt embeddings must be finite with dimension 512")
        self.embeddings /= np.maximum(np.linalg.norm(self.embeddings, axis=1, keepdims=True), 1e-12)
        configured_names = [str(item) for item in config.get("class_names", self.class_names)]
        if configured_names != self.class_names:
            raise InferenceError(
                f"Prompt class order mismatch: configured={configured_names}, file={self.class_names}"
            )
        self.session = session
        self.input_size = int(config.get("input_size", 256))
        self.input_name = str(config.get("input_name") or session.inputs[0].name)
        self.output_name = str(config.get("output_name") or session.outputs[0].name)
        if self.input_name != session.inputs[0].name or self.output_name != session.outputs[0].name:
            raise InferenceError("MobileCLIP input/output names do not match the ONNX model")
        self.image_scale = float(config.get("image_scale", 1.0 / 255.0))
        self.mean = np.asarray(config.get("mean", [0.0, 0.0, 0.0]), dtype=np.float32)
        self.std = np.asarray(config.get("std", [1.0, 1.0, 1.0]), dtype=np.float32)
        self.temperature = float(config.get("temperature", 100.0))

    def _tensor(self, image: Image.Image) -> np.ndarray:
        fitted = ImageOps.fit(
            image.convert("RGB"),
            (self.input_size, self.input_size),
            method=Image.Resampling.BICUBIC,
            centering=(0.5, 0.5),
        )
        values = np.asarray(fitted, dtype=np.float32) * self.image_scale
        values = (values - self.mean) / np.maximum(self.std, 1e-12)
        return np.transpose(values, (2, 0, 1))[None, ...]

    def classify(self, image: Image.Image, person_id: str, crop_id: str) -> PersonClassification:
        output = self.session.run(
            {self.input_name: self._tensor(image)},
            [self.output_name],
        )[0]
        embedding = np.asarray(output, dtype=np.float32).reshape(-1)
        if embedding.size != self.embeddings.shape[1]:
            raise InferenceError(
                f"MobileCLIP embedding dimension {embedding.size} does not match prompts {self.embeddings.shape[1]}"
            )
        finite = bool(np.isfinite(embedding).all())
        if not finite:
            raise InferenceError("MobileCLIP produced NaN or Infinity")
        norm = float(np.linalg.norm(embedding))
        embedding = embedding / max(norm, 1e-12)
        logits = self.temperature * (self.embeddings @ embedding)
        logits -= float(np.max(logits))
        probabilities = np.exp(logits)
        probabilities /= max(float(np.sum(probabilities)), 1e-12)
        if not np.isfinite(probabilities).all():
            raise InferenceError("MobileCLIP softmax produced NaN or Infinity")
        scores = {name: float(probabilities[index]) for index, name in enumerate(self.class_names)}
        winner_index = int(np.argmax(probabilities))
        return PersonClassification(
            person_id=person_id,
            crop_id=crop_id,
            predicted_class=self.class_names[winner_index],
            confidence=float(probabilities[winner_index]),
            scores=scores,
            embedding_dimension=int(embedding.size),
            embedding_norm=norm,
            embedding_finite=finite,
        )
