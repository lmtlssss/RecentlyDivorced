#!/usr/bin/env bash
set -euo pipefail

codex plugin remove recentlydivorced@recentlydivorced >/dev/null 2>&1 || true
codex plugin marketplace remove recentlydivorced >/dev/null 2>&1 || true
printf '%s\n' "RecentlyDivorced removed. Stock Codex is unchanged."
