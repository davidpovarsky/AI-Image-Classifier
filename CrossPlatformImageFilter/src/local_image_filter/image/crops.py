from __future__ import annotations

from dataclasses import dataclass

from PIL import Image

from ..domain.models import BoundingBox
from ..geometry import expanded_box


@dataclass(slots=True)
class CropResult:
    crop_id: str
    person_id: str
    image: Image.Image
    source_box: BoundingBox
    expanded_box: BoundingBox
    pixel_rect: tuple[int, int, int, int]


def crop_person(
    image: Image.Image,
    person_id: str,
    source_box: BoundingBox,
    horizontal_padding: float,
    vertical_padding: float,
) -> CropResult:
    box = expanded_box(source_box, horizontal_padding, vertical_padding)
    left = max(0, int(round(box.x * image.width)))
    top = max(0, int(round(box.y * image.height)))
    right = min(image.width, int(round((box.x + box.width) * image.width)))
    bottom = min(image.height, int(round((box.y + box.height) * image.height)))
    if right <= left or bottom <= top:
        raise ValueError(f"Invalid crop for {person_id}")
    return CropResult(
        crop_id=f"{person_id}-crop",
        person_id=person_id,
        image=image.crop((left, top, right, bottom)),
        source_box=source_box,
        expanded_box=box,
        pixel_rect=(left, top, right - left, bottom - top),
    )
