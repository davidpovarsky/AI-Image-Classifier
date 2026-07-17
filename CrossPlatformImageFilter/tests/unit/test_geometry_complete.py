import math

import pytest

from local_image_filter.domain.models import BoundingBox, PersonDetection, RawNudityDetection
from local_image_filter.geometry import (
    assign_person_ids,
    expanded_box,
    intersection,
    iou,
    map_crop_box_to_original,
    merge_detections,
)


def raw(identifier: str, label: str, confidence: float, box: BoundingBox) -> RawNudityDetection:
    return RawNudityDetection(identifier, label, confidence, "fullImage", box, 0)


@pytest.mark.parametrize("value", [math.nan, math.inf, -math.inf])
def test_nonfinite_box_is_rejected(value: float) -> None:
    with pytest.raises(ValueError, match="finite"):
        BoundingBox(value, 0, 1, 1)


def test_negative_area_is_rejected() -> None:
    with pytest.raises(ValueError, match="must not be negative"):
        BoundingBox(0, 0, -1, 1)


def test_clamp_and_expansion_at_edge() -> None:
    result = expanded_box(BoundingBox(0, 0, 0.2, 0.4), 0.5, 0.25)
    assert (result.x, result.y, result.width, result.height) == pytest.approx((0, 0, 0.3, 0.5))
    assert result.area == pytest.approx(0.15)


def test_portrait_and_landscape_mapping() -> None:
    portrait = map_crop_box_to_original(
        BoundingBox(0.2, 0.1, 0.4, 0.8), BoundingBox(0, 0.5, 1, 0.5)
    )
    landscape = map_crop_box_to_original(
        BoundingBox(0.1, 0.2, 0.8, 0.4), BoundingBox(0.5, 0, 0.5, 1)
    )
    assert (portrait.x, portrait.y, portrait.width, portrait.height) == pytest.approx(
        (0.2, 0.5, 0.4, 0.4)
    )
    assert (landscape.x, landscape.y, landscape.width, landscape.height) == pytest.approx(
        (0.5, 0.2, 0.4, 0.4)
    )


def test_zero_area_iou_and_intersection() -> None:
    zero = BoundingBox(0.2, 0.2, 0, 0.5)
    normal = BoundingBox(0.1, 0.1, 0.5, 0.5)
    assert intersection(zero, normal) == 0
    assert iou(zero, normal) == 0


def test_center_and_overlap_assignment() -> None:
    people = [PersonDetection("p1", "yolox", 0.9, BoundingBox(0.2, 0.2, 0.4, 0.4))]
    assert assign_person_ids(BoundingBox(0.3, 0.3, 0.1, 0.1), people, 0.9) == ("p1",)
    assert assign_person_ids(BoundingBox(0.1, 0.1, 0.2, 0.2), people, 0.2) == ("p1",)


def test_merge_keeps_winner_provenance_and_different_labels() -> None:
    a = raw("a", "A", 0.5, BoundingBox(0.1, 0.1, 0.4, 0.4))
    b = raw("b", "A", 0.8, BoundingBox(0.12, 0.12, 0.4, 0.4))
    c = raw("c", "B", 0.9, BoundingBox(0.12, 0.12, 0.4, 0.4))
    merged = merge_detections([a, b, c], [], 0.5, 0.5)
    assert len(merged) == 2
    first = next(item for item in merged if item.raw_label == "A")
    assert first.confidence == 0.8
    assert set(first.source_detection_ids) == {"a", "b"}
    assert set(first.source_confidences) == {0.5, 0.8}


def test_transitive_overlap_is_documented_by_group_membership() -> None:
    detections = [
        raw("a", "A", 0.9, BoundingBox(0.0, 0, 0.4, 0.4)),
        raw("b", "A", 0.8, BoundingBox(0.2, 0, 0.4, 0.4)),
        raw("c", "A", 0.7, BoundingBox(0.4, 0, 0.4, 0.4)),
    ]
    merged = merge_detections(detections, [], 0.3, 0.5)
    assert len(merged) == 1
    assert merged[0].source_detection_ids == ("a", "b", "c")
