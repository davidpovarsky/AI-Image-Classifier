#!/usr/bin/env bash
set -euo pipefail
certificate="${1:-$HOME/.mitmproxy/mitmproxy-ca-cert.pem}"
[[ -f "$certificate" ]] || { echo "Missing $certificate; start mitmdump once first." >&2; exit 1; }
keychain="$HOME/Library/Keychains/login.keychain-db"
fingerprint="$(openssl x509 -in "$certificate" -noout -fingerprint -sha256 | cut -d= -f2 | tr -d :)"
security add-trusted-cert -r trustRoot -k "$keychain" "$certificate"
state_dir="$HOME/Library/Application Support/LocalAIImageFilter"
mkdir -p "$state_dir"
printf '%s\n' "$fingerprint" > "$state_dir/ca-thumbprint.txt"
echo "Trusted mitmproxy CA in the login keychain ($fingerprint)."
