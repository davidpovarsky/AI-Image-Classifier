#!/usr/bin/env bash
set -euo pipefail

: "${TOKEN:?Set TOKEN to the token displayed by the app}"
BASE_URL="${BASE_URL:-http://127.0.0.1:8765}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

command -v jq >/dev/null || { echo "jq is required" >&2; exit 1; }
command -v sips >/dev/null || { echo "sips is required to create safe fixtures" >&2; exit 1; }

printf 'P3\n2 2\n255\n0 128 255 0 128 255 0 128 255 0 128 255\n' > "$work/safe.ppm"
sips -s format jpeg "$work/safe.ppm" --out "$work/safe.jpg" >/dev/null
sips -s format png "$work/safe.ppm" --out "$work/safe.png" >/dev/null

health_code="$(curl -sS -o "$work/health.json" -w '%{http_code}' "$BASE_URL/health")"
test "$health_code" = 200
jq -e '
  .status == "ok" and
  .serverVersion == 5 and
  .model == "MobileCLIP2-S2" and
  .modelLoaded == true and
  .imageEncoderLoaded == true and
  .textEncoderBundled == false and
  .promptEmbeddingsReady == true and
  .modelPrecision == "float16"
' "$work/health.json" >/dev/null

unauthorized_code="$(curl -sS -o "$work/unauthorized.json" -w '%{http_code}' \
  -H 'Content-Type: image/jpeg' --data-binary @"$work/safe.jpg" "$BASE_URL/v1/person-classify")"
test "$unauthorized_code" = 401

for fixture in safe.jpg safe.png; do
  case "$fixture" in *.jpg) media=image/jpeg ;; *.png) media=image/png ;; esac
  code="$(curl -sS -o "$work/$fixture.json" -w '%{http_code}' \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: $media" \
    --data-binary @"$work/$fixture" "$BASE_URL/v1/person-classify")"
  test "$code" = 200
  jq -e '
    .success == true and
    .model == "MobileCLIP2-S2" and
    .serverVersion == 5 and
    (.durationMs | type == "number") and
    (.people | type == "array") and
    (.peopleCount == (.people | length)) and
    (.people | all(
      (.box.x >= 0 and .box.x <= 1) and
      (.box.y >= 0 and .box.y <= 1) and
      ((.scores.woman + .scores.man + .scores.uncertain + .scores.notPerson) > 0.999) and
      ((.scores.woman + .scores.man + .scores.uncertain + .scores.notPerson) < 1.001)
    )) and
    (has("allowed") | not) and
    (has("risk") | not) and
    (has("confidence") | not) and
    (has("triggeredClass") | not) and
    (has("predictions") | not)
  ' "$work/$fixture.json" >/dev/null
done

malformed_code="$(printf 'not an image' | curl -sS -o "$work/malformed.json" -w '%{http_code}' \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: image/jpeg' \
  --data-binary @- "$BASE_URL/v1/person-classify")"
test "$malformed_code" = 400
jq -e '.success == false and .error == "invalid_image"' "$work/malformed.json" >/dev/null

echo "Local MobileCLIP2-S2 server contract passed for health, auth, JPEG, PNG, malformed input, and schema."
