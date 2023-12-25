// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use crate::partition::services::non_deterministic::Effects as NBISEffects;
use crate::partition::types::{InvokerEffect, TimerValue};
use restate_types::identifiers::{IngressDispatcherId, PartitionId, PeerId};
use restate_types::invocation::{InvocationResponse, InvocationTermination, ServiceInvocation};
use restate_types::message::{AckKind, MessageIndex};

/// Envelope for [`partition::Command`] that might require an explicit acknowledge.
#[derive(Debug)]
pub struct AckCommand {
    cmds: Commands,
    ack_mode: AckMode,
}

#[derive(Debug)]
pub enum AckMode {
    Ack(AckTarget),
    Dedup(DeduplicationSource),
    None,
}

impl AckCommand {
    /// Create a command that requires an acknowledgement upon reception.
    pub fn ack(cmds: impl Into<Commands>, ack_target: AckTarget) -> Self {
        Self {
            cmds: cmds.into(),
            ack_mode: AckMode::Ack(ack_target),
        }
    }

    /// Create a command that should be de-duplicated with respect to the `producer_id` and the
    /// `seq_number` by the receiver.
    pub fn dedup(cmds: impl Into<Commands>, deduplication_source: DeduplicationSource) -> Self {
        Self {
            cmds: cmds.into(),
            ack_mode: AckMode::Dedup(deduplication_source),
        }
    }

    /// Create a command that should not be acknowledged.
    pub fn no_ack(cmds: impl Into<Commands>) -> Self {
        Self {
            cmds: cmds.into(),
            ack_mode: AckMode::None,
        }
    }

    pub fn into_inner(self) -> (Commands, AckMode) {
        (self.cmds, self.ack_mode)
    }
}

#[derive(Debug)]
pub enum DeduplicationSource {
    Shuffle {
        producing_partition_id: PartitionId,
        shuffle_id: PeerId,
        seq_number: MessageIndex,
    },
    Ingress {
        ingress_dispatcher_id: IngressDispatcherId,
        // String used to distinguish between different seq_numbers indexes produced by the ingress
        source_id: String,
        seq_number: MessageIndex,
    },
}

impl DeduplicationSource {
    pub fn shuffle(
        shuffle_id: PeerId,
        producing_partition_id: PartitionId,
        seq_number: MessageIndex,
    ) -> Self {
        DeduplicationSource::Shuffle {
            shuffle_id,
            producing_partition_id,
            seq_number,
        }
    }

    pub fn ingress(
        ingress_dispatcher_id: IngressDispatcherId,
        source_id: String,
        seq_number: MessageIndex,
    ) -> Self {
        DeduplicationSource::Ingress {
            ingress_dispatcher_id,
            source_id,
            seq_number,
        }
    }

    pub(crate) fn acknowledge(self) -> AckResponse {
        match self {
            DeduplicationSource::Shuffle {
                shuffle_id,
                seq_number,
                ..
            } => AckResponse::Shuffle(ShuffleDeduplicationResponse {
                shuffle_target: shuffle_id,
                kind: AckKind::Acknowledge(seq_number),
            }),
            DeduplicationSource::Ingress {
                ingress_dispatcher_id,
                seq_number,
                source_id,
            } => AckResponse::Ingress(IngressAckResponse {
                _ingress_dispatcher_id: ingress_dispatcher_id,
                dedup_source: Some(source_id),
                kind: AckKind::Acknowledge(seq_number),
            }),
        }
    }

    pub fn duplicate(self, last_known_seq_number: MessageIndex) -> AckResponse {
        match self {
            DeduplicationSource::Shuffle {
                shuffle_id,
                seq_number,
                ..
            } => AckResponse::Shuffle(ShuffleDeduplicationResponse {
                shuffle_target: shuffle_id,
                kind: AckKind::Duplicate {
                    seq_number,
                    last_known_seq_number,
                },
            }),
            DeduplicationSource::Ingress {
                ingress_dispatcher_id,
                seq_number,
                source_id,
            } => AckResponse::Ingress(IngressAckResponse {
                _ingress_dispatcher_id: ingress_dispatcher_id,
                dedup_source: Some(source_id),
                kind: AckKind::Duplicate {
                    seq_number,
                    last_known_seq_number,
                },
            }),
        }
    }
}

#[derive(Debug)]
pub enum AckTarget {
    Ingress {
        ingress_dispatcher_id: IngressDispatcherId,
        seq_number: MessageIndex,
    },
}

impl AckTarget {
    pub fn ingress(ingress_dispatcher_id: IngressDispatcherId, seq_number: MessageIndex) -> Self {
        AckTarget::Ingress {
            ingress_dispatcher_id,
            seq_number,
        }
    }

    pub fn acknowledge(self) -> AckResponse {
        match self {
            AckTarget::Ingress {
                ingress_dispatcher_id,
                seq_number,
            } => AckResponse::Ingress(IngressAckResponse {
                _ingress_dispatcher_id: ingress_dispatcher_id,
                dedup_source: None,
                kind: AckKind::Acknowledge(seq_number),
            }),
        }
    }
}

#[derive(Debug)]
pub enum AckResponse {
    Shuffle(ShuffleDeduplicationResponse),
    Ingress(IngressAckResponse),
}

#[derive(Debug)]
pub struct ShuffleDeduplicationResponse {
    pub(crate) shuffle_target: PeerId,
    pub(crate) kind: AckKind,
}

#[derive(Debug)]
pub struct IngressAckResponse {
    pub(crate) _ingress_dispatcher_id: IngressDispatcherId,
    pub(crate) dedup_source: Option<String>,
    pub(crate) kind: AckKind,
}

/// State machine input commands
#[derive(Debug)]
pub enum Command {
    TerminateInvocation(InvocationTermination),
    Invoker(InvokerEffect),
    Timer(TimerValue),
    OutboxTruncation(MessageIndex),
    Invocation(ServiceInvocation),
    Response(InvocationResponse),
    BuiltInInvoker(NBISEffects),
}

#[derive(Debug)]
pub enum Commands {
    Single(Command),
    Multiple(Vec<Command>),
}

impl From<Command> for Commands {
    fn from(command: Command) -> Self {
        Commands::Single(command)
    }
}

impl From<Vec<Command>> for Commands {
    fn from(commands: Vec<Command>) -> Self {
        Commands::Multiple(commands)
    }
}

impl IntoIterator for Commands {
    type Item = Command;
    type IntoIter = CommandsIter;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Commands::Single(command) => CommandsIter::Single(std::iter::once(command)),
            Commands::Multiple(commands) => CommandsIter::Multiple(commands.into_iter()),
        }
    }
}

pub enum CommandsIter {
    Single(std::iter::Once<Command>),
    Multiple(std::vec::IntoIter<Command>),
}

impl Iterator for CommandsIter {
    type Item = Command;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CommandsIter::Single(iter) => iter.next(),
            CommandsIter::Multiple(iter) => iter.next(),
        }
    }
}
