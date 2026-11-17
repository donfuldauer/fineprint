// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Deterministic property-test harness (test builds only; REQ-CORE-001, REQ-CORE-006).
//!
//! A SplitMix64 generator with fixed literal seeds: no clock, no environment, no thread
//! randomness, so every run checks exactly the same inputs.

/// Edge values every property is checked against in addition to random inputs.
pub(crate) const EDGES: [i64; 23] = [
    0,
    1,
    -1,
    2,
    -2,
    50,
    -50,
    99,
    -99,
    100,
    -100,
    101,
    -101,
    i64::MIN,
    i64::MIN + 1,
    i64::MAX - 1,
    i64::MAX,
    150,
    -150,
    5,
    -5,
    10,
    -10,
];

/// SplitMix64 pseudo-random generator.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A random i64 drawn from a mix of magnitudes (full range, ~2^32, small), so that both
    /// overflowing and non-overflowing cases are frequent.
    pub(crate) fn next_i64(&mut self) -> i64 {
        let bits = self.next_u64();
        let v = i64::from_le_bytes(self.next_u64().to_le_bytes());
        match bits % 4 {
            0 => v,
            1 => v >> 31,
            2 => v >> 48,
            _ => v >> 56,
        }
    }
}

/// Every pair of edge values followed by `n` random pairs.
pub(crate) fn pairs(rng: &mut Rng, n: usize) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for &a in &EDGES {
        for &b in &EDGES {
            out.push((a, b));
        }
    }
    for _ in 0..n {
        let a = rng.next_i64();
        let b = rng.next_i64();
        out.push((a, b));
    }
    out
}
