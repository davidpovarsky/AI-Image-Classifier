#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import zipfile
from pathlib import Path


def copy(source: Path, destination: Path) -> None:
    if source.is_dir():
        shutil.copytree(source, destination, dirs_exist_ok=True)
    else:
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--wheel", type=Path, required=True)
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--offline-wheels", type=Path)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    staging = args.output.parent / f"staging-{args.platform}"
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    copy(args.wheel, staging / "wheel" / args.wheel.name)
    copy(args.root / "config", staging / "config")
    copy(args.root / "scripts" / args.platform, staging / "scripts")
    copy(args.root / "docs", staging / "docs")
    copy(args.root / "README.md", staging / "README.md")
    copy(args.models, staging / "models")
    if args.offline_wheels and args.offline_wheels.is_dir():
        copy(args.offline_wheels, staging / "offline-wheels")
    with zipfile.ZipFile(
        args.output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9
    ) as archive:
        for path in sorted(item for item in staging.rglob("*") if item.is_file()):
            information = zipfile.ZipInfo(path.relative_to(staging).as_posix())
            information.date_time = (2026, 1, 1, 0, 0, 0)
            information.external_attr = 0o644 << 16
            archive.writestr(information, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
    digest = hashlib.sha256(args.output.read_bytes()).hexdigest()
    report = {"file": args.output.name, "bytes": args.output.stat().st_size, "sha256": digest}
    args.output.with_suffix(args.output.suffix + ".sha256").write_text(
        f"{digest}  {args.output.name}\n", encoding="utf-8"
    )
    args.output.with_suffix(".json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
