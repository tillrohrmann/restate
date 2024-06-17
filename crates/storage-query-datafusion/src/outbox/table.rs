// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::fmt::Debug;
use std::ops::RangeInclusive;
use std::sync::Arc;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::arrow::record_batch::RecordBatch;

use crate::context::QueryContext;
use crate::generic_table::{GenericTableProvider, RangeScanner};
use crate::outbox::row::append_outbox_message;
use crate::outbox::schema::OutboxBuilder;
use datafusion::physical_plan::stream::RecordBatchReceiverStream;
use datafusion::physical_plan::SendableRecordBatchStream;
pub use datafusion_expr::UserDefinedLogicalNode;
use restate_storage_api::StorageError;
use restate_storage_rocksdb::outbox_table::KeyedOutboxMessage;
use restate_storage_rocksdb::RocksDBStorage;
use restate_types::identifiers::PartitionKey;
use tokio::sync::mpsc::Sender;

pub(crate) fn register_self(
    ctx: &QueryContext,
    storage: RocksDBStorage,
) -> datafusion::common::Result<()> {
    let table =
        GenericTableProvider::new(OutboxBuilder::schema(), Arc::new(OutboxScanner(storage)));

    ctx.as_ref()
        .register_table("sys_outbox", Arc::new(table))
        .map(|_| ())
}

#[derive(Debug, Clone)]
struct OutboxScanner(RocksDBStorage);

impl RangeScanner for OutboxScanner {
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
            let outbox_messages = db.all_outbox_messages();
            for_each_state(schema, tx, outbox_messages).await;
            Ok(())
        };
        stream_builder.spawn(background_task);
        stream_builder.build()
    }
}

async fn for_each_state(
    schema: SchemaRef,
    tx: Sender<datafusion::common::Result<RecordBatch>>,
    mut outbox_messages: impl Iterator<Item = Result<KeyedOutboxMessage, StorageError>>,
) {
    let mut builder = OutboxBuilder::new(schema.clone());
    let mut temp = String::new();

    while let Some(Ok(outbox_message)) = outbox_messages.next() {
        append_outbox_message(&mut builder, &mut temp, outbox_message);

        if builder.full() {
            let batch = builder.finish();
            if tx.send(Ok(batch)).await.is_err() {
                // not sure what to do here?
                // the other side has hung up on us.
                // we probably don't want to panic, is it will cause the entire process to exit
                return;
            }
            builder = OutboxBuilder::new(schema.clone());
        }
    }
    if !builder.empty() {
        let result = builder.finish();
        let _ = tx.send(Ok(result)).await;
    }
}
