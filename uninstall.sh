#!/usr/bin/env sh
set -eu

init='{"method":"initialize","id":1,"params":{"clientInfo":{"name":"recentlydivorced-uninstaller","version":"0.3.10"}}}'
ready='{"method":"initialized","params":{}}'
clear='{"method":"config/batchWrite","id":2,"params":{"edits":[{"keyPath":"hooks.state.\"recentlydivorced@recentlydivorced:hooks/hooks.json:session_start:0:0\"","value":null,"mergeStrategy":"replace"},{"keyPath":"hooks.state.\"recentlydivorced@recentlydivorced:hooks/hooks.json:user_prompt_submit:0:0\"","value":null,"mergeStrategy":"replace"}],"reloadUserConfig":true}}'
codex_home="${CODEX_HOME:-$HOME/.codex}"
plugin_data="$codex_home/plugins/data/recentlydivorced-recentlydivorced"
if [ -x "$plugin_data/recentlydivorced" ]; then
  PLUGIN_DATA="$plugin_data" "$plugin_data/recentlydivorced" --restore
fi
{ printf '%s\n' "$init"; sleep 1; printf '%s\n' "$ready"; sleep 1; printf '%s\n' "$clear"; sleep 2; } | codex app-server --stdio >/dev/null 2>&1 || true
codex plugin remove recentlydivorced@recentlydivorced >/dev/null 2>&1 || true
codex plugin marketplace remove recentlydivorced >/dev/null 2>&1 || true
rm -f "$plugin_data/recentlydivorced"
rmdir "$plugin_data" 2>/dev/null || true
printf '%s\n' "RecentlyDivorced removed. Stock Codex is unchanged."
