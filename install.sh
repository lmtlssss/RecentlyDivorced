#!/usr/bin/env sh
set -eu

repo="lmtlssss/RecentlyDivorced"
marketplace="recentlydivorced"
plugin="recentlydivorced"
asset="recentlydivorced-x86_64-unknown-linux-gnu"

command -v codex >/dev/null || { echo "install Codex first" >&2; exit 1; }
command -v curl >/dev/null || { echo "RecentlyDivorced requires curl" >&2; exit 1; }
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || { echo "RecentlyDivorced currently ships Linux x86_64 only" >&2; exit 1; }

codex plugin marketplace add "$repo" --ref main >/dev/null
codex plugin marketplace upgrade "$marketplace" >/dev/null 2>&1 || true
install_json="$(codex plugin add "$plugin@$marketplace" --json)"
plugin_root="$(printf '%s\n' "$install_json" | sed -n 's/^[[:space:]]*"installedPath":[[:space:]]*"\([^"]*\)".*/\1/p')"
[ -n "$plugin_root" ] && [ -d "$plugin_root" ] || { echo "Codex did not return the installed plugin path" >&2; exit 1; }

codex_home="${CODEX_HOME:-$HOME/.codex}"
plugin_data="$codex_home/plugins/data/recentlydivorced-recentlydivorced"
mkdir -p "$plugin_data"
curl --fail --silent --show-error --location "https://github.com/$repo/releases/latest/download/$asset" -o "$plugin_data/recentlydivorced"
chmod 0755 "$plugin_data/recentlydivorced"
"$plugin_data/recentlydivorced" --trust
estimate="$(PLUGIN_DATA="$plugin_data" "$plugin_data/recentlydivorced" --estimate)"
printf '%s\n' "Archive: $estimate. The first uncached pass uses gpt-5.6-sol at low reasoning."
printf '%s\n' "This consumes Codex usage. Later launches use gpt-5.3-codex-spark only for changed conversations."
if ! PLUGIN_DATA="$plugin_data" "$plugin_data/recentlydivorced" --catch-up; then
  printf '%s\n' "Archive catch-up paused; the next Codex launch will retry it." >&2
fi
printf '%s\n' "RecentlyDivorced installed. Use codex normally."
