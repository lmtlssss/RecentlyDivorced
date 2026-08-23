#!/usr/bin/env bash
set -euo pipefail
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
current="$(readlink -f "$root/current")"
previous="$(readlink -f "$root/previous")"
[[ -x "$current/bin/codex" && -x "$previous/bin/codex" ]] || { echo 'no valid rollback payload' >&2; exit 1; }
ln -sfn "$current" "$root/previous.new"; mv -Tf "$root/previous.new" "$root/previous"
ln -sfn "$previous" "$root/current.new"; mv -Tf "$root/current.new" "$root/current"
printf 'RecentlyDivorced rolled back to %s\n' "$previous"
