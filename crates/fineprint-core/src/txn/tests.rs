// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of checkpoints, rollback, durable sub-transactions (REQ-CORE-005, CR 734.1,
//! CR 734.2) and the sandbox snapshot store (REQ-CORE-005, REQ-API-003). Synthetic data only;
//! literal seeds only.

use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::PlayerId;

use super::*;
use crate::state::{DesignationValue, GameState};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

/// A toy state: two integers and a priority player. `carry_durable` adds the delta of `a` and
/// copies `b`, so record order is observable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Toy {
    a: i64,
    b: i64,
    prio: Option<PlayerId>,
}

impl Transactional for Toy {
    fn state_hash(&self) -> Digest {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.a.to_le_bytes());
        bytes.extend_from_slice(&self.b.to_le_bytes());
        bytes.extend_from_slice(&self.prio.map_or(0, PlayerId::get).to_le_bytes());
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.prio
    }

    fn carry_durable(mut restored: Toy, before: &Toy, after: &Toy) -> Toy {
        restored.a += after.a - before.a;
        restored.b = after.b;
        restored
    }
}

fn p(n: u64) -> Option<PlayerId> {
    Some(PlayerId::new(n))
}

fn toy() -> Transactions<Toy> {
    Transactions::new(Toy {
        a: 0,
        b: 0,
        prio: p(1),
    })
}

/// A toy state whose `carry_durable` appends `after.log` minus `before.log` and counts its own
/// calls in `carries` (reset by every rollback to the checkpoint's 0), so double application
/// of a record is observable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LogToy {
    log: Vec<i64>,
    carries: u32,
}

