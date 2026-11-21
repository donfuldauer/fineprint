// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for SBA results that replacements change (REQ-SBA-001, REQ-CORE-001; CR 704.3,
//! 704.5f, 704.5p, 704.5q, 704.5r, 704.8, 614.1, 616.1f, 122.2). Black-box: written from the
//! work-item briefs (PR #214 round 1, finding F1; round 2, findings G1, G2, G6, G9 and decision
//! D1) only. Every card is synthetic (R5).
//!
//! CR 704.3: "...the game checks for any of the listed conditions for state-based actions, then
//! performs all applicable state-based actions simultaneously as a single event." D1 (round 2):
//! the zone changes, every 704.5q/704.5r `RemoveCounters` and every 704.5n/704.5p `Unattach` of
//! a pass go into ONE bus batch, for every applicable permanent, including one that also moves
//! in the same pass; exactly one `Unattach` per attachment that ends. A Skip that prevents an
//! object's SBA move leaves it on the battlefield with its other SBA results applied. Replacement
//! eligibility is judged against the state before the event (CR 614.1).

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

/// Asserts that `id` (a creature of [`doomed`] shape) actually left in the pass and, per D1
/// (CR 704.3: one simultaneous event), still got its 704.5q and 704.5r `RemoveCounters` events
/// (two +1/+1, two -1/-1, two charge) and exactly one `Unattach` event in `executed`; it is not
/// listed in `unattached` (it left, D2); its new object carries no counters (CR 122.2).
fn assert_left_with_side_events(r: &SbaRun, id: ObjectId) {
    assert_to_owners_graveyard(r, id, ZoneChangeCause::StateBasedAction);
    assert_eq!(
        removals_of(r, id),
        vec![
            ("+1/+1".to_owned(), 2),
            ("-1/-1".to_owned(), 2),
            ("charge".to_owned(), 2)
        ],
        "D1: the 704.5q/704.5r removals of {id:?} are executed although it leaves"
    );
    assert_eq!(
        unattach_count(r, id),
        1,
        "D1: exactly one Unattach for {id:?}: {:?}",
        executed_events(r)
    );
    assert!(
        !r.outcome().unattached.contains(&id),
        "D2: {id:?} left, so it is not in `unattached`"
    );
    let new = r.moved_to(id).expect("moved");
    for kind in ["+1/+1", "-1/-1", "charge"] {
        assert_eq!(r.after.counter(new, kind), 0, "CR 122.2: {kind} on {new:?}");
    }
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

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 704.5q, 704.5r, 122.2 (F1 control, corrected in round
// 2 for D1): the same board without the replacement: the creature goes to its owner's graveyard
// and, in the same single event, its 704.5q/704.5r RemoveCounters events and exactly one Unattach
// event are executed; no other object gets an event; one batch; nothing is listed in
// `unattached` (the object left).
#[test]
fn req_sba_001_f1_control_unprevented_move_has_side_events() {
    let b = board(false, false);
    let r = run_sba(b.sim);
    let o = r.outcome();
    assert!(o.performed);
    assert_left_with_side_events(&r, b.x);
    assert!(o.unattached.is_empty());
    assert_eq!(r.moves().len(), 1, "only the creature moves");
    assert!(r.after.on_battlefield(b.host));
    for e in executed_events(&r) {
        match e {
            GameEvent::ZoneChange { object, .. } => assert_eq!(object, b.x),
            GameEvent::RemoveCounters { target, .. } => {
                assert_eq!(target, CounterTarget::Object(b.x))
            }
            GameEvent::Unattach { attachment } => assert_eq!(attachment, b.x),
            other => panic!("unexpected event {other:?}"),
        }
    }
    assert_eq!(r.nonempty_batches().len(), 1, "D1: one batch (CR 704.3)");
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 704.5q, 704.5r (F1, corrected in round 2 for D1): a
// mixed pass: the prevented creature stays and gets its removals and unattach; a second creature
// of the same shape whose move is not prevented leaves in the same pass and ALSO gets its
// RemoveCounters and exactly one Unattach event; all in ONE batch; a single `perform_all` does
// not loop (each finding once, the still-0-toughness creature is not checked again).
#[test]
fn req_sba_001_f1_mixed_pass() {
    let b = board(true, true);
    let other = b.other.expect("second creature");
    let r = run_sba(b.sim);
    let o = r.outcome();
    assert!(o.performed);
    assert!(r.after.on_battlefield(b.x));
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
    assert_eq!(unattach_count(&r, b.x), 1);
    assert_eq!(r.after.permanents.attached_to(b.x), None);
    assert_left_with_side_events(&r, other);
    let mut both = vec![b.x, other];
    both.sort();
    let mut un = unattach_events(&r);
    un.sort();
    assert_eq!(un, both, "one Unattach per attachment, for both objects");
    assert_eq!(
        o.unattached,
        vec![b.x],
        "D2: only the one still on the battlefield"
    );
    assert_eq!(r.moves().len(), 1);
    assert_eq!(r.nonempty_batches().len(), 1, "D1: one batch (CR 704.3)");
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

// =============================================================================================
// PR #214 round 2: G1, G2, G9, G6 (one SBA batch, D1; pre-event replacement eligibility)
// =============================================================================================

use fineprint_core::replacement::Action;
use fineprint_core::zones::{MoveRequest, ZoneKind};

/// The G1 board: creature X (base 1/0) and creature Y (base 1/3), each with one +1/+1 and one
/// -1/-1 counter (`y_counters` false: Y has none); X's toughness is 0 (CR 704.5f) and a Skip
/// (effect 1) replaces X's SBA zone change; with `requires_y`, a Skip (effect 2) replaces the
/// removal of X's +1/+1 counter only while Y has a +1/+1 counter (`requires_counter`).
fn g1_board(requires_y: bool, y_counters: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let x = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let y = s.creature(P1, "test.vanilla_1_3", 1, 3);
    s.counters(x, "+1/+1", 1);
    s.counters(x, "-1/-1", 1);
    if y_counters {
        s.counters(y, "+1/+1", 1);
        s.counters(y, "-1/-1", 1);
    }
    s.skip_sba_move(1, x);
    if requires_y {
        let mut m = remove_counters_of(x, "+1/+1");
        m.requires_counter = Some((y, "+1/+1".to_owned()));
        s.replace(2, m, Action::Skip);
    }
    (s, x, y)
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, 704.5f, 704.5q, 614.1 (G1, D1): X (0 toughness, move
// Skipped) and Y each carry a +1/+1 and a -1/-1 counter; a Skip on removing X's +1/+1 requires
// Y to have a +1/+1 counter. Eligibility is judged against the state BEFORE the pass's sibling
// removals (Y still has its counter then), so the Skip applies: X stays with exactly one +1/+1
// and no -1/-1; Y loses both; the pass is performed.
#[test]
fn req_sba_001_g1_counter_requirement_reads_pre_event_state() {
    let (s, x, y) = g1_board(true, true);
    let mut probe = s.clone();
    let found = probe.check();
    for f in [
        finding("704.5f", obj(x)),
        finding("704.5q", obj(x)),
        finding("704.5q", obj(y)),
    ] {
        assert!(found.contains(&f), "{f:?} in {found:?}");
    }
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_still_on_battlefield(&r, x);
    assert_eq!(
        r.after.counter(x, "+1/+1"),
        1,
        "the +1/+1 removal was skipped"
    );
    assert_eq!(r.after.counter(x, "-1/-1"), 0);
    assert_eq!(r.after.counter(y, "+1/+1"), 0);
    assert_eq!(r.after.counter(y, "-1/-1"), 0);
    assert_still_on_battlefield(&r, y);
}

// REQ-SBA-001, CR 704.3, 704.5q (G1 control): the same board without the counter-requirement
// Skip: X (move Skipped) ends with no +1/+1 and no -1/-1 counter; Y likewise.
#[test]
fn req_sba_001_g1_control_without_requirement_skip() {
    let (s, x, y) = g1_board(false, true);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_still_on_battlefield(&r, x);
    for id in [x, y] {
        assert_eq!(r.after.counter(id, "+1/+1"), 0);
        assert_eq!(r.after.counter(id, "-1/-1"), 0);
    }
}

// REQ-SBA-001, CR 704.5q, 614.1 (G1 mirror): Y has NO counters before the pass: the
// requirement fails, the Skip does not apply and X's +1/+1 removal happens: X ends with none of
// either kind.
#[test]
fn req_sba_001_g1_mirror_requirement_absent_before_pass() {
    let (s, x, y) = g1_board(true, false);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_still_on_battlefield(&r, x);
    assert_eq!(r.after.counter(x, "+1/+1"), 0);
    assert_eq!(r.after.counter(x, "-1/-1"), 0);
    assert_eq!(r.after.counter(y, "+1/+1"), 0);
    assert_eq!(
        counter_removals(&r)
            .into_iter()
            .filter(|(o, k, _)| *o == x && k == "+1/+1")
            .count(),
        1,
        "X's +1/+1 removal executed"
    );
}

/// The G2 board: X (base 1/0, one +1/+1 and one -1/-1 counter, SBA move Skipped by effect 1)
/// and Y (base 1/0 when `y_dies`, else 1/3); effect 2 replaces removing X's +1/+1 with
/// `Instead([Tap { Y }])`.
fn g2_board(y_dies: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let x = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let y = if y_dies {
        s.creature(P2, "test.vanilla_1_0", 1, 0)
    } else {
        s.creature(P2, "test.vanilla_1_3", 1, 3)
    };
    s.counters(x, "+1/+1", 1);
    s.counters(x, "-1/-1", 1);
    s.skip_sba_move(1, x);
    s.replace(
        2,
        remove_counters_of(x, "+1/+1"),
        Action::Instead(vec![GameEvent::Tap { object: y }]),
    );
    (s, x, y)
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, 704.5f, 704.5q, 614.1, 616.1f (G2, D1): an Instead(Tap Y)
// replaces removing X's +1/+1 while Y dies in the same pass: one bus call, the pass completes Ok
// (no UnknownObject error, no rollback); Y is in its owner's graveyard; X stays; the Instead
// replaced only the +1/+1 removal; the -1/-1 removal proceeded: X has exactly one +1/+1 and no
// -1/-1 counter.
#[test]
fn req_sba_001_g2_instead_naming_retiring_object() {
    let (s, x, y) = g2_board(true);
    let r = drive_sba(s, |i, req| panic!("unexpected request #{i}: {req:?}"));
    assert!(
        r.result.is_ok(),
        "the pass completes without a bus error (no UnknownObject): {:?}",
        r.result
    );
    let o = r.outcome();
    assert!(o.performed);
    assert_to_owners_graveyard(&r, y, ZoneChangeCause::StateBasedAction);
    assert_still_on_battlefield(&r, x);
    assert_eq!(
        r.after.counter(x, "+1/+1"),
        1,
        "the +1/+1 removal was replaced"
    );
    assert_eq!(
        r.after.counter(x, "-1/-1"),
        0,
        "the -1/-1 removal proceeded"
    );
    // Ruling (Orchestrator): the bus runs the non-zone-change events of a batch before the zone
    // changes and judges replacements against pre-event state (CR 704.3, CR 614.1; REQ-SBA-001
    // "one event"), so Y is still on the battlefield when the replacement's Tap executes and the
    // Tap of Y executes exactly once.
    assert_eq!(
        executed_events(&r)
            .iter()
            .filter(|e| matches!(e, GameEvent::Tap { object } if *object == y))
            .count(),
        1,
        "exactly one Tap of Y: {:?}",
        executed_events(&r)
    );
}

// REQ-SBA-001, CR 704.5q, 614.1 (G2 control): the same Instead with Y not dying: the pass is
// Ok, Y is tapped by the replacement (a Tap of Y is executed), X keeps one +1/+1 and no -1/-1.
#[test]
fn req_sba_001_g2_control_instead_target_survives() {
    let (s, x, y) = g2_board(false);
    let r = run_sba(s);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert!(r.outcome().performed);
    assert_still_on_battlefield(&r, x);
    assert_still_on_battlefield(&r, y);
    assert_eq!(r.after.counter(x, "+1/+1"), 1);
    assert_eq!(r.after.counter(x, "-1/-1"), 0);
    assert_eq!(
        executed_events(&r)
            .iter()
            .filter(|e| matches!(e, GameEvent::Tap { object } if *object == y))
            .count(),
        1,
        "the Instead's Tap of Y executed"
    );
}

/// The G9 board: host creature H (base 1/0) and creature X (base 1/0) attached to H (CR 704.5p);
/// both have toughness 0 (CR 704.5f); with `skip`, a Skip replaces X's SBA zone change.
fn g9_board(skip: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let x = s.creature(P2, "test.vanilla_1_0", 1, 0);
    s.attach_to(x, host);
    if skip {
        s.skip_sba_move(1, x);
    }
    (s, host, x)
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 701.3d (G9, D1, D2): X is attached to H, both die;
// X's move is Skipped: H goes to its owner's graveyard; X stays, detached; `unattached == [X]`;
// exactly ONE Unattach event names X (the host's leaving produced no second one).
#[test]
fn req_sba_001_g9_prevented_attached_creature_host_leaves() {
    let (mut s, host, x) = g9_board(true);
    let found = s.check();
    for f in [
        finding("704.5f", obj(host)),
        finding("704.5f", obj(x)),
        finding("704.5p", obj(x)),
    ] {
        assert!(found.contains(&f), "{f:?} in {found:?}");
    }
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_to_owners_graveyard(&r, host, ZoneChangeCause::StateBasedAction);
    assert_still_on_battlefield(&r, x);
    assert_eq!(r.after.permanents.attached_to(x), None, "X is detached");
    assert_eq!(o.unattached, vec![x]);
    assert_eq!(unattach_count(&r, x), 1, "exactly one Unattach for X");
    assert_eq!(unattach_events(&r), vec![x], "no other Unattach");
}

// REQ-SBA-001, CR 704.3, 704.5f, 704.5p, 701.3d (G9 control, D1, D2): no Skip: X and H both
// leave; exactly ONE Unattach event names X (not zero, not two); `unattached` does not list X
// (it left).
#[test]
fn req_sba_001_g9_control_both_leave_one_unattach() {
    let (s, host, x) = g9_board(false);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_to_owners_graveyard(&r, host, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, x, ZoneChangeCause::StateBasedAction);
    assert_eq!(unattach_count(&r, x), 1, "exactly one Unattach for X");
    assert_eq!(unattach_events(&r), vec![x]);
    assert!(!o.unattached.contains(&x), "X left: not in `unattached`");
    assert!(o.unattached.is_empty());
}

/// The G6 board: an Equipment attached to player P2 (CR 704.5n) and a creature d (base 1/0,
/// CR 704.5f); with `instead`, d's SBA move is replaced by
/// `Instead([ZoneChange d -> graveyard (StateBasedAction), ZoneChange Equipment -> exile (Other)])`.
fn g6_board(instead: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach(eq, fineprint_core::permanent::AttachTarget::Player(P2));
    let d = s.creature(P1, "test.vanilla_1_0", 1, 0);
    if instead {
        s.replace(
            1,
            sba_zone_change_of(d),
            Action::Instead(vec![
                GameEvent::ZoneChange {
                    object: d,
                    request: MoveRequest::new(ZoneKind::Graveyard),
                    cause: ZoneChangeCause::StateBasedAction,
                },
                GameEvent::ZoneChange {
                    object: eq,
                    request: MoveRequest::new(ZoneKind::Exile),
                    cause: ZoneChangeCause::Other,
                },
            ]),
        );
    }
    (s, eq, d)
}

// REQ-SBA-001, CR 704.3, 704.5n, 701.3d (G6, D2): the Equipment is unattached by 704.5n and
// moved to exile by a replacement of another object's SBA move in the same pass: it ends in
// exile; `unattached` does NOT list it (it is no longer on the battlefield); its Unattach event
// IS in `executed`; d is in its owner's graveyard.
#[test]
fn req_sba_001_g6_unattached_excludes_object_that_left() {
    let (s, eq, d) = g6_board(true);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    let new_eq = r.moved_to(eq).expect("the Equipment moved");
    assert_eq!(r.after.zone_of(new_eq), Some(ZoneKind::Exile));
    assert!(r.after.zones.object(eq).is_none());
    assert!(!o.unattached.contains(&eq), "D2: it left");
    assert!(o.unattached.is_empty());
    assert_eq!(unattach_count(&r, eq), 1, "its Unattach event is executed");
    let new_d = r.moved_to(d).expect("d moved");
    assert_eq!(r.after.zone_of(new_d), Some(ZoneKind::Graveyard));
}

// REQ-SBA-001, CR 704.5n (G6 control): without the Instead the Equipment stays on the
// battlefield, unattached, and is listed in `unattached`; one Unattach event.
#[test]
fn req_sba_001_g6_control_equipment_stays_listed() {
    let (s, eq, d) = g6_board(false);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert_still_on_battlefield(&r, eq);
    assert_eq!(r.after.permanents.attached_to(eq), None);
    assert_eq!(o.unattached, vec![eq]);
    assert_eq!(unattach_count(&r, eq), 1);
    assert_to_owners_graveyard(&r, d, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3 (round 2, contract item 9): the G1 and G9 boards (and
// their controls) run twice from equal inputs give equal outcomes, state hashes and final states.
#[test]
fn req_sba_001_round2_deterministic() {
    let boards: Vec<fn() -> Sim> = vec![
        || g1_board(true, true).0,
        || g1_board(false, true).0,
        || g2_board(true).0,
        || g9_board(true).0,
        || g9_board(false).0,
        || g6_board(true).0,
    ];
    for make in boards {
        let a = run_sba(make());
        let b = run_sba(make());
        assert_eq!(a.result, b.result);
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.after.snap(), b.after.snap());
        assert_eq!(a.after.canonical(), b.after.canonical());
    }
}

// ---------------------------------------------------------------------------------------------
// PR #214 round 3 (H4, H6): replacement-produced Unattach / Attach events
// ---------------------------------------------------------------------------------------------

use fineprint_core::permanent::AttachTarget;

/// The H4 board: Equipment E attached to a LEGAL creature `host` (no 704.5n finding for E) and a
/// creature `d` (base 1/0, CR 704.5f) whose SBA zone change is replaced by
/// `Instead([ZoneChange d -> graveyard (StateBasedAction), Unattach E, (exile E when `exile`)])`.
fn h4_board(exile: bool) -> (Sim, ObjectId, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let host = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, host);
    let d = s.creature(P1, "test.vanilla_1_0", 1, 0);
    let mut events = vec![
        GameEvent::ZoneChange {
            object: d,
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause: ZoneChangeCause::StateBasedAction,
        },
        GameEvent::Unattach { attachment: eq },
    ];
    if exile {
        events.push(GameEvent::ZoneChange {
            object: eq,
            request: MoveRequest::new(ZoneKind::Exile),
            cause: ZoneChangeCause::Other,
        });
    }
    s.replace(1, sba_zone_change_of(d), Action::Instead(events));
    (s, eq, d, host)
}

// REQ-SBA-001, REQ-CORE-001, CR 704.3, 704.5n, 701.3d, 614.1 (H4): an Unattach produced by a
// replacement (not planned by any SBA) is executed once, the Equipment stays on the battlefield
// detached and `unattached` lists it.
#[test]
fn req_sba_001_h4_replacement_unattach_listed() {
    let (mut s, eq, d, host) = h4_board(false);
    assert!(s.check().iter().all(|f| f.entry_id != "704.5n"));
    let r = run_sba(s);
    assert!(r.result.is_ok(), "{:?}", r.result);
    let o = r.outcome();
    assert!(o.performed);
    assert_still_on_battlefield(&r, eq);
    assert_still_on_battlefield(&r, host);
    assert_eq!(r.after.permanents.attached_to(eq), None);
    assert_eq!(unattach_count(&r, eq), 1, "exactly one Unattach for E");
    assert_eq!(o.unattached, vec![eq]);
    assert_to_owners_graveyard(&r, d, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 701.3d, 704.3 (H4 control): the replacement also exiles E: E is not listed in
// `unattached` (it left the battlefield); its Unattach is still in `executed`.
#[test]
fn req_sba_001_h4_control_replacement_unattach_then_exile() {
    let (s, eq, _d, _host) = h4_board(true);
    let r = run_sba(s);
    assert!(r.result.is_ok(), "{:?}", r.result);
    let o = r.outcome();
    assert!(r.after.zones.object(eq).is_none(), "E left the battlefield");
    let new_eq = r.moved_to(eq).expect("E moved");
    assert_eq!(r.after.zone_of(new_eq), Some(ZoneKind::Exile));
    assert!(!o.unattached.contains(&eq));
    assert!(o.unattached.is_empty());
    assert_eq!(unattach_count(&r, eq), 1, "its Unattach is executed");
}

// REQ-SBA-001, REQ-CORE-001 (H4): the H4 boards run twice give equal outcomes and states.
#[test]
fn req_sba_001_h4_deterministic() {
    for exile in [false, true] {
        let a = run_sba(h4_board(exile).0);
        let b = run_sba(h4_board(exile).0);
        assert_eq!(a.result, b.result);
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.outcome().unattached, b.outcome().unattached);
        assert_eq!(a.after.snap(), b.after.snap());
        assert_eq!(a.after.canonical(), b.after.canonical());
    }
}

/// The H6 board: Equipment E attached to an illegal host (704.5n, so an Unattach is planned),
/// `new_host`, and `x` (base 1/0, +1/+1 and -1/-1 counters, CR 704.5q) whose SBA move is Skipped
/// (when `via_counters`) with its +1/+1 removal replaced by `Instead([RemoveCounters +1/+1,
/// Attach E -> new_host])`; otherwise creature `d` (base 1/0) whose SBA move is replaced by
/// `Instead([ZoneChange d, Attach E -> new_host])`.
fn h6_board(via_counters: bool) -> (Sim, ObjectId, ObjectId) {
    let mut s = Sim::new();
    let old_host = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let new_host = s.creature(P2, "test.vanilla_3_3", 3, 3);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, old_host);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(old_host)));
    let attach = GameEvent::Attach {
        attachment: eq,
        target: AttachTarget::Permanent(new_host),
    };
    if via_counters {
        let x = s.creature(P1, "test.vanilla_1_0", 1, 0);
        s.counters(x, "+1/+1", 1);
        s.counters(x, "-1/-1", 1);
        s.skip_sba_move(1, x);
        s.replace(
            2,
            remove_counters_of(x, "+1/+1"),
            Action::Instead(vec![
                GameEvent::RemoveCounters {
                    target: CounterTarget::Object(x),
                    kind: "+1/+1".to_owned(),
                    amount: 1,
                },
                attach,
            ]),
        );
    } else {
        let d = s.creature(P1, "test.vanilla_1_0", 1, 0);
        s.replace(
            1,
            sba_zone_change_of(d),
            Action::Instead(vec![
                GameEvent::ZoneChange {
                    object: d,
                    request: MoveRequest::new(ZoneKind::Graveyard),
                    cause: ZoneChangeCause::StateBasedAction,
                },
                attach,
            ]),
        );
    }
    (s, eq, new_host)
}

// REQ-SBA-001, CR 704.3, 704.5n, 704.5q, 614.1 (H6): the planned Unattach does not clear a new
// link an Attach made in the same pass: E ends attached to the NEW host; it is listed in
// `unattached` (its Unattach occurred and it stays on the battlefield).
#[test]
fn req_sba_001_h6_replacement_attach_keeps_new_host_counters() {
    let (s, eq, new_host) = h6_board(true);
    let r = run_sba(s);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_still_on_battlefield(&r, eq);
    assert_eq!(
        r.after.permanents.attached_to(eq),
        Some(AttachTarget::Permanent(new_host))
    );
    assert_eq!(r.outcome().unattached, vec![eq]);
    assert_eq!(unattach_count(&r, eq), 1);
}

// REQ-SBA-001, CR 704.3, 704.5n, 614.1 (H6, zone-change variant): same as above where the Attach
// is produced by replacing another object's SBA zone change.
#[test]
fn req_sba_001_h6_replacement_attach_keeps_new_host_zone_change() {
    let (s, eq, new_host) = h6_board(false);
    let r = run_sba(s);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_still_on_battlefield(&r, eq);
    assert_eq!(
        r.after.permanents.attached_to(eq),
        Some(AttachTarget::Permanent(new_host))
    );
    assert_eq!(r.outcome().unattached, vec![eq]);
    assert_eq!(unattach_count(&r, eq), 1);
}

// REQ-SBA-001, CR 704.5n (item 4, unchanged behaviour): an illegal Equipment link with no
// replacement present: covered by `req_sba_001_704_5n_f5_illegal_equipment_target_unattached`
// in spec_sba_attach.rs (unattached, stays, listed); the exactly-one-Unattach count here.
#[test]
fn req_sba_001_h6_unchanged_illegal_link_without_replacement() {
    let mut s = Sim::new();
    let old_host = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let eq = s.permanent(P1, P1, equipment_chars("test.equipment_a"));
    s.attach_to(eq, old_host);
    s.env
        .illegal_equipment_targets
        .insert((eq, AttachTarget::Permanent(old_host)));
    let r = run_sba(s);
    assert_still_on_battlefield(&r, eq);
    assert_eq!(r.after.permanents.attached_to(eq), None);
    assert_eq!(unattach_count(&r, eq), 1);
    assert_eq!(r.outcome().unattached, vec![eq]);
}
