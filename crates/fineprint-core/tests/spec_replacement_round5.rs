// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for review round 5 of the replacement bus (REQ-EVENT-001, REQ-CORE-001; CR 704.3,
//! 704.7, 704.8, 614.5, 614.6, 614.7, 616.1f, 616.2, 400.7, 707.5, 615.5, 615.13, 122.1c).
//!
//! * B1: every FINAL event of the submitted members and of every `Instead` lineage (a group
//!   member's, a singleton's, an `sba = false` member's, a grandchild's) executes in ONE executed
//!   batch (CR 704.3, 614.6); prevention follow-ups form later batch(es).
//! * B2: inside it the final events that are not zone changes come first, then the surviving
//!   zone changes.
//! * B3: one object moves once per call; a replacement-produced or redirected move beats a plain
//!   submitted move; never an error, never a rollback (reading Q2: the CR is silent).
//! * B4: a lineage is judged against the state before the call executes anything (reading Q3).
//! * B5: the prevention follow-ups of one call are one later batch (reading Q1).
//! * B6: separate results are scheduled APNAP then submission order, so whole-run equality is
//!   compared only between orders of ONE group's members or of one object's two events.
//!
//! Black-box: written from the work-item brief only. Every object and effect is synthetic (R5).

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{
    EventBatch, EventError, ExecutedBatch, GameEvent, Outcome, ZoneChangeCause,
};
use fineprint_core::memory::MemoryError;
use fineprint_core::permanent::PermanentError;
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield, Tier,
};
use fineprint_core::zones::{ZoneError, ZoneId, ZoneKind};
use support::*;

use ZoneChangeCause::{Destroy, Other, StateBasedAction as Sba};

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

/// "by_cause(n, c, a)": a one-use effect on every ZoneChange with cause `c` (any object).
fn by_cause(n: u64, cause: ZoneChangeCause, action: Action) -> ReplacementEffect {
    fx(
        n,
        EventMatcher {
            cause: Some(cause),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        action,
    )
    .with_shield(Shield::Uses(1))
}

/// "on_object(n, kind, o, a)": a one-use effect on events of `kind` affecting object `o`.
fn on_object(n: u64, kind: BusEventKind, o: ObjectId, action: Action) -> ReplacementEffect {
    fx(n, object_matcher(kind, o), action).with_shield(Shield::Uses(1))
}

fn object_matcher(kind: BusEventKind, o: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(o),
        ..EventMatcher::new(kind)
    }
}

fn mv(obj: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    zone_change(obj, to, cause)
}

fn gy(obj: ObjectId, cause: ZoneChangeCause) -> GameEvent {
    mv(obj, ZoneKind::Graveyard, cause)
}

/// Whether `e` is a ZoneChange of `obj` (by its id at submission).
fn is_move_of(e: &GameEvent, obj: ObjectId) -> bool {
    matches!(e, GameEvent::ZoneChange { object, .. } if *object == obj)
}

/// The executed ZoneChange events of `obj` over every executed batch.
fn moves(r: &Run, obj: ObjectId) -> Vec<GameEvent> {
    r.executed()
        .iter()
        .map(|e| e.instance.event().clone())
        .filter(|e| is_move_of(e, obj))
        .collect()
}

fn moves_of(r: &Run, obj: ObjectId) -> usize {
    moves(r, obj).len()
}

/// The new id of the single executed move of `obj`; panics unless there is exactly one.
fn moved_to(r: &Run, obj: ObjectId) -> ObjectId {
    let ids: Vec<ObjectId> = r
        .executed()
        .iter()
        .filter(|e| is_move_of(e.instance.event(), obj))
        .map(|e| match e.outcome {
            Outcome::ZoneChange(m) => m.new,
            ref other => panic!("expected a ZoneChange outcome, got {other:?}"),
        })
        .collect();
    assert_eq!(ids.len(), 1, "exactly one executed move of {obj:?}");
    ids[0]
}

