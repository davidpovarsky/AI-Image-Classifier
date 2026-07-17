import pytest

from local_image_filter.domain.models import FilterAction
from local_image_filter.policy.engine import PolicyEngine


def engine(fail_action: str = "replace") -> PolicyEngine:
    return PolicyEngine(
        {
            "block_action": "blur",
            "woman_min_score": 0.42,
            "woman_min_margin_over_man": 0.10,
            "uncertain_min_score": 0.90,
            "block_woman_without_margin_if_score_at_least": 0.70,
            "nudity_thresholds": {
                "FEMALE_BREAST_EXPOSED": 0.3,
                "FACE_FEMALE": 1.01,
            },
        },
        fail_action,
    )


def response(
    *,
    woman: float = 0,
    man: float = 0,
    uncertain: float = 0,
    nude: float = 0,
    module_status: str = "success",
) -> dict:
    return {
        "people": [
            {
                "personId": "person-1",
                "detection": {"source": "yoloPersonDetector"},
                "mobileCLIP2": {"scores": {"woman": woman, "man": man, "uncertain": uncertain}},
            }
        ],
        "nudity": {
            "mergedDetections": [
                {
                    "mergedDetectionId": "n1",
                    "rawLabel": "FEMALE_BREAST_EXPOSED",
                    "confidence": nude,
                }
            ]
        },
        "summary": {
            "moduleStatuses": {
                "imageDecode": "success",
                "nudeNetFullImage": module_status,
                "personDetection": "success",
                "mobileCLIP2": "success",
            }
        },
    }


@pytest.mark.parametrize(
    "kwargs",
    [
        {"woman": 0.55, "man": 0.2},
        {"woman": 0.75, "man": 0.74},
        {"uncertain": 0.95},
        {"nude": 0.3},
    ],
)
def test_block_rules(kwargs: dict) -> None:
    assert engine().evaluate(response(**kwargs)).action == FilterAction.BLUR


def test_threshold_1_01_disables_label() -> None:
    value = response()
    value["nudity"]["mergedDetections"][0].update({"rawLabel": "FACE_FEMALE", "confidence": 1.0})
    assert engine().evaluate(value).action == FilterAction.ALLOW


def test_partial_failure_uses_fail_action_without_erasing_evidence() -> None:
    decision = engine().evaluate(response(module_status="failed"))
    assert decision.action == FilterAction.REPLACE
    assert decision.model_evidence_complete is False
    blocked = engine().evaluate(response(nude=0.8, module_status="failed"))
    assert blocked.action == FilterAction.BLUR
    assert blocked.model_evidence_complete is False


def test_no_evidence_allows() -> None:
    value = response()
    value["people"] = []
    value["nudity"]["mergedDetections"] = []
    assert engine().evaluate(value).action == FilterAction.ALLOW
