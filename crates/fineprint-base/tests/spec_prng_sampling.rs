// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::prng` (bounded integers, shuffle, choose, subset).
//! Black-box: written from the work-item brief only.

use fineprint_base::prng::Pcg64Dxsm;
use std::collections::BTreeMap;

// ---- Independent reference model (pure integer, from the algorithm text) ----

const SM_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MUL: u64 = 0xda94_2042_e4dd_58b5;
const SEED_MUL: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

fn sm_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(SM_GAMMA);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

struct Model {
    state: u128,
    inc: u128,
}

impl Model {
    fn from_seed(seed: u64) -> Self {
        let mut s = seed;
        let o0 = u128::from(sm_next(&mut s));
        let o1 = u128::from(sm_next(&mut s));
        let o2 = u128::from(sm_next(&mut s));
        let o3 = u128::from(sm_next(&mut s));
        let (initstate, initseq) = ((o0 << 64) | o1, (o2 << 64) | o3);
        let inc = (initseq << 1) | 1;
        let state = inc
            .wrapping_add(initstate)
            .wrapping_mul(SEED_MUL)
            .wrapping_add(inc);
        Model { state, inc }
    }
    fn next(&mut self) -> u64 {
        let mut hi = (self.state >> 64) as u64;
        let lo = (self.state as u64) | 1;
        hi ^= hi >> 32;
        hi = hi.wrapping_mul(MUL);
        hi ^= hi >> 48;
        hi = hi.wrapping_mul(lo);
        self.state = self
            .state
            .wrapping_mul(u128::from(MUL))
            .wrapping_add(self.inc);
        hi
    }
    fn bounded(&mut self, bound: u64) -> u64 {
        let mut x = self.next();
        let mut m = u128::from(x) * u128::from(bound);
        let mut l = m as u64;
        if l < bound {
            let t = bound.wrapping_neg() % bound;
            while l < t {
                x = self.next();
                m = u128::from(x) * u128::from(bound);
                l = m as u64;
            }
        }
        (m >> 64) as u64
    }
    fn shuffle(&mut self, v: &mut [u32]) {
        for i in (1..v.len()).rev() {
            let j = self.bounded(i as u64 + 1) as usize;
            v.swap(i, j);
        }
    }
    fn subset(&mut self, n: usize, k: usize) -> Vec<usize> {
        let mut chosen = Vec::new();
        for i in 0..n {
            let needed = k - chosen.len();
            if needed == 0 {
                break;
            }
            if self.bounded((n - i) as u64) < needed as u64 {
                chosen.push(i);
            }
        }
        chosen
    }
}

fn rng(seed: u64) -> Pcg64Dxsm {
    Pcg64Dxsm::from_u64_seed(seed)
}

/// REQ-CORE-001: bounded/shuffle/subset equal an independent model over 2_000 seeds.
#[test]
fn req_core_001_sampling_matches_reference_model() {
    let bounds: [u64; 9] = [1, 2, 3, 6, 10, 1000, (1 << 32) + 7, (1 << 63) + 1, u64::MAX];
    let mut seeder = 0x0DDB_1A5E_5BAD_5EED_u64;
    for _ in 0..2_000 {
        let seed = sm_next(&mut seeder);
        let mut model = Model::from_seed(seed);
        let mut real = rng(seed);
        for _ in 0..20 {
            let b = bounds[(sm_next(&mut seeder) % 9) as usize];
            assert_eq!(
                real.next_bounded(b),
                Some(model.bounded(b)),
                "seed {seed:#x}"
            );
        }
        let mut a: Vec<u32> = (0..17).collect();
        let mut b = a.clone();
        real.shuffle(&mut a);
        model.shuffle(&mut b);
        assert_eq!(a, b, "seed {seed:#x}");
        assert_eq!(real.random_subset(17, 5), Some(model.subset(17, 5)));
        assert_eq!(real.raw(), (model.state, model.inc));
    }
}

/// REQ-CORE-001: sampling fixed values from from_u64_seed(42).
#[test]
fn req_core_001_sampling_fixed_values() {
    let mut r = rng(42);
    let got: Vec<u64> = (0..10)
        .map(|_| r.next_bounded(6).expect("bound > 0"))
        .collect();
    assert_eq!(got, vec![2, 5, 3, 0, 0, 3, 4, 3, 5, 3]);

    let mut r = rng(42);
    let mut v: Vec<u32> = (0..10).collect();
    r.shuffle(&mut v);
    assert_eq!(v, vec![5, 2, 1, 8, 3, 6, 0, 9, 7, 4]);

    let mut r = rng(42);
    assert_eq!(r.random_subset(10, 4), Some(vec![3, 4, 7, 9]));
}

