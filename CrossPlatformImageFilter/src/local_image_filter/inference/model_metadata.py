from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .base import InferenceError


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass(frozen=True, slots=True)
class RuntimeManifest:
    path: Path
    data: dict[str, Any]
    fingerprint: str

    @classmethod
    def load(cls, path: Path) -> RuntimeManifest:
        if not path.is_file():
            raise InferenceError(f"Runtime manifest not found: {path}")
        raw = path.read_bytes()
        try:
            data = json.loads(raw)
        except json.JSONDecodeError as error:
            raise InferenceError(f"Invalid runtime manifest JSON: {error}") from error
        if data.get("schemaVersion") != 1 or not isinstance(data.get("models"), list):
            raise InferenceError("Runtime manifest must use schemaVersion 1 and contain models[]")
        manifest = cls(path=path, data=data, fingerprint=hashlib.sha256(raw).hexdigest())
        manifest.verify_files()
        return manifest

    @property
    def root(self) -> Path:
        return self.path.parent

    def model(self, role: str) -> dict[str, Any]:
        matches = [item for item in self.data["models"] if item.get("role") == role]
        if len(matches) != 1:
            raise InferenceError(f"Runtime manifest must contain exactly one {role!r} model")
        return dict(matches[0])

    def verify_files(self) -> None:
        for item in self.data["models"]:
            runtime_file = item.get("runtimeFile")
            expected_hash = item.get("runtimeSHA256")
            if not isinstance(runtime_file, str) or not isinstance(expected_hash, str):
                raise InferenceError("Every model needs runtimeFile and runtimeSHA256")
            path = (self.root / runtime_file).resolve()
            if path.parent != self.root.resolve():
                raise InferenceError(f"Runtime model path escapes model directory: {runtime_file}")
            if not path.is_file():
                raise InferenceError(f"Runtime model is missing: {path}")
            actual_hash = sha256_file(path)
            if actual_hash != expected_hash.lower():
                raise InferenceError(
                    f"Runtime model hash mismatch for {runtime_file}: "
                    f"expected {expected_hash}, got {actual_hash}"
                )
            declared_bytes = item.get("bytes")
            if declared_bytes != path.stat().st_size:
                raise InferenceError(
                    f"Runtime model size mismatch for {runtime_file}: "
                    f"expected {declared_bytes}, got {path.stat().st_size}"
                )
            artifacts = item.get("artifacts", [])
            if not isinstance(artifacts, list):
                raise InferenceError(f"Model artifacts must be a list for {runtime_file}")
            for artifact in artifacts:
                name = artifact.get("file")
                artifact_hash = artifact.get("sha256")
                if not isinstance(name, str) or not isinstance(artifact_hash, str):
                    raise InferenceError(f"Invalid auxiliary artifact metadata for {runtime_file}")
                artifact_path = (self.root / name).resolve()
                if artifact_path.parent != self.root.resolve() or not artifact_path.is_file():
                    raise InferenceError(f"Runtime artifact is missing or unsafe: {name}")
                actual_artifact_hash = sha256_file(artifact_path)
                if actual_artifact_hash != artifact_hash.lower():
                    raise InferenceError(
                        f"Runtime artifact hash mismatch for {name}: "
                        f"expected {artifact_hash}, got {actual_artifact_hash}"
                    )
                if artifact.get("bytes") != artifact_path.stat().st_size:
                    raise InferenceError(f"Runtime artifact size mismatch for {name}")
