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
use futures::future;
use prost::Message;
use restate_errors::NotRunningError;
use restate_invoker_api::{Effect, EffectKind, InvokeInputJournal};
use restate_types::identifiers::{
    EntryIndex, FullInvocationId, PartitionKey, PartitionLeaderEpoch,
};
use restate_types::journal::enriched::{EnrichedEntryHeader, EnrichedRawEntry};
use restate_types::journal::Completion;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct InvokeCommand {
    pub partition: PartitionLeaderEpoch,
    pub full_invocation_id: FullInvocationId,
    pub journal: InvokeInputJournal,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum InputCommand {
    Invoke(InvokeCommand),
    Completion {
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        completion: Completion,
    },
    StoredEntryAck {
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        entry_index: EntryIndex,
    },

    /// Abort specific invocation id
    Abort {
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
    },

    /// Command used to clean up internal state when a partition leader is going away
    AbortAllPartition {
        partition: PartitionLeaderEpoch,
    },

    // needed for dynamic registration at Invoker
    RegisterPartition {
        partition: PartitionLeaderEpoch,
        partition_key_range: RangeInclusive<PartitionKey>,
        sender: mpsc::Sender<Effect>,
    },
}

#[derive(Debug)]
pub struct Service<StateReader> {
    cmd_rx: mpsc::UnboundedReceiver<InputCommand>,

    cmd_tx: mpsc::UnboundedSender<InputCommand>,

    partitions: HashMap<PartitionLeaderEpoch, mpsc::Sender<Effect>>,

    _state_reader: StateReader,
}

impl<StateReader> Service<StateReader>
where
    StateReader: restate_invoker_api::StateReader,
{
    pub fn new(state_reader: StateReader) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        Self {
            cmd_rx,
            cmd_tx,
            partitions: HashMap::default(),
            _state_reader: state_reader,
        }
    }

    pub fn handle(&self) -> ServiceHandle {
        ServiceHandle {
            cmd_tx: self.cmd_tx.clone(),
        }
    }

    pub async fn run(mut self, watch: drain::Watch) {
        let shutdown = watch.signaled();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_cmd(cmd).await
                },
                _ = &mut shutdown => {
                    break;
                }
            }
        }
    }

    async fn handle_cmd(&mut self, cmd: InputCommand) {
        match cmd {
            InputCommand::Invoke(invoke_cmd) => self.handle_invoke_cmd(invoke_cmd).await,
            InputCommand::Completion { .. } => {
                // nothing todo
            }
            InputCommand::StoredEntryAck { .. } => {
                // nothing todo
            }
            InputCommand::Abort { .. } => {
                // nothing todo
            }
            InputCommand::AbortAllPartition { .. } => {
                // nothing todo
            }
            InputCommand::RegisterPartition {
                partition, sender, ..
            } => {
                self.partitions.insert(partition, sender);
            }
        }
    }

    async fn handle_invoke_cmd(&mut self, invoke_cmd: InvokeCommand) {
        let InvokeCommand {
            partition,
            full_invocation_id,
            ..
        } = invoke_cmd;

        let pp_tx = self.partitions.get(&partition).unwrap().clone();
        tokio::spawn(async move {
            // simulate counter.Counter/GetAndAdd

            let get_state_entry = restate_service_protocol::pb::protocol::GetStateEntryMessage {
                key: Bytes::default(),
                result: Some(
                    restate_service_protocol::pb::protocol::get_state_entry_message::Result::Value(
                        Bytes::from_static(b"10"),
                    ),
                ),
            };
            pp_tx
                .send(Effect {
                    full_invocation_id: full_invocation_id.clone(),
                    kind: EffectKind::JournalEntry {
                        entry_index: 1,
                        entry: EnrichedRawEntry::new(
                            EnrichedEntryHeader::GetState { is_completed: true },
                            get_state_entry.encode_to_vec().into(),
                        ),
                    },
                })
                .await
                .unwrap();
            let set_state_entry = restate_service_protocol::pb::protocol::SetStateEntryMessage {
                key: Bytes::default(),
                value: Bytes::from_static(b"10"),
            };
            pp_tx
                .send(Effect {
                    full_invocation_id: full_invocation_id.clone(),
                    kind: EffectKind::JournalEntry {
                        entry_index: 2,
                        entry: EnrichedRawEntry::new(
                            EnrichedEntryHeader::SetState,
                            set_state_entry.encode_to_vec().into(),
                        ),
                    },
                })
                .await
                .unwrap();
            let output_stream_message =
                restate_service_protocol::pb::protocol::OutputStreamEntryMessage {
                    result: Some(restate_service_protocol::pb::protocol::output_stream_entry_message::Result::Value(Bytes::default()))
                };
            pp_tx
                .send(Effect {
                    full_invocation_id: full_invocation_id.clone(),
                    kind: EffectKind::JournalEntry {
                        entry_index: 3,
                        entry: EnrichedRawEntry::new(
                            EnrichedEntryHeader::OutputStream,
                            output_stream_message.encode_to_vec().into(),
                        ),
                    },
                })
                .await
                .unwrap();
            pp_tx
                .send(Effect {
                    full_invocation_id,
                    kind: EffectKind::End,
                })
                .await
                .unwrap();
        });
    }
}

#[derive(Debug, Clone)]
pub struct ServiceHandle {
    cmd_tx: mpsc::UnboundedSender<InputCommand>,
}

impl restate_invoker_api::ServiceHandle for ServiceHandle {
    type Future = futures::future::Ready<Result<(), NotRunningError>>;

    fn invoke(
        &mut self,
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        journal: InvokeInputJournal,
    ) -> Self::Future {
        let result = self
            .cmd_tx
            .send(InputCommand::Invoke(InvokeCommand {
                partition,
                full_invocation_id,
                journal,
            }))
            .map_err(|_| NotRunningError);
        future::ready(result)
    }

    fn resume(
        &mut self,
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        journal: InvokeInputJournal,
    ) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::Invoke(InvokeCommand {
                    partition,
                    full_invocation_id,
                    journal,
                }))
                .map_err(|_| NotRunningError),
        )
    }

    fn notify_completion(
        &mut self,
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        completion: Completion,
    ) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::Completion {
                    partition,
                    full_invocation_id,
                    completion,
                })
                .map_err(|_| NotRunningError),
        )
    }

    fn notify_stored_entry_ack(
        &mut self,
        partition: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
        entry_index: EntryIndex,
    ) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::StoredEntryAck {
                    partition,
                    full_invocation_id,
                    entry_index,
                })
                .map_err(|_| NotRunningError),
        )
    }

    fn abort_all_partition(&mut self, partition: PartitionLeaderEpoch) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::AbortAllPartition { partition })
                .map_err(|_| NotRunningError),
        )
    }

    fn abort_invocation(
        &mut self,
        partition_leader_epoch: PartitionLeaderEpoch,
        full_invocation_id: FullInvocationId,
    ) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::Abort {
                    partition: partition_leader_epoch,
                    full_invocation_id,
                })
                .map_err(|_| NotRunningError),
        )
    }

    fn register_partition(
        &mut self,
        partition: PartitionLeaderEpoch,
        partition_key_range: RangeInclusive<PartitionKey>,
        sender: Sender<Effect>,
    ) -> Self::Future {
        future::ready(
            self.cmd_tx
                .send(InputCommand::RegisterPartition {
                    partition,
                    partition_key_range,
                    sender,
                })
                .map_err(|_| NotRunningError),
        )
    }
}
