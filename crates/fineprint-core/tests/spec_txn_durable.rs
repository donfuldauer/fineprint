// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::txn` durable sub-transactions: a sub-transaction committed
//! with `commit_durable` survives the rollback of every ancestor (REQ-CORE-005, CR 734.1), and
//! the player who had priority keeps it (CR 734.2). Library-touching mana abilities are
//! modelled by PRNG draws and id issues (brief CONTRACTS note). Black-box: written from the
//! work-item brief only.

#[path = "spec_txn_support.rs"]
mod support;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::PlayerId;
use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::txn::{Transactional, Transactions, TxnId};
use support::*;

/// REQ-CORE-005, CR 734.1, CR 734.2 (acceptance: "A committed sub-transaction survives
/// rollback of its parent"): the outer action changes player 1's life and passes priority; a
/// durable sub draws from the PRNG and issues object ids; the outer action then changes player
/// 2's life; rolling back the outer action leaves exactly the checkpoint state plus the sub's
/// PRNG advance and issued ids.
#[test]
fn durable_sub_survives_parent_rollback() {
    let mut txns = Transactions::new(game(2));
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 13);
    txns.state_mut().set_priority_player(Some(pid(2))).unwrap();
    let sub = txns.checkpoint();
    draw(txns.state_mut(), 4);
    issue_objects(txns.state_mut(), 3);
    txns.commit_durable(sub).unwrap();
    assert_eq!(txns.depth(), 1);
    set_life(txns.state_mut(), 2, 7);
    let report = txns.rollback(outer).unwrap();
    assert_eq!(txns.depth(), 0);

    // Expected, built independently from the checkpoint state.
    let mut expected = checkpoint_state.clone();
    draw(&mut expected, 4);
    issue_objects(&mut expected, 3);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    assert_eq!(txns.state(), &expected);
    assert_ne!(expected.state_hash(), checkpoint_state.state_hash());
    // The outer action's own changes are reverted.
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 20);
    assert_eq!(txns.state().player(pid(2)).unwrap().life(), 20);
    // CR 734.2: priority is the checkpoint's.
    assert_eq!(report.priority_player, Some(pid(1)));
    assert_eq!(txns.state().turn().priority_player(), Some(pid(1)));
    assert_eq!(report.state_hash, expected.state_hash());
    assert_eq!(txns.into_state().unwrap(), expected);
}

/// REQ-CORE-005, CR 734.1: `commit_durable` leaves the live state unchanged at commit time.
#[test]
fn commit_durable_leaves_live_state_unchanged() {
    let mut txns = Transactions::new(game(2));
    let outer = txns.checkpoint();
    let sub = txns.checkpoint();
    draw(txns.state_mut(), 2);
    let live = txns.state().clone();
    txns.commit_durable(sub).unwrap();
    assert_eq!(txns.state(), &live);
    txns.commit(outer).unwrap();
    assert_eq!(txns.into_state().unwrap(), live);
}