/// Asserts that `obj`'s single executed move put its card in `zone` (CR 400.7: a new object of the
/// same card; the old id is in no zone).
fn assert_card_in(r: &Run, obj: ObjectId, zone: ZoneId) {
    let card = r
        .before
        .world
        .zones
        .object(obj)
        .expect("present before")
        .card;
    let new = moved_to(r, obj);
    let rec = r.world().zones.object(new).expect("the new object exists");
    assert_eq!(rec.card, card, "the same card");
    assert!(
        r.world().zones.contents(zone).contains(&new),
        "the card is in {zone:?}"
    );
    assert_eq!(r.world().zone_of(obj), None, "the old id is retired");
}

/// Asserts that `obj` did not move and is still on the battlefield.
fn assert_stays(r: &Run, obj: ObjectId) {
    assert_eq!(moves_of(r, obj), 0, "{obj:?} does not move");
    assert_eq!(r.world().zone_of(obj), Some(ZoneKind::Battlefield));
}

/// The single non-empty executed batch of `r` (B1); the log gains exactly that batch.
fn one_batch(r: &Run) -> ExecutedBatch {
    let batches = r.nonempty_batches();
    assert_eq!(
        batches.len(),
        1,
        "ONE non-empty executed batch: {batches:?}"
    );
    let logged: Vec<ExecutedBatch> = r
        .new_log()
        .into_iter()
        .filter(|b| !b.events.is_empty())
        .collect();
    assert_eq!(logged.len(), 1, "the log gains one non-empty batch");
    assert_eq!(bytes(&logged[0]), bytes(&batches[0]), "logged = reported");
    batches[0].clone()
}

/// The position of the first event of `batch` satisfying `pred`.
fn position(batch: &ExecutedBatch, pred: impl Fn(&GameEvent) -> bool) -> usize {
    batch
        .events
        .iter()
        .position(|e| pred(e.instance.event()))
        .unwrap_or_else(|| panic!("no such event in {batch:?}"))
}

fn assert_same(a: &Run, b: &Run, what: &str) {
    assert_eq!(a.result, b.result, "{what}: report");
    assert_eq!(
        bytes(&a.after.registry),
        bytes(&b.after.registry),
        "{what}: registry"
    );
    assert_eq!(a.after.snap(), b.after.snap(), "{what}: world");
    assert_eq!(a.after.canonical(), b.after.canonical(), "{what}: state");
    assert_eq!(a.hash, b.hash, "{what}: state hash");
}

/// Runs `first` and `second` (two orders of one group's members or of one object's two events,
/// every other event at the same position, B6) as `sba = true` batches without requests; asserts
/// whole-run equality and that a re-run of `first` is identical (REQ-CORE-001).
fn two_orders(base: &Bus, first: Vec<GameEvent>, second: Vec<GameEvent>) -> (Run, Run) {
    let a = run_batch(base.clone(), first.clone(), true, &[]);
    let b = run_batch(base.clone(), second, true, &[]);
    assert_same(&a, &b, "the output depends on the member order");
    let again = run_batch(base.clone(), first, true, &[]);
    assert_same(&a, &again, "a re-run differs");
    (a, b)
}

/// Two permanents X, Y owned and controlled by player 1.
fn two_permanents() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let y = w.create(ZoneKind::Battlefield, P1);
    (w, x, y)
}

// ---------------------------------------------------------------------------------------------
// K2: a child move of another object (thread r4632122468)
// ---------------------------------------------------------------------------------------------

