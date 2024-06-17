// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

mod row;
mod schema;
mod table;

#[derive(Debug, Default)]
struct FsmVariables {
    partition_id: PartitionId,
    inbox_sequence_number: u64,
    outbox_sequence_number: u64,
    last_applied_lsn: u64,
}

use restate_types::identifiers::PartitionId;
pub(crate) use table::register_self;
