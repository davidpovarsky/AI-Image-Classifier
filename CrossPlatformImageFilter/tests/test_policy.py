from local_image_filter.domain.models import FilterAction
from local_image_filter.policy.engine import PolicyEngine


def response(woman: float = 0, man: float = 0, nude: float = 0) -> dict:
    return {
        "people": [
            {
                "personId": "person-1",
                "detection": {"source": "yoloPersonDetector"},
                "mobileCLIP2": {"scores": {"woman": woman, "man": man, "uncertain": 0}},
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
                "nudeNetFullImage": "success",
                "personDetection": "success",
                "mobileCLIP2": "success",
            }
        },
    }


def engine() -> PolicyEngine:
    return PolicyEngine(
        {
            "block_action": "blur",
            "woman_min_score": 0.42,
            "woman_min_margin_over_man": 0.10,
            "uncertain_min_score": 0.90,
            "block_woman_without_margin_if_score_at_least": 0.70,
            "nudity_thresholds": {"FEMALE_BREAST_EXPOSED": 0.3},
        },
        "blur",
    )


def test_blocks_woman_margin() -> None:
    assert engine().evaluate(response(woman=0.7, man=0.2)).action == FilterAction.BLUR


def test_blocks_nudity() -> None:
    assert engine().evaluate(response(nude=0.5)).action == FilterAction.BLUR


def test_allows_no_match() -> None:
    assert engine().evaluate(response(woman=0.1, man=0.8)).action == FilterAction.ALLOW
