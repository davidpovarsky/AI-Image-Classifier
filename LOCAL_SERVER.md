# Local MobileCLIP2-S2 HTTP server

The app listens only on `http://127.0.0.1:8765`. A coordinator actor serializes
Vision, crop, and Core ML work. Both encoders and the prompt-category embeddings
are retained in memory and are never recreated for each crop.

Images remain in memory and responses use `Cache-Control: no-store`. The raw
request-body limit is 10 MiB. The server is a neutral inference engine; client
code owns all moderation or blocking decisions.

## Authentication

Send `Authorization: Bearer <token>`. The app creates the UUID token once in
`UserDefaults` and displays copy controls on the Local Server screen. Tokens
and image data are not logged.

## `GET /health`

HTTP 200 is returned only when the image encoder, text encoder, and prompt
embeddings are all ready:

```json
{
  "status": "ok",
  "serverVersion": 4,
  "model": "MobileCLIP2-S2",
  "modelLoaded": true,
  "imageEncoderLoaded": true,
  "textEncoderLoaded": true,
  "promptEmbeddingsReady": true,
  "humanDetector": "VNDetectHumanRectanglesRequest",
  "computeUnits": "all",
  "error": null
}
```

A missing or failed asset returns HTTP 503, `status: "error"`,
`modelLoaded: false`, individual readiness flags, and a sanitized error.

## `POST /v1/person-classify`

Send raw JPEG, PNG, HEIC, or HEIF data with the matching `Content-Type`.

```json
{
  "success": true,
  "model": "MobileCLIP2-S2",
  "serverVersion": 4,
  "durationMs": 83,
  "imageWidth": 1920,
  "imageHeight": 1080,
  "peopleCount": 1,
  "people": [{
    "id": "72E9299C-0FE8-47F8-974F-B7846B5849E0",
    "detectionSource": "humanRectangle",
    "personDetectionConfidence": 0.94,
    "box": {"x": 0.12, "y": 0.08, "width": 0.31, "height": 0.82},
    "predictedClass": "woman",
    "confidence": 0.81,
    "scores": {"woman": 0.81, "man": 0.10, "uncertain": 0.07, "notPerson": 0.02}
  }]
}
```

Boxes use normalized top-left client coordinates. `humanRectangle` is a Vision
person result; `faceFallback` is an expanded face region and is not represented
as a full-body detection. With no usable detection the successful response has
`peopleCount: 0` and an empty `people` array.

Errors are 400 `invalid_image`, 401 `unauthorized`, 413
`payload_too_large`, 415 `unsupported_media_type`, 503 `model_unavailable`, and
500 `classification_failed`.

## Lifecycle

iOS may suspend an ordinary foreground app. No unsupported background mode was
added, so keep the app active while using the server. The server exposes only
raw model observations and scores; it does not expose or apply NudeNet policy.
