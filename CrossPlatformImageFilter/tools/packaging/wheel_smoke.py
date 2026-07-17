#!/usr/bin/env python3
from __future__ import annotations

import argparse
import io
import json
import subprocess
import sys
import tempfile
import venv
from pathlib import Path

from PIL import Image


def executable(environment: Path, name: str) -> Path:
    scripts = environment / ("Scripts" if sys.platform == "win32" else "bin")
    suffix = ".exe" if sys.platform == "win32" else ""
    return scripts / f"{name}{suffix}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--mock-config", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="local-image-filter-wheel-") as directory:
        environment = Path(directory) / "venv"
        venv.EnvBuilder(with_pip=True).create(environment)
        python = executable(environment, "python")
        subprocess.run([str(python), "-m", "pip", "install", str(args.wheel.resolve())], check=True)
        command = executable(environment, "local-image-filter")
        config = subprocess.run(
            [str(command), "print-config"], check=True, text=True, capture_output=True
        )
        doctor = subprocess.run(
            [str(command), "doctor", "--config", str(args.mock_config.resolve())],
            check=True,
            text=True,
            capture_output=True,
        )
        image_path = Path(directory) / "synthetic.png"
        stream = io.BytesIO()
        Image.new("RGB", (128, 96), "white").save(stream, format="PNG")
        image_path.write_bytes(stream.getvalue())
        classification = subprocess.run(
            [
                str(command),
                "classify",
                "--config",
                str(args.mock_config.resolve()),
                "--mime-type",
                "image/png",
                str(image_path),
            ],
            check=True,
            text=True,
            capture_output=True,
        )
        report = {
            "wheel": args.wheel.name,
            "packagedDefault": json.loads(config.stdout),
            "doctor": json.loads(doctor.stdout),
            "classification": json.loads(classification.stdout),
        }
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
