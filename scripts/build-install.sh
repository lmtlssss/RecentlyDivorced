#!/usr/bin/env bash
set -euo pipefail

root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
lock="$repo_root/upstream.lock"
stock_link="$HOME/.local/bin/codex"
stock_record="$root/stock-codex.path"
mapfile -t lock_values < <(python3 - "$lock" <<'PY'
import sys, tomllib
data = tomllib.load(open(sys.argv[1], 'rb'))
required = {'schema', 'repo', 'commit', 'stock_version', 'target', 'patches'}
if set(data) != required or data['schema'] != 1 or not isinstance(data['patches'], list):
    raise SystemExit('invalid RecentlyDivorced upstream.lock')
for value in (data['repo'], data['commit'], data['stock_version'], data['target'], *data['patches']):
    if not isinstance(value, str) or not value:
        raise SystemExit('invalid RecentlyDivorced lock value')
print(data['repo']); print(data['commit']); print(data['target']); print(data['stock_version']); print(*data['patches'], sep='\n')
PY
)
remote="${lock_values[0]}"; commit="${lock_values[1]}"; target="${lock_values[2]}"; stock_version="${lock_values[3]}"
expected_patches=("${lock_values[@]:4}")
patch_files=("$repo_root/patches/latest-user-prompt-preview.patch" "$repo_root/patches/replay-and-fallback-preview.patch")
[[ ${#expected_patches[@]} -eq ${#patch_files[@]} ]] || { echo 'patch lock mismatch' >&2; exit 1; }
actual_patches=()
for patch_file in "${patch_files[@]}"; do actual_patches+=("$(sha256sum "$patch_file" | awk '{print $1}')"); done
[[ "${actual_patches[*]}" == "${expected_patches[*]}" ]] || { echo 'patch hash mismatch' >&2; exit 1; }
identity="$(printf '%s\0' "$commit" "${actual_patches[@]}" | sha256sum | awk '{print $1}')"

mkdir -p "$root/payloads" "$HOME/.local/bin"
exec 9>"$root/install.lock"
flock -n 9 || { echo 'RecentlyDivorced install is already running' >&2; exit 1; }
if [[ ! -f "$stock_record" ]]; then
  original_link="$(readlink "$stock_link")"
  [[ -n "$original_link" ]] || { echo 'stock codex must be a symlink before activation' >&2; exit 1; }
  if [[ "$original_link" = /* ]]; then stock="$original_link"; else stock="$(dirname "$stock_link")/$original_link"; fi
  [[ -x "$stock" && "$stock" != "$root/codex-launcher" && "$stock" != "$root"/* ]] || { echo 'stock codex target is unsafe' >&2; exit 1; }
  printf '%s\n%s\n' "$original_link" "$stock" > "$stock_record"
fi
mapfile -t stock_lines < "$stock_record"
[[ ${#stock_lines[@]} -eq 2 && -x "${stock_lines[1]}" && "${stock_lines[1]}" != "$root"/* ]] || { echo 'invalid stock record' >&2; exit 1; }
[[ "$("${stock_lines[1]}" --version | awk '{print $2}')" == "$stock_version" ]] || { echo 'stock Codex version diverges from reviewed lock' >&2; exit 1; }

payload="$root/payloads/$identity/$target"
mkdir -p "$(dirname "$payload")"
if [[ ! -x "$payload/bin/codex" ]]; then
  work="$(mktemp -d "$root/.build.XXXXXX")"
  trap 'rm -rf "$work"' EXIT
  git clone --quiet "$remote" "$work/codex"
  git -C "$work/codex" checkout --quiet "$commit"
  [[ "$(git -C "$work/codex" config --get remote.origin.url)" == "$remote" ]] || { echo 'upstream origin mismatch' >&2; exit 1; }
  [[ "$(git -C "$work/codex" rev-parse HEAD)" == "$commit" ]] || { echo 'upstream commit mismatch' >&2; exit 1; }
  "$repo_root/scripts/apply-patch.sh" "$work/codex"; "$repo_root/scripts/verify.sh" "$work/codex"
  (cd "$work/codex/codex-rs" && cargo build --release --locked -p codex-cli --target "$target")
  stage="$(mktemp -d "$root/payloads/.stage.XXXXXX")"
  mkdir -p "$stage/bin"; install -m 0755 "$work/codex/codex-rs/target/$target/release/codex" "$stage/bin/codex"
  [[ "$("$stage/bin/codex" --version | awk '{print $2}')" == "$stock_version" ]] || { echo 'patched binary version mismatch' >&2; exit 1; }
  (cd "$stage" && sha256sum bin/codex > SHA256SUMS)
  printf 'commit=%s\nidentity=%s\ntarget=%s\n' "$commit" "$identity" "$target" > "$stage/MANIFEST"
  mv -T "$stage" "$payload"
else
  (cd "$payload" && sha256sum -c SHA256SUMS >/dev/null)
fi
old_payload="$(readlink -f "$root/current" 2>/dev/null || true)"
if [[ -n "$old_payload" && "$old_payload" != "$payload" && -x "$old_payload/bin/codex" ]]; then ln -sfn "$old_payload" "$root/previous.new"; mv -Tf "$root/previous.new" "$root/previous"; fi
ln -sfn "$payload" "$root/current.new"
mv -Tf "$root/current.new" "$root/current"

launcher="$root/codex-launcher"
launcher_stage="$(mktemp "$root/.launcher.XXXXXX")"
cat > "$launcher_stage" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root='__RECENTLYDIVORCED_ROOT__'
if [[ "${1:-}" == "--recentlydivorced-stock" ]]; then
  shift
  mapfile -t stock < "$root/stock-codex.path"
  [[ ${#stock[@]} -eq 2 && -x "${stock[1]}" && "${stock[1]}" != "$root"/* ]] || { echo 'invalid stock record' >&2; exit 1; }
  exec "${stock[1]}" "$@"
fi
exec "$root/current/bin/codex" "$@"
EOF
sed -i "s|__RECENTLYDIVORCED_ROOT__|$root|" "$launcher_stage"
chmod 0755 "$launcher_stage"; mv -Tf "$launcher_stage" "$launcher"
ln -sfn "$launcher" "$HOME/.local/bin/.codex-recentlydivorced.new"
mv -Tf "$HOME/.local/bin/.codex-recentlydivorced.new" "$stock_link"
printf 'RecentlyDivorced active: %s\n' "$commit"
