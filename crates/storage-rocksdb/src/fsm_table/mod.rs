// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use crate::keys::{define_table_key, KeyKind, TableKey};
use crate::owned_iter::OwnedIterator;
use crate::scan::TableScan;
use crate::TableKind::PartitionStateMachine;
use crate::{RocksDBStorage, RocksDBTransaction, StorageAccess};
use bytes::Bytes;
use restate_storage_api::fsm_table::{FsmTable, ReadOnlyFsmTable};
use restate_storage_api::{Result, StorageError};
use restate_types::fsm::FsmVariable;
use restate_types::identifiers::PartitionId;
use restate_types::storage::{StorageDecode, StorageEncode};
use std::future;
use std::future::Future;

define_table_key!(
    PartitionStateMachine,
    KeyKind::Fsm,
    PartitionStateMachineKey(partition_id: PartitionId, state_id: u64)
);

fn get<T: StorageDecode, S: StorageAccess>(
    storage: &mut S,
    partition_id: PartitionId,
    state_id: u64,
) -> Result<Option<T>> {
    let key = PartitionStateMachineKey::default()
        .partition_id(partition_id)
        .state_id(state_id);
    storage.get_value(key)
}

fn put<S: StorageAccess>(
    storage: &mut S,
    partition_id: PartitionId,
    state_id: u64,
    state_value: impl StorageEncode,
) {
    let key = PartitionStateMachineKey::default()
        .partition_id(partition_id)
        .state_id(state_id);
    storage.put_kv(key, state_value);
}

fn clear<S: StorageAccess>(storage: &mut S, partition_id: PartitionId, state_id: u64) {
    let key = PartitionStateMachineKey::default()
        .partition_id(partition_id)
        .state_id(state_id);
    storage.delete_key(&key);
}

impl ReadOnlyFsmTable for RocksDBStorage {
    async fn get<T>(&mut self, partition_id: PartitionId, state_id: u64) -> Result<Option<T>>
    where
        T: StorageDecode,
    {
        get(self, partition_id, state_id)
    }
}

impl<'a> ReadOnlyFsmTable for RocksDBTransaction<'a> {
    async fn get<T>(&mut self, partition_id: PartitionId, state_id: u64) -> Result<Option<T>>
    where
        T: StorageDecode,
    {
        get(self, partition_id, state_id)
    }
}

impl<'a> FsmTable for RocksDBTransaction<'a> {
    fn put(
        &mut self,
        partition_id: PartitionId,
        state_id: u64,
        state_value: impl StorageEncode,
    ) -> impl Future<Output = ()> + Send {
        put(self, partition_id, state_id, state_value);
        future::ready(())
    }

    async fn clear(&mut self, partition_id: PartitionId, state_id: u64) {
        clear(self, partition_id, state_id)
    }
}

pub type FsmKeyValue = (PartitionId, FsmVariable, Bytes);

impl RocksDBStorage {
    pub fn all_fsm_variables(&self) -> impl Iterator<Item = crate::Result<FsmKeyValue>> + '_ {
        let fsm_prefix = PartitionStateMachineKey::default();
        OwnedIterator::new(self.iterator_from(TableScan::KeyPrefix(fsm_prefix)))
            .map(|(key, value)| extract_fsm_variable(key, value))
    }
}

fn extract_fsm_variable(mut key: Bytes, value: Bytes) -> crate::Result<FsmKeyValue> {
    match PartitionStateMachineKey::deserialize_from(&mut key)?.into_inner() {
        (Some(partition_id), Some(state_id)) => {
            let fsm_variable =
                FsmVariable::from_repr(state_id).ok_or(StorageError::DataIntegrityError)?;
            Ok((partition_id, fsm_variable, value))
        }
        _ => Err(StorageError::DataIntegrityError),
    }
}
