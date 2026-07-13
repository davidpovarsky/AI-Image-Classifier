#!/usr/bin/env python3
from __future__ import annotations

import json
import os
from pathlib import Path

import coremltools as ct

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = ROOT / "AI-Image-Classifier" / "Models" / "NudeNet320n.mlpackage"
LABEL_MAP = ROOT / "AI-Image-Classifier" / "Models" / "NudeNet320n.labels.json"
EXPECTED_LABELS = [
    "FEMALE_GENITALIA_COVERED", "FACE_FEMALE", "BUTTOCKS_EXPOSED",
    "FEMALE_BREAST_EXPOSED", "FEMALE_GENITALIA_EXPOSED",
    "MALE_BREAST_EXPOSED", "ANUS_EXPOSED", "FEET_EXPOSED",
    "BELLY_COVERED", "FEET_COVERED", "ARMPITS_COVERED",
    "ARMPITS_EXPOSED", "FACE_MALE", "BELLY_EXPOSED",
    "MALE_GENITALIA_EXPOSED", "ANUS_COVERED",
    "FEMALE_BREAST_COVERED", "BUTTOCKS_COVERED",
]


def directory_size(path: Path) -> int:
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def main() -> None:
    if not PACKAGE.is_dir() or not any(PACKAGE.rglob("*")):
        raise SystemExit(f"Missing or empty model package: {PACKAGE}")
    label_data = json.loads(LABEL_MAP.read_text(encoding="utf-8"))
    if label_data != {"model": "NudeNet320n", "inputSize": 320, "labels": EXPECTED_LABELS}:
        raise SystemExit("Bundled label map does not match the canonical 18 NudeNet labels")

    model = ct.models.MLModel(str(PACKAGE), compute_units=ct.ComputeUnit.ALL)
    spec = model.get_spec()
    inputs = list(spec.description.input)
    outputs = list(spec.description.output)
    if len(inputs) != 1 or not inputs[0].type.HasField("imageType"):
        raise SystemExit(f"Expected one image input, got {[item.name for item in inputs]}")
    image = inputs[0].type.imageType
    if (image.width, image.height) != (320, 320):
        raise SystemExit(f"Expected 320x320 input, got {image.width}x{image.height}")
    output_names = [item.name for item in outputs]
    if not outputs or not ({"confidence", "coordinates"} <= set(output_names)):
        raise SystemExit(f"Expected post-NMS detection outputs, got {output_names}")

    model_type = spec.WhichOneof("Type")
    if model_type not in {"mlProgram", "pipeline"}:
        raise SystemExit(f"Expected ML Program or pipeline, got {model_type}")
    metadata = dict(model.user_defined_metadata)
    print(json.dumps({
        "package": os.fspath(PACKAGE.relative_to(ROOT)),
        "sizeBytes": directory_size(PACKAGE),
        "inputNames": [item.name for item in inputs],
        "inputSize": [image.width, image.height],
        "batchSize": 1,
        "outputNames": output_names,
        "modelType": model_type,
        "precision": "float16 requested during export",
        "computeUnits": "all",
        "metadata": metadata,
        "labels": EXPECTED_LABELS,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
