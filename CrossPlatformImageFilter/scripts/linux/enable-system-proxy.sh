#!/usr/bin/env bash
set -euo pipefail
host_address="${1:-127.0.0.1}"
port="${2:-8080}"
[[ "$port" =~ ^[0-9]+$ ]] && ((port >= 1 && port <= 65535)) || { echo "Invalid port" >&2; exit 1; }
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/local-ai-image-filter"
backup="$state_dir/gnome-proxy-backup.env"
mkdir -p "$state_dir"
if command -v gsettings >/dev/null 2>&1 && gsettings writable org.gnome.system.proxy mode >/dev/null 2>&1; then
  if [[ ! -f "$backup" ]]; then
    {
      printf 'MODE=%q\n' "$(gsettings get org.gnome.system.proxy mode)"
      printf 'HTTP_HOST=%q\n' "$(gsettings get org.gnome.system.proxy.http host)"
      printf 'HTTP_PORT=%q\n' "$(gsettings get org.gnome.system.proxy.http port)"
      printf 'HTTPS_HOST=%q\n' "$(gsettings get org.gnome.system.proxy.https host)"
      printf 'HTTPS_PORT=%q\n' "$(gsettings get org.gnome.system.proxy.https port)"
      printf 'IGNORE_HOSTS=%q\n' "$(gsettings get org.gnome.system.proxy ignore-hosts)"
    } > "$backup"
  fi
  gsettings set org.gnome.system.proxy mode 'manual'
  gsettings set org.gnome.system.proxy.http host "$host_address"
  gsettings set org.gnome.system.proxy.http port "$port"
  gsettings set org.gnome.system.proxy.https host "$host_address"
  gsettings set org.gnome.system.proxy.https port "$port"
  gsettings set org.gnome.system.proxy ignore-hosts "['localhost', '127.0.0.0/8', '::1']"
  echo "GNOME proxy enabled; backup: $backup"
else
  echo "GNOME settings are unavailable. Configure the application environment:" >&2
  echo "export HTTP_PROXY=http://$host_address:$port" >&2
  echo "export HTTPS_PROXY=http://$host_address:$port" >&2
  echo "export NO_PROXY=localhost,127.0.0.1,::1" >&2
fi
