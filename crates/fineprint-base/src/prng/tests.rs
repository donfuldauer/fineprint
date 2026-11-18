// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the PRNG module (REQ-CORE-001, REQ-API-004).
//!
//! Every expected value is hard-coded, so the same assertions run on Linux, Windows and macOS
//! CI and pin the frozen output. Arithmetic is integer only.

use super::{ALGORITHM_ID, Pcg64Dxsm, SplitMix64};
use crate::ids::CanonicalEncode;

/// A NumPy-published PCG64-DXSM vector.
struct Vector {
    initstate: u128,
    initseq: u128,
    seeded_state: u128,
    seeded_increment: u128,
    first16: [u64; 16],
    row999: u64,
}

/// NumPy `pcg64dxsm-testset-1.csv` (seed 0xdeadbeaf), rows 0..=15 and 999.
const DEADBEAF: Vector = Vector {
    initstate: 0x50312a63d516e24a_4063bc8bca44dc77,
    initseq: 0xb780f022ab4ec9e2_2056086e8249885a,
    seeded_state: 0xdcbf51eca8a06d6da11c0e6f95be4591,
    seeded_increment: 0x6f01e045569d93c440ac10dd049310b5,
    first16: [
        0xdf1ddcf1e22521fe,
        0xc71b2f9c706cf151,
        0x6922a8cc24ad96b2,
        0x82738c549beccc30,
        0x5e8415cdb1f17580,
        0x64c54ad0c09cb43,
        0x361a17a607dce278,
        0x4346f6afb7acad68,
        0x6e9f14d4f6398d6b,
        0xf818d4343f8ed822,
        0x6327647daf508ed6,
        0xe1d1dbe5496a262a,
        0xfc081e619076b2e0,
        0x37126563a956ab1,
        0x8bb46e155db16b9,
        0x56449f006c9f3fb4,
    ],
    row999: 0x4955c92aaa231578,
};

/// NumPy `pcg64dxsm-testset-2.csv` (seed 0x0), rows 0..=15 and 999.
const ZERO: Vector = Vector {
    initstate: 0xdb2cd7e7b0f478be_abf4641a2c71ba49,
    initseq: 0x20c6ed6d9d7b8d41_2c4099de223c39d4,
    seeded_state: 0x1aa1b5345996452d09585eb7a69561e3,
    seeded_increment: 0x418ddadb3af71a82588133bc447873a9,
    first16: [
        0xd97e4a147f788a70,
        0x8dfa7bce56e3a253,
        0x13556ed9f53d3c10,
        0x55dbf1c241341e98,
        0xa2cd98f722eb0e0a,
        0x83dfc407203ade8,
        0xeaa083df518f030d,
        0x44968c87e432852b,
        0x573107b9cb8d9ecc,
        0x9eedd1da50b9daca,
        0xb33a6735ca451e3c,
        0x72830d2b39677262,
        0x9da8c512fd0207e8,
        0x1fc5c91954a2672b,
        0xd33479437116e08,
        0x9ccdd9390cee46f3,
    ],
    row999: 0xc0a277e3b2bfc3de,
};

fn first_four(rng: &mut Pcg64Dxsm) -> [u64; 4] {
    std::array::from_fn(|_| rng.next_u64())
}

fn hi64(v: u128) -> u64 {
    (v >> 64) as u64
}

fn lo64(v: u128) -> u64 {
    v as u64
}

/// REQ-CORE-001: the algorithm ID recorded in replays and state.
#[test]
fn req_core_001_algorithm_id_is_pcg64_dxsm() {
    assert_eq!(ALGORITHM_ID, "PCG64-DXSM");
}

/// T1, REQ-CORE-001: SplitMix64 reference outputs (vector a).
#[test]
fn req_core_001_splitmix64_vectors() {
    let mut a = SplitMix64::new(1_234_567);
    let got: [u64; 5] = std::array::from_fn(|_| a.next_u64());
    assert_eq!(
        got,
        [
            6457827717110365317,
            3203168211198807973,
            9817491932198370423,
            4593380528125082431,
            16408922859458223821,
        ]
    );
    let mut b = SplitMix64::new(0);
    let got: [u64; 3] = std::array::from_fn(|_| b.next_u64());
    assert_eq!(
        got,
        [0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f]
    );
    let c = SplitMix64::new(9);
    assert_eq!(c.clone(), c);
}

