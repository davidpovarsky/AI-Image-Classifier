from __future__ import annotations

import os
import random
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from tuf.ngclient import Updater
from tuf.ngclient.fetcher import FetcherInterface

from ..config import Settings
from .bundle import (
    VerificationContext,
    apply_verified_policy,
    load_policy_json,
    verify_policy_bundle,
)

TARGET_PATTERN = re.compile(
    r"policies/(?:channels/(?:stable|beta)|devices/[A-Za-z0-9][A-Za-z0-9._-]{0,127})/"
    r"policy-bundle\.json\Z"
)


class TufPolicyError(ValueError):
    """A policy repository or activation transaction was rejected."""


@dataclass(frozen=True)
class RefreshSchedule:
    base_seconds: int = 21_600
    maximum_backoff_seconds: int = 86_400
    jitter_fraction: float = 0.2

    def delay(self, failures: int, *, entropy: random.Random | None = None) -> float:
        if failures < 0:
            raise TufPolicyError("failure count cannot be negative")
        generator = entropy or random.SystemRandom()
        base = min(self.base_seconds * (2 ** min(failures, 8)), self.maximum_backoff_seconds)
        return base * generator.uniform(1 - self.jitter_fraction, 1 + self.jitter_fraction)


def _validated_origin(url: str, allowed_origins: frozenset[str]) -> str:
    parts = urlsplit(url)
    if parts.scheme != "https" or not parts.netloc or parts.username or parts.password:
        raise TufPolicyError("TUF repository URL must use an authenticated HTTPS origin")
    origin = f"https://{parts.netloc}"
    if origin not in allowed_origins:
        raise TufPolicyError("TUF repository origin is not compiled into the product allowlist")
    if parts.query or parts.fragment:
        raise TufPolicyError("TUF repository URL must not contain query or fragment components")
    return url.rstrip("/") + "/"


def _validated_target_path(target_path: str) -> str:
    if not TARGET_PATTERN.fullmatch(target_path) or ".." in target_path:
        raise TufPolicyError("policy target path is not allowlisted")
    return target_path


class AtomicPolicyStore:
    def __init__(self, directory: Path) -> None:
        self.directory = directory.resolve()
        self.active_path = self.directory / "active-policy.json"
        self.last_known_good_path = self.directory / "last-known-good-policy.json"

    def activate(self, verified_bytes: bytes) -> None:
        self.directory.mkdir(parents=True, exist_ok=True)
        descriptor, temporary_name = tempfile.mkstemp(
            prefix="policy-", suffix=".staged", dir=self.directory
        )
        temporary = Path(temporary_name)
        try:
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(verified_bytes)
                stream.flush()
                os.fsync(stream.fileno())
            if self.active_path.is_file():
                _atomic_write(self.last_known_good_path, self.active_path.read_bytes())
            os.replace(temporary, self.active_path)
        finally:
            temporary.unlink(missing_ok=True)


def _atomic_write(path: Path, content: bytes) -> None:
    descriptor, temporary_name = tempfile.mkstemp(prefix=f"{path.name}-", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


class TufPolicyClient:
    def __init__(
        self,
        *,
        metadata_directory: Path,
        target_directory: Path,
        metadata_base_url: str,
        target_base_url: str,
        bootstrap_root: bytes,
        allowed_origins: frozenset[str],
        fetcher: FetcherInterface | None = None,
    ) -> None:
        if not bootstrap_root:
            raise TufPolicyError("trusted TUF bootstrap root is required")
        metadata_url = _validated_origin(metadata_base_url, allowed_origins)
        target_url = _validated_origin(target_base_url, allowed_origins)
        self.target_directory = target_directory.resolve()
        self.target_directory.mkdir(parents=True, exist_ok=True)
        metadata_directory.resolve().mkdir(parents=True, exist_ok=True)
        self.updater = Updater(
            metadata_dir=str(metadata_directory.resolve()),
            metadata_base_url=metadata_url,
            target_dir=str(self.target_directory),
            target_base_url=target_url,
            fetcher=fetcher,
            bootstrap=bootstrap_root,
        )

    def fetch_verified_bundle(
        self,
        target_path: str,
        *,
        context: VerificationContext,
        base_settings: Settings,
    ) -> tuple[dict[str, Any], bytes]:
        target_path = _validated_target_path(target_path)
        self.updater.refresh()
        target = self.updater.get_targetinfo(target_path)
        if target is None:
            raise TufPolicyError("policy target is not present in trusted TUF metadata")
        with tempfile.NamedTemporaryFile(
            prefix="policy-download-", dir=self.target_directory, delete=False
        ) as temporary:
            destination = Path(temporary.name)
        try:
            downloaded = Path(self.updater.download_target(target, filepath=str(destination)))
            content = downloaded.read_bytes()
            bundle = load_policy_json(downloaded)
            verified = verify_policy_bundle(bundle, context)
            apply_verified_policy(base_settings, verified)
            return verified, content
        finally:
            destination.unlink(missing_ok=True)
