# Local NudeNet HTTP server

The app listens only on `http://127.0.0.1:8765`. NudeNet 320n is loaded once
with Core ML compute units set to `all`, warmed once, and retained. The actor
serializes inference so HTTP parsing can remain concurrent without parallel
model executions or per-request model loading.

Images remain in memory and are never persisted, uploaded, or logged. Responses
set `Cache-Control: no-store`. The limit is 10 MiB.

The local server performs NudeNet inference only. It does not apply moderation,
blocking, safety, or policy decisions. Clients must interpret detection labels
and confidence values themselves.

## Authentication

The first server use generates a UUID bearer token and stores it in
`UserDefaults`; it remains stable across launches. Copy it from **Local Server**
and send `Authorization: Bearer <token>`. Tokens and image data are never logged.

## `GET /health`

Ready response (HTTP 200):

```json
{
  "status": "ok",
  "serverVersion": 3,
  "model": "NudeNet320n",
  "modelLoaded": true,
  "inputSize": 320,
  "computeUnits": "all",
  "error": null
}
```

Loading, warm-up, or a sanitized model failure returns HTTP 503 with
`status: "unavailable"` and `modelLoaded: false`. Classification is not accepted
until readiness is true.

## `POST /v1/classify`

Send raw JPEG, PNG, WebP, HEIC, or HEIF bytes with the matching `Content-Type`.
JPEG and PNG decoding is validated before inference. HTTP 200 means inference
succeeded. The response includes the model name, inference duration, and every
detection returned by NudeNet. Each detection contains its class ID, label,
confidence, and normalized Vision-coordinate box. Detections are sorted by
confidence in descending order.

```json
{
  "success": true,
  "model": "NudeNet320n",
  "durationMs": 8,
  "detections": [
    {
      "classId": 3,
      "label": "FEMALE_BREAST_EXPOSED",
      "confidence": 0.61,
      "box": {
        "x": 0.31,
        "y": 0.22,
        "width": 0.18,
        "height": 0.24
      }
    }
  ]
}
```

Errors:

- 400 `invalid_image`
- 401 `unauthorized`
- 413 `payload_too_large`
- 415 `unsupported_media_type`
- 503 `model_unavailable`
- 500 `classification_failed`

Run `TOKEN='<copied token>' scripts/test-local-server.sh` in a macOS shell while
the app is active to generate harmless solid-color JPEG/PNG fixtures and check
health, auth, decoding, malformed input, and response schema.

## Lifecycle and manual validation

iOS can suspend an ordinary foreground app. No unsupported background mode was
added and the server cannot promise indefinite background execution. Keep the
app active (Split View is useful on iPad). The retained model is not
intentionally unloaded or repeatedly warmed during scene changes.

For real-material accuracy validation, use private, lawfully obtained NSFW
fixtures on the target physical iPad. Do not add those files to this repository,
logs, screenshots, or CI artifacts. Record only aggregate latency and memory
measurements in the README benchmark table.

## Scripting and unsigned IPA

A future on-device Scripting workflow can POST raw file bytes with the bearer
token and interpret the returned detection labels and confidence values. It must
not use Base64 or multipart.

The **Build unsigned IPA** Action builds `generic/platform=iOS` with signing
disabled and uploads artifact `AI-Image-Classifier-unsigned-ipa`, containing
`AI-Image-Classifier-unsigned.ipa`. Open the successful Actions run, scroll to
**Artifacts**, and download it. It contains an unsigned `.app`; it is not an
App Store export and has no certificate or provisioning profile.
