// Copyright (c) 2024 - Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

/// Variables of the partition processor
#[derive(Debug, Copy, Clone, strum_macros::FromRepr)]
#[repr(u64)]
pub enum FsmVariable {
    InboxSeqNumber = 0,
    OutboxSeqNumber = 1,
    AppliedLsn = 2,
}

impl FsmVariable {
    pub fn as_repr(&self) -> u64 {
        *self as u64
    }
}
