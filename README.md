# RecentlyDivorced

Codex `/resume` should orient you with the last thing you asked, not the prompt
that started a twenty-day-long thread.

> bro nobody with adhd remembers wtf is what that way

```text
first prompt  -> provenance and title
latest prompt -> /resume preview
```

RecentlyDivorced is a Linux CLI-only Codex patch and release manager. It changes
only thread preview metadata. It does not rewrite transcripts or touch model,
prompt, session, plugin, auth, or token caches.

## install

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/install.sh | bash
```

Then use `codex` normally. There are no wrapper commands to remember.

## uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/lmtlssss/RecentlyDivorced/main/uninstall.sh | bash
```

That returns the public `codex` path to its exact stock target (or removes the
PATH shadow created by the installer). Codex history and caches stay untouched.

## what happens

```text
curl install
  -> discover stock Codex
  -> download a signed, exact-version payload
  -> verify signature, hash, and codex --version
  -> atomically claim the public codex link

normal codex
  -> local marker/current lookup
  -> exec patched Codex with the original argv/env/fds

codex update
  -> run stock update under one lock
  -> fetch only an exact signed payload for that stock version + target
  -> verify and atomically promote it
```

Normal `codex` invocations make no network request and do not modify Codex
state. If no matching release exists after an upstream update, the update exits
nonzero and keeps the last known-good patched payload; it never lies that the
active patched Codex updated.

## scope

- Linux CLI, x86_64 only for the first release.
- Portable musl payloads; no systemd requirement.
- Systemd may later be offered only as optional protection against external
  stock-link drift. `codex update` is the portable core.
- macOS, Windows, and the desktop app are deliberately out of scope.

## upstream patch

The patch is pinned in [upstream.lock](upstream.lock). It updates the stored
`preview` only when a non-empty human message arrives. The initial human message
and title behavior remain intact.

The legacy transcript fallback stays bounded. RecentlyDivorced does not turn
`/resume` into a full scan of every old conversation just to chase a prettier
row.

## release boundary

Each release contains an authenticated manifest, detached Ed25519 signature,
manager binary, and patched Codex payload. The embedded public key verifies the
manifest before installation or promotion. Payloads must match:

```text
stock Codex version + Linux target + SHA-256
```

Build and test the exact upstream pin before publishing any new payload:

```bash
scripts/apply-patch.sh codex
scripts/verify.sh codex
```

See [UPSTREAM.md](UPSTREAM.md) for the source boundary and [NOTICE](NOTICE) for
upstream attribution.
