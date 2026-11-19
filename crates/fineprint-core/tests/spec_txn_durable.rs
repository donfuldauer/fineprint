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

use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::txn::{Transactional, Transactions};
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

/// REQ-CORE-005, CR 734.1 (PR #175 finding 4): durable B inside durable A, then the root is
/// rolled back. A's record covers B's window, so each durable effect is applied exactly once:
/// the PRNG draws and object ids of A and B, and B's life change (life is carried as a delta,
/// so a second application would show). The root's own change before A is reverted.
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

/// REQ-CORE-005, CR 734.1 (PR #175 finding 4): root R; A appends 7; B inside A appends 8; B
/// and then A are committed durably; R is rolled back. A's single record covers B, so the log
/// is exactly `[7, 8]` (not `[8, 7, 8]`) from exactly one `carry_durable` call.
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
    assert_eq!(txns.state().calls, vec![(1, 3)]);
    assert_eq!(txns.state().a, 0);
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
