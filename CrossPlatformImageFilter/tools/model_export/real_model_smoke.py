#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import tempfile
import time
import urllib.request
from pathlib import Path
from typing import Any

from PIL import Image, ImageDraw

from local_image_filter.config import load_settings
from local_image_filter.image.rewrite import rewrite_image
from local_image_filter.runtime.container import build_runtime

PERSON_FIXTURE_URL = "https://raw.githubusercontent.com/opencv/opencv/31b0eeea0b44b370fd0712312df4214d4ae1b158/samples/data/basketball1.png"
PERSON_FIXTURE_SHA256 = "ba06f6701f7260998b430c39b6557f775497e6ce7b1a74f0b7ea6af371bf54a6"


def encoded(image: Image.Image) -> bytes:
    stream = io.BytesIO()
    image.save(stream, format="PNG")
    return stream.getvalue()


def assert_finite(value: Any) -> None:
    if isinstance(value, float) and not math.isfinite(value):
        raise ValueError(f"Non-finite value in smoke response: {value}")
    if isinstance(value, dict):
        for item in value.values():
            assert_finite(item)
    elif isinstance(value, list):
        for item in value:
            assert_finite(item)


def measured(callable):
    started = time.perf_counter()
    value = callable()
    return value, round((time.perf_counter() - started) * 1000, 2)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="real-model-smoke-") as directory:
        temporary = Path(directory)
        settings = load_settings(args.config)
        settings.data["cache"]["sqlite_path"] = str(temporary / "cache.sqlite3")
        settings.data["diagnostics"]["directory"] = str(temporary / "diagnostics")
        runtime, load_ms = measured(lambda: build_runtime(settings))

        blank = encoded(Image.new("RGB", (640, 480), "white"))
        shapes_image = Image.new("RGB", (640, 480), "white")
        draw = ImageDraw.Draw(shapes_image)
        draw.rectangle((100, 80, 300, 400), fill="navy")
        draw.ellipse((360, 100, 520, 260), fill="orange")
        shapes = encoded(shapes_image)
        person_path = temporary / "basketball1.png"
        with urllib.request.urlopen(PERSON_FIXTURE_URL, timeout=120) as response:
            person_content = response.read()
        if hashlib.sha256(person_content).hexdigest() != PERSON_FIXTURE_SHA256:
            raise ValueError("Safe person fixture SHA-256 mismatch")
        person_path.write_bytes(person_content)

        first, first_ms = measured(lambda: runtime.pipeline.analyze(blank, "image/png"))
        warm, warm_ms = measured(lambda: runtime.pipeline.analyze(shapes, "image/png"))
        person, person_ms = measured(lambda: runtime.pipeline.analyze(person_content, "image/png"))
        cached, cache_ms = measured(lambda: runtime.pipeline.analyze(blank, "image/png"))
        decoded = Image.open(io.BytesIO(blank)).convert("RGB")
        _, rewrite_ms = measured(lambda: rewrite_image(decoded, "PNG", "replace", 32, "hidden", 88))
        responses = [first.response, warm.response, person.response, cached.response]
        for response in responses:
            assert_finite(response)
            json.dumps(response)
        if not cached.cache_hit:
            raise ValueError("Expected a cache hit during real-model smoke")
        report = {
            "passed": True,
            "fixture": {
                "source": PERSON_FIXTURE_URL,
                "sha256": PERSON_FIXTURE_SHA256,
                "licenseContext": "OpenCV 4.11.0 repository sample; Apache-2.0 repository license",
            },
            "providers": runtime.selected_providers,
            "models": runtime.model_descriptions,
            "timingsMs": {
                "modelLoadAndWarmup": load_ms,
                "firstInference": first_ms,
                "warmInference": warm_ms,
                "safePersonPipeline": person_ms,
                "cacheHit": cache_ms,
                "rewrite": rewrite_ms,
            },
            "summaries": [response["summary"] for response in responses],
            "personFixtureDetectedPeople": person.response["summary"]["personCount"],
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
