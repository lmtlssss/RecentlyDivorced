# RecentlyDivorced

shows the most recent prompt in `/resume`.

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

## behavior

new prompts update the active thread preview through the Codex hook.

Codex updates keep the installed plugin. Uninstall removes the plugin,
marketplace, and hook trust record.

## build

```bash
cargo test --locked --manifest-path plugins/recentlydivorced/runtime/Cargo.toml
```
