# Partition processor state machine specification

This document describes Restate's partition processor state machine behavior based on its inputs, effects/state changes and actions.

The `StateMachine` processes `Commands` it consumes from its log.
Every command can result into state changes, which are written to the PartitionProcessor's RocksDB instance, and actions which are sent to the actuators if the PartitionProcessor is the leader.

The PartitionProcessor can be in the following states:

* Follower
* Leader
* Candidate (soon to be added)

Before becoming leader, the partition processor will process all commands that were written before claiming the leadership.
This guarantees that the partition processor has seen all commands a previous leader might have written.

When becoming leader, the partition processor starts the following actuators:

* Shuffle
* TimerService

Moreover, the partition processor scans the invocation status table for *invoked* invocations.
The invoked invocations are being sent to the `Invoker`.

## Commands

This section describes the individual commands and their effects.

### Invoke

#### Description

Instructs the partition processor to invoke the given `ServiceInvocation`.

#### Effects

If idempotency key is set, try to resolve request:

If invocation is still ongoing, then attach to response sink (`Effect::AppendResponseSink`).
If invocation is completed, then send response to caller (`Effect::EnqueueIntoOutbox`/`Effect::IngressResponse` depending on caller).
If invocation is free, then send gone response to caller (`Effect::EnqueueIntoOutbox`).

If request could be resolved, then send submit notification if needed (`Effect::IngressSubmitNotification`) and return.
If request could not be resolved, then store idempotency id (`Effect::StoreIdempotencyId`).

If execution time is configured, then register timer (`Effect::RegisterTimer`) and return.

If invocation target is `VirtualObject`, then check whether virtual object is locked.
If it is locked, then enqueue invocation in the inbox (`Effect::EnqueueIntoInbox`), send submit notification (`Effect::IngressSubmitNotification`), store inboxed invocation (`Effect::StoreInboxedInvocation`) and return.

If invocation target is `Workflow`, then check whether it is locked.
If it is locked, then send `WORKFLOW_ALREADY_INVOKED_INVOCATION_ERROR` back to caller (`Effect::EnqueueIntoOutbox`/`Effect::IngressResponse` depending on caller), send submit notification (`Effect::IngressSubmitNotification`) and return.

Send submit notification if needed (`Effect::IngressSubmitNotification`) and invoke service `Effect::InvokeService`.

### InvocationResponse

#### Description

Complete journal entry with invocation response.

#### Effects

Try to complete journal entry if still present.
If invocation is invoked, then complete journal entry and forward completion to invoker (`Effect::StoreCompletion` and `Effect::ForwardCompletion`).
If invocation is suspended, then complete journal entry and check whether invocation can be resumed (`Effect::StoreCompletion` and `Effect::ResumeService`).

### InvokerEffect

#### Description

Handle the effects of an ongoing invocation.

#### Effects

Handle invoker effects:

##### PinnedDeployment

`Effect::StorePinnedDeployment`.

##### JournalEntry

###### Input/Output

Noop

###### GetState

Read state from partition storage and complete journal entry if target has state.
Forward completion to invoker (`Effect::ForwardCompletion`).

###### SetState

`Effect::SetState` with new state value.

###### Others

more to follow :-(

##### Suspended

Resume service if it can be resumed (`Effect::ResumeService`), otherwise suspend (`Effect::SuspendService`).

##### End

End an invocation.
Pop from inbox to invoke waiting invocation if target is `VirtualObject` (`Effect::PopInbox`).
Send response to sinks (other services `Effect::EnqueueIntoOutbox`, ingress `Effect::IngressResponse`).
Store completed result if completion retention is non-zero (`Effect::StoreCompletedInvocation`), otherwise free invocation status (`Effect::FreeInvocation`).
Drop journal (`Effect::DropJournal`).

##### Failed

An invocation failed terminally. Same effects as end just with failed result.

### Timer

#### Description

Handle fired timer.

#### Effects

Delete timer under given key (`Effect::DeleteTimer`).
If timer is complete journal entry, then treat as `InvocationResponse` command.
If timer is invoke, then treat as `Invoke` command.
If timer is clean invocation status, then free invocation (`Effect::FreeInvocation`), delete idempotency key (`Effect::DeleteIdempotencyId`), unlock service id (`Effect::UnlockService`) and clear all state (`Effect::ClearAllState`) if target is `Workflow`.

### Truncate outbox

#### Description

Truncate the outbox (remove the outbox entry with the given index).

#### Effects

`Effect::TruncateOutbox`.