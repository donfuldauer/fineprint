// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::prng` (vectors, seeding, fork, encoding, raw round trip).
//! Black-box: written from the work-item brief only.

use fineprint_base::ids::CanonicalEncode;
use fineprint_base::prng::{ALGORITHM_ID, Pcg64Dxsm, SplitMix64};

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
    fn new(initstate: u128, initseq: u128) -> Self {
        let inc = (initseq << 1) | 1;
        let state = inc
            .wrapping_add(initstate)
            .wrapping_mul(SEED_MUL)
            .wrapping_add(inc);
        Model { state, inc }
    }
    fn from_seed(seed: u64) -> Self {
        let mut s = seed;
        let o0 = u128::from(sm_next(&mut s));
        let o1 = u128::from(sm_next(&mut s));
        let o2 = u128::from(sm_next(&mut s));
        let o3 = u128::from(sm_next(&mut s));
        Model::new((o0 << 64) | o1, (o2 << 64) | o3)
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
}

fn from_seed_real(seed: u64) -> Pcg64Dxsm {
    Pcg64Dxsm::from_u64_seed(seed)
}

fn draw(rng: &mut Pcg64Dxsm, n: usize) -> Vec<u64> {
    (0..n).map(|_| rng.next_u64()).collect()
}

/// REQ-CORE-001: SplitMix64 published vectors.
#[test]
fn req_core_001_splitmix64_vectors() {
    let mut sm = SplitMix64::new(1_234_567);
    let got: Vec<u64> = (0..5).map(|_| sm.next_u64()).collect();
    assert_eq!(
        got,
        vec![
            6_457_827_717_110_365_317,
            3_203_168_211_198_807_973,
            9_817_491_932_198_370_423,
            4_593_380_528_125_082_431,
            16_408_922_859_458_223_821
        ]
    );
    let mut sm = SplitMix64::new(0);
    let got: Vec<u64> = (0..3).map(|_| sm.next_u64()).collect();
    assert_eq!(
        got,
        vec![
            0xe220_a839_7b1d_cdaf,
            0x6e78_9e6a_a1b9_65f4,
            0x06c4_5d18_8009_454f
        ]
    );
}

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

/// REQ-CORE-001: NumPy-published PCG64-DXSM vectors (seeded raw state, first 16, row 999).
#[test]
fn req_core_001_numpy_published_vectors() {
    for v in [DEADBEAF, ZERO] {
        let mut rng = Pcg64Dxsm::new(v.initstate, v.initseq);
        assert_eq!(rng.raw(), (v.seeded_state, v.seeded_increment));
        let first = draw(&mut rng, 16);
        assert_eq!(first, v.first16.to_vec());
        let mut last = 0;
        for _ in 16..1000 {
            last = rng.next_u64();
        }
        assert_eq!(last, v.row999);
    }
}

/// REQ-CORE-001: the library equals an independent model for 2_000 seeds
/// (seeding, raw state, 8 outputs).
#[test]
fn req_core_001_matches_reference_model() {
    let mut seeder = 0x00C0_FFEE_1234_5678_u64;
    for _ in 0..2_000 {
        let seed = sm_next(&mut seeder);
        let mut model = Model::from_seed(seed);
        let mut real = from_seed_real(seed);
        assert_eq!(real.raw(), (model.state, model.inc), "seed {seed:#x}");
        for i in 0..8 {
            assert_eq!(real.next_u64(), model.next(), "seed {seed:#x} draw {i}");
        }
        assert_eq!(real.raw(), (model.state, model.inc));
    }
}

fn check_seed_vector(seed: u64, raw: (u128, u128), first4: [u64; 4]) {
    let mut rng = Pcg64Dxsm::from_u64_seed(seed);
    assert_eq!(rng.raw(), raw, "seed {seed:#x}");
    assert_eq!(draw(&mut rng, 4), first4.to_vec(), "seed {seed:#x}");
}

/// REQ-CORE-001, REQ-API-004: from_u64_seed fixed values.
#[test]
fn req_core_001_req_api_004_from_u64_seed_vectors() {
    check_seed_vector(
        0,
        (
            0xeaecae570be9588304c20265ecd6861a,
            0x0d88ba3100128a9ff1177150e49903d9,
        ),
        [
            0x9359facb6893fb8c,
            0x95b20565e8a32559,
            0x30df5cc4f7afd2ab,
            0x240b6567c8cc62ce,
        ],
    );
    check_seed_vector(
        0xdeadbeaf,
        (
            0x73271ee19d0bbde949ef04bf821f7ffd,
            0x60bc1fa636e442e55d8715a5a23808fb,
        ),
        [
            0x2442410dc7aa14ef,
            0xefcf3fd38974d04f,
            0xe8a72b3f857f5da2,
            0x2b00effed157a91e,
        ],
    );
    check_seed_vector(
        0x0123456789abcdef,
        (
            0x36a649ed1d94073781d2fc089e55d4f5,
            0x5f216e5d32db997d45a83266988ccfd9,
        ),
        [
            0xc95b21bd56248a1c,
            0xf558a598f7a61873,
            0x9003b0b81c9da272,
            0xfb6e77187f825f66,
        ],
    );
}

