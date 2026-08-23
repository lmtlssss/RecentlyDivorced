# upstream

```text
UPSTREAM       https://github.com/openai/codex
PIN            83d1fe0e67b1323f71febc2925817732b449f1d9
OWNER          codex-rs/thread-store/src/thread_metadata_sync.rs
SURFACE        /resume and codex resume
```

RecentlyDivorced is a narrow upstream patch. It does not rewrite session JSONL,
replace the Codex binary in place, or change first-user-message provenance.

The patch updates only the mutable discovery preview when Codex observes a
non-empty human message. The first user message and derived title keep their
current meaning.

Compatibility fails closed. The patch applies only after `git apply --check`
accepts the checked-out upstream tree. A new upstream release requires a fresh
pin, a reviewed patch, and targeted tests.
