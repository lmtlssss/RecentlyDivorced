#!/usr/bin/env bash
set -euo pipefail
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
link="$HOME/.local/bin/codex"
mapfile -t stock < "$root/stock-codex.path"
[[ ${#stock[@]} -eq 2 && -x "${stock[1]}" ]] || { echo 'invalid stock record' >&2; exit 1; }
[[ "$(readlink "$link")" == "$root/codex-launcher" ]] || { echo 'refusing to overwrite a non-RecentlyDivorced codex link' >&2; exit 1; }
ln -sfn "${stock[0]}" "$HOME/.local/bin/.codex-stock.new"
mv -Tf "$HOME/.local/bin/.codex-stock.new" "$link"
printf 'Restored stock codex link. Payloads retained at %s\n' "$root"
