# Debate protocol

## Initial reviews

Each selected role reviews independently. No reviewer sees peer output until every
initial report is saved or has failed twice. From the returned summaries, create
`candidates.md`: one row per candidate with ID (`R2-001`), author, severity,
category, title, status (`open`), and file path.

## Round 1: critique

Each reviewer critiques its two assigned peers in full and scans `candidates.md`
for anything else within its remit. Every role is critiqued by two peers; the pairs
link roles that tend to see the same bug from different angles. If a peer was skipped
or failed, reassign that slot to another selected role.

| Critic | R1 | R2 | R3 | R4 | R5 | R6 | R7 | R8 | R9 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Peers | R3, R6 | R4, R8 | R2, R5 | R2, R3 | R4, R9 | R1, R7 | R6, R8 | R5, R9 | R1, R7 |

For each entry, give an ID (`R4-C1-001`), the target candidate ID, and `path:line`
evidence. An entry is one of:

- **Challenge:** wrong assumption, missing causal link, existing guard, unreachable
  path, wrong severity, or pre-existing behavior labeled as introduced.
- **Corroboration:** only with independent evidence or a distinct trigger.
- **Duplicate:** same root cause as another candidate.

New candidates found while critiquing get full records and the critic's ID prefix.
If there is nothing substantive to challenge, say so; never manufacture objections.
Add every entry's ID and target to `candidates.md`.

## Round 1: response

Only authors whose candidates were challenged respond; mark the others `n/a`. Give
each author the critique files that target its candidates. For every challenge, cite
its ID, add evidence, and choose **retain**, **revise**, **withdraw**, or
**unresolved** with a reason. Return only changes by candidate ID. Unchanged
candidates carry forward; a missing response never implies withdrawal. If an author
failed, keep its last candidate state and leave the challenge to the lead.
Update statuses in `candidates.md`.

## Round 2

Run round 2 only if round 1 left revised, new, or unresolved candidates or unanswered
challenges; otherwise record the skip in `run.md`. Use the same assignment, but
critics examine only the open candidates listed in `candidates.md` and open earlier
files only as needed. Responses follow the round-1 rules and state final positions.

## Closing rules

Claims first raised in round-2 responses were never critiqued; verify them yourself
and label them as such. There is no third round. Keep all phase files intact.
Broadcast corrections to shared facts to every reviewer and record them as amendments
in `context.md`; never edit earlier reports.
