from __future__ import annotations

import argparse
import base64
import json
from pathlib import Path
from typing import Any

import rfc8785
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

EXPECTED_MODELS = {"mobileCLIP2", "nudeNet", "personDetector"}


def load_object(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def verify(
    approval: dict[str, Any], manifest: dict[str, Any], key_id: str, public_key: bytes
) -> None:
    if approval.get("approvalId") == "EXAMPLE-NOT-AN-APPROVAL":
        raise ValueError("The example approval document can never authorize a release")
    models = approval.get("models")
    if not isinstance(models, list):
        raise ValueError("Approval models must be an array")
    by_identifier = {item.get("modelIdentifier"): item for item in models if isinstance(item, dict)}
    if set(by_identifier) != EXPECTED_MODELS:
        raise ValueError("Approval must contain each redistributed model exactly once")
    runtime_models = manifest.get("models")
    if not isinstance(runtime_models, dict):
        raise ValueError("Runtime manifest models are missing")
    for identifier in EXPECTED_MODELS:
        item = by_identifier[identifier]
        if item.get("commercialRedistributionApproved") is not True:
            raise ValueError(f"Commercial redistribution is not approved for {identifier}")
        runtime = runtime_models.get(identifier)
        if not isinstance(runtime, dict) or item.get("approvedRuntimeSHA256") != runtime.get(
            "sha256"
        ):
            raise ValueError(f"Approved hash does not match the runtime artifact for {identifier}")
        for required in ("reviewer", "reviewDate", "approvalReference"):
            if item.get(required) in {None, "", "NONE", "UNREVIEWED", "1970-01-01"}:
                raise ValueError(f"Approval field {required} is not real for {identifier}")
    signature = approval.get("signature")
    if not isinstance(signature, dict) or signature.get("algorithm") != "Ed25519":
        raise ValueError("Approval must use Ed25519")
    if signature.get("keyId") != key_id:
        raise ValueError("Approval signing key is not trusted")
    payload = dict(approval)
    payload.pop("signature", None)
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(
            base64.b64decode(signature.get("signature", ""), validate=True), rfc8785.dumps(payload)
        )
    except (InvalidSignature, TypeError, ValueError) as error:
        raise ValueError("Approval signature is invalid") from error


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--approval", type=Path, required=True)
    parser.add_argument("--runtime-manifest", type=Path, required=True)
    parser.add_argument("--key-id", required=True)
    parser.add_argument("--public-key-base64", required=True)
    arguments = parser.parse_args()
    verify(
        load_object(arguments.approval),
        load_object(arguments.runtime_manifest),
        arguments.key_id,
        base64.b64decode(arguments.public_key_base64, validate=True),
    )
    print("Commercial model distribution approval verified")


if __name__ == "__main__":
    main()
