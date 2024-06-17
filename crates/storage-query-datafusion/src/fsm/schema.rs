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

define_table!(fsm(
    partition_id: DataType::UInt64,

    inbox_sequence_number: DataType::UInt64,

    outbox_sequence_number: DataType::UInt64,

    last_applied_lsn: DataType::UInt64,
));
