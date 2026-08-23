#!/usr/bin/env bash
set -euo pipefail
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced/source"
repo='https://github.com/lmtlssss/RecentlyDivorced.git'
if [[ -d "$root/.git" ]]; then git -C "$root" pull --ff-only; else git clone "$repo" "$root"; fi
exec "$root/scripts/build-install.sh"
