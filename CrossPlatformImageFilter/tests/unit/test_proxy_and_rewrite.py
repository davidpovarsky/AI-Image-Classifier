import io

from PIL import Image

from local_image_filter.image.rewrite import create_failure_placeholder
from local_image_filter.proxy.addon import INVALIDATED_HEADERS, apply_replacement
from local_image_filter.proxy.filters import ResponseDisposition, ResponseEligibility


class Response:
    def __init__(self) -> None:
        self.status_code = 206
        self.content = b"old"
        self.headers = {header: "old" for header in INVALIDATED_HEADERS}


def eligibility() -> ResponseEligibility:
    return ResponseEligibility(
        {
            "allowed_content_types": ["image/jpeg", "image/png", "image/webp"],
            "ignore_hosts": ["localhost", "*.internal"],
            "max_body_bytes": 100,
        }
    )


def classify(**overrides):
    arguments = {
        "host": "example.com",
        "status_code": 200,
        "content_type": "image/png",
        "content_length": 20,
        "has_range": False,
    }
    arguments.update(overrides)
    return eligibility().classify(**arguments)


def test_non_image_and_ignored_host_are_ignored() -> None:
    assert classify(content_type="application/json").disposition == ResponseDisposition.IGNORE
    assert classify(host="api.internal").reason == "ignoredHost"


def test_partial_oversized_and_unsupported_images_fail_closed() -> None:
    assert classify(has_range=True).reason == "partialImageContent"
    assert classify(content_length=101).reason == "imageTooLargeForInspection"
    assert classify(content_type="image/svg+xml").reason == "unsupportedImageFormat"
    assert classify(has_range=True).disposition == ResponseDisposition.FAIL_CLOSED_IMAGE


def test_supported_jpeg_is_inspected() -> None:
    assert (
        classify(content_type="image/jpeg; charset=binary").disposition
        == ResponseDisposition.INSPECT
    )


def test_failure_placeholder_is_valid_png_and_preserves_known_size() -> None:
    source = io.BytesIO()
    Image.new("RGB", (42, 24), "white").save(source, format="PNG")
    content, mime = create_failure_placeholder(source.getvalue())
    assert mime == "image/png"
    with Image.open(io.BytesIO(content)) as image:
        assert image.size == (42, 24)


def test_replacement_headers_are_consistent() -> None:
    response = Response()
    apply_replacement(response, b"png", "image/png", reason="unsupported-format")
    assert response.status_code == 200
    assert response.content == b"png"
    assert response.headers["content-length"] == "3"
    assert response.headers["cache-control"] == "no-store"
    assert response.headers["x-local-image-filter"] == "replace"
    assert all(header not in response.headers for header in INVALIDATED_HEADERS)
