// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use super::schema::OutboxBuilder;
use crate::table_util::format_using;
use restate_storage_api::outbox_table::OutboxMessage;
use restate_storage_rocksdb::outbox_table::KeyedOutboxMessage;
use restate_types::invocation::TerminationFlavor;

#[inline]
pub(crate) fn append_outbox_message(
    builder: &mut OutboxBuilder,
    output: &mut String,
    outbox_message: KeyedOutboxMessage,
) {
    let (partition_id, index, message) = outbox_message;

    let mut row = builder.row();
    row.partition_id(partition_id);
    row.outbox_number(index);

    if row.is_target_id_defined() {
        row.target_id(format_using(output, message.invocation_id()));
    }

    match message {
        OutboxMessage::ServiceInvocation(service_invocation) => {
            row.message_type("invocation");

            if row.is_invocation_target_defined() {
                row.invocation_target(format_using(output, &service_invocation.invocation_target));
            }
        }
        OutboxMessage::ServiceResponse(response) => {
            row.message_type("response");
            row.response_entry_index(response.entry_index);
        }
        OutboxMessage::InvocationTermination(termination) => {
            row.message_type("termination");

            match termination.flavor {
                TerminationFlavor::Kill => row.termination_flavor("kill"),
                TerminationFlavor::Cancel => row.termination_flavor("cancel"),
            }
        }
    }
}
