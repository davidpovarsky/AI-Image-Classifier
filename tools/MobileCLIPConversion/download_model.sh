#!/usr/bin/env bash
set -euo pipefail

root="${1:-$PWD/.model-cache/mobileclip2-s2}"
revision="72424e7025436db18f15c3eff6ee8c7c15ad4481"
expected_sha256="37c2d839a856491f2fcc82c40dc28672dbd0907235b4cd4c38dfff6457f0c09f"
mkdir -p "$root"
huggingface-cli download apple/MobileCLIP2-S2 mobileclip2_s2.pt --revision "$revision" --local-dir "$root"
checkpoint="$root/mobileclip2_s2.pt"
test -s "$checkpoint"
sha256="$(shasum -a 256 "$checkpoint" | awk '{print $1}')"
[[ "$sha256" == "$expected_sha256" ]] || {
  printf 'Checkpoint SHA-256 mismatch: expected %s, got %s\n' "$expected_sha256" "$sha256" >&2
  exit 1
}
python tools/MobileCLIPConversion/write_manifest.py "$checkpoint" "$sha256"
printf 'Downloaded %s\nSHA-256: %s\n' "$checkpoint" "$sha256"