/// T2, REQ-CORE-001: NumPy-published PCG64-DXSM vectors (vector b), both seeds.
#[test]
fn req_core_001_pcg64_dxsm_matches_numpy_published_vectors() {
    for vector in [DEADBEAF, ZERO] {
        let mut rng = Pcg64Dxsm::new(vector.initstate, vector.initseq);
        assert_eq!(rng.raw(), (vector.seeded_state, vector.seeded_increment));
        let first16: [u64; 16] = std::array::from_fn(|_| rng.next_u64());
        assert_eq!(first16, vector.first16);
        let mut last = 0;
        for _ in 16..1000 {
            last = rng.next_u64();
        }
        assert_eq!(last, vector.row999);
    }
}

/// T3, REQ-CORE-001, REQ-API-004: from_u64_seed(0) (vector c), including the SplitMix64 words.
#[test]
fn req_core_001_req_api_004_from_u64_seed_zero_fixed_values() {
    let mut sm = SplitMix64::new(0);
    let words: [u64; 4] = std::array::from_fn(|_| sm.next_u64());
    assert_eq!(
        words,
        [
            0xe220a8397b1dcdaf,
            0x6e789e6aa1b965f4,
            0x06c45d188009454f,
            0xf88bb8a8724c81ec
        ]
    );
    let mut rng = Pcg64Dxsm::from_u64_seed(0);
    assert_eq!(
        rng.raw(),
        (
            0xeaecae570be9588304c20265ecd6861a,
            0x0d88ba3100128a9ff1177150e49903d9
        )
    );
    assert_eq!(
        first_four(&mut rng),
        [
            0x9359facb6893fb8c,
            0x95b20565e8a32559,
            0x30df5cc4f7afd2ab,
            0x240b6567c8cc62ce
        ]
    );
}

/// T3, REQ-CORE-001, REQ-API-004: from_u64_seed(0xdeadbeaf) and (0x0123456789abcdef)
/// (vectors d and e).
#[test]
fn req_core_001_req_api_004_from_u64_seed_fixed_values() {
    let mut d = Pcg64Dxsm::from_u64_seed(0xdeadbeaf);
    assert_eq!(
        d.raw(),
        (
            0x73271ee19d0bbde949ef04bf821f7ffd,
            0x60bc1fa636e442e55d8715a5a23808fb
        )
    );
    assert_eq!(
        first_four(&mut d),
        [
            0x2442410dc7aa14ef,
            0xefcf3fd38974d04f,
            0xe8a72b3f857f5da2,
            0x2b00effed157a91e
        ]
    );
    let mut e = Pcg64Dxsm::from_u64_seed(0x0123456789abcdef);
    assert_eq!(
        e.raw(),
        (
            0x36a649ed1d94073781d2fc089e55d4f5,
            0x5f216e5d32db997d45a83266988ccfd9
        )
    );
    assert_eq!(
        first_four(&mut e),
        [
            0xc95b21bd56248a1c,
            0xf558a598f7a61873,
            0x9003b0b81c9da272,
            0xfb6e77187f825f66
        ]
    );
}