/// REQ-CORE-001: edge cases and draw consumption.
#[test]
fn req_core_001_edge_cases() {
    let mut r = rng(5);
    let before = r.raw();
    assert_eq!(r.next_bounded(0), None);
    assert_eq!(r.raw(), before);

    let mut one = rng(5);
    let mut ref_rng = rng(5);
    assert_eq!(one.next_bounded(1), Some(0));
    ref_rng.next_u64();
    assert_eq!(one.raw(), ref_rng.raw());

    let mut r = rng(6);
    for _ in 0..1_000 {
        let a = r.next_bounded(u64::MAX).expect("nonzero");
        assert!(a < u64::MAX);
        let b = r.next_bounded((1 << 63) + 1).expect("nonzero");
        assert!(b < (1 << 63) + 1);
    }

    let mut r = rng(7);
    let before = r.raw();
    let empty: [u8; 0] = [];
    assert_eq!(r.choose(&empty), None);
    assert_eq!(r.raw(), before);
    let mut ref_rng = rng(7);
    assert_eq!(r.choose(&[9u8]), Some(&9));
    ref_rng.next_u64();
    assert_eq!(r.raw(), ref_rng.raw());

    let mut r = rng(8);
    let before = r.raw();
    let mut e: [u8; 0] = [];
    r.shuffle(&mut e);
    let mut single = [1u8];
    r.shuffle(&mut single);
    assert_eq!(single, [1]);
    assert_eq!(r.raw(), before);

    assert_eq!(r.random_subset(5, 0), Some(vec![]));
    assert_eq!(r.raw(), before);
    assert_eq!(r.random_subset(5, 5), Some(vec![0, 1, 2, 3, 4]));
    let before = r.raw();
    assert_eq!(r.random_subset(3, 4), None);
    assert_eq!(r.raw(), before);
    assert_eq!(r.random_subset(0, 0), Some(vec![]));
}

fn chi_ok(counts: &[u64], expected: u64, crit_milli: i128) -> bool {
    let sum: i128 = counts
        .iter()
        .map(|&o| {
            let d = i128::from(o) - i128::from(expected);
            d * d
        })
        .sum();
    sum * 1000 <= crit_milli * i128::from(expected)
}

/// REQ-CORE-001: deterministic chi-squared (integer arithmetic) on shuffle and bounded draws.
#[test]
fn req_core_001_chi_squared_uniformity() {
    for seed in 1..=5u64 {
        let mut r = rng(seed);

        // (a) shuffle of 4 elements: 24 permutations, df 23.
        let mut tally: BTreeMap<[u8; 4], u64> = BTreeMap::new();
        for _ in 0..24_000 {
            let mut a = [0u8, 1, 2, 3];
            r.shuffle(&mut a);
            *tally.entry(a).or_insert(0) += 1;
        }
        assert_eq!(tally.len(), 24, "seed {seed}");
        let counts: Vec<u64> = tally.values().copied().collect();
        assert!(
            chi_ok(&counts, 1000, 49_730),
            "shuffle4 seed {seed}: {counts:?}"
        );

        // (b) bounded(6), df 5.
        let mut c = [0u64; 6];
        for _ in 0..60_000 {
            c[r.next_bounded(6).expect("nonzero") as usize] += 1;
        }
        assert!(chi_ok(&c, 10_000, 20_520), "bounded6 seed {seed}: {c:?}");

        // (c) position of element 0 in shuffles of 6, df 5.
        let mut p = [0u64; 6];
        for _ in 0..60_000 {
            let mut a = [0u8, 1, 2, 3, 4, 5];
            r.shuffle(&mut a);
            let pos = a.iter().position(|&x| x == 0).expect("present");
            p[pos] += 1;
        }
        assert!(chi_ok(&p, 10_000, 20_520), "position seed {seed}: {p:?}");
    }
}

/// REQ-CORE-001: properties over 5_000+ iterations.
#[test]
fn req_core_001_properties() {
    let mut seeder = 0x1357_9BDF_0246_8ACE_u64;
    for _ in 0..5_000 {
        let seed = sm_next(&mut seeder);
        let mut r = rng(seed);

        let bound = match sm_next(&mut seeder) % 4 {
            0 => sm_next(&mut seeder) % 100 + 1,
            1 => sm_next(&mut seeder) | 1,
            2 => (sm_next(&mut seeder) >> 33) + 1,
            _ => u64::MAX - sm_next(&mut seeder) % 4,
        };
        assert!(r.next_bounded(bound).expect("nonzero") < bound);

        let len = (sm_next(&mut seeder) % 41) as usize;
        let orig: Vec<u32> = (0..len as u32).collect();
        let mut v = orig.clone();
        r.shuffle(&mut v);
        v.sort_unstable();
        assert_eq!(v, orig);

        let n = (sm_next(&mut seeder) % 41) as usize;
        let k = (sm_next(&mut seeder) % (n as u64 + 1)) as usize;
        let s = r.random_subset(n, k).expect("k <= n");
        assert_eq!(s.len(), k);
        assert!(s.windows(2).all(|w| w[0] < w[1]));
        assert!(s.iter().all(|&i| i < n));
    }
}

fn mixed_ops(seed: u64) -> Vec<u64> {
    let mut r = rng(seed);
    let mut out = Vec::new();
    for i in 0..50u64 {
        out.push(r.next_u64());
        out.push(r.next_bounded(i + 1).expect("nonzero"));
        let mut v: Vec<u64> = (0..=(i % 10)).collect();
        r.shuffle(&mut v);
        out.extend(v);
        out.push(*r.choose(&[10u64, 20, 30, 40]).expect("nonempty"));
        out.extend(
            r.random_subset(12, (i % 13) as usize)
                .expect("k <= n")
                .into_iter()
                .map(|x| x as u64),
        );
    }
    out
}

/// REQ-CORE-001: same seed gives identical mixed-operation sequences; distinct seeds differ.
#[test]
fn req_core_001_determinism_and_distinct_seeds() {
    let mut seeder = 0xFACE_FEED_u64;
    for _ in 0..200 {
        let seed = sm_next(&mut seeder);
        assert_eq!(mixed_ops(seed), mixed_ops(seed));
    }
    let mut firsts = std::collections::BTreeSet::new();
    let mut s = 0x77_u64;
    for _ in 0..1_000 {
        let seed = sm_next(&mut s);
        firsts.insert(rng(seed).next_u64());
    }
    assert_eq!(firsts.len(), 1_000);
}
