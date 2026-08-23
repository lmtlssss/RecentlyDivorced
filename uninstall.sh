#!/usr/bin/env bash
set -euo pipefail

init='{"method":"initialize","id":1,"params":{"clientInfo":{"name":"recentlydivorced-uninstaller","version":"0.1.0"}}}'
ready='{"method":"initialized","params":{}}'
clear='{"method":"config/batchWrite","id":2,"params":{"edits":[{"keyPath":"hooks.state.\"recentlydivorced@recentlydivorced:hooks/hooks.json:user_prompt_submit:0:0\"","value":null,"mergeStrategy":"replace"}],"reloadUserConfig":true}}'
{ printf '%s\n%s\n' "$init" "$ready"; sleep 1; printf '%s\n' "$clear"; sleep 1; } | codex app-server --stdio >/dev/null 2>&1 || true
codex plugin remove recentlydivorced@recentlydivorced >/dev/null 2>&1 || true
codex plugin marketplace remove recentlydivorced >/dev/null 2>&1 || true
printf '%s\n' "RecentlyDivorced removed. Stock Codex is unchanged."
