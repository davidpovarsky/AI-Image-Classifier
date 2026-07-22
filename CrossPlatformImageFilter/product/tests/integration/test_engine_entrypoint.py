from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pytest

from local_image_filter import cli
from product.engine import build_engine


def test_production_entrypoint_delegates_to_existing_cli() -> None:
    root = Path(__file__).resolve().parents[3]
    result = subprocess.run(
        [sys.executable, str(root / "product/engine/entrypoint.py"), "--help"],
        capture_output=True,
        check=False,
        text=True,
    )
    assert result.returncode == 0
    assert "local-image-filter" in result.stdout


def test_pyinstaller_spec_is_onedir_and_excludes_model_weights() -> None:
    root = Path(__file__).resolve().parents[3]
    source = (root / "product/engine/engine.spec").read_text(encoding="utf-8")
    assert "COLLECT(" in source
    assert "exclude_binaries=True" in source
    assert "person_detector.onnx" not in source
    assert "models/" not in source
    assert "onefile" not in source.lower()


def test_linux_engine_repairs_vendored_heif_rpaths(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    executable_directory = tmp_path / "filter-engine"
    internal_directory = executable_directory / "_internal"
    vendor_directory = internal_directory / "pillow_heif.libs"
    vendor_directory.mkdir(parents=True)
    libde265 = vendor_directory / "libde265-vendored.so"
    libheif = vendor_directory / "libheif-vendored.so.1"
    libx265 = vendor_directory / "libx265-vendored.so.215"
    duplicate = internal_directory / libheif.name
    for library in (libde265, libheif, libx265, duplicate):
        library.touch()

    calls: list[list[str]] = []

    def fake_run(command: list[str], *, check: bool) -> None:
        assert check is True
        calls.append(command)

    monkeypatch.setattr(build_engine.sys, "platform", "linux")
    monkeypatch.setattr(build_engine.shutil, "which", lambda _: "/usr/bin/patchelf")
    monkeypatch.setattr(build_engine.subprocess, "run", fake_run)

    build_engine.repair_linux_pillow_heif_rpaths(executable_directory)

    assert calls == [
        ["/usr/bin/patchelf", "--set-rpath", "$ORIGIN", str(libheif)],
        ["/usr/bin/patchelf", "--set-rpath", "$ORIGIN", str(libx265)],
        ["/usr/bin/patchelf", "--set-rpath", "$ORIGIN", str(duplicate)],
    ]


def test_packaged_run_uses_bundled_mitmdump(monkeypatch: pytest.MonkeyPatch) -> None:
    captured: list[str] = []

    def fake_mitmdump(arguments: list[str]) -> int:
        captured.extend(arguments)
        return 0

    mitmproxy = ModuleType("mitmproxy")
    tools = ModuleType("mitmproxy.tools")
    main = ModuleType("mitmproxy.tools.main")
    main.mitmdump = fake_mitmdump  # type: ignore[attr-defined]
    mitmproxy.tools = tools  # type: ignore[attr-defined]
    tools.main = main  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "mitmproxy", mitmproxy)
    monkeypatch.setitem(sys.modules, "mitmproxy.tools", tools)
    monkeypatch.setitem(sys.modules, "mitmproxy.tools.main", main)
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    arguments = argparse.Namespace(
        config=None,
        overlay=[],
        listen_host="127.0.0.1",
        listen_port=18080,
        mode="regular",
        mitm_args=[],
    )

    assert cli._run(arguments) == 0
    assert captured[:6] == [
        "--listen-host",
        "127.0.0.1",
        "--listen-port",
        "18080",
        "--mode",
        "regular",
    ]
    assert "-s" in captured