impl Transactional for LogToy {
    fn state_hash(&self) -> Digest {
        let mut bytes = Vec::new();
        for v in &self.log {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&self.carries.to_le_bytes());
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(mut restored: LogToy, before: &LogToy, after: &LogToy) -> LogToy {
        restored
            .log
            .extend_from_slice(&after.log[before.log.len()..]);
        restored.carries += 1;
        restored
    }
}

fn log_toy() -> Transactions<LogToy> {
    Transactions::new(LogToy {
        log: Vec::new(),
        carries: 0,
    })
}

fn game() -> GameState {
    let mut s = GameState::new(0x5eed_0026, 3, 20).unwrap();
    s.set_turn_number(4);
    s.set_active_player(p(1)).unwrap();
    s.set_priority_player(p(1)).unwrap();
    s
}

// ---------------------------------------------------------------------------------------------
// REQ-CORE-005, CR 734.1, CR 734.2: checkpoint / rollback
// ---------------------------------------------------------------------------------------------

#[test]
fn req_core_005_new_has_depth_zero_and_wraps_state() {
    let t = toy();
    assert_eq!(t.depth(), 0);
    assert_eq!(t.state().a, 0);
    assert_eq!(t.into_state().unwrap().prio, p(1));
}

#[test]
fn req_core_005_cr_734_1_rollback_restores_exact_hash() {
    let mut t = Transactions::new(game());
    let h0 = t.state().state_hash();
    let id = t.checkpoint();
    assert_eq!(t.depth(), 1);
    let s = t.state_mut();
    s.player_mut(PlayerId::new(2)).unwrap().set_life(7);
    s.prng_mut().next_u64();
    s.issue_object_id().unwrap();
    s.set_turn_number(9);
    assert_ne!(t.state().state_hash(), h0);
    let report = t.rollback(id).unwrap();
    assert_eq!(report.state_hash, h0);
    assert_eq!(t.state().state_hash(), h0);
    assert_eq!(*t.state(), game());
    assert_eq!(t.depth(), 0);
}

#[test]
fn req_core_005_cr_734_2_priority_retained_after_rollback() {
    let mut t = Transactions::new(game());
    let id = t.checkpoint();
    t.state_mut().set_priority_player(p(3)).unwrap();
    assert_eq!(Transactional::priority_player(t.state()), p(3));
    let report = t.rollback(id).unwrap();
    assert_eq!(report.priority_player, p(1));
    assert_eq!(t.state().turn().priority_player(), p(1));
}

#[test]
fn req_core_005_cr_734_1_nested_rollback_of_inner_only() {
    let mut t = toy();
    let outer = t.checkpoint();
    t.state_mut().a = 1;
    let inner = t.checkpoint();
    assert!(inner > outer);
    t.state_mut().a = 2;
    t.rollback(inner).unwrap();
    assert_eq!(t.state().a, 1);
    assert_eq!(t.depth(), 1);
    t.commit(outer).unwrap();
    assert_eq!(t.state().a, 1);
    assert_eq!(t.depth(), 0);
}

#[test]
fn req_core_005_cr_734_1_commit_then_outer_rollback_discards() {
    let mut t = toy();
    let h0 = t.state().state_hash();
    let outer = t.checkpoint();
    let inner = t.checkpoint();
    t.state_mut().a = 5;
    t.state_mut().b = 6;
    t.commit(inner).unwrap();
    assert_eq!(t.state().a, 5);
    let r = t.rollback(outer).unwrap();
    assert_eq!(r.state_hash, h0);
    assert_eq!((t.state().a, t.state().b), (0, 0));
}

// ---------------------------------------------------------------------------------------------
// REQ-CORE-005, CR 734.1: durable sub-transactions
// ---------------------------------------------------------------------------------------------

#[test]
fn req_core_005_cr_734_1_durable_survives_parent_rollback_game_state() {
    let mut t = Transactions::new(game());
    let base = game();
    let outer = t.checkpoint();
    // The action changes priority and life of player 3 (both must be reversed).
    t.state_mut().set_priority_player(p(2)).unwrap();
    t.state_mut()
        .player_mut(PlayerId::new(3))
        .unwrap()
        .set_life(1);
    let sub = t.checkpoint();
    {
        let s = t.state_mut();
        s.prng_mut().next_u64();
        s.issue_object_id().unwrap();
        s.issue_object_id().unwrap();
        s.issue_timestamp().unwrap();
        let mut pl = s.player_mut(PlayerId::new(2)).unwrap();
        pl.set_life(17);
        pl.set_counter("test.counter_a", 2);
        pl.set_designation("test.flag", DesignationValue::Flag);
    }
    let after = t.state().clone();
    t.commit_durable(sub).unwrap();
    assert_eq!(t.depth(), 1);
    let r = t.rollback(outer).unwrap();
    let s = t.state();
    assert_eq!(r.priority_player, p(1));
    assert_eq!(s.turn().priority_player(), p(1));
    assert_eq!(s.prng(), after.prng());
    assert_eq!(s.ids().object.last_issued(), 2);
    assert_eq!(s.ids().timestamp.last_issued(), 1);
    assert_eq!(s.ids().card.last_issued(), 0);
    let p2 = s.player(PlayerId::new(2)).unwrap();
    assert_eq!(p2.life(), 17);
    assert_eq!(p2.counter("test.counter_a"), 2);
    assert_eq!(p2.designation("test.flag"), Some(&DesignationValue::Flag));
    // The action's own change to player 3 is reversed.
    assert_eq!(s.player(PlayerId::new(3)).unwrap().life(), 20);
    assert_eq!(s.player(PlayerId::new(1)), base.player(PlayerId::new(1)));
    assert_eq!(r.state_hash, s.state_hash());
    assert_ne!(r.state_hash, base.state_hash());
}

#[test]
fn req_core_005_cr_734_1_durable_at_root_is_plain_commit() {
    let mut t = toy();
    let id = t.checkpoint();
    t.state_mut().a = 3;
    t.commit_durable(id).unwrap();
    assert_eq!(t.depth(), 0);
    assert_eq!(t.state().a, 3);
    // Nothing is recorded: a new root rollback restores only its own checkpoint.
    let id2 = t.checkpoint();
    t.state_mut().a = 4;
    t.rollback(id2).unwrap();
    assert_eq!(t.state().a, 3);
}

#[test]
fn req_core_005_cr_734_1_durable_survives_two_levels() {
    let mut t = toy();
    let root = t.checkpoint();
    t.state_mut().b = 100;
    let sub = t.checkpoint();
    t.state_mut().b = 200;
    let subsub = t.checkpoint();
    t.state_mut().a += 7;
    t.commit_durable(subsub).unwrap();
    t.commit(sub).unwrap();
    let r = t.rollback(root).unwrap();
    // a: carried delta 7; b: carried value at the durable commit (200).
    assert_eq!(t.state().a, 7);
    assert_eq!(t.state().b, 200);
    assert_eq!(r.priority_player, p(1));
    assert_eq!(t.depth(), 0);
}

#[test]
fn req_core_005_cr_734_1_durable_survives_inner_then_outer_rollback() {
    let mut t = toy();
    let root = t.checkpoint();
    t.state_mut().a = 10;
    let sub = t.checkpoint();
    let subsub = t.checkpoint();
    t.state_mut().a += 3;
    t.commit_durable(subsub).unwrap();
    t.state_mut().a += 50;
    t.rollback(sub).unwrap();
    assert_eq!(t.state().a, 13);
    t.rollback(root).unwrap();
    assert_eq!(t.state().a, 3);
}

#[test]
fn req_core_005_cr_734_1_durable_records_applied_in_commit_order() {
    let mut t = toy();
    let root = t.checkpoint();
    let first = t.checkpoint();
    t.state_mut().b = 1;
    t.commit_durable(first).unwrap();
    let second = t.checkpoint();
    t.state_mut().b = 2;
    t.commit_durable(second).unwrap();
    t.rollback(root).unwrap();
    // `carry_durable` copies b, so the last applied record wins: commit order means 2.
    assert_eq!(t.state().b, 2);
}

#[test]
fn req_core_005_records_dropped_at_root() {
    let mut t = toy();
    let root = t.checkpoint();
    let sub = t.checkpoint();
    t.state_mut().a = 1;
    t.commit_durable(sub).unwrap();
    t.commit(root).unwrap();
    let again = t.checkpoint();
    t.state_mut().a = 9;
    t.rollback(again).unwrap();
    assert_eq!(t.state().a, 1);
}

#[test]
fn req_core_005_cr_734_1_durable_inside_durable_applied_once() {
    let mut t = log_toy();
    let root = t.checkpoint();
    let a = t.checkpoint();
    t.state_mut().log.push(7);
    let b = t.checkpoint();
    t.state_mut().log.push(8);
    t.commit_durable(b).unwrap();
    // A's window is [] -> [7, 8]; it already contains B's change, so B's record is dropped.
    t.commit_durable(a).unwrap();
    t.rollback(root).unwrap();
    assert_eq!(t.state().log, vec![7, 8]);
    assert_eq!(t.state().carries, 1);
}

#[test]
fn req_core_005_cr_734_1_durable_inside_plain_commit_applied_once() {
    let mut t = log_toy();
    let root = t.checkpoint();
    let a = t.checkpoint();
    t.state_mut().log.push(7);
    let b = t.checkpoint();
    t.state_mut().log.push(8);
    t.commit_durable(b).unwrap();
    t.commit(a).unwrap();
    t.rollback(root).unwrap();
    assert_eq!(t.state().log, vec![8]);
    assert_eq!(t.state().carries, 1);
}

#[test]
fn req_core_005_cr_734_1_durable_inside_rolled_back_applied_once_per_rollback() {
    let mut t = log_toy();
    let root = t.checkpoint();
    t.state_mut().log.push(1);
    let a = t.checkpoint();
    t.state_mut().log.push(7);
    let b = t.checkpoint();
    t.state_mut().log.push(8);
    t.commit_durable(b).unwrap();
    t.state_mut().log.push(9);
    t.rollback(a).unwrap();
    assert_eq!(t.state().log, vec![1, 8]);
    assert_eq!(t.state().carries, 1);
    t.rollback(root).unwrap();
    assert_eq!(t.state().log, vec![8]);
    assert_eq!(t.state().carries, 1);
}

// ---------------------------------------------------------------------------------------------
// REQ-CORE-005: errors; a failed call changes nothing
// ---------------------------------------------------------------------------------------------

#[test]
fn req_core_005_errors_change_nothing() {
    let mut t = toy();
    let outer = t.checkpoint();
    let inner = t.checkpoint();
    t.state_mut().a = 4;
    let snapshot = t.state().clone();
    let expected = TxnError::NotInnermost {
        requested: outer,
        innermost: inner,
    };
    assert_eq!(t.commit(outer), Err(expected.clone()));
    assert_eq!(t.commit_durable(outer), Err(expected.clone()));
    assert_eq!(t.rollback(outer), Err(expected));
    let never = TxnId(9_999);
    assert_eq!(t.commit(never), Err(TxnError::UnknownTxn(never)));
    assert_eq!(t.commit_durable(never), Err(TxnError::UnknownTxn(never)));
    assert_eq!(t.rollback(never), Err(TxnError::UnknownTxn(never)));
    assert_eq!(*t.state(), snapshot);
    assert_eq!(t.depth(), 2);
    t.rollback(inner).unwrap();
    t.rollback(outer).unwrap();
    assert_eq!(t.state().a, 0);
}

#[test]
fn req_core_005_unknown_on_empty_stack() {
    let mut t = toy();
    let never = TxnId(1);
    assert_eq!(t.rollback(never), Err(TxnError::UnknownTxn(never)));
}

#[test]
fn req_core_005_stale_id_after_commit_and_rollback() {
    let mut t = toy();
    let outer = t.checkpoint();
    let a = t.checkpoint();
    t.commit(a).unwrap();
    assert_eq!(t.rollback(a), Err(TxnError::UnknownTxn(a)));
    let b = t.checkpoint();
    t.rollback(b).unwrap();
    assert_eq!(t.rollback(b), Err(TxnError::UnknownTxn(b)));
    let c = t.checkpoint();
    t.commit_durable(c).unwrap();
    assert_eq!(t.commit(c), Err(TxnError::UnknownTxn(c)));
    // Ids are never reused.
    let d = t.checkpoint();
    assert!(d > c && c > b && b > a && a > outer);
    assert_eq!(t.depth(), 2);
}

#[test]
fn req_core_005_into_state_with_open_transactions_fails() {
    let mut t = toy();
    t.checkpoint();
    t.checkpoint();
    assert_eq!(
        t.into_state().map(|_| ()),
        Err(TxnError::OpenTransactions { open: 2 })
    );
}

#[test]
fn req_core_005_error_messages_not_empty() {
    let errs = [
        TxnError::UnknownTxn(TxnId(3)),
        TxnError::NotInnermost {
            requested: TxnId(1),
            innermost: TxnId(2),
        },
        TxnError::OpenTransactions { open: 1 },
    ];
    for e in &errs {
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = e;
    }
    for e in [
        SnapshotError::ZeroCapacity,
        SnapshotError::NotEnoughSnapshots {
            requested: 2,
            available: 1,
        },
    ] {
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}

// ---------------------------------------------------------------------------------------------
// REQ-CORE-005, CR 734.1, CR 734.2: GameState carry rule
// ---------------------------------------------------------------------------------------------

#[test]
fn req_core_005_cr_734_2_carry_never_touches_turn_or_priority() {
    let restored = game();
    let before = game();
    let mut after = game();
    after.set_priority_player(p(3)).unwrap();
    after.set_active_player(p(2)).unwrap();
    after.set_turn_number(11);
    after
        .set_turn_order(vec![PlayerId::new(3), PlayerId::new(2), PlayerId::new(1)])
        .unwrap();
    let out = GameState::carry_durable(restored.clone(), &before, &after);
    assert_eq!(out, restored);
}

#[test]
fn req_core_005_carry_removes_counters_and_designations_absent_after() {
    let mut restored = game();
    {
        let mut pl = restored.player_mut(PlayerId::new(1)).unwrap();
        pl.set_counter("test.counter_a", 3);
        pl.set_counter("test.counter_b", 1);
        pl.set_designation("test.flag", DesignationValue::Int(2));
        pl.set_designation("test.keep", DesignationValue::Flag);
    }
    let before = restored.clone();
    let mut after = restored.clone();
    {
        let mut pl = after.player_mut(PlayerId::new(1)).unwrap();
        pl.set_counter("test.counter_a", 0);
        pl.set_counter("test.counter_b", 4);
        pl.clear_designation("test.flag");
        pl.set_designation("test.text", DesignationValue::Text("x".into()));
    }
    let out = GameState::carry_durable(restored, &before, &after);
    let pl = out.player(PlayerId::new(1)).unwrap();
    assert_eq!(pl, after.player(PlayerId::new(1)).unwrap());
    assert_eq!(pl.counter("test.counter_a"), 0);
    assert_eq!(pl.counter("test.counter_b"), 4);
    assert_eq!(pl.designation("test.flag"), None);
}

#[test]
fn req_core_005_carry_skips_unchanged_and_missing_players() {
    let mut restored = game();
    restored.player_mut(PlayerId::new(2)).unwrap().set_life(5);
    let before = game();
    let mut after = game();
    // A player added during the durable sub-transaction is not in `restored`: skipped.
    let new_id = after.add_player(30).unwrap();
    // Player 3 is set to the life it already has: unchanged before -> after, nothing carried.
    after.player_mut(PlayerId::new(3)).unwrap().set_life(20);
    let out = GameState::carry_durable(restored.clone(), &before, &after);
    assert_eq!(out.player(new_id), None);
    // Player 2 unchanged before -> after: restored's value kept.
    assert_eq!(out.player(PlayerId::new(2)).unwrap().life(), 5);
    assert_eq!(out, restored);
}

const P1: u64 = 1;

/// Runs `outer` inside a root transaction, then `sub` inside a durable sub-transaction, then
/// rolls the root back (CR 734.1) and returns the resulting state.
fn roll_back_around_durable(
    setup: impl FnOnce(&mut GameState),
    outer: impl FnOnce(&mut GameState),
    sub: impl FnOnce(&mut GameState),
) -> GameState {
    let mut base = game();
    setup(&mut base);
    let mut t = Transactions::new(base);
    let root = t.checkpoint();
    outer(t.state_mut());
    let id = t.checkpoint();
    sub(t.state_mut());
    t.commit_durable(id).unwrap();
    t.rollback(root).unwrap();
    t.into_state().unwrap()
}

#[test]
fn req_core_005_cr_734_1_carry_life_is_a_delta() {
    let out = roll_back_around_durable(
        |_| {},
        |s| s.player_mut(PlayerId::new(P1)).unwrap().set_life(8),
        |s| s.player_mut(PlayerId::new(P1)).unwrap().set_life(5),
    );
    // Checkpoint 20, outer 20 -> 8 reversed, durable 8 -> 5 carried: 20 - 3.
    assert_eq!(out.player(PlayerId::new(P1)).unwrap().life(), 17);
}

#[test]
fn req_core_005_cr_734_1_outer_change_not_carried() {
    let out = roll_back_around_durable(
        |_| {},
        |s| s.player_mut(PlayerId::new(P1)).unwrap().set_life(8),
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.counter_a", 3);
        },
    );
    let pl = out.player(PlayerId::new(P1)).unwrap();
    assert_eq!(pl.life(), 20);
    assert_eq!(pl.counter("test.counter_a"), 3);
}

#[test]
fn req_core_005_cr_734_1_carry_counter_is_a_delta() {
    let out = roll_back_around_durable(
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 2);
        },
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 5);
        },
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 7);
        },
    );
    assert_eq!(out.player(PlayerId::new(P1)).unwrap().counter("test.x"), 4);
}

