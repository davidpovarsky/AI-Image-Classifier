from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

from .cache.store import AnalysisCache
from .config import ConfigurationError, Settings, load_settings
from .diagnostics.logger import DiagnosticsLogger
from .inference.model_metadata import RuntimeManifest
from .policy.bundle import (
    VerificationContext,
    apply_verified_policy,
    load_policy_json,
    load_trusted_keys,
    verify_policy_bundle,
)
from .policy.engine import PolicyEngine
from .runtime.container import build_runtime


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="local-image-filter")
    subparsers = parser.add_subparsers(dest="command", required=True)

    def common(command: argparse.ArgumentParser) -> None:
        command.add_argument("--config")
        command.add_argument("--overlay", action="append", default=[])

    run = subparsers.add_parser("run", help="Start mitmproxy with the image-filter addon")
    common(run)
    run.add_argument("--listen-host")
    run.add_argument("--listen-port", type=int)
    run.add_argument("--mode")
    run.add_argument("--ca-directory")
    run.add_argument("mitm_args", nargs=argparse.REMAINDER)

    doctor = subparsers.add_parser("doctor", help="Validate configuration and runtime assets")
    common(doctor)
    doctor.add_argument("--load-models", action="store_true")

    classify = subparsers.add_parser("classify", help="Classify one local image")
    common(classify)
    classify.add_argument("image")
    classify.add_argument("--mime-type", default="image/jpeg")
    classify.add_argument("--write-output")

    print_config = subparsers.add_parser("print-config", help="Print merged TOML as JSON")
    common(print_config)

    cache = subparsers.add_parser("cache", help="Inspect or clear the decision cache")
    cache_subcommands = cache.add_subparsers(dest="cache_command", required=True)
    for name in ("stats", "clear"):
        command = cache_subcommands.add_parser(name)
        common(command)

    models = subparsers.add_parser("models", help="Inspect or verify runtime models")
    model_subcommands = models.add_subparsers(dest="models_command", required=True)
    for name in ("inspect", "verify"):
        command = model_subcommands.add_parser(name)
        common(command)

    diagnostics = subparsers.add_parser("diagnostics", help="Read diagnostic summaries")
    diagnostics_subcommands = diagnostics.add_subparsers(dest="diagnostics_command", required=True)
    summary = diagnostics_subcommands.add_parser("summary")
    common(summary)

    policy = subparsers.add_parser("policy", help="Inspect and verify signed policy bundles")
    policy_subcommands = policy.add_subparsers(dest="policy_command", required=True)
    for name in ("inspect", "verify", "effective", "simulate"):
        command = policy_subcommands.add_parser(name)
        common(command)
        command.add_argument("bundle")
        command.add_argument("--trusted-keys", required=name != "inspect")
        command.add_argument("--device-id")
        command.add_argument("--minimum-revision", type=int, default=0)
        command.add_argument("--engine-version", default="0.1.0")
        command.add_argument("--product-version", default="0.1.0")
        if name == "simulate":
            command.add_argument("--evidence", required=True)
    return parser


def _settings(args: argparse.Namespace) -> Settings:
    return load_settings(getattr(args, "config", None), getattr(args, "overlay", []))


def _validate_mode(mode: str) -> str:
    if mode in {"regular", "local"}:
        return mode
    if (
        mode.startswith("local:")
        and mode[6:]
        and all(character.isalnum() or character in "._-" for character in mode[6:])
    ):
        return mode
    raise ConfigurationError("Proxy mode must be regular, local, or local:<process-name>")


def _find_mitmdump() -> str | None:
    executable_name = "mitmdump.exe" if sys.platform == "win32" else "mitmdump"
    sibling = Path(sys.executable).resolve().parent / executable_name
    return str(sibling) if sibling.is_file() else shutil.which("mitmdump")


