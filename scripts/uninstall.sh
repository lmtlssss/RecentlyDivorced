#!/usr/bin/env bash
set -euo pipefail
link="$HOME/.local/bin/codex"
owned_target="$(readlink "$link")"
root="$(dirname "$owned_target")"
[[ "$(basename "$owned_target")" == "codex-launcher" ]] || { echo 'public codex link is not RecentlyDivorced-owned' >&2; exit 1; }
exec 9>"$root/install.lock"; flock -n 9 || { echo 'RecentlyDivorced install is running' >&2; exit 1; }
mapfile -t stock < "$root/stock-codex.path"
[[ ${#stock[@]} -eq 2 && -x "${stock[1]}" ]] || { echo 'invalid stock record' >&2; exit 1; }
[[ "$owned_target" == "$root/codex-launcher" ]] || { echo 'refusing to overwrite a non-RecentlyDivorced codex link' >&2; exit 1; }
ln -sfn -- "${stock[0]}" "$HOME/.local/bin/.codex-stock.new"
mv -Tf "$HOME/.local/bin/.codex-stock.new" "$link"
[[ "$(readlink "$link")" == "${stock[0]}" ]] || { echo 'stock restore verification failed' >&2; exit 1; }
printf 'Restored stock codex link. Payloads retained at %s\n' "$root"
