#!/usr/bin/env bash
set -euo pipefail

root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
manager="$root/manager/current/recentlydivorced"
[[ -x "$manager" ]] || { echo "RecentlyDivorced is not installed" >&2; exit 0; }

if command -v systemctl >/dev/null && systemctl --user show-environment >/dev/null 2>&1; then
  systemctl --user disable --now recentlydivorced-repair.path >/dev/null 2>&1 || true
  systemctl --user disable --now recentlydivorced-repair.service >/dev/null 2>&1 || true
fi
exec "$manager" --rd-bootstrap-uninstall
