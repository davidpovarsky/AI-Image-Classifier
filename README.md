# AI Image Classifier for iOS

This SwiftUI app runs a local, bearer-token-protected image-evidence server.
The original MobileCLIP2-S2 person-classification endpoint remains available,
and a modular image-safety endpoint combines Vision, MobileCLIP2-S2, and the
bundled NudeNet320n detector without applying a blocking policy.

This system performs visual zero-shot classification.
It does not determine biological sex or gender identity.
Its labels describe the visual category selected by the model prompts.

The inference server returns raw model scores.
Blocking policy is intentionally left to the client.

## Architecture

1. `VNDetectHumanRectanglesRequest` detects full-body rectangles. If it finds
   none, an optional `VNDetectFaceRectanglesRequest` fallback supplies clearly
   marked `faceFallback` regions.
2. `PersonCropService` converts Vision's lower-left coordinates once, applies
   12% padding, clamps to the image, and crops the oriented `CGImage`.
3. A `MobileCLIPService` actor retains both Core ML encoders and serialized
   inference. Category embeddings are produced from multiple prompts, cached
   as a generated build asset, and validated against the prompt configuration.
4. NudeNet runs once on the normalized full image and sequentially on each
   selected person crop. Crop detections are mapped back to the original image.
5. Same-label NudeNet detections with IoU >= 0.50 are merged while retaining
   every raw detection, confidence, source ID, and person association.
6. The local HTTP layer returns every detected person, Vision confidence,
   normalized top-left bounding box, predicted visual class, and all four
   softmax scores. It never returns `allowed`, `blocked`, or risk policy.

```text
Input Image
   |-- Vision Person Detection
   |      `-- Person Crops
   |             |-- MobileCLIP2-S2
   |             `-- NudeNet Crop Detection
   |-- NudeNet Full Image Detection
   |-- Coordinate Mapping
   `-- Duplicate Merge
```

The server returns model evidence and module diagnostics. Blocking policy is
intentionally implemented by the client.

## Model provenance and conversion

- Official implementation: [apple/ml-mobileclip](https://github.com/apple/ml-mobileclip)
- Official checkpoint: [apple/MobileCLIP2-S2](https://huggingface.co/apple/MobileCLIP2-S2)
- Human detection: [VNDetectHumanRectanglesRequest](https://developer.apple.com/documentation/vision/vndetecthumanrectanglesrequest)
- Model terms: [Apple ML Research Model License](https://github.com/apple/ml-mobileclip/blob/main/LICENSE_MODELS)

Apple's official Core ML download currently contains first-generation
MobileCLIP models, not MobileCLIP2-S2. This repository therefore downloads the
official `mobileclip2_s2.pt` checkpoint during CI, loads Apple's architecture,
calls `eval()` and `reparameterize_model()`, and converts separate image and
image encoder as Float16 without quantization and precomputes prompt embeddings;
the text encoder and PyTorch checkpoint are not bundled or committed.

```bash
python -m pip install -r tools/MobileCLIPConversion/requirements.txt
tools/MobileCLIPConversion/download_model.sh .model-cache/mobileclip2-s2
python tools/MobileCLIPConversion/convert_mobileclip2_s2.py \
  --checkpoint .model-cache/mobileclip2-s2/mobileclip2_s2.pt
python tools/MobileCLIPConversion/verify_conversion.py \
  --checkpoint .model-cache/mobileclip2-s2/mobileclip2_s2.pt
```

The complete macOS setup and reproducibility notes are in
`tools/MobileCLIPConversion/README.md`. Conversion verification compares at
least ten deterministic image samples plus every configured text prompt,
checks embedding cosine similarity and class ranking, and fails CI on drift.

## Local HTTP API

The app listens on `127.0.0.1:8765`. The first use creates a stable UUID bearer
token in `UserDefaults`; copy it from the Local Server screen. Send raw JPEG,
PNG, HEIC, or HEIF bytes (never multipart or Base64):

```bash
curl -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: image/jpeg' \
  --data-binary @photo.jpg \
  http://127.0.0.1:8765/v1/person-classify
```

See `LOCAL_SERVER.md` for complete request, response, readiness, and error
contracts, and `docs/image-safety-api.md` for the combined pipeline.

## Privacy and limitations

All image inference is on-device. Image bytes and crops stay in memory and are
not persisted, logged, uploaded, sent to Hugging Face, Apple, OpenAI, external
telemetry, or any other server. Models are downloaded only by the build job and
packaged into the app.

MobileCLIP2-S2 was not trained as a dedicated woman/man classifier. False
positives and negatives are expected. Illustrations, children, small or partial
people, side views, obscured faces, and dense groups can be especially hard.
`uncertain` is a valid result, scores are not absolute truth, and prompt wording
materially affects zero-shot output. Human-rectangle detection can also miss a
person. Do not use this system to infer gender identity or biological sex.

## Build

The project is `AI-Image-Classifier.xcodeproj`, scheme
`AI-Image-Classifier`, targets iOS/iPadOS 26.5, and uses the latest stable Xcode
selected by GitHub Actions. The workflow downloads and converts the model,
verifies Core ML against PyTorch, runs unit tests, builds for a generic iOS
device without signing, validates the bundled model assets, and uploads
`AI-Image-Classifier-MobileCLIP2-S2-unsigned` containing the unsigned IPA,
model manifest, conversion report, and build diagnostics. An unsigned IPA
requires a separate signing process before installation.

The Apple model terms limit the model to research purposes and require the
license and attribution on redistribution. Review
`tools/MobileCLIPConversion/MODEL_LICENSE.md` before using or distributing the
generated model. This is not legal advice.

## Desktop image filter

See [CrossPlatformImageFilter](CrossPlatformImageFilter/README.md).
