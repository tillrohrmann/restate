// Copyright (c) 2023 - 2026 Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Benchmark comparing standard Bloom filters vs Hybrid Ribbon bloom filters in RocksDB.
//!
//! Ribbon filters save ~30% filter memory compared to standard bloom filters at the same
//! false positive rate, at the cost of ~3-4x higher CPU during filter construction (which
//! happens during compaction, in the background).
//!
//! This benchmark measures:
//! 1. Point lookup latency for existing keys (positive lookups)
//! 2. Point lookup latency for non-existing keys (negative lookups — where filters matter most)
//! 3. Filter space savings (table reader memory and SST size)
//!
//! **Important notes on `optimize_filters_for_hits`:**
//! Restate sets this flag, which skips filter construction for the bottommost LSM level.
//! This is beneficial when most lookups are hits (saves filter memory for the largest level),
//! but it means that negative lookups at the bottom level always hit disk. For this benchmark,
//! we test two scenarios:
//! - **with_bottommost_filters=false** (Restate's production config): Filters only at non-bottom levels
//! - **with_bottommost_filters=true** (all levels have filters): Shows full filter impact
//!
//! Run with: cargo bench -p restate-partition-store --bench bloom_filter_benchmark
//!
//! For more verbose output: cargo bench -p restate-partition-store --bench bloom_filter_benchmark -- --nocapture

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rocksdb::{BlockBasedOptions, DB, DBCompressionType, Options};
use tempfile::TempDir;

/// Number of unique keys to populate in each DB instance.
const NUM_KEYS: usize = 500_000;

/// Number of lookups per benchmark iteration.
const LOOKUPS_PER_ITER: usize = 1_000;

/// Key prefix length matching Restate's DB_PREFIX_LENGTH (2 bytes KeyKind + 8 bytes partition key).
const PREFIX_LENGTH: usize = 10;

/// Number of write waves. Each wave flushes to create separate L0 files that get compacted
/// to lower levels, creating a realistic multi-level LSM tree shape.
const NUM_WAVES: usize = 5;

#[derive(Clone, Copy)]
enum FilterType {
    Bloom,
    HybridRibbon,
}

impl std::fmt::Display for FilterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterType::Bloom => write!(f, "bloom"),
            FilterType::HybridRibbon => write!(f, "hybrid-ribbon"),
        }
    }
}

/// Build a key mimicking Restate's key layout: [2-byte kind][8-byte entity id][suffix].
fn make_key(kind: &[u8; 2], entity_id: u64, suffix: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(PREFIX_LENGTH + 4);
    key.extend_from_slice(kind);
    key.extend_from_slice(&entity_id.to_be_bytes());
    key.extend_from_slice(&suffix.to_be_bytes());
    key
}

/// Create a RocksDB instance configured to match Restate's partition-store settings.
///
/// - `filter_type`: Bloom or HybridRibbon
/// - `bottommost_filters`: if false, sets `optimize_filters_for_hits(true)` which skips
///   filter construction at the bottommost level (Restate's production setting).
fn open_db(filter_type: FilterType, bottommost_filters: bool) -> (DB, TempDir) {
    let tmp = TempDir::new().expect("failed to create temp dir");

    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_level_compaction_dynamic_level_bytes(true);
    opts.set_num_levels(7);
    opts.set_compression_per_level(&[
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
        DBCompressionType::Zstd,
    ]);

    // Use a small memtable budget to create many L0 files and push data into lower levels.
    let memtables_budget: usize = 4 * 1024 * 1024; // 4 MiB
    opts.set_write_buffer_size(memtables_budget / 4);
    opts.set_min_write_buffer_number_to_merge(2);
    opts.set_max_write_buffer_number(4);
    opts.set_level_zero_file_num_compaction_trigger(2);
    opts.set_target_file_size_base(memtables_budget as u64 / 8);
    opts.set_max_bytes_for_level_base(memtables_budget as u64);

    // Prefix extractor matching Restate's 10-byte prefix.
    opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(PREFIX_LENGTH));
    opts.set_memtable_prefix_bloom_ratio(0.2);
    opts.set_memtable_whole_key_filtering(true);

    // When bottommost_filters=false (production mode), filters are skipped at the bottom
    // level. When true, all levels get filters (useful for seeing the full filter impact).
    if !bottommost_filters {
        opts.set_optimize_filters_for_hits(true);
    }

    // Block-based table options matching Restate's defaults.
    let mut block_opts = BlockBasedOptions::default();
    match filter_type {
        FilterType::Bloom => block_opts.set_bloom_filter(10.0, true),
        // bloom_before_level=1: Bloom for L0, Ribbon for L1+
        FilterType::HybridRibbon => block_opts.set_hybrid_ribbon_filter(10.0, 1),
    }
    block_opts.set_format_version(6);
    block_opts.set_optimize_filters_for_memory(true);
    block_opts.set_index_block_restart_interval(4);
    block_opts.set_cache_index_and_filter_blocks(true);
    block_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
    block_opts.set_block_size(64 * 1024); // 64 KiB

    opts.set_block_based_table_factory(&block_opts);

    // Enable statistics to read filter metrics later.
    opts.enable_statistics();

    let db = DB::open(&opts, tmp.path()).expect("failed to open DB");
    (db, tmp)
}

