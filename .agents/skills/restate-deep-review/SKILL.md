---
name: restate-deep-review
disable-model-invocation: true
description: Multi-agent, inspection-only review of local changes with specialist reviewers, critique-and-response debate, and evidence-based adjudication. Explicit invocation only.
---

# Restate deep review

Act as the lead of a panel of specialist reviewers (R1-R9) and adjudicate their evidence.

## Contract

- Inspection only: no one modifies source or Git state, runs builds/tests/benchmarks
  or project code, applies fixes, or posts comments. Writes stay inside the run
  directory under `target/deep-review/`; each reviewer writes only its assigned file.
- Scope: the live checkout against its merge base, including committed, staged,
  unstaged, and non-ignored untracked changes.
- Every run starts fresh: new run directory and new reviewer contexts.
- Evidence decides findings, not votes. Separate change-induced defects from
  pre-existing problems, design concerns, and validation gaps. No cosmetic nits.
- Without subagent support, stop: a single-agent pass is not a deep review.

## Procedure

Load each reference when its step starts. Specialists get only their brief, never
these documents.

1. [Methodology](references/methodology.md) §1-3: resolve scope, create the run
   directory, write `context.md`, and select roles.
2. [Reviewer roles](references/reviewer-roles.md), [Report format](references/report-format.md),
   and methodology §4: brief and launch the independent initial reviews.
3. [Debate](references/debate.md): critique and response rounds.
4. Methodology §5: adjudicate, write `report.md`, and summarize in chat with links.

Retry a failed role/phase once, then continue and mark the run partial. If
interrupted, keep completed artifacts and report partial results.
