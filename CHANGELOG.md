# Changes

## 0.3.13

- Defer generic-only openings until normal conversation or optional GraphFather context identifies work.
- Include stored project context without a repository scan; prioritize the current task over old background.
- Preserve unknown and externally renamed titles. Add explicit per-thread `--auto` and `--pin` controls.
- Reject model results when observed activity, ownership, context, or the stored name has changed.
- Stop replaying cached labels over manual names. Stop replacing transcript previews with new labels.
- Cache uncertain results without publishing blank names or retrying unchanged evidence.
- Exclude child/internal activity events, including events that carry a parent session ID.
- Read one bounded rollout tail and accept valid JSON with ordinary whitespace.
- Preserve legacy label ownership on cache migration. Restore only still-owned labels on uninstall.
- Run the isolated lifecycle proof in CI and against the release binary.

The local conditional database write is not a substitute for native Codex title-origin,
revision, and notification support. The shipped platform remains Linux x86_64.

Related acceptance case: [a3mentia's same-opening, same-project example](https://github.com/openai/codex/issues/14044#issuecomment-5854939866).
