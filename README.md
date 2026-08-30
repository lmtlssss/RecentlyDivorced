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

the curl installer automatically crawls every available rollout and labels it
with `gpt-5.6-sol` at low reasoning. later Codex launches automatically use
`gpt-5.3-codex-spark` only for rollouts whose file fingerprint changed.

the first pass batches up to 100 conversations, then permanently marks the
bootstrap complete. after that, every new or changed conversation uses Spark
in batches up to six. Sol is not used for maintenance.

each conversation sends at most 480 characters. the latest persisted Codex
context summary is preferred; chats without one use the first ask, prior
label, and latest three semantic turns. labels are at most 12 words.

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
