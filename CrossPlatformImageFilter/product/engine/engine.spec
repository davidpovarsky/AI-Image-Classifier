# -*- mode: python ; coding: utf-8 -*-

from pathlib import Path

from PyInstaller.utils.hooks import collect_all, collect_data_files, collect_dynamic_libs

spec_root = Path(SPECPATH).resolve()
package_root = spec_root.parents[1]
source_root = package_root / "src"

datas = collect_data_files(
    "local_image_filter",
    includes=["proxy/addon.py", "resources/*.toml"],
    include_py_files=True,
)
binaries = []
hiddenimports = []
for package in ("mitmproxy", "pillow_heif"):
    package_datas, package_binaries, package_hidden = collect_all(package)
    datas += package_datas
    binaries += package_binaries
    hiddenimports += package_hidden
binaries += collect_dynamic_libs("onnxruntime")
datas += collect_data_files("onnxruntime")
hiddenimports += [
    "onnxruntime.capi._pybind_state",
    "onnxruntime.capi.onnxruntime_inference_collection",
]

analysis = Analysis(
    [str(spec_root / "entrypoint.py")],
    pathex=[str(source_root)],
    binaries=binaries,
    datas=datas,
    hiddenimports=sorted(set(hiddenimports)),
    hookspath=[str(spec_root / "hooks")],
    excludes=["tkinter", "pytest", "mypy", "ruff"],
    noarchive=False,
)
pyz = PYZ(analysis.pure)
executable = EXE(
    pyz,
    analysis.scripts,
    [],
    exclude_binaries=True,
    name="filter-engine",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,
)
bundle = COLLECT(
    executable,
    analysis.binaries,
    analysis.datas,
    strip=False,
    upx=False,
    name="filter-engine",
)
