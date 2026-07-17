from __future__ import annotations

from typing import Any

from ..domain.models import FilterAction, FilterDecision


class PolicyEngine:
    def __init__(self, policy: dict[str, Any], fail_action: str) -> None:
        self.policy = policy
        self.fail_action = FilterAction(fail_action)
        self.block_action = FilterAction(str(policy.get("block_action", "blur")))
        self.nudity_thresholds = {
            str(label): float(value)
            for label, value in dict(policy.get("nudity_thresholds", {})).items()
        }

    def evaluate(self, response: dict[str, Any]) -> FilterDecision:
        reasons: list[str] = []
        evidence_ids: list[str] = []

        for person in response.get("people", []):
            classification = person.get("mobileCLIP2")
            if not isinstance(classification, dict):
                continue
            scores = classification.get("scores", {})
            woman = float(scores.get("woman", 0.0))
            man = float(scores.get("man", 0.0))
            uncertain = float(scores.get("uncertain", 0.0))
            source = str(person.get("detection", {}).get("source", "yoloPersonDetector"))
            threshold = float(self.policy.get("woman_min_score", 0.42))
            if source == "faceFallback":
                threshold = float(self.policy.get("face_fallback_woman_min_score", 0.48))
            elif source == "wholeImageFallback":
                threshold = float(self.policy.get("whole_image_fallback_woman_min_score", 0.58))
            margin = woman - man
            very_high = float(self.policy.get("block_woman_without_margin_if_score_at_least", 0.70))
            person_id = str(person.get("personId", "unknown"))
            if woman >= very_high or (
                woman >= threshold
                and margin >= float(self.policy.get("woman_min_margin_over_man", 0.10))
            ):
                reasons.append(
                    f"{person_id}: woman={woman:.4f}, man={man:.4f}, margin={margin:.4f}"
                )
                evidence_ids.append(person_id)
            elif uncertain >= float(self.policy.get("uncertain_min_score", 0.90)):
                reasons.append(f"{person_id}: uncertain={uncertain:.4f}")
                evidence_ids.append(person_id)

        for detection in response.get("nudity", {}).get("mergedDetections", []):
            label = str(detection.get("rawLabel", ""))
            confidence = float(detection.get("confidence", 0.0))
            nudity_threshold = self.nudity_thresholds.get(label)
            if nudity_threshold is not None and confidence >= nudity_threshold:
                reasons.append(f"{label}={confidence:.4f} (threshold={nudity_threshold:.4f})")
                evidence_ids.append(str(detection.get("mergedDetectionId", label)))

        statuses = response.get("summary", {}).get("moduleStatuses", {})
        required = ("imageDecode", "nudeNetFullImage", "personDetection", "mobileCLIP2")
        incomplete = any(statuses.get(name) not in {"success", "skipped"} for name in required)

        if reasons:
            return FilterDecision(
                action=self.block_action,
                reasons=tuple(reasons),
                evidence_ids=tuple(evidence_ids),
                model_evidence_complete=not incomplete,
            )
        if incomplete:
            return FilterDecision(
                action=self.fail_action,
                reasons=("One or more required model modules failed",),
                model_evidence_complete=False,
            )
        return FilterDecision(
            action=FilterAction.ALLOW,
            reasons=("No configured blocking rule matched",),
            model_evidence_complete=True,
        )
