// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use super::leadership::ActionEffect;
use crate::partition::services::non_deterministic::Effects as NBISEffects;
use crate::partition::state_machine::{AckCommand, Command};
use crate::util::IdentitySender;

/// Responsible for proposing [ActionEffect].
pub(super) struct ActionEffectHandler {
    proposal_tx: IdentitySender<AckCommand>,
}

impl ActionEffectHandler {
    pub(super) fn new(proposal_tx: IdentitySender<AckCommand>) -> Self {
        Self { proposal_tx }
    }

    pub(super) async fn handle(&self, actuator_output_buffer: &mut Vec<ActionEffect>) {
        let ack_command = if actuator_output_buffer.len() == 1 {
            AckCommand::no_ack(Self::map_action_to_command(
                actuator_output_buffer
                    .drain(..)
                    .next()
                    .expect("one element needs to be present"),
            ))
        } else {
            let commands = actuator_output_buffer
                .drain(..)
                .map(Self::map_action_to_command)
                .collect::<Vec<_>>();

            AckCommand::no_ack(commands)
        };

        // Err only if the consensus module is shutting down
        let _ = self.proposal_tx.send(ack_command).await;
    }

    fn map_action_to_command(action: ActionEffect) -> Command {
        match action {
            ActionEffect::Invoker(invoker_output) => Command::Invoker(invoker_output),
            ActionEffect::Shuffle(outbox_truncation) => {
                Command::OutboxTruncation(outbox_truncation.index())
            }
            ActionEffect::Timer(timer) => Command::Timer(timer),
            ActionEffect::BuiltInInvoker(invoker_output) => {
                let (fid, effects) = invoker_output.into_inner();
                Command::BuiltInInvoker(NBISEffects::new(fid, effects))
            }
        }
    }
}
