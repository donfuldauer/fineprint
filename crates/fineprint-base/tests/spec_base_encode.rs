// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::encode` (byte layouts, injectivity, macro, existing impls).
//! Black-box: written from the work-item brief only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::{encode_to_vec, put_bytes, put_len, put_variant};
use fineprint_base::hash::{Digest, Label};
use fineprint_base::ids::{CanonicalEncode, ObjectId};
use fineprint_base::num::Int;
use fineprint_base::prng::Pcg64Dxsm;

fn enc<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    encode_to_vec(v)
}

/// Generic function bound on the `ids` trait, called with the value from the `encode` path.
fn via_ids_trait<T: fineprint_base::ids::CanonicalEncode>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// Generic function bound on the `encode` re-export of the trait.
fn via_encode_trait<T: fineprint_base::encode::CanonicalEncode>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// REQ-NFR-006: primitives encode as fixed-width little-endian at edge values.
#[test]
fn req_nfr_006_primitive_layouts() {
    assert_eq!(enc(&0u8), vec![0]);
    assert_eq!(enc(&1u8), vec![1]);
    assert_eq!(enc(&u8::MAX), vec![0xFF]);
    assert_eq!(enc(&0u16), vec![0, 0]);
    assert_eq!(enc(&0x0102u16), vec![0x02, 0x01]);
    assert_eq!(enc(&u16::MAX), vec![0xFF; 2]);
    assert_eq!(enc(&0u32), vec![0; 4]);
    assert_eq!(enc(&0x0102_0304u32), vec![4, 3, 2, 1]);
    assert_eq!(enc(&u32::MAX), vec![0xFF; 4]);
    assert_eq!(enc(&0u64), vec![0; 8]);
    assert_eq!(enc(&1u64), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(enc(&0x0102_0304_0506_0708u64), vec![8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(enc(&u64::MAX), vec![0xFF; 8]);
    assert_eq!(enc(&0i8), vec![0]);
    assert_eq!(enc(&-1i8), vec![0xFF]);
    assert_eq!(enc(&i8::MIN), vec![0x80]);
    assert_eq!(enc(&i8::MAX), vec![0x7F]);
    assert_eq!(enc(&-2i16), vec![0xFE, 0xFF]);
    assert_eq!(enc(&i16::MIN), vec![0x00, 0x80]);
    assert_eq!(enc(&i16::MAX), vec![0xFF, 0x7F]);
    assert_eq!(enc(&1i32), vec![1, 0, 0, 0]);
    assert_eq!(enc(&-1i32), vec![0xFF; 4]);
    assert_eq!(enc(&i32::MIN), vec![0, 0, 0, 0x80]);
    assert_eq!(enc(&i32::MAX), vec![0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(enc(&0i64), vec![0; 8]);
    assert_eq!(enc(&-1i64), vec![0xFF; 8]);
    assert_eq!(enc(&i64::MIN), vec![0, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(
        enc(&i64::MAX),
        vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]
    );
    assert_eq!(enc(&0x0102_0304_0506_0708i64), vec![8, 7, 6, 5, 4, 3, 2, 1]);
}

/// REQ-NFR-006: bool is one byte, 0 or 1.
#[test]
fn req_nfr_006_bool_layout() {
    assert_eq!(enc(&false), vec![0]);
    assert_eq!(enc(&true), vec![1]);
}

/// REQ-NFR-006: strings are length-prefixed UTF-8.
#[test]
fn req_nfr_006_string_layouts() {
    assert_eq!(enc(""), vec![0, 0, 0, 0]);
    assert_eq!(enc("ab"), vec![2, 0, 0, 0, b'a', b'b']);
    assert_eq!(enc(&String::from("ab")), vec![2, 0, 0, 0, b'a', b'b']);
    // U+00E9 (2 bytes), U+20AC (3 bytes), U+1F600 (4 bytes)
    assert_eq!(
        enc("\u{e9}\u{20ac}\u{1f600}"),
        vec![
            9, 0, 0, 0, 0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x9F, 0x98, 0x80
        ]
    );
    assert_eq!(enc(&String::new()), vec![0, 0, 0, 0]);
}

/// REQ-NFR-006: slices and Vecs are length-prefixed; arrays are not.
#[test]
fn req_nfr_006_sequence_layouts() {
    let empty: Vec<u8> = Vec::new();
    assert_eq!(enc(&empty), vec![0, 0, 0, 0]);
    assert_eq!(enc(&vec![1u8, 2, 3]), vec![3, 0, 0, 0, 1, 2, 3]);
    assert_eq!(enc(&[1u8, 2, 3][..]), vec![3, 0, 0, 0, 1, 2, 3]);
    assert_eq!(enc(&vec![0x0102u16, 3]), vec![2, 0, 0, 0, 2, 1, 3, 0]);
    assert_eq!(
        enc(&vec![vec![1u8], vec![]]),
        vec![2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0]
    );
    let nested_empty: Vec<Vec<u8>> = vec![];
    assert_eq!(enc(&nested_empty), vec![0, 0, 0, 0]);
    assert_eq!(enc(&[1u8, 2, 3]), vec![1, 2, 3]);
    assert_eq!(enc(&[0x0102u16, 3]), vec![2, 1, 3, 0]);
    let zero_arr: [u8; 0] = [];
    assert_eq!(enc(&zero_arr), Vec::<u8>::new());
    assert_eq!(enc(&[[1u8, 2], [3, 4]]), vec![1, 2, 3, 4]);
}

/// REQ-NFR-006: Option, tuples and references.
#[test]
fn req_nfr_006_option_tuple_ref_layouts() {
    assert_eq!(enc(&None::<u8>), vec![0]);
    assert_eq!(enc(&Some(7u8)), vec![1, 7]);
    assert_eq!(enc(&Some(0u8)), vec![1, 0]);
    assert_eq!(enc(&Some(Some(1u8))), vec![1, 1, 1]);
    assert_eq!(enc(&Some(None::<u8>)), vec![1, 0]);
    assert_eq!(enc(&Some(String::from("a"))), vec![1, 1, 0, 0, 0, b'a']);
    assert_eq!(enc(&(1u8, 2u16)), vec![1, 2, 0]);
    assert_eq!(enc(&(1u8, 2u16, true)), vec![1, 2, 0, 1]);
    assert_eq!(enc(&("a", 1u8)), vec![1, 0, 0, 0, b'a', 1]);
    let r: &u32 = &0x0A0B_0C0Du32;
    assert_eq!(enc(&r), enc(&0x0A0B_0C0Du32));
    assert_eq!(enc(&&&5u8), vec![5]);
    assert_eq!(enc(&"xy"), enc("xy"));
}

/// REQ-NFR-006: maps and sets encode count then ascending entries regardless of insertion order.
#[test]
fn req_nfr_006_map_set_layouts() {
    let mut m: BTreeMap<u8, u16> = BTreeMap::new();
    for (k, v) in [(30u8, 3u16), (10, 1), (20, 2), (0, 9)] {
        m.insert(k, v);
    }
    assert_eq!(
        enc(&m),
        vec![4, 0, 0, 0, 0, 9, 0, 10, 1, 0, 20, 2, 0, 30, 3, 0]
    );
    let mut s: BTreeSet<u16> = BTreeSet::new();
    for k in [0x0300u16, 1, 0x0200] {
        s.insert(k);
    }
    assert_eq!(enc(&s), vec![3, 0, 0, 0, 1, 0, 0, 2, 0, 3]);
    assert_eq!(enc(&BTreeMap::<u8, u8>::new()), vec![0, 0, 0, 0]);
    assert_eq!(enc(&BTreeSet::<u8>::new()), vec![0, 0, 0, 0]);
    let mut ms: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    ms.insert("b".to_string(), vec![2]);
    ms.insert("a".to_string(), vec![]);
    assert_eq!(
        enc(&ms),
        vec![
            2, 0, 0, 0, 1, 0, 0, 0, b'a', 0, 0, 0, 0, 1, 0, 0, 0, b'b', 1, 0, 0, 0, 2
        ]
    );
}

/// REQ-NFR-006: Digest and Label layouts.
#[test]
fn req_nfr_006_digest_and_label_layouts() {
    let mut raw = [0u8; 32];
    for (i, b) in raw.iter_mut().enumerate() {
        *b = i as u8;
    }
    assert_eq!(enc(&Digest::from_bytes(raw)), raw.to_vec());
    assert_eq!(enc(&Label::name("ab")), vec![0, 2, 0, 0, 0, b'a', b'b']);
    assert_eq!(enc(&Label::name("")), vec![0, 0, 0, 0, 0]);
    assert_eq!(enc(&Label::index(0)), vec![1, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        enc(&Label::index(0x0102_0304_0506_0708)),
        vec![1, 8, 7, 6, 5, 4, 3, 2, 1]
    );
    assert_eq!(enc(&Label::index(u64::MAX)), {
        let mut v = vec![1u8];
        v.extend([0xFF; 8]);
        v
    });
}

/// REQ-NFR-006: put_len boundary encodings.
#[test]
fn req_nfr_006_put_len_layouts() {
    let mut o = Vec::new();
    put_len(&mut o, 0);
    assert_eq!(o, vec![0, 0, 0, 0]);
    o.clear();
    put_len(&mut o, 1);
    assert_eq!(o, vec![1, 0, 0, 0]);
    o.clear();
    put_len(&mut o, 0x0102_0304);
    assert_eq!(o, vec![4, 3, 2, 1]);
    o.clear();
    put_len(&mut o, (u32::MAX - 1) as usize);
    assert_eq!(o, vec![0xFE, 0xFF, 0xFF, 0xFF]);
}

/// REQ-NFR-006: put_len escape form for lengths at or above u32::MAX.
#[cfg(target_pointer_width = "64")]
#[test]
fn req_nfr_006_put_len_escape_layouts() {
    let mut o = Vec::new();
    put_len(&mut o, u32::MAX as usize);
    assert_eq!(
        o,
        vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0]
    );
    o.clear();
    put_len(&mut o, u32::MAX as usize + 1);
    assert_eq!(o, vec![0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(o.len(), 12);
}

/// REQ-NFR-006: put_bytes and put_variant, and appending (never clearing).
#[test]
fn req_nfr_006_put_helpers_and_append() {
    let mut o = vec![0xAA, 0xBB];
    put_bytes(&mut o, &[9, 8]);
    assert_eq!(o, vec![0xAA, 0xBB, 2, 0, 0, 0, 9, 8]);
    put_variant(&mut o, 0x7E);
    assert_eq!(o, vec![0xAA, 0xBB, 2, 0, 0, 0, 9, 8, 0x7E]);
    put_bytes(&mut o, &[]);
    assert_eq!(&o[9..], &[0, 0, 0, 0]);
    put_len(&mut o, 3);
    assert_eq!(&o[13..], &[3, 0, 0, 0]);

    let mut o = vec![0x55];
    7u32.encode_canonical(&mut o);
    Some(1u8).encode_canonical(&mut o);
    "q".encode_canonical(&mut o);
    vec![1u8].encode_canonical(&mut o);
    (1u8, 2u8).encode_canonical(&mut o);
    assert_eq!(
        o,
        vec![
            0x55, 7, 0, 0, 0, 1, 1, 1, 0, 0, 0, b'q', 1, 0, 0, 0, 1, 1, 2
        ]
    );
}

/// REQ-NFR-006: pre-existing impls still encode as before, through both trait paths.
#[test]
fn req_nfr_006_existing_impls_unchanged() {
    assert_eq!(via_ids_trait(&Int::new(-2)), (-2i64).to_le_bytes().to_vec());
    assert_eq!(
        via_encode_trait(&Int::new(-2)),
        (-2i64).to_le_bytes().to_vec()
    );
    assert_eq!(enc(&Int::new(i64::MIN)), i64::MIN.to_le_bytes().to_vec());
    let id = ObjectId::new(0x0102_0304_0506_0708);
    assert_eq!(via_ids_trait(&id), vec![8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(via_encode_trait(&id), vec![8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(enc(&id), vec![8, 7, 6, 5, 4, 3, 2, 1]);
    let mut g = Pcg64Dxsm::from_u64_seed(42);
    let bytes = via_ids_trait(&g);
    assert_eq!(bytes.len(), 32);
    assert_eq!(bytes, via_encode_trait(&g));
    assert_eq!(bytes, enc(&g));
    g.next_u64();
    assert_ne!(enc(&g), bytes);
    // Existing impls nest inside the new containers without extra tags.
    assert_eq!(enc(&vec![id]), {
        let mut v = vec![1, 0, 0, 0];
        v.extend([8, 7, 6, 5, 4, 3, 2, 1]);
        v
    });
    assert_eq!(enc(&Some(Int::new(1))), vec![1, 1, 0, 0, 0, 0, 0, 0, 0]);
}

struct Pair {
    a: u8,
    b: String,
}
fineprint_base::impl_canonical_struct!(Pair { a, b });

struct Outer {
    zone_a: Pair,
    perm_0007: Vec<u16>,
    flag: bool,
    inner: Option<Pair>,
}
fineprint_base::encode::impl_canonical_struct!(Outer {
    zone_a,
    perm_0007,
    flag,
    inner
});

/// REQ-NFR-006: the struct macro encodes listed fields in order, with no prefix.
#[test]
fn req_nfr_006_struct_macro() {
    let p = Pair {
        a: 7,
        b: "z".to_string(),
    };
    assert_eq!(enc(&p), vec![7, 1, 0, 0, 0, b'z']);
    let o = Outer {
        zone_a: Pair {
            a: 1,
            b: String::new(),
        },
        perm_0007: vec![0x0203],
        flag: true,
        inner: Some(Pair {
            a: 2,
            b: "y".to_string(),
        }),
    };
    assert_eq!(
        enc(&o),
        vec![
            1, 0, 0, 0, 0, // zone_a
            1, 0, 0, 0, 3, 2, // perm_0007
            1, // flag
            1, 2, 1, 0, 0, 0, b'y', // inner
        ]
    );
    let o2 = Outer {
        zone_a: Pair {
            a: 1,
            b: String::new(),
        },
        perm_0007: vec![],
        flag: false,
        inner: None,
    };
    assert_eq!(enc(&o2), vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        enc(&vec![Pair {
            a: 1,
            b: String::new()
        }]),
        vec![1, 0, 0, 0, 1, 0, 0, 0, 0]
    );
}

/// Deterministic generator over the crate's own PRNG with a literal seed.
struct Gen(Pcg64Dxsm);
impl Gen {
    fn new(seed: u64) -> Gen {
        Gen(Pcg64Dxsm::from_u64_seed(seed))
    }
    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }
    fn small_bytes(&mut self) -> Vec<u8> {
        let n = self.below(4) as usize;
        (0..n).map(|_| self.below(3) as u8).collect()
    }
    fn small_string(&mut self) -> String {
        let n = self.below(3) as usize;
        (0..n)
            .map(|_| ["a", "b", "\u{e9}", "\u{0}"][self.below(4) as usize])
            .collect()
    }
}

fn check_pair<T: CanonicalEncode + PartialEq + std::fmt::Debug>(
    a: &T,
    b: &T,
    equal_seen: &mut u32,
) {
    let ea = enc(a);
    let eb = enc(b);
    assert_eq!(enc(a), ea, "determinism");
    assert_eq!(a == b, ea == eb, "injectivity: {a:?} vs {b:?}");
    if a == b {
        *equal_seen += 1;
    }
}

/// REQ-NFR-006: encoding is injective and deterministic on generated nested values.
#[test]
fn req_nfr_006_injective_on_generated_values() {
    let mut g = Gen::new(0x1234_5678);
    let mut eq = [0u32; 5];
    for _ in 0..6000 {
        // Vec<Option<(u8, String)>>
        let mk = |g: &mut Gen| -> Vec<Option<(u8, String)>> {
            (0..g.below(3))
                .map(|_| {
                    if g.below(2) == 0 {
                        None
                    } else {
                        Some((g.below(2) as u8, g.small_string()))
                    }
                })
                .collect()
        };
        let (a, b) = (mk(&mut g), mk(&mut g));
        check_pair(&a, &b, &mut eq[0]);

        // BTreeMap<u64, Vec<i64>>
        let mk = |g: &mut Gen| -> BTreeMap<u64, Vec<i64>> {
            (0..g.below(3))
                .map(|_| {
                    (
                        g.below(3),
                        (0..g.below(3)).map(|_| g.below(3) as i64 - 1).collect(),
                    )
                })
                .collect()
        };
        let (a, b) = (mk(&mut g), mk(&mut g));
        check_pair(&a, &b, &mut eq[1]);

        // (Vec<u8>, Vec<u8>)
        let a = (g.small_bytes(), g.small_bytes());
        let b = (g.small_bytes(), g.small_bytes());
        check_pair(&a, &b, &mut eq[2]);

        // [u16; 3]
        let mk =
            |g: &mut Gen| -> [u16; 3] { [g.below(2) as u16, g.below(2) as u16, g.below(2) as u16] };
        let (a, b) = (mk(&mut g), mk(&mut g));
        check_pair(&a, &b, &mut eq[3]);

        // Vec<Vec<u8>>
        let mk =
            |g: &mut Gen| -> Vec<Vec<u8>> { (0..g.below(3)).map(|_| g.small_bytes()).collect() };
        let (a, b) = (mk(&mut g), mk(&mut g));
        check_pair(&a, &b, &mut eq[4]);
    }
    for (i, n) in eq.iter().enumerate() {
        assert!(*n > 20, "family {i} saw too few equal pairs: {n}");
    }
}

/// REQ-NFR-006: adversarial fixed cases that a naive concatenation encoder would conflate.
#[test]
fn req_nfr_006_injective_adversarial_cases() {
    assert_ne!(enc(&vec![vec![1u8], vec![2]]), enc(&vec![vec![1u8, 2]]));
    assert_ne!(enc(&Some(0u8)), enc(&None::<u8>));
    assert_ne!(enc(&("ab", "c")), enc(&("a", "bc")));
    assert_ne!(enc(&Vec::<u8>::new()), enc(&vec![0u8]));
    // empty string and empty vec share a layout by contract (both are an empty length-prefixed run)
    assert_eq!(enc(""), enc(&Vec::<u8>::new()));
    assert_ne!(
        enc(&(vec![1u8], vec![2u8, 3])),
        enc(&(vec![1u8, 2], vec![3u8]))
    );
    assert_ne!(enc(&vec![None::<u8>, None]), enc(&vec![None::<u8>]));
    assert_ne!(enc(&vec![Some(None::<u8>)]), enc(&vec![None::<Option<u8>>]));
    assert_ne!(enc(&Label::name("")), enc(&Label::index(0)));
}

/// REQ-NFR-006: hostile inputs never panic (empty, long, large).
#[test]
fn req_nfr_006_hostile_inputs_do_not_panic() {
    let big = vec![0xA5u8; 10_000];
    let e = enc(&big);
    assert_eq!(e.len(), 10_004);
    let long = "x".repeat(70_000);
    assert_eq!(enc(long.as_str()).len(), 70_004);
    let mut o = Vec::new();
    put_bytes(&mut o, &big);
    assert_eq!(o, e);
    let _ = enc(&Label::name(&long));
}
