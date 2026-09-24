# Local review skill

Single-session review of local changes. See `../restate-deep-review/README.md` for
the multi-agent variant and runtime discovery details.

- **OpenCode:** `Use restate-local-review to review this checkout against <base-ref>.`
- **Codex:** `$restate-local-review Review this checkout against <base-ref>.`
- **Claude Code:** `/restate-local-review Review this checkout against <base-ref>.`

Omit the base to use the open PR target, or else `main` of the `restatedev/restate`
remote. Name another scope (a commit, range, or staged changes) to narrow the review.

The investigation methods are shared with the deep review in
[`.agents/review/perspectives.md`](../../review/perspectives.md). Invocation is
explicit-only (Codex metadata, Claude frontmatter, and `AGENTS.md`). These are
workflow instructions, not a sandbox; runtime permissions still apply.