/// The K2 effect: the Destroy-only one-use effect whose `Instead` child exiles `y`.
fn exile_other(y: ObjectId) -> ReplacementEffect {
    by_cause(
        1,
        Destroy,
        Action::Instead(vec![mv(y, ZoneKind::Exile, Other)]),
    )
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.3, 704.7, 614.6 (K2 a, thread r4632122468): X's group
// [Destroy, StateBasedAction] and Y's StateBasedAction move; the Destroy member's Instead child
// exiles Y. Ok (no UnknownObject, no rollback): X goes to the graveyard (its StateBasedAction
// member), Y to exile (the lineage redirect beats Y's plain move, B3); one move each; ONE executed
// batch (B1); one application; the effect consumed. Both orders of X's members (Y last) agree.
#[test]
fn req_event_001_r5_k2_group_child_move_of_singleton_object() {
    let (w, x, y) = two_permanents();
    let base = Bus::new(w, vec![exile_other(y)]);
    let (a, b) = two_orders(
        &base,
        vec![gy(x, Destroy), gy(x, Sba), gy(y, Sba)],
        vec![gy(x, Sba), gy(x, Destroy), gy(y, Sba)],
    );
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(moves_of(r, y), 1);
        assert_card_in(r, x, ZoneId::Graveyard(P1));
        assert_card_in(r, y, ZoneId::Exile);
        one_batch(r);
        assert_eq!(r.applications(), eids(&[1]));
        assert!(r.after.registry.is_empty(), "the effect is consumed");
    }
}

// REQ-EVENT-001, CR 704.3, 614.6 (K2 b, thread r4632122468): X's single Destroy move is replaced
// by the child exiling Y, Y's StateBasedAction move is submitted too: Ok; X stays (its only move
// was replaced); Y in exile; exactly one move (of Y); ONE executed batch; one application. The
// reversed submission is checked for zones and counts only (B6).
#[test]
fn req_event_001_r5_k2_ungrouped_child_move_of_singleton_object() {
    let (w, x, y) = two_permanents();
    let base = Bus::new(w, vec![exile_other(y)]);
    for events in [
        vec![gy(x, Destroy), gy(y, Sba)],
        vec![gy(y, Sba), gy(x, Destroy)],
    ] {
        let r = run_batch(base.clone(), events, true, &[]);
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_stays(&r, x);
        assert_eq!(moves_of(&r, y), 1);
        assert_card_in(&r, y, ZoneId::Exile);
        let batch = one_batch(&r);
        assert_eq!(batch.events.len(), 1, "exactly one executed event");
        assert_eq!(r.applications(), eids(&[1]));
    }
}

// REQ-EVENT-001, CR 614.6 (K2 c, thread r4632122468): `sba = false`, [X Destroy, Y Destroy] to the
// graveyard; X's move is replaced by an Instead child exiling Y. This is NOT a caller duplicate:
// the lineage redirect beats Y's submitted move (B3): Ok; X stays; Y in exile; one move of Y; ONE
// executed batch; one application.
#[test]
fn req_event_001_r5_k2_non_sba_child_move_of_another_members_object() {
    let (w, x, y) = two_permanents();
    let effect = on_object(
        1,
        BusEventKind::ZoneChange,
        x,
        Action::Instead(vec![mv(y, ZoneKind::Exile, Other)]),
    );
    let base = Bus::new(w, vec![effect]);
    let r = run_batch(base, vec![gy(x, Destroy), gy(y, Destroy)], false, &[]);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_stays(&r, x);
    assert_eq!(moves_of(&r, y), 1);
    assert_card_in(&r, y, ZoneId::Exile);
    one_batch(&r);
    assert_eq!(r.applications(), eids(&[1]));
}

// ---------------------------------------------------------------------------------------------
// K3 / E3: a fully replaced group or singleton stays in the batch (thread r4632122510)
// ---------------------------------------------------------------------------------------------

/// The K3 world: X with one shield counter, Y; the one-use effect on X's moves whose Instead
/// child removes X's shield counter.
fn k3_base() -> (Bus, ObjectId, ObjectId) {
    let (mut w, x, y) = two_permanents();
    w.put_counters(x, "shield", 1);
    let effect = on_object(
        1,
        BusEventKind::ZoneChange,
        x,
        Action::Instead(vec![remove_counters(x, "shield", 1)]),
    );
    (Bus::new(w, vec![effect]), x, y)
}