def _run(args: argparse.Namespace) -> int:
    settings = _settings(args)
    proxy = settings.section("proxy")
    packaged = bool(getattr(sys, "frozen", False))
    executable = None if packaged else _find_mitmdump()
    if not packaged and executable is None:
        print(
            "mitmdump was not found. Install this package in the active environment.",
            file=sys.stderr,
        )
        return 2
    addon = Path(__file__).resolve().parent / "proxy" / "addon.py"
    host = args.listen_host or str(proxy.get("listen_host", "127.0.0.1"))
    port = args.listen_port or int(proxy.get("listen_port", 8080))
    mode = _validate_mode(args.mode or str(proxy.get("mode", "regular")))
    command = [
        "--listen-host",
        host,
        "--listen-port",
        str(port),
        "--mode",
        mode,
    ]
    if args.ca_directory:
        ca_directory = Path(args.ca_directory).expanduser().resolve()
        if not ca_directory.is_dir():
            raise ConfigurationError(f"CA directory does not exist: {ca_directory}")
        command.extend(["--set", f"confdir={ca_directory}"])
    environment = dict(os.environ)
    if settings.source_path is not None:
        command.extend(["--set", f"local_image_filter_config={settings.source_path}"])
        environment["LOCAL_IMAGE_FILTER_CONFIG"] = str(settings.source_path)
    else:
        environment.pop("LOCAL_IMAGE_FILTER_CONFIG", None)
    command.extend(["--set", "connection_strategy=lazy", "-s", str(addon)])
    command.extend(args.mitm_args)
    print(f"Starting local image filter at {host}:{port} in {mode} mode")
    if packaged:
        from mitmproxy.tools.main import mitmdump

        previous_environment = os.environ.copy()
        try:
            os.environ.clear()
            os.environ.update(environment)
            return int(mitmdump(command) or 0)
        finally:
            os.environ.clear()
            os.environ.update(previous_environment)
    assert executable is not None
    command.insert(0, executable)
    return subprocess.run(command, env=environment, check=False).returncode


def _writable_directory(path: Path) -> tuple[bool, str | None]:
    try:
        path.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=path, prefix="doctor-", delete=True):
            pass
        return True, None
    except OSError as error:
        return False, str(error)


def _port_available(host: str, port: int) -> bool:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.bind((host, port))
        return True
    except OSError:
        return False


def _doctor(args: argparse.Namespace) -> int:
    settings = _settings(args)
    mock = bool(settings.section("runtime").get("mock_models", False))
    model_keys = (
        "models.person.path",
        "models.mobileclip.path",
        "models.mobileclip.prompt_embeddings_path",
        "models.nudenet.path",
        "models.nudenet.labels_path",
        "models.manifest.path",
    )
    models: dict[str, Any] = {}
    for key in model_keys:
        path = settings.path(key)
        models[key] = {
            "path": str(path),
            "exists": path.is_file(),
            "bytes": path.stat().st_size if path.is_file() else None,
        }
    cache_writable, cache_error = _writable_directory(settings.path("cache.sqlite_path").parent)
    diagnostics_writable, diagnostics_error = _writable_directory(
        settings.path("diagnostics.directory")
    )
    proxy = settings.section("proxy")
    report: dict[str, Any] = {
        "config": settings.source_description,
        "platform": sys.platform,
        "architecture": platform.machine(),
        "pythonVersion": platform.python_version(),
        "mitmdump": _find_mitmdump(),
        "models": models,
        "storage": {
            "cacheWritable": cache_writable,
            "cacheError": cache_error,
            "diagnosticsWritable": diagnostics_writable,
            "diagnosticsError": diagnostics_error,
        },
        "proxy": {
            "listenHost": proxy.get("listen_host"),
            "listenPort": proxy.get("listen_port"),
            "portAvailable": _port_available(
                str(proxy.get("listen_host", "127.0.0.1")), int(proxy.get("listen_port", 8080))
            ),
            "caExists": (Path.home() / ".mitmproxy" / "mitmproxy-ca-cert.pem").is_file(),
        },
    }
    try:
        import onnxruntime as ort

        report["onnxRuntime"] = {
            "version": ort.__version__,
            "availableProviders": ort.get_available_providers(),
            "selectedProviders": {},
        }
    except ImportError as error:
        report["onnxRuntime"] = {"error": str(error)}

    model_load_failed = False
    if args.load_models:
        try:
            runtime = build_runtime(settings)
            report["modelLoad"] = {
                "success": True,
                "selectedProviders": runtime.selected_providers,
                "models": runtime.model_descriptions,
            }
            report["onnxRuntime"]["selectedProviders"] = runtime.selected_providers
        except Exception as error:
            model_load_failed = True
            report["modelLoad"] = {
                "success": False,
                "error": f"{error.__class__.__name__}: {error}",
            }
    print(json.dumps(report, indent=2, ensure_ascii=False))
    if report.get("mitmdump") is None or "error" in report["onnxRuntime"]:
        return 2
    if not cache_writable or not diagnostics_writable:
        return 3
    missing = any(not item["exists"] for item in models.values())
    if (missing and not mock) or model_load_failed:
        return 1
    return 0


def _classify(args: argparse.Namespace) -> int:
    settings = _settings(args)
    runtime = build_runtime(settings)
    image_path = Path(args.image).expanduser().resolve()
    content = image_path.read_bytes()
    outcome = runtime.pipeline.filter_bytes(content, args.mime_type, None)
    print(
        json.dumps(
            {
                "decision": outcome.decision.to_dict(),
                "cacheHit": outcome.cache_hit,
                "response": outcome.response,
            },
            indent=2,
            ensure_ascii=False,
        )
    )
    if args.write_output and outcome.replacement_bytes is not None:
        Path(args.write_output).write_bytes(outcome.replacement_bytes)
    return 0


