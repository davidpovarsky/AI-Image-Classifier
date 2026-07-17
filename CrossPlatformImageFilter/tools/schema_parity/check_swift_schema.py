#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


def struct_fields(source: str, name: str) -> set[str]:
    match = re.search(rf"struct\s+{re.escape(name)}\b.*?\{{(?P<body>.*?)\n\}}", source, re.DOTALL)
    if match is None:
        raise ValueError(f"Swift struct {name} was not found")
    return set(re.findall(r"^\s+(?:let|var)\s+(\w+)\s*(?::|=)", match.group("body"), re.MULTILINE))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--swift", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()
    source = args.swift.read_text(encoding="utf-8")
    expected = json.loads(args.fixture.read_text(encoding="utf-8"))
    mappings = {
        "topLevel": "ImageSafetyResponse",
        "pipelineModules": "ImageSafetyPipelineReports",
        "nudity": "NudityEvidence",
    }
    for fixture_key, struct_name in mappings.items():
        fields = struct_fields(source, struct_name)
        missing = set(expected[fixture_key]) - fields
        if missing:
            raise SystemExit(f"Swift {struct_name} is missing required fields: {sorted(missing)}")
    print("Swift/Desktop required response fields are aligned")


if __name__ == "__main__":
    main()
