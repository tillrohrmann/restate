# Methodology

## 1. Establish the run

Run from the repository root. Record the branch, `HEAD`, and
`git status --short --untracked-files=all`. Ask the user to leave the checkout
unchanged until the review ends (a notice, not a gate). Do not snapshot, stash,
commit, or create worktrees.

Base, in order: an explicit base; the current branch's open PR target (`gh`; record
the PR URL and head); `main` on the remote whose URL points at `restatedev/restate`
(not necessarily `origin`). Resolve refs to commit IDs and require a single
`git merge-base --all` result, which is the comparison boundary. Ask only if none
resolves or history is missing; do not fetch or switch branches.

Create `target/deep-review/<UTC-timestamp>-<random-suffix>/` and initialize `run.md`
([Report format](report-format.md)).

## 2. Inventory the change

List committed, staged, and unstaged changes with name/status summaries, and untracked
files with `git ls-files --others --exclude-standard`. Read the net patch
`git diff --no-ext-diff <merge-base> --`; open per-layer patches only to resolve
differences. Review the resulting working tree: a defect removed by a later local
layer is not a finding. Record renames, deletions, binaries, and submodules; report
unreadable content as a gap. Exclude ignored files and run artifacts.

If no layer has changes, write a no-changes report and stop. For large changes,
inventory everything before working in batches.

## 3. Build shared context and select roles

Read `.agents/review/perspectives.md` from the repository root (not relative to the
skill's path). Trace the change once here so reviewers start from a shared map instead
of each rediscovering it: read the changed code, its subsystem, tests, and analogous
implementations, then write the map described in the **Context map** section of
`perspectives.md` to `context.md`, including the full inventory.

Trace exhaustively for changed public or trait entry points and persisted or
transmitted data; elsewhere sample and state the gap. Keep it factual: record what the
code does and who depends on it, but do not judge whether preconditions hold, suspect
defects, or assign severities. Reviewers verify what their findings depend on and may
challenge the context.

R2 always runs. Skip another role only when its **Applicability** line plainly does
not match the inventory (for example, R8 when no persisted or transmitted value changes,
even indirectly), and record the evidence in `run.md`. When in doubt, run it.

## 4. Delegate

Use one fresh context per selected role, briefed per [Reviewer roles](reviewer-roles.md).
Do not fork the lead's conversation, preload this skill, or request worktree isolation.
Do not pass a model override; reviewers inherit the lead's model. Record the model in
`run.md`, noting any configured subagent override (e.g. `CLAUDE_CODE_SUBAGENT_MODEL`).

- **Claude Code:** Agent tool with the `general-purpose` type; continue a reviewer via SendMessage.
- **Codex:** native spawn, wait, and follow-up tools with fresh contexts.
- **OpenCode:** Task tool with the `general` subagent; keep task IDs for follow-ups.

Launch concurrently up to runtime capacity and queue the rest; deferral is not failure.
Reviewers never spawn agents or talk to each other. Each writes its full report to its
assigned file and replies with a short summary; open the files only when you need
detail. Continue a reviewer across phases when the runtime allows; otherwise rebuild
it from its brief and its own earlier files.

A phase fails when the reviewer errors or its file is missing or unusable. Retry once
with the same inputs; after a second failure, record the gap and continue.

## 5. Adjudicate and publish

Reconstruct each candidate's final state from `candidates.md` and the phase files.
Verify retained claims yourself against source, callers, guards, and base behavior.
Explain how the change introduces, exposes, or worsens each defect, or label it
pre-existing or uncertain. Deduplicate by root cause while keeping distinct triggers.
Record every disposition with evidence in `adjudication.md`; vote counts are not
evidence. Consequential unresolved disagreements become open questions.

Recheck `HEAD`, status, and the inventory. If they changed, mark the run partial due
to scope drift and name the affected evidence.

Write `report.md`, finish `run.md`, and reply with a concise summary linking
`report.md`, `run.md`, and `adjudication.md`.
