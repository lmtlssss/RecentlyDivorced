#!/usr/bin/env bash
set -euo pipefail

root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
lock="$repo_root/upstream.lock"
stock_link="$HOME/.local/bin/codex"
stock_record="$root/stock-codex.path"
commit="$(sed -n 's/^commit = "\(.*\)"$/\1/p' "$lock")"
remote="$(sed -n 's/^repo = "\(.*\)"$/\1/p' "$lock")"
target="$(sed -n 's/^target = "\(.*\)"$/\1/p' "$lock")"

mkdir -p "$root/payloads" "$HOME/.local/bin"
if [[ ! -f "$stock_record" ]]; then
  stock="$(readlink -f "$stock_link")"
  [[ -x "$stock" ]] || { echo 'stock codex is not executable' >&2; exit 1; }
  printf '%s\n' "$stock" > "$stock_record"
fi

work="$(mktemp -d "$root/.build.XXXXXX")"
trap 'rm -rf "$work"' EXIT
git clone --quiet "$remote" "$work/codex"
git -C "$work/codex" checkout --quiet "$commit"
"$repo_root/scripts/apply-patch.sh" "$work/codex"
"$repo_root/scripts/verify.sh" "$work/codex"
(cd "$work/codex/codex-rs" && cargo build --release -p codex-cli)

payload="$root/payloads/$commit/$target"
stage="$root/payloads/.${commit}.${target}.new"
mkdir -p "$stage/bin"
install -m 0755 "$work/codex/codex-rs/target/release/codex" "$stage/bin/codex"
mv -T "$stage" "$payload"
ln -sfn "$payload" "$root/current.new"
mv -Tf "$root/current.new" "$root/current"

launcher="$root/codex-launcher"
cat > "$launcher" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
if [[ "${1:-}" == "--recentlydivorced-stock" ]]; then
  shift
  exec "$(cat "$root/stock-codex.path")" "$@"
fi
exec "$root/current/bin/codex" "$@"
EOF
chmod 0755 "$launcher"
ln -sfn "$launcher" "$HOME/.local/bin/.codex-recentlydivorced.new"
mv -Tf "$HOME/.local/bin/.codex-recentlydivorced.new" "$stock_link"
printf 'RecentlyDivorced active: %s\n' "$commit"
