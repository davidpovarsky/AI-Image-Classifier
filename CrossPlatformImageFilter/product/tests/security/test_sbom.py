from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def test_sbom_generator_is_deterministic_and_deduplicates(tmp_path: Path) -> None:
    root = Path(__file__).resolve().parents[3]
    cargo = tmp_path / "cargo.json"
    frozen = tmp_path / "freeze.txt"
    output = tmp_path / "sbom.json"
    cargo.write_text(
        json.dumps({"packages": [{"name": "serde", "version": "1.0.0"}]}), encoding="utf-8"
    )
    frozen.write_text("cryptography==48.0.1\ncryptography==48.0.1\n", encoding="utf-8")
    command = [
        sys.executable,
        str(root / "product/scripts/generate_sbom.py"),
        "--cargo-metadata",
        str(cargo),
        "--python-freeze",
        str(frozen),
        "--output",
        str(output),
    ]
    subprocess.run(command, check=True)
    first = output.read_bytes()
    subprocess.run(command, check=True)
    assert output.read_bytes() == first
    document = json.loads(first)
    assert document["specVersion"] == "1.6"
    assert len(document["components"]) == 2
