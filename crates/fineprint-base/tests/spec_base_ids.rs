// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::ids`. Black-box: written from the work-item brief only.

use core::fmt::Debug;
use fineprint_base::ids::{
    AbilityInstanceId, CanonicalEncode, CardId, EffectId, Id, IdExhausted, IdGenerator,
    IdGenerators, LinkId, ObjectId, PlayerId, Timestamp,
};

/// Deterministic splitmix64 generator with a fixed seed (no time, no env).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

const EDGES: [u64; 12] = [
    0,
    1,
    2,
    50,
    99,
    100,
    101,
    41,
    42,
    u64::MAX - 1,
    u64::MAX,
    0x0102_0304_0506_0708,
];

/// Edge values followed by random values (a mix of full-range and small).
fn values(seed: u64) -> Vec<u64> {
    let mut v = EDGES.to_vec();
    let mut rng = Rng(seed);
    for k in 0..6_000 {
        let x = rng.next();
        v.push(if k % 3 == 0 { x % 1000 } else { x });
    }
    v
}

fn enc<T: CanonicalEncode>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// Runs the type-generic checks for one ID type.
fn check_id_type<I: Id + Debug>() {
    let vals = values(7);
    for w in vals.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ia, ib) = (I::from_raw(a), I::from_raw(b));
        assert_eq!(ia.cmp(&ib), a.cmp(&b));
        assert_eq!(ia == ib, a == b);
        assert_eq!(ia.partial_cmp(&ib), a.partial_cmp(&b));
    }
    for &x in &vals {
        let id = I::from_raw(x);
        assert_eq!(id.raw(), x);
        let b = enc(&id);
        assert_eq!(b.len(), 8);
        assert_eq!(u64::from_le_bytes(b.try_into().unwrap()), x);
    }
}

/// Runs the generator checks for one ID type.
fn check_generator<I: Id + Debug>() {
    let mut g = IdGenerator::<I>::new();
    assert_eq!(g.last_issued(), 0);
    let first = g.issue().unwrap();
    assert_eq!(first.raw(), 1);
    assert_eq!(g.last_issued(), 1);
    let mut prev = first;
    for n in 2..=10_000u64 {
        let next = g.issue().unwrap();
        assert!(next > prev);
        assert_eq!(next.raw(), n);
        assert_eq!(g.last_issued(), n);
        prev = next;
    }

    let mut g = IdGenerator::<I>::from_last_issued(41);
    assert_eq!(g.last_issued(), 41);
    assert_eq!(g.issue().unwrap().raw(), 42);
    assert_eq!(g.last_issued(), 42);

    let mut d = IdGenerator::<I>::default();
    assert_eq!(d, IdGenerator::<I>::new());
    assert_eq!(d.issue().unwrap().raw(), 1);

    let mut g = IdGenerator::<I>::from_last_issued(u64::MAX - 1);
    assert_eq!(g.issue().unwrap().raw(), u64::MAX);
    assert_eq!(g.last_issued(), u64::MAX);
    let snapshot = g.clone();
    for _ in 0..5 {
        assert_eq!(g.issue(), Err(IdExhausted));
        assert_eq!(g.last_issued(), u64::MAX);
        assert_eq!(g, snapshot);
    }
}

// REQ-CORE-002
#[test]
fn spec_ids_inherent_new_get_roundtrip() {
    macro_rules! check {
        ($($t:ident),*) => {$(
            for x in values(11) {
                assert_eq!($t::new(x).get(), x);
                assert_eq!($t::new(x), $t::new(x));
            }
            let (lo, hi) = ($t::new(1), $t::new(2));
            assert!(lo < hi && hi > lo && lo != hi);
            assert_eq!($t::new(u64::MAX).get(), u64::MAX);
        )*};
    }
    check!(
        CardId,
        ObjectId,
        AbilityInstanceId,
        EffectId,
        Timestamp,
        LinkId,
        PlayerId
    );
}

// REQ-CORE-002
#[test]
fn spec_ids_ordering_and_trait_roundtrip_all_types() {
    check_id_type::<CardId>();
    check_id_type::<ObjectId>();
    check_id_type::<AbilityInstanceId>();
    check_id_type::<EffectId>();
    check_id_type::<Timestamp>();
    check_id_type::<LinkId>();
    check_id_type::<PlayerId>();
}

// REQ-CORE-002
#[test]
fn spec_ids_inherent_ordering_is_numeric() {
    for w in values(12).windows(2) {
        let (a, b) = (w[0], w[1]);
        assert_eq!(ObjectId::new(a).cmp(&ObjectId::new(b)), a.cmp(&b));
        assert_eq!(Timestamp::new(a).cmp(&Timestamp::new(b)), a.cmp(&b));
        assert_eq!(CardId::new(a).cmp(&CardId::new(b)), a.cmp(&b));
        assert_eq!(PlayerId::new(a).cmp(&PlayerId::new(b)), a.cmp(&b));
    }
}

// REQ-CORE-002
#[test]
fn spec_id_generators_every_type() {
    check_generator::<CardId>();
    check_generator::<ObjectId>();
    check_generator::<AbilityInstanceId>();
    check_generator::<EffectId>();
    check_generator::<Timestamp>();
    check_generator::<LinkId>();
    check_generator::<PlayerId>();
}

