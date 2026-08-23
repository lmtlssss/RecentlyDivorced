#!/usr/bin/env bash
set -euo pipefail

readonly expected_commit='83d1fe0e67b1323f71febc2925817732b449f1d9'
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly patches=(
  "$script_dir/../patches/latest-user-prompt-preview.patch"
  "$script_dir/../patches/replay-and-fallback-preview.patch"
)
source_root="${1:?usage: scripts/apply-patch.sh /path/to/openai-codex-checkout}"

actual_commit="$(git -C "$source_root" rev-parse HEAD)"
if [[ "$actual_commit" != "$expected_commit" ]]; then
  printf 'RecentlyDivorced refuses upstream %s; reviewed pin is %s\n' "$actual_commit" "$expected_commit" >&2
  exit 1
fi

for patch_file in "${patches[@]}"; do
  git -C "$source_root" apply --check "$patch_file"
done
for patch_file in "${patches[@]}"; do
  git -C "$source_root" apply "$patch_file"
done
printf 'RecentlyDivorced patch applied to %s\n' "$actual_commit"