fn parent() -> Pcg64Dxsm {
    let mut p = Pcg64Dxsm::from_u64_seed(0x0123456789abcdef);
    for _ in 0..3 {
        p.next_u64();
    }
    p
}

/// REQ-API-004: fork fixed values and fork seed formula.
#[test]
fn req_api_004_fork_fixed_values() {
    let p = parent();
    assert_eq!(
        p.raw(),
        (
            0x17a3da68fe0de4080fcc7298c6250cc8,
            0x5f216e5d32db997d45a83266988ccfd9
        )
    );
    let salt = 0xfeedface_u64;
    let (s, i) = p.raw();
    let expected = ((s >> 64) as u64) ^ (s as u64) ^ ((i >> 64) as u64) ^ (i as u64) ^ salt;
    assert_eq!(p.fork_seed64(salt), expected);
    let mut f = p.fork(salt);
    assert_eq!(
        f.raw(),
        (
            0x1d6e07594bcdb647243c23cc113546fb,
            0x19536274a8ef9aef1db6c84a1c050953
        )
    );
    assert_eq!(
        draw(&mut f, 4),
        vec![
            0xc2a077ec96049794,
            0xd6a17bb6098ee9bc,
            0xb8ba2eea439649a5,
            0x33a9ccd66a0c62db
        ]
    );
}

/// REQ-API-004: fork equals from_u64_seed(fork_seed64); parent untouched; reproducible.
#[test]
fn req_api_004_fork_properties() {
    let p = parent();
    let before = p.raw();
    let salt = 0xfeedface_u64;
    assert_eq!(p.fork(salt), Pcg64Dxsm::from_u64_seed(p.fork_seed64(salt)));
    assert_eq!(p.raw(), before);
    assert_eq!(p.fork(salt), p.fork(salt));
    assert_ne!(p.fork(1), p.fork(2));
    assert_ne!(p.fork(salt), p);

    // Same state, different increment: forks differ for the same salt.
    let (s, _) = before;
    let a = Pcg64Dxsm::from_raw(s, 1).expect("odd");
    let b = Pcg64Dxsm::from_raw(s, 3).expect("odd");
    assert_ne!(a.fork(salt), b.fork(salt));

    // Forking after a draw differs from forking before it.
    let mut q = parent();
    let early = q.fork(salt);
    q.next_u64();
    assert_ne!(early, q.fork(salt));
}

/// REQ-CORE-001: raw round trip, from_raw validation, clone, equality, algorithm id.
#[test]
fn req_core_001_save_and_restore() {
    assert_eq!(ALGORITHM_ID, "PCG64-DXSM");
    let mut a = Pcg64Dxsm::from_u64_seed(99);
    a.next_u64();
    let (s, i) = a.raw();
    let mut b = Pcg64Dxsm::from_raw(s, i).expect("odd increment");
    assert_eq!(a, b);
    let mut c = a.clone();
    let xa = draw(&mut a, 100);
    assert_eq!(xa, draw(&mut b, 100));
    assert_eq!(xa, draw(&mut c, 100));

    for even in [0u128, 2, u128::MAX - 1] {
        assert!(Pcg64Dxsm::from_raw(5, even).is_none());
    }
    for odd in [1u128, u128::MAX] {
        assert!(Pcg64Dxsm::from_raw(5, odd).is_some());
    }
    let mut seeder = 0xABCD_u64;
    for _ in 0..5_000 {
        let st = (u128::from(sm_next(&mut seeder)) << 64) | u128::from(sm_next(&mut seeder));
        let inc = (u128::from(sm_next(&mut seeder)) << 64) | u128::from(sm_next(&mut seeder));
        let r = Pcg64Dxsm::from_raw(st, inc);
        assert_eq!(r.is_some(), inc & 1 == 1);
    }

    let x = Pcg64Dxsm::from_raw(10, 1).expect("odd");
    assert_ne!(x, Pcg64Dxsm::from_raw(11, 1).expect("odd"));
    assert_ne!(x, Pcg64Dxsm::from_raw(10, 3).expect("odd"));
    assert_eq!(x, Pcg64Dxsm::from_raw(10, 1).expect("odd"));
}

/// REQ-CORE-001: canonical encoding is state LE then increment LE, appended.
#[test]
fn req_core_001_canonical_encoding() {
    let mut rng = Pcg64Dxsm::from_u64_seed(7);
    let (s, i) = rng.raw();
    let mut expected = s.to_le_bytes().to_vec();
    expected.extend_from_slice(&i.to_le_bytes());

    let mut out = Vec::new();
    rng.encode_canonical(&mut out);
    assert_eq!(out.len(), 32);
    assert_eq!(out, expected);

    let mut pre = vec![1u8, 2, 3];
    rng.encode_canonical(&mut pre);
    assert_eq!(&pre[..3], &[1, 2, 3]);
    assert_eq!(&pre[3..], expected.as_slice());

    let mut twice = Vec::new();
    rng.encode_canonical(&mut twice);
    rng.encode_canonical(&mut twice);
    assert_eq!(twice.len(), 64);
    assert_eq!(&twice[..32], &twice[32..]);

    rng.next_u64();
    let mut after = Vec::new();
    rng.encode_canonical(&mut after);
    assert_ne!(&after[..16], &expected[..16]);
    assert_eq!(&after[16..], &expected[16..]);
}