#[test]
fn req_core_005_cr_734_1_carry_counter_clamps_at_zero() {
    let out = roll_back_around_durable(
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 1);
        },
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 6);
        },
        |s| {
            s.player_mut(PlayerId::new(P1))
                .unwrap()
                .set_counter("test.x", 0);
        },
    );
    let pl = out.player(PlayerId::new(P1)).unwrap();
    // 1 + (0 - 6) clamps to 0: the entry is removed.
    assert_eq!(pl.counter("test.x"), 0);
    assert!(!pl.counters().contains_key("test.x"));
}

#[test]
fn req_core_005_cr_734_1_carry_counter_removal_and_designations() {
    let out = roll_back_around_durable(
        |s| {
            let mut pl = s.player_mut(PlayerId::new(P1)).unwrap();
            pl.set_counter("test.gone", 2);
            pl.set_designation("test.changed", DesignationValue::Int(1));
            pl.set_designation("test.removed", DesignationValue::Flag);
            pl.set_designation("test.outer_only", DesignationValue::Int(10));
        },
        |s| {
            let mut pl = s.player_mut(PlayerId::new(P1)).unwrap();
            pl.set_counter("test.gone", 5);
            pl.set_designation("test.outer_only", DesignationValue::Int(11));
        },
        |s| {
            let mut pl = s.player_mut(PlayerId::new(P1)).unwrap();
            pl.set_counter("test.gone", 0);
            pl.set_designation("test.added", DesignationValue::Text("t".into()));
            pl.set_designation("test.changed", DesignationValue::Int(2));
            pl.clear_designation("test.removed");
        },
    );
    let pl = out.player(PlayerId::new(P1)).unwrap();
    // 2 + (0 - 5) clamps to 0.
    assert_eq!(pl.counter("test.gone"), 0);
    assert!(!pl.counters().contains_key("test.gone"));
    assert_eq!(
        pl.designation("test.added"),
        Some(&DesignationValue::Text("t".into()))
    );
    assert_eq!(
        pl.designation("test.changed"),
        Some(&DesignationValue::Int(2))
    );
    assert_eq!(pl.designation("test.removed"), None);
    // Changed only by the outer action: the checkpoint's value is restored, not carried.
    assert_eq!(
        pl.designation("test.outer_only"),
        Some(&DesignationValue::Int(10))
    );
}

