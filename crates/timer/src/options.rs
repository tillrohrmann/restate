// Copyright (c) 2023 -  Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use crate::service::TimerService;
use serde_with::serde_as;
use std::fmt::Debug;

/// # Timer options
#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, derive_builder::Builder)]
#[cfg_attr(feature = "options_schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "options_schema", schemars(rename = "TimerOptions", default))]
#[builder(default)]
pub struct Options {
    /// # Num timers in memory limit
    ///
    /// The number of timers in memory limit is used to bound the amount of timers loaded in memory. If this limit is set, when exceeding it, the timers farther in the future will be spilled to disk.
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    #[cfg_attr(feature = "options_schema", schemars(with = "Option<usize>"))]
    num_timers_in_memory_limit: Option<usize>,
}

impl Options {
    pub fn build<Timer, TimerReader, Clock>(
        &self,
        timer_reader: TimerReader,
        clock: Clock,
    ) -> TimerService<Timer, Clock, TimerReader>
    where
        Timer: crate::Timer + Debug + Clone + 'static,
        TimerReader: crate::TimerReader<Timer> + Send + 'static,
        Clock: crate::Clock,
    {
        TimerService::new(clock, self.num_timers_in_memory_limit, timer_reader)
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            num_timers_in_memory_limit: Some(1024),
        }
    }
}
