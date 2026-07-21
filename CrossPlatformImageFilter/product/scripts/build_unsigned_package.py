from __future__ import annotations

import argparse
import hashlib
import shutil
import tarfile
import tempfile
import zipfile
from pathlib import Path


def copy_required(source: Path, destination: Path) -> None:
    if not source.is_file():
        raise SystemExit(f"required package input is missing: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def build_package(
    *,
    platform: str,
    engine_directory: Path,
    desktop: Path,
    supervisor: Path,
    filterctl: Path,
    output_directory: Path,
) -> tuple[Path, str]:
    product_root = Path(__file__).resolve().parents[1]
    if not engine_directory.is_dir() or not any(engine_directory.iterdir()):
        raise SystemExit(f"required engine directory is missing or empty: {engine_directory}")
    output_directory.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="local-image-filter-package-") as temporary:
        staging = Path(temporary) / "local-ai-image-filter"
        shutil.copytree(engine_directory, staging / "engine")
        executable_suffix = ".exe" if platform == "windows" else ""
        copy_required(desktop, staging / "bin" / f"local-image-filter-desktop{executable_suffix}")
        copy_required(supervisor, staging / "bin" / f"supervisor{executable_suffix}")
        copy_required(filterctl, staging / "bin" / f"filterctl{executable_suffix}")
        shutil.copytree(product_root / "installers" / platform, staging / "installer")
        copy_required(
            product_root / "policy" / "trusted-root" / "root.json",
            staging / "policy" / "trusted-root" / "root.json",
        )
        copy_required(
            product_root / "policy" / "defaults" / "base-policy.json",
            staging / "policy" / "defaults" / "base-policy.json",
        )
        (staging / "UNSIGNED-INTERNAL-BUILD.txt").write_text(
            "Unsigned internal validation artifact. Not approved for redistribution.\n",
            encoding="utf-8",
        )

        if platform == "windows":
            archive = output_directory / "local-ai-image-filter-windows-x64-unsigned.zip"
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for path in sorted(staging.rglob("*")):
                    if path.is_file():
                        info = zipfile.ZipInfo(path.relative_to(staging.parent).as_posix())
                        info.date_time = (1980, 1, 1, 0, 0, 0)
                        info.external_attr = 0o100644 << 16
                        bundle.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
        else:
            archive = output_directory / f"local-ai-image-filter-{platform}-x64-unsigned.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                for path in sorted(staging.rglob("*")):
                    relative = path.relative_to(staging.parent)
                    info = bundle.gettarinfo(path, arcname=relative.as_posix())
                    info.mtime = 0
                    if path.is_file():
                        with path.open("rb") as source:
                            bundle.addfile(info, source)
                    else:
                        bundle.addfile(info)

    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = archive.with_name("SHA256SUMS")
    checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8", newline="\n")
    return archive, digest


def main() -> None:
    parser = argparse.ArgumentParser(description="Build an unsigned internal product package")
    parser.add_argument("--platform", choices=("windows", "macos", "linux"), required=True)
    parser.add_argument("--engine-directory", type=Path, required=True)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--supervisor", type=Path, required=True)
    parser.add_argument("--filterctl", type=Path, required=True)
    parser.add_argument("--output-directory", type=Path, required=True)
    arguments = parser.parse_args()
    archive, digest = build_package(
        platform=arguments.platform,
        engine_directory=arguments.engine_directory,
        desktop=arguments.desktop,
        supervisor=arguments.supervisor,
        filterctl=arguments.filterctl,
        output_directory=arguments.output_directory,
    )
    print(f"{digest}  {archive}")


if __name__ == "__main__":
    main()
