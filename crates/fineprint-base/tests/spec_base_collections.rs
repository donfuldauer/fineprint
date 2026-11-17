// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::collections`. Black-box: written from the work-item brief only.

use fineprint_base::collections::{OrderedMap, OrderedSet, stable_sort_by_object_timestamp};
use fineprint_base::ids::{ObjectId, Timestamp};
use std::collections::BTreeMap;

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

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for k in (1..v.len()).rev() {
            let j = self.below(k as u64 + 1) as usize;
            v.swap(k, j);
        }
    }
}

fn o(x: u64) -> ObjectId {
    ObjectId::new(x)
}

fn t(x: u64) -> Timestamp {
    Timestamp::new(x)
}

fn sorted_keys(n: u64) -> Vec<u64> {
    (0..n).map(|k| k * 3 + 1).collect()
}

// REQ-CORE-006
#[test]
fn spec_ordered_map_iterates_ascending_any_insertion_order() {
    let asc = sorted_keys(200);
    let mut desc = asc.clone();
    desc.reverse();
    let mut orders = vec![asc.clone(), desc];
    let mut rng = Rng(0x1234);
    for _ in 0..5 {
        let mut s = asc.clone();
        rng.shuffle(&mut s);
        orders.push(s);
    }
    for order in orders {
        let mut m: OrderedMap<ObjectId, u64> = OrderedMap::new();
        for &k in &order {
            m.insert(o(k), k * 2);
        }
        let keys: Vec<u64> = m.keys().map(|k| k.get()).collect();
        assert_eq!(keys, asc);
        let vals: Vec<u64> = m.values().copied().collect();
        assert_eq!(vals, asc.iter().map(|k| k * 2).collect::<Vec<_>>());
        let pairs: Vec<(u64, u64)> = m.iter().map(|(k, v)| (k.get(), *v)).collect();
        assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0));
    }
}

// REQ-CORE-006
#[test]
fn spec_ordered_set_iterates_ascending_any_insertion_order() {
    let asc = sorted_keys(200);
    let mut desc = asc.clone();
    desc.reverse();
    let mut orders = vec![asc.clone(), desc];
    let mut rng = Rng(0x5678);
    for _ in 0..5 {
        let mut s = asc.clone();
        rng.shuffle(&mut s);
        // Duplicates must collapse.
        let extra: Vec<u64> = s.iter().take(10).copied().collect();
        s.extend(extra);
        orders.push(s);
    }
    for order in orders {
        let mut set: OrderedSet<ObjectId> = OrderedSet::new();
        for &k in &order {
            set.insert(o(k));
        }
        let keys: Vec<u64> = set.iter().map(|k| k.get()).collect();
        assert_eq!(keys, asc);
        assert_eq!(set.len(), asc.len());
    }
}

// REQ-CORE-006
#[test]
fn spec_ordered_map_set_are_btree_aliases() {
    fn takes_btree<K: Ord, V>(m: BTreeMap<K, V>) -> usize {
        m.len()
    }
    fn takes_btree_set<T: Ord>(s: std::collections::BTreeSet<T>) -> usize {
        s.len()
    }
    let mut m: OrderedMap<ObjectId, u8> = OrderedMap::new();
    m.insert(o(2), 1);
    m.insert(o(1), 2);
    assert_eq!(takes_btree(m), 2);
    let mut s: OrderedSet<Timestamp> = OrderedSet::new();
    s.insert(t(9));
    assert_eq!(takes_btree_set(s), 1);
    let plain: BTreeMap<ObjectId, u8> = BTreeMap::new();
    let alias: OrderedMap<ObjectId, u8> = plain;
    assert!(alias.is_empty());
}

fn reference(items: &[(usize, u64, u64)]) -> Vec<usize> {
    let mut v = items.to_vec();
    v.sort_by_key(|&(idx, ob, ts)| (ob, ts, idx));
    v.into_iter().map(|(idx, _, _)| idx).collect()
}

fn run_sort(items: &mut [(usize, u64, u64)]) -> Vec<usize> {
    stable_sort_by_object_timestamp(items, |&(_, ob, ts)| (o(ob), t(ts)));
    items.iter().map(|&(idx, _, _)| idx).collect()
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_empty_and_single() {
    let mut empty: Vec<(usize, u64, u64)> = Vec::new();
    stable_sort_by_object_timestamp(&mut empty, |&(_, ob, ts)| (o(ob), t(ts)));
    assert!(empty.is_empty());
    let mut one = vec![(0usize, 5u64, 5u64)];
    assert_eq!(run_sort(&mut one), vec![0]);
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_sorted_and_reverse() {
    let mut sorted: Vec<(usize, u64, u64)> = (0..100).map(|k| (k, k as u64, 0)).collect();
    assert_eq!(run_sort(&mut sorted), (0..100).collect::<Vec<_>>());
    let mut rev: Vec<(usize, u64, u64)> = (0..100).map(|k| (k, 100 - k as u64, 0)).collect();
    let expect: Vec<usize> = (0..100).rev().collect();
    assert_eq!(run_sort(&mut rev), expect);
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_equal_keys_keep_original_order() {
    let mut items: Vec<(usize, u64, u64)> = (0..500).map(|k| (k, 7, 3)).collect();
    assert_eq!(run_sort(&mut items), (0..500).collect::<Vec<_>>());

    // Few distinct keys, many ties.
    let mut rng = Rng(99);
    let mut items: Vec<(usize, u64, u64)> = (0..2_000)
        .map(|k| (k, rng.below(4), rng.below(3)))
        .collect();
    let expect = reference(&items);
    assert_eq!(run_sort(&mut items), expect);
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_timestamp_breaks_object_ties() {
    let mut items = vec![(0usize, 1u64, 30u64), (1, 1, 10), (2, 0, 99), (3, 1, 20)];
    assert_eq!(run_sort(&mut items), vec![2, 1, 3, 0]);
    // Object id dominates timestamp.
    let mut items = vec![(0usize, 2u64, 1u64), (1, 1, 100)];
    assert_eq!(run_sort(&mut items), vec![1, 0]);
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_random_matches_reference() {
    let mut rng = Rng(0xABCD_EF01);
    for round in 0..8u64 {
        let range = if round % 2 == 0 { 50 } else { 1_000_000 };
        let mut items: Vec<(usize, u64, u64)> = (0..5_000)
            .map(|k| (k, rng.below(range), rng.below(range)))
            .collect();
        let expect = reference(&items);
        assert_eq!(run_sort(&mut items), expect);
        // Payload still pairs with its key (permutation, not corruption).
        assert!(
            items
                .windows(2)
                .all(|w| (w[0].1, w[0].2) <= (w[1].1, w[1].2))
        );
    }
}

// REQ-CORE-006
#[test]
fn spec_stable_sort_extreme_ids() {
    let mut items = vec![
        (0usize, u64::MAX, u64::MAX),
        (1, 0, 0),
        (2, u64::MAX, 0),
        (3, 0, u64::MAX),
    ];
    assert_eq!(run_sort(&mut items), vec![1, 3, 2, 0]);
}
