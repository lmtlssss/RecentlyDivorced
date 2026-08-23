# RecentlyDivorced

codex resume previews the last human ask.

```text
RECENTLY DIVORCED
──────────────────────────────────────────────────────────────

old picker       first prompt  ──► stale thread identity
new picker       last prompt   ──► the thing you mean now

first prompt     ──► preserved provenance
thread title     ──► preserved title behavior
conversation     ──► never rewritten
```

## install

this is a pinned, reviewable upstream patch kit.

```bash
git clone https://github.com/lmtlssss/RecentlyDivorced.git
cd RecentlyDivorced
```

inspect first:

```bash
less UPSTREAM.md patches/latest-user-prompt-preview.patch patches/replay-and-fallback-preview.patch scripts/apply-patch.sh
```

## apply

clone the exact upstream pin, then apply the patch.

```bash
git clone https://github.com/openai/codex.git codex
cd codex
git checkout 83d1fe0e67b1323f71febc2925817732b449f1d9
cd ..
scripts/apply-patch.sh codex
scripts/verify.sh codex
```

## behavior

```text
USER PROMPT A  ──► first_user_message  ──► provenance and title
USER PROMPT B  ──► preview             ──► /resume row
USER PROMPT C  ──► preview             ──► /resume row
```

only non-empty human messages change `preview`. System messages, tool output,
assistant output, goals, and internal agent traffic do not replace a later human
ask.

```text
NOT TOUCHED
──────────────────────────────────────────────────────────────

model cache        prompt cache        session cache
plugin cache       auth state          conversation transcript
tool calls         model selection     token usage
```

RecentlyDivorced changes the thread-discovery `preview` metadata only.

## update gate

```text
upstream release
      │
      ▼
git apply --check
      │
      ├─ passes  ──► targeted test ──► build candidate
      └─ fails   ──► stop; review and repin
```

no binary patching. no session-log rewrite. no silent apply to a new Codex
release. `UPSTREAM.md` is the release boundary.

## verify

```bash
scripts/verify.sh codex
```

the targeted test proves that a later user message refreshes the resume preview
while the first prompt remains unchanged.

## build

after verification, build Codex from the patched upstream checkout using its
normal release process. RecentlyDivorced intentionally does not replace an
installed Codex binary in place.
