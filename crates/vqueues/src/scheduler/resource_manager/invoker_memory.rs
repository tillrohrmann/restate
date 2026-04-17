// Copyright (c) 2023 - 2026 Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::collections::VecDeque;
use std::task::{Context, Poll};

use hashbrown::HashMap;

use restate_memory::{MemoryLease, MemoryPool, NonZeroByteCount, PollMemoryPool};

use crate::scheduler::VQueueHandle;

/// Gates Inbox → Running transitions on invoker memory availability.
///
/// FIFO-ordered with a **per-head promise map**: when `poll_head` pre-reserves
/// bytes for a waiter it stores the lease under that vqueue's handle in
/// [`Self::pending_leases`]. Only that vqueue can consume the lease on its next
/// [`Self::poll_reserve`] call; other callers (fresh arrivals or later waiters)
/// go through the standard FIFO path and reserve their own bytes from the pool.
/// This eliminates the "promise-stealing" race inherent to a shared accumulator.
///
/// # Invariant
///
/// A vqueue handle is tracked in at most one of [`Self::waiters`] and
/// [`Self::pending_leases`] at a time. The scheduler enforces this by calling
/// [`Self::remove_from_waiters`] before re-eligibilising a blocked vqueue; this
/// limiter preserves it by atomically moving the head from `waiters` to
/// `pending_leases` in [`Self::poll_head`] and by consuming the promise from
/// `pending_leases` before touching `waiters` in [`Self::poll_reserve`].
///
/// # Head-of-line blocking under strict FIFO
///
/// Memory reservations have variable sizes, so strict FIFO means a head waiter
/// requesting more bytes than the pool can currently supply will block every
/// smaller waiter behind it — even when those smaller waiters could be
/// satisfied from the available budget. We accept this trade-off for simplicity
/// and fairness; a plausible follow-up is to let smaller-footprint entries
/// bypass the head when their reservation can be satisfied immediately.
pub struct InvokerMemoryLimiter {
    /// Default minimum memory reserved per invocation when no `memory_hint` is
    /// available for the entry.
    // todo make dynamically configurable via configuration
    initial_invocation_memory: NonZeroByteCount,
    memory_limiter: PollMemoryPool,
    /// FIFO of vqueues waiting for memory, paired with the size each needs.
    waiters: VecDeque<(VQueueHandle, NonZeroByteCount)>,
    /// Leases pre-reserved by [`Self::poll_head`] for specific vqueues. Each
    /// entry is consumed by exactly one [`Self::poll_reserve`] call from the
    /// matching vqueue.
    pending_leases: HashMap<VQueueHandle, MemoryLease>,
}

impl InvokerMemoryLimiter {
    pub fn new(memory_pool: MemoryPool, initial_invocation_memory: NonZeroByteCount) -> Self {
        Self {
            initial_invocation_memory,
            memory_limiter: PollMemoryPool::new(memory_pool),
            waiters: VecDeque::new(),
            pending_leases: HashMap::new(),
        }
    }

    pub(crate) fn remove_from_waiters(&mut self, vqueue: VQueueHandle) {
        self.waiters.retain(|(h, _)| *h != vqueue);
        // Drop any pre-reserved lease for this vqueue — its `Drop` returns the
        // bytes to the pool automatically.
        self.pending_leases.remove(&vqueue);
    }

    /// Size to reserve for an invocation. The invoker-reported `memory_hint` is
    /// the amount it previously failed to acquire, so we take the larger of the
    /// hint and the default floor.
    pub fn reservation_size(&self, memory_hint: Option<NonZeroByteCount>) -> NonZeroByteCount {
        match memory_hint {
            Some(hint) => hint.max(self.initial_invocation_memory),
            None => self.initial_invocation_memory,
        }
    }

    pub(super) fn poll_reserve(
        &mut self,
        cx: &mut Context<'_>,
        vqueue: VQueueHandle,
        size: NonZeroByteCount,
    ) -> Option<MemoryLease> {
        let size_bytes = size.as_usize();

        // Per-head promise: if poll_head already reserved for us, consume that
        // lease now (reconciling the size if the request changed).
        if let Some(mut lease) = self.pending_leases.remove(&vqueue) {
            let held = lease.size().as_usize();
            match size_bytes.cmp(&held) {
                std::cmp::Ordering::Less => {
                    lease.shrink(held - size_bytes);
                    return Some(lease);
                }
                std::cmp::Ordering::Equal => return Some(lease),
                std::cmp::Ordering::Greater => {
                    if lease.try_grow(size_bytes - held) {
                        return Some(lease);
                    }
                    // Couldn't grow in place; drop the partial lease (returns
                    // bytes to the pool) and fall through to a fresh reservation.
                    drop(lease);
                }
            }
        }

        // Standard FIFO path for non-promised callers.
        self.waiters.push_back((vqueue, size));

        if self.waiters.front().is_none_or(|(h, _)| *h != vqueue) {
            // Not the head — wait our turn; the pool's `Notify` will wake the
            // scheduler when a release happens.
            return None;
        }

        match self.memory_limiter.poll_reserve(cx, size_bytes) {
            Poll::Ready(lease) => {
                self.waiters.pop_front();
                Some(lease)
            }
            Poll::Pending => None,
        }
    }

