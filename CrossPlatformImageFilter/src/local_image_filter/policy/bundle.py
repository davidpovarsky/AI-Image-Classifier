from __future__ import annotations

import base64
import copy
import json
import math
import re
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import rfc8785
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from ..config import Settings, validate_settings

MAX_POLICY_BYTES = 1_048_576
POLICY_FIELDS = {
    "block_action",
    "woman_min_score",
    "woman_min_margin_over_man",
    "face_fallback_woman_min_score",
    "whole_image_fallback_woman_min_score",
    "uncertain_min_score",
    "block_woman_without_margin_if_score_at_least",
    "nudity_thresholds",
}
PROCESSING_FIELDS = {
    "minimum_width",
    "minimum_height",
    "minimum_area",
    "maximum_dimension",
    "maximum_total_pixels",
    "maximum_person_crops",
    "horizontal_crop_padding",
    "vertical_crop_padding",
    "blur_radius",
    "replacement_behavior",
    "animated_image_mode",
    "host_exceptions",
    "diagnostics_privacy_level",
}
TOP_LEVEL_FIELDS = {
    "schemaVersion",
    "policyId",
    "revision",
    "channel",
    "issuedAt",
    "expiresAt",
    "minimumEngineVersion",
    "minimumProductVersion",
    "subject",
    "policy",
    "processing",
    "metadata",
    "signature",
}
SUBJECT_FIELDS = {"type", "id"}
SIGNATURE_FIELDS = {"keyId", "algorithm", "signature"}
SAFE_IDENTIFIER = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")


class PolicyBundleError(ValueError):
    pass


def _reject_constant(value: str) -> None:
    raise PolicyBundleError(f"Non-finite JSON number is forbidden: {value}")


def _object_no_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise PolicyBundleError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def load_policy_json(path: Path, *, maximum_bytes: int = MAX_POLICY_BYTES) -> dict[str, Any]:
    try:
        size = path.stat().st_size
    except OSError as error:
        raise PolicyBundleError(f"Unable to inspect policy bundle: {error}") from error
    if size > maximum_bytes:
        raise PolicyBundleError(f"Policy bundle exceeds {maximum_bytes} bytes")
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_object_no_duplicates,
            parse_constant=_reject_constant,
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise PolicyBundleError(f"Unable to parse policy bundle: {error}") from error
    if not isinstance(value, dict):
        raise PolicyBundleError("Policy bundle must be a JSON object")
    return value


def canonical_signed_payload(bundle: dict[str, Any]) -> bytes:
    payload = copy.deepcopy(bundle)
    payload.pop("signature", None)
    try:
        return rfc8785.dumps(payload)
    except (TypeError, ValueError) as error:
        raise PolicyBundleError(
            f"Policy payload is not RFC 8785 canonicalizable: {error}"
        ) from error


def _strict_fields(value: dict[str, Any], allowed: set[str], location: str) -> None:
    unexpected = sorted(set(value) - allowed)
    if unexpected:
        raise PolicyBundleError(f"Unexpected {location} fields: {', '.join(unexpected)}")


def _timestamp(value: Any, name: str) -> datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise PolicyBundleError(f"{name} must be an RFC 3339 UTC timestamp")
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError as error:
        raise PolicyBundleError(f"{name} is invalid: {error}") from error
    return parsed.astimezone(UTC)


def _version(value: Any, name: str) -> tuple[int, int, int]:
    if (
        not isinstance(value, str)
        or re.fullmatch(r"(?:0|[1-9]\d*)(?:\.(?:0|[1-9]\d*)){0,2}", value) is None
    ):
        raise PolicyBundleError(f"{name} must be a stable numeric semantic version")
    parts = tuple(int(part) for part in value.split("."))
    padded = parts + (0, 0, 0)
    return padded[0], padded[1], padded[2]


def _number(value: Any, name: str, minimum: float, maximum: float) -> None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PolicyBundleError(f"{name} must be numeric")
    if not math.isfinite(float(value)) or not minimum <= float(value) <= maximum:
        raise PolicyBundleError(f"{name} must be between {minimum} and {maximum}")


