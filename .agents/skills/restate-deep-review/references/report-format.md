# Report format

## Artifacts

```text
target/deep-review/<run-id>/
  run.md                      lead
  context.md                  lead
  candidates.md               lead; candidate index updated after every phase
  initial/R#.md               reviewer
  round-1/critiques/R#.md     reviewer
  round-1/responses/R#.md     reviewer
  round-2/...                 reviewer; only if round 2 runs
  adjudication.md             lead
  report.md                   lead
```

Each reviewer writes only its assigned file. For a failed phase, the lead writes that
file with the error and both attempts.

`run.md` records start/end time, repository root, branch, HEAD, the base and how it
was chosen, merge base, PR URL, model and any subagent override, selected and skipped
roles with reasons, whether round 2 ran, initial/final checkout status, and measured
time/usage (`unavailable` if not exposed). Its ledger has one row per role and one
column per phase; cells are `complete`, `failed` (with attempts), `n/a`, or `pending`,
with the agent ID when available.

Status:

- **Complete:** every required phase finished and no scope drift was observed. This
  describes the process, not proof of correctness.
- **Partial:** a phase failed, the run was interrupted, or scope drift was observed.
  Name the gaps prominently.
- **Blocked:** the panel could not start (missing base, history, subagents, or write
  permission).
- **No changes:** every change layer was empty.

## Reviewer output

The report file names the role, phase, inputs inspected, claims tested that held (with
the guard or argument), and coverage gaps. No
findings is valid; an empty report is not. Initial and new candidates use:

```text
<R#-seq> · <P0-P3> · <category> — <title>
Evidence: <path:line and reachable trigger, with relevant callers/guards>
Impact: <consequence; how the change causes or worsens it, or why it predates it>
Action: <fix direction, alternative with trade-offs, or specific verification>
Uncertainty/counterevidence: <only when material>
```

Categories: change-induced, pre-existing, design concern, validation gap. Severity is
impact, not confidence: P0 demonstrated critical release-blocking impact; P1 high
impact to fix before merge; P2 material localized concern; P3 lower-impact substantive
concern. Design concerns need a concrete consequence; validation gaps need a specific
uncovered scenario; unsupported suspicions stay questions. Cite real source lines
(label base-commit and untracked evidence), not diff hunk offsets. Keep IDs stable,
update only your own candidates, and propose changes to others as challenges.

The reply to the lead is only the output path followed by one line per candidate or
entry (`ID · severity · target ID if any · title`), or "no findings".

## Adjudication record

For each candidate: disposition (`retained`, `merged`, `rejected`, `unresolved`),
canonical ID if merged, category, severity, and the evidence-based reason. Note
withdrawals, unanswered consequential challenges, and claims never critiqued.

## Final report

`report.md` has these sections; write `None identified` for an empty one:

1. **Summary:** status, scope/base/HEAD, key conclusions, and limitations.
2. **Change-induced findings:** full final records, ordered by impact.
3. **Pre-existing problems**
4. **Design concerns**
5. **Validation gaps and open questions:** competing evidence and the verification needed.
6. **Coverage:** paths inspected, skipped roles and areas, assumptions, and a ledger link.
   No validation commands were run.
7. **Debate outcome:** material corrections and withdrawals, remaining disagreements,
   and a link to `adjudication.md`.

Present each concern once, at its final severity. The report is advisory.
