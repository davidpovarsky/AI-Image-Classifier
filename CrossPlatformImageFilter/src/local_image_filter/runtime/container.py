from __future__ import annotations

import hashlib
import json
import platform
import sys
from dataclasses import dataclass
from typing import Any

from ..cache.store import AnalysisCache
from ..config import Settings
from ..diagnostics.logger import DiagnosticsLogger
from ..inference.base import MobileCLIPProtocol, NudeNetProtocol, PersonDetectorProtocol
from ..inference.mobileclip import MobileCLIPClassifier
from ..inference.mock import MockMobileCLIPClassifier, MockNudeNetDetector, MockPersonDetector
from ..inference.model_metadata import RuntimeManifest
from ..inference.nudenet import NudeNetDetector
from ..inference.onnx_session import OnnxSession
from ..inference.person_detector import PersonDetector
from ..pipeline.service import ImageSafetyPipeline
from ..policy.engine import PolicyEngine


def _fingerprint(value: Any) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


@dataclass(slots=True)
class RuntimeContainer:
    settings: Settings
    pipeline: ImageSafetyPipeline
    selected_providers: dict[str, list[str]]
    model_descriptions: dict[str, dict[str, Any]]
    manifest: RuntimeManifest | None


def build_runtime(settings: Settings) -> RuntimeContainer:
    runtime = settings.section("runtime")
    provider_priority = [str(item) for item in runtime.get("provider_priority", [])]
    intra = int(runtime.get("intra_op_threads", 0))
    inter = int(runtime.get("inter_op_threads", 1))
    selected: dict[str, list[str]] = {}
    descriptions: dict[str, dict[str, Any]] = {}
    manifest: RuntimeManifest | None = None
    person_detector: PersonDetectorProtocol
    mobileclip: MobileCLIPProtocol
    nudenet: NudeNetProtocol

    if bool(runtime.get("mock_models", False)):
        person_detector = MockPersonDetector()
        mobileclip = MockMobileCLIPClassifier()
        nudenet = MockNudeNetDetector()
        selected = {"personDetector": ["Mock"], "mobileCLIP2": ["Mock"], "nudeNet": ["Mock"]}
        manifest_fingerprint = "mock-models"
    else:
        manifest = RuntimeManifest.load(settings.path("models.manifest.path"))
        manifest_fingerprint = manifest.fingerprint
        person_metadata = manifest.model("personDetector")
        clip_metadata = manifest.model("mobileCLIP2")
        nude_metadata = manifest.model("nudeNet")
        configured_models = settings.section("models")
        for role, configuration, metadata in (
            ("personDetector", configured_models["person"], person_metadata),
            ("mobileCLIP2", configured_models["mobileclip"], clip_metadata),
            ("nudeNet", configured_models["nudenet"], nude_metadata),
        ):
            if configuration.get("adapter") != metadata.get("adapter"):
                raise ValueError(
                    f"Adapter mismatch for {role}: config={configuration.get('adapter')}, "
                    f"manifest={metadata.get('adapter')}"
                )
            configured_path = settings.path(
                {
                    "personDetector": "models.person.path",
                    "mobileCLIP2": "models.mobileclip.path",
                    "nudeNet": "models.nudenet.path",
                }[role]
            )
            manifest_path = (manifest.root / str(metadata.get("runtimeFile"))).resolve()
            if configured_path != manifest_path:
                raise ValueError(
                    f"Runtime path mismatch for {role}: config={configured_path}, manifest={manifest_path}"
                )

        person_session = OnnxSession(
            settings.path("models.person.path"), provider_priority, intra, inter, person_metadata
        )
        clip_session = OnnxSession(
            settings.path("models.mobileclip.path"), provider_priority, intra, inter, clip_metadata
        )
        nude_session = OnnxSession(
            settings.path("models.nudenet.path"), provider_priority, intra, inter, nude_metadata
        )
        selected = {
            "personDetector": person_session.providers,
            "mobileCLIP2": clip_session.providers,
            "nudeNet": nude_session.providers,
        }
        descriptions = {
            "personDetector": person_session.describe(),
            "mobileCLIP2": clip_session.describe(),
            "nudeNet": nude_session.describe(),
        }
        person_detector = PersonDetector(person_session, configured_models["person"])
        mobileclip = MobileCLIPClassifier(
            clip_session,
            configured_models["mobileclip"],
            settings.path("models.mobileclip.prompt_embeddings_path"),
        )
        nudenet = NudeNetDetector(
            nude_session,
            configured_models["nudenet"],
            settings.path("models.nudenet.labels_path"),
        )
        if mobileclip.class_names != clip_metadata.get("classNames"):
            raise ValueError("MobileCLIP class order does not match runtime manifest")
        if nudenet.labels != nude_metadata.get("classNames"):
            raise ValueError("NudeNet label order does not match runtime manifest")
        person_size = int(configured_models["person"]["input_size"])
        clip_size = int(configured_models["mobileclip"]["input_size"])
        nude_size = int(configured_models["nudenet"]["input_size"])
        person_session.warm_up({person_session.inputs[0].name: (1, 3, person_size, person_size)})
        clip_session.warm_up({clip_session.inputs[0].name: (1, 3, clip_size, clip_size)})
        nude_session.warm_up({nude_session.inputs[0].name: (1, 3, nude_size, nude_size)})

    policy_fingerprint = _fingerprint(settings.section("policy"))
    processing_fingerprint = _fingerprint(settings.section("processing"))
    namespace = _fingerprint(
        {
            "manifest": manifest_fingerprint,
            "policy": policy_fingerprint,
            "policyRevision": settings.section("signed_policy").get("revision"),
            "processing": processing_fingerprint,
            "merge": settings.section("merge"),
        }
    )[:24]
    policy = PolicyEngine(
        settings.section("policy"),
        fail_action=str(settings.section("proxy").get("fail_action", "replace")),
    )
    cache_config = settings.section("cache")
    cache = AnalysisCache(
        enabled=bool(cache_config.get("enabled", True)),
        memory_entries=int(cache_config.get("memory_entries", 1500)),
        sqlite_path=settings.path("cache.sqlite_path"),
        ttl_seconds=int(cache_config.get("ttl_seconds", 604800)),
        namespace=namespace,
        policy_fingerprint=policy_fingerprint,
        manifest_fingerprint=manifest_fingerprint,
    )
    diagnostics_config = settings.section("diagnostics")
    diagnostics = DiagnosticsLogger(diagnostics_config, settings.path("diagnostics.directory"))
    try:
        import onnxruntime as ort

        ort_version = ort.__version__
    except ImportError:
        ort_version = None
    runtime_info = {
        "platform": {"win32": "windows", "darwin": "macos"}.get(sys.platform, "linux"),
        "architecture": platform.machine(),
        "pythonVersion": platform.python_version(),
        "onnxRuntimeVersion": ort_version,
        "selectedProviders": selected,
        "transport": "mitmproxy",
        "manifestFingerprint": manifest_fingerprint,
    }
    pipeline = ImageSafetyPipeline(
        settings=settings.data,
        person_detector=person_detector,
        mobileclip=mobileclip,
        nudenet=nudenet,
        policy=policy,
        cache=cache,
        diagnostics=diagnostics,
        runtime_info=runtime_info,
    )
    return RuntimeContainer(
        settings=settings,
        pipeline=pipeline,
        selected_providers=selected,
        model_descriptions=descriptions,
        manifest=manifest,
    )