def _validate_policy_values(policy: dict[str, Any], processing: dict[str, Any]) -> None:
    _strict_fields(policy, POLICY_FIELDS, "policy")
    _strict_fields(processing, PROCESSING_FIELDS, "processing")
    if policy.get("block_action") not in {"blur", "replace", "error"}:
        raise PolicyBundleError("policy.block_action must be blur, replace, or error")
    for name in POLICY_FIELDS - {"block_action", "nudity_thresholds"}:
        _number(policy.get(name), f"policy.{name}", 0, 1.01)
    thresholds = policy.get("nudity_thresholds")
    if not isinstance(thresholds, dict) or not thresholds:
        raise PolicyBundleError("policy.nudity_thresholds must be a non-empty object")
    for label, value in thresholds.items():
        if SAFE_IDENTIFIER.fullmatch(label) is None:
            raise PolicyBundleError(f"Invalid nudity label: {label}")
        _number(value, f"policy.nudity_thresholds.{label}", 0, 1.01)

    integer_ranges = {
        "minimum_width": (1, 16_384),
        "minimum_height": (1, 16_384),
        "minimum_area": (1, 268_435_456),
        "maximum_dimension": (1, 16_384),
        "maximum_total_pixels": (1, 268_435_456),
        "maximum_person_crops": (1, 128),
    }
    for name, (minimum, maximum) in integer_ranges.items():
        value = processing.get(name)
        if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
            raise PolicyBundleError(f"processing.{name} must be an integer in range")
    for name in ("horizontal_crop_padding", "vertical_crop_padding"):
        _number(processing.get(name), f"processing.{name}", 0, 1)
    _number(processing.get("blur_radius"), "processing.blur_radius", 0, 512)
    if processing.get("replacement_behavior") not in {"placeholder", "solid"}:
        raise PolicyBundleError("processing.replacement_behavior is unsupported")
    if processing.get("animated_image_mode") not in {"replace", "sample"}:
        raise PolicyBundleError("processing.animated_image_mode is unsupported")
    if processing.get("diagnostics_privacy_level") not in {"strict", "minimal"}:
        raise PolicyBundleError("processing.diagnostics_privacy_level is unsupported")
    hosts = processing.get("host_exceptions")
    if not isinstance(hosts, list) or len(hosts) > 128:
        raise PolicyBundleError("processing.host_exceptions must be an array of at most 128 hosts")
    for host in hosts:
        if (
            not isinstance(host, str)
            or len(host) > 253
            or any(token in host for token in ("/", "\\", ".."))
        ):
            raise PolicyBundleError("Host exceptions may contain host patterns only")


@dataclass(frozen=True, slots=True)
class VerificationContext:
    trusted_keys: dict[str, bytes]
    engine_version: str
    product_version: str
    device_id: str | None = None
    minimum_revision: int = 0
    now: datetime | None = None