/// The K3 / E3 expectations: ONE executed batch holding the removal before Y's move (B2); X
/// stays with no shield counter; Y in the graveyard; one application; the effect consumed.
fn assert_k3(r: &Run, x: ObjectId, y: ObjectId) {
    assert!(r.result.is_ok(), "{:?}", r.result);
    let batch = one_batch(r);
    let removal = position(&batch, |e| *e == remove_counters(x, "shield", 1));
    let y_move = position(&batch, |e| is_move_of(e, y));
    assert!(removal < y_move, "the RemoveCounters precedes Y's move");
    assert_stays(r, x);
    assert_eq!(r.world().counter(x, "shield"), 0);
    assert_eq!(moves_of(r, y), 1);
    assert_card_in(r, y, ZoneId::Graveyard(P1));
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.after.registry.is_empty(), "the effect is consumed");
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.3, 704.7, 614.6 (K3 a, thread r4632122510): X's group
// [Destroy, StateBasedAction] is fully replaced (one application to both members) by the counter
// removal; the removal executes in the same batch as Y's StateBasedAction move, before it. Both
// orders of X's members (Y last) agree.
#[test]
fn req_event_001_r5_k3_fully_replaced_group_stays_in_the_batch() {
    let (base, x, y) = k3_base();
    let (a, b) = two_orders(
        &base,
        vec![gy(x, Destroy), gy(x, Sba), gy(y, Sba)],
        vec![gy(x, Sba), gy(x, Destroy), gy(y, Sba)],
    );
    for r in [&a, &b] {
        assert_k3(r, x, y);
    }
}

// REQ-EVENT-001, CR 704.3, 614.6 (E3, thread r4632122510): as K3 a, X's single StateBasedAction
// move is fully replaced: the removal and Y's move share ONE batch, the removal first.
#[test]
fn req_event_001_r5_e3_fully_replaced_singleton_stays_in_the_batch() {
    let (base, x, y) = k3_base();
    let r = run_batch(base, vec![gy(x, Sba), gy(y, Sba)], true, &[]);
    assert_k3(&r, x, y);
}

// ---------------------------------------------------------------------------------------------
// K5: nested Instead lineage (thread r4632122481)
// ---------------------------------------------------------------------------------------------

/// The K5 world: X (no counters); e1 replaces the Destroy member by removing X's shield counter,
/// e2 replaces that removal by moving X to `to` with cause Other.
fn k5_base(to: ZoneKind) -> (Bus, ObjectId) {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let effects = vec![
        by_cause(
            1,
            Destroy,
            Action::Instead(vec![remove_counters(x, "shield", 1)]),
        ),
        on_object(
            2,
            BusEventKind::RemoveCounters,
            x,
            Action::Instead(vec![mv(x, to, Other)]),
        ),
    ];
    (Bus::new(w, effects), x)
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.7, 614.6, 616.1f, 616.2 (K5, thread r4632122481): the
// group [Destroy, StateBasedAction] of X; the Destroy member becomes a RemoveCounters (e1) which
// becomes a move to exile (e2): the grandchild is in the one executed batch and beats the plain
// StateBasedAction move (B3): X's card in exile, one move, applications [e1, e2], both consumed;
// both member orders agree.
#[test]
fn req_event_001_r5_k5_nested_instead_lineage_reaches_final_move() {
    let (base, x) = k5_base(ZoneKind::Exile);
    let (a, b) = two_orders(
        &base,
        vec![gy(x, Destroy), gy(x, Sba)],
        vec![gy(x, Sba), gy(x, Destroy)],
    );
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Exile);
        assert_eq!(r.applications(), eids(&[1, 2]));
        assert!(r.after.registry.is_empty(), "both effects are consumed");
        one_batch(r);
    }
}

