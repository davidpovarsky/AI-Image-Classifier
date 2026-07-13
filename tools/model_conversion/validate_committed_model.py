#!/usr/bin/env python3
from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODELS = ROOT / "AI-Image-Classifier" / "Models"
PACKAGE = MODELS / "NudeNet320n.mlpackage"
LABEL_MAP = MODELS / "NudeNet320n.labels.json"
EXPECTED = [
    "FEMALE_GENITALIA_COVERED", "FACE_FEMALE", "BUTTOCKS_EXPOSED",
    "FEMALE_BREAST_EXPOSED", "FEMALE_GENITALIA_EXPOSED", "MALE_BREAST_EXPOSED",
    "ANUS_EXPOSED", "FEET_EXPOSED", "BELLY_COVERED", "FEET_COVERED",
    "ARMPITS_COVERED", "ARMPITS_EXPOSED", "FACE_MALE", "BELLY_EXPOSED",
    "MALE_GENITALIA_EXPOSED", "ANUS_COVERED", "FEMALE_BREAST_COVERED",
    "BUTTOCKS_COVERED",
]

if not PACKAGE.is_dir():
    raise SystemExit(f"Missing {PACKAGE.relative_to(ROOT)}")
files = [path for path in PACKAGE.rglob("*") if path.is_file()]
if not files or sum(path.stat().st_size for path in files) < 1_000_000:
    raise SystemExit("NudeNet320n.mlpackage is empty or unexpectedly small")
manifest = json.loads((PACKAGE / "Manifest.json").read_text(encoding="utf-8"))
if not manifest.get("itemInfoEntries"):
    raise SystemExit("Invalid ML package manifest")
metadata = json.loads(LABEL_MAP.read_text(encoding="utf-8"))
if metadata != {"model": "NudeNet320n", "inputSize": 320, "labels": EXPECTED}:
    raise SystemExit("Unexpected NudeNet label metadata")
print(f"Validated committed NudeNet320n.mlpackage ({sum(p.stat().st_size for p in files)} bytes)")
