#!/usr/bin/env bash
set -euo pipefail

source_root="${1:?usage: scripts/verify.sh /path/to/openai-codex-checkout}"
git -C "$source_root" diff --check
(cd "$source_root/codex-rs" && cargo test -p codex-thread-store thread_metadata_sync)
(cd "$source_root/codex-rs" && cargo test -p codex-state extract)
(cd "$source_root/codex-rs" && cargo test -p codex-rollout list)
(cd "$source_root/codex-rs" && cargo test -p codex-app-server thread_processor)