def verify_policy_bundle(bundle: dict[str, Any], context: VerificationContext) -> dict[str, Any]:
    _strict_fields(bundle, TOP_LEVEL_FIELDS, "top-level")
    missing = sorted(TOP_LEVEL_FIELDS - set(bundle))
    if missing:
        raise PolicyBundleError(f"Missing policy fields: {', '.join(missing)}")
    if bundle.get("schemaVersion") != 1:
        raise PolicyBundleError("Unsupported policy schemaVersion")
    for name in ("policyId", "channel"):
        value = bundle.get(name)
        if not isinstance(value, str) or SAFE_IDENTIFIER.fullmatch(value) is None:
            raise PolicyBundleError(f"{name} is invalid")
    revision = bundle.get("revision")
    if isinstance(revision, bool) or not isinstance(revision, int) or revision < 1:
        raise PolicyBundleError("revision must be a positive integer")
    if revision < context.minimum_revision:
        raise PolicyBundleError("Policy revision rollback rejected")

    now = (context.now or datetime.now(UTC)).astimezone(UTC)
    issued = _timestamp(bundle.get("issuedAt"), "issuedAt")
    expires = _timestamp(bundle.get("expiresAt"), "expiresAt")
    if issued > now:
        raise PolicyBundleError("Policy is not yet valid")
    if expires <= now or expires <= issued:
        raise PolicyBundleError("Policy is expired")
    if _version(bundle.get("minimumEngineVersion"), "minimumEngineVersion") > _version(
        context.engine_version, "engine_version"
    ):
        raise PolicyBundleError("Policy requires a newer engine")
    if _version(bundle.get("minimumProductVersion"), "minimumProductVersion") > _version(
        context.product_version, "product_version"
    ):
        raise PolicyBundleError("Policy requires a newer product")

    subject = bundle.get("subject")
    if not isinstance(subject, dict):
        raise PolicyBundleError("subject must be an object")
    _strict_fields(subject, SUBJECT_FIELDS, "subject")
    subject_type = subject.get("type")
    subject_id = subject.get("id")
    if subject_type not in {"vendor-global", "tenant", "device"}:
        raise PolicyBundleError("subject.type is unsupported")
    if not isinstance(subject_id, str) or SAFE_IDENTIFIER.fullmatch(subject_id) is None:
        raise PolicyBundleError("subject.id is invalid")
    if subject_type == "device" and subject_id != context.device_id:
        raise PolicyBundleError("Policy device ID does not match this device")

    policy = bundle.get("policy")
    processing = bundle.get("processing")
    metadata = bundle.get("metadata")
    if (
        not isinstance(policy, dict)
        or not isinstance(processing, dict)
        or not isinstance(metadata, dict)
    ):
        raise PolicyBundleError("policy, processing, and metadata must be objects")
    _validate_policy_values(policy, processing)

    signature = bundle.get("signature")
    if not isinstance(signature, dict):
        raise PolicyBundleError("signature must be an object")
    _strict_fields(signature, SIGNATURE_FIELDS, "signature")
    if signature.get("algorithm") != "Ed25519":
        raise PolicyBundleError("Unsupported policy signature algorithm")
    key_id = signature.get("keyId")
    if not isinstance(key_id, str) or key_id not in context.trusted_keys:
        raise PolicyBundleError("Unknown policy signing key")
    try:
        encoded_signature = base64.b64decode(signature.get("signature", ""), validate=True)
        Ed25519PublicKey.from_public_bytes(context.trusted_keys[key_id]).verify(
            encoded_signature, canonical_signed_payload(bundle)
        )
    except (ValueError, TypeError, InvalidSignature) as error:
        raise PolicyBundleError("Invalid policy signature") from error
    return copy.deepcopy(bundle)


def apply_verified_policy(settings: Settings, bundle: dict[str, Any]) -> Settings:
    data = settings.to_dict()
    data["policy"].update(copy.deepcopy(bundle["policy"]))
    processing = copy.deepcopy(bundle["processing"])
    hosts = processing.pop("host_exceptions")
    processing.pop("diagnostics_privacy_level")
    processing.pop("replacement_behavior")
    data["processing"].update(processing)
    data["proxy"]["ignore_hosts"] = hosts
    data["signed_policy"] = {
        "policy_id": bundle["policyId"],
        "revision": bundle["revision"],
        "channel": bundle["channel"],
        "subject": copy.deepcopy(bundle["subject"]),
    }
    result = Settings(
        data, settings.source_path, settings.source_description, settings.project_root
    )
    validate_settings(result)
    return result


def load_trusted_keys(path: Path) -> dict[str, bytes]:
    value = load_policy_json(path)
    keys = value.get("keys")
    if not isinstance(keys, dict):
        raise PolicyBundleError("Trusted key file must contain a keys object")
    result: dict[str, bytes] = {}
    for key_id, encoded in keys.items():
        if not isinstance(key_id, str) or not isinstance(encoded, str):
            raise PolicyBundleError("Trusted key entries must be base64 strings")
        try:
            public = base64.b64decode(encoded, validate=True)
        except ValueError as error:
            raise PolicyBundleError(f"Trusted key {key_id} is not valid base64") from error
        if len(public) != 32:
            raise PolicyBundleError(f"Trusted key {key_id} is not an Ed25519 public key")
        result[key_id] = public
    return result
