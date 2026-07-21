import json
from pathlib import Path

import pytest

from product.scripts.check_model_distribution_approval import verify


def test_example_approval_fails_closed() -> None:
    root = Path(__file__).resolve().parents[3]
    approval = json.loads(
        (root / "product/legal/model-distribution-approval.example.json").read_text()
    )
    with pytest.raises(ValueError, match="example"):
        verify(approval, {"models": {}}, "key", bytes(32))


def test_unapproved_model_cannot_pass_even_with_matching_shape() -> None:
    approval = {
        "approvalId": "review-1",
        "models": [
            {"modelIdentifier": name, "commercialRedistributionApproved": False}
            for name in ("mobileCLIP2", "nudeNet", "personDetector")
        ],
        "signature": {"keyId": "key", "algorithm": "Ed25519", "signature": ""},
    }
    with pytest.raises(ValueError, match="not approved"):
        verify(approval, {"models": {}}, "key", bytes(32))
