// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use crate::keys::TableKey;
use crate::keys::{define_table_key, KeyKind};
use crate::owned_iter::OwnedIterator;
use crate::scan::TableScan::FullScanPartitionKeyRange;
use crate::TableKind::Journal;
use crate::{PartitionStore, RocksDBTransaction, StorageAccess};
use crate::{TableScan, TableScanIterationDecision};
use futures::Stream;
use futures_util::stream;
use restate_storage_api::journal_table::{JournalEntry, JournalTable, ReadOnlyJournalTable};
use restate_storage_api::{Result, StorageError};
use restate_types::identifiers::{
    EntryIndex, InvocationId, InvocationUuid, PartitionKey, WithPartitionKey,
};
use restate_types::storage::StorageCodec;
use std::io::Cursor;
use std::ops::RangeInclusive;

define_table_key!(
    Journal,
    KeyKind::Journal,
    JournalKey(
        partition_key: PartitionKey,
        invocation_uuid: InvocationUuid,
        journal_index: u32
    )
);

fn write_journal_entry_key(invocation_id: &InvocationId, journal_index: u32) -> JournalKey {
    JournalKey::default()
        .partition_key(invocation_id.partition_key())
        .invocation_uuid(invocation_id.invocation_uuid())
        .journal_index(journal_index)
}

fn put_journal_entry<S: StorageAccess>(
    storage: &mut S,
    invocation_id: &InvocationId,
    journal_index: u32,
    journal_entry: JournalEntry,
) {
    let key = write_journal_entry_key(invocation_id, journal_index);

    storage.put_kv(key, journal_entry);
}

fn get_journal_entry<S: StorageAccess>(
    storage: &mut S,
    invocation_id: &InvocationId,
    journal_index: u32,
) -> Result<Option<JournalEntry>> {
    let key = write_journal_entry_key(invocation_id, journal_index);

    storage.get_value(key)
}

fn get_journal<S: StorageAccess>(
    storage: &mut S,
    invocation_id: &InvocationId,
    journal_length: EntryIndex,
) -> Vec<Result<(EntryIndex, JournalEntry)>> {
    let key = JournalKey::default()
        .partition_key(invocation_id.partition_key())
        .invocation_uuid(invocation_id.invocation_uuid());

    let mut n = 0;
    storage.for_each_key_value_in_place(
        TableScan::SinglePartitionKeyPrefix(invocation_id.partition_key(), key),
        move |k, mut v| {
            let key = JournalKey::deserialize_from(&mut Cursor::new(k)).map(|journal_key| {
                journal_key
                    .journal_index
                    .expect("The journal index must be part of the journal key.")
            });
            let entry = StorageCodec::decode::<JournalEntry, _>(&mut v)
                .map_err(|error| StorageError::Generic(error.into()));

            let result = key.and_then(|key| entry.map(|entry| (key, entry)));

            n += 1;
            if n < journal_length {
                TableScanIterationDecision::Emit(result)
            } else {
                TableScanIterationDecision::BreakWith(result)
            }
        },
    )
}

fn delete_journal<S: StorageAccess>(
    storage: &mut S,
    invocation_id: &InvocationId,
    journal_length: EntryIndex,
) {
    let mut key = write_journal_entry_key(invocation_id, 0);
    let k = &mut key;
    for journal_index in 0..journal_length {
        k.journal_index = Some(journal_index);
        storage.delete_key(k);
    }
}

impl ReadOnlyJournalTable for PartitionStore {
    async fn get_journal_entry(
        &mut self,
        invocation_id: &InvocationId,
        journal_index: u32,
    ) -> Result<Option<JournalEntry>> {
        get_journal_entry(self, invocation_id, journal_index)
    }

    fn get_journal(
        &mut self,
        invocation_id: &InvocationId,
        journal_length: EntryIndex,
    ) -> impl Stream<Item = Result<(EntryIndex, JournalEntry)>> + Send {
        stream::iter(get_journal(self, invocation_id, journal_length))
    }
}

impl<'a> ReadOnlyJournalTable for RocksDBTransaction<'a> {
    async fn get_journal_entry(
        &mut self,
        invocation_id: &InvocationId,
        journal_index: u32,
    ) -> Result<Option<JournalEntry>> {
        get_journal_entry(self, invocation_id, journal_index)
    }

    fn get_journal(
        &mut self,
        invocation_id: &InvocationId,
        journal_length: EntryIndex,
    ) -> impl Stream<Item = Result<(EntryIndex, JournalEntry)>> + Send {
        stream::iter(get_journal(self, invocation_id, journal_length))
    }
}

impl<'a> JournalTable for RocksDBTransaction<'a> {
    async fn put_journal_entry(
        &mut self,
        invocation_id: &InvocationId,
        journal_index: u32,
        journal_entry: JournalEntry,
    ) {
        put_journal_entry(self, invocation_id, journal_index, journal_entry)
    }

    async fn delete_journal(&mut self, invocation_id: &InvocationId, journal_length: EntryIndex) {
        delete_journal(self, invocation_id, journal_length)
    }
}

#[derive(Debug)]
pub struct OwnedJournalRow {
    pub invocation_id: InvocationId,
    pub journal_index: u32,
    pub journal_entry: JournalEntry,
}

impl PartitionStore {
    pub fn all_journal(
        &self,
        range: RangeInclusive<PartitionKey>,
    ) -> impl Iterator<Item = OwnedJournalRow> + '_ {
        let iter = self.iterator_from(FullScanPartitionKeyRange::<JournalKey>(range));
        OwnedIterator::new(iter).map(|(mut key, mut value)| {
            let journal_key = JournalKey::deserialize_from(&mut key)
                .expect("journal key must deserialize into JournalKey");
            let journal_entry = StorageCodec::decode::<JournalEntry, _>(&mut value)
                .expect("journal entry must deserialize into JournalEntry");
            OwnedJournalRow {
                invocation_id: InvocationId::from_parts(
                    journal_key
                        .partition_key
                        .expect("journal key must have a partition key"),
                    journal_key
                        .invocation_uuid
                        .expect("journal key must have an invocation uuid"),
                ),
                journal_index: journal_key
                    .journal_index
                    .expect("journal key must have an index"),
                journal_entry,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::journal_table::write_journal_entry_key;
    use crate::keys::TableKey;
    use bytes::Bytes;
    use restate_types::identifiers::{InvocationId, InvocationUuid};

    fn journal_entry_key(invocation_id: &InvocationId, journal_index: u32) -> Bytes {
        write_journal_entry_key(invocation_id, journal_index)
            .serialize()
            .freeze()
    }

    #[test]
    fn journal_keys_sort_lex() {
        //
        // across invocations
        //
        assert!(
            journal_entry_key(
                &InvocationId::from_parts(1337, InvocationUuid::from_parts(0, 1)),
                0
            ) < journal_entry_key(
                &InvocationId::from_parts(1337, InvocationUuid::from_parts(0, 2)),
                0
            )
        );
        //
        // within the same service and key
        //
        let mut previous_key = journal_entry_key(
            &InvocationId::from_parts(1337, InvocationUuid::from_parts(0, 1)),
            0,
        );
        for i in 1..300 {
            let current_key = journal_entry_key(
                &InvocationId::from_parts(1337, InvocationUuid::from_parts(0, 1)),
                i,
            );
            assert!(previous_key < current_key);
            previous_key = current_key;
        }
    }
}
