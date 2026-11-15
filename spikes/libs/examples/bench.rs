// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T7 benchmark printer (REQ-NFR-009: median of repeated runs; never asserted, R7).
//!
//! Run: `cargo run --release -p fineprint-spike-libs --example bench`
//! Soft SHA-256 backend: add `RUSTFLAGS='--cfg sha2_backend="soft"'`.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

use fineprint_spike_libs::merkle::{
    Blake3Hasher, Component, MerkleHasher, MerkleTree, Sha256Hasher, State, encoded_len,
    synthetic_state,
};
use fineprint_spike_libs::prng::Pcg64Dxsm;
use rand_core::Rng;
use serde::{Deserialize, Serialize};

const SAMPLES: usize = 51;

/// Median over `SAMPLES` samples of the time per operation, in picoseconds.
fn median_ps(ops_per_sample: u128, mut sample: impl FnMut()) -> u128 {
    let mut times: Vec<u128> = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            sample();
            start.elapsed().as_nanos() * 1000 / ops_per_sample
        })
        .collect();
    times.sort_unstable();
    times[SAMPLES / 2]
}

fn fmt_ns(ps: u128) -> String {
    format!("{}.{:03} ns", ps / 1000, ps % 1000)
}

/// Bytes per picosecond * 10^6 = MB/s (10^6 bytes per second).
fn mb_per_s(bytes: u128, ps: u128) -> u128 {
    bytes * 1_000_000 / ps.max(1)
}

fn bump(state: &mut State, id: u64) {
    if let Some(permanent) = state.battlefield.get_mut(&id) {
        permanent.damage += 1;
    }
}

fn merkle<H: MerkleHasher>(name: &str) {
    let mut state = synthetic_state(0x5eed_0004, 200);
    let payload = encoded_len(&state) as u128;
    let full = median_ps(20, || {
        for _ in 0..20 {
            black_box(MerkleTree::<H>::full(black_box(&state)).root());
        }
    });
    println!(
        "{name} merkle full (200 permanents, {payload} payload bytes): {} median, {} MB/s payload, budget 2 ms: {}",
        fmt_ns(full),
        mb_per_s(payload, full),
        if full <= 2_000_000_000 {
            "PASS"
        } else {
            "MISS"
        }
    );
    for changes in [1_u64, 10] {
        let mut tree = MerkleTree::<H>::full(&state);
        let mut next = 0_u64;
        let ps = median_ps(200, || {
            for _ in 0..200 {
                let dirty: Vec<Component> = (0..changes)
                    .map(|_| {
                        next = (next + 37) % 200;
                        bump(&mut state, 100 + next);
                        Component::Permanent(100 + next)
                    })
                    .collect();
                tree.update(&state, &dirty);
                black_box(tree.root());
            }
        });
        println!(
            "{name} merkle incremental ({changes} changed, includes mutation): {} median, budget 2 ms: {}",
            fmt_ns(ps),
            if ps <= 2_000_000_000 { "PASS" } else { "MISS" }
        );
    }
}

fn raw_throughput<H: MerkleHasher>(name: &str) {
    let buffer: Vec<u8> = (0..1_048_576_u32).map(|i| (i % 251) as u8).collect();
    let ps = median_ps(1, || {
        let mut hasher = H::default();
        hasher.update(black_box(&buffer));
        black_box(hasher.finish());
    });
    println!(
        "{name} raw 1 MiB: {} median, {} MB/s",
        fmt_ns(ps),
        mb_per_s(1_048_576, ps)
    );
}

#[derive(Clone, Serialize, Deserialize)]
enum Kind {
    Plain,
    Tagged(u32),
    Linked { to: u64, slot: u8 },
}

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    id: u64,
    def: String,
    power: i64,
    toughness: i64,
    tapped: bool,
    attachments: Vec<u64>,
    kind: Kind,
}

fn snapshots() -> Vec<Snapshot> {
    (0..200_u64)
        .map(|i| Snapshot {
            id: 100 + i,
            def: format!("test.perm_{i:04}"),
            power: (i % 7) as i64,
            toughness: (i % 5) as i64 + 1,
            tapped: i % 3 == 0,
            attachments: (0..i % 3).collect(),
            kind: match i % 3 {
                0 => Kind::Plain,
                1 => Kind::Tagged(i as u32),
                _ => Kind::Linked { to: i, slot: 2 },
            },
        })
        .collect()
}

fn machine() -> String {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines()
                .find(|line| line.starts_with("model name"))
                .and_then(|line| line.split(':').nth(1))
                .map(|model| model.trim().to_string())
        })
        .unwrap_or_else(|| "unknown cpu".to_string());
    format!(
        "{} {} / {cpu} / {} threads available",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, usize::from)
    )
}

fn main() {
    println!("machine: {}", machine());
    println!("samples per figure: {SAMPLES} (median)");

    let mut rng = Pcg64Dxsm::new(1, 2);
    let ps = median_ps(1_000_000, || {
        for _ in 0..1_000_000 {
            black_box(rng.next_u64());
        }
    });
    println!("prng in-house PCG64-DXSM: {} per u64", fmt_ns(ps));
    let mut candidate = rand_pcg::Pcg64Dxsm::new(1, 2);
    let ps = median_ps(1_000_000, || {
        for _ in 0..1_000_000 {
            black_box(candidate.next_u64());
        }
    });
    println!("prng rand_pcg Pcg64Dxsm: {} per u64", fmt_ns(ps));

    merkle::<Sha256Hasher>("sha256");
    merkle::<Blake3Hasher>("blake3");
    raw_throughput::<Sha256Hasher>("sha256");
    raw_throughput::<Blake3Hasher>("blake3");

    let values = snapshots();
    let named = rmp_serde::to_vec_named(&values).unwrap_or_default();
    let compact = rmp_serde::to_vec(&values).unwrap_or_default();
    let json = serde_json::to_vec(&values).unwrap_or_default();
    println!(
        "messagepack 200 snapshots: named {} bytes, compact {} bytes, json {} bytes",
        named.len(),
        compact.len(),
        json.len()
    );
    let ps = median_ps(100, || {
        for _ in 0..100 {
            black_box(fineprint_spike_libs::transport::encode(black_box(&values)).ok());
        }
    });
    println!("messagepack encode named: {}", fmt_ns(ps));
    let ps = median_ps(100, || {
        for _ in 0..100 {
            black_box(
                fineprint_spike_libs::transport::decode::<Vec<Snapshot>>(black_box(&named)).ok(),
            );
        }
    });
    println!("messagepack decode named: {}", fmt_ns(ps));

    let document: BTreeMap<String, serde_json::Value> = values
        .iter()
        .map(|snapshot| {
            (
                snapshot.def.clone(),
                serde_json::to_value(snapshot).unwrap_or_default(),
            )
        })
        .collect();
    let value = serde_json::to_value(&document).unwrap_or_default();
    let ps = median_ps(100, || {
        for _ in 0..100 {
            black_box(serde_json_canonicalizer::to_vec(black_box(&value)).ok());
        }
    });
    println!(
        "jcs serde_json_canonicalizer ({} bytes): {}",
        json.len(),
        fmt_ns(ps)
    );
    let ps = median_ps(100, || {
        for _ in 0..100 {
            black_box(serde_jcs::to_vec(black_box(&value)).ok());
        }
    });
    println!("jcs serde_jcs: {}", fmt_ns(ps));
}