/// REQ-CORE-005, CR 734.1, CR 734.2: a durable sub that changes player 2's life, a counter
/// (set and removed) and designations (set and cleared) keeps exactly those after the parent
/// rollback; the outer action's later change to player 2 and its earlier change to player 1
/// are reverted; a priority change inside the sub is not carried.
#[test]
fn durable_sub_player_state_variant() {
    let mut txns = Transactions::new(game(3));
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 9);
    txns.state_mut().set_active_player(Some(pid(3))).unwrap();
    let sub = txns.checkpoint();
    {
        let mut p = txns.state_mut().player_mut(pid(2)).unwrap();
        p.set_life(17);
        p.set_counter("test.counter_c", 4);
        p.set_counter("test.counter_b", 0); // removes the counter
        p.set_designation("test.level", DesignationValue::Int(2));
        p.set_designation("test.label", DesignationValue::Text("sub".to_owned()));
        p.clear_designation("test.flag");
    }
    txns.state_mut().set_priority_player(Some(pid(3))).unwrap();
    txns.commit_durable(sub).unwrap();
    set_life(txns.state_mut(), 2, 1);
    txns.state_mut()
        .player_mut(pid(2))
        .unwrap()
        .set_counter("test.counter_d", 3);
    let report = txns.rollback(outer).unwrap();

    let mut expected = checkpoint_state.clone();
    {
        let mut p = expected.player_mut(pid(2)).unwrap();
        p.set_life(17);
        p.set_counter("test.counter_c", 4);
        p.set_counter("test.counter_b", 0);
        p.set_designation("test.level", DesignationValue::Int(2));
        p.set_designation("test.label", DesignationValue::Text("sub".to_owned()));
        p.clear_designation("test.flag");
    }
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    let p2 = txns.state().player(pid(2)).unwrap();
    assert_eq!(p2.counter("test.counter_b"), 0);
    assert!(!p2.counters().contains_key("test.counter_b"));
    assert!(!p2.counters().contains_key("test.counter_d"));
    assert!(p2.designation("test.flag").is_none());
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 20);
    assert_eq!(txns.state().turn().active_player(), Some(pid(1)));
    assert_eq!(report.priority_player, Some(pid(1)));
    assert_eq!(txns.state().turn().priority_player(), Some(pid(1)));
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 1): a durable sub carries only its own change to a
/// player (the before -> after delta over the restored checkpoint). An outer-action change to
/// the same player made before the sub began is not carried: the checkpoint life is restored,
/// and only the sub's new counter survives.
#[test]
fn durable_sub_carries_only_its_own_player_delta() {
    let mut txns = Transactions::new(game(2));
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 8);
    let sub = txns.checkpoint();
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_c", 1);
    txns.commit_durable(sub).unwrap();
    txns.rollback(outer).unwrap();
    let mut expected = checkpoint_state;
    expected
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_c", 1);
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    // The outer-only life change (20 -> 8) is reverted, not carried.
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 20);
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 1): same player, same field. Life is 20 at the
/// outer checkpoint, the outer action sets 8, the durable sub goes 8 -> 5; after the outer
/// rollback the life is 20 + (5 - 8) = 17.
#[test]
fn durable_sub_life_delta_over_outer_change_same_player() {
    let mut txns = Transactions::new(game(2));
    let checkpoint_state = txns.state().clone();
    assert_eq!(checkpoint_state.player(pid(1)).unwrap().life(), 20);
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 8);
    let sub = txns.checkpoint();
    set_life(txns.state_mut(), 1, 5);
    txns.commit_durable(sub).unwrap();
    let report = txns.rollback(outer).unwrap();
    let mut expected = checkpoint_state;
    set_life(&mut expected, 1, 17);
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    assert_eq!(report.state_hash, expected.state_hash());
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 17);
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 1): same player, same counter key. The counter is 2
/// at the outer checkpoint, the outer action sets 5, the durable sub goes 5 -> 7; after the
/// outer rollback it is 2 + (7 - 5) = 4.
#[test]
fn durable_sub_counter_delta_over_outer_change_same_key() {
    let mut txns = Transactions::new(game(2));
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 2);
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 5);
    let sub = txns.checkpoint();
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 7);
    txns.commit_durable(sub).unwrap();
    txns.rollback(outer).unwrap();
    let mut expected = checkpoint_state;
    expected
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 4);
    assert_eq!(txns.state(), &expected);
    assert_eq!(
        txns.state()
            .player(pid(1))
            .unwrap()
            .counter("test.counter_x"),
        4
    );
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 1, decided over-decrement policy): the counter is 1
/// at the outer checkpoint, the outer action raises it to 6, the durable sub takes it 6 -> 0.
/// The carried count 1 + (0 - 6) is clamped at 0, so the entry is absent; life is untouched.
#[test]
fn durable_sub_counter_over_decrement_clamps_at_zero() {
    let mut txns = Transactions::new(game(2));
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 1);
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 6);
    let sub = txns.checkpoint();
    txns.state_mut()
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 0);
    txns.commit_durable(sub).unwrap();
    txns.rollback(outer).unwrap();
    let mut expected = checkpoint_state;
    expected
        .player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_x", 0);
    assert_eq!(txns.state(), &expected);
    let p1 = txns.state().player(pid(1)).unwrap();
    assert_eq!(p1.counter("test.counter_x"), 0);
    assert!(!p1.counters().contains_key("test.counter_x"));
    assert_eq!(p1.life(), 20);
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 1): with outer-action changes to the same player
/// before the sub began, the durable sub's counter removal and its designation addition,
/// change and removal are carried, while the outer-only counter and designation changes are
/// not; a designation changed by both takes the sub's (`after`'s) value.
#[test]
fn durable_sub_carries_counter_and_designation_changes_not_outer_ones() {
    let mut txns = Transactions::new(game(2));
    let checkpoint_state = txns.state().clone();
    let outer = txns.checkpoint();
    {
        let mut p = txns.state_mut().player_mut(pid(1)).unwrap();
        p.set_counter("test.counter_a", 9); // outer only
        p.set_designation("test.outer_only", DesignationValue::Int(3)); // outer only
        p.set_designation("test.level", DesignationValue::Int(5)); // outer, then sub
    }
    let sub = txns.checkpoint();
    {
        let mut p = txns.state_mut().player_mut(pid(1)).unwrap();
        p.set_counter("test.counter_b", 0); // removed by the sub
        p.set_designation("test.label", DesignationValue::Text("sub".to_owned())); // added
        p.set_designation("test.level", DesignationValue::Int(2)); // changed
        p.clear_designation("test.flag"); // removed
    }
    txns.commit_durable(sub).unwrap();
    txns.rollback(outer).unwrap();
    let mut expected = checkpoint_state;
    {
        let mut p = expected.player_mut(pid(1)).unwrap();
        p.set_counter("test.counter_b", 0);
        p.set_designation("test.label", DesignationValue::Text("sub".to_owned()));
        p.set_designation("test.level", DesignationValue::Int(2));
        p.clear_designation("test.flag");
    }
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    let p1 = txns.state().player(pid(1)).unwrap();
    assert_eq!(p1.counter("test.counter_a"), 2);
    assert!(!p1.counters().contains_key("test.counter_b"));
    assert!(p1.designation("test.outer_only").is_none());
    assert!(p1.designation("test.flag").is_none());
    assert_eq!(
        p1.designation("test.level"),
        Some(&DesignationValue::Int(2))
    );
}

/// REQ-CORE-005, CR 734.1: a durable sub that changes nothing carries nothing.
#[test]
fn durable_sub_with_no_change_carries_nothing() {
    let original = game(2);
    let mut txns = Transactions::new(original.clone());
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 1, 2);
    draw(txns.state_mut(), 1);
    let sub = txns.checkpoint();
    txns.commit_durable(sub).unwrap();
    let report = txns.rollback(outer).unwrap();
    assert_eq!(txns.state(), &original);
    assert_eq!(report.state_hash, original.state_hash());
}

