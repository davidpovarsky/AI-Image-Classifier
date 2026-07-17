#!/usr/bin/env bash
set -euo pipefail
host_address="${1:-127.0.0.1}"
port="${2:-8080}"
[[ "$port" =~ ^[0-9]+$ ]] && ((port >= 1 && port <= 65535)) || { echo "Invalid port" >&2; exit 1; }
state_dir="$HOME/Library/Application Support/LocalAIImageFilter"
backup="$state_dir/proxy-backup.env"
mkdir -p "$state_dir"

default_interface="$(route -n get default | awk '/interface:/{print $2; exit}')"
service="$(networksetup -listnetworkserviceorder | awk -v interface="$default_interface" '
  /^\([0-9]+\)/ { line=$0; sub(/^\([0-9]+\) /,"",line); service=line }
  index($0,"Device: " interface ")") { print service; exit }
')"
[[ -n "$service" ]] || { echo "Could not identify the active macOS network service." >&2; exit 1; }

value() { awk -F': ' -v key="$2" '$1 == key { print substr($0, length(key) + 3); exit }' <<<"$1"; }
if [[ ! -f "$backup" ]]; then
  web="$(networksetup -getwebproxy "$service")"
  secure="$(networksetup -getsecurewebproxy "$service")"
  auto="$(networksetup -getautoproxyurl "$service")"
  {
    printf 'SERVICE=%q\n' "$service"
    printf 'WEB_ENABLED=%q\n' "$(value "$web" Enabled)"
    printf 'WEB_SERVER=%q\n' "$(value "$web" Server)"
    printf 'WEB_PORT=%q\n' "$(value "$web" Port)"
    printf 'SECURE_ENABLED=%q\n' "$(value "$secure" Enabled)"
    printf 'SECURE_SERVER=%q\n' "$(value "$secure" Server)"
    printf 'SECURE_PORT=%q\n' "$(value "$secure" Port)"
    printf 'AUTO_ENABLED=%q\n' "$(value "$auto" Enabled)"
    printf 'AUTO_URL=%q\n' "$(value "$auto" URL)"
  } > "$backup"
fi

networksetup -setwebproxy "$service" "$host_address" "$port"
networksetup -setsecurewebproxy "$service" "$host_address" "$port"
networksetup -setwebproxystate "$service" on
networksetup -setsecurewebproxystate "$service" on
echo "Enabled proxy on active service '$service'; backup: $backup"
