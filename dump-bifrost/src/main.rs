// Copyright (c) 2024 - Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use anyhow::Context;
use clap::Parser;
use restate_bifrost::loglet::{LogletOffset, LogletProvider};
use restate_bifrost::loglets::local_loglet::LocalLogletProvider;
use restate_bifrost::Record;
use restate_core::TaskCenterBuilder;
use restate_rocksdb::RocksDbManager;
use restate_types::config::Configuration;
use restate_types::config::NODE_BASE_DIR;
use restate_types::logs::metadata::LogletParams;
use restate_types::logs::{LogId, SequenceNumber};
use restate_wal_protocol::Envelope;
use std::path::PathBuf;
#[derive(Debug, clap::Parser)]
#[command(author, version, about, arg_required_else_help(true))]
struct DumpBifrostArguments {
    /// Directory containing the Restate data. Note, it needs to be the node base directory which
    /// is by default restate-data/<hostname>.
    #[arg(short, long = "path", value_name = "FILE")]
    path: PathBuf,

    /// Specifies the number of logs to dump. The tool will iterate over the range [0, logs).
    #[arg(short, long = "logs", default_value_t = 64)]
    logs: u64,
}

fn main() -> anyhow::Result<()> {
    let args = DumpBifrostArguments::parse();

    // update node base dir to point to correct directory
    NODE_BASE_DIR.get_or_init(|| args.path);

    let config = Configuration::default();
    restate_types::config::set_current_config(config);

    let config = Configuration::pinned();

    if !config.bifrost.local.data_dir().exists() {
        eprintln!(
            "The specified path '{}' does not contain a local-loglet directory.",
            NODE_BASE_DIR
                .get()
                .expect("path needs to be specified")
                .display()
        );
        std::process::exit(1);
    }

    let task_center = TaskCenterBuilder::default().build()?;
    task_center.block_on("main", None, async {
        let _rocksdb_manager =
            RocksDbManager::init(Configuration::mapped_updateable(|c| &c.common));

        let local_loglet_provider = LocalLogletProvider::new(
            &config.bifrost.local,
            Configuration::mapped_updateable(|c| &c.bifrost.local.rocksdb),
        )?;

        local_loglet_provider.start()?;

        for log_id in 0..args.logs {
            let log_id = LogId::from(log_id);

            // we assume a single log segment for the time being :-(
            let loglet = local_loglet_provider
                .get_loglet(&LogletParams(log_id.to_string()))
                .await
                .context(format!("Could not open local loglet with id {}", log_id))?;

            let mut offset = LogletOffset::INVALID;

            while let Some(record) = loglet.read_next_single_opt(offset).await? {
                if let Some(data) = record.record.payload() {
                    let envelope = Envelope::from_bytes(data.as_ref())?;
                    let decoded_log_record = DecodedLogRecord {
                        log_id,
                        offset: offset.into(),
                        envelope,
                    };
                    println!("{}", serde_json::to_string(&decoded_log_record)?);
                }

                let read_pointer = match &record.record {
                    Record::TrimGap(trim_gap) => trim_gap.until,
                    Record::Data(_) => record.offset,
                    Record::Seal(_) => record.offset,
                };

                offset = read_pointer;
            }
        }

        Ok(())
    })
}

#[derive(Debug, serde::Serialize)]
struct DecodedLogRecord {
    log_id: LogId,
    offset: u64,
    envelope: Envelope,
}
