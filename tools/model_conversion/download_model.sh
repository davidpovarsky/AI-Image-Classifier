#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
weights="$script_dir/320n.pt"
checksum_file="$script_dir/model.sha256"
public_url="https://github.com/notAI-tech/NudeNet/releases/download/v3.4-weights/320n.pt"
api_url="https://api.github.com/repos/notAI-tech/NudeNet/releases/assets/176832011"

rm -f "$weights.tmp"
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  curl --fail --location --silent --show-error \
    -H "Accept: application/octet-stream" \
    -H "Authorization: Bearer $GITHUB_TOKEN" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "$api_url" -o "$weights.tmp"
else
  curl --fail --location --silent --show-error "$public_url" -o "$weights.tmp"
fi

test -s "$weights.tmp"
mv "$weights.tmp" "$weights"

if [[ -s "$checksum_file" ]]; then
  (cd "$script_dir" && shasum -a 256 -c "$(basename "$checksum_file")")
else
  (cd "$script_dir" && shasum -a 256 "$(basename "$weights")") | tee "$checksum_file"
fi

echo "Downloaded verified NudeNet 320n weights to $weights"
