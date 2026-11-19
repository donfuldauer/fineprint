// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::txn::SnapshotStore`, the engine-level snapshot store behind
//! sandbox rewind (REQ-API-003, REQ-CORE-005). It is not governed by CR 734 and is independent
//! of `Transactions`. Black-box: written from the work-item brief only.

#[path = "spec_txn_support.rs"]
mod support;

use fineprint_core::txn::{SnapshotError, SnapshotStore, Transactions};
use support::*;

fn store_of(capacity: usize, values: &[i64]) -> SnapshotStore<i64> {
    let mut s = SnapshotStore::new(capacity).unwrap();
    for v in values {
        s.record(v);
    }
    s
}

/// REQ-API-003: capacity 0 is rejected.
#[test]
fn zero_capacity_is_an_error() {
    assert!(matches!(
        SnapshotStore::<i64>::new(0),
        Err(SnapshotError::ZeroCapacity)
    ));
    let e = SnapshotError::ZeroCapacity;
    assert!(!e.to_string().is_empty());
    let e2 = SnapshotError::NotEnoughSnapshots {
        requested: 3,
        available: 1,
    };
    assert!(!e2.to_string().is_empty());
    let d: &dyn std::error::Error = &e2;
    assert!(!d.to_string().is_empty());
}

/// REQ-API-003: record, len, latest, is_empty and capacity.
#[test]
fn record_len_latest_capacity() {
    let mut s = SnapshotStore::<i64>::new(4).unwrap();
    assert_eq!(s.capacity(), 4);
    assert_eq!(s.len(), 0);
    assert!(s.is_empty());
    assert_eq!(s.latest(), None);
    s.record(&10);
    assert_eq!(s.len(), 1);
    assert!(!s.is_empty());
    assert_eq!(s.latest(), Some(&10));
    s.record(&11);
    s.record(&12);
    assert_eq!(s.len(), 3);
    assert_eq!(s.latest(), Some(&12));
    assert_eq!(s.capacity(), 4);
    let one = store_of(1, &[]);
    assert_eq!(one.capacity(), 1);
}

/// REQ-API-003: at capacity, recording drops the oldest.
#[test]
fn eviction_of_oldest_at_capacity() {
    let mut s = store_of(3, &[1, 2, 3]);
    assert_eq!(s.len(), 3);
    s.record(&4);
    assert_eq!(s.len(), 3);
    assert_eq!(s.latest(), Some(&4));
    s.record(&5);
    assert_eq!(s.len(), 3);
    // Oldest remaining is 3: rewinding by 2 reaches it, by 3 is not possible.
    assert_eq!(
        s.rewind(3),
        Err(SnapshotError::NotEnoughSnapshots {
            requested: 3,
            available: 3
        })
    );
    assert_eq!(s.rewind(2), Ok(3));
    assert_eq!(s.len(), 1);
    // Capacity 1 keeps only the newest.
    let mut one = store_of(1, &[7, 8, 9]);
    assert_eq!(one.len(), 1);
    assert_eq!(one.latest(), Some(&9));
    assert_eq!(one.rewind(0), Ok(9));
}

/// REQ-API-003: rewind(0) returns the latest and leaves the store unchanged.
#[test]
fn rewind_zero_returns_latest_unchanged() {
    let mut s = store_of(5, &[1, 2, 3]);
    assert_eq!(s.rewind(0), Ok(3));
    assert_eq!(s.len(), 3);
    assert_eq!(s.latest(), Some(&3));
    assert_eq!(s.rewind(1), Ok(2));
}

/// REQ-API-003: rewind(n) returns the snapshot n records before the latest and discards the n
/// newest, for every valid n.
#[test]
fn rewind_n_for_several_n() {
    let values = [10, 20, 30, 40, 50];
    for n in 0..values.len() {
        let mut s = store_of(8, &values);
        let want = values[values.len() - 1 - n];
        assert_eq!(s.rewind(n), Ok(want), "n = {n}");
        assert_eq!(s.len(), values.len() - n, "n = {n}");
        assert_eq!(s.latest(), Some(&want), "n = {n}");
        assert_eq!(s.capacity(), 8);
    }
    // Successive rewinds compose.
    let mut s = store_of(8, &values);
    assert_eq!(s.rewind(1), Ok(40));
    assert_eq!(s.rewind(2), Ok(20));
    assert_eq!(s.len(), 2);
    assert_eq!(s.rewind(1), Ok(10));
    assert_eq!(s.len(), 1);
}

