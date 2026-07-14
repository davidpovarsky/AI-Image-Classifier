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
  .serverVersion == 3 and
  .model == "NudeNet320n" and
  .modelLoaded == true and
  (has("policyVersion") | not)
' "$work/health.json" >/dev/null

unauthorized_code="$(curl -sS -o "$work/unauthorized.json" -w '%{http_code}' \
  -H 'Content-Type: image/jpeg' --data-binary @"$work/safe.jpg" "$BASE_URL/v1/classify")"
test "$unauthorized_code" = 401

for fixture in safe.jpg safe.png; do
  case "$fixture" in *.jpg) media=image/jpeg ;; *.png) media=image/png ;; esac
  code="$(curl -sS -o "$work/$fixture.json" -w '%{http_code}' \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: $media" \
    --data-binary @"$work/$fixture" "$BASE_URL/v1/classify")"
  test "$code" = 200
  jq -e '
    .success == true and
    .model == "NudeNet320n" and
    (.durationMs | type == "number") and
    (.detections | type == "array") and
    (has("allowed") | not) and
    (has("risk") | not) and
    (has("confidence") | not) and
    (has("triggeredClass") | not) and
    (has("predictions") | not)
  ' "$work/$fixture.json" >/dev/null
done

malformed_code="$(printf 'not an image' | curl -sS -o "$work/malformed.json" -w '%{http_code}' \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: image/jpeg' \
  --data-binary @- "$BASE_URL/v1/classify")"
test "$malformed_code" = 400
jq -e '.success == false and .error == "invalid_image"' "$work/malformed.json" >/dev/null

echo "Local NudeNet server contract passed for health, auth, JPEG, PNG, malformed input, and schema."
