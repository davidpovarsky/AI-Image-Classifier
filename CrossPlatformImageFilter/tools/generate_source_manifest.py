#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

EXCLUDED_PARTS = {
    ".mypy_cache",
    ".model-cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    ".wheel-test",
    "__pycache__",
}
EXCLUDED_ROOT_PARTS = {"build", "diagnostics", "dist", "logs", "reports"}
EXCLUDED_NAMES = {"MANIFEST.sha256", "runtime-manifest.json", "cache.sqlite3"}
EXCLUDED_SUFFIXES = {".onnx", ".npz", ".pt", ".pyc"}


def included(path: Path, root: Path) -> bool:
    relative = path.relative_to(root)
    if any(part in EXCLUDED_PARTS or part.endswith(".egg-info") for part in relative.parts):
        return False
    if relative.parts and relative.parts[0] in EXCLUDED_ROOT_PARTS:
        return False
    if path.name in EXCLUDED_NAMES or path.suffix in EXCLUDED_SUFFIXES:
        return False
    if path.name.startswith("cache.sqlite3"):
        return False
    return path.is_file()


def generate(root: Path) -> str:
    repository = root.parent
    lines = []
    for path in sorted(root.rglob("*"), key=lambda item: item.as_posix()):
        if not included(path, root):
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        relative = path.relative_to(repository).as_posix()
        lines.append(f"{digest}  {relative}")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.root.resolve()
    manifest_path = root / "MANIFEST.sha256"
    expected = generate(root)
    if args.check:
        actual = manifest_path.read_text(encoding="utf-8") if manifest_path.is_file() else ""
        if actual != expected:
            raise SystemExit("MANIFEST.sha256 is stale; run tools/generate_source_manifest.py")
    else:
        manifest_path.write_text(expected, encoding="utf-8", newline="\n")
        print(f"Wrote {manifest_path} with {expected.count(chr(10))} entries")


if __name__ == "__main__":
    main()
