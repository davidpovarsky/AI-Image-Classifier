from __future__ import annotations

import io
from typing import Literal

from PIL import Image, ImageDraw, ImageFilter, ImageFont


def _output_format(source_format: str) -> tuple[str, str]:
    value = source_format.upper()
    if value in {"JPEG", "JPG"}:
        return "JPEG", "image/jpeg"
    if value == "WEBP":
        return "WEBP", "image/webp"
    if value == "AVIF":
        return "AVIF", "image/avif"
    return "PNG", "image/png"


def _draw_placeholder(size: tuple[int, int], text: str) -> Image.Image:
    width = max(1, min(size[0], 2048))
    height = max(1, min(size[1], 2048))
    output = Image.new("RGB", (width, height), (32, 32, 32))
    draw = ImageDraw.Draw(output)
    try:
        font = ImageFont.load_default(size=max(12, min(width, height) // 18))
    except TypeError:
        font = ImageFont.load_default()
    box = draw.multiline_textbbox((0, 0), text, font=font, align="center")
    text_width = box[2] - box[0]
    text_height = box[3] - box[1]
    draw.multiline_text(
        ((width - text_width) / 2, (height - text_height) / 2),
        text,
        fill=(235, 235, 235),
        font=font,
        align="center",
    )
    return output


def create_failure_placeholder(
    content: bytes | None = None,
    *,
    text: str = "Image unavailable",
    fallback_size: tuple[int, int] = (320, 180),
) -> tuple[bytes, str]:
    size = fallback_size
    if content:
        try:
            with Image.open(io.BytesIO(content)) as source:
                if source.width > 0 and source.height > 0:
                    size = (source.width, source.height)
        except (OSError, ValueError):
            pass
    output = _draw_placeholder(size, text)
    buffer = io.BytesIO()
    output.save(buffer, format="PNG", optimize=True)
    return buffer.getvalue(), "image/png"


def rewrite_image(
    image: Image.Image,
    source_format: str,
    action: Literal["blur", "replace"],
    blur_radius: float,
    placeholder_text: str,
    jpeg_quality: int,
) -> tuple[bytes, str]:
    if action == "blur":
        output = image.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    elif action == "replace":
        output = _draw_placeholder(image.size, placeholder_text)
    else:
        raise ValueError(f"Unsupported rewrite action: {action}")

    format_name, mime_type = _output_format(source_format)
    if format_name == "JPEG":
        output = output.convert("RGB")
    buffer = io.BytesIO()
    save_options: dict[str, object] = {"optimize": True}
    if format_name in {"JPEG", "WEBP", "AVIF"}:
        save_options["quality"] = jpeg_quality
    try:
        output.save(buffer, format=format_name, **save_options)
    except (KeyError, OSError, ValueError):
        buffer = io.BytesIO()
        output.save(buffer, format="PNG", optimize=True)
        mime_type = "image/png"
    return buffer.getvalue(), mime_type
