#!/usr/bin/env bash
set -euo pipefail

root="${1:-$PWD/.model-cache/mobileclip2-s2}"
mkdir -p "$root"
huggingface-cli download apple/MobileCLIP2-S2 --local-dir "$root"
checkpoint="$root/mobileclip2_s2.pt"
test -s "$checkpoint"
sha256="$(shasum -a 256 "$checkpoint" | awk '{print $1}')"
python tools/MobileCLIPConversion/write_manifest.py "$checkpoint" "$sha256"
printf 'Downloaded %s\nSHA-256: %s\n' "$checkpoint" "$sha256"
