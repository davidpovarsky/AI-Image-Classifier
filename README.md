# AI Image Classifier for iOS

This SwiftUI app performs on-device nudity object detection with NudeNet 320n,
an Ultralytics YOLOv8n detector exported as a 320×320 Float16 Core ML ML
Program. It returns labeled detections, confidence scores, normalized bounding
boxes, and a configurable allow/block policy. Speech and camera demonstrations
from the upstream project remain available.

Inference is local. Images submitted through the UI or loopback HTTP server are
held in memory only: the app does not persist, upload, or log image bytes.

## Model provenance and conversion

- Source: [official NudeNet `320n.pt`](https://github.com/notAI-tech/NudeNet/releases/download/v3.4-weights/320n.pt)
- Weight release: `v3.4-weights`
- SHA-256: `1d25e219d536dcd6994651020d3c7cba642d13990e6eef934ed7a8ba650fb582`
- App artifact: `AI-Image-Classifier/Models/NudeNet320n.mlpackage`
- Conversion command: `python tools/model_conversion/convert_nudenet_320n.py`
- Verification command: `python tools/model_conversion/verify_model.py`

The reproducible macOS/Python 3.11 workflow is documented in
`tools/model_conversion/README.md`. Tested pins are Ultralytics 8.4.95,
coremltools 9.0, PyTorch 2.13.0, torchvision 0.28.0, Pillow 12.3.0, and NumPy
2.3.5. These are conversion-only tools and are not iOS runtime dependencies.

### Canonical labels

| ID | Label | ID | Label |
|---:|---|---:|---|
| 0 | FEMALE_GENITALIA_COVERED | 9 | FEET_COVERED |
| 1 | FACE_FEMALE | 10 | ARMPITS_COVERED |
| 2 | BUTTOCKS_EXPOSED | 11 | ARMPITS_EXPOSED |
| 3 | FEMALE_BREAST_EXPOSED | 12 | FACE_MALE |
| 4 | FEMALE_GENITALIA_EXPOSED | 13 | BELLY_EXPOSED |
| 5 | MALE_BREAST_EXPOSED | 14 | MALE_GENITALIA_EXPOSED |
| 6 | ANUS_EXPOSED | 15 | ANUS_COVERED |
| 7 | FEET_EXPOSED | 16 | FEMALE_BREAST_COVERED |
| 8 | BELLY_COVERED | 17 | BUTTOCKS_COVERED |

## Default policy

The standard profile blocks exposed female breast (0.45), female genitalia
(0.35), male genitalia (0.35), anus (0.35), and buttocks (0.50). Strict mode
can additionally block exposed male breast (0.75), belly (0.90), and armpits
(0.95). Faces, covered classes, and feet never block on their own. Policy lives
separately from inference in `NudityFilterPolicy.swift`, so thresholds can be
changed without retraining the model.

## Local HTTP API

The app exposes a bearer-token-protected server on `127.0.0.1:8765`. Send raw
encoded image bytes, never multipart or Base64:

```bash
curl -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: image/jpeg' \
  --data-binary @safe.jpg \
  http://127.0.0.1:8765/v1/classify
```

```json
{
  "success": true,
  "allowed": false,
  "risk": "nudity",
  "confidence": 0.93,
  "triggeredClass": "FEMALE_BREAST_EXPOSED",
  "durationMs": 18,
  "model": "NudeNet320n",
  "detections": [{
    "classId": 3,
    "label": "FEMALE_BREAST_EXPOSED",
    "confidence": 0.93,
    "box": {"x": 0.12, "y": 0.28, "width": 0.24, "height": 0.31}
  }],
  "predictions": [{"label": "FEMALE_BREAST_EXPOSED", "confidence": 0.93}]
}
```

`predictions` is deprecated compatibility output. See `LOCAL_SERVER.md` for the
complete contract, errors, lifecycle limits, and unsigned IPA instructions.

## Accuracy and validation limits

Detection and policy decisions can produce false positives and false negatives.
They must not be treated as age verification, consent determination, or a
substitute for human review. Real NSFW validation material is intentionally not
committed. Validate privately on a physical device using lawfully obtained test
material and do not record or redistribute it.

### iPad M3 benchmark

No physical iPad M3 was available during CI verification, so values are not
invented.

| Metric | Result |
|---|---|
| Cold model load | Pending physical iPad M3 measurement |
| Warm-up | Pending physical iPad M3 measurement |
| First inference | Pending physical iPad M3 measurement |
| Median warm inference | Pending physical iPad M3 measurement |
| p95 warm inference | Pending physical iPad M3 measurement |
| Peak memory | Pending physical iPad M3 measurement |

## Licensing

The NudeNet repository and `v3.4-weights` tag contain AGPL-3.0 license files,
but NudeNet's PyPI metadata and `setup.py` declare MIT; the release does not
state an unambiguous separate license for the weights. Ultralytics is AGPL-3.0
and is used only during conversion. coremltools and the other conversion tools
carry their respective BSD-style or bundled licenses. Resolve the NudeNet and
weights discrepancy with the copyright holder or legal counsel before
distribution. This inventory is not legal advice.

## Build

The project is `AI-Image-Classifier.xcodeproj`, scheme
`AI-Image-Classifier`, target iOS 26.2. GitHub Actions validates the committed
model, runs unit tests, performs a generic physical-device Release build with
signing disabled, confirms the compiled model is in the `.app`, and packages an
unsigned IPA. An unsigned IPA is not directly installable without a separate
signing process.
