#!/usr/bin/env bash
set -euo pipefail

tag="${1:?usage: package-release.sh <tag> <manager> <codex> <output-dir>}"
manager="${2:?missing manager binary}"
codex="${3:?missing patched Codex binary}"
out="${4:?missing output directory}"
target="${RECENTLYDIVORCED_TARGET:-x86_64-unknown-linux-gnu}"
key="${RECENTLYDIVORCED_SIGNING_KEY:-$HOME/.config/recentlydivorced-release/ed25519-private.pem}"

[[ -x "$manager" && -x "$codex" && -f "$key" ]] || { echo "missing executable or signing key" >&2; exit 1; }
version="$("$codex" --version | awk '{for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+[.][0-9]+/) {print $i; exit}}')"
[[ -n "$version" ]] || { echo "could not read patched Codex version" >&2; exit 1; }
commit="$(awk -F\" '/^commit = / {print $2}' upstream.lock)"
patches=($(awk -F\" '/^[[:space:]]*"[0-9a-f]/ {print $2}' upstream.lock))
identity="$(printf '%s\0%s\0%s' "$commit" "${patches[0]}" "${patches[1]}" | sha256sum | awk '{print $1}')"
mkdir -p "$out"
install -m 0755 "$manager" "$out/recentlydivorced-$target"
install -m 0755 "$codex" "$out/codex-$target"
hash="$(sha256sum "$out/codex-$target" | awk '{print $1}')"

printf "schema = 1\nmanager_version = '%s'\n\n[[payloads]]\nstock_version = '%s'\ntarget = '%s'\nidentity = '%s'\nasset = 'codex-%s'\nsha256 = '%s'\n" "$tag" "$version" "$target" "$identity" "$target" "$hash" > "$out/release.toml"
openssl pkeyutl -sign -rawin -inkey "$key" -in "$out/release.toml" | base64 -w0 > "$out/release.toml.sig"
printf '%s\n' "$out"
