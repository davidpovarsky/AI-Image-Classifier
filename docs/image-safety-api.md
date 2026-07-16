# Image Safety API

`POST /v1/image-safety-classify` accepts one raw JPEG, PNG, HEIC, or HEIF body
at `http://127.0.0.1:8765`. It returns model evidence and diagnostics; it never
decides whether content is allowed or blocked.

## Authentication and request

Send `Authorization: Bearer <token>` and the matching image `Content-Type`.
The maximum encoded body is 10 MiB. Images larger than 2048 pixels on either
axis or 12 megapixels are downscaled once after EXIF orientation normalization.
All later modules consume that same normalized image and return normalized
coordinates, so the normalized boxes remain valid for the original image.

```bash
curl \
  -X POST \
  -H "Authorization: Bearer TOKEN" \
  -H "Content-Type: image/jpeg" \
  --data-binary "@image.jpg" \
  http://127.0.0.1:8765/v1/image-safety-classify
```

Missing or invalid authorization returns HTTP 401. Unsupported content types
return 415, oversized payloads return 413, and undecodable images return 400.
Tokens and image bytes are never written to diagnostics.

## Pipeline and coordinates

The serialized pipeline is decode, Vision person detection, person crop
creation, MobileCLIP2 classification, NudeNet full-image detection, NudeNet
crop detection, coordinate mapping, and duplicate merging. At most 12 people
are processed, ordered by bounding-box area and then Vision confidence. Crops
use 15% horizontal and 20% vertical padding and are not persisted.

Raw Vision person rectangles retain their native `normalized-bottom-left`
coordinates. Aggregated NudeNet and merged boxes use `normalized-top-left`,
where `(0, 0)` is the upper-left corner. Crop detections include both
`boundingBoxInCrop` and `boundingBoxInOriginalImage`.

NudeNet exposes the exact raw label from its canonical 18-label map. No server
confidence threshold or label normalization is applied. Duplicate detections
merge only when the raw labels match and IoU is at least 0.50. The highest
confidence box wins, while all source IDs, confidences, scopes, and person IDs
are retained. Both raw and merged arrays remain in the response.

## Response and partial failure

HTTP 200 means the request was decoded and a diagnostic response was produced.
`pipeline.status` is `success` or `partialSuccess`; every module has its own
status, duration, warnings, and structured error. A failure in MobileCLIP2 or
either NudeNet scope does not erase successful evidence from other modules.

The top-level shape is:

```json
{
  "success": true,
  "requestId": "UUID",
  "serverVersion": 6,
  "pipelineVersion": 1,
  "input": {"orientationNormalized": true},
  "pipeline": {"status": "success", "partialFailure": false, "modules": {}},
  "people": [],
  "nudity": {
    "fullImageRawDetections": [],
    "personCropRawDetections": [],
    "allRawDetections": [],
    "mergedDetections": [],
    "unassignedDetections": []
  },
  "summary": {
    "hasPersonDetections": false,
    "hasNudityDetections": false
  },
  "warnings": [],
  "errors": []
}
```

There is deliberately no `blocked`, `allowed`, or risk-policy field.

## Diagnostics and privacy

`GET /health` reports independent MobileCLIP2 and NudeNet readiness plus the
pipeline endpoint. Authenticated `GET /v1/diagnostics/status` reports the last
pipeline request and module timings. Detailed files are available under
`Files > On My iPad > AI Image Classifier > AI-Image-Classifier Diagnostics`:

- `image-safety-events.jsonl`
- `image-safety-summary.json`
- `image-safety-module-errors.jsonl`

Images, crops, image bytes, URLs, bearer tokens, and full 512-value embeddings
are not persisted or returned. Only embedding dimension, norm, and finiteness
are included. Inference is on-device and sequential to limit memory pressure.

Physical-device execution of the new combined pipeline has not been confirmed
until the generated IPA is installed and tested on the user's iPad.