/// REQ-CORE-005, CR 734.1, case 5(a): root R, sub A, sub-sub B durable inside A, A committed
/// plainly, R rolled back: B's effect survives.
#[test]
fn durable_survives_through_plain_commit_of_intermediate() {
    let mut txns = Transactions::new(game(2));
    let cp = txns.state().clone();
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 3);
    let a = txns.checkpoint();
    set_life(txns.state_mut(), 2, 4);
    let b = txns.checkpoint();
    draw(txns.state_mut(), 6);
    issue_objects(txns.state_mut(), 2);
    txns.commit_durable(b).unwrap();
    assert_eq!(txns.depth(), 2);
    txns.commit(a).unwrap();
    assert_eq!(txns.depth(), 1);
    txns.rollback(r).unwrap();
    assert_eq!(txns.depth(), 0);
    let mut expected = cp;
    draw(&mut expected, 6);
    issue_objects(&mut expected, 2);
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 4, round 3 entry semantics): durable B inside
/// durable A, then the root is rolled back. A's durable commit hands the root A's segments
/// (A's own work before B, B's entry, A's work after B), all durable, so each durable effect is
/// applied exactly once: the PRNG draws and object ids of A and B, and B's life change (life is
/// carried as a delta, so a second application would show). The root's own change before A is
/// reverted. The number of `carry_durable` calls is not specified here; only the state is.
#[test]
fn durable_inside_durable_then_root_rollback_applies_once() {
    let mut txns = Transactions::new(game(2));
    let cp = txns.state().clone();
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 2, 4);
    let a = txns.checkpoint();
    issue_objects(txns.state_mut(), 2);
    let b = txns.checkpoint();
    draw(txns.state_mut(), 6);
    issue_objects(txns.state_mut(), 1);
    set_life(txns.state_mut(), 1, 17);
    txns.commit_durable(b).unwrap();
    txns.commit_durable(a).unwrap();
    assert_eq!(txns.depth(), 1);
    let report = txns.rollback(r).unwrap();
    assert_eq!(txns.depth(), 0);
    let mut expected = cp.clone();
    issue_objects(&mut expected, 2);
    draw(&mut expected, 6);
    issue_objects(&mut expected, 1);
    set_life(&mut expected, 1, 17);
    assert_eq!(txns.state(), &expected);
    assert_eq!(txns.state().state_hash(), expected.state_hash());
    assert_eq!(report.state_hash, expected.state_hash());
    assert_eq!(txns.state().ids().object.last_issued(), 3);
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 17);
    assert_eq!(txns.state().player(pid(2)).unwrap().life(), 20);
    // CR 734.2: the checkpoint's priority player keeps priority.
    assert_eq!(report.priority_player, cp.priority_player());
}

/// REQ-CORE-005, CR 734.1, case 5(b): R, A, B durable, A rolled back: B survives A's rollback;
/// then R rolled back: B still survives (the record propagated through A's rollback).
#[test]
fn durable_survives_intermediate_rollback_then_root_rollback() {
    let mut txns = Transactions::new(game(2));
    let cp_r = txns.state().clone();
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 3);
    let cp_a = txns.state().clone();
    let a = txns.checkpoint();
    set_life(txns.state_mut(), 2, 4);
    let b = txns.checkpoint();
    draw(txns.state_mut(), 5);
    issue_objects(txns.state_mut(), 1);
    txns.commit_durable(b).unwrap();
    txns.rollback(a).unwrap();
    assert_eq!(txns.depth(), 1);
    let mut expected_a = cp_a;
    draw(&mut expected_a, 5);
    issue_objects(&mut expected_a, 1);
    assert_eq!(txns.state(), &expected_a);
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), 3);
    // More outer work, then roll back the root.
    set_life(txns.state_mut(), 2, 2);
    txns.rollback(r).unwrap();
    let mut expected_r = cp_r;
    draw(&mut expected_r, 5);
    issue_objects(&mut expected_r, 1);
    assert_eq!(txns.state(), &expected_r);
    assert_eq!(txns.state().state_hash(), expected_r.state_hash());
}

/// REQ-CORE-005, CR 734.1, case 5(c): two durable subs B1 then B2 under the same parent; after
/// the parent's rollback both are present: the PRNG equals the checkpoint PRNG advanced by the
/// sum of both subs' draws, and both subs' issued ids remain.
#[test]
fn two_durable_subs_both_survive() {
    let mut txns = Transactions::new(game(2));
    let cp = txns.state().clone();
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 3);
    let b1 = txns.checkpoint();
    draw(txns.state_mut(), 3);
    issue_objects(txns.state_mut(), 1);
    txns.commit_durable(b1).unwrap();
    set_life(txns.state_mut(), 2, 5);
    let b2 = txns.checkpoint();
    draw(txns.state_mut(), 4);
    issue_objects(txns.state_mut(), 2);
    txns.commit_durable(b2).unwrap();
    txns.rollback(r).unwrap();
    let mut expected = cp;
    draw(&mut expected, 7);
    issue_objects(&mut expected, 3);
    assert_eq!(txns.state().prng(), expected.prng());
    assert_eq!(txns.state().ids().object.last_issued(), 3);
    assert_eq!(txns.state(), &expected);
}

/// REQ-CORE-005, case 5(d): `commit_durable` at depth 1 behaves like `commit`: the changes stay,
/// `into_state` keeps them, and a later checkpoint/rollback cycle does not re-apply anything.
#[test]
fn durable_commit_at_depth_one_is_plain_commit() {
    let mut txns = Transactions::new(game(2));
    let r = txns.checkpoint();
    set_life(txns.state_mut(), 1, 6);
    draw(txns.state_mut(), 2);
    let live = txns.state().clone();
    txns.commit_durable(r).unwrap();
    assert_eq!(txns.depth(), 0);
    assert_eq!(txns.state(), &live);
    let c = txns.checkpoint();
    set_life(txns.state_mut(), 2, 6);
    let report = txns.rollback(c).unwrap();
    assert_eq!(txns.state(), &live);
    assert_eq!(report.state_hash, live.state_hash());
    assert_eq!(txns.into_state().unwrap(), live);
}

/// REQ-CORE-005, CR 734.1, case 5(e): a plainly committed (not durable) sub does not survive
/// the parent's rollback.
#[test]
fn plain_committed_sub_does_not_survive() {
    let original = game(2);
    let mut txns = Transactions::new(original.clone());
    let r = txns.checkpoint();
    let a = txns.checkpoint();
    draw(txns.state_mut(), 3);
    issue_objects(txns.state_mut(), 2);
    set_life(txns.state_mut(), 2, 1);
    txns.commit(a).unwrap();
    let report = txns.rollback(r).unwrap();
    assert_eq!(txns.state(), &original);
    assert_eq!(report.state_hash, original.state_hash());
}

