# RecentlyDivorced

labels every top-level human CLI conversation.

```text
stock compaction summary ─┐
CompactVeteran handoff    ├─► 480-char capsule ─► 12-word label ─► /resume
young chat ───────────────┘
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

the source ladder is:

1. stock compaction summary, plus the latest user turn;
2. CompactVeteran Objective + Cursor + Next action;
3. young first ask + latest three semantic turns.

legacy and paginated top-level human CLI rows are covered. subagents, exec,
fork, and internal threads are excluded. local rollout reading stops at 64 KiB;
model input stops at 480 characters.

the first pass batches with Sol at low reasoning. changed-only maintenance uses
Spark. one-line labels stay short.

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | sh
```

## build

```bash
cargo test --locked --manifest-path plugins/recentlydivorced/runtime/Cargo.toml
```
