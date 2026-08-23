#!/usr/bin/env bash
set -euo pipefail
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced/source"
repo='https://github.com/lmtlssss/RecentlyDivorced.git'
ref="${RECENTLYDIVORCED_REF:?set RECENTLYDIVORCED_REF to a reviewed release tag or commit}"
if [[ -d "$root/.git" ]]; then git -C "$root" fetch --quiet --tags "$repo"; else git clone --quiet "$repo" "$root"; fi
git -C "$root" checkout --quiet --detach "$ref"
exec "$root/scripts/build-install.sh"
