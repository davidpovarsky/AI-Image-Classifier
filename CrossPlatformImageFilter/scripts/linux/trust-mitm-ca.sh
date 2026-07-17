#!/usr/bin/env bash
set -euo pipefail
CERT="${1:-$HOME/.mitmproxy/mitmproxy-ca-cert.cer}"
[[ -f "$CERT" ]] || { echo "Missing $CERT; start mitmdump once first." >&2; exit 1; }
if command -v update-ca-certificates >/dev/null 2>&1; then
  sudo cp "$CERT" /usr/local/share/ca-certificates/local-image-filter-mitmproxy.crt
  sudo update-ca-certificates
elif command -v trust >/dev/null 2>&1; then
  sudo trust anchor "$CERT"
else
  echo "Install the certificate manually in your distribution/browser trust store: $CERT" >&2
fi
