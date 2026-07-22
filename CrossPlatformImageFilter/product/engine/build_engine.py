from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


def repair_linux_pillow_heif_rpaths(executable_directory: Path) -> None:
    """Make Pillow-HEIF's vendored ELF dependency chain self-resolving.

    The wheel's extension supplies the original transitive RPATH. PyInstaller
    also emits the vendored libraries as standalone files, which linuxdeploy
    inspects directly while building an AppImage. Give the dependency-bearing
    libheif and libx265 libraries a local search path so that direct inspection
    resolves their sibling libraries. Other wheel libraries, especially glibc's
    libmvec, must remain byte-for-byte unchanged.
    """
    if sys.platform != "linux":
        return

    patchelf = shutil.which("patchelf")
    if patchelf is None:
        raise SystemExit("patchelf is required to package the Linux engine")

    internal_directory = executable_directory / "_internal"
    vendor_directory = internal_directory / "pillow_heif.libs"
    if not vendor_directory.is_dir():
        raise SystemExit(f"PyInstaller did not create {vendor_directory}")

    vendor_libraries: list[Path] = []
    for codec in ("libheif", "libx265"):
        matches = sorted(path for path in vendor_directory.glob(f"{codec}*.so*") if path.is_file())
        if not matches:
            raise SystemExit(f"PyInstaller did not collect {codec} in {vendor_directory}")
        vendor_libraries.extend(matches)

    targets = list(vendor_libraries)
    targets.extend(
        duplicate
        for library in vendor_libraries
        if (duplicate := internal_directory / library.name).is_file()
    )
    for target in targets:
        subprocess.run(
            [patchelf, "--set-rpath", "$ORIGIN", str(target)],
            check=True,
        )


def main() -> None:
    parser = argparse.ArgumentParser(description="Build the self-contained onedir engine")
    parser.add_argument("--output", type=Path, default=Path("build/product-engine"))
    parser.add_argument("--work", type=Path, default=Path("build/pyinstaller-work"))
    parser.add_argument("--models", type=Path)
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    spec = Path(__file__).with_name("engine.spec")
    output = arguments.output.resolve()
    work = arguments.work.resolve()
    build_root = (root / "build").resolve()
    if not output.is_relative_to(build_root) or output == build_root:
        raise SystemExit("--output must be a child of the repository build directory")
    if not work.is_relative_to(build_root) or work == build_root:
        raise SystemExit("--work must be a child of the repository build directory")
    if output.exists():
        shutil.rmtree(output)
    command = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--noconfirm",
        "--clean",
        "--distpath",
        str(output),
        "--workpath",
        str(work),
        str(spec),
    ]
    subprocess.run(command, cwd=root, check=True)
    executable = (
        output
        / "filter-engine"
        / ("filter-engine.exe" if sys.platform == "win32" else "filter-engine")
    )
    if not executable.is_file():
        raise SystemExit(f"PyInstaller did not create {executable}")
    repair_linux_pillow_heif_rpaths(executable.parent)
    if arguments.models is not None:
        models = arguments.models.resolve(strict=True)
        required = {
            "mobileclip2_s2_image_encoder.onnx",
            "mobileclip2_s2_prompt_embeddings.npz",
            "nudenet320n.onnx",
            "person_detector.onnx",
            "runtime-manifest.json",
            "model-sources.json",
            "nudenet_labels.json",
            "coco_labels.json",
        }
        missing = sorted(name for name in required if not (models / name).is_file())
        if missing:
            raise SystemExit(f"model package is incomplete: {', '.join(missing)}")
        shutil.copytree(models, executable.parent / "models")
        config_directory = executable.parent / "config"
        config_directory.mkdir()
        shutil.copy2(root / "config" / "default.toml", config_directory / "default.toml")
    subprocess.run([str(executable), "--help"], check=True)


if __name__ == "__main__":
    main()
