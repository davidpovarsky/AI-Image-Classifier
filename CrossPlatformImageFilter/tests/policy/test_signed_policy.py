from __future__ import annotations

import base64
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from local_image_filter.cli import _parser, _settings
from local_image_filter.config import load_settings
from local_image_filter.policy.bundle import (
    PolicyBundleError,
    VerificationContext,
    apply_verified_policy,
    canonical_signed_payload,
    load_policy_json,
    verify_policy_bundle,
)
from local_image_filter.runtime.container import build_runtime

ROOT = Path(__file__).resolve().parents[2]
PRIVATE_KEY = Ed25519PrivateKey.from_private_bytes(bytes(range(1, 33)))
PUBLIC_KEY = PRIVATE_KEY.public_key().public_bytes(
    serialization.Encoding.Raw, serialization.PublicFormat.Raw
)


def signed_bundle(**changes: object) -> dict:
    bundle = json.loads((ROOT / "product/policy/defaults/base-policy.json").read_text())
    bundle.update(changes)
    bundle["signature"] = {
        "keyId": "test",
        "algorithm": "Ed25519",
        "signature": base64.b64encode(PRIVATE_KEY.sign(canonical_signed_payload(bundle))).decode(),
    }
    return bundle


def context(**changes: object) -> VerificationContext:
    values = {
        "trusted_keys": {"test": PUBLIC_KEY},
        "engine_version": "0.1.0",
        "product_version": "0.1.0",
        "now": datetime(2026, 7, 22, tzinfo=UTC),
    }
    values.update(changes)
    return VerificationContext(**values)


def test_base_policy_has_exact_default_parity() -> None:
    bundle = signed_bundle()
    verified = verify_policy_bundle(bundle, context())
    settings = load_settings(ROOT / "config/default.toml")
    effective = apply_verified_policy(settings, verified)
    for key, value in bundle["policy"].items():
        assert effective.section("policy")[key] == value
    mapped_processing = set(bundle["processing"]) - {
        "host_exceptions",
        "replacement_behavior",
        "diagnostics_privacy_level",
    }
    for key in mapped_processing:
        assert effective.section("processing")[key] == bundle["processing"][key]
    assert effective.section("proxy")["ignore_hosts"] == bundle["processing"]["host_exceptions"]


def test_signature_unknown_key_and_unsupported_algorithm_are_rejected() -> None:
    bundle = signed_bundle()
    bundle["policy"]["woman_min_score"] = 0.99
    with pytest.raises(PolicyBundleError, match="Invalid policy signature"):
        verify_policy_bundle(bundle, context())
    with pytest.raises(PolicyBundleError, match="Unknown"):
        verify_policy_bundle(signed_bundle(), context(trusted_keys={}))
    algorithm = signed_bundle()
    algorithm["signature"]["algorithm"] = "RSA"
    with pytest.raises(PolicyBundleError, match="Unsupported"):
        verify_policy_bundle(algorithm, context())


@pytest.mark.parametrize(
    ("changes", "context_changes", "message"),
    [
        ({"expiresAt": "2026-07-21T00:00:00Z"}, {}, "expired"),
        ({"issuedAt": "2026-07-23T00:00:00Z"}, {}, "not yet valid"),
        ({"revision": 2}, {"minimum_revision": 3}, "rollback"),
        (
            {"subject": {"type": "tenant", "id": "tenant-a"}},
            {"tenant_id": "tenant-b"},
            "tenant ID",
        ),
        (
            {"subject": {"type": "device", "id": "device-a"}},
            {"device_id": "device-b"},
            "device ID",
        ),
        ({"schemaVersion": 2}, {}, "schemaVersion"),
        ({"minimumEngineVersion": "9.0.0"}, {}, "newer engine"),
    ],
)
def test_temporal_revision_subject_schema_and_version_checks(
    changes: dict, context_changes: dict, message: str
) -> None:
    with pytest.raises(PolicyBundleError, match=message):
        verify_policy_bundle(signed_bundle(**changes), context(**context_changes))


def test_ranges_unexpected_fields_and_path_like_hosts_are_rejected() -> None:
    for mutate, message in (
        (lambda item: item["policy"].update(woman_min_score=2), "between"),
        (lambda item: item["policy"].update(executable_path="bad"), "Unexpected"),
        (lambda item: item["processing"].update(host_exceptions=["../secret"]), "[Hh]ost"),
    ):
        bundle = signed_bundle()
        mutate(bundle)
        bundle = signed_bundle(policy=bundle["policy"], processing=bundle["processing"])
        with pytest.raises(PolicyBundleError, match=message):
            verify_policy_bundle(bundle, context())


def test_duplicate_keys_and_nonfinite_numbers_are_rejected(tmp_path: Path) -> None:
    duplicate = tmp_path / "duplicate.json"
    duplicate.write_text('{"revision":1,"revision":2}', encoding="utf-8")
    with pytest.raises(PolicyBundleError, match="Duplicate"):
        load_policy_json(duplicate)
    nonfinite = tmp_path / "nonfinite.json"
    nonfinite.write_text('{"value":NaN}', encoding="utf-8")
    with pytest.raises(PolicyBundleError, match="Non-finite"):
        load_policy_json(nonfinite)


def test_policy_revision_participates_in_cache_namespace(tmp_path: Path) -> None:
    base = load_settings(ROOT / "config/mock.toml")
    base.data["cache"]["sqlite_path"] = str(tmp_path / "cache.sqlite3")
    base.data["diagnostics"]["directory"] = str(tmp_path / "diagnostics")
    first_bundle = verify_policy_bundle(signed_bundle(revision=1), context())
    second_bundle = verify_policy_bundle(signed_bundle(revision=2), context())
    first = build_runtime(apply_verified_policy(base, first_bundle)).pipeline.cache.namespace
    second = build_runtime(apply_verified_policy(base, second_bundle)).pipeline.cache.namespace
    assert first != second


def test_expiration_boundary_is_fail_closed() -> None:
    now = datetime(2026, 7, 22, tzinfo=UTC)
    bundle = signed_bundle(
        expiresAt=(now + timedelta(seconds=1)).isoformat().replace("+00:00", "Z")
    )
    assert verify_policy_bundle(bundle, context(now=now))["revision"] == 1


def test_active_policy_precedence_is_vendor_tenant_device(tmp_path: Path) -> None:
    trusted = tmp_path / "trusted.json"
    trusted.write_text(
        json.dumps({"keys": {"test": base64.b64encode(PUBLIC_KEY).decode()}}),
        encoding="utf-8",
    )
    bundles: list[Path] = []
    for index, (kind, identifier, score) in enumerate(
        (
            ("vendor-global", "vendor", 0.11),
            ("tenant", "tenant-a", 0.22),
            ("device", "device-a", 0.33),
        )
    ):
        policy = signed_bundle()["policy"]
        policy["woman_min_score"] = score
        path = tmp_path / f"{index}.json"
        path.write_text(
            json.dumps(
                signed_bundle(
                    revision=index + 1,
                    subject={"type": kind, "id": identifier},
                    policy=policy,
                )
            ),
            encoding="utf-8",
        )
        bundles.append(path)
    arguments = [
        "print-config",
        "--config",
        str(ROOT / "config/mock.toml"),
        "--active-policy-trusted-keys",
        str(trusted),
        "--active-policy-tenant-id",
        "tenant-a",
        "--active-policy-device-id",
        "device-a",
    ]
    for bundle in bundles:
        arguments.extend(("--active-policy-bundle", str(bundle)))
    settings = _settings(_parser().parse_args(arguments))
    assert settings.section("policy")["woman_min_score"] == 0.33
