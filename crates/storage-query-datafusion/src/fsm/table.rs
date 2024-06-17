// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use bytes::Bytes;
use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::sync::Arc;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::arrow::record_batch::RecordBatch;

use crate::context::QueryContext;
use crate::fsm::row::append_fsm_variables;
use crate::fsm::schema::FsmBuilder;
use crate::fsm::FsmVariables;
use crate::generic_table::{GenericTableProvider, RangeScanner};
use datafusion::physical_plan::stream::RecordBatchReceiverStream;
use datafusion::physical_plan::SendableRecordBatchStream;
pub use datafusion_expr::UserDefinedLogicalNode;
use restate_storage_api::fsm_table::SequenceNumber;
use restate_storage_api::StorageError;
use restate_storage_rocksdb::fsm_table::FsmKeyValue;
use restate_storage_rocksdb::RocksDBStorage;
use restate_types::fsm::FsmVariable;
use restate_types::identifiers::PartitionKey;
use restate_types::storage::StorageCodec;
use tokio::sync::mpsc::Sender;
use tracing::info;

pub(crate) fn register_self(
    ctx: &QueryContext,
    storage: RocksDBStorage,
) -> datafusion::common::Result<()> {
    let table = GenericTableProvider::new(FsmBuilder::schema(), Arc::new(FsmScanner(storage)));

    ctx.as_ref()
        .register_table("sys_fsm", Arc::new(table))
        .map(|_| ())
}

#[derive(Debug, Clone)]
struct FsmScanner(RocksDBStorage);

impl RangeScanner for FsmScanner {
    fn scan(
        &self,
        _range: RangeInclusive<PartitionKey>,
        projection: SchemaRef,
    ) -> SendableRecordBatchStream {
        let db = self.0.clone();
        let schema = projection.clone();
        let mut stream_builder = RecordBatchReceiverStream::builder(projection, 16);
        let tx = stream_builder.tx();
        let background_task = async move {
            let fsm_variables = db.all_fsm_variables();
            let fsm_variables = group_fsm_variables(fsm_variables);
            match fsm_variables {
                Ok(fsm_variables) => for_each_fsm_variables(schema, tx, fsm_variables).await,
                Err(err) => {
                    info!("failed reading fsm table: {err}");
                }
            };
            Ok(())
        };
        stream_builder.spawn(background_task);
        stream_builder.build()
    }
}

fn group_fsm_variables(
    mut fsm_variables: impl Iterator<Item = Result<FsmKeyValue, StorageError>>,
) -> Result<impl Iterator<Item = FsmVariables>, StorageError> {
    let mut result = Vec::new();
    if let Some(fsm_variable) = fsm_variables.next() {
        let fsm_variable = fsm_variable?;
        let mut current_partition_id = fsm_variable.0;
        let mut current_variables = FsmVariables {
            partition_id: fsm_variable.0,
            ..FsmVariables::default()
        };
        update_fsm_variables(&mut current_variables, fsm_variable.1, fsm_variable.2)?;

        for fsm_variable in fsm_variables {
            let fsm_variable = fsm_variable?;

            if current_partition_id != fsm_variable.0 {
                result.push(current_variables);
                current_variables = FsmVariables {
                    partition_id: fsm_variable.0,
                    ..FsmVariables::default()
                };
                current_partition_id = fsm_variable.0;
            }

            update_fsm_variables(&mut current_variables, fsm_variable.1, fsm_variable.2)?;
        }
        result.push(current_variables);
    }

    Ok(result.into_iter())
}

fn update_fsm_variables(
    current_variables: &mut FsmVariables,
    variable: FsmVariable,
    mut value: Bytes,
) -> Result<(), StorageError> {
    let sequence_number = StorageCodec::decode::<SequenceNumber, _>(&mut value)
        .map_err(|err| StorageError::Conversion(err.into()))?
        .into();

    match variable {
        FsmVariable::InboxSeqNumber => {
            current_variables.inbox_sequence_number = sequence_number;
        }
        FsmVariable::OutboxSeqNumber => {
            current_variables.outbox_sequence_number = sequence_number;
        }
        FsmVariable::AppliedLsn => {
            current_variables.last_applied_lsn = sequence_number;
        }
    }
    Ok(())
}

async fn for_each_fsm_variables(
    schema: SchemaRef,
    tx: Sender<datafusion::common::Result<RecordBatch>>,
    mut fsm_variables: impl Iterator<Item = FsmVariables>,
) {
    let mut builder = FsmBuilder::new(schema.clone());

    while let Some(fsm_variables) = fsm_variables.next() {
        append_fsm_variables(&mut builder, fsm_variables);

        if builder.full() {
            let batch = builder.finish();
            if tx.send(Ok(batch)).await.is_err() {
                // not sure what to do here?
                // the other side has hung up on us.
                // we probably don't want to panic, is it will cause the entire process to exit
                return;
            }
            builder = FsmBuilder::new(schema.clone());
        }
    }
    if !builder.empty() {
        let result = builder.finish();
        let _ = tx.send(Ok(result)).await;
    }
}
