// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::txn`: checkpoint and rollback restore the exact prior state
//! hash, the player who had priority keeps it, and plain nesting (REQ-CORE-005, CR 734.1,
//! CR 734.2). Black-box: written from the work-item brief only.

#[path = "spec_txn_support.rs"]
mod support;

use fineprint_core::state::GameState;
use fineprint_core::txn::{RollbackReport, Transactional, Transactions};
use support::*;

/// REQ-CORE-005, CR 734.1: checkpoint, mutate every public component, rollback; the state, its
/// hash and the report's hash equal the pre-checkpoint ones, for 2, 3 and 4 players.
#[test]
fn rollback_restores_exact_hash_for_2_3_4_players() {
    for players in [2usize, 3, 4] {
        let original = game(players);
        let h0 = original.state_hash();
        let mut txns = Transactions::new(original.clone());
        assert_eq!(txns.depth(), 0);
        let id = txns.checkpoint();
        assert_eq!(txns.depth(), 1);
        mutate_everything(txns.state_mut());
        assert_ne!(txns.state().state_hash(), h0, "mutation must be visible");
        let report = txns.rollback(id).unwrap();
        assert_eq!(txns.depth(), 0);
        assert_eq!(txns.state().state_hash(), h0, "{players} players");
        assert_eq!(txns.state(), &original, "{players} players");
        assert_eq!(report.state_hash, h0);
        assert_eq!(
            report,
            RollbackReport {
                priority_player: Some(pid(1)),
                state_hash: h0
            }
        );
        assert_eq!(txns.into_state().unwrap(), original);
    }
}

/// REQ-CORE-005, CR 734.1: each component on its own is restored by rollback.
#[test]
fn rollback_restores_each_component_separately() {
    type Mutation = fn(&mut GameState);
    let muts: [(&str, Mutation); 12] = [
        ("life", |s| set_life(s, 2, 1)),
        ("counter", |s| {
            s.player_mut(pid(1))
                .unwrap()
                .set_counter("test.counter_a", 9)
        }),
        ("designation", |s| {
            s.player_mut(pid(2))
                .unwrap()
                .clear_designation("test.level");
        }),
        ("prng", |s| draw(s, 1)),
        ("card id", |s| {
            s.issue_card_id().unwrap();
        }),
        ("object id", |s| issue_objects(s, 1)),
        ("effect id", |s| {
            s.issue_effect_id().unwrap();
        }),
        ("timestamp", |s| {
            s.issue_timestamp().unwrap();
        }),
        ("priority", |s| s.set_priority_player(Some(pid(2))).unwrap()),
        ("active", |s| s.set_active_player(Some(pid(2))).unwrap()),
        ("turn number", |s| s.set_turn_number(99)),
        ("turn order", |s| {
            s.set_turn_order(vec![pid(2), pid(1)]).unwrap()
        }),
    ];
    for (name, m) in muts {
        let original = game(2);
        let h0 = original.state_hash();
        let mut txns = Transactions::new(original.clone());
        let id = txns.checkpoint();
        m(txns.state_mut());
        assert_ne!(txns.state().state_hash(), h0, "{name} must change the hash");
        let report = txns.rollback(id).unwrap();
        assert_eq!(txns.state().state_hash(), h0, "{name}");
        assert_eq!(txns.state(), &original, "{name}");
        assert_eq!(report.state_hash, h0, "{name}");
    }
}

/// REQ-CORE-005, CR 734.1: rollback with zero mutations returns the identical state and hash.
#[test]
fn rollback_with_zero_mutations() {
    let original = game(2);
    let h0 = original.state_hash();
    let mut txns = Transactions::new(original.clone());
    let id = txns.checkpoint();
    let report = txns.rollback(id).unwrap();
    assert_eq!(report.state_hash, h0);
    assert_eq!(report.priority_player, Some(pid(1)));
    assert_eq!(txns.state(), &original);
    assert_eq!(txns.depth(), 0);
}

/// REQ-CORE-005: `Transactional for GameState` delegates `state_hash` and `priority_player`.
#[test]
fn gamestate_transactional_delegates() {
    let mut s = game(3);
    assert_eq!(Transactional::state_hash(&s), GameState::state_hash(&s));
    assert_eq!(Transactional::priority_player(&s), Some(pid(1)));
    s.set_priority_player(Some(pid(3))).unwrap();
    assert_eq!(Transactional::priority_player(&s), Some(pid(3)));
    s.set_priority_player(None).unwrap();
    assert_eq!(Transactional::priority_player(&s), None);
}

/// REQ-CORE-005, CR 734.2: P1 (the first seat) has priority at checkpoint; during the action
/// priority passes and the active player changes; after rollback the first player has priority
/// again, both in the report and in the state.
#[test]
fn priority_is_retained_after_rollback() {
    for players in [2usize, 3, 4] {
        let mut txns = Transactions::new(game(players));
        assert_eq!(txns.state().priority_player(), Some(pid(1)));
        let id = txns.checkpoint();
        txns.state_mut().set_priority_player(Some(pid(2))).unwrap();
        txns.state_mut().set_active_player(Some(pid(2))).unwrap();
        set_life(txns.state_mut(), 1, 4);
        let report = txns.rollback(id).unwrap();
        assert_eq!(report.priority_player, Some(pid(1)));
        assert_eq!(txns.state().turn().priority_player(), Some(pid(1)));
        assert_eq!(txns.state().turn().active_player(), Some(pid(1)));
        assert_eq!(
            report,
            RollbackReport {
                priority_player: txns.state().priority_player(),
                state_hash: txns.state().state_hash()
            }
        );
    }
}

