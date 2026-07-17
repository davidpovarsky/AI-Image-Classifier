from __future__ import annotations

import copy
import hashlib
import uuid
from dataclasses import replace
from datetime import UTC, datetime
from time import perf_counter
from typing import Any, Literal

from ..cache.store import AnalysisCache
from ..diagnostics.logger import DiagnosticsLogger
from ..domain.models import (
    FilterAction,
    FilterOutcome,
    ModuleReport,
    PipelineStatus,
    RawNudityDetection,
)
from ..geometry import map_crop_box_to_original, merge_detections
from ..image.crops import crop_person
from ..image.decode import decode_image
from ..image.rewrite import rewrite_image
from ..inference.base import MobileCLIPProtocol, NudeNetProtocol, PersonDetectorProtocol
from ..policy.engine import PolicyEngine


class ImageSafetyPipeline:
    pipeline_version = 1

    def __init__(
        self,
        *,
        settings: dict[str, Any],
        person_detector: PersonDetectorProtocol,
        mobileclip: MobileCLIPProtocol,
        nudenet: NudeNetProtocol,
        policy: PolicyEngine,
        cache: AnalysisCache,
        diagnostics: DiagnosticsLogger,
        runtime_info: dict[str, Any],
    ) -> None:
        self.settings = settings
        self.person_detector = person_detector
        self.mobileclip = mobileclip
        self.nudenet = nudenet
        self.policy = policy
        self.cache = cache
        self.diagnostics = diagnostics
        self.runtime_info = runtime_info

    @staticmethod
    def _error(error: Exception) -> dict[str, Any]:
        return {
            "domain": error.__class__.__module__,
            "code": 0,
            "message": str(error),
            "underlyingErrors": [],
        }

    def analyze(self, content: bytes, mime_type: str, url: str | None = None) -> FilterOutcome:
        content_hash = hashlib.sha256(content).hexdigest()
        cached = self.cache.get(content_hash)
        if cached is not None:
            self.diagnostics.append(
                {
                    "event": "imageAnalysisCacheHit",
                    "contentHash": content_hash,
                    "url": url,
                    "decision": cached.decision.to_dict(),
                }
            )
            return FilterOutcome(
                content_hash=content_hash,
                response=copy.deepcopy(cached.response),
                decision=cached.decision,
                cache_hit=True,
            )

        request_id = str(uuid.uuid4())
        started = perf_counter()
        processing = self.settings["processing"]
        reports: dict[str, ModuleReport] = {}
        errors: list[dict[str, Any]] = []
        warnings: list[dict[str, Any]] = []

        decode_started = perf_counter()
        decoded = decode_image(
            content,
            maximum_dimension=int(processing["maximum_dimension"]),
            maximum_total_pixels=int(processing["maximum_total_pixels"]),
            animated_image_mode=str(processing.get("animated_image_mode", "replace")),
        )
        reports["imageDecode"] = ModuleReport(
            duration_ms=round((perf_counter() - decode_started) * 1000),
            details={
                "input": {"byteCount": len(content), "mimeType": mime_type},
                "output": {
                    "pixelWidth": decoded.processing_width,
                    "pixelHeight": decoded.processing_height,
                    "orientationNormalized": True,
                    "sourceFormat": decoded.source_format,
                },
            },
        )

        full_raw: list[RawNudityDetection] = []
        full_started = perf_counter()
        try:
            full_raw = self.nudenet.detect(
                decoded.image,
                source="fullImage",
                id_prefix="nudity-full",
            )
            reports["nudeNetFullImage"] = ModuleReport(
                duration_ms=round((perf_counter() - full_started) * 1000),
                details={
                    "coordinateSystem": "normalized-top-left",
                    "detections": [item.to_dict() for item in full_raw],
                },
            )
        except Exception as error:
            item = self._error(error)
            errors.append(item)
            reports["nudeNetFullImage"] = ModuleReport(
                status=PipelineStatus.FAILED,
                duration_ms=round((perf_counter() - full_started) * 1000),
                error=item,
            )

        detection_started = perf_counter()
        people = []
        try:
            people = self.person_detector.detect(decoded.image)
            reports["personDetection"] = ModuleReport(
                duration_ms=round((perf_counter() - detection_started) * 1000),
                details={
                    "detector": "onnxPersonDetector",
                    "coordinateSystem": "normalized-top-left",
                    "people": [person.to_dict() for person in people],
                },
            )
        except Exception as error:
            item = self._error(error)
            errors.append(item)
            reports["personDetection"] = ModuleReport(
                status=PipelineStatus.FAILED,
                duration_ms=round((perf_counter() - detection_started) * 1000),
                error=item,
            )

        maximum_crops = int(processing["maximum_person_crops"])
        selected_people = people[:maximum_crops]
        if len(people) > maximum_crops:
            warning = {
                "code": "personCropLimitReached",
                "message": "Only the largest person detections were processed.",
                "detectedPersonCount": len(people),
                "processedPersonCount": maximum_crops,
            }
            warnings.append(warning)

        crop_started = perf_counter()
        crops = []
        crop_by_person: dict[str, Any] = {}
        for person in selected_people:
            try:
                crop = crop_person(
                    decoded.image,
                    person.person_id,
                    person.bounding_box,
                    float(processing["horizontal_crop_padding"]),
                    float(processing["vertical_crop_padding"]),
                )
                crops.append(crop)
                crop_by_person[person.person_id] = crop
            except ValueError as error:
                warnings.append({"code": "invalidPersonCrop", "message": str(error)})
        reports["personCrops"] = ModuleReport(
            duration_ms=round((perf_counter() - crop_started) * 1000),
            warnings=warnings.copy(),
            details={
                "crops": [
                    {
                        "personId": crop.person_id,
                        "cropId": crop.crop_id,
                        "sourceBoundingBox": crop.source_box.to_dict(),
                        "expandedBoundingBox": crop.expanded_box.to_dict(),
                        "pixelRect": {
                            "x": crop.pixel_rect[0],
                            "y": crop.pixel_rect[1],
                            "width": crop.pixel_rect[2],
                            "height": crop.pixel_rect[3],
                        },
                        "padding": {
                            "horizontal": float(processing["horizontal_crop_padding"]),
                            "vertical": float(processing["vertical_crop_padding"]),
                        },
                    }
                    for crop in crops
                ]
            },
        )

        classifications = []
        mobile_started = perf_counter()
        mobile_errors: list[dict[str, Any]] = []
        for crop in crops:
            try:
                classifications.append(
                    self.mobileclip.classify(crop.image, crop.person_id, crop.crop_id)
                )
            except Exception as error:
                mobile_error = self._error(error)
                mobile_errors.append(mobile_error)
                errors.append(mobile_error)
                continue
        if not crops:
            mobile_status = PipelineStatus.SKIPPED
        elif mobile_errors and classifications:
            mobile_status = PipelineStatus.PARTIAL_SUCCESS
        elif mobile_errors:
            mobile_status = PipelineStatus.FAILED
        else:
            mobile_status = PipelineStatus.SUCCESS
        reports["mobileCLIP2"] = ModuleReport(
            status=mobile_status,
            duration_ms=round((perf_counter() - mobile_started) * 1000),
            error=mobile_errors[-1] if mobile_errors else None,
            warnings=[{"code": "cropInferenceFailed", "error": item} for item in mobile_errors],
            details={"classifications": [item.to_dict() for item in classifications]},
        )

        crop_raw: list[RawNudityDetection] = []
        crop_nude_started = perf_counter()
        crop_nude_errors: list[dict[str, Any]] = []
        for crop in crops:
            try:
                local_detections = self.nudenet.detect(
                    crop.image,
                    source="personCrop",
                    id_prefix=f"nudity-{crop.person_id}",
                    person_id=crop.person_id,
                    crop_id=crop.crop_id,
                )
                for local in local_detections:
                    original = map_crop_box_to_original(crop.expanded_box, local.bounding_box)
                    crop_raw.append(replace(local, bounding_box=original))
            except Exception as error:
                crop_nude_error = self._error(error)
                crop_nude_errors.append(crop_nude_error)
                errors.append(crop_nude_error)
                continue
        if not crops:
            crop_nude_status = PipelineStatus.SKIPPED
        elif crop_nude_errors and crop_raw:
            crop_nude_status = PipelineStatus.PARTIAL_SUCCESS
        elif crop_nude_errors:
            crop_nude_status = PipelineStatus.FAILED
        else:
            crop_nude_status = PipelineStatus.SUCCESS
        reports["nudeNetPersonCrops"] = ModuleReport(
            status=crop_nude_status,
            duration_ms=round((perf_counter() - crop_nude_started) * 1000),
            error=crop_nude_errors[-1] if crop_nude_errors else None,
            warnings=[{"code": "cropInferenceFailed", "error": item} for item in crop_nude_errors],
            details={
                "coordinateSystem": "normalized-top-left",
                "detections": [item.to_dict() for item in crop_raw],
            },
        )

        all_raw = full_raw + crop_raw
        reports["coordinateMapping"] = ModuleReport(
            duration_ms=0,
            details={
                "coordinateSystem": "normalized-top-left",
                "mappedDetectionCount": len(all_raw),
            },
        )

        merge_started = perf_counter()
        merge_config = self.settings["merge"]
        merged = merge_detections(
            all_raw,
            people,
            iou_threshold=float(merge_config["iou_threshold"]),
            assignment_overlap_threshold=float(merge_config["assignment_overlap_threshold"]),
        )
        reports["detectionMerge"] = ModuleReport(
            duration_ms=round((perf_counter() - merge_started) * 1000),
            details={
                "rawDetectionCount": len(all_raw),
                "mergedDetectionCount": len(merged),
            },
        )

        people_response = []
        classification_by_person = {item.person_id: item for item in classifications}
        for person in people:
            person_crop = crop_by_person.get(person.person_id)
            classification = classification_by_person.get(person.person_id)
            people_response.append(
                {
                    "personId": person.person_id,
                    "detection": person.to_dict(),
                    "crop": (
                        {
                            "cropId": person_crop.crop_id,
                            "expandedBoundingBox": person_crop.expanded_box.to_dict(),
                        }
                        if person_crop
                        else None
                    ),
                    "mobileCLIP2": classification.to_dict() if classification else None,
                    "nudeNetCropDetections": [
                        item.to_dict() for item in crop_raw if item.person_id == person.person_id
                    ],
                    "assignedMergedDetections": [
                        item.to_dict() for item in merged if person.person_id in item.person_ids
                    ],
                }
            )

        module_statuses = {name: report.status.value for name, report in reports.items()}
        partial = bool(errors)
        total_duration = round((perf_counter() - started) * 1000)
        response: dict[str, Any] = {
            "success": True,
            "requestId": request_id,
            "evidenceSchemaVersion": 1,
            "pipelineVersion": self.pipeline_version,
            "timestamp": datetime.now(UTC).isoformat(),
            "input": {
                "mimeType": mime_type,
                "byteCount": len(content),
                "pixelWidth": decoded.processing_width,
                "pixelHeight": decoded.processing_height,
                "orientationNormalized": True,
                "originalPixelWidth": decoded.original_width,
                "originalPixelHeight": decoded.original_height,
                "processingPixelWidth": decoded.processing_width,
                "processingPixelHeight": decoded.processing_height,
                "scaleFactor": decoded.scale_factor,
            },
            "pipeline": {
                "status": "partialSuccess" if partial else "success",
                "totalDurationMs": total_duration,
                "partialFailure": partial,
                "modules": {name: report.to_dict() for name, report in reports.items()},
            },
            "people": people_response,
            "nudity": {
                "fullImageRawDetections": [item.to_dict() for item in full_raw],
                "personCropRawDetections": [item.to_dict() for item in crop_raw],
                "allRawDetections": [item.to_dict() for item in all_raw],
                "mergedDetections": [item.to_dict() for item in merged],
                "unassignedDetections": [item.to_dict() for item in merged if not item.person_ids],
            },
            "summary": {
                "personCount": len(people),
                "mobileCLIPClassificationCount": len(classifications),
                "fullImageNudityDetectionCount": len(full_raw),
                "personCropNudityDetectionCount": len(crop_raw),
                "mergedNudityDetectionCount": len(merged),
                "unassignedNudityDetectionCount": sum(1 for item in merged if not item.person_ids),
                "hasPersonDetections": bool(people),
                "hasNudityDetections": bool(merged),
                "highestWomanScore": max(
                    (item.scores.get("woman", 0.0) for item in classifications),
                    default=None,
                ),
                "highestNudityConfidence": max((item.confidence for item in merged), default=None),
                "moduleStatuses": module_statuses,
            },
            "warnings": warnings,
            "errors": errors,
            "runtime": copy.deepcopy(self.runtime_info),
        }
        policy_started = perf_counter()
        decision = self.policy.evaluate(response)
        reports["policy"] = ModuleReport(
            duration_ms=round((perf_counter() - policy_started) * 1000),
            details={"decision": decision.to_dict()},
        )
        reports["imageRewrite"] = ModuleReport(status=PipelineStatus.SKIPPED)
        response["pipeline"]["modules"] = {
            name: report.to_dict() for name, report in reports.items()
        }
        response["summary"]["moduleStatuses"] = {
            name: report.status.value for name, report in reports.items()
        }
        self.cache.put(content_hash, response, decision)
        self.diagnostics.append(
            {
                "event": "imageAnalysisCompleted",
                "requestId": request_id,
                "contentHash": content_hash,
                "url": url,
                "mimeType": mime_type,
                "inputBytes": len(content),
                "dimensions": {
                    "width": decoded.processing_width,
                    "height": decoded.processing_height,
                },
                "totalDurationMs": total_duration,
                "cacheHit": False,
                "decision": decision.to_dict(),
                "summary": response["summary"],
                "modules": {
                    name: {
                        "status": report.status.value,
                        "durationMs": report.duration_ms,
                        "error": report.error,
                    }
                    for name, report in reports.items()
                },
                "providerSelection": self.runtime_info.get("selectedProviders", {}),
            }
        )
        return FilterOutcome(
            content_hash=content_hash,
            response=response,
            decision=decision,
            cache_hit=False,
        )

    def filter_bytes(self, content: bytes, mime_type: str, url: str | None = None) -> FilterOutcome:
        outcome = self.analyze(content, mime_type, url)
        rewrite_action: Literal["blur", "replace"]
        if outcome.decision.action == FilterAction.BLUR:
            rewrite_action = "blur"
        elif outcome.decision.action == FilterAction.REPLACE:
            rewrite_action = "replace"
        else:
            return outcome
        processing = self.settings["processing"]
        decoded = decode_image(
            content,
            maximum_dimension=int(processing["maximum_dimension"]),
            maximum_total_pixels=int(processing["maximum_total_pixels"]),
            animated_image_mode=str(processing.get("animated_image_mode", "replace")),
        )
        replacement, replacement_type = rewrite_image(
            decoded.image,
            decoded.source_format,
            action=rewrite_action,
            blur_radius=float(processing["blur_radius"]),
            placeholder_text=str(processing["placeholder_text"]),
            jpeg_quality=int(processing["jpeg_quality"]),
        )
        outcome.replacement_bytes = replacement
        outcome.replacement_mime_type = replacement_type
        outcome.response["pipeline"]["modules"]["imageRewrite"] = ModuleReport(
            status=PipelineStatus.SUCCESS,
            details={"outputMimeType": replacement_type, "outputBytes": len(replacement)},
        ).to_dict()
        outcome.response["summary"]["moduleStatuses"]["imageRewrite"] = "success"
        return outcome
