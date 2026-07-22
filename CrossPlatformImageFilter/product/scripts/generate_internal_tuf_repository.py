from __future__ import annotations

import argparse
import base64
import copy
import hashlib
import json
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import rfc8785
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def canonical_tuf(value: dict[str, Any]) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode(
        "utf-8"
    )


def signed_metadata(signed: dict[str, Any], key_id: str, private_key: Ed25519PrivateKey) -> bytes:
    signature = private_key.sign(canonical_tuf(signed)).hex()
    return (
        json.dumps(
            {"signatures": [{"keyid": key_id, "sig": signature}], "signed": signed},
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n"
    ).encode("utf-8")


def descriptor(content: bytes, version: int | None = None) -> dict[str, Any]:
    result: dict[str, Any] = {
        "length": len(content),
        "hashes": {"sha256": hashlib.sha256(content).hexdigest()},
    }
    if version is not None:
        result["version"] = version
    return result


def sign_policy(
    base: dict[str, Any],
    *,
    key_id: str,
    private_key: Ed25519PrivateKey,
    subject_type: str,
    subject_id: str,
    policy_id: str,
    revision: int,
) -> bytes:
    policy = copy.deepcopy(base)
    policy.update(
        {
            "policyId": policy_id,
            "revision": revision,
            "subject": {"type": subject_type, "id": subject_id},
        }
    )
    policy.pop("signature", None)
    policy["signature"] = {
        "keyId": key_id,
        "algorithm": "Ed25519",
        "signature": base64.b64encode(private_key.sign(rfc8785.dumps(policy))).decode("ascii"),
    }
    return (json.dumps(policy, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def write_target(root: Path, relative_path: str, content: bytes) -> dict[str, Any]:
    digest = hashlib.sha256(content).hexdigest()
    destination = root / relative_path
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(content)
    consistent = destination.with_name(f"{digest}.{destination.name}")
    consistent.write_bytes(content)
    return descriptor(content)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--base-policy", type=Path, required=True)
    parser.add_argument("--private-key", type=Path, required=True)
    parser.add_argument("--tenant-id", required=True)
    parser.add_argument("--device-id", required=True)
    parser.add_argument("--key-id", default="development-policy-2026")
    parser.add_argument("--root-output", type=Path)
    args = parser.parse_args()

    private_document = json.loads(args.private_key.read_text(encoding="utf-8"))
    if private_document.get("testOnly") is not True:
        raise SystemExit("internal TUF generation accepts an explicitly test-only key")
    seed = base64.b64decode(private_document["privateSeedBase64"], validate=True)
    private_key = Ed25519PrivateKey.from_private_bytes(seed)
    public_key = private_key.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    base_policy = json.loads(args.base_policy.read_text(encoding="utf-8"))
    output = args.output.resolve()
    metadata = output / "metadata"
    targets = output / "targets"
    metadata.mkdir(parents=True, exist_ok=True)
    targets.mkdir(parents=True, exist_ok=True)

    expires = (datetime.now(UTC) + timedelta(days=3650)).strftime("%Y-%m-%dT%H:%M:%SZ")
    root_signed = {
        "_type": "root",
        "spec_version": "1.0.31",
        "version": 1,
        "expires": expires,
        "consistent_snapshot": True,
        "keys": {
            args.key_id: {
                "keytype": "ed25519",
                "scheme": "ed25519",
                "keyval": {"public": public_key.hex()},
            }
        },
        "roles": {
            role: {"keyids": [args.key_id], "threshold": 1}
            for role in ("root", "snapshot", "targets", "timestamp")
        },
    }
    root_bytes = signed_metadata(root_signed, args.key_id, private_key)
    (metadata / "1.root.json").write_bytes(root_bytes)
    (metadata / "root.json").write_bytes(root_bytes)
    if args.root_output:
        args.root_output.parent.mkdir(parents=True, exist_ok=True)
        args.root_output.write_bytes(root_bytes)

    target_descriptors = {
        "policies/channels/stable/policy-bundle.json": write_target(
            targets,
            "policies/channels/stable/policy-bundle.json",
            sign_policy(
                base_policy,
                key_id=args.key_id,
                private_key=private_key,
                subject_type="vendor-global",
                subject_id="local-ai-image-filter",
                policy_id="vendor-base",
                revision=1,
            ),
        ),
        f"policies/tenants/{args.tenant_id}/policy-bundle.json": write_target(
            targets,
            f"policies/tenants/{args.tenant_id}/policy-bundle.json",
            sign_policy(
                base_policy,
                key_id=args.key_id,
                private_key=private_key,
                subject_type="tenant",
                subject_id=args.tenant_id,
                policy_id="internal-tenant",
                revision=2,
            ),
        ),
        f"policies/devices/{args.device_id}/policy-bundle.json": write_target(
            targets,
            f"policies/devices/{args.device_id}/policy-bundle.json",
            sign_policy(
                base_policy,
                key_id=args.key_id,
                private_key=private_key,
                subject_type="device",
                subject_id=args.device_id,
                policy_id="internal-device",
                revision=3,
            ),
        ),
    }
    targets_signed = {
        "_type": "targets",
        "spec_version": "1.0.31",
        "version": 1,
        "expires": expires,
        "targets": target_descriptors,
    }
    targets_bytes = signed_metadata(targets_signed, args.key_id, private_key)
    (metadata / "1.targets.json").write_bytes(targets_bytes)
    (metadata / "targets.json").write_bytes(targets_bytes)
    snapshot_signed = {
        "_type": "snapshot",
        "spec_version": "1.0.31",
        "version": 1,
        "expires": expires,
        "meta": {"targets.json": descriptor(targets_bytes, 1)},
    }
    snapshot_bytes = signed_metadata(snapshot_signed, args.key_id, private_key)
    (metadata / "1.snapshot.json").write_bytes(snapshot_bytes)
    (metadata / "snapshot.json").write_bytes(snapshot_bytes)
    timestamp_signed = {
        "_type": "timestamp",
        "spec_version": "1.0.31",
        "version": 1,
        "expires": expires,
        "meta": {"snapshot.json": descriptor(snapshot_bytes, 1)},
    }
    (metadata / "timestamp.json").write_bytes(
        signed_metadata(timestamp_signed, args.key_id, private_key)
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