// REQ-EVENT-001, CR 704.7, 614.6, 616.2 (K5 variant): the grandchild moves X to the graveyard
// with cause Other (the group's destination): Ok, one move, the card in the graveyard.
#[test]
fn req_event_001_r5_k5_nested_instead_lineage_same_destination() {
    let (base, x) = k5_base(ZoneKind::Graveyard);
    for events in [
        vec![gy(x, Destroy), gy(x, Sba)],
        vec![gy(x, Sba), gy(x, Destroy)],
    ] {
        let r = run_batch(base.clone(), events, true, &[]);
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(&r, x), 1);
        assert_card_in(&r, x, ZoneId::Graveyard(P1));
    }
}

// ---------------------------------------------------------------------------------------------
// K4 and the inverse: prevention follow-ups of one call (thread r4632122496)
// ---------------------------------------------------------------------------------------------

/// The K4 world: Q (player 2) with two shield counters, A, B (player 2), X (player 1); effects
/// e9 = Q's shield-counter damage prevention plus `extra`.
struct K4 {
    bus: Bus,
    q: ObjectId,
    a: ObjectId,
    b: ObjectId,
    x: ObjectId,
}

fn k4_world() -> (World, [ObjectId; 4]) {
    let mut w = World::new();
    let q = w.create(ZoneKind::Battlefield, P2);
    w.put_counters(q, "shield", 2);
    let a = w.create(ZoneKind::Battlefield, P2);
    let b = w.create(ZoneKind::Battlefield, P2);
    let x = w.create(ZoneKind::Battlefield, P1);
    (w, [q, a, b, x])
}

fn k4(extra: impl Fn([ObjectId; 4]) -> Vec<ReplacementEffect>) -> K4 {
    let (w, objs @ [q, a, b, x]) = k4_world();
    let mut effects = vec![ReplacementEffect::shield_counter_damage(eid(9), q)];
    effects.extend(extra(objs));
    K4 {
        bus: Bus::new(w, effects),
        q,
        a,
        b,
        x,
    }
}

/// A shield-less prevention of damage to `o` whose follow-up deals 1 damage to `q`.
fn prevent_then_hit(n: u64, o: ObjectId, q: ObjectId) -> ReplacementEffect {
    fx(
        n,
        object_matcher(BusEventKind::Damage, o),
        Action::Prevent {
            then: vec![dmg_object(q, 1)],
        },
    )
}

fn k4_prevent_ab([q, a, b, _]: [ObjectId; 4]) -> Vec<ReplacementEffect> {
    vec![prevent_then_hit(2, a, q), prevent_then_hit(3, b, q)]
}

// REQ-EVENT-001, CR 615.5, 615.13, 122.1c, 614.6 (K4 a, thread r4632122496): plain control
// [damage A, damage B]: both prevented, their follow-ups (1 damage to Q each) form one later
// batch and Q's shield removes ONE counter (2 -> 1, reading Q1). The group case, where the damage
// to A is the Instead child of X's Destroy member, gives the same: Q ends with one counter.
#[test]
fn req_event_001_r5_k4_group_case_equals_plain_batch() {
    let control = k4(k4_prevent_ab);
    let r = run_batch(
        control.bus.clone(),
        vec![dmg_object(control.a, 1), dmg_object(control.b, 1)],
        true,
        &[],
    );
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().counter(control.q, "shield"), 1, "control: 2 -> 1");

    let group = k4(|objs| {
        let [_, a, _, _] = objs;
        let mut v = k4_prevent_ab(objs);
        v.push(by_cause(
            1,
            Destroy,
            Action::Instead(vec![dmg_object(a, 1)]),
        ));
        v
    });
    let r = run_batch(
        group.bus.clone(),
        vec![
            gy(group.x, Destroy),
            gy(group.x, Sba),
            dmg_object(group.b, 1),
        ],
        true,
        &[],
    );
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().counter(group.q, "shield"), 1, "group: 2 -> 1");
    assert_eq!(moves_of(&r, group.x), 1);
    assert_card_in(&r, group.x, ZoneId::Graveyard(P1));
}

