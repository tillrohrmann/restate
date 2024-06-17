// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use crate::fsm::schema::FsmBuilder;
use crate::fsm::FsmVariables;

#[inline]
pub(crate) fn append_fsm_variables(builder: &mut FsmBuilder, fsm_variables: FsmVariables) {
    let mut row = builder.row();
    row.partition_id(fsm_variables.partition_id);
    row.inbox_sequence_number(fsm_variables.inbox_sequence_number);
    row.outbox_sequence_number(fsm_variables.outbox_sequence_number);
    row.last_applied_lsn(fsm_variables.last_applied_lsn);
}
