#!/usr/bin/env bash
set -euo pipefail
state="$HOME/Library/Application Support/LocalAIImageFilter/ca-thumbprint.txt"
[[ -f "$state" ]] || { echo "Saved CA fingerprint missing; refusing name-based deletion." >&2; exit 1; }
fingerprint="$(tr -d '[:space:]' < "$state")"
[[ "$fingerprint" =~ ^[0-9A-Fa-f]{64}$ ]] || { echo "Invalid saved fingerprint." >&2; exit 1; }
security delete-certificate -Z "$fingerprint" "$HOME/Library/Keychains/login.keychain-db"
rm -f -- "$state"
echo "Removed the saved mitmproxy CA from the login keychain."
