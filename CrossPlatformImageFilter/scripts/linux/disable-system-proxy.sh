#!/usr/bin/env bash
set -euo pipefail
backup="${XDG_STATE_HOME:-$HOME/.local/state}/local-ai-image-filter/gnome-proxy-backup.env"
if [[ -f "$backup" ]]; then
  # shellcheck disable=SC1090
  source "$backup"
  gsettings set org.gnome.system.proxy mode "$MODE"
  gsettings set org.gnome.system.proxy.http host "$HTTP_HOST"
  gsettings set org.gnome.system.proxy.http port "$HTTP_PORT"
  gsettings set org.gnome.system.proxy.https host "$HTTPS_HOST"
  gsettings set org.gnome.system.proxy.https port "$HTTPS_PORT"
  gsettings set org.gnome.system.proxy ignore-hosts "$IGNORE_HOSTS"
  rm -f -- "$backup"
  echo "Restored previous GNOME proxy settings."
else
  echo "No GNOME proxy backup exists. Unset HTTP_PROXY, HTTPS_PROXY, and NO_PROXY if configured." >&2
fi
