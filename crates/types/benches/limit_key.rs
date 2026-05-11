// Copyright (c) 2023 - 2026 Restate Software, Inc., Restate GmbH.
// All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Bench comparing wire-format candidates for `LimitKey` in the service protocol.
//!
//! The decision: should `CallCommandMessage.limit_key` stay as `optional string`
//! (currently formatted as `"level1"` / `"level1/level2"` and parsed on decode)
//! or move to a structured Protobuf message? See the TODO at
//! `service-protocol/dev/restate/service/protocol.proto:496`.
//!
//! Per-component `RestrictedValue` validation is unavoidable in every variant
//! (untrusted SDK input), so the candidate variants differ only in framing.

use std::hint::black_box;
use std::str::FromStr;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use prost::Message;

use restate_types::limit_key::LimitKey;
use restate_util_string::{ReString, RestrictedValue};

// --- ad-hoc proto messages used only by this bench -------------------------

#[derive(Clone, PartialEq, prost::Message)]
struct StringForm {
    #[prost(string, optional, tag = "1")]
    limit_key: Option<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct StructuredLimitKey {
    #[prost(string, tag = "1")]
    level1: String,
    #[prost(string, optional, tag = "2")]
    level2: Option<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct StructuredForm {
    #[prost(message, optional, tag = "1")]
    limit_key: Option<StructuredLimitKey>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct RepeatedForm {
    #[prost(string, repeated, tag = "1")]
    levels: Vec<String>,
}

// --- encoders: LimitKey<ReString> -> bytes ---------------------------------

fn encode_string(key: &LimitKey<ReString>) -> Vec<u8> {
    let s = if key.is_none() {
        None
    } else {
        Some(key.to_string())
    };
    StringForm { limit_key: s }.encode_to_vec()
}

fn encode_structured(key: &LimitKey<ReString>) -> Vec<u8> {
    let inner = match key {
        LimitKey::None => None,
        LimitKey::L1(l1) => Some(StructuredLimitKey {
            level1: l1.as_str().to_string(),
            level2: None,
        }),
        LimitKey::L2(l1, l2) => Some(StructuredLimitKey {
            level1: l1.as_str().to_string(),
            level2: Some(l2.as_str().to_string()),
        }),
    };
    StructuredForm { limit_key: inner }.encode_to_vec()
}

fn encode_repeated(key: &LimitKey<ReString>) -> Vec<u8> {
    let levels = match key {
        LimitKey::None => Vec::new(),
        LimitKey::L1(l1) => vec![l1.as_str().to_string()],
        LimitKey::L2(l1, l2) => vec![l1.as_str().to_string(), l2.as_str().to_string()],
    };
    RepeatedForm { levels }.encode_to_vec()
}

// --- decoders: bytes -> LimitKey<ReString> ---------------------------------

fn decode_string(bytes: &[u8]) -> LimitKey<ReString> {
    let form = StringForm::decode(bytes).expect("decode");
    match form.limit_key {
        None => LimitKey::None,
        Some(s) => s.parse().expect("parse limit key"),
    }
}

fn decode_structured(bytes: &[u8]) -> LimitKey<ReString> {
    let form = StructuredForm::decode(bytes).expect("decode");
    match form.limit_key {
        None => LimitKey::None,
        Some(StructuredLimitKey {
            level1,
            level2: None,
        }) => LimitKey::L1(RestrictedValue::<ReString>::from_str(&level1).expect("level1")),
        Some(StructuredLimitKey {
            level1,
            level2: Some(level2),
        }) => LimitKey::L2(
            RestrictedValue::<ReString>::from_str(&level1).expect("level1"),
            RestrictedValue::<ReString>::from_str(&level2).expect("level2"),
        ),
    }
}

fn decode_repeated(bytes: &[u8]) -> LimitKey<ReString> {
    let form = RepeatedForm::decode(bytes).expect("decode");
    match form.levels.len() {
        0 => LimitKey::None,
        1 => LimitKey::L1(RestrictedValue::<ReString>::from_str(&form.levels[0]).expect("level1")),
        2 => LimitKey::L2(
            RestrictedValue::<ReString>::from_str(&form.levels[0]).expect("level1"),
            RestrictedValue::<ReString>::from_str(&form.levels[1]).expect("level2"),
        ),
        n => panic!("too many levels: {n}"),
    }
}

// --- workloads -------------------------------------------------------------

fn workloads() -> Vec<(&'static str, LimitKey<ReString>)> {
    let max_component: String = "a".repeat(RestrictedValue::<ReString>::MAX_LEN);
    let max_l1 = RestrictedValue::<ReString>::from_str(&max_component).expect("max l1");
    let max_l2 = RestrictedValue::<ReString>::from_str(&max_component).expect("max l2");

    vec![
        ("none", LimitKey::None),
        ("l1_short", "tenant1".parse().expect("l1_short")),
        ("l2_short", "tenant1/region2".parse().expect("l2_short")),
        ("l2_max", LimitKey::L2(max_l1, max_l2)),
    ]
}

// --- benches ---------------------------------------------------------------

pub fn limit_key_bench(c: &mut Criterion) {
    let cases = workloads();

    // print encoded sizes once for context (not timed)
    eprintln!("\n--- limit_key encoded sizes (bytes) ---");
    eprintln!(
        "{:<10} {:>8} {:>12} {:>10}",
        "workload", "string", "structured", "repeated"
    );
    for (name, key) in &cases {
        eprintln!(
            "{:<10} {:>8} {:>12} {:>10}",
            name,
            encode_string(key).len(),
            encode_structured(key).len(),
            encode_repeated(key).len(),
        );
    }
    eprintln!();

    let mut encode_group = c.benchmark_group("limit_key/encode");
    for (name, key) in &cases {
        encode_group.bench_with_input(BenchmarkId::new("string", name), key, |b, k| {
            b.iter(|| encode_string(black_box(k)))
        });
        encode_group.bench_with_input(BenchmarkId::new("structured", name), key, |b, k| {
            b.iter(|| encode_structured(black_box(k)))
        });
        encode_group.bench_with_input(BenchmarkId::new("repeated", name), key, |b, k| {
            b.iter(|| encode_repeated(black_box(k)))
        });
    }
    encode_group.finish();

    let mut decode_group = c.benchmark_group("limit_key/decode");
    for (name, key) in &cases {
        let string_bytes = encode_string(key);
        let structured_bytes = encode_structured(key);
        let repeated_bytes = encode_repeated(key);

        decode_group.bench_with_input(BenchmarkId::new("string", name), &string_bytes, |b, bs| {
            b.iter(|| decode_string(black_box(bs.as_slice())))
        });
        decode_group.bench_with_input(
            BenchmarkId::new("structured", name),
            &structured_bytes,
            |b, bs| b.iter(|| decode_structured(black_box(bs.as_slice()))),
        );
        decode_group.bench_with_input(
            BenchmarkId::new("repeated", name),
            &repeated_bytes,
            |b, bs| b.iter(|| decode_repeated(black_box(bs.as_slice()))),
        );
    }
    decode_group.finish();

    let mut roundtrip_group = c.benchmark_group("limit_key/roundtrip");
    for (name, key) in &cases {
        roundtrip_group.bench_with_input(BenchmarkId::new("string", name), key, |b, k| {
            b.iter(|| {
                let bytes = encode_string(black_box(k));
                decode_string(black_box(&bytes))
            })
        });
        roundtrip_group.bench_with_input(BenchmarkId::new("structured", name), key, |b, k| {
            b.iter(|| {
                let bytes = encode_structured(black_box(k));
                decode_structured(black_box(&bytes))
            })
        });
        roundtrip_group.bench_with_input(BenchmarkId::new("repeated", name), key, |b, k| {
            b.iter(|| {
                let bytes = encode_repeated(black_box(k));
                decode_repeated(black_box(&bytes))
            })
        });
    }
    roundtrip_group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = limit_key_bench
);

criterion_main!(benches);
