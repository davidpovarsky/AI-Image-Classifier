from __future__ import annotations

import asyncio
import os
from typing import Any

from mitmproxy import ctx, http

from local_image_filter.config import load_settings
from local_image_filter.domain.models import FilterAction
from local_image_filter.image.rewrite import create_failure_placeholder
from local_image_filter.proxy.filters import ResponseDisposition, ResponseEligibility
from local_image_filter.runtime.container import RuntimeContainer, build_runtime

INVALIDATED_HEADERS = (
    "content-encoding",
    "content-range",
    "accept-ranges",
    "etag",
    "content-md5",
    "last-modified",
)


def apply_replacement(
    response: Any,
    body: bytes,
    mime_type: str,
    *,
    reason: str,
    cache: str = "miss",
) -> None:
    response.status_code = 200
    response.content = body
    response.headers["content-type"] = mime_type
    response.headers["content-length"] = str(len(body))
    response.headers["cache-control"] = "no-store"
    response.headers["x-local-image-filter"] = "replace"
    response.headers["x-local-image-filter-cache"] = cache
    response.headers["x-local-image-filter-reason"] = reason
    for header in INVALIDATED_HEADERS:
        response.headers.pop(header, None)


class LocalImageFilterAddon:
    def __init__(self) -> None:
        self.runtime: RuntimeContainer | None = None
        self.eligibility: ResponseEligibility | None = None
        self.semaphore: asyncio.Semaphore | None = None

    def load(self, loader: Any) -> None:
        loader.add_option(
            name="local_image_filter_config",
            typespec=str,
            default=os.environ.get("LOCAL_IMAGE_FILTER_CONFIG", ""),
            help="Path to Local AI Image Filter TOML configuration",
        )

    def running(self) -> None:
        configured = str(ctx.options.local_image_filter_config).strip() or None
        settings = load_settings(configured)
        self.runtime = build_runtime(settings)
        proxy_config = settings.section("proxy")
        self.eligibility = ResponseEligibility(proxy_config)
        concurrency = min(
            int(proxy_config.get("max_parallel_images", 2)),
            int(settings.section("runtime").get("max_concurrent_inference", 1)),
        )
        self.semaphore = asyncio.Semaphore(concurrency)
        ignored = [str(value) for value in proxy_config.get("ignore_hosts", [])]
        if ignored:
            ctx.options.update(ignore_hosts=ignored)
        ctx.log.info(
            f"Local image filter ready; config={settings.source_description}; "
            f"providers={self.runtime.selected_providers}"
        )

    def request(self, flow: http.HTTPFlow) -> None:
        """Answer the supervisor's fixed health probe without contacting a remote host."""
        if (
            flow.request.pretty_host == "local-filter.invalid"
            and flow.request.path == "/.well-known/local-image-filter/health"
        ):
            flow.response = http.Response.make(
                204,
                b"",
                {
                    "cache-control": "no-store",
                    "x-local-image-filter-health": "ready",
                },
            )

    async def response(self, flow: http.HTTPFlow) -> None:
        if self.runtime is None or self.eligibility is None or self.semaphore is None:
            return
        response = flow.response
        if response is None:
            return
        content = response.content or b""
        result = self.eligibility.classify(
            host=flow.request.pretty_host,
            status_code=response.status_code,
            content_type=response.headers.get("content-type", ""),
            content_length=len(content),
            has_range="range" in flow.request.headers or "content-range" in response.headers,
        )
        if result.disposition == ResponseDisposition.IGNORE:
            return
        if result.disposition == ResponseDisposition.FAIL_CLOSED_IMAGE:
            body, mime_type = create_failure_placeholder(content, text="Image not inspected")
            apply_replacement(response, body, mime_type, reason=result.reason)
            return

        async with self.semaphore:
            try:
                outcome = await asyncio.to_thread(
                    self.runtime.pipeline.filter_bytes,
                    content,
                    result.mime_type,
                    None,
                )
            except Exception as error:
                ctx.log.error(f"Image filter pipeline error: {error.__class__.__name__}: {error}")
                fail_action = str(
                    self.runtime.settings.section("proxy").get("fail_action", "replace")
                )
                if fail_action == "allow":
                    response.headers["x-local-image-filter"] = "allow"
                    response.headers["x-local-image-filter-reason"] = "model-failure-fail-open"
                    return
                body, mime_type = create_failure_placeholder(content, text="Image analysis failed")
                apply_replacement(response, body, mime_type, reason="model-failure")
                return

        response.headers["x-local-image-filter"] = outcome.decision.action.value
        response.headers["x-local-image-filter-cache"] = "hit" if outcome.cache_hit else "miss"
        response.headers["x-local-image-filter-reason"] = "policy"

        if outcome.decision.action in {FilterAction.BLUR, FilterAction.REPLACE}:
            if outcome.replacement_bytes is None:
                body, mime_type = create_failure_placeholder(content, text="Image unavailable")
                apply_replacement(response, body, mime_type, reason="missing-rewrite")
                return
            response.content = outcome.replacement_bytes
            response.headers["content-type"] = outcome.replacement_mime_type or "image/png"
            response.headers["content-length"] = str(len(outcome.replacement_bytes))
            response.headers["cache-control"] = "no-store"
            for header in INVALIDATED_HEADERS:
                response.headers.pop(header, None)
        elif outcome.decision.action == FilterAction.ERROR:
            body, mime_type = create_failure_placeholder(content, text="Image analysis failed")
            apply_replacement(response, body, mime_type, reason="policy-error")


addons = [LocalImageFilterAddon()]