// REQ-EVENT-001, CR 704.3, 614.6, 615.13, 122.1c (K4 inverse, thread r4632122496): the Instead
// child of X's Destroy member deals 1 damage to Q, simultaneous with a submitted 1 damage to Q:
// both are prevented by Q's shield and ONE counter is removed (2 -> 1), exactly as the control
// [damage Q, damage Q] submitted together in a fresh world; the totals prevented agree.
#[test]
fn req_event_001_r5_k4_inverse_simultaneous_damage_shares_one_removal() {
    let control = k4(|_| Vec::new());
    let c = run_batch(
        control.bus.clone(),
        vec![dmg_object(control.q, 1), dmg_object(control.q, 1)],
        true,
        &[],
    );
    assert!(c.result.is_ok(), "{:?}", c.result);
    assert_eq!(c.world().counter(control.q, "shield"), 1, "control: 2 -> 1");

    let lineage = k4(|[q, _, _, _]| {
        vec![by_cause(
            1,
            Destroy,
            Action::Instead(vec![dmg_object(q, 1)]),
        )]
    });
    let r = run_batch(
        lineage.bus.clone(),
        vec![
            gy(lineage.x, Destroy),
            gy(lineage.x, Sba),
            dmg_object(lineage.q, 1),
        ],
        true,
        &[],
    );
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().counter(lineage.q, "shield"), 1, "lineage: 2 -> 1");
    assert_eq!(
        r.world().events.markers().damage(lineage.q),
        0,
        "all prevented"
    );
    assert_eq!(r.report().prevented, c.report().prevented);
    assert_eq!(r.report().prevented, 2);
}

