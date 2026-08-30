# RecentlyDivorced

labels every `/resume` row with a tiny description of the conversation.

```text
RECENTLYDIVORCED
──────────────────────────────────────────────────────────────

codex starts  ──►  changed conversations  ──►  one-line labels
                                                   │
                                                   └─ /resume
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

## usage

the first install labels the available archive with `gpt-5.6-sol` at low
reasoning. later Codex launches use `gpt-5.3-codex-spark` only for
conversations whose transcript changed.

requests are batched up to ten conversations. each conversation sends a
bounded capsule: its first ask, prior label, and recent turns. labels are at
most 12 words.

this consumes Codex model usage. the first pass scales with archive size;
later usage scales with changed conversations. conversation excerpts are sent
to OpenAI through the installed Codex CLI.

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | sh
```

uninstall restores stock previews and removes the plugin, marketplace, hook
trust, helper, and summary index.

## build

```bash
cargo test --locked --manifest-path plugins/recentlydivorced/runtime/Cargo.toml
```
