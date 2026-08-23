# RecentlyDivorced

codex resume previews the last human ask.

```text
RECENTLYDIVORCED
──────────────────────────────────────────────────────────────

user prompt          ──►  stock codex hook  ──►  threads.preview
                                                       │
                                                       └─ /resume row
```

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

## what it does

Codex runs the plugin after each submitted user prompt. The plugin updates that
thread's `preview` value with the submitted prompt. `/resume` reads
`preview`, so it shows the latest prompt instead of the original thread
prompt.

It does not modify the Codex executable, transcript, title, first prompt,
model settings, caches, auth, tool calls, or token use.

## existing threads

install reads each existing thread rollout once and sets its preview to the
last user prompt. New prompts use the hook.

Codex updates keep the installed plugin. Uninstall removes the plugin,
marketplace, and hook trust record.

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