// ---------------------------------------------------------------------------------------------
// E1 / E4: two destinations for one object, no effects (round-4 verifier)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-CORE-001, CR 704.3, 704.7, 400.7 (E1/E4): X with several submitted
// destinations and no effects, in three shapes (two singletons; two groups; a group and a
// singleton), each in the submitted and the reverse order: Ok (no UnknownObject, no rollback);
// exactly one move of X; its card in exactly one of {graveyard, exile}; the report, registry
// bytes, state hash and zone agree between the orders. Which zone is not asserted (B3, reading
// Q2: the CR is silent).
#[test]
fn req_event_001_r5_e1_e4_two_destinations_one_object_moves_once() {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let base = Bus::new(w, Vec::new());
    let ex = |cause| mv(x, ZoneKind::Exile, cause);
    let shapes = [
        vec![gy(x, Sba), ex(Sba)],
        vec![gy(x, Sba), gy(x, Destroy), ex(Sba), ex(Other)],
        vec![gy(x, Sba), gy(x, Destroy), ex(Sba)],
    ];
    for (i, events) in shapes.into_iter().enumerate() {
        let mut reversed = events.clone();
        reversed.reverse();
        let (a, b) = two_orders(&base, events, reversed);
        let zone = |r: &Run| r.world().zone_of(moved_to(r, x));
        for r in [&a, &b] {
            assert!(r.result.is_ok(), "shape {i}: {:?}", r.result);
            assert_eq!(moves_of(r, x), 1, "shape {i}");
            assert!(
                matches!(zone(r), Some(ZoneKind::Graveyard | ZoneKind::Exile)),
                "shape {i}: {:?}",
                zone(r)
            );
            assert_eq!(
                r.world().zone_of(x),
                None,
                "shape {i}: the old id is retired"
            );
        }
        assert_eq!(
            zone(&a),
            zone(&b),
            "shape {i}: the same zone in both orders"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// E2: a child counter removal on an object that moves in the same batch
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 704.3, 704.8, 614.6, 122.1c (E2): Y with one shield counter; X's Destroy
// member is replaced by removing Y's counter while Y's StateBasedAction move is in the same call:
// Ok; ONE batch with the removal before Y's move (B2); Y's card in the graveyard; Y's last known
// information is the state before the batch: ("shield", 1) (CR 704.8).
#[test]
fn req_event_001_r5_e2_child_counter_removal_on_moving_object() {
    let (mut w, x, y) = two_permanents();
    w.put_counters(y, "shield", 1);
    let base = Bus::new(
        w,
        vec![by_cause(
            1,
            Destroy,
            Action::Instead(vec![remove_counters(y, "shield", 1)]),
        )],
    );
    let r = run_batch(
        base,
        vec![gy(x, Destroy), gy(x, Sba), gy(y, Sba)],
        true,
        &[],
    );
    assert!(r.result.is_ok(), "{:?}", r.result);
    let batch = one_batch(&r);
    let removal = position(&batch, |e| *e == remove_counters(y, "shield", 1));
    let y_move = position(&batch, |e| is_move_of(e, y));
    assert!(removal < y_move, "the RemoveCounters precedes Y's move");
    assert_eq!(moves_of(&r, y), 1);
    assert_card_in(&r, y, ZoneId::Graveyard(P1));
    let lki = r.world().lki.get(y).expect("Y's LKI record");
    assert_eq!(lki.counters().get("shield").copied(), Some(1), "CR 704.8");
}

// ---------------------------------------------------------------------------------------------
// N1: a losing move cannot supply the copy plan (CodeRabbit nitpick)
// ---------------------------------------------------------------------------------------------

const C: u64 = 1;

/// The round-4 J4 world: card H in player 1's hand, object c on the battlefield, the one-use copy
/// effect C on every ZoneChange to the battlefield.
fn j4_base() -> (Bus, ObjectId, ObjectId) {
    let mut w = World::new();
    let h = w.create(ZoneKind::Hand, P1);
    let copied = w.create(ZoneKind::Battlefield, P2);
    let effect = fx(
        C,
        EventMatcher {
            to: Some(ZoneKind::Battlefield),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        Action::Modify(Modification::EntersAsCopyOf(copied)),
    )
    .with_tier(Tier::CopyEtb)
    .with_shield(Shield::Uses(1));
    (Bus::new(w, vec![effect]), h, copied)
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.7, 707.5, 614.5 (N1): A = H to the battlefield with cause
// Other and C already applied; B = the same with cause StateBasedAction and no applied effect. C
// applies to B only, but A wins on canonical event content (Other encodes before
// StateBasedAction, checked first), so the losing B cannot supply its copy plan: the card enters
// once and NOT as a copy; both orders agree.
#[test]
fn req_event_001_r5_n1_losing_move_cannot_supply_copy_plan() {
    let (base, h, _copied) = j4_base();
    let seeded = mv(h, ZoneKind::Battlefield, Other);
    let plain = mv(h, ZoneKind::Battlefield, Sba);
    assert!(
        bytes(&seeded) < bytes(&plain),
        "precondition: Other encodes before StateBasedAction"
    );
    let call = |first_seeded: bool| {
        let a = instance_with(seeded.clone(), &[C]);
        let b = instance_with(plain.clone(), &[]);
        let members = if first_seeded { vec![a, b] } else { vec![b, a] };
        Call::Batch(EventBatch::new(members), true)
    };
    let x = run_preferring(base.clone(), call(true), &[C]);
    let y = run_preferring(base.clone(), call(false), &[C]);
    assert_same(&x, &y, "the output depends on submission order");
    for r in [&x, &y] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, h), 1, "the card enters exactly once");
        assert_card_in(r, h, ZoneId::Battlefield);
        let entered = moved_to(r, h);
        assert_eq!(
            r.world().events.markers().copy_of(entered),
            None,
            "no copy link on the new id"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// N3: the final destination classifies a redirect (CodeRabbit nitpick)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-CORE-001, CR 704.7, 614.6, 616.1f, 616.2 (N3): the group [Destroy,
// StateBasedAction] of X to the graveyard; e1 replaces the Destroy member by a move to the
// graveyard with cause Other, which e2 redirects to exile. The child's FINAL destination differs
// from its member's, so it is a redirect and wins (B3): X's card in exile, one move, applications
// [e1, e2], both consumed, ONE batch; both member orders agree.
#[test]
fn req_event_001_r5_n3_final_destination_classifies_redirect() {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let base = Bus::new(
        w,
        vec![
            by_cause(1, Destroy, Action::Instead(vec![gy(x, Other)])),
            by_cause(
                2,
                Other,
                Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
            ),
        ],
    );
    let (a, b) = two_orders(
        &base,
        vec![gy(x, Destroy), gy(x, Sba)],
        vec![gy(x, Sba), gy(x, Destroy)],
    );
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Exile);
        assert_eq!(r.applications(), eids(&[1, 2]));
        assert!(r.after.registry.is_empty(), "both effects are consumed");
        one_batch(r);
    }
}

// ---------------------------------------------------------------------------------------------
// follow-ups on an object moved earlier in the same call (added after the Fixer's follow-up)
// ---------------------------------------------------------------------------------------------

/// A world with X (player 1) and A (player 2); a shield-less prevention of damage to A whose
/// follow-up is `then(x)`.
fn follow_up_base(then: impl Fn(ObjectId) -> GameEvent) -> (Bus, ObjectId, ObjectId) {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let a = w.create(ZoneKind::Battlefield, P2);
    let effect = fx(
        1,
        object_matcher(BusEventKind::Damage, a),
        Action::Prevent {
            then: vec![then(x)],
        },
    );
    (Bus::new(w, vec![effect]), x, a)
}

// REQ-EVENT-001, CR 614.6, 615.5, 400.7: the follow-up of a prevention (damage to X, or a move of
// X to exile) comes after the batch in which X's StateBasedAction move executed; X is a new
// object, so the follow-up is an impossible instruction and is ignored: Ok, the earlier batch is
// kept (X's card in the graveyard, one move), the follow-up is not executed.
#[test]
fn req_event_001_r5_follow_up_on_moved_object_is_ignored() {
    let follow_ups: [fn(ObjectId) -> GameEvent; 2] =
        [|x| dmg_object(x, 1), |x| mv(x, ZoneKind::Exile, Other)];
    for (i, then) in follow_ups.into_iter().enumerate() {
        let (base, x, a) = follow_up_base(then);
        let r = run_batch(base, vec![dmg_object(a, 1), gy(x, Sba)], true, &[]);
        assert!(r.result.is_ok(), "case {i}: {:?}", r.result);
        assert_eq!(r.report().prevented, 1, "case {i}");
        assert_eq!(moves_of(&r, x), 1, "case {i}");
        assert_card_in(&r, x, ZoneId::Graveyard(P1));
        assert!(
            r.executed()
                .iter()
                .all(|e| !matches!(e.instance.event(), GameEvent::Damage { .. })),
            "case {i}: the follow-up damage is not executed"
        );
        assert_eq!(
            r.nonempty_batches().len(),
            1,
            "case {i}: only the first batch"
        );
    }
}

/// Whether `e` is an unknown-object error for `id` at any layer.
fn unknown_object(e: &BusError, id: ObjectId) -> bool {
    matches!(
        e,
        BusError::Event(
            EventError::UnknownObject(g)
                | EventError::Zone(ZoneError::UnknownObject(g))
                | EventError::Permanent(PermanentError::UnknownObject(g))
                | EventError::Memory(MemoryError::UnknownObject(g))
        ) if *g == id
    )
}

// REQ-EVENT-001, CR 615.5: a follow-up naming an object id that was never issued still fails with
// UnknownObject, and the whole call rolls back.
#[test]
fn req_event_001_r5_follow_up_on_never_issued_object_fails() {
    let ghost = oid(9_999);
    let (base, x, a) = follow_up_base(|_| dmg_object(ghost, 1));
    let r = run_batch(base, vec![dmg_object(a, 1), gy(x, Sba)], true, &[]);
    assert!(unknown_object(r.error(), ghost), "{:?}", r.error());
    assert_unchanged(&r);
}
