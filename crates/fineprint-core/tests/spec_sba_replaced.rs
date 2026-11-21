// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for an SBA move that a replacement prevents (REQ-SBA-001, REQ-CORE-001; CR 704.3,
//! 704.5f, 704.5p, 704.5q, 704.5r, 704.8). Black-box: written from the work-item brief
//! (PR #214 round 1, finding F1) only. Every card is synthetic (R5).
//!
//! CR 704.3: "...the game checks for any of the listed conditions for state-based actions, then
//! performs all applicable state-based actions simultaneously as a single event." A Skip
//! replacement that prevents an object's SBA move leaves it on the battlefield, and the other
//! SBA results for that object (counter removals, unattach) still happen in the same pass. An
//! object that actually leaves gets no RemoveCounters or Unattach event.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{CounterTarget, GameEvent, ZoneChangeCause};
use support::*;

/// The board: a host creature; creature `x` with base toughness -1 and three +1/+1 and two
/// -1/-1 counters (toughness 0, CR 704.5f; CR 704.5q), three charge counters with a limit of one
/// (CR 704.5r), attached to the host (CR 704.5p); with `other`, a second creature of the same
/// shape attached to the host. With `skip`, a persistent Skip replaces `x`'s SBA move.
struct Board {
    sim: Sim,
    host: ObjectId,
    x: ObjectId,
    other: Option<ObjectId>,
}

fn doomed(s: &mut Sim, host: ObjectId) -> ObjectId {
    let id = s.creature(P1, "test.vanilla_2_m1", 2, -1);
    s.counters(id, "+1/+1", 3);
    s.counters(id, "-1/-1", 2);
    s.counters(id, "charge", 3);
    s.limit(id, "charge", 1);
    s.attach_to(id, host);
    id
}

fn board(skip: bool, other: bool) -> Board {
    let mut s = Sim::new();
    let host = s.creature(P2, "test.vanilla_3_3", 3, 3);
    let x = doomed(&mut s, host);
    let other = other.then(|| doomed(&mut s, host));
    if skip {
        s.skip_sba_move(1, x);
    }
    Board {
        sim: s,
        host,
        x,
        other,
    }
}

/// The RemoveCounters events of the pass for `id`, as `(kind, amount)`, sorted.
fn removals_of(r: &SbaRun, id: ObjectId) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = counter_removals(r)
        .into_iter()
        .filter(|(o, _, _)| *o == id)
        .map(|(_, k, n)| (k, n))
        .collect();
    v.sort();
    v
}