/// REQ-CORE-005, case 5(f): a durable record is dropped when its root frame pops; a new
/// checkpoint/rollback cycle afterwards restores exactly the hash taken before it.
#[test]
fn durable_record_dropped_when_root_pops() {
    let mut txns = Transactions::new(game(2));
    let r = txns.checkpoint();
    let b = txns.checkpoint();
    draw(txns.state_mut(), 3);
    txns.commit_durable(b).unwrap();
    txns.rollback(r).unwrap();
    let h_before = txns.state().state_hash();
    let s_before = txns.state().clone();
    let r2 = txns.checkpoint();
    set_life(txns.state_mut(), 1, 1);
    let report = txns.rollback(r2).unwrap();
    assert_eq!(txns.state().state_hash(), h_before);
    assert_eq!(report.state_hash, h_before);
    assert_eq!(txns.state(), &s_before);
    // Same after a root that was committed rather than rolled back.
    let r3 = txns.checkpoint();
    let b3 = txns.checkpoint();
    draw(txns.state_mut(), 1);
    txns.commit_durable(b3).unwrap();
    txns.commit(r3).unwrap();
    let s3 = txns.state().clone();
    let r4 = txns.checkpoint();
    set_life(txns.state_mut(), 2, 1);
    txns.rollback(r4).unwrap();
    assert_eq!(txns.state(), &s3);
}

/// REQ-CORE-005, semantics i(ii): when the checkpoint state's generator is already past the
/// durable state's value it is left alone (an id generator never moves backwards).
#[test]
fn carry_durable_never_moves_id_generator_back() {
    let mut before = game(2);
    issue_objects(&mut before, 1);
    let mut after = before.clone();
    issue_objects(&mut after, 1); // object last_issued 2
    let mut restored = game(2);
    issue_objects(&mut restored, 5); // object last_issued 5
    let out = GameState::carry_durable(restored.clone(), &before, &after);
    assert_eq!(out.ids().object.last_issued(), 5);
    assert_eq!(out, restored);
}

/// REQ-CORE-005, semantics i: `carry_durable` directly: carries PRNG, advanced generators and
/// changed players; not turn number, active player, priority player or turn order.
#[test]
fn carry_durable_gamestate_direct() {
    let before = game(3);
    let mut after = before.clone();
    draw(&mut after, 2);
    after.issue_timestamp().unwrap();
    after.issue_link_id().unwrap();
    set_life(&mut after, 3, 1);
    after.set_turn_number(50);
    after.set_active_player(Some(pid(2))).unwrap();
    after.set_priority_player(Some(pid(2))).unwrap();
    after.set_turn_order(vec![pid(3), pid(2), pid(1)]).unwrap();

    let mut restored = game(3);
    set_life(&mut restored, 1, 5);
    restored.set_priority_player(Some(pid(3))).unwrap();
    let out = GameState::carry_durable(restored.clone(), &before, &after);

    let mut expected = restored.clone();
    draw(&mut expected, 2);
    expected.issue_timestamp().unwrap();
    expected.issue_link_id().unwrap();
    set_life(&mut expected, 3, 1);
    assert_eq!(out, expected);
    assert_eq!(out.priority_player(), restored.priority_player());
    assert_eq!(out.turn().turn_number(), restored.turn().turn_number());
    assert_eq!(out.turn_order(), restored.turn_order());
}

// ---------------------------------------------------------------------------------------------
// Requirement 8: the generic contract over a toy Transactional.
// ---------------------------------------------------------------------------------------------

fn toy_append(t: &mut Transactions<Toy>, bytes: &[u8]) {
    t.state_mut().log.extend_from_slice(bytes);
}

/// REQ-CORE-005, CR 734.1, case 5(c): durable records are applied in commit order (B1 then B2)
/// and receive the states at sub-begin (`before`) and at durable commit (`after`).
#[test]
fn toy_records_applied_in_commit_order_with_right_arguments() {
    let mut txns = Transactions::new(Toy::new(0));
    toy_append(&mut txns, &[9]);
    let r = txns.checkpoint();
    txns.state_mut().a = 10;
    toy_append(&mut txns, &[100]);
    txns.state_mut().prio = Some(pid(2));
    let b1 = txns.checkpoint();
    txns.state_mut().a = 11;
    toy_append(&mut txns, &[1]);
    txns.commit_durable(b1).unwrap();
    txns.state_mut().a = 20;
    let b2 = txns.checkpoint();
    toy_append(&mut txns, &[2, 3]);
    txns.state_mut().a = 21;
    txns.commit_durable(b2).unwrap();
    txns.state_mut().a = 30;
    toy_append(&mut txns, &[200]);
    let report = txns.rollback(r).unwrap();
    let expected = Toy {
        a: 0,
        log: vec![9, 1, 2, 3],
        prio: Some(pid(1)),
        calls: vec![(10, 11), (20, 21)],
    };
    assert_eq!(txns.state(), &expected);
    assert_eq!(report.state_hash, expected.state_hash());
    assert_eq!(report.priority_player, Some(pid(1)));
}

/// REQ-CORE-005, CR 734.1: records propagate through a plain commit of an intermediate frame;
/// one `carry_durable` call per durable record beneath the rolled-back frame.
#[test]
fn toy_records_propagate_through_plain_commit() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    txns.state_mut().a = 1;
    let a = txns.checkpoint();
    txns.state_mut().a = 2;
    toy_append(&mut txns, &[7]);
    let b = txns.checkpoint();
    txns.state_mut().a = 3;
    toy_append(&mut txns, &[8]);
    txns.commit_durable(b).unwrap();
    let c = txns.checkpoint();
    txns.state_mut().a = 4;
    toy_append(&mut txns, &[6]);
    txns.commit_durable(c).unwrap();
    txns.commit(a).unwrap();
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![8, 6]);
    assert_eq!(txns.state().calls, vec![(2, 3), (3, 4)]);
    assert_eq!(txns.state().a, 0);
}

