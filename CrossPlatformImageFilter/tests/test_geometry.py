import pytest

from local_image_filter.domain.models import BoundingBox, PersonDetection, RawNudityDetection
from local_image_filter.geometry import iou, map_crop_box_to_original, merge_detections


def test_iou_exact_match() -> None:
    box = BoundingBox(0.1, 0.2, 0.3, 0.4)
    assert iou(box, box) == 1.0


def test_crop_mapping() -> None:
    crop = BoundingBox(0.2, 0.1, 0.5, 0.8)
    inner = BoundingBox(0.5, 0.25, 0.2, 0.5)
    result = map_crop_box_to_original(crop, inner)
    assert result.x == pytest.approx(0.45)
    assert result.y == pytest.approx(0.30)
    assert result.width == pytest.approx(0.10)
    assert result.height == pytest.approx(0.40)


def test_merge_same_label_and_keep_sources() -> None:
    a = RawNudityDetection("a", "X", 0.6, "fullImage", BoundingBox(0.1, 0.1, 0.4, 0.4), 1)
    b = RawNudityDetection(
        "b", "X", 0.8, "personCrop", BoundingBox(0.11, 0.11, 0.4, 0.4), 1, "person-1"
    )
    people = [PersonDetection("person-1", "yoloPersonDetector", 0.9, BoundingBox(0, 0, 0.8, 0.9))]
    merged = merge_detections([a, b], people, 0.5, 0.5)
    assert len(merged) == 1
    assert merged[0].confidence == 0.8
    assert set(merged[0].sources) == {"fullImage", "personCrop"}
    assert merged[0].person_ids == ("person-1",)
