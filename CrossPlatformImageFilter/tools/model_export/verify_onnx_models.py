#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import numpy as np
import onnxruntime as ort


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def inspect(path: Path) -> dict[str, object]:
    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    return {
        "file": path.name,
        "bytes": path.stat().st_size,
        "sha256": sha256(path),
        "inputs": [
            {"name": item.name, "shape": item.shape, "type": item.type}
            for item in session.get_inputs()
        ],
        "outputs": [
            {"name": item.name, "shape": item.shape, "type": item.type}
            for item in session.get_outputs()
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("models", nargs="+", type=Path)
    parser.add_argument("--prompt-embeddings", type=Path)
    args = parser.parse_args()
    result = {"models": [inspect(path) for path in args.models]}
    if args.prompt_embeddings:
        bundle = np.load(args.prompt_embeddings, allow_pickle=False)
        result["promptEmbeddings"] = {
            "file": args.prompt_embeddings.name,
            "sha256": sha256(args.prompt_embeddings),
            "classNames": bundle["class_names"].tolist(),
            "shape": list(bundle["embeddings"].shape),
            "finite": bool(np.isfinite(bundle["embeddings"]).all()),
        }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