/// REQ-CORE-005, CR 734.1: records propagate through the rollback of an intermediate frame and
/// are applied again over the ancestor's checkpoint; call counts match the records beneath
/// each rolled-back frame.
#[test]
fn toy_records_propagate_through_intermediate_rollback() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    txns.state_mut().a = 1;
    toy_append(&mut txns, &[50]);
    let a = txns.checkpoint();
    txns.state_mut().a = 2;
    let b = txns.checkpoint();
    txns.state_mut().a = 3;
    toy_append(&mut txns, &[5]);
    txns.commit_durable(b).unwrap();
    txns.state_mut().a = 4;
    toy_append(&mut txns, &[60]);
    txns.rollback(a).unwrap();
    // After A's rollback: A's checkpoint plus B (one call).
    assert_eq!(
        txns.state(),
        &Toy {
            a: 1,
            log: vec![50, 5],
            prio: Some(pid(1)),
            calls: vec![(2, 3)],
        }
    );
    txns.state_mut().a = 9;
    toy_append(&mut txns, &[70]);
    txns.rollback(r).unwrap();
    // After R's rollback: R's checkpoint plus B again (one call over R's checkpoint).
    assert_eq!(
        txns.state(),
        &Toy {
            a: 0,
            log: vec![5],
            prio: Some(pid(1)),
            calls: vec![(2, 3)],
        }
    );
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 4, round 3 entry semantics): root R; A appends 7;
/// B inside A appends 8; B and then A are committed durably; R is rolled back. Each durable
/// effect is applied exactly once, so the log is exactly `[7, 8]` (not `[8, 7, 8]`), and R's
/// own change to `a` is reverted. For a durable frame containing durable work the number of
/// `carry_durable` calls is not specified, so the state (without `calls`) and the log are
/// asserted instead of the call list.
#[test]
fn toy_durable_inside_durable_root_rollback_applies_once() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    txns.state_mut().a = 1;
    let a = txns.checkpoint();
    txns.state_mut().a = 2;
    toy_append(&mut txns, &[7]);
    let b = txns.checkpoint();
    txns.state_mut().a = 3;
    toy_append(&mut txns, &[8]);
    txns.commit_durable(b).unwrap();
    txns.commit_durable(a).unwrap();
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![7, 8]);
    assert_eq!(txns.state().a, 0);
    assert_eq!(txns.state().prio, Some(pid(1)));
    assert_eq!(txns.depth(), 0);
    // The durable effects stay applied once through a later checkpoint/rollback cycle.
    let c = txns.checkpoint();
    toy_append(&mut txns, &[9]);
    txns.rollback(c).unwrap();
    assert_eq!(txns.state().log, vec![7, 8]);
}

/// REQ-CORE-005, CR 734.1 (PR #175 finding 4): B durable inside A; A rolled back, then R rolled
/// back. B's effect is applied exactly once at each rollback: the log is `[8]` after each.
#[test]
fn toy_durable_inside_rolled_back_frame_applies_once_per_rollback() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    let a = txns.checkpoint();
    toy_append(&mut txns, &[7]);
    let b = txns.checkpoint();
    toy_append(&mut txns, &[8]);
    txns.commit_durable(b).unwrap();
    txns.rollback(a).unwrap();
    assert_eq!(txns.state().log, vec![8]);
    assert_eq!(txns.state().calls.len(), 1);
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![8]);
    assert_eq!(txns.state().calls.len(), 1);
}

/// REQ-CORE-005: no `carry_durable` call when no durable record lies beneath the frame, and a
/// plain commit beneath leaves nothing to carry.
#[test]
fn toy_no_records_no_calls() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    let a = txns.checkpoint();
    toy_append(&mut txns, &[1]);
    txns.commit(a).unwrap();
    let report = txns.rollback(r).unwrap();
    assert_eq!(txns.state(), &Toy::new(0));
    assert_eq!(report.state_hash, Toy::new(0).state_hash());
}

/// REQ-CORE-005: a durable sub's record lives on the parent only; rolling back a sibling frame
/// opened after it does not apply it (it is not beneath that sibling).
#[test]
fn toy_record_not_applied_by_sibling_rollback() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    let b = txns.checkpoint();
    txns.state_mut().a = 1;
    toy_append(&mut txns, &[1]);
    txns.commit_durable(b).unwrap();
    let live = txns.state().clone();
    let c = txns.checkpoint();
    toy_append(&mut txns, &[2]);
    txns.rollback(c).unwrap();
    assert_eq!(txns.state(), &live, "sibling rollback applies no record");
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().log, vec![1]);
    assert_eq!(txns.state().calls, vec![(0, 1)]);
}

/// REQ-CORE-005, CR 734.1 (round 3 entry semantics): a flat frame whose direct children are
/// committed durably, with plain commits of children without durable work in between, calls
/// `carry_durable` once per durable child, in commit order, with (child checkpoint, live at
/// that child's commit); the plain children's changes and the frame's own changes are reverted.
#[test]
fn toy_flat_durable_children_with_plain_commits_between() {
    let mut txns = Transactions::new(Toy::new(0));
    let r = txns.checkpoint();
    txns.state_mut().a = 1;
    toy_append(&mut txns, &[100]);
    let b1 = txns.checkpoint();
    txns.state_mut().a = 2;
    toy_append(&mut txns, &[1]);
    txns.commit_durable(b1).unwrap();
    let p1 = txns.checkpoint();
    txns.state_mut().a = 3;
    toy_append(&mut txns, &[101]);
    txns.commit(p1).unwrap();
    let b2 = txns.checkpoint();
    txns.state_mut().a = 4;
    toy_append(&mut txns, &[2]);
    txns.commit_durable(b2).unwrap();
    let p2 = txns.checkpoint();
    txns.state_mut().a = 5;
    toy_append(&mut txns, &[102]);
    txns.commit(p2).unwrap();
    let b3 = txns.checkpoint();
    txns.state_mut().a = 6;
    toy_append(&mut txns, &[3]);
    txns.commit_durable(b3).unwrap();
    txns.state_mut().a = 7;
    let report = txns.rollback(r).unwrap();
    let expected = Toy {
        a: 0,
        log: vec![1, 2, 3],
        prio: Some(pid(1)),
        calls: vec![(1, 2), (3, 4), (5, 6)],
    };
    assert_eq!(txns.state(), &expected);
    assert_eq!(report.state_hash, expected.state_hash());
    assert_eq!(report.priority_player, Some(pid(1)));
}

