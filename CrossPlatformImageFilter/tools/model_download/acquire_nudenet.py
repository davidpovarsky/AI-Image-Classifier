#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

VERSION = "3.4.2"
WHEEL_NAME = "nudenet-3.4.2-py3-none-any.whl"
WHEEL_SHA256 = "5937dbd84e5d8e5de038f08ffea5a1bb50a08475776bf2b4795914ce0eaf0331"
MODEL_SHA256 = "c15d8273adad2d0a92f014cc69ab2d6c311a06777a55545f2c4eb46f51911f0f"


def sha256_bytes(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--licenses", type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.licenses.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="nudenet-wheel-") as directory:
        subprocess.run(
            [
                sys.executable,
                "-m",
                "pip",
                "download",
                "--disable-pip-version-check",
                "--no-deps",
                "--only-binary=:all:",
                "--dest",
                directory,
                f"nudenet=={VERSION}",
            ],
            check=True,
        )
        wheel = Path(directory) / WHEEL_NAME
        wheel_bytes = wheel.read_bytes()
        if sha256_bytes(wheel_bytes) != WHEEL_SHA256:
            raise SystemExit("NudeNet wheel SHA-256 mismatch")
        with zipfile.ZipFile(wheel) as archive:
            model = archive.read("nudenet/320n.onnx")
            if sha256_bytes(model) != MODEL_SHA256:
                raise SystemExit("NudeNet 320n.onnx SHA-256 mismatch")
            args.output.write_bytes(model)
            for source, destination in (
                (f"nudenet-{VERSION}.dist-info/LICENSE", "NudeNet-LICENSE-AGPL-3.0.txt"),
                (
                    f"nudenet-{VERSION}.dist-info/LICENSE.md",
                    "NudeNet-LICENSE-md-AGPL-3.0.txt",
                ),
                (f"nudenet-{VERSION}.dist-info/METADATA", "NudeNet-METADATA.txt"),
            ):
                (args.licenses / destination).write_bytes(archive.read(source))
    print(f"Wrote {args.output} ({args.output.stat().st_size} bytes, {MODEL_SHA256})")


if __name__ == "__main__":
    main()