/// T4, REQ-API-004: fork fixed values (vector f) and fork properties.
#[test]
fn req_api_004_fork_fixed_values_and_properties() {
    let mut parent = Pcg64Dxsm::from_u64_seed(0x0123456789abcdef);
    for _ in 0..3 {
        parent.next_u64();
    }
    let (state, increment) = parent.raw();
    assert_eq!(
        (state, increment),
        (
            0x17a3da68fe0de4080fcc7298c6250cc8,
            0x5f216e5d32db997d45a83266988ccfd9
        )
    );
    let salt = 0xfeed_face;
    let by_hand = hi64(state) ^ lo64(state) ^ hi64(increment) ^ lo64(increment) ^ salt;
    assert_eq!(parent.fork_seed64(salt), by_hand);

    let before = parent.clone();
    let mut fork = parent.fork(salt);
    assert_eq!(parent, before, "fork() must not advance the parent");
    assert_eq!(fork, Pcg64Dxsm::from_u64_seed(by_hand));

    let mut sm = SplitMix64::new(by_hand);
    let words: [u64; 4] = std::array::from_fn(|_| sm.next_u64());
    assert_eq!(
        words,
        [
            0x9df0a5d241d2ff5a,
            0xf2d73877ff29cc35,
            0x8ca9b13a5477cd77,
            0x8edb64250e0284a9
        ]
    );
    assert_eq!(
        fork.raw(),
        (
            0x1d6e07594bcdb647243c23cc113546fb,
            0x19536274a8ef9aef1db6c84a1c050953
        )
    );
    assert_eq!(
        first_four(&mut fork),
        [
            0xc2a077ec96049794,
            0xd6a17bb6098ee9bc,
            0xb8ba2eea439649a5,
            0x33a9ccd66a0c62db
        ]
    );

    // Same parent, same salt: equal forks. Different salt: different fork.
    assert_eq!(parent.fork(salt), parent.fork(salt));
    assert_ne!(parent.fork(salt), parent.fork(salt ^ 1));

    // Equal state, different increment: different forks.
    let a = Pcg64Dxsm::from_raw(state, 0x1_0000_0000_0000_0001).unwrap_or_else(|| parent.clone());
    let b = Pcg64Dxsm::from_raw(state, 0x3).unwrap_or_else(|| parent.clone());
    assert_eq!(a.raw().0, b.raw().0);
    assert_ne!(a.raw().1, b.raw().1);
    assert_ne!(a.fork(salt), b.fork(salt));
}

/// T5, REQ-CORE-001: sampling fixed values from from_u64_seed(42) (vector g).
#[test]
fn req_core_001_sampling_fixed_values() {
    let mut rng = Pcg64Dxsm::from_u64_seed(42);
    let rolls: Vec<Option<u64>> = (0..10).map(|_| rng.next_bounded(6)).collect();
    let expected: Vec<Option<u64>> = [2, 5, 3, 0, 0, 3, 4, 3, 5, 3]
        .into_iter()
        .map(Some)
        .collect();
    assert_eq!(rolls, expected);

    let mut rng = Pcg64Dxsm::from_u64_seed(42);
    let mut deck: Vec<u32> = (0..10).collect();
    rng.shuffle(&mut deck);
    assert_eq!(deck, [5, 2, 1, 8, 3, 6, 0, 9, 7, 4]);

    let mut rng = Pcg64Dxsm::from_u64_seed(42);
    assert_eq!(rng.random_subset(10, 4), Some(vec![3, 4, 7, 9]));
}

/// T5, REQ-CORE-001: next_bounded edge cases (0, 1, top of range) and draw consumption.
#[test]
fn req_core_001_next_bounded_edges() {
    let start = Pcg64Dxsm::from_u64_seed(7);
    let mut rng = start.clone();
    assert_eq!(rng.next_bounded(0), None);
    assert_eq!(rng, start, "bound 0 consumes no draw");

    assert_eq!(rng.next_bounded(1), Some(0));
    let mut one_draw = start.clone();
    one_draw.next_u64();
    assert_eq!(rng, one_draw, "bound 1 consumes exactly one draw");

    let mut rng = Pcg64Dxsm::from_u64_seed(8);
    for bound in [u64::MAX, (1u64 << 63) + 1, 1u64 << 63, u64::MAX - 1] {
        for _ in 0..2000 {
            assert!(rng.next_bounded(bound).is_some_and(|v| v < bound));
        }
    }
}

/// T5, REQ-CORE-001: `random_subset` returns `None` (no panic, no draw) when the result vector
/// cannot be reserved because its byte size exceeds `isize::MAX`.
#[test]
fn req_core_001_random_subset_capacity_overflow_returns_none() {
    let start = Pcg64Dxsm::from_u64_seed(9);
    let mut rng = start.clone();
    assert_eq!(rng.random_subset(usize::MAX, usize::MAX), None);
    assert_eq!(rng.random_subset(usize::MAX, usize::MAX / 2), None);
    assert_eq!(rng, start, "no draw is consumed when the reservation fails");
    assert_eq!(rng.random_subset(5, 5), Some(vec![0, 1, 2, 3, 4]));
}