// ---------------------------------------------------------------------------------------------
// PR #175 round 3, finding A: exact life carry at the i64 boundaries through `rollback`.
// ---------------------------------------------------------------------------------------------

/// Root checkpoint with player 1's life at `start`; the outer action changes only player 2;
/// a durable child takes player 1 from `start` to `end`; the outer action changes player 2
/// again; the outer transaction is rolled back. Checks the result is the checkpoint state with
/// player 1's life `end` (exact delta `start + (end - start)`, no intermediate saturation).
fn life_boundary_through_rollback(start: i64, end: i64) {
    let mut s = game(2);
    set_life(&mut s, 1, start);
    assert_eq!(
        s.player(pid(1)).unwrap().life(),
        start,
        "set_life kept the value"
    );
    let mut txns = Transactions::new(s);
    let cp = txns.state().clone();
    let outer = txns.checkpoint();
    set_life(txns.state_mut(), 2, 11); // outer's own change, other player
    let child = txns.checkpoint();
    set_life(txns.state_mut(), 1, end);
    txns.commit_durable(child).unwrap();
    set_life(txns.state_mut(), 2, 12); // outer's own change, other player
    let report = txns.rollback(outer).unwrap();
    let mut expected = cp;
    set_life(&mut expected, 1, end);
    assert_eq!(txns.state().player(pid(1)).unwrap().life(), end);
    assert_eq!(txns.state().player(pid(2)).unwrap().life(), 20);
    assert_eq!(txns.state(), &expected);
    assert_eq!(report.state_hash, expected.state_hash());
    // CR 734.2: the checkpoint's priority player keeps priority.
    assert_eq!(report.priority_player, Some(pid(1)));
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding A): life `i64::MIN` at the outer checkpoint,
/// a durable child takes it to `i64::MAX`; after the outer rollback the life is exactly
/// `i64::MIN + (i64::MAX - i64::MIN) = i64::MAX`.
#[test]
fn durable_life_min_to_max_through_rollback() {
    life_boundary_through_rollback(i64::MIN, i64::MAX);
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding A, mirrored): `i64::MAX -> i64::MIN` gives
/// exactly `i64::MIN` after the outer rollback.
#[test]
fn durable_life_max_to_min_through_rollback() {
    life_boundary_through_rollback(i64::MAX, i64::MIN);
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding A): life -1 at the outer checkpoint, a durable
/// child takes it to `i64::MAX` (a delta of `2^63`, outside i64); the result is `i64::MAX`.
#[test]
fn durable_life_minus_one_to_max_through_rollback() {
    life_boundary_through_rollback(-1, i64::MAX);
}

// ---------------------------------------------------------------------------------------------
// PR #175 round 3, finding B: per-segment durable entries (no single covering record).
// ---------------------------------------------------------------------------------------------

/// A counter kind the CR defines (R5 allows rules vocabulary).
const PLUS: &str = "+1/+1";

fn count1(s: &GameState) -> i64 {
    s.player(pid(1)).unwrap().counter(PLUS)
}

fn set_count1(s: &mut GameState, v: i64) {
    s.player_mut(pid(1)).unwrap().set_counter(PLUS, v);
}

fn life1(s: &GameState) -> i64 {
    s.player(pid(1)).unwrap().life()
}

fn set_life1(s: &mut GameState, v: i64) {
    set_life(s, 1, v);
}

/// Gives player 2 life 3 (an own change of frame A, on a field the scenarios never carry
/// otherwise).
fn a_own_gs(s: &mut GameState) {
    set_life(s, 2, 3);
}

fn no_own<S>(_: &mut S) {}

/// How frame A is closed in the scenarios below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseA {
    Durable,
    Plain,
    RolledBack,
}

const CLOSES: [CloseA; 3] = [CloseA::Durable, CloseA::Plain, CloseA::RolledBack];

/// Accessors for the scalar a scenario drives.
struct Acc<S> {
    get: fn(&S) -> i64,
    set: fn(&mut S, i64),
    /// An own change of frame A made before its first child (checked by the caller).
    a_own: fn(&mut S),
}

fn close_a<S: Transactional>(txns: &mut Transactions<S>, a: TxnId, how: CloseA) {
    match how {
        CloseA::Durable => txns.commit_durable(a).unwrap(),
        CloseA::Plain => txns.commit(a).unwrap(),
        CloseA::RolledBack => {
            txns.rollback(a).unwrap();
        }
    }
}

/// The reviewer scenario (PR #175 round 3 finding B). The scalar is 1 at the root checkpoint;
/// the root sets 5; A begins; C1 (inside A) sets 0 and commits durably; optionally a plain
/// child P sets 3 and commits; C2 sets 5 and commits durably; A is closed as `how`; the root
/// sets 2; the root is rolled back. Returns (value right after A closes, value after the root
/// rollback, final state).
fn reviewer<S: Transactional>(
    init: S,
    acc: &Acc<S>,
    plain_between: bool,
    how: CloseA,
) -> (i64, i64, S) {
    let mut txns = Transactions::new(init);
    assert_eq!((acc.get)(txns.state()), 1);
    let root = txns.checkpoint();
    (acc.set)(txns.state_mut(), 5);
    let a = txns.checkpoint();
    (acc.a_own)(txns.state_mut());
    let c1 = txns.checkpoint();
    (acc.set)(txns.state_mut(), 0);
    txns.commit_durable(c1).unwrap();
    if plain_between {
        let p = txns.checkpoint();
        (acc.set)(txns.state_mut(), 3);
        txns.commit(p).unwrap();
    }
    let c2 = txns.checkpoint();
    (acc.set)(txns.state_mut(), 5);
    txns.commit_durable(c2).unwrap();
    close_a(&mut txns, a, how);
    let after_a = (acc.get)(txns.state());
    (acc.set)(txns.state_mut(), 2);
    txns.rollback(root).unwrap();
    let fin = (acc.get)(txns.state());
    (after_a, fin, txns.into_state().unwrap())
}