def _cache(settings: Settings) -> AnalysisCache:
    policy = hashlib.sha256(
        json.dumps(settings.section("policy"), sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    processing = hashlib.sha256(
        json.dumps(settings.section("processing"), sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    manifest_path = settings.path("models.manifest.path")
    manifest = (
        hashlib.sha256(manifest_path.read_bytes()).hexdigest()
        if manifest_path.is_file()
        else "missing"
    )
    namespace = hashlib.sha256(
        json.dumps(
            {
                "manifest": manifest,
                "policy": policy,
                "processing": processing,
                "merge": settings.section("merge"),
            },
            sort_keys=True,
            separators=(",", ":"),
        ).encode()
    ).hexdigest()[:24]
    config = settings.section("cache")
    return AnalysisCache(
        enabled=bool(config.get("enabled", True)),
        memory_entries=int(config.get("memory_entries", 1500)),
        sqlite_path=settings.path("cache.sqlite_path"),
        ttl_seconds=int(config.get("ttl_seconds", 604800)),
        namespace=namespace,
        policy_fingerprint=policy,
        manifest_fingerprint=manifest,
    )


def _verified_policy(args: argparse.Namespace) -> tuple[dict[str, Any], Settings]:
    bundle = load_policy_json(Path(args.bundle))
    verified = verify_policy_bundle(
        bundle,
        VerificationContext(
            trusted_keys=load_trusted_keys(Path(args.trusted_keys)),
            engine_version=args.engine_version,
            product_version=args.product_version,
            device_id=args.device_id,
            minimum_revision=args.minimum_revision,
        ),
    )
    return verified, apply_verified_policy(_settings(args), verified)


def _policy_command(args: argparse.Namespace) -> int:
    if args.policy_command == "inspect":
        bundle = load_policy_json(Path(args.bundle))
        summary = {
            key: bundle.get(key)
            for key in (
                "schemaVersion",
                "policyId",
                "revision",
                "channel",
                "issuedAt",
                "expiresAt",
                "minimumEngineVersion",
                "minimumProductVersion",
                "subject",
                "metadata",
                "signature",
            )
        }
        print(json.dumps(summary, indent=2, ensure_ascii=False))
        return 0

    bundle, settings = _verified_policy(args)
    if args.policy_command == "verify":
        result: dict[str, Any] = {
            "valid": True,
            "policyId": bundle["policyId"],
            "revision": bundle["revision"],
        }
    elif args.policy_command == "effective":
        result = {
            "policy": settings.section("policy"),
            "processing": settings.section("processing"),
            "signedPolicy": settings.section("signed_policy"),
        }
    else:
        evidence = load_policy_json(Path(args.evidence), maximum_bytes=4_194_304)
        result = (
            PolicyEngine(
                settings.section("policy"),
                fail_action=str(settings.section("proxy").get("fail_action", "replace")),
            )
            .evaluate(evidence)
            .to_dict()
        )
    print(json.dumps(result, indent=2, ensure_ascii=False))
    return 0


def main() -> None:
    args = _parser().parse_args()
    try:
        if args.command == "run":
            code = _run(args)
        elif args.command == "doctor":
            code = _doctor(args)
        elif args.command == "classify":
            code = _classify(args)
        elif args.command == "print-config":
            print(json.dumps(_settings(args).to_dict(), indent=2, ensure_ascii=False))
            code = 0
        elif args.command == "cache":
            cache = _cache(_settings(args))
            if args.cache_command == "stats":
                print(json.dumps(cache.stats(), indent=2))
            else:
                print(json.dumps({"clearedEntries": cache.clear()}, indent=2))
            code = 0
        elif args.command == "models":
            settings = _settings(args)
            manifest = RuntimeManifest.load(settings.path("models.manifest.path"))
            result: dict[str, Any] = {
                "manifest": str(manifest.path),
                "fingerprint": manifest.fingerprint,
                "models": manifest.data["models"],
            }
            if args.models_command == "verify":
                runtime = build_runtime(settings)
                result["selectedProviders"] = runtime.selected_providers
                result["sessions"] = runtime.model_descriptions
            print(json.dumps(result, indent=2, ensure_ascii=False))
            code = 0
        elif args.command == "diagnostics":
            settings = _settings(args)
            logger = DiagnosticsLogger(
                settings.section("diagnostics"), settings.path("diagnostics.directory")
            )
            print(json.dumps(logger.summary(), indent=2, ensure_ascii=False))
            code = 0
        elif args.command == "policy":
            code = _policy_command(args)
        else:
            raise AssertionError(args.command)
    except (ConfigurationError, OSError, ValueError) as error:
        print(f"{error.__class__.__name__}: {error}", file=sys.stderr)
        code = 1
    raise SystemExit(code)
