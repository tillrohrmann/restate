# Deep review skill

## Start a review

- **OpenCode:** `Use restate-deep-review to review this checkout against <base-ref>.`
- **Codex:** `$restate-deep-review Review this checkout against <base-ref>.`
- **Claude Code:** `/restate-deep-review Review this checkout against <base-ref>.`

Omit the base to use the open PR target, or else `main` of the `restatedev/restate`
remote; the lead asks only if neither resolves. The review covers the live checkout,
including staged, unstaged, and non-ignored untracked files, so leave it unchanged
until the review finishes. Results go to a unique directory under `target/deep-review/`.

Requirements: native subagents, Git, write access to `target/`, and `gh` (only for PR
lookup). These are workflow instructions, not a sandbox; runtime permissions still
apply. Reviewers inherit the lead's model, so check for configured subagent model
overrides before a run.

## How it works

The lead traces the change once into shared context (callers, callees, data readers,
execution contexts, and claims), selects roles (R2 always; others unless their
Applicability line plainly does not match), and launches one independent reviewer
per role. Each reviewer writes its own phase file and returns a one-line-per-candidate
summary. In round 1 each reviewer critiques two assigned peers, and only challenged
authors respond; round 2 runs only while candidates or challenges remain open.
The lead adjudicates by evidence and writes `report.md`.

Both review skills share [`.agents/review/perspectives.md`](../../review/perspectives.md):
the context map, R1-R9 methods, Restate execution contexts, and anti-patterns. The deep
review writes the map to `context.md`; the local review keeps a proportional one in its
working notes. Edit it to improve both
skills. Skills refer to it by repository-root path because relative paths through
the `.claude/skills` symlinks would not resolve to `.agents/review/`.

## Runtime setup

OpenCode and Codex discover `.agents/skills/<name>/SKILL.md`:

- [OpenCode skill discovery](https://opencode.ai/docs/skills/#place-files)
- [Codex skill discovery and invocation policy](https://developers.openai.com/codex/build-skills)
- [OpenCode subagents and model inheritance](https://opencode.ai/docs/agents/#model)
- [Codex subagents](https://developers.openai.com/codex/agent-configuration/subagents)

Claude Code discovers the skills through the symlinks in `.claude/skills/`:

- [Claude Code skill discovery and symlink support](https://code.claude.com/docs/en/skills#where-skills-live)
- [Claude Code subagents](https://code.claude.com/docs/en/sub-agents)

Invocation is explicit-only: Codex metadata (`agents/openai.yaml`), Claude frontmatter
(`disable-model-invocation`), and the `AGENTS.md` note. Restart the client if a skill
is not visible (OpenCode always needs a restart after installation).

## Pilot validation

Check discovery first: `opencode debug skill` in OpenCode, `/skills` in Codex, and the
`/` menu in Claude Code. Then exercise these scenarios in a disposable checkout:

| Scenario | Expected behavior |
| --- | --- |
| Ordinary "review my changes" request | Does not activate either skill. |
| No explicit base, no PR | Uses `main` of the `restatedev/restate` remote; asks only if that is missing. |
| Committed, staged, unstaged, and untracked changes | Inventories every layer, excludes ignored files, reviews the resulting tree. |
| Clean checkout with commits since the merge base | Reviews the commits rather than reporting no changes. |
| No changes in any layer | Writes a no-changes report without launching reviewers. |
| Docs- or config-only change | Skips inapplicable roles with recorded evidence. |
| Runtime capacity below the panel size | Queues roles and still completes every phase barrier. |
| A reviewer fails twice | Records both attempts; the report is partial and names the gap. |
| No open items after round 1 | Skips round 2 and records the skip. |
| Checkout changes during review | Reports scope drift and marks the run partial. |

After a full run, check the ledger in `run.md`, that reviewers wrote only their own
files, and that source and Git state are unchanged. Compare initial reports with the
final report to judge whether debate improved the result. Record usefulness, false
positives, unique findings, elapsed time, and usage; a well-formed report alone does
not establish review quality.
