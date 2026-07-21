from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

FREEZE_PATTERN = re.compile(r"(?P<name>[A-Za-z0-9_.-]+)==(?P<version>[^\s]+)\Z")


def _component(kind: str, name: str, version: str, purl: str) -> dict[str, str]:
    return {"type": kind, "name": name, "version": version, "purl": purl}


def python_components(path: Path) -> list[dict[str, str]]:
    components = []
    for line in path.read_text(encoding="utf-8").splitlines():
        match = FREEZE_PATTERN.fullmatch(line.strip())
        if match:
            name, version = match.group("name", "version")
            components.append(_component("library", name, version, f"pkg:pypi/{name}@{version}"))
    return components


def cargo_components(path: Path) -> list[dict[str, str]]:
    metadata = json.loads(path.read_text(encoding="utf-8"))
    return [
        _component(
            "library",
            package["name"],
            package["version"],
            f"pkg:cargo/{package['name']}@{package['version']}",
        )
        for package in metadata["packages"]
    ]


def pnpm_components(path: Path) -> list[dict[str, str]]:
    roots: list[dict[str, Any]] = json.loads(path.read_text(encoding="utf-8"))
    discovered: dict[tuple[str, str], dict[str, str]] = {}

    def visit(node: dict[str, Any]) -> None:
        name = node.get("name")
        version = node.get("version")
        if isinstance(name, str) and isinstance(version, str):
            discovered[(name, version)] = _component(
                "library", name, version, f"pkg:npm/{name}@{version}"
            )
        for dependencies in (node.get("dependencies", {}), node.get("devDependencies", {})):
            if isinstance(dependencies, dict):
                for dependency in dependencies.values():
                    if isinstance(dependency, dict):
                        visit(dependency)

    for root in roots:
        visit(root)
    return list(discovered.values())


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate a deterministic CycloneDX 1.6 SBOM")
    parser.add_argument("--cargo-metadata", type=Path, action="append", default=[])
    parser.add_argument("--python-freeze", type=Path, action="append", default=[])
    parser.add_argument("--pnpm-list", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    components = []
    for path in arguments.cargo_metadata:
        components.extend(cargo_components(path))
    for path in arguments.python_freeze:
        components.extend(python_components(path))
    for path in arguments.pnpm_list:
        components.extend(pnpm_components(path))
    unique = {item["purl"]: item for item in components}
    document = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "version": 1,
        "metadata": {"component": {"type": "application", "name": "local-ai-image-filter"}},
        "components": [unique[key] for key in sorted(unique)],
    }
    arguments.output.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
    )


if __name__ == "__main__":
    main()
