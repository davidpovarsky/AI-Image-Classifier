from __future__ import annotations

import threading
from pathlib import Path
from typing import Any

import numpy as np

from .base import InferenceError


class OnnxSession:
    def __init__(
        self,
        model_path: Path,
        provider_priority: list[str],
        intra_op_threads: int = 0,
        inter_op_threads: int = 1,
        expected_metadata: dict[str, Any] | None = None,
    ) -> None:
        if not model_path.is_file():
            raise InferenceError(f"ONNX model not found: {model_path}")
        try:
            import onnxruntime as ort
        except ImportError as error:
            raise InferenceError("onnxruntime is not installed") from error

        available = list(ort.get_available_providers())
        selected = [provider for provider in provider_priority if provider in available]
        if "CPUExecutionProvider" not in selected and "CPUExecutionProvider" in available:
            selected.append("CPUExecutionProvider")
        if not selected:
            raise InferenceError(
                f"No requested ONNX providers are available. Available: {sorted(available)}"
            )

        options = ort.SessionOptions()
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        uses_directml = "DmlExecutionProvider" in selected
        options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        options.enable_mem_pattern = not uses_directml
        options.enable_cpu_mem_arena = True
        if intra_op_threads > 0:
            options.intra_op_num_threads = intra_op_threads
        if inter_op_threads > 0:
            options.inter_op_num_threads = inter_op_threads

        self._ort = ort
        self._lock = threading.Lock() if uses_directml else None
        self.path = model_path
        self.providers = selected
        try:
            self.session = ort.InferenceSession(
                str(model_path),
                sess_options=options,
                providers=selected,
            )
        except Exception as error:
            raise InferenceError(f"Unable to load ONNX model {model_path.name}: {error}") from error
        self.inputs = self.session.get_inputs()
        self.outputs = self.session.get_outputs()
        self._validate_metadata(expected_metadata)

    @staticmethod
    def _normalized_shape(shape: list[Any]) -> list[int | str | None]:
        return [value if isinstance(value, (int, str)) else None for value in shape]

    def _validate_metadata(self, expected: dict[str, Any] | None) -> None:
        if expected is None:
            return
        actual_inputs = [
            {"name": item.name, "shape": self._normalized_shape(item.shape), "type": item.type}
            for item in self.inputs
        ]
        actual_outputs = [
            {"name": item.name, "shape": self._normalized_shape(item.shape), "type": item.type}
            for item in self.outputs
        ]
        for key, actual in (("inputs", actual_inputs), ("outputs", actual_outputs)):
            declared = expected.get(key)
            if not isinstance(declared, list) or not declared:
                raise InferenceError(f"Runtime manifest has no {key} for {self.path.name}")
            if len(declared) != len(actual):
                raise InferenceError(
                    f"{self.path.name} {key} count mismatch: manifest={len(declared)}, model={len(actual)}"
                )
            for index, (wanted, found) in enumerate(zip(declared, actual, strict=True)):
                if wanted.get("name") != found["name"]:
                    raise InferenceError(
                        f"{self.path.name} {key}[{index}] name mismatch: "
                        f"manifest={wanted.get('name')}, model={found['name']}"
                    )
                wanted_shape = wanted.get("shape")
                if wanted_shape and list(wanted_shape) != found["shape"]:
                    raise InferenceError(
                        f"{self.path.name} {key}[{index}] shape mismatch: "
                        f"manifest={wanted_shape}, model={found['shape']}"
                    )

    def run(self, feeds: dict[str, Any], output_names: list[str] | None = None) -> list[Any]:
        try:
            if self._lock is None:
                return list(self.session.run(output_names, feeds))
            with self._lock:
                return list(self.session.run(output_names, feeds))
        except Exception as error:  # ONNX Runtime exposes provider-specific exception types.
            raise InferenceError(f"ONNX inference failed for {self.path.name}: {error}") from error

    def warm_up(self, input_shapes: dict[str, tuple[int, ...]] | None = None) -> None:
        feeds: dict[str, np.ndarray] = {}
        for item in self.inputs:
            override = (input_shapes or {}).get(item.name)
            shape = override if override is not None else tuple(item.shape or ())
            if not shape or any(not isinstance(value, int) or value < 1 for value in shape):
                raise InferenceError(f"Cannot warm up dynamic input {item.name}: {item.shape}")
            dtype = np.float32 if item.type == "tensor(float)" else np.int64
            feeds[item.name] = np.zeros(shape, dtype=dtype)
        outputs = self.run(feeds)
        for output in outputs:
            values = np.asarray(output)
            if not np.isfinite(values).all():
                raise InferenceError(f"Warm-up produced NaN or Infinity for {self.path.name}")

    def describe(self) -> dict[str, Any]:
        return {
            "file": self.path.name,
            "providers": self.providers,
            "inputs": [
                {"name": item.name, "shape": list(item.shape), "type": item.type}
                for item in self.inputs
            ],
            "outputs": [
                {"name": item.name, "shape": list(item.shape), "type": item.type}
                for item in self.outputs
            ],
        }