/// Populate the DB in multiple waves to create a realistic multi-level LSM tree.
/// Each wave writes a subset of keys, flushes, and waits for background compactions.
fn populate(db: &DB) {
    let kinds: &[[u8; 2]] = &[*b"st", *b"de", *b"ib", *b"iv", *b"jn"];
    let value = vec![0xABu8; 100]; // 100-byte values for realistic SST sizes

    let keys_per_wave = NUM_KEYS / NUM_WAVES;
    let mut key_counter = 0usize;

    for _wave in 0..NUM_WAVES {
        let mut batch = rocksdb::WriteBatch::default();
        for _ in 0..keys_per_wave {
            let kind_idx = key_counter % kinds.len();
            let entity_id = key_counter as u64;
            let key = make_key(&kinds[kind_idx], entity_id, 0);
            batch.put(&key, &value);
            key_counter += 1;

            if batch.len() >= 5_000 {
                db.write(&batch).expect("write failed");
                batch.clear();
            }
        }
        if !batch.is_empty() {
            db.write(&batch).expect("write failed");
        }
        db.flush().expect("flush failed");
    }

    // Wait for compactions to settle. This gives the background compaction threads
    // time to move data from L0 into lower levels.
    let wait_opts = rocksdb::WaitForCompactOptions::default();
    db.wait_for_compact(&wait_opts)
        .expect("wait for compaction failed");
}

/// Print SST file statistics for each level.
fn print_sst_stats(db: &DB, label: &str) {
    let props = [
        "rocksdb.estimate-table-readers-mem",
        "rocksdb.num-files-at-level0",
        "rocksdb.num-files-at-level1",
        "rocksdb.num-files-at-level2",
        "rocksdb.num-files-at-level3",
        "rocksdb.num-files-at-level4",
        "rocksdb.num-files-at-level5",
        "rocksdb.num-files-at-level6",
        "rocksdb.estimate-num-keys",
        "rocksdb.total-sst-files-size",
    ];

    eprintln!("\n=== SST Statistics for {label} ===");
    for prop in &props {
        if let Some(value) = db.property_value(*prop).ok().flatten() {
            eprintln!("  {prop}: {value}");
        }
    }

    // Filter-specific counters from statistics.
    if let Some(stats_str) = db.property_value("rocksdb.stats").ok().flatten() {
        let filter_keywords = [
            "rocksdb.bloom.filter.useful",
            "rocksdb.bloom.filter.full.positive",
            "rocksdb.bloom.filter.full.true.positive",
        ];
        for keyword in &filter_keywords {
            for line in stats_str.lines() {
                if line.contains(keyword) {
                    eprintln!("  {}", line.trim());
                    break;
                }
            }
        }
    }
    eprintln!();
}

/// Pre-compute lookup keys to avoid allocation noise during benchmark iterations.
struct LookupKeys {
    positive: Vec<Vec<u8>>,
    negative: Vec<Vec<u8>>,
}