/// Asserts that `id` actually left: one SBA move to its owner's graveyard and no RemoveCounters
/// or Unattach event naming it.
fn assert_left_without_side_events(r: &SbaRun, id: ObjectId) {
    assert_to_owners_graveyard(r, id, ZoneChangeCause::StateBasedAction);
    assert!(
        removals_of(r, id).is_empty(),
        "no RemoveCounters for {id:?}"
    );
    assert!(
        !unattach_events(r).contains(&id),
        "no Unattach for {id:?}: {:?}",
        executed_events(r)
    );
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 704.5q, 704.5r (F1): the 0-toughness creature's SBA
// move is prevented by a Skip: it stays on the battlefield and still gets, in the same pass, its
// 704.5q removal (two of each, leaving one +1/+1 and no -1/-1), its 704.5r removal (two charge
// counters) and its 704.5p unattach (`Unattach` event, listed in `unattached`); the pass is
// performed.
#[test]
fn req_sba_001_f1_prevented_move_keeps_other_results() {
    let b = board(true, false);
    let mut sim = b.sim.clone();
    let found = sim.check();
    for e in ["704.5f", "704.5p", "704.5q", "704.5r"] {
        assert!(found.contains(&finding(e, obj(b.x))), "{e}: {found:?}");
    }
    let r = run_sba(b.sim);
    let o = r.outcome();
    assert!(o.performed);
    assert_eq!(o.findings, found);
    assert!(r.after.on_battlefield(b.x), "the Skip kept it");
    assert!(r.moved_to(b.x).is_none());
    assert_eq!(r.after.counter(b.x, "+1/+1"), 1);
    assert_eq!(r.after.counter(b.x, "-1/-1"), 0);
    assert_eq!(r.after.counter(b.x, "charge"), 1);
    assert_eq!(
        removals_of(&r, b.x),
        vec![
            ("+1/+1".to_owned(), 2),
            ("-1/-1".to_owned(), 2),
            ("charge".to_owned(), 2)
        ]
    );
    assert_eq!(unattach_events(&r), vec![b.x]);
    assert_eq!(o.unattached, vec![b.x]);
    assert_eq!(r.after.permanents.attached_to(b.x), None);
    assert!(r.after.on_battlefield(b.host));
    assert!(r.moves().is_empty(), "nothing changed zones");
    // The RemoveCounters events name the object on the battlefield.
    for e in executed_events(&r) {
        if let GameEvent::RemoveCounters { target, .. } = e {
            assert_eq!(target, CounterTarget::Object(b.x));
        }
    }
}

// REQ-SBA-001, CR 704.3, 704.5f (F1 control): the same board without the replacement: the
// creature goes to its owner's graveyard and no RemoveCounters or Unattach event names it.
#[test]
fn req_sba_001_f1_control_unprevented_move_has_no_side_events() {
    let b = board(false, false);
    let r = run_sba(b.sim);
    assert!(r.outcome().performed);
    assert_left_without_side_events(&r, b.x);
    assert!(
        executed_events(&r)
            .iter()
            .all(|e| matches!(e, GameEvent::ZoneChange { .. })),
        "only the move: {:?}",
        executed_events(&r)
    );
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 704.5q, 704.5r (F1): a mixed pass: the prevented
// creature stays and gets its removals and unattach; a second creature of the same shape whose
// move is not prevented leaves in the same pass, with no RemoveCounters or Unattach event; one
// batch; a single `perform_all` does not loop (each finding once, the still-0-toughness
// creature is not checked again).
#[test]
fn req_sba_001_f1_mixed_pass() {
    let b = board(true, true);
    let other = b.other.expect("second creature");
    let r = run_sba(b.sim);
    let o = r.outcome();
    assert!(o.performed);
    assert!(r.after.on_battlefield(b.x));
    assert_eq!(r.after.counter(b.x, "+1/+1"), 1);
    assert_eq!(r.after.counter(b.x, "-1/-1"), 0);
    assert_eq!(r.after.counter(b.x, "charge"), 1);
    assert_eq!(unattach_events(&r), vec![b.x]);
    assert!(o.unattached.contains(&b.x));
    assert_left_without_side_events(&r, other);
    assert_eq!(r.moves().len(), 1);
    // One `perform_all` call (run_sba) performs the whole pass. The other object's move and
    // the prevented object's RemoveCounters and Unattach are all executed in it; the
    // follow-up events may be in a later batch of the same pass than the move (Orchestrator
    // ruling, DECISIONS.md "F1 follow-up batch"), never an earlier one.
    let batch_of = |pred: &dyn Fn(&GameEvent) -> bool| -> Vec<usize> {
        o.executed
            .iter()
            .enumerate()
            .filter(|(_, bt)| bt.events.iter().any(|e| pred(e.instance.event())))
            .map(|(i, _)| i)
            .collect()
    };
    let mv = batch_of(&|e| matches!(e, GameEvent::ZoneChange { object, .. } if *object == other));
    assert_eq!(mv.len(), 1, "the other object's ZoneChange executed once");
    let rc = batch_of(
        &|e| matches!(e, GameEvent::RemoveCounters { target, .. } if *target == CounterTarget::Object(b.x)),
    );
    let un = batch_of(&|e| matches!(e, GameEvent::Unattach { attachment } if *attachment == b.x));
    assert!(
        !rc.is_empty(),
        "RemoveCounters for the prevented object executed"
    );
    assert_eq!(un.len(), 1, "one Unattach for the prevented object");
    assert!(
        rc.iter().chain(un.iter()).all(|i| *i >= mv[0]),
        "follow-up events are not before the move's batch"
    );
    let fx = o
        .findings
        .iter()
        .filter(|f| f == &&finding("704.5f", obj(b.x)))
        .count();
    assert_eq!(fx, 1, "no loop: 704.5f reported once");
    assert_findings_ordered(&o.findings);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3 (F1): the same input gives the same outcome, state hash
// and final state twice (prevented and mixed boards).
#[test]
fn req_sba_001_f1_deterministic() {
    for other in [false, true] {
        let a = run_sba(board(true, other).sim);
        let b = run_sba(board(true, other).sim);
        assert_eq!(a.result, b.result);
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.after.snap(), b.after.snap());
    }
}
