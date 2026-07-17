#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import urllib.request
from pathlib import Path

URL = "https://github.com/Megvii-BaseDetection/YOLOX/releases/download/0.1.1rc0/yolox_nano.pth"
SHA256 = "cd28f55fbbc1829f99d9ac9b38a16d259a22889739c8728ea877610201feff7b"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.output.with_suffix(args.output.suffix + ".tmp")
    try:
        with urllib.request.urlopen(URL, timeout=120) as response, temporary.open("wb") as stream:
            while chunk := response.read(1024 * 1024):
                stream.write(chunk)
        actual = hashlib.sha256(temporary.read_bytes()).hexdigest()
        if actual != SHA256:
            raise SystemExit(f"YOLOX checkpoint SHA-256 mismatch: expected {SHA256}, got {actual}")
        temporary.replace(args.output)
    finally:
        if temporary.exists():
            temporary.unlink()
    print(f"Wrote {args.output} ({args.output.stat().st_size} bytes, {SHA256})")


if __name__ == "__main__":
    main()
