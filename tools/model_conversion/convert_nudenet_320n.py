#!/usr/bin/env python3
from __future__ import annotations

import json
import shutil
from pathlib import Path

from ultralytics import YOLO

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools" / "model_conversion"
WEIGHTS = TOOLS / "320n.pt"
DESTINATION = ROOT / "AI-Image-Classifier" / "Models" / "NudeNet320n.mlpackage"
LABEL_MAP = ROOT / "AI-Image-Classifier" / "Models" / "NudeNet320n.labels.json"

LABELS = [
    "FEMALE_GENITALIA_COVERED", "FACE_FEMALE", "BUTTOCKS_EXPOSED",
    "FEMALE_BREAST_EXPOSED", "FEMALE_GENITALIA_EXPOSED",
    "MALE_BREAST_EXPOSED", "ANUS_EXPOSED", "FEET_EXPOSED",
    "BELLY_COVERED", "FEET_COVERED", "ARMPITS_COVERED",
    "ARMPITS_EXPOSED", "FACE_MALE", "BELLY_EXPOSED",
    "MALE_GENITALIA_EXPOSED", "ANUS_COVERED",
    "FEMALE_BREAST_COVERED", "BUTTOCKS_COVERED",
]


def main() -> None:
    if not WEIGHTS.is_file():
        raise SystemExit(f"Missing weights: {WEIGHTS}. Run download_model.sh first.")

    model = YOLO(str(WEIGHTS))
    actual_labels = [model.names[index] for index in sorted(model.names)]
    if actual_labels != LABELS:
        raise SystemExit(f"Unexpected label order: {actual_labels!r}")

    exported = model.export(
        format="coreml",
        imgsz=320,
        batch=1,
        nms=True,
        half=True,
    )
    exported_path = Path(str(exported)).resolve()
    if exported_path.suffix != ".mlpackage" or not exported_path.is_dir():
        raise SystemExit(f"Expected ML Program .mlpackage, got: {exported_path}")

    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    if DESTINATION.exists():
        shutil.rmtree(DESTINATION)
    shutil.copytree(exported_path, DESTINATION)
    LABEL_MAP.write_text(
        json.dumps({"model": "NudeNet320n", "inputSize": 320, "labels": LABELS}, indent=2)
        + "\n",
        encoding="utf-8",
    )
    print(f"Core ML package: {DESTINATION}")
    print(f"Bundled label map: {LABEL_MAP}")


if __name__ == "__main__":
    main()
