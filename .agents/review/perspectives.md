# Review perspectives

Investigation methods shared by both review skills: a context map traced once per
review, then the R1-R9 perspectives, which start from the map and extend it rather than
re-tracing. The invoking skill decides scope, depth, and output. Treat each bullet as a
question to verify in source, not an assumed property of the code.

## Context map

Trace the change before applying any perspective, citing `path:line` labeled base or
working tree:

- **Intent and inventory:** stated intent with its source, and the changed files.
- **Per changed function or entry point:** what changed; callers traced 3-5 levels up
  to a terminal caller (API handler, state-machine apply, background task), naming the
  branches that depend on changed return values or state; callees with what they
  return, what shared state they mutate, and their preconditions; the sibling or
  reference implementation and where the change deviates from it.
- **Data consumers:** every value the change writes, persists, or sends (state
  tables, log records, messages, metadata, metrics) and each reader found, with the
  ordering, visibility, lifetime, and error-handling rules that reader applies.
- **Execution contexts:** for each row of the [table](#restate-execution-contexts),
  whether the changed path is reached (yes/no/unknown, with citation).
- **Invariants and claims:** invariants established by code, comments, assertions,
  and tests; claims the change relies on ("safe because …", "cannot happen") with the
  preconditions each needs.
- **Related code:** existing helpers, conventions, and tests covering the changed
  behavior; CI results inspected (with revision).
- **Gaps:** explicit unknowns, and which lists are exhaustive versus sampled.

## R1 — Design and approach

**Objective:** determine whether the approach solves the stated problem at the right layer.

- Read the requirements and commit rationale; separate intent from observed behavior.
- Compare a sibling implementation or existing helper. Identify which layer owns
  policy, mechanism, and state; check whether the change crosses those boundaries.
- Consider a simpler alternative. Report a design concern only with a concrete
  correctness, operational, or maintenance consequence and the alternative's trade-offs.

**Applicability:** assess briefly for every change; deepen for new abstractions or ownership changes.

## R2 — Correctness and failure paths

**Objective:** find reachable paths that produce incorrect behavior.

- Trace changed decisions through their callers to an observable result. Check
  empty/boundary inputs, overflow, and security checks at affected trust boundaries.
- For important callees, inspect both returned values and mutations: counters,
  ownership transfers, notifications, persistence, and cleanup. Verify preconditions.
- Interrupt the path at awaits, errors, timeouts, cancellation, and partial failure.
  Determine what remains committed, released, retried, or duplicated; check recovery.

**Applicability:** behavioral changes, including refactors that alter execution order or lifetime.

## R3 — Cross-component analysis

**Objective:** detect incompatible assumptions between producers and consumers.

- List the data, events, and shared state the change produces or mutates. Find their
  readers, including background tasks and persisted-data consumers outside the call graph.
- Compare writer assumptions with each relevant reader's ordering, visibility,
  lifetime, and error-handling rules. Follow the mismatch to its consequence.
- Check which [Restate execution contexts](#restate-execution-contexts) reach the
  changed path and whether the writer's assumptions still hold in each.

**Applicability:** shared state or behavior crossing component/lifecycle boundaries.

## R4 — Invariant adversary

**Objective:** break the feature's claims and the system invariants it touches, rather
than confirm its implementation.

- Work from intent, not code: from the requirements, commit message, and component
  contracts, write down what must hold before relying on the implementation's
  explanation; consult it afterwards only to find guards. This keeps its explanation
  from framing yours.
- Collect claims precisely: design claims ("redundant", "no-op", "safe because",
  "cannot happen", "idempotent", "at most once"), `debug_assert!`/`assert!`,
  `unreachable!`, `expect` messages, and `// SAFETY:` comments. Add the system
  invariants from the execution-contexts table that the change touches, such as
  identical state on every replica, determinism on replay, and fenced leadership.
- For each claim, list its preconditions and construct a concrete counterexample:
  input values, prior state, and an ordered sequence of steps. Levers: crash or restart
  between two writes, replaying the same log entry, leadership change at an `.await`,
  duplicated or reordered messages, restore from snapshot, a mixed-version peer,
  extreme configuration, and concurrent tasks on shared state.
- Trace each counterexample through callers and guards to its release-build
  consequence: a skipped `debug_assert!`, a panic (which recurs if the path is
  replayed), silent corruption, or lost progress. Distinguish expected rejection from
  a reachable violation, and safety failures from loss of progress. If a claim
  survives, record the guard or argument that makes it hold.

**Applicability:** state transitions, concurrency, recovery, or changes justified by invariants.

## R5 — Caller-context audit

**Objective:** find atypical but reachable inputs that the changed implementation overlooks.

- Identify changed entry points. Search direct callers, wrappers, constructors,
  trait implementations, and dispatch sites; do not stop at the first use.
- Determine the values and execution context each relevant caller can supply,
  including extremes, missing values, configuration combinations, and resources held.
- Compare those inputs with the implementation's preconditions. Trace changed return
  values/errors upward to the decision that relies on them. State search gaps for
  unresolved indirect callers rather than claiming exhaustive coverage.

**Applicability:** changed interfaces, preconditions, return semantics, or calling contexts.

## R6 — Performance

**Objective:** identify concrete latency, throughput, or resource regressions.

- Establish whether the path is hot and what input size, concurrency, or retry rate
  multiplies its work. Compare old/new allocation, copying, I/O, and algorithmic cost.
- Follow lock/resource lifetimes and blocking calls across awaits, including references
  from `Configuration::pinned()`. Inspect queues and buffers for growth, backpressure,
  and cleanup under slow consumers. `AGENTS.md` lists the latency-critical components.
- Explain the workload and cost mechanism; use existing measurements when available,
  never invented numbers. Prefer a simpler efficient alternative, with correctness first.

**Applicability:** changed work, data structures, resource lifetimes, or scheduling.

## R7 — API and compatibility

**Objective:** expose unintended changes to existing users' observable behavior.

- Compare old/new behavior for previously accepted inputs, including undocumented
  or ignored values, defaults, errors, and resource impact. Intentional change alone
  is not a defect.
- Find dependent callers, stored configuration, and mixed-version participants.
  Trace upgrade, downgrade, and rollout requirements where supported by source.
- Check rationale, documentation, regression coverage, and required user action.
  Behavior changes need a note per `release-notes/README.md`; CLI changes follow
  `crates/cli-util/README.md`; new or deprecated config options need `/// Since vX.Y.Z`.

**Applicability:** externally visible interfaces, configuration, behavior, or version boundaries.

## R8 — Serialization/deserialization

**Objective:** verify that readers and writers agree across versions and malformed input.

- Follow changed values to their encoders, storage/transport, and decoders—even
  when the diff does not touch a format definition.
- Compare old-reader/new-writer and new-reader/old-writer behavior: field tags,
  discriminants, defaults, unknown/missing fields, and migration/replay semantics.
- Check lengths, bounds, integer arithmetic, truncation, and invalid input before
  allocation or state mutation. Inspect fixtures for compatibility, not only self round-trips.

**Applicability:** changes affecting persisted or transmitted data or its interpretation.

## R9 — Test coverage

**Objective:** identify important changed contracts that current tests would not protect.

- Read assertions and fixtures, not just test names. Map them to changed behavior
  and the caller/consumer interactions they actually exercise.
- For a candidate failure, explain why an existing test would miss it. Look for
  specific boundary, cancellation, concurrency, recovery, and compatibility gaps.
- Check whether synchronization and assertions reliably detect the problem. Suggest
  the smallest useful scenario and expected outcome; inspect supplied CI revisions
  without treating unrelated passing CI as proof.

**Applicability:** changed contracts and their regression protection; missing tests are not proof of a bug.

## Restate execution contexts

The same code can run under different assumptions. For each context below, confirm
from source whether the changed path is reached before reasoning about it.

| Context | Where to look | Typical failure |
| --- | --- | --- |
| Leader vs follower apply | `is_leader` in `crates/worker/src/partition/state_machine/mod.rs`; `State` in `crates/worker/src/partition/leadership/mod.rs` | State or side effects diverge between replicas |
| Log replay and restart | State-machine apply path; append-time handling in `crates/worker/src/partition/state_machine/entries/mod.rs` | Non-determinism from wall-clock, randomness, iteration order, or config read during apply |
| Leadership change | `crates/worker/src/partition/leadership/fencing.rs`, `crates/worker/src/partition/leadership/leader_state.rs` | Stale leader acts; in-flight work lost or duplicated on step-down |
| Snapshot restore and log trim | `crates/partition-store/src/snapshots.rs`, `crates/worker/src/partition/leadership/trim_queue.rs` | Restored state lacks data the change assumes; required log prefix trimmed |
| Mixed-version cluster (upgrade/downgrade) | `crates/types/src/partitions/features.rs`, `crates/types/src/restate_version.rs` | Old node reads a new command/field; feature enabled before all nodes support it |
| Data written by older versions | `crates/partition-store/src/migrations/`, `crates/worker/src/partition/state_machine/lifecycle/migrate_journal_table.rs` | Old storage layout (e.g. journal v1 vs v2) handled incorrectly |
| Loglet providers | `crates/bifrost/src/providers/{local_loglet,replicated_loglet}` | Seal/trim/gap behavior assumed for one provider only |
| Experimental flags | `experimental!` in `crates/types/src/config/common.rs` | Path breaks with a flag on/off, or when a flag is toggled on existing data |

## Anti-patterns

| Anti-pattern | Instead |
| --- | --- |
| Return-value tunnel vision | Trace what callees mutate, not only what they return (R2). |
| Default-configuration bias | Walk the execution-contexts table (R3). |
| Assert-as-proof | Try to construct an input that fires the assertion (R4). |
| Write-path-only analysis | Find every reader of data the change writes (R3). |
| Confirmation-seeking | State the claim first, then try to falsify it (R4). |
| Callers vs readers confusion | Who calls the code (R5) differs from who reads its data (R3). |

## Sources

Adapted for Restate from RocksDB's [investigation procedures and reviewer roles](https://github.com/facebook/rocksdb/blob/662cdbc9ec3f608043b77ccc40616465457d41e6/claude_md/ci_review_prompt.md#L72-L437)
and [proportional local-review approach](https://github.com/facebook/rocksdb/blob/662cdbc9ec3f608043b77ccc40616465457d41e6/claude_md/manual_code_review.md#L26-L52).
