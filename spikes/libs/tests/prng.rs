// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T1 (REQ-CORE-001): PCG64-DXSM against PUBLISHED vectors.
//!
//! Expected outputs: NumPy `numpy/random/tests/data/pcg64dxsm-testset-1.csv` (seed 0xdeadbeaf)
//! and `pcg64dxsm-testset-2.csv` (seed 0x0), rows 0..=15 and row 999, copied verbatim.
//! NumPy derives the 128-bit `initstate`/`initseq` from the integer seed with `SeedSequence`;
//! those four 64-bit words (`SeedSequence(seed).generate_state(4, uint64)`) and the resulting
//! raw state were computed with NumPy 2.5.3 and are hard-coded below.

use fineprint_spike_libs::prng::Pcg64Dxsm;
use rand_core::Rng;

struct Vector {
    initstate: u128,
    initseq: u128,
    seeded_state: u128,
    seeded_increment: u128,
    first16: [u64; 16],
    row999: u64,
}

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

fn outputs(mut next: impl FnMut() -> u64) -> ([u64; 16], u64) {
    let first16 = std::array::from_fn(|_| next());
    let row999 = (16..1000).map(|_| next()).last().unwrap_or(0);
    (first16, row999)
}

#[test]
fn req_core_001_in_house_pcg64_dxsm_matches_published_vectors() {
    for vector in [DEADBEAF, ZERO] {
        let mut rng = Pcg64Dxsm::new(vector.initstate, vector.initseq);
        assert_eq!(rng.raw(), (vector.seeded_state, vector.seeded_increment));
        assert_eq!(outputs(|| rng.next_u64()), (vector.first16, vector.row999));
    }
}

/// The candidate crate produces the published outputs from NumPy's seeded raw state. Its own
/// `new` seeds with the cheap multiplier (pcg-cpp convention), which NumPy does not, so its
/// seeded state differs: that difference is asserted too, as evidence for DECISIONS.md.
#[test]
fn req_core_001_rand_pcg_candidate_matches_published_vectors() {
    for vector in [DEADBEAF, ZERO] {
        let mut rng =
            rand_pcg::Pcg64Dxsm::from_state(vector.seeded_state, vector.seeded_increment >> 1);
        assert_eq!(outputs(|| rng.next_u64()), (vector.first16, vector.row999));
        let seeded = rand_pcg::Pcg64Dxsm::new(vector.initstate, vector.initseq);
        assert_ne!(seeded.state(), vector.seeded_state);
    }
}

#[test]
fn req_core_001_clone_replays_the_same_sequence() {
    let mut a = Pcg64Dxsm::new(7, 11);
    let _ = a.next_u64();
    let mut b = a.clone();
    assert_eq!(a, b);
    assert_eq!(outputs(|| a.next_u64()), outputs(|| b.next_u64()));
}