// REQ-CORE-002
#[test]
fn spec_id_exhausted_traits() {
    fn is_error<E: std::error::Error>(_: &E) {}
    let e = IdExhausted;
    is_error(&e);
    let copy = e;
    assert_eq!(e, copy);
    assert!(!e.to_string().is_empty());
    assert!(!format!("{e:?}").is_empty());
}

// REQ-CORE-002
#[test]
fn spec_id_generators_bundle_independent() {
    let mut gens = IdGenerators::new();
    assert_eq!(gens, IdGenerators::default());
    assert_eq!(gens.issue_probe_free_state(), [0; 7]);
    let o = gens.object.issue().unwrap();
    assert_eq!(o, ObjectId::new(1));
    assert_eq!(gens.object.last_issued(), 1);
    assert_eq!(gens.last_all(), [0, 1, 0, 0, 0, 0, 0]);
    assert_ne!(gens, IdGenerators::new());

    gens.timestamp.issue().unwrap();
    gens.timestamp.issue().unwrap();
    gens.player.issue().unwrap();
    assert_eq!(gens.last_all(), [0, 1, 0, 0, 2, 0, 1]);
    assert_eq!(gens.card.issue().unwrap(), CardId::new(1));
    assert_eq!(
        gens.ability_instance.issue().unwrap(),
        AbilityInstanceId::new(1)
    );
    assert_eq!(gens.effect.issue().unwrap(), EffectId::new(1));
    assert_eq!(gens.link.issue().unwrap(), LinkId::new(1));
    assert_eq!(gens.last_all(), [1, 1, 1, 1, 2, 1, 1]);
}

/// Test-local helpers over the public fields.
trait Probe {
    fn last_all(&self) -> [u64; 7];
    fn issue_probe_free_state(&self) -> [u64; 7];
}

impl Probe for IdGenerators {
    fn last_all(&self) -> [u64; 7] {
        [
            self.card.last_issued(),
            self.object.last_issued(),
            self.ability_instance.last_issued(),
            self.effect.last_issued(),
            self.timestamp.last_issued(),
            self.link.last_issued(),
            self.player.last_issued(),
        ]
    }

    fn issue_probe_free_state(&self) -> [u64; 7] {
        self.last_all()
    }
}

// REQ-CORE-002
#[test]
fn spec_id_generators_clone_replays_sequence() {
    let mut a = IdGenerators::new();
    for _ in 0..10 {
        a.object.issue().unwrap();
        a.timestamp.issue().unwrap();
    }
    let mut b = a.clone();
    assert_eq!(a, b);
    for _ in 0..100 {
        assert_eq!(a.object.issue(), b.object.issue());
        assert_eq!(a.timestamp.issue(), b.timestamp.issue());
        assert_eq!(a.effect.issue(), b.effect.issue());
    }
    assert_eq!(a, b);
    // Diverging one clone does not affect the other.
    b.link.issue().unwrap();
    assert_ne!(a, b);
    assert_eq!(a.link.last_issued(), 0);
}

// REQ-CORE-002
#[test]
fn spec_id_generators_bundle_clone_of_generator_replays() {
    let mut g = IdGenerator::<ObjectId>::from_last_issued(5);
    let mut h = g.clone();
    for _ in 0..50 {
        assert_eq!(g.issue(), h.issue());
    }
}

// REQ-CORE-002
#[test]
fn spec_id_canonical_encoding_examples() {
    assert_eq!(
        enc(&ObjectId::new(0x0102_0304_0506_0708)),
        vec![8, 7, 6, 5, 4, 3, 2, 1]
    );
    assert_eq!(
        enc(&Timestamp::new(0x0102_0304_0506_0708)),
        vec![8, 7, 6, 5, 4, 3, 2, 1]
    );
    assert_eq!(enc(&CardId::new(1)), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(enc(&PlayerId::new(0)), vec![0; 8]);
    assert_eq!(enc(&LinkId::new(u64::MAX)), vec![0xFF; 8]);
    assert_eq!(enc(&EffectId::new(256)), vec![0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        enc(&AbilityInstanceId::new(0xAABB)),
        vec![0xBB, 0xAA, 0, 0, 0, 0, 0, 0]
    );
}

// REQ-CORE-002
#[test]
fn spec_id_canonical_encoding_appends_never_clears() {
    let mut out = Vec::new();
    ObjectId::new(1).encode_canonical(&mut out);
    PlayerId::new(2).encode_canonical(&mut out);
    assert_eq!(out.len(), 16);
    assert_eq!(&out[..8], &[1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&out[8..], &[2, 0, 0, 0, 0, 0, 0, 0]);

    let mut pre = vec![0xDE, 0xAD];
    CardId::new(3).encode_canonical(&mut pre);
    assert_eq!(pre.len(), 10);
    assert_eq!(&pre[..2], &[0xDE, 0xAD]);
    assert_eq!(&pre[2..], &[3, 0, 0, 0, 0, 0, 0, 0]);
}

// REQ-CORE-002
#[test]
fn spec_id_canonical_encoding_roundtrip_random() {
    for x in values(13) {
        let b = enc(&Timestamp::new(x));
        assert_eq!(b.len(), 8);
        assert_eq!(u64::from_le_bytes(b.clone().try_into().unwrap()), x);
        assert_eq!(b, x.to_le_bytes().to_vec());
    }
}
