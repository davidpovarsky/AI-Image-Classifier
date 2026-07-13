# Local Core ML HTTP Server

The app can run a loopback-only HTTP server at `http://127.0.0.1:8765`. Classification is performed locally by the existing MobileNetV2 Core ML service. Images are held in memory only: the server does not save them, log their bytes, or upload them.

## Authentication

The app creates a random UUID token on first use and stores it in `UserDefaults`. The token remains stable between launches and is shown on the **Local Server** screen. Send it as:

```http
Authorization: Bearer <token>
```

## API

### `GET /health`

Returns HTTP 200 and JSON:

```json
{
  "status": "ok",
  "model": "MobileNetV2",
  "serverVersion": 1
}
```

### `POST /v1/classify`

Send the encoded image bytes directly as the request body. Multipart forms and Base64 are not supported.

Supported media types are `image/jpeg`, `image/png`, `image/webp`, `image/heic`, and `image/heif`. The maximum body size is 10 MiB (`10 * 1024 * 1024` bytes). A successful response contains up to three predictions sorted by descending confidence:

```json
{
  "success": true,
  "predictions": [
    {
      "label": "example",
      "confidence": 0.94
    }
  ],
  "durationMs": 35
}
```

Errors use a stable JSON body and the matching status code:

- 401: `unauthorized`
- 413: `payload_too_large`
- 415: `unsupported_media_type`
- 422: `invalid_image`
- 500: `classification_failed`

## iOS lifecycle limitation

iOS may suspend an ordinary app after it moves to the background. A listening socket therefore cannot be guaranteed while the app is suspended. Keep the app active while another local automation client uses it; on iPad, Split View is a practical option. Returning to the active scene asks the server to start again.

## Future use from Scripting

A future Scripting workflow can read an image file as raw data, set the appropriate `Content-Type` and bearer token headers, and POST those bytes to `http://127.0.0.1:8765/v1/classify`. The client should not encode the image as Base64 or multipart data.

## Unsigned IPA from GitHub Actions

The **Build unsigned IPA** workflow compiles the Release scheme for a generic physical iOS device with code signing disabled. It packages the resulting `.app` as `AI-Image-Classifier-unsigned.ipa`. This IPA is not signed for installation or App Store distribution; no certificate, provisioning profile, or Apple Developer signing secret is used.

To download it, open the repository's **Actions** tab, select the successful **Build unsigned IPA** run, and download the `AI-Image-Classifier-unsigned-ipa` artifact from the run summary. The artifact contains `AI-Image-Classifier-unsigned.ipa`.
