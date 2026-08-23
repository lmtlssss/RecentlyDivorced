# RecentlyDivorced

Codex `/resume` should show the last thing you asked, not the prompt that
started the thread.

> bro nobody with adhd remembers wtf is what that way

```text
first prompt  -> provenance and title
latest prompt -> /resume preview
```

RecentlyDivorced is a small stock-Codex plugin. It does not replace, fork, wrap,
or rebuild Codex.

## install

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/install.sh | bash
```

Then use `codex` exactly as normal. Codex owns the plugin installation and
keeps it when Codex updates.

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | bash
```

That removes the plugin and its marketplace. Stock Codex was never modified.

## the whole primitive

```text
Codex UserPromptSubmit hook
  -> session_id + submitted human prompt
  -> UPDATE threads SET preview = prompt WHERE id = session_id
  -> /resume shows the last prompt
```

The hook is asynchronous. It does not block a prompt, scan transcripts, change
the conversation, alter a title, change model or prompt caches, touch auth, or
adjust token use. Its only write is the existing thread `preview` metadata
field that `/resume` already reads.

## why this survives updates

RecentlyDivorced uses Codex’s public plugin marketplace and lifecycle-hook
system. The helper is a tiny Rust binary downloaded by the curl installer; the
Codex executable stays stock. Updates preserve the configured plugin, so the
same hook continues to run.

## scope

- Linux CLI, x86_64 GNU/Linux.
- Normal Codex plugin install and removal.
- No macOS, Windows, or desktop app surface.

The upstream clone in `references/` was used to verify the hook contract and
the stock SQLite schema. It is not a forked runtime.
