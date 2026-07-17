#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import numpy as np
import onnx
import onnxruntime as ort


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def node(item: Any) -> dict[str, Any]:
    return {"name": item.name, "shape": list(item.shape), "type": item.type}


def opset(path: Path) -> int:
    model = onnx.load(path, load_external_data=False)
    versions = [item.version for item in model.opset_import if item.domain in {"", "ai.onnx"}]
    if len(versions) != 1:
        raise ValueError(f"Expected one ONNX opset in {path}, got {versions}")
    return int(versions[0])


def inspect_model(
    path: Path,
    *,
    role: str,
    adapter: str,
    source: dict[str, Any],
    class_names: list[str],
    preprocessing: dict[str, Any],
    license_file: str,
    artifacts: list[Path] | None = None,
) -> dict[str, Any]:
    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    return {
        "name": source["name"],
        "role": role,
        "source": source["source"],
        "revision": source["revision"],
        "sourceSHA256": source["sourceSHA256"],
        "runtimeFile": path.name,
        "runtimeSHA256": sha256(path),
        "bytes": path.stat().st_size,
        "opset": opset(path),
        "inputs": [node(item) for item in session.get_inputs()],
        "outputs": [node(item) for item in session.get_outputs()],
        "adapter": adapter,
        "classNames": class_names,
        "preprocessing": preprocessing,
        "licenseFile": license_file,
        "testedProviders": ["CPUExecutionProvider"],
        "declaredLicense": source["declaredLicense"],
        "licenseVerificationStatus": source["licenseVerificationStatus"],
        "artifacts": [
            {"file": item.name, "sha256": sha256(item), "bytes": item.stat().st_size}
            for item in artifacts or []
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--sources", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    sources = json.loads(args.sources.read_text(encoding="utf-8"))
    labels = json.loads((args.models / "nudenet_labels.json").read_text(encoding="utf-8"))
    coco = json.loads((args.models / "coco_labels.json").read_text(encoding="utf-8"))
    prompt_path = args.models / "mobileclip2_s2_prompt_embeddings.npz"
    prompt_bundle = np.load(prompt_path, allow_pickle=False)
    prompt_names = [str(value) for value in prompt_bundle["class_names"].tolist()]
    if prompt_names != ["woman", "man", "uncertain", "notPerson"]:
        raise ValueError(f"Unexpected prompt class order: {prompt_names}")
    models = [
        inspect_model(
            args.models / "mobileclip2_s2_image_encoder.onnx",
            role="mobileCLIP2",
            adapter="mobileclip2-image",
            source=sources["mobileCLIP2"],
            class_names=prompt_names,
            preprocessing={
                "colorLayout": "RGB",
                "shape": [1, 3, 256, 256],
                "scale": 1 / 255,
                "mean": [0, 0, 0],
                "std": [1, 1, 1],
                "resize": "aspect-fill-center-crop",
                "embeddingDimension": 512,
                "embeddingNormalization": "L2",
            },
            license_file="licenses/MobileCLIP2-MODEL-LICENSE.md",
            artifacts=[prompt_path],
        ),
        inspect_model(
            args.models / "nudenet320n.onnx",
            role="nudeNet",
            adapter="nudenet-yolov8",
            source=sources["nudeNet"],
            class_names=labels,
            preprocessing={
                "colorLayout": "RGB",
                "shape": [1, 3, 320, 320],
                "scale": 1 / 255,
                "padding": "right-bottom-square",
            },
            license_file="licenses/NudeNet-LICENSE-AGPL-3.0.txt",
            artifacts=[args.models / "nudenet_labels.json"],
        ),
        inspect_model(
            args.models / "person_detector.onnx",
            role="personDetector",
            adapter="yolox",
            source=sources["personDetector"],
            class_names=coco,
            preprocessing={
                "colorLayout": "BGR",
                "shape": [1, 3, 416, 416],
                "scale": 1,
                "padding": "right-bottom-114",
                "outputLayout": "decoded-cxcywh-objectness-class-scores",
            },
            license_file="licenses/YOLOX-CODE-LICENSE.txt",
            artifacts=[args.models / "coco_labels.json"],
        ),
    ]
    manifest = {
        "schemaVersion": 1,
        "models": models,
        "sourcePackage": {
            "nudeNet": {
                "sourceVersion": sources["nudeNet"]["revision"],
                "sourceURL": sources["nudeNet"]["source"],
                "packageSHA256": sources["nudeNet"]["packageSHA256"],
                "modelSHA256": sources["nudeNet"]["modelSHA256"],
                "declaredLicense": sources["nudeNet"]["declaredLicense"],
                "licenseVerificationStatus": sources["nudeNet"]["licenseVerificationStatus"],
            }
        },
    }
    args.output.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
