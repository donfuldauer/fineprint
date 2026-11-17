// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Ordered collections and the stable (ObjectId, Timestamp) sort (REQ-CORE-006).
//!
//! [`OrderedMap`] and [`OrderedSet`] are the only map and set types rules code may use.
//! Iteration in hash order is forbidden: wherever iteration order can affect the outcome, use an
//! ordered container or [`stable_sort_by_object_timestamp`].

use crate::ids::{ObjectId, Timestamp};

/// The map type for rules code: iteration is in key order (REQ-CORE-006).
pub type OrderedMap<K, V> = std::collections::BTreeMap<K, V>;

/// The set type for rules code: iteration is in element order (REQ-CORE-006).
pub type OrderedSet<T> = std::collections::BTreeSet<T>;

/// Sorts `items` ascending by the key `(ObjectId, Timestamp)` returned by `key` (REQ-CORE-006).
///
/// The sort is stable: items with equal keys keep their original relative order.
pub fn stable_sort_by_object_timestamp<T, F>(items: &mut [T], key: F)
where
    F: FnMut(&T) -> (ObjectId, Timestamp),
{
    items.sort_by_key(key);
}

#[cfg(test)]
mod tests {
    // REQ-CORE-006: ordered containers and stable sort on (ObjectId, Timestamp).
    use super::*;
    use crate::num::testkit::Rng;

    // REQ-CORE-006
    #[test]
    fn ordered_aliases_iterate_in_order() {
        let mut m: OrderedMap<ObjectId, u8> = OrderedMap::new();
        for k in [5, 1, 9, 3] {
            m.insert(ObjectId::new(k), 0);
        }
        let keys: Vec<u64> = m.keys().map(|k| k.get()).collect();
        assert_eq!(keys, [1, 3, 5, 9]);
        let s: OrderedSet<Timestamp> = [7, 2, 4].into_iter().map(Timestamp::new).collect();
        let v: Vec<u64> = s.iter().map(|t| t.get()).collect();
        assert_eq!(v, [2, 4, 7]);
    }

    // REQ-CORE-006: fixed example with ties.
    #[test]
    fn sort_example_keeps_ties_in_order() {
        let o = ObjectId::new;
        let t = Timestamp::new;
        let mut items = vec![
            (o(2), t(1), 'a'),
            (o(1), t(5), 'b'),
            (o(1), t(2), 'c'),
            (o(2), t(1), 'd'),
            (o(1), t(5), 'e'),
        ];
        stable_sort_by_object_timestamp(&mut items, |x| (x.0, x.1));
        let order: String = items.iter().map(|x| x.2).collect();
        assert_eq!(order, "cbead");
        let mut empty: Vec<(ObjectId, Timestamp)> = Vec::new();
        stable_sort_by_object_timestamp(&mut empty, |x| *x);
        assert!(empty.is_empty());
    }

    // REQ-CORE-006: property, matches a reference sort on (ObjectId, Timestamp, original index).
    #[test]
    fn prop_matches_reference() {
        let mut rng = Rng::new(0x5027_0008);
        for _ in 0..5_000 {
            let len = usize::try_from(rng.next_u64() % 24).unwrap();
            let items: Vec<(ObjectId, Timestamp, usize)> = (0..len)
                .map(|i| {
                    (
                        ObjectId::new(rng.next_u64() % 4),
                        Timestamp::new(rng.next_u64() % 4),
                        i,
                    )
                })
                .collect();
            let mut sorted = items.clone();
            stable_sort_by_object_timestamp(&mut sorted, |x| (x.0, x.1));
            let mut reference = items;
            reference.sort_by_key(|x| (x.0, x.1, x.2));
            assert_eq!(sorted, reference);
            assert!(
                sorted
                    .windows(2)
                    .all(|w| (w[0].0, w[0].1) <= (w[1].0, w[1].1))
            );
        }
    }
}
