# RecentlyDivorced

labels every top-level human CLI conversation.

```text
pending activity ─────────┐
stock / CompactVeteran     ├─► 1500-char capsule ─► 12-word label ─► /resume
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
2. optional read-only GraphFather objective + Cursor + Next action;
3. young first ask + latest six semantic turns.

legacy and paginated top-level human CLI rows are covered. subagents, exec,
fork, and internal threads are excluded. local rollout reading stops at 64 KiB;
conversation evidence stops at 1500 characters.

the first pass batches with Sol at low reasoning. changed-only maintenance uses
Spark. one-line labels stay short.

Meaningful activity is queued locally on prompt submit. Labels refresh
asynchronously after a response, so typing never replaces a good title with a
raw prompt fragment.

To inspect labels without changing the cache or production thread titles, pass
one or more IDs to the read-only preview command:

```bash
recentlydivorced --preview THREAD_ID [THREAD_ID ...]
```

The preview combines the optional GraphFather context with recent conversation
evidence and returns JSON labels and capsules.

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | sh
```

## build

```bash
cargo test --locked --manifest-path plugins/recentlydivorced/runtime/Cargo.toml
```
