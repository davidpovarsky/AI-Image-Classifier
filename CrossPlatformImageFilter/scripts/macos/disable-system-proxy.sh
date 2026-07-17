#!/usr/bin/env bash
set -euo pipefail
backup="$HOME/Library/Application Support/LocalAIImageFilter/proxy-backup.env"
[[ -f "$backup" ]] || { echo "Proxy backup missing: $backup; refusing to guess." >&2; exit 1; }
# shellcheck disable=SC1090
source "$backup"
networksetup -setwebproxy "$SERVICE" "$WEB_SERVER" "$WEB_PORT"
networksetup -setsecurewebproxy "$SERVICE" "$SECURE_SERVER" "$SECURE_PORT"
networksetup -setwebproxystate "$SERVICE" "$( [[ "$WEB_ENABLED" == Yes ]] && echo on || echo off )"
networksetup -setsecurewebproxystate "$SERVICE" "$( [[ "$SECURE_ENABLED" == Yes ]] && echo on || echo off )"
if [[ "$AUTO_ENABLED" == Yes && "$AUTO_URL" != '(null)' && -n "$AUTO_URL" ]]; then
  networksetup -setautoproxyurl "$SERVICE" "$AUTO_URL"
  networksetup -setautoproxystate "$SERVICE" on
else
  networksetup -setautoproxystate "$SERVICE" off
fi
rm -f -- "$backup"
echo "Restored proxy settings for '$SERVICE'."