#[test]
fn req_core_005_carry_skips_player_missing_from_restored() {
    let restored = game();
    let mut before = game();
    let id = before.add_player(30).unwrap();
    let mut after = before.clone();
    after.player_mut(id).unwrap().set_life(4);
    let out = GameState::carry_durable(restored.clone(), &before, &after);
    assert_eq!(out, restored);
}

#[test]
fn req_core_005_carry_ids_never_rewind() {
    let mut restored = game();
    for _ in 0..5 {
        restored.issue_card_id().unwrap();
    }
    let before = game();
    let mut after = game();
    after.issue_card_id().unwrap();
    after.issue_effect_id().unwrap();
    after.issue_ability_instance_id().unwrap();
    after.issue_link_id().unwrap();
    let out = GameState::carry_durable(restored, &before, &after);
    assert_eq!(out.ids().card.last_issued(), 5);
    assert_eq!(out.ids().effect.last_issued(), 1);
    assert_eq!(out.ids().ability_instance.last_issued(), 1);
    assert_eq!(out.ids().link.last_issued(), 1);
    assert_eq!(out.ids().object.last_issued(), 0);
}

#[test]
fn req_core_005_advance_stops_when_issue_fails() {
    let mut restored = game();
    let before = game();
    let mut after = game();
    after.issue_card_id().unwrap();
    after.issue_card_id().unwrap();
    // An issuer that always fails (as on IdExhausted): advance stops without panicking.
    super::game_state::advance(
        &mut restored,
        &before,
        &after,
        |s| s.ids().card.last_issued(),
        |_| false,
    );
    assert_eq!(restored.ids().card.last_issued(), 0);
    // A working issuer reaches the target.
    super::game_state::advance(
        &mut restored,
        &before,
        &after,
        |s| s.ids().card.last_issued(),
        |s| s.issue_card_id().is_ok(),
    );
    assert_eq!(restored.ids().card.last_issued(), 2);
}