/// REQ-API-003: rewind(n >= len) is an error and leaves the store unchanged.
#[test]
fn rewind_too_far_is_error_and_unchanged() {
    let mut empty = SnapshotStore::<i64>::new(3).unwrap();
    assert_eq!(
        empty.rewind(0),
        Err(SnapshotError::NotEnoughSnapshots {
            requested: 0,
            available: 0
        })
    );
    assert!(empty.is_empty());
    let mut s = store_of(6, &[1, 2, 3]);
    for n in [3usize, 4, 100, usize::MAX] {
        assert_eq!(
            s.rewind(n),
            Err(SnapshotError::NotEnoughSnapshots {
                requested: n,
                available: 3
            })
        );
        assert_eq!(s.len(), 3);
        assert_eq!(s.latest(), Some(&3));
    }
    assert_eq!(s.rewind(2), Ok(1));
}

/// REQ-API-003: after a rewind, recording continues from the rewound snapshot.
#[test]
fn rewind_then_record_continues() {
    let mut s = store_of(5, &[1, 2, 3, 4]);
    assert_eq!(s.rewind(2), Ok(2));
    s.record(&9);
    assert_eq!(s.len(), 3);
    assert_eq!(s.latest(), Some(&9));
    assert_eq!(s.rewind(1), Ok(2));
    assert_eq!(s.rewind(1), Ok(1));
    assert_eq!(s.len(), 1);
}

/// REQ-API-003, REQ-CORE-005: with GameState, the rewound state equals the recorded one and its
/// hash matches; recording never changes the state handed in.
#[test]
fn works_with_gamestate() {
    let mut s = SnapshotStore::new(4).unwrap();
    let mut g = game(2);
    let mut hashes = Vec::new();
    let mut states = Vec::new();
    for step in 0..4i64 {
        let before = g.clone();
        s.record(&g);
        assert_eq!(g, before, "record never changes the state");
        hashes.push(g.state_hash());
        states.push(g.clone());
        set_life(&mut g, 1, 20 - step);
        draw(&mut g, 1);
    }
    assert_eq!(s.latest().unwrap().state_hash(), hashes[3]);
    let back = s.rewind(2).unwrap();
    assert_eq!(back.state_hash(), hashes[1]);
    assert_eq!(back, states[1]);
    assert_eq!(s.latest(), Some(&states[1]));
}

/// REQ-API-003, REQ-CORE-005: a Transactions rollback never touches a SnapshotStore holding
/// earlier clones, and rewinding a store does not affect an open Transactions.
#[test]
fn independent_of_transactions() {
    let mut txns = Transactions::new(game(2));
    let mut store = SnapshotStore::new(4).unwrap();
    store.record(txns.state());
    let t0 = txns.state().clone();
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 5);
    draw(txns.state_mut(), 2);
    store.record(txns.state());
    let t1 = txns.state().clone();
    let b = txns.checkpoint();
    set_life(txns.state_mut(), 2, 6);
    store.record(txns.state());
    let t2 = txns.state().clone();

    // Rolling back the inner transaction leaves the store as it was.
    txns.rollback(b).unwrap();
    assert_eq!(store.len(), 3);
    assert_eq!(store.latest(), Some(&t2));

    // Rewinding the store leaves the open Transactions as it was.
    let live = txns.state().clone();
    let depth = txns.depth();
    assert_eq!(store.rewind(1), Ok(t1.clone()));
    assert_eq!(txns.state(), &live);
    assert_eq!(txns.depth(), depth);

    // Rolling back the root leaves the store unchanged too.
    txns.rollback(r).unwrap();
    assert_eq!(txns.state(), &t0);
    assert_eq!(store.len(), 2);
    assert_eq!(store.latest(), Some(&t1));
    assert_eq!(store.rewind(1), Ok(t0));
}