/// T5, REQ-CORE-001: random_subset, choose and shuffle edge cases.
#[test]
fn req_core_001_sampling_edge_cases() {
    let start = Pcg64Dxsm::from_u64_seed(9);

    let mut rng = start.clone();
    assert_eq!(rng.random_subset(5, 0), Some(Vec::new()));
    assert_eq!(rng.random_subset(0, 0), Some(Vec::new()));
    assert_eq!(rng.random_subset(3, 4), None);
    assert_eq!(rng, start, "k = 0 and k > n consume no draw");
    assert_eq!(rng.random_subset(6, 6), Some(vec![0, 1, 2, 3, 4, 5]));

    let mut rng = start.clone();
    let empty: [u8; 0] = [];
    assert_eq!(rng.choose(&empty), None);
    assert_eq!(rng, start, "choose on empty consumes no draw");
    assert_eq!(rng.choose(&[17u8]), Some(&17));
    let items = [10u8, 20, 30];
    for _ in 0..100 {
        assert!(rng.choose(&items).is_some_and(|v| items.contains(v)));
    }

    let mut rng = start.clone();
    let mut none: [u8; 0] = [];
    rng.shuffle(&mut none);
    let mut single = [42u8];
    rng.shuffle(&mut single);
    assert_eq!(single, [42]);
    assert_eq!(rng, start, "shuffle of 0 or 1 elements consumes no draw");

    let mut many: Vec<u16> = (0..50).rev().collect();
    let original = many.clone();
    rng.shuffle(&mut many);
    let mut sorted = many.clone();
    sorted.sort_unstable();
    let mut original_sorted = original;
    original_sorted.sort_unstable();
    assert_eq!(sorted, original_sorted);
}

/// T6, REQ-CORE-001: from_raw/raw round trip, even increments rejected, canonical encoding and
/// clone replay.
#[test]
fn req_core_001_round_trip_encoding_and_clone() {
    let mut rng = Pcg64Dxsm::from_u64_seed(1234);
    rng.next_u64();
    let (state, increment) = rng.raw();
    let restored = Pcg64Dxsm::from_raw(state, increment);
    assert_eq!(restored.as_ref(), Some(&rng));
    let mut restored = restored.unwrap_or_else(|| Pcg64Dxsm::from_u64_seed(0));
    let mut cloned = rng.clone();
    for _ in 0..100 {
        let x = rng.next_u64();
        assert_eq!(restored.next_u64(), x);
        assert_eq!(cloned.next_u64(), x);
    }

    assert_eq!(Pcg64Dxsm::from_raw(state, increment & !1), None);
    assert_eq!(Pcg64Dxsm::from_raw(0, 0), None);
    assert!(Pcg64Dxsm::from_raw(0, 1).is_some());

    let rng = Pcg64Dxsm::from_u64_seed(0);
    let mut out = vec![0xAA, 0xBB, 0xCC];
    rng.encode_canonical(&mut out);
    assert_eq!(out.len(), 3 + 32);
    assert_eq!(out[..3], [0xAA, 0xBB, 0xCC]);
    let (state, increment) = rng.raw();
    assert_eq!(out[3..19], state.to_le_bytes());
    assert_eq!(out[19..35], increment.to_le_bytes());
    // Explicit little-endian bytes, independent of the host.
    assert_eq!(out[3], 0x1a);
    assert_eq!(out[18], 0xea);
    assert_eq!(out[19], 0xd9);
    assert_eq!(out[34], 0x0d);
    assert!(format!("{rng:?}").contains("Pcg64Dxsm"));
}

/// Lehmer rank of a permutation of 0..4, in 0..24.
fn rank4(p: &[u8; 4]) -> usize {
    let mut rank = 0;
    for i in 0..4 {
        let smaller_after = p[i + 1..].iter().filter(|&&x| x < p[i]).count();
        rank = rank * (4 - i) + smaller_after;
    }
    rank
}

/// sum((O - E)^2) over all cells, integer only.
fn chi_numerator(counts: &[u64], expected: u64) -> u64 {
    counts.iter().map(|&o| o.abs_diff(expected).pow(2)).sum()
}