fn prepare_lookup_keys() -> LookupKeys {
    let kinds: &[[u8; 2]] = &[*b"st", *b"de", *b"ib", *b"iv", *b"jn"];

    // Positive lookups: keys that exist (spread across kinds and entity IDs).
    let step = NUM_KEYS / LOOKUPS_PER_ITER;
    let positive: Vec<Vec<u8>> = (0..LOOKUPS_PER_ITER)
        .map(|i| {
            let key_counter = i * step;
            let kind_idx = key_counter % kinds.len();
            let entity_id = key_counter as u64;
            make_key(&kinds[kind_idx], entity_id, 0)
        })
        .collect();

    // Negative lookups: keys that definitely don't exist (suffix=999 was never written).
    let negative: Vec<Vec<u8>> = (0..LOOKUPS_PER_ITER)
        .map(|i| {
            let key_counter = i * step;
            let kind_idx = key_counter % kinds.len();
            let entity_id = key_counter as u64;
            make_key(&kinds[kind_idx], entity_id, 999)
        })
        .collect();

    LookupKeys { positive, negative }
}

/// Label for a benchmark variant.
fn variant_label(filter_type: FilterType, bottommost_filters: bool) -> String {
    let filter_name = match filter_type {
        FilterType::Bloom => "bloom",
        FilterType::HybridRibbon => "hybrid-ribbon",
    };
    let mode = if bottommost_filters {
        "all-levels"
    } else {
        "skip-bottom"
    };
    format!("{filter_name}/{mode}")
}

fn bloom_filter_benchmark(c: &mut Criterion) {
    let lookup_keys = prepare_lookup_keys();

    let variants: Vec<(FilterType, bool)> = vec![
        (FilterType::Bloom, true),         // Bloom, all levels have filters
        (FilterType::HybridRibbon, true),  // Hybrid ribbon, all levels have filters
        (FilterType::Bloom, false),        // Bloom, skip bottom (production mode)
        (FilterType::HybridRibbon, false), // Hybrid ribbon, skip bottom (production mode)
    ];

    // Open and populate databases for each variant.
    let dbs: Vec<_> = variants
        .iter()
        .map(|(ft, bottommost)| {
            let label = variant_label(*ft, *bottommost);
            let (db, tmp) = open_db(*ft, *bottommost);
            populate(&db);
            print_sst_stats(&db, &label);
            (label, db, tmp)
        })
        .collect();

    // Benchmark positive lookups (key exists).
    {
        let mut group = c.benchmark_group("point_lookup_positive");
        group.sample_size(50);
        for (label, db, _) in &dbs {
            group.bench_with_input(BenchmarkId::new("filter", label), label, |b, _| {
                b.iter(|| {
                    for key in &lookup_keys.positive {
                        black_box(db.get(key).expect("get failed"));
                    }
                });
            });
        }
        group.finish();
    }

    // Benchmark negative lookups (key doesn't exist — where bloom filters matter most).
    {
        let mut group = c.benchmark_group("point_lookup_negative");
        group.sample_size(50);
        for (label, db, _) in &dbs {
            group.bench_with_input(BenchmarkId::new("filter", label), label, |b, _| {
                b.iter(|| {
                    for key in &lookup_keys.negative {
                        black_box(db.get(key).expect("get failed"));
                    }
                });
            });
        }
        group.finish();
    }

    // Benchmark mixed workload (80% negative, 20% positive — realistic worst case).
    {
        let mut group = c.benchmark_group("point_lookup_mixed_80neg_20pos");
        group.sample_size(50);
        for (label, db, _) in &dbs {
            group.bench_with_input(BenchmarkId::new("filter", label), label, |b, _| {
                b.iter(|| {
                    // 80% negative
                    for key in lookup_keys.negative.iter().take(LOOKUPS_PER_ITER * 4 / 5) {
                        black_box(db.get(key).expect("get failed"));
                    }
                    // 20% positive
                    for key in lookup_keys.positive.iter().take(LOOKUPS_PER_ITER / 5) {
                        black_box(db.get(key).expect("get failed"));
                    }
                });
            });
        }
        group.finish();
    }

    // Print final stats after benchmarking (filter hit/miss counters will have accumulated).
    for (label, db, _) in &dbs {
        print_sst_stats(db, label);
    }
}

criterion_group!(benches, bloom_filter_benchmark);
criterion_main!(benches);
