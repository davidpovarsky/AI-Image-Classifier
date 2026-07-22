from __future__ import annotations

import argparse
import json
from pathlib import Path

TARGETS = {
    "windows": ["nsis"],
    "macos": ["app", "dmg"],
    "linux": ["deb", "rpm", "appimage"],
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=TARGETS, required=True)
    parser.add_argument("--engine-directory", type=Path, required=True)
    parser.add_argument("--service-directory", type=Path, required=True)
    parser.add_argument("--updater-public-key", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    engine = args.engine_directory.resolve(strict=True)
    service = args.service_directory.resolve(strict=True)
    public_key = args.updater_public_key.read_text(encoding="utf-8").strip()
    if not public_key or any(character.isspace() for character in public_key):
        raise SystemExit("updater public key must be one non-empty encoded line")
    required_service_files = ["supervisor", "filterctl"]
    if args.platform == "windows":
        required_service_files = [f"{name}.exe" for name in required_service_files]
    for name in required_service_files:
        if not (service / name).is_file():
            raise SystemExit(f"missing service binary: {name}")
    if not any(path.is_file() for path in engine.rglob("*")):
        raise SystemExit("engine package is empty")

    repository = Path(__file__).resolve().parents[2]
    installers = (repository / "product" / "installers").resolve(strict=True)
    bundle: dict[str, object] = {
        "targets": TARGETS[args.platform],
        "createUpdaterArtifacts": True,
        "resources": {
            f"{engine.as_posix()}/": "engine/",
            f"{service.as_posix()}/": "service/",
            f"{installers.as_posix()}/": "installers/",
        },
    }
    config: dict[str, object] = {
        "bundle": bundle,
        "plugins": {"updater": {"pubkey": public_key}},
    }
    if args.platform == "windows":
        bundle["windows"] = {
            "nsis": {
                "installMode": "perMachine",
                "installerHooks": str((installers / "windows" / "installer-hooks.nsh").as_posix()),
            }
        }
    elif args.platform == "linux":
        shared_files = {
            "/opt/local-ai-image-filter/bin/supervisor": str((service / "supervisor").as_posix()),
            "/opt/local-ai-image-filter/bin/filterctl": str((service / "filterctl").as_posix()),
            "/usr/lib/systemd/system/local-ai-image-filter.service": str(
                (installers / "linux" / "local-ai-image-filter.service").as_posix()
            ),
        }
        bundle["linux"] = {
            "deb": {
                "files": shared_files,
                "postInstallScript": str((installers / "linux" / "postinst").as_posix()),
                "preRemoveScript": str((installers / "linux" / "prerm").as_posix()),
            },
            "rpm": {
                "files": shared_files,
                "postInstallScript": str((installers / "linux" / "postinst").as_posix()),
                "preRemoveScript": str((installers / "linux" / "prerm").as_posix()),
            },
        }
    else:
        bundle["macOS"] = {
            "files": {
                "Library/LaunchDaemons/com.localimagefilter.supervisor.plist": str(
                    (installers / "macos" / "com.localimagefilter.supervisor.plist").as_posix()
                ),
                "Library/LaunchServices/supervisor": str((service / "supervisor").as_posix()),
                "Library/LaunchServices/filterctl": str((service / "filterctl").as_posix()),
                "Resources/engine": str(engine.as_posix()),
            }
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
