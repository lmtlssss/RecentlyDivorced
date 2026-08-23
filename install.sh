#!/usr/bin/env bash
set -euo pipefail

release_base="${RECENTLYDIVORCED_RELEASE_BASE:-https://github.com/lmtlssss/RecentlyDivorced/releases/latest/download}"
root="${XDG_DATA_HOME:-$HOME/.local/share}/recentlydivorced"
target="x86_64-unknown-linux-musl"
manager_asset="recentlydivorced-$target"
payload_asset="codex-$target"

command -v curl >/dev/null || { echo "RecentlyDivorced requires curl" >&2; exit 1; }
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || { echo "RecentlyDivorced currently ships Linux x86_64 only" >&2; exit 1; }
stock="$(command -v codex || true)"
[[ -n "$stock" && -x "$stock" ]] || { echo "install stock Codex first" >&2; exit 1; }
[[ "$stock" != "$root"/manager/* ]] || { echo "RecentlyDivorced is already installed" >&2; exit 0; }

bin="$HOME/.local/bin"
mkdir -p "$bin"
if [[ -L "$stock" && -w "$(dirname "$stock")" ]]; then
  public="$stock"
  created_public_link=false
else
  [[ ! -e "$bin/codex" ]] || { echo "$bin/codex already exists; refusing to shadow it" >&2; exit 1; }
  case ":$PATH:" in
    *":$bin:"*) public="$bin/codex" ;;
    *) echo "$bin must be on PATH to safely shadow stock Codex" >&2; exit 1 ;;
  esac
  ln -s "$stock" "$public"
  created_public_link=true
fi

mkdir -p "$root"
stage="$(mktemp -d "$root/.install.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
curl --fail --silent --show-error --location "$release_base/release.toml" -o "$stage/release.toml"
curl --fail --silent --show-error --location "$release_base/release.toml.sig" -o "$stage/release.toml.sig"
curl --fail --silent --show-error --location "$release_base/$manager_asset" -o "$stage/recentlydivorced"
curl --fail --silent --show-error --location "$release_base/$payload_asset" -o "$stage/codex"
chmod 0755 "$stage/recentlydivorced" "$stage/codex"

exec "$stage/recentlydivorced" --rd-bootstrap-install --rd-root "$root" --rd-public-link "$public" --rd-target "$target" --rd-installation-id "$(date +%s)-$$" --rd-created-public-link="$created_public_link" --rd-payload "$stage/codex" --rd-release-manifest "$stage/release.toml" --rd-release-signature "$stage/release.toml.sig"