    /// Pre-reserves memory for the head waiter so the scheduler can move it back
    /// onto the ready ring. The lease is stashed in [`Self::pending_leases`]
    /// keyed by the head's handle; only that vqueue can consume it on its next
    /// `poll_reserve` call.
    ///
    /// Returns `Ready(None)` only when `waiters` is empty — the caller's
    /// drain loop can treat that as a genuine terminator.
    pub(super) fn poll_head(&mut self, cx: &mut Context<'_>) -> Poll<Option<VQueueHandle>> {
        let Some(&(head_vqueue, head_size)) = self.waiters.front() else {
            return Poll::Ready(None);
        };

        match self.memory_limiter.poll_reserve(cx, head_size.as_usize()) {
            Poll::Ready(lease) => {
                self.pending_leases.insert(head_vqueue, lease);
                self.waiters.pop_front();
                Poll::Ready(Some(head_vqueue))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::task::{Context, Poll, Waker};

    use slotmap::SlotMap;

    use super::*;

    fn size(bytes: usize) -> NonZeroByteCount {
        NonZeroByteCount::new(NonZeroUsize::new(bytes).unwrap())
    }

    fn pool(capacity: usize) -> MemoryPool {
        MemoryPool::with_capacity(size(capacity))
    }

    #[test]
    fn poll_head_promise_is_not_stolen_by_fresh_arrival() {
        // Pool of 150 with 100-byte reservations: after V1 runs, the pool has
        // exactly 100 bytes free — enough for either V2 (promised) or V3 (fresh),
        // but not both.
        let mut limiter = InvokerMemoryLimiter::new(pool(150), size(100));
        let mut keys: SlotMap<VQueueHandle, ()> = SlotMap::with_key();
        let v1 = keys.insert(());
        let v2 = keys.insert(());
        let v3 = keys.insert(());

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // V1 reserves 100 — succeeds.
        let lease_v1 = limiter
            .poll_reserve(&mut cx, v1, size(100))
            .expect("V1 should get its reservation");

        // V2 reserves 100 — pool has only 50 left, so it blocks.
        assert!(limiter.poll_reserve(&mut cx, v2, size(100)).is_none());

        // V1 drops its lease — 100 bytes return to the pool.
        drop(lease_v1);

        // poll_head pre-reserves V2's 100 bytes and pops V2 from waiters.
        match limiter.poll_head(&mut cx) {
            Poll::Ready(Some(handle)) => assert_eq!(handle, v2),
            other => panic!("expected Ready(Some(V2)), got {other:?}"),
        }

        // A fresh V3 shows up before V2 polls. Under the old shared-cache
        // design V3 would have split bytes out of the accumulator intended for
        // V2; under the per-head promise it cannot: pending_leases[V3] is
        // empty, so V3 falls to the FIFO path and finds the pool exhausted
        // (V2's 100 bytes are still held by pending_leases[V2]).
        assert!(limiter.poll_reserve(&mut cx, v3, size(100)).is_none());

        // V2 now consumes its promised lease.
        let lease_v2 = limiter
            .poll_reserve(&mut cx, v2, size(100))
            .expect("V2 should consume the promised lease");
        assert_eq!(lease_v2.size().as_usize(), 100);
    }

    #[test]
    fn remove_from_waiters_drops_pending_lease() {
        let mut limiter = InvokerMemoryLimiter::new(pool(100), size(100));
        let mut keys: SlotMap<VQueueHandle, ()> = SlotMap::with_key();
        let v1 = keys.insert(());
        let v2 = keys.insert(());

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // V1 acquires immediately.
        let lease_v1 = limiter
            .poll_reserve(&mut cx, v1, size(100))
            .expect("V1 acquires");
        // V2 blocks.
        assert!(limiter.poll_reserve(&mut cx, v2, size(100)).is_none());
        // Release V1 so poll_head can promise V2.
        drop(lease_v1);
        assert!(matches!(
            limiter.poll_head(&mut cx),
            Poll::Ready(Some(h)) if h == v2
        ));

        // Dropping V2 (e.g., its vqueue became dormant) should return the
        // pre-reserved bytes to the pool.
        limiter.remove_from_waiters(v2);

        // V1 can now reserve a fresh 100 — pool is fully free again.
        let lease_v1_again = limiter
            .poll_reserve(&mut cx, v1, size(100))
            .expect("pool must be free after V2's pending lease is dropped");
        assert_eq!(lease_v1_again.size().as_usize(), 100);
    }
}
