// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::txn` errors: unknown, stale and non-innermost transaction
//! ids, open transactions at `into_state`, and id uniqueness (REQ-CORE-005). A failed call
//! changes nothing. Black-box: written from the work-item brief only.

#[path = "spec_txn_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::txn::{Transactions, TxnError, TxnId};
use support::*;

/// A `TxnId` never issued by a fresh instance: taken from another instance advanced further.
fn foreign_id(advance: usize) -> TxnId {
    let mut other = Transactions::new(Toy::new(0));
    let mut last = None;
    for _ in 0..advance {
        let id = other.checkpoint();
        other.commit(id).unwrap();
        last = Some(id);
    }
    last.unwrap()
}

/// REQ-CORE-005: an id never issued by this instance is `UnknownTxn` for every call, at depth 0
/// and with open transactions; nothing changes.
#[test]
fn never_issued_id_is_unknown() {
    let far = foreign_id(64);
    let mut txns = Transactions::new(game(2));
    for call in ALL_CALLS {
        expect_err(&mut txns, call, far, &TxnError::UnknownTxn(far));
    }
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 2);
    let _a = txns.checkpoint();
    draw(txns.state_mut(), 1);
    for call in ALL_CALLS {
        expect_err(&mut txns, call, far, &TxnError::UnknownTxn(far));
    }
    assert_eq!(txns.depth(), 2);
    let _ = r;
}

/// REQ-CORE-005, semantics f: an id is stale after commit, after commit_durable and after
/// rollback; every call on it is `UnknownTxn` and changes nothing.
#[test]
fn stale_ids_are_unknown() {
    let mut txns = Transactions::new(game(2));
    let root = txns.checkpoint();

    let committed = txns.checkpoint();
    draw(txns.state_mut(), 1);
    txns.commit(committed).unwrap();

    let durable = txns.checkpoint();
    draw(txns.state_mut(), 1);
    txns.commit_durable(durable).unwrap();

    let rolled = txns.checkpoint();
    draw(txns.state_mut(), 1);
    txns.rollback(rolled).unwrap();

    set_life(txns.state_mut(), 2, 5);
    for stale in [committed, durable, rolled] {
        for call in ALL_CALLS {
            expect_err(&mut txns, call, stale, &TxnError::UnknownTxn(stale));
        }
    }
    // Stale ids stay unknown with a newer frame open above the root.
    let _inner = txns.checkpoint();
    for stale in [committed, durable, rolled] {
        for call in ALL_CALLS {
            expect_err(&mut txns, call, stale, &TxnError::UnknownTxn(stale));
        }
    }
    let _ = root;
}

/// REQ-CORE-005, semantics f: rolling back the same id twice, and rolling back after commit,
/// give `UnknownTxn`; also after the stack is empty.
#[test]
fn double_rollback_and_rollback_after_commit() {
    let mut txns = Transactions::new(game(2));
    let a = txns.checkpoint();
    set_life(txns.state_mut(), 1, 1);
    txns.rollback(a).unwrap();
    expect_err(&mut txns, Call::Rollback, a, &TxnError::UnknownTxn(a));
    let b = txns.checkpoint();
    set_life(txns.state_mut(), 1, 1);
    txns.commit(b).unwrap();
    expect_err(&mut txns, Call::Rollback, b, &TxnError::UnknownTxn(b));
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 1);
    assert_eq!(txns.depth(), 0);
}

/// REQ-CORE-005, semantics b: an open id that is not the innermost gives `NotInnermost` with
/// the requested and innermost ids, for every call and at several depths; nothing changes.
#[test]
fn not_innermost_reports_both_ids() {
    let mut txns = Transactions::new(game(3));
    let a = txns.checkpoint();
    set_life(txns.state_mut(), 1, 10);
    let b = txns.checkpoint();
    set_life(txns.state_mut(), 2, 10);
    for call in ALL_CALLS {
        expect_err(
            &mut txns,
            call,
            a,
            &TxnError::NotInnermost {
                requested: a,
                innermost: b,
            },
        );
    }
    let c = txns.checkpoint();
    draw(txns.state_mut(), 2);
    for requested in [a, b] {
        for call in ALL_CALLS {
            expect_err(
                &mut txns,
                call,
                requested,
                &TxnError::NotInnermost {
                    requested,
                    innermost: c,
                },
            );
        }
    }
    // The innermost still works afterwards and the stack unwinds normally.
    txns.rollback(c).unwrap();
    expect_err(
        &mut txns,
        Call::Commit,
        a,
        &TxnError::NotInnermost {
            requested: a,
            innermost: b,
        },
    );
    txns.commit(b).unwrap();
    txns.commit(a).unwrap();
    assert_eq!(txns.depth(), 0);
}

/// REQ-CORE-005, semantics g: `into_state` with open transactions reports the open count.
#[test]
fn into_state_with_open_transactions() {
    for open in 1..=4usize {
        let mut txns = Transactions::new(game(2));
        for _ in 0..open {
            txns.checkpoint();
        }
        assert_eq!(txns.depth(), open);
        let err = txns.into_state().unwrap_err();
        assert_eq!(err, TxnError::OpenTransactions { open });
        assert!(!err.to_string().is_empty());
    }
    // After closing some, the count reflects the remaining ones.
    let mut txns = Transactions::new(game(2));
    let _a = txns.checkpoint();
    let b = txns.checkpoint();
    let c = txns.checkpoint();
    txns.commit(c).unwrap();
    txns.rollback(b).unwrap();
    assert_eq!(
        txns.into_state().unwrap_err(),
        TxnError::OpenTransactions { open: 1 }
    );
}

/// REQ-CORE-005, semantics a: ids increase and are never reused across many commit, durable
/// commit and rollback cycles and nesting depths.
#[test]
fn ids_are_increasing_and_never_reused() {
    let mut txns = Transactions::new(Toy::new(0));
    let mut all: Vec<TxnId> = Vec::new();
    for round in 0..200usize {
        let depth = 1 + round % 4;
        let mut stack = Vec::new();
        for _ in 0..depth {
            let id = txns.checkpoint();
            if let Some(prev) = all.last() {
                assert!(id > *prev, "ids increase");
            }
            all.push(id);
            stack.push(id);
            txns.state_mut().log.push(round as u8);
        }
        for (k, id) in stack.into_iter().rev().enumerate() {
            match (round + k) % 3 {
                0 => txns.commit(id).unwrap(),
                1 => txns.commit_durable(id).unwrap(),
                _ => {
                    txns.rollback(id).unwrap();
                }
            }
        }
        assert_eq!(txns.depth(), 0);
    }
    let set: BTreeSet<TxnId> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len());
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len());
}

/// REQ-CORE-005: `Display` of every `TxnError` variant is non-empty, and it is a
/// `std::error::Error`.
#[test]
fn txn_error_display_non_empty() {
    let mut txns = Transactions::new(Toy::new(0));
    let a = txns.checkpoint();
    let b = txns.checkpoint();
    let errs = [
        TxnError::UnknownTxn(a),
        TxnError::NotInnermost {
            requested: a,
            innermost: b,
        },
        TxnError::OpenTransactions { open: 3 },
    ];
    for e in &errs {
        assert!(!e.to_string().is_empty(), "{e:?}");
        let dyn_err: &dyn std::error::Error = e;
        assert!(!dyn_err.to_string().is_empty());
    }
}