/// Expected (after A closes, after the root rollback) of [`reviewer`], for a carry that clamps
/// at 0 (`clamp`) or an exact linear one. Hand-computed from the entry specification.
fn reviewer_expected(clamp: bool, plain_between: bool, how: CloseA) -> (i64, i64) {
    match (clamp, plain_between, how) {
        (true, false, _) => (5, 5),
        (false, false, _) => (5, 1),
        (true, true, CloseA::Durable) => (5, 5),
        (false, true, CloseA::Durable) => (5, 1),
        (true, true, CloseA::Plain) => (5, 2),
        (false, true, CloseA::Plain) => (5, -2),
        (true, true, CloseA::RolledBack) => (2, 2),
        (false, true, CloseA::RolledBack) => (2, -2),
    }
}

/// The counter-example (PR #175 round 3 finding B). The scalar is 0 at the root and A
/// checkpoints; X (inside A) sets 2 (non-durable); optionally a plain grandchild P takes it to
/// 3; a durable grandchild G adds 5 (2 -> 7, or 3 -> 8); X adds 1 (non-durable); X is rolled
/// back; A is closed as `how`; the root sets 4; the root is rolled back. Returns (value after
/// X's rollback, value right after A closes, value after the root rollback).
fn counter_example<S: Transactional>(
    init: S,
    acc: &Acc<S>,
    plain_between: bool,
    how: CloseA,
) -> (i64, i64, i64) {
    let mut txns = Transactions::new(init);
    assert_eq!((acc.get)(txns.state()), 0);
    let root = txns.checkpoint();
    let a = txns.checkpoint();
    let x = txns.checkpoint();
    (acc.set)(txns.state_mut(), 2);
    let mut v = 2;
    if plain_between {
        let p = txns.checkpoint();
        (acc.set)(txns.state_mut(), 3);
        txns.commit(p).unwrap();
        v = 3;
    }
    let g = txns.checkpoint();
    (acc.set)(txns.state_mut(), v + 5);
    txns.commit_durable(g).unwrap();
    (acc.set)(txns.state_mut(), v + 6);
    txns.rollback(x).unwrap();
    let after_x = (acc.get)(txns.state());
    close_a(&mut txns, a, how);
    let after_a = (acc.get)(txns.state());
    (acc.set)(txns.state_mut(), 4);
    txns.rollback(root).unwrap();
    (after_x, after_a, (acc.get)(txns.state()))
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B, reviewer scenario): GameState counters.
/// Count 1 at the root checkpoint, the root sets 5, A begins, C1 sets 0 durably, C2 sets 5
/// durably, A commits durably, the root is rolled back. The durable segments are carried one
/// by one with the counter clamp at 0 (1 -> 0 -> 5), so the count is 5 (a single covering
/// record 5 -> 5 would leave 1). A's own change (player 2's life 3) is durable with A.
#[test]
fn reviewer_scenario_gamestate_counter() {
    let mut init = game(2);
    set_count1(&mut init, 1);
    let cp = init.clone();
    let acc = Acc {
        get: count1,
        set: set_count1,
        a_own: a_own_gs,
    };
    let (after_a, fin, state) = reviewer(init, &acc, false, CloseA::Durable);
    assert_eq!(after_a, 5);
    assert_eq!(fin, 5);
    let mut expected = cp;
    set_count1(&mut expected, 5);
    set_life(&mut expected, 2, 3);
    assert_eq!(state, expected);
    assert_eq!(state.state_hash(), expected.state_hash());
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B): the reviewer scenario on GameState
/// counters in every variant: with and without a plain commit between the durable children,
/// and with A committed durably, committed plainly (then only the durable children's effects
/// survive the root rollback, not A's own changes), or rolled back first (each durable effect
/// once per rollback).
#[test]
fn reviewer_scenario_gamestate_counter_variants() {
    for plain_between in [false, true] {
        for how in CLOSES {
            let mut init = game(2);
            set_count1(&mut init, 1);
            let cp = init.clone();
            let acc = Acc {
                get: count1,
                set: set_count1,
                a_own: a_own_gs,
            };
            let got = reviewer(init, &acc, plain_between, how);
            let want = reviewer_expected(true, plain_between, how);
            assert_eq!((got.0, got.1), want, "{plain_between} {how:?}");
            let mut expected = cp;
            set_count1(&mut expected, want.1);
            if how == CloseA::Durable {
                set_life(&mut expected, 2, 3);
            }
            assert_eq!(got.2, expected, "{plain_between} {how:?}");
        }
    }
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B): the reviewer scenario on life, whose
/// carry is an exact delta (linear): every variant gives the hand-computed value, and A's own
/// change to player 2 survives only when A is committed durably.
#[test]
fn reviewer_scenario_gamestate_life_variants() {
    for plain_between in [false, true] {
        for how in CLOSES {
            let mut init = game(2);
            set_life1(&mut init, 1);
            let cp = init.clone();
            let acc = Acc {
                get: life1,
                set: set_life1,
                a_own: a_own_gs,
            };
            let got = reviewer(init, &acc, plain_between, how);
            let want = reviewer_expected(false, plain_between, how);
            assert_eq!((got.0, got.1), want, "{plain_between} {how:?}");
            let mut expected = cp;
            set_life1(&mut expected, want.1);
            if how == CloseA::Durable {
                set_life(&mut expected, 2, 3);
            }
            assert_eq!(got.2, expected, "{plain_between} {how:?}");
        }
    }
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B, counter-example): GameState counters and
/// life. A's checkpoint value 0; inside A, X sets 2, a durable grandchild G takes 2 -> 7, X adds
/// 1, X is rolled back (value 5); A commits durably; the root is rolled back: 5, not 7. Also
/// with a plain grandchild before G, and with A committed plainly or rolled back.
#[test]
fn counter_example_gamestate_counter_and_life() {
    for plain_between in [false, true] {
        for how in CLOSES {
            let init = game(2);
            assert_eq!(count1(&init), 0);
            let acc = Acc {
                get: count1,
                set: set_count1,
                a_own: no_own,
            };
            assert_eq!(
                counter_example(init, &acc, plain_between, how),
                (5, 5, 5),
                "counter {plain_between} {how:?}"
            );
            let mut init = game(2);
            set_life1(&mut init, 0);
            let acc = Acc {
                get: life1,
                set: set_life1,
                a_own: no_own,
            };
            assert_eq!(
                counter_example(init, &acc, plain_between, how),
                (5, 5, 5),
                "life {plain_between} {how:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Round 3 task 3: a toy Transactional with a NON-LINEAR carry (a counter clamped at 0), and its
// linear (unclamped) twin.
// ---------------------------------------------------------------------------------------------

/// A one-number state; the priority player is fixed. `carry_durable` adds `after.n - before.n`
/// to `restored.n`, clamped at 0 when `CLAMP` (non-linear: carrying two segments one after the
/// other differs from carrying their union), unclamped otherwise (linear). Saturating.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NToy<const CLAMP: bool> {
    n: i64,
}

impl<const CLAMP: bool> Transactional for NToy<CLAMP> {
    fn state_hash(&self) -> Digest {
        sha256(&self.n.to_le_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(pid(1))
    }

    fn carry_durable(mut restored: Self, before: &Self, after: &Self) -> Self {
        let v = restored.n.saturating_add(after.n.saturating_sub(before.n));
        restored.n = if CLAMP { v.max(0) } else { v };
        restored
    }
}

fn ntoy_acc<const CLAMP: bool>() -> Acc<NToy<CLAMP>> {
    Acc {
        get: |s| s.n,
        set: |s, v| s.n = v,
        a_own: no_own,
    }
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B on the non-linear toy): the reviewer
/// scenario in every variant gives the hand-computed clamped values (5 with A durable; a single
/// covering record would give 1), and its linear twin the linear values.
#[test]
fn ntoy_reviewer_scenario_all_variants() {
    for plain_between in [false, true] {
        for how in CLOSES {
            let got = reviewer(NToy::<true> { n: 1 }, &ntoy_acc(), plain_between, how);
            assert_eq!(
                (got.0, got.1),
                reviewer_expected(true, plain_between, how),
                "clamped {plain_between} {how:?}"
            );
            let got = reviewer(NToy::<false> { n: 1 }, &ntoy_acc(), plain_between, how);
            assert_eq!(
                (got.0, got.1),
                reviewer_expected(false, plain_between, how),
                "linear {plain_between} {how:?}"
            );
        }
    }
}

/// REQ-CORE-005, CR 734.1 (PR #175 round 3 finding B on the non-linear toy): the counter-example
/// gives 5 (not 7) in every variant, clamped and linear.
#[test]
fn ntoy_counter_example_all_variants() {
    for plain_between in [false, true] {
        for how in CLOSES {
            assert_eq!(
                counter_example(NToy::<true> { n: 0 }, &ntoy_acc(), plain_between, how),
                (5, 5, 5),
                "clamped {plain_between} {how:?}"
            );
            assert_eq!(
                counter_example(NToy::<false> { n: 0 }, &ntoy_acc(), plain_between, how),
                (5, 5, 5),
                "linear {plain_between} {how:?}"
            );
        }
    }
}

/// Nested durable windows over increments with distinct decimal digits: the root adds 100000
/// (own, reverted); A adds 1; B (in A) adds 10; C (in B) adds 1000, commits durably; B adds 100,
/// commits durably; A adds 10000, commits durably; the root adds 200000 (own) and is rolled
/// back. Returns the final value.
fn nested_windows<const CLAMP: bool>() -> i64 {
    let mut txns = Transactions::new(NToy::<CLAMP> { n: 0 });
    let add = |t: &mut Transactions<NToy<CLAMP>>, d: i64| t.state_mut().n += d;
    let r = txns.checkpoint();
    add(&mut txns, 100_000);
    let a = txns.checkpoint();
    add(&mut txns, 1);
    let b = txns.checkpoint();
    add(&mut txns, 10);
    let c = txns.checkpoint();
    add(&mut txns, 1000);
    txns.commit_durable(c).unwrap();
    add(&mut txns, 100);
    txns.commit_durable(b).unwrap();
    add(&mut txns, 10_000);
    txns.commit_durable(a).unwrap();
    add(&mut txns, 200_000);
    txns.rollback(r).unwrap();
    txns.state().n
}

/// REQ-CORE-005, CR 734.1 (round 3 task 3, linear check): with the linear carry, nested durable
/// windows give exactly the result of replaying every durable effect once (the sum of 1, 10,
/// 1000, 100 and 10000 is 11111; any duplicated segment would change a digit); the clamped
/// carry agrees because every increment is positive.
#[test]
fn ntoy_nested_durable_windows_linear_replay_once() {
    assert_eq!(nested_windows::<false>(), 11_111);
    assert_eq!(nested_windows::<true>(), 11_111);
}

/// REQ-CORE-005, CR 734.1 (round 3 task 3, linear check): a durable window inside a frame that
/// is rolled back, inside a durable frame: the root adds 100000 (own); A adds 1; X (in A) adds
/// 10 (own, reverted by X's rollback); G (in X) adds 1000 durably; X adds 100 and is rolled
/// back; A adds 10000 and commits durably; the root is rolled back: 1 + 1000 + 10000 = 11001.
#[test]
fn ntoy_durable_inside_rolled_back_inside_durable_linear() {
    let mut txns = Transactions::new(NToy::<false> { n: 0 });
    let r = txns.checkpoint();
    txns.state_mut().n += 100_000;
    let a = txns.checkpoint();
    txns.state_mut().n += 1;
    let x = txns.checkpoint();
    txns.state_mut().n += 10;
    let g = txns.checkpoint();
    txns.state_mut().n += 1000;
    txns.commit_durable(g).unwrap();
    txns.state_mut().n += 100;
    txns.rollback(x).unwrap();
    assert_eq!(txns.state().n, 100_000 + 1 + 1000);
    txns.state_mut().n += 10_000;
    txns.commit_durable(a).unwrap();
    txns.rollback(r).unwrap();
    assert_eq!(txns.state().n, 11_001);
}
