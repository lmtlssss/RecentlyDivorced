#!/usr/bin/env bash
set -euo pipefail
link="$HOME/.local/bin/codex"
owned_target="$(readlink "$link")"
[[ "$owned_target" = /* && -x "$owned_target" ]] || { echo 'public codex link is not an owned absolute launcher' >&2; exit 1; }
root="$(dirname "$owned_target")"
[[ "$(basename "$owned_target")" == "codex-launcher" ]] || { echo 'public codex link is not RecentlyDivorced-owned' >&2; exit 1; }
[[ -f "$root/stock-codex.path" && -d "$root/payloads" ]] || { echo 'missing RecentlyDivorced install marker' >&2; exit 1; }
exec 9>"$root/install.lock"; flock -n 9 || { echo 'RecentlyDivorced install is running' >&2; exit 1; }
mapfile -t stock < "$root/stock-codex.path"
[[ ${#stock[@]} -eq 2 && -x "${stock[1]}" ]] || { echo 'invalid stock record' >&2; exit 1; }
[[ "$owned_target" == "$root/codex-launcher" ]] || { echo 'refusing to overwrite a non-RecentlyDivorced codex link' >&2; exit 1; }
stage="$(mktemp "$HOME/.local/bin/.codex-stock.XXXXXX")"
ln -sfn -- "${stock[0]}" "$stage"
mv -Tf "$stage" "$link"
[[ "$(readlink "$link")" == "${stock[0]}" ]] || { echo 'stock restore verification failed' >&2; exit 1; }
printf 'Restored stock codex link. Payloads retained at %s\n' "$root"
