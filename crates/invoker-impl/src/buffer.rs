// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use restate_invoker_api::JournalEntry;

/// Buffer for journal entries that are being received from the service deployment.
///
/// The buffer has a maximum number of elements and maximum size in bytes.
pub struct JournalBuffer {
    max_size_in_bytes: Option<usize>,

    buffer: Vec<JournalEntry>,
    requires_ack: Vec<bool>,
    size_in_bytes: usize,
}

impl JournalBuffer {
    pub fn new(capacity: usize, max_size_in_bytes: Option<usize>) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            requires_ack: Vec::with_capacity(capacity),
            max_size_in_bytes,
            size_in_bytes: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty() && self.requires_ack.is_empty() && self.size_in_bytes == 0
    }

    pub fn is_full(&self) -> bool {
        self.buffer.len() >= self.buffer.capacity()
            || self
                .max_size_in_bytes
                .map(|max_size| self.size_in_bytes >= max_size)
                .unwrap_or(false)
    }

    pub fn push_entry(&mut self, journal_entry: JournalEntry, requires_ack: bool) {
        assert!(!self.is_full(), "buffer is full");

        self.size_in_bytes += journal_entry.entry.serialized_entry().len();

        self.buffer.push(journal_entry);
        self.requires_ack.push(requires_ack);
    }

    pub fn drain(
        &mut self,
    ) -> (
        usize,
        impl Iterator<Item = JournalEntry> + '_,
        impl Iterator<Item = bool> + '_,
    ) {
        self.size_in_bytes = 0;
        (
            self.buffer.len(),
            self.buffer.drain(..),
            self.requires_ack.drain(..),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::JournalBuffer;
    use bytes::Bytes;
    use restate_invoker_api::JournalEntry;
    use restate_types::journal::enriched::EnrichedRawEntry;
    use restate_types::journal::raw::EntryHeader;

    #[test]
    fn is_empty() {
        let mut journal_buffer = JournalBuffer::new(1024, None);

        assert!(journal_buffer.is_empty());

        journal_buffer.push_entry(
            JournalEntry {
                entry: EnrichedRawEntry::new(EntryHeader::ClearState, Bytes::default()),
                entry_index: 0,
            },
            false,
        );

        assert!(!journal_buffer.is_empty());

        {
            // drain the buffer
            drop(journal_buffer.drain());
        }

        assert!(journal_buffer.is_empty());
    }
}
