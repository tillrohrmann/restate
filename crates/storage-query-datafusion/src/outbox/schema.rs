// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

#![allow(dead_code)]

use crate::table_macro::*;

use datafusion::arrow::datatypes::DataType;

define_table!(outbox(
    partition_id: DataType::UInt64,

    outbox_number: DataType::UInt64,

    message_type: DataType::LargeUtf8,

    target_id: DataType::LargeUtf8,

    invocation_target: DataType::LargeUtf8,

    response_entry_index: DataType::UInt32,

    termination_flavor: DataType::LargeUtf8,
));
