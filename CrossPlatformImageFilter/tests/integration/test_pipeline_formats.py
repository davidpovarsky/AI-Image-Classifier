import io
from pathlib import Path

import pytest
from PIL import Image

from local_image_filter.config import load_settings
from local_image_filter.image.decode import ImageDecodeError, decode_image
from local_image_filter.runtime.container import build_runtime


def encoded(format_name: str, size: tuple[int, int] = (128, 96)) -> bytes:
    stream = io.BytesIO()
    Image.new("RGB", size, "white").save(stream, format=format_name)
    return stream.getvalue()


def runtime(tmp_path: Path):
    root = Path(__file__).resolve().parents[2]
    settings = load_settings(root / "config" / "mock.toml")
    settings.data["cache"]["enabled"] = True
    settings.data["cache"]["sqlite_path"] = str(tmp_path / "cache.sqlite3")
    settings.data["diagnostics"]["enabled"] = True
    settings.data["diagnostics"]["directory"] = str(tmp_path / "diagnostics")
    return build_runtime(settings)


@pytest.mark.parametrize(
    ("format_name", "mime_type"),
    [("PNG", "image/png"), ("JPEG", "image/jpeg"), ("WEBP", "image/webp")],
)
def test_mock_pipeline_supported_formats(tmp_path: Path, format_name: str, mime_type: str) -> None:
    outcome = runtime(tmp_path).pipeline.filter_bytes(encoded(format_name), mime_type)
    assert outcome.decision.action.value == "allow"
    assert outcome.response["pipeline"]["status"] == "success"
    assert outcome.replacement_bytes is None
    assert outcome.response["runtime"]["transport"] == "mitmproxy"
    assert outcome.response["evidenceSchemaVersion"] == 1


def test_small_image_and_downscale(tmp_path: Path) -> None:
    small = runtime(tmp_path).pipeline.analyze(encoded("PNG", (10, 10)), "image/png")
    assert small.response["input"]["processingPixelWidth"] == 10
    large = decode_image(
        encoded("PNG", (3000, 1000)), maximum_dimension=2048, maximum_total_pixels=12_000_000
    )
    assert max(large.processing_width, large.processing_height) == 2048


def test_exif_orientation_is_normalized() -> None:
    stream = io.BytesIO()
    image = Image.new("RGB", (80, 40), "white")
    exif = image.getexif()
    exif[274] = 6
    image.save(stream, format="JPEG", exif=exif)
    decoded = decode_image(stream.getvalue(), 2048, 12_000_000)
    assert decoded.image.size == (40, 80)


def test_corrupted_and_animated_images_fail_for_proxy_replacement() -> None:
    with pytest.raises(ImageDecodeError, match="Unable to decode"):
        decode_image(b"not-an-image", 2048, 12_000_000)
    frames = [Image.new("RGB", (16, 16), color) for color in ("red", "blue")]
    stream = io.BytesIO()
    frames[0].save(stream, format="GIF", save_all=True, append_images=frames[1:])
    with pytest.raises(ImageDecodeError, match="Animated"):
        decode_image(stream.getvalue(), 2048, 12_000_000, animated_image_mode="replace")


def test_cache_hit_keeps_serializable_response(tmp_path: Path) -> None:
    instance = runtime(tmp_path)
    content = encoded("PNG")
    first = instance.pipeline.analyze(content, "image/png")
    second = instance.pipeline.analyze(content, "image/png")
    assert first.cache_hit is False
    assert second.cache_hit is True
    import json

    json.dumps(second.response)
