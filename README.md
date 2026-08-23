# RecentlyDivorced

codex resume previews the last human ask.

bro nobody with adhd remembers wtf is what that way.

```text
RECENTLYDIVORCED
──────────────────────────────────────────────────────────────

user prompt          ──►  stock codex hook  ──►  threads.preview
                                                       │
                                                       └─ /resume row
```

first prompt stays provenance.
title stays title.
conversation stays untouched.

## install

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/install.sh | sh
```

inspect first:

```bash
curl -fsSLO https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/install.sh
less install.sh
sh install.sh
```

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | sh
```

## behavior

```text
stock codex
──────────────────────────────────────────────────────────────

user prompt           hook input
session id            active thread id
prompt                latest human ask
```

```text
plugin write
──────────────────────────────────────────────────────────────

UPDATE threads
SET preview = prompt
WHERE id = session_id
```

one field.
one row.
no transcript rewrite.

## existing threads

install runs one backfill:

```text
thread rollout        ──►  last user input  ──►  preview
```

after that, normal prompts use the live hook.

## stock behavior

```text
not touched
──────────────────────────────────────────────────────────────

codex executable       model cache        prompt cache
session cache          auth               plugins
conversation           title              first prompt
token usage            tool calls         model selection
```

RecentlyDivorced is a stock Codex plugin.
codex updates keep the plugin.
uninstall removes the plugin, marketplace, and hook trust record.

## release files

```text
file                                            use
──────────────────────────────────────────────────────────────
recentlydivorced-x86_64-unknown-linux-gnu      hook helper
```

## build

```bash
cargo test --locked --manifest-path plugins/recentlydivorced/runtime/Cargo.toml
```
