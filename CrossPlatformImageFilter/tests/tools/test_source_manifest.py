from __future__ import annotations

import importlib.util
from pathlib import Path


def load_generator():
    path = Path(__file__).resolve().parents[2] / "tools/generate_source_manifest.py"
    spec = importlib.util.spec_from_file_location("generate_source_manifest", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_generated_and_nested_build_outputs_are_excluded(tmp_path: Path) -> None:
    generator = load_generator()
    source = tmp_path / "source.py"
    nested_source = tmp_path / "product/source.rs"
    root_build = tmp_path / "build/output.bin"
    nested_build = tmp_path / "product/build/output.bin"
    generated = tmp_path / "product/apps/desktop/src-tauri/gen/schema.json"
    for path in (source, nested_source, root_build, nested_build, generated):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(path.name, encoding="utf-8")

    manifest = generator.generate(tmp_path)

    assert "source.py" in manifest
    assert "product/source.rs" in manifest
    assert "build/output.bin" not in manifest
    assert "product/build/output.bin" not in manifest
    assert "src-tauri/gen/schema.json" not in manifest