#[test]
fn req_core_005_cr_734_1_rollback_with_durable_keeps_checkpoint_priority_game_state() {
    let mut t = Transactions::new(game());
    let root = t.checkpoint();
    let sub = t.checkpoint();
    t.state_mut().set_priority_player(p(2)).unwrap();
    t.state_mut().prng_mut().next_u64();
    t.commit_durable(sub).unwrap();
    let r = t.rollback(root).unwrap();
    assert_eq!(r.priority_player, p(1));
    assert_eq!(Transactional::state_hash(t.state()), r.state_hash);
}

// ---------------------------------------------------------------------------------------------
// REQ-CORE-005, REQ-API-003: snapshot store (not CR 734)
// ---------------------------------------------------------------------------------------------

#[test]
fn req_api_003_zero_capacity_rejected() {
    assert_eq!(
        SnapshotStore::<i64>::new(0).map(|_| ()),
        Err(SnapshotError::ZeroCapacity)
    );
}

#[test]
fn req_api_003_capacity_eviction() {
    let mut s = SnapshotStore::new(3).unwrap();
    assert!(s.is_empty());
    assert_eq!(s.latest(), None);
    assert_eq!(s.capacity(), 3);
    for v in 1..=5_i64 {
        s.record(&v);
    }
    assert_eq!(s.len(), 3);
    assert!(!s.is_empty());
    assert_eq!(s.latest(), Some(&5));
    assert_eq!(s.rewind(2), Ok(3));
    assert_eq!(s.len(), 1);
    assert_eq!(
        s.rewind(1),
        Err(SnapshotError::NotEnoughSnapshots {
            requested: 1,
            available: 1
        })
    );
}

#[test]
fn req_api_003_rewind_zero_and_n() {
    let mut s = SnapshotStore::new(10).unwrap();
    for v in 10..15_i64 {
        s.record(&v);
    }
    assert_eq!(s.rewind(0), Ok(14));
    assert_eq!(s.len(), 5);
    assert_eq!(s.rewind(1), Ok(13));
    assert_eq!(s.latest(), Some(&13));
    assert_eq!(s.len(), 4);
}

#[test]
fn req_api_003_rewind_error_leaves_store_unchanged() {
    let mut s = SnapshotStore::new(4).unwrap();
    assert_eq!(
        s.rewind(0),
        Err(SnapshotError::NotEnoughSnapshots {
            requested: 0,
            available: 0
        })
    );
    s.record(&game());
    s.record(&game());
    let err = s.rewind(2).unwrap_err();
    assert_eq!(
        err,
        SnapshotError::NotEnoughSnapshots {
            requested: 2,
            available: 2
        }
    );
    assert_eq!(s.len(), 2);
    assert_eq!(s.latest(), Some(&game()));
}
