// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the canonical encoding (REQ-NFR-006).
//!
//! Byte layouts are pinned with literal expectations; injectivity is checked by a hand-rolled
//! deterministic property harness (PCG64-DXSM with literal seeds, at least 5 000 values per
//! property) plus a fixed list of adversarial boundaries.

use std::collections::{BTreeMap, BTreeSet};

use super::{CanonicalEncode, encode_to_vec, put_bytes, put_len, put_variant};
use crate::ids::{ObjectId, PlayerId};
use crate::num::{Fixed2, Int};
use crate::prng::Pcg64Dxsm;

const ITERATIONS: usize = 5_000;

fn enc<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    encode_to_vec(v)
}

#[test]
fn req_nfr_006_unsigned_integers_are_fixed_width_le() {
    assert_eq!(enc(&0u8), [0]);
    assert_eq!(enc(&1u8), [1]);
    assert_eq!(enc(&u8::MAX), [0xff]);
    assert_eq!(enc(&0x0102u16), [0x02, 0x01]);
    assert_eq!(enc(&u16::MAX), [0xff; 2]);
    assert_eq!(enc(&0u16), [0; 2]);
    assert_eq!(enc(&0x0102_0304u32), [4, 3, 2, 1]);
    assert_eq!(enc(&u32::MAX), [0xff; 4]);
    assert_eq!(enc(&1u32), [1, 0, 0, 0]);
    assert_eq!(enc(&0x0102_0304_0506_0708u64), [8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(enc(&u64::MAX), [0xff; 8]);
    assert_eq!(enc(&0u64), [0; 8]);
}

#[test]
fn req_nfr_006_signed_integers_are_fixed_width_le_twos_complement() {
    assert_eq!(enc(&0i8), [0]);
    assert_eq!(enc(&-1i8), [0xff]);
    assert_eq!(enc(&i8::MIN), [0x80]);
    assert_eq!(enc(&i8::MAX), [0x7f]);
    assert_eq!(enc(&0x0102i16), [2, 1]);
    assert_eq!(enc(&i16::MIN), [0, 0x80]);
    assert_eq!(enc(&i16::MAX), [0xff, 0x7f]);
    assert_eq!(enc(&0x0102_0304i32), [4, 3, 2, 1]);
    assert_eq!(enc(&i32::MIN), [0, 0, 0, 0x80]);
    assert_eq!(enc(&-1i32), [0xff; 4]);
    assert_eq!(enc(&1i64), [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(enc(&i64::MIN), [0, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(
        enc(&i64::MAX),
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]
    );
    assert_eq!(enc(&0x0102_0304_0506_0708i64), [8, 7, 6, 5, 4, 3, 2, 1]);
}

#[test]
fn req_nfr_006_bool_is_one_byte() {
    assert_eq!(enc(&false), [0]);
    assert_eq!(enc(&true), [1]);
}

#[test]
fn req_nfr_006_strings_are_length_prefixed_utf8() {
    assert_eq!(enc(""), [0, 0, 0, 0]);
    assert_eq!(enc("ab"), [2, 0, 0, 0, b'a', b'b']);
    assert_eq!(enc(&String::from("ab")), enc("ab"));
    // Two-byte UTF-8 character: the length counts bytes.
    assert_eq!(enc("\u{e9}"), [2, 0, 0, 0, 0xc3, 0xa9]);
}

#[test]
fn req_nfr_006_sequences_and_arrays() {
    assert_eq!(enc(&Vec::<u8>::new()), [0, 0, 0, 0]);
    assert_eq!(enc(&vec![1u8, 2]), [2, 0, 0, 0, 1, 2]);
    assert_eq!(enc(&[1u16, 2][..]), [2, 0, 0, 0, 1, 0, 2, 0]);
    assert_eq!(enc(&vec![0x0102u16]), enc(&[0x0102u16][..]));
    // Arrays carry no length.
    assert_eq!(enc(&[1u8, 2, 3]), [1, 2, 3]);
    assert_eq!(enc(&[0u32; 0]), Vec::<u8>::new());
    assert_eq!(enc(&[[1u8, 2], [3, 4]]), [1, 2, 3, 4]);
}

#[test]
fn req_nfr_006_option_and_tuples() {
    assert_eq!(enc(&None::<u8>), [0]);
    assert_eq!(enc(&Some(0u8)), [1, 0]);
    assert_eq!(enc(&Some(Some(7u8))), [1, 1, 7]);
    assert_eq!(enc(&Some(None::<u8>)), [1, 0]);
    assert_eq!(enc(&(1u8, 0x0203u16)), [1, 3, 2]);
    assert_eq!(enc(&(1u8, true, "a")), [1, 1, 1, 0, 0, 0, b'a']);
}

#[test]
fn req_nfr_006_ordered_containers_encode_in_ascending_order() {
    let mut map = BTreeMap::new();
    for k in [3u8, 1, 2] {
        map.insert(k, u16::from(k) * 10);
    }
    assert_eq!(enc(&map), [3, 0, 0, 0, 1, 10, 0, 2, 20, 0, 3, 30, 0]);
    let set: BTreeSet<u8> = [9u8, 4, 7].into_iter().collect();
    assert_eq!(enc(&set), [3, 0, 0, 0, 4, 7, 9]);
    assert_eq!(enc(&BTreeMap::<u8, u8>::new()), [0, 0, 0, 0]);
    assert_eq!(enc(&BTreeSet::<u8>::new()), [0, 0, 0, 0]);
}

#[test]
fn req_nfr_006_references_delegate() {
    let v = 0x0102u16;
    assert_eq!(enc(&&v), enc(&v));
    assert_eq!(enc(&&&"xy"), enc("xy"));
    let refs: Vec<&u8> = vec![&1, &2];
    assert_eq!(enc(&refs), enc(&vec![1u8, 2]));
}

#[test]
fn req_nfr_006_put_len_short_form() {
    let mut out = vec![0xaa];
    put_len(&mut out, 0);
    put_len(&mut out, 0x0102_0304);
    assert_eq!(out, [0xaa, 0, 0, 0, 0, 4, 3, 2, 1], "put_len appends");
}

#[cfg(target_pointer_width = "64")]
#[test]
fn req_nfr_006_put_len_escape_at_and_above_u32_max() {
    let below = u32::MAX as usize - 1;
    let mut out = Vec::new();
    put_len(&mut out, below);
    assert_eq!(out, [0xfe, 0xff, 0xff, 0xff]);

    let mut out = Vec::new();
    put_len(&mut out, u32::MAX as usize);
    assert_eq!(
        out,
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
    );

    let mut out = Vec::new();
    put_len(&mut out, u32::MAX as usize + 1);
    assert_eq!(out, [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 1, 0, 0, 0]);

    let mut out = Vec::new();
    put_len(&mut out, usize::MAX);
    assert_eq!(out, [0xff; 12]);

    // Distinct lengths around the boundary give distinct prefixes.
    let mut seen = BTreeSet::new();
    for len in [
        below - 1,
        below,
        below + 1,
        below + 2,
        below + 3,
        usize::MAX - 1,
        usize::MAX,
    ] {
        let mut out = Vec::new();
        put_len(&mut out, len);
        assert!(seen.insert(out), "duplicate prefix for {len}");
    }
}

#[test]
fn req_nfr_006_put_bytes_and_variant() {
    let mut out = vec![9];
    put_bytes(&mut out, &[7, 8]);
    put_variant(&mut out, 5);
    put_bytes(&mut out, &[]);
    assert_eq!(out, [9, 2, 0, 0, 0, 7, 8, 5, 0, 0, 0, 0]);
}

/// Accepts any value through the re-exported trait path.
fn via_encode_trait<T: super::CanonicalEncode>(v: &T) -> usize {
    encode_to_vec(v).len()
}

/// Accepts any value through the original trait path.
fn via_ids_trait<T: crate::ids::CanonicalEncode>(v: &T) -> usize {
    via_encode_trait(v)
}

#[test]
fn req_nfr_006_encode_trait_is_the_ids_trait() {
    assert_eq!(via_ids_trait(&Int::new(-1)), 8);
    assert_eq!(via_encode_trait(&Fixed2::from_raw(5)), 8);
    assert_eq!(via_encode_trait(&ObjectId::new(3)), 8);
    assert_eq!(via_ids_trait(&Pcg64Dxsm::from_u64_seed(1)), 32);
    assert_eq!(via_encode_trait(&7u8), 1);
    assert_eq!(enc(&ObjectId::new(0x0102)), enc(&0x0102u64));
    assert_eq!(enc(&Some(PlayerId::new(1))), [1, 1, 0, 0, 0, 0, 0, 0, 0]);
}

struct SyntheticRecord {
    alpha: u8,
    beta: Option<u16>,
    gamma: String,
}
crate::impl_canonical_struct!(SyntheticRecord { alpha, beta, gamma });

struct SyntheticEmpty;
crate::impl_canonical_struct!(SyntheticEmpty {});

struct SyntheticReordered {
    first: u8,
    second: u8,
}
super::impl_canonical_struct!(SyntheticReordered { second, first });

#[test]
fn req_nfr_006_struct_macro_encodes_fields_in_listed_order() {
    let r = SyntheticRecord {
        alpha: 1,
        beta: Some(0x0302),
        gamma: "z".to_owned(),
    };
    assert_eq!(enc(&r), [1, 1, 2, 3, 1, 0, 0, 0, b'z']);
    assert_eq!(enc(&SyntheticEmpty), Vec::<u8>::new());
    assert_eq!(
        enc(&SyntheticReordered {
            first: 1,
            second: 2
        }),
        [2, 1]
    );
}

#[test]
fn req_nfr_006_adversarial_boundaries_are_distinct() {
    assert_ne!(enc(&vec![vec![1u8], vec![2]]), enc(&vec![vec![1u8, 2]]));
    assert_ne!(enc(&Some(0u8)), enc(&None::<u8>));
    assert_ne!(enc(&("ab", "c")), enc(&("a", "bc")));
    assert_ne!(enc(&(String::new(), "a")), enc(&("a", String::new())));
    assert_ne!(enc(&vec![Vec::<u8>::new()]), enc(&Vec::<Vec<u8>>::new()));
    assert_ne!(enc(&Some(None::<u8>)), enc(&None::<Option<u8>>));
    assert_ne!(enc(&vec![Some(0u8)]), enc(&vec![None::<u8>, None]));
    let one: BTreeMap<u8, Vec<u8>> = [(1, vec![2])].into_iter().collect();
    let two: BTreeMap<u8, Vec<u8>> = [(1, vec![]), (2, vec![])].into_iter().collect();
    assert_ne!(enc(&one), enc(&two));
}

// ---------------------------------------------------------------------------------------------
// Property harness: generators over PCG64-DXSM with small alphabets so near-collisions occur.
// ---------------------------------------------------------------------------------------------

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap_or(0)
}

fn gen_small_string(rng: &mut Pcg64Dxsm) -> String {
    let len = below(rng, 4);
    (0..len)
        .map(|_| if below(rng, 2) == 0 { 'a' } else { 'b' })
        .collect()
}

fn gen_u8(rng: &mut Pcg64Dxsm) -> u8 {
    const EDGES: [u8; 5] = [0, 1, 2, 0x7f, 0xff];
    EDGES[usize::try_from(below(rng, 5)).unwrap_or(0)]
}

fn gen_i64(rng: &mut Pcg64Dxsm) -> i64 {
    const EDGES: [i64; 6] = [0, 1, -1, i64::MIN, i64::MAX, 0x0102];
    EDGES[usize::try_from(below(rng, 6)).unwrap_or(0)]
}

fn gen_nested(rng: &mut Pcg64Dxsm) -> Vec<Vec<Option<(u8, String)>>> {
    (0..below(rng, 3))
        .map(|_| {
            (0..below(rng, 3))
                .map(|_| {
                    if below(rng, 3) == 0 {
                        None
                    } else {
                        Some((gen_u8(rng), gen_small_string(rng)))
                    }
                })
                .collect()
        })
        .collect()
}

fn gen_map(rng: &mut Pcg64Dxsm) -> BTreeMap<u64, Vec<i64>> {
    (0..below(rng, 3))
        .map(|_| {
            (
                below(rng, 3),
                (0..below(rng, 3)).map(|_| gen_i64(rng)).collect(),
            )
        })
        .collect()
}

fn gen_array_tuple(rng: &mut Pcg64Dxsm) -> ([u8; 2], (String, String, Option<bool>)) {
    let arr = [gen_u8(rng), gen_u8(rng)];
    let flag = match below(rng, 3) {
        0 => None,
        1 => Some(false),
        _ => Some(true),
    };
    (arr, (gen_small_string(rng), gen_small_string(rng), flag))
}

/// Generates `ITERATIONS` values and checks, across every pair of them, that equal bytes imply
/// equal values and equal values imply equal bytes. Returns the number of distinct values seen.
fn check_injective<T, G>(seed: u64, mut generate: G) -> usize
where
    T: CanonicalEncode + PartialEq + core::fmt::Debug,
    G: FnMut(&mut Pcg64Dxsm) -> T,
{
    let mut rng = Pcg64Dxsm::from_u64_seed(seed);
    let mut by_bytes: BTreeMap<Vec<u8>, T> = BTreeMap::new();
    for _ in 0..ITERATIONS {
        let a = generate(&mut rng);
        let b = generate(&mut rng);
        let (ea, eb) = (enc(&a), enc(&b));
        assert_eq!(a == b, ea == eb, "pair {a:?} / {b:?}");
        for (v, e) in [(a, ea), (b, eb)] {
            match by_bytes.get(&e) {
                Some(prev) => assert_eq!(prev, &v, "two values share bytes {e:?}"),
                None => {
                    by_bytes.insert(e, v);
                }
            }
        }
    }
    by_bytes.len()
}

#[test]
fn req_nfr_006_property_injective_nested_vec_option_tuple() {
    let distinct = check_injective(0x5eed_0001, gen_nested);
    assert!(distinct > 100, "generator too narrow: {distinct}");
}

#[test]
fn req_nfr_006_property_injective_btreemap() {
    let distinct = check_injective(0x5eed_0002, gen_map);
    assert!(distinct > 100, "generator too narrow: {distinct}");
}

#[test]
fn req_nfr_006_property_injective_arrays_and_tuples() {
    let distinct = check_injective(0x5eed_0003, gen_array_tuple);
    assert!(distinct > 100, "generator too narrow: {distinct}");
}

#[test]
fn req_nfr_006_property_equal_values_equal_bytes() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x5eed_0004);
    for _ in 0..ITERATIONS {
        let a = gen_nested(&mut rng);
        let b = a.clone();
        assert_eq!(enc(&a), enc(&b));
        let m = gen_map(&mut rng);
        // Rebuild the map inserting in descending key order: same bytes.
        let rebuilt: BTreeMap<u64, Vec<i64>> =
            m.iter().rev().map(|(k, v)| (*k, v.clone())).collect();
        assert_eq!(enc(&m), enc(&rebuilt));
    }
}