/// T7, REQ-CORE-001: shuffle uniformity, deterministic chi-squared on seeds {1..=5}, 23 degrees
/// of freedom, p = 0.001 critical value 49.73.
#[test]
fn req_core_001_shuffle_uniformity_chi_squared() {
    const E: u64 = 1000;
    for seed in 1..=5u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut counts = [0u64; 24];
        for _ in 0..24_000 {
            let mut p = [0u8, 1, 2, 3];
            rng.shuffle(&mut p);
            counts[rank4(&p)] += 1;
        }
        let num = chi_numerator(&counts, E);
        assert!(
            num * 1000 <= 49_730 * E,
            "seed {seed}: chi-squared numerator {num}"
        );
    }
}

/// T7, REQ-CORE-001: next_bounded(6) uniformity, deterministic chi-squared on seeds {1..=5},
/// 5 degrees of freedom, p = 0.001 critical value 20.52.
#[test]
fn req_core_001_bounded_uniformity_chi_squared() {
    const E: u64 = 10_000;
    for seed in 1..=5u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut counts = [0u64; 6];
        for _ in 0..60_000 {
            let v = rng.next_bounded(6).unwrap_or(6);
            assert!(v < 6);
            counts[v as usize] += 1;
        }
        let num = chi_numerator(&counts, E);
        assert!(
            num * 1000 <= 20_520 * E,
            "seed {seed}: chi-squared numerator {num}"
        );
    }
}

/// T8, REQ-CORE-001: property test with a deterministic SplitMix64 harness (literal seed,
/// 5 000 iterations): bounded results are in range, shuffle is a permutation, random_subset
/// returns k distinct ascending indices below n, and a clone advanced the same way stays equal.
#[test]
fn req_core_001_sampling_invariants_property() {
    let mut harness = SplitMix64::new(0x5EED_0001);
    for _ in 0..5_000 {
        let seed = harness.next_u64();
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut twin = rng.clone();

        // A bound drawn from a mix of magnitudes, excluding 0.
        let raw_bound = harness.next_u64();
        let bound = match raw_bound % 3 {
            0 => raw_bound,
            1 => raw_bound >> 32,
            _ => raw_bound % 100,
        }
        .max(1);
        let v = rng.next_bounded(bound);
        assert!(v.is_some_and(|v| v < bound));
        assert_eq!(twin.next_bounded(bound), v);

        let len = (harness.next_u64() % 40) as usize;
        let mut items: Vec<usize> = (0..len).collect();
        rng.shuffle(&mut items);
        let mut twin_items: Vec<usize> = (0..len).collect();
        twin.shuffle(&mut twin_items);
        assert_eq!(items, twin_items);
        items.sort_unstable();
        assert!(items.iter().copied().eq(0..len));

        let n = (harness.next_u64() % 40) as usize;
        let k = (harness.next_u64() % 41) as usize;
        let subset = rng.random_subset(n, k);
        assert_eq!(twin.random_subset(n, k), subset);
        match subset {
            None => assert!(k > n),
            Some(s) => {
                assert!(k <= n);
                assert_eq!(s.len(), k);
                assert!(s.windows(2).all(|w| w[0] < w[1]));
                assert!(s.iter().all(|&i| i < n));
            }
        }

        assert_eq!(rng, twin);
    }
}

/// T9, REQ-CORE-001 (R12): from_raw never panics on arbitrary (state, increment) pairs, accepts
/// exactly the odd increments, and the restored generator can be used.
#[test]
fn req_core_001_from_raw_random_inputs_never_panic() {
    let mut harness = SplitMix64::new(0x5EED_0002);
    let edges = [0u128, 1, 2, u128::MAX, u128::MAX - 1, 1u128 << 127];
    let mut pairs: Vec<(u128, u128)> = Vec::new();
    for &s in &edges {
        for &i in &edges {
            pairs.push((s, i));
        }
    }
    for _ in 0..5_000 {
        let s = (u128::from(harness.next_u64()) << 64) | u128::from(harness.next_u64());
        let i = (u128::from(harness.next_u64()) << 64) | u128::from(harness.next_u64());
        pairs.push((s, i));
    }
    for (s, i) in pairs {
        match Pcg64Dxsm::from_raw(s, i) {
            None => assert_eq!(i & 1, 0),
            Some(mut rng) => {
                assert_eq!(i & 1, 1);
                assert_eq!(rng.raw(), (s, i));
                rng.next_u64();
                let _ = rng.next_bounded(u64::MAX);
                let _ = rng.fork(0);
            }
        }
    }
}
