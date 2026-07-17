from __future__ import annotations

from .domain.models import BoundingBox, MergedNudityDetection, PersonDetection, RawNudityDetection


def intersection(a: BoundingBox, b: BoundingBox) -> float:
    left = max(a.x, b.x)
    top = max(a.y, b.y)
    right = min(a.x + a.width, b.x + b.width)
    bottom = min(a.y + a.height, b.y + b.height)
    return max(0.0, right - left) * max(0.0, bottom - top)


def iou(a: BoundingBox, b: BoundingBox) -> float:
    overlap = intersection(a, b)
    union = a.area + b.area - overlap
    return min(1.0, max(0.0, overlap / union)) if union > 0 else 0.0


def expanded_box(box: BoundingBox, horizontal: float, vertical: float) -> BoundingBox:
    dx = box.width * horizontal
    dy = box.height * vertical
    return BoundingBox(
        x=box.x - dx,
        y=box.y - dy,
        width=box.width + 2 * dx,
        height=box.height + 2 * dy,
    ).clamped()


def map_crop_box_to_original(crop: BoundingBox, inner: BoundingBox) -> BoundingBox:
    return BoundingBox(
        x=crop.x + inner.x * crop.width,
        y=crop.y + inner.y * crop.height,
        width=inner.width * crop.width,
        height=inner.height * crop.height,
    ).clamped()


def assign_person_ids(
    detection_box: BoundingBox,
    people: list[PersonDetection],
    overlap_threshold: float,
) -> tuple[str, ...]:
    center_x, center_y = detection_box.center
    assigned: set[str] = set()
    for person in people:
        box = person.bounding_box
        center_inside = (
            box.x <= center_x <= box.x + box.width and box.y <= center_y <= box.y + box.height
        )
        overlap_ratio = intersection(detection_box, box) / max(detection_box.area, 1e-12)
        if center_inside or overlap_ratio >= overlap_threshold:
            assigned.add(person.person_id)
    return tuple(sorted(assigned))


def merge_detections(
    detections: list[RawNudityDetection],
    people: list[PersonDetection],
    iou_threshold: float,
    assignment_overlap_threshold: float,
) -> list[MergedNudityDetection]:
    groups: list[list[RawNudityDetection]] = []
    for detection in sorted(detections, key=lambda item: item.confidence, reverse=True):
        target: list[RawNudityDetection] | None = None
        for group in groups:
            if group[0].raw_label != detection.raw_label:
                continue
            if (
                max(iou(existing.bounding_box, detection.bounding_box) for existing in group)
                >= iou_threshold
            ):
                target = group
                break
        if target is None:
            groups.append([detection])
        else:
            target.append(detection)

    merged: list[MergedNudityDetection] = []
    for index, group in enumerate(groups, start=1):
        winner = max(group, key=lambda item: item.confidence)
        maximum_iou = (
            1.0
            if len(group) == 1
            else max(
                iou(a.bounding_box, b.bounding_box)
                for position, a in enumerate(group)
                for b in group[position + 1 :]
            )
        )
        person_ids = set(
            assign_person_ids(winner.bounding_box, people, assignment_overlap_threshold)
        )
        person_ids.update(item.person_id for item in group if item.person_id)
        merged.append(
            MergedNudityDetection(
                merged_detection_id=f"merged-nudity-{index}",
                raw_label=winner.raw_label,
                confidence=winner.confidence,
                bounding_box=winner.bounding_box,
                sources=tuple(sorted({item.source for item in group})),
                source_detection_ids=tuple(item.detection_id for item in group),
                source_confidences=tuple(item.confidence for item in group),
                person_ids=tuple(sorted(person_ids)),
                maximum_iou=maximum_iou,
                merge_threshold=iou_threshold,
            )
        )
    return merged
