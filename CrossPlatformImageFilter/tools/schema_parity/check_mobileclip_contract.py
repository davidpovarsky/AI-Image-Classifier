#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "model_export"))
from mobileclip_contract import load_ios_contract  # noqa: E402

EXPECTED_MODEL_ID = "apple/MobileCLIP2-S2"
EXPECTED_MODEL_NAME = "MobileCLIP2-S2"
EXPECTED_IMAGE_SIZE = 256
EXPECTED_CLASS_ORDER = ["woman", "man", "uncertain", "notPerson"]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ios-converter", type=Path, required=True)
    parser.add_argument("--desktop-config", type=Path, required=True)
    args = parser.parse_args()
    contract = load_ios_contract(args.ios_converter)
    with args.desktop_config.open("rb") as stream:
        config = tomllib.load(stream)
    mobileclip = config["models"]["mobileclip"]
    actual = {
        "modelId": contract.model_id,
        "modelName": contract.model_name,
        "imageSize": contract.image_size,
        "classOrder": contract.class_names,
        "desktopImageSize": mobileclip["input_size"],
        "desktopClassOrder": mobileclip["class_names"],
    }
    expected = {
        "modelId": EXPECTED_MODEL_ID,
        "modelName": EXPECTED_MODEL_NAME,
        "imageSize": EXPECTED_IMAGE_SIZE,
        "classOrder": EXPECTED_CLASS_ORDER,
        "desktopImageSize": EXPECTED_IMAGE_SIZE,
        "desktopClassOrder": EXPECTED_CLASS_ORDER,
    }
    if actual != expected:
        raise SystemExit("MobileCLIP desktop/iOS contract drift:\n" + json.dumps(actual, indent=2))
    print(json.dumps(actual, indent=2))


if __name__ == "__main__":
    main()
