from __future__ import annotations

import base64
import random
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import unquote, urlsplit

import pytest
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from tuf.api.exceptions import DownloadHTTPError
from tuf.ngclient.fetcher import FetcherInterface

from local_image_filter.config import load_settings
from local_image_filter.policy.bundle import VerificationContext
from local_image_filter.policy.tuf_client import (
    AtomicPolicyStore,
    RefreshSchedule,
    TufPolicyClient,
    TufPolicyError,
    _validated_origin,
    _validated_target_path,
)

ROOT = Path(__file__).resolve().parents[2]


class RepositoryFetcher(FetcherInterface):
    def __init__(self, repository: Path) -> None:
        self.repository = repository

    def _fetch(self, url: str):  # type: ignore[no-untyped-def]
        relative = unquote(urlsplit(url).path).removeprefix("/")
        path = self.repository / relative
        if not path.is_file():
            raise DownloadHTTPError("not found", 404)
        yield path.read_bytes()


def test_repository_origin_is_https_and_compiled_allowlisted() -> None:
    allowed = frozenset({"https://policy.example.com"})
    assert (
        _validated_origin("https://policy.example.com/tuf", allowed)
        == "https://policy.example.com/tuf/"
    )
    for rejected in (
        "http://policy.example.com/tuf",
        "https://attacker.example/tuf",
        "https://user:password@policy.example.com/tuf",
        "https://policy.example.com/tuf?target=attacker",
    ):
        with pytest.raises(TufPolicyError):
            _validated_origin(rejected, allowed)


def test_target_paths_are_narrow_and_traversal_safe() -> None:
    assert _validated_target_path("policies/channels/stable/policy-bundle.json")
    assert _validated_target_path("policies/tenants/tenant-123/policy-bundle.json")
    assert _validated_target_path("policies/devices/device-123/policy-bundle.json")
    for rejected in (
        "../policy-bundle.json",
        "policies/channels/arbitrary/policy-bundle.json",
        "policies/devices/../../commands.json",
        "executables/engine.exe",
    ):
        with pytest.raises(TufPolicyError):
            _validated_target_path(rejected)


def test_atomic_store_preserves_last_known_good(tmp_path: Path) -> None:
    store = AtomicPolicyStore(tmp_path)
    store.activate(b"first")
    store.activate(b"second")
    assert store.active_path.read_bytes() == b"second"
    assert store.last_known_good_path.read_bytes() == b"first"


def test_refresh_schedule_has_bounded_backoff_and_jitter() -> None:
    schedule = RefreshSchedule(base_seconds=100, maximum_backoff_seconds=500)
    delay = schedule.delay(20, entropy=random.Random(1))
    assert 400 <= delay <= 600


def test_generated_internal_repository_verifies_all_policy_scopes(tmp_path: Path) -> None:
    tenant_id = "11111111-1111-4111-8111-111111111111"
    device_id = "22222222-2222-4222-8222-222222222222"
    repository = tmp_path / "repository"
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "product/scripts/generate_internal_tuf_repository.py"),
            "--output",
            str(repository),
            "--base-policy",
            str(ROOT / "product/policy/defaults/base-policy.json"),
            "--private-key",
            str(ROOT / "product/policy/test-keys/development-private-key.json"),
            "--tenant-id",
            tenant_id,
            "--device-id",
            device_id,
        ],
        check=True,
    )
    private_key = Ed25519PrivateKey.from_private_bytes(bytes(range(1, 33)))
    public_key = private_key.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    settings = load_settings(ROOT / "config/default.toml")
    targets = (
        ("policies/channels/stable/policy-bundle.json", None, None, 1),
        (f"policies/tenants/{tenant_id}/policy-bundle.json", tenant_id, None, 2),
        (f"policies/devices/{device_id}/policy-bundle.json", None, device_id, 3),
    )
    for index, (target, tenant, device, revision) in enumerate(targets):
        client = TufPolicyClient(
            metadata_directory=tmp_path / f"metadata-{index}",
            target_directory=tmp_path / f"targets-{index}",
            metadata_base_url="https://internal.example/metadata",
            target_base_url="https://internal.example/targets",
            bootstrap_root=(repository / "metadata/root.json").read_bytes(),
            allowed_origins=frozenset({"https://internal.example"}),
            fetcher=RepositoryFetcher(repository),
        )
        verified, content = client.fetch_verified_bundle(
            target,
            context=VerificationContext(
                trusted_keys={"development-policy-2026": public_key},
                engine_version="0.1.0",
                product_version="0.1.0",
                now=datetime.now(UTC),
                tenant_id=tenant,
                device_id=device,
            ),
            base_settings=settings,
        )
        assert verified["revision"] == revision
        assert base64.b64decode(verified["signature"]["signature"], validate=True)
        assert content
