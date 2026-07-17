from __future__ import annotations

import io
import math
from dataclasses import dataclass

from PIL import Image, ImageOps, UnidentifiedImageError


class ImageDecodeError(ValueError):
    def __init__(self, reason: str, message: str) -> None:
        super().__init__(message)
        self.reason = reason


def register_image_plugins() -> None:
    try:
        from pillow_heif import register_avif_opener, register_heif_opener

        register_heif_opener()
        register_avif_opener()
    except ImportError:
        return


register_image_plugins()


@dataclass(slots=True)
class DecodedImage:
    image: Image.Image
    original_width: int
    original_height: int
    processing_width: int
    processing_height: int
    scale_factor: float
    source_format: str
    animated: bool


def decode_image(
    content: bytes,
    maximum_dimension: int,
    maximum_total_pixels: int,
    animated_image_mode: str = "replace",
) -> DecodedImage:
    if not content:
        raise ImageDecodeError("emptyImage", "Image body is empty")
    try:
        with Image.open(io.BytesIO(content)) as source:
            animated = bool(
                getattr(source, "is_animated", False) and getattr(source, "n_frames", 1) > 1
            )
            if animated and animated_image_mode == "replace":
                raise ImageDecodeError(
                    "animatedImageNotInspected",
                    "Animated images are replaced unless frame sampling is enabled",
                )
            source_format = (source.format or "PNG").upper()
            normalized = ImageOps.exif_transpose(source).convert("RGB")
    except ImageDecodeError:
        raise
    except (UnidentifiedImageError, OSError, ValueError) as error:
        raise ImageDecodeError("imageDecodeFailed", f"Unable to decode image: {error}") from error

    original_width, original_height = normalized.size
    if original_width < 1 or original_height < 1:
        raise ImageDecodeError("invalidImageDimensions", "Image dimensions must be positive")
    dimension_scale = min(1.0, maximum_dimension / max(original_width, original_height))
    pixel_scale = min(1.0, math.sqrt(maximum_total_pixels / (original_width * original_height)))
    scale = min(dimension_scale, pixel_scale)
    if scale < 1.0:
        target = (
            max(1, round(original_width * scale)),
            max(1, round(original_height * scale)),
        )
        normalized = normalized.resize(target, Image.Resampling.LANCZOS)
    return DecodedImage(
        image=normalized,
        original_width=original_width,
        original_height=original_height,
        processing_width=normalized.width,
        processing_height=normalized.height,
        scale_factor=normalized.width / original_width,
        source_format=source_format,
        animated=animated,
    )
