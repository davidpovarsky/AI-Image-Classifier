from __future__ import annotations

import random
from pathlib import Path

import pytest

from local_image_filter.policy.tuf_client import (
    AtomicPolicyStore,
    RefreshSchedule,
    TufPolicyError,
    _validated_origin,
    _validated_target_path,
)


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
