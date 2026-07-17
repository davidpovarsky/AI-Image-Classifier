from __future__ import annotations

import fnmatch
from dataclasses import dataclass
from enum import StrEnum
from typing import Any


class ResponseDisposition(StrEnum):
    IGNORE = "ignore"
    INSPECT = "inspect"
    FAIL_CLOSED_IMAGE = "failClosedImage"


@dataclass(frozen=True, slots=True)
class EligibilityResult:
    disposition: ResponseDisposition
    reason: str
    mime_type: str


class ResponseEligibility:
    def __init__(self, config: dict[str, Any]) -> None:
        self.allowed_content_types = {
            str(value).lower() for value in config.get("allowed_content_types", [])
        }
        self.ignore_hosts = [str(value).lower() for value in config.get("ignore_hosts", [])]
        self.max_body_bytes = int(config.get("max_body_bytes", 25_000_000))

    def is_ignored_host(self, host: str) -> bool:
        lowered = host.lower()
        return any(
            lowered == pattern or fnmatch.fnmatch(lowered, pattern) for pattern in self.ignore_hosts
        )

    def classify(
        self,
        *,
        host: str,
        status_code: int,
        content_type: str,
        content_length: int,
        has_range: bool,
    ) -> EligibilityResult:
        normalized_type = content_type.split(";", 1)[0].strip().lower()
        is_image = normalized_type.startswith("image/")
        if self.is_ignored_host(host):
            return EligibilityResult(ResponseDisposition.IGNORE, "ignoredHost", normalized_type)
        if not is_image:
            return EligibilityResult(ResponseDisposition.IGNORE, "nonImage", normalized_type)
        if status_code < 200 or status_code >= 300:
            return EligibilityResult(ResponseDisposition.IGNORE, "nonSuccessImage", normalized_type)
        if has_range or status_code == 206:
            return EligibilityResult(
                ResponseDisposition.FAIL_CLOSED_IMAGE, "partialImageContent", normalized_type
            )
        if normalized_type not in self.allowed_content_types:
            return EligibilityResult(
                ResponseDisposition.FAIL_CLOSED_IMAGE, "unsupportedImageFormat", normalized_type
            )
        if content_length <= 0:
            return EligibilityResult(
                ResponseDisposition.FAIL_CLOSED_IMAGE, "emptyImage", normalized_type
            )
        if content_length > self.max_body_bytes:
            return EligibilityResult(
                ResponseDisposition.FAIL_CLOSED_IMAGE,
                "imageTooLargeForInspection",
                normalized_type,
            )
        return EligibilityResult(ResponseDisposition.INSPECT, "eligibleImage", normalized_type)
