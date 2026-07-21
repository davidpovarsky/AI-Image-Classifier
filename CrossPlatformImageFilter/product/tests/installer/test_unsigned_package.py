from __future__ import annotations

import tarfile
import zipfile
from pathlib import Path

import pytest

from product.scripts.build_unsigned_package import build_package


@pytest.mark.parametrize("platform", ["windows", "macos", "linux"])
def test_unsigned_package_contains_required_product_boundaries(
    platform: str, tmp_path: Path
) -> None:
    suffix = ".exe" if platform == "windows" else ""
    inputs = tmp_path / "inputs"
    engine = inputs / "engine"
    engine.mkdir(parents=True)
    (engine / f"filter-engine{suffix}").write_bytes(b"engine")
    desktop = inputs / f"desktop{suffix}"
    supervisor = inputs / f"supervisor{suffix}"
    filterctl = inputs / f"filterctl{suffix}"
    for path in (desktop, supervisor, filterctl):
        path.write_bytes(path.name.encode())

    archive, digest = build_package(
        platform=platform,
        engine_directory=engine,
        desktop=desktop,
        supervisor=supervisor,
        filterctl=filterctl,
        output_directory=tmp_path / "output",
    )

    assert len(digest) == 64
    assert archive.is_file()
    if platform == "windows":
        with zipfile.ZipFile(archive) as bundle:
            names = set(bundle.namelist())
    else:
        with tarfile.open(archive) as bundle:
            names = set(bundle.getnames())
    assert f"local-ai-image-filter/bin/supervisor{suffix}" in names
    assert f"local-ai-image-filter/bin/filterctl{suffix}" in names
    assert "local-ai-image-filter/policy/trusted-root/root.json" in names
    assert "local-ai-image-filter/UNSIGNED-INTERNAL-BUILD.txt" in names