/// REQ-CORE-005, CR 734.2: priority `None` at checkpoint stays `None` after rollback.
#[test]
fn priority_none_is_retained_after_rollback() {
    let mut s = game(2);
    s.set_priority_player(None).unwrap();
    let h0 = s.state_hash();
    let mut txns = Transactions::new(s);
    let id = txns.checkpoint();
    txns.state_mut().set_priority_player(Some(pid(2))).unwrap();
    let report = txns.rollback(id).unwrap();
    assert_eq!(report.priority_player, None);
    assert_eq!(txns.state().turn().priority_player(), None);
    assert_eq!(report.state_hash, h0);
}

/// REQ-CORE-005: rolling back the inner frame keeps the outer frame's changes.
#[test]
fn inner_rollback_keeps_outer_changes() {
    let mut txns = Transactions::new(game(2));
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 11);
    let mid = txns.state().clone();
    let h_mid = mid.state_hash();
    let inner = txns.checkpoint();
    assert_eq!(txns.depth(), 2);
    set_life(txns.state_mut(), 2, 12);
    draw(txns.state_mut(), 3);
    txns.state_mut().set_priority_player(Some(pid(2))).unwrap();
    let report = txns.rollback(inner).unwrap();
    assert_eq!(txns.depth(), 1);
    assert_eq!(txns.state(), &mid);
    assert_eq!(report.state_hash, h_mid);
    assert_eq!(report.priority_player, Some(pid(1)));
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 11);
    txns.commit(outer).unwrap();
    assert_eq!(txns.depth(), 0);
    assert_eq!(txns.into_state().unwrap(), mid);
}

/// REQ-CORE-005, CR 734.1: a plainly committed inner frame is reverted by the outer rollback.
#[test]
fn inner_commit_then_outer_rollback_reverts_inner() {
    let original = game(3);
    let h0 = original.state_hash();
    let mut txns = Transactions::new(original.clone());
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 11);
    let inner = txns.checkpoint();
    set_life(txns.state_mut(), 2, 12);
    draw(txns.state_mut(), 2);
    issue_objects(txns.state_mut(), 2);
    let live = txns.state().clone();
    txns.commit(inner).unwrap();
    assert_eq!(txns.depth(), 1);
    assert_eq!(
        txns.state(),
        &live,
        "commit leaves the live state unchanged"
    );
    let report = txns.rollback(outer).unwrap();
    assert_eq!(txns.depth(), 0);
    assert_eq!(txns.state(), &original);
    assert_eq!(report.state_hash, h0);
}

/// REQ-CORE-005: committing everything keeps every change; `into_state` returns it; depth is
/// tracked after every call.
#[test]
fn commit_all_keeps_changes() {
    let mut txns = Transactions::new(game(2));
    let a = txns.checkpoint();
    assert_eq!(txns.depth(), 1);
    set_life(txns.state_mut(), 1, 1);
    let b = txns.checkpoint();
    assert_eq!(txns.depth(), 2);
    draw(txns.state_mut(), 1);
    let c = txns.checkpoint();
    assert_eq!(txns.depth(), 3);
    issue_objects(txns.state_mut(), 4);
    txns.state_mut().set_priority_player(Some(pid(2))).unwrap();
    let expected = txns.state().clone();
    let h = expected.state_hash();
    txns.commit(c).unwrap();
    assert_eq!(txns.depth(), 2);
    assert_eq!(txns.state().state_hash(), h);
    txns.commit(b).unwrap();
    assert_eq!(txns.depth(), 1);
    assert_eq!(txns.state().state_hash(), h);
    txns.commit(a).unwrap();
    assert_eq!(txns.depth(), 0);
    assert_eq!(txns.state().state_hash(), h);
    let out = txns.into_state().unwrap();
    assert_eq!(out, expected);
    assert_eq!(out.turn().priority_player(), Some(pid(2)));
}

/// REQ-CORE-005: `into_state` at depth 0 without any transaction returns the given state, and
/// `state_mut` outside any transaction mutates directly.
#[test]
fn into_state_without_transactions() {
    let mut txns = Transactions::new(game(2));
    assert_eq!(txns.depth(), 0);
    set_life(txns.state_mut(), 2, 3);
    let mut expected = game(2);
    set_life(&mut expected, 2, 3);
    assert_eq!(txns.into_state().unwrap(), expected);
}

/// REQ-CORE-005: sequential (sibling) transactions: each rollback returns to its own checkpoint.
#[test]
fn sibling_transactions_restore_their_own_checkpoints() {
    let mut txns = Transactions::new(game(2));
    let root = txns.checkpoint();
    set_life(txns.state_mut(), 1, 15);
    let s1 = txns.state().clone();
    let a = txns.checkpoint();
    draw(txns.state_mut(), 2);
    txns.commit(a).unwrap();
    let s2 = txns.state().clone();
    let b = txns.checkpoint();
    set_life(txns.state_mut(), 2, 1);
    txns.rollback(b).unwrap();
    assert_eq!(txns.state(), &s2);
    assert_ne!(txns.state(), &s1);
    txns.commit(root).unwrap();
    assert_eq!(txns.into_state().unwrap(), s2);
}
