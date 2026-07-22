from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path


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
