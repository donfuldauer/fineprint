// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for review round 4 of the replacement bus (REQ-EVENT-001, REQ-CORE-001; CR 704.3,
//! 704.7, 704.8, 701.8b, 122.1c, 614.6, 614.7, 616.1f, 400.7, 707.5). With `sba = true` the
//! ZoneChange moves of ONE object to the same destination (the group's original destination, D0)
//! form a group that is resolved first and then executed once:
//!
//! * J1: an `Instead` child of the group's application that moves the group's object competes
//!   with the group's surviving moves; its FINAL destination (after its own processing) is used;
//!   a move whose final destination differs from D0 wins; never an error, never a rollback; the
//!   object moves exactly once.
//! * J2: the departing object's last known information is the state before any of the group's
//!   events (CR 704.8), counters included; the shield-counter removal and the move share one
//!   executed batch, the removal first.
//! * J3: a modification that leaves the destination alone never beats a redirect.
//! * J4: of two otherwise identical members, the one carrying an entry-copy plan wins.
//!
//! Every case runs both submission orders and compares the output, registry bytes, world
//! snapshot and state hash; re-running gives the same result (determinism).
//! Black-box: written from the work-item brief only. Every object and effect is synthetic (R5).

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::{EffectId, ObjectId};
use fineprint_core::events::{EventBatch, ExecutedBatch, GameEvent, Outcome, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield, Tier,
};
use fineprint_core::zones::{ZoneId, ZoneKind};
use support::*;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

/// A world with one battlefield permanent X owned and controlled by player 1.
fn one_permanent() -> (World, ObjectId) {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    (w, x)
}

/// A move of `obj` to the graveyard with `cause`.
fn death(obj: ObjectId, cause: ZoneChangeCause) -> GameEvent {
    zone_change(obj, ZoneKind::Graveyard, cause)
}

/// The mixed-cause group [Destroy, StateBasedAction] of `obj` to the graveyard.
fn group(obj: ObjectId) -> Vec<GameEvent> {
    vec![
        death(obj, ZoneChangeCause::Destroy),
        death(obj, ZoneChangeCause::StateBasedAction),
    ]
}

/// A ZoneChange matcher for the given cause only (any object, any zones).
fn cause_is(cause: ZoneChangeCause) -> EventMatcher {
    EventMatcher {
        cause: Some(cause),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// A one-use effect `n` on moves with `cause`, doing `action`.
fn one_use(n: u64, cause: ZoneChangeCause, action: Action) -> ReplacementEffect {
    fx(n, cause_is(cause), action).with_shield(Shield::Uses(1))
}

fn set_dest(to: ZoneKind) -> Action {
    Action::Modify(Modification::SetDestination(to))
}

/// The Destroy-only one-use effect `n` whose action is `Instead([move obj to `to`])`.
fn instead_move(n: u64, obj: ObjectId, to: ZoneKind) -> ReplacementEffect {
    one_use(
        n,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zone_change(obj, to, ZoneChangeCause::Other)]),
    )
}

/// A one-use effect `n` on every ZoneChange whose destination is `to`, doing `action`.
fn on_moves_to(n: u64, to: ZoneKind, action: Action) -> ReplacementEffect {
    fx(
        n,
        EventMatcher {
            to: Some(to),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        action,
    )
    .with_shield(Shield::Uses(1))
}

/// Whether `e` is a ZoneChange of `obj` (by its id at submission).
fn is_move_of(e: &GameEvent, obj: ObjectId) -> bool {
    matches!(e, GameEvent::ZoneChange { object, .. } if *object == obj)
}

/// The number of executed ZoneChange events of `obj` over every executed batch.
fn moves_of(r: &Run, obj: ObjectId) -> usize {
    r.executed()
        .iter()
        .filter(|e| is_move_of(e.instance.event(), obj))
        .count()
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

/// Asserts that `obj`'s single executed move put its card in `zone`: the new object exists there,
/// carries the same card, and `obj` itself is gone (CR 400.7).
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

/// Runs `events` and their reverse as one SBA batch on `base`, answering each request by effect-id
/// preference `prefer`; asserts both runs give the same output, registry bytes, world snapshot,
/// canonical state and state hash, and that re-running the first order is identical.
fn both_orders(base: &Bus, events: Vec<GameEvent>, prefer: &[u64]) -> (Run, Run) {
    let mut reversed = events.clone();
    reversed.reverse();
    let a = run_preferring(base.clone(), Call::batch(events.clone(), true), prefer);
    let b = run_preferring(base.clone(), Call::batch(reversed, true), prefer);
    assert_same(&a, &b, "the output depends on submission order");
    let again = run_preferring(base.clone(), Call::batch(events, true), prefer);
    assert_same(&a, &again, "a re-run differs");
    (a, b)
}

fn assert_same(a: &Run, b: &Run, what: &str) {
    assert_eq!(a.result, b.result, "{what}");
    assert_eq!(bytes(&a.after.registry), bytes(&b.after.registry), "{what}");
    assert_eq!(a.after.snap(), b.after.snap(), "{what}");
    assert_eq!(a.after.canonical(), b.after.canonical(), "{what}");
    assert_eq!(a.hash, b.hash, "{what}");
    assert_eq!(a.requests.len(), b.requests.len(), "{what}");
}

/// The executed batches (non-empty) holding a ZoneChange of `obj`.
fn batches_moving(r: &Run, obj: ObjectId) -> Vec<ExecutedBatch> {
    r.nonempty_batches()
        .into_iter()
        .filter(|b| b.events.iter().any(|e| is_move_of(e.instance.event(), obj)))
        .collect()
}

/// Asserts the shield-counter shape of a group (J1 d, J2): the RemoveCounters of `obj` and the
/// move of `obj` are in ONE executed batch, the removal first, removing `removed` counters.
fn assert_removal_then_move_in_one_batch(r: &Run, obj: ObjectId, removed: i64) {
    let holding = batches_moving(r, obj);
    assert_eq!(holding.len(), 1, "one executed batch moves the object");
    let events: Vec<&GameEvent> = holding[0]
        .events
        .iter()
        .map(|e| e.instance.event())
        .collect();
    let removal = events
        .iter()
        .position(|e| **e == remove_counters(obj, "shield", removed))
        .unwrap_or_else(|| panic!("the RemoveCounters is in the moving batch: {events:?}"));
    let mv = events
        .iter()
        .position(|e| is_move_of(e, obj))
        .expect("the move");
    assert!(removal < mv, "the RemoveCounters comes before the move");
    let removals = r
        .executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::RemoveCounters { .. }))
        .count();
    assert_eq!(removals, 1, "exactly one RemoveCounters executed");
}

/// The LKI shield-counter count recorded for `obj` (CR 704.8).
fn lki_shields(r: &Run, obj: ObjectId) -> Option<i64> {
    let rec = r
        .world()
        .lki
        .get(obj)
        .expect("an LKI record for the departed object");
    rec.counters().get("shield").copied()
}

// ---------------------------------------------------------------------------------------------
// J1: an Instead child that moves the group's object (thread r4631818351)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 704.3, 704.7, 614.6 (J1 a): [Destroy, StateBasedAction] to the graveyard and
// a Destroy-only one-use effect whose Instead child moves X to exile: no error, no rollback; the
// child's redirect beats the StateBasedAction member's graveyard move: the card ends in exile,
// X moves exactly once, one application, the effect consumed. Both submission orders.
#[test]
fn req_event_001_r4_j1_instead_child_redirect_wins() {
    let (w, x) = one_permanent();
    let base = Bus::new(w, vec![instead_move(1, x, ZoneKind::Exile)]);
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Exile);
        assert_eq!(r.applications(), eids(&[1]));
        assert!(r.after.registry.is_empty(), "the effect is consumed");
    }
}

// REQ-EVENT-001, CR 704.3, 704.7, 614.6 (J1 b): as (a) but the child moves X to the graveyard
// (the group's original destination): no error, the card ends in the graveyard, one move.
// Review round 5 (N2): the one executed move of X, across every batch, is the Instead child
// (cause Other), which beats the StateBasedAction member (a replacement-produced move wins over a
// plain submitted move; Other also encodes before StateBasedAction, checked first).
#[test]
fn req_event_001_r4_j1_instead_child_same_destination() {
    let (w, x) = one_permanent();
    let child = zone_change(x, ZoneKind::Graveyard, ZoneChangeCause::Other);
    assert!(
        bytes(&child) < bytes(&death(x, ZoneChangeCause::StateBasedAction)),
        "precondition: Other encodes before StateBasedAction"
    );
    let base = Bus::new(w, vec![instead_move(1, x, ZoneKind::Graveyard)]);
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        let executed_moves: Vec<GameEvent> = r
            .executed()
            .iter()
            .map(|e| e.instance.event().clone())
            .filter(|e| is_move_of(e, x))
            .collect();
        assert_eq!(
            executed_moves,
            vec![child.clone()],
            "the Instead child wins"
        );
        assert_card_in(r, x, ZoneId::Graveyard(P1));
        assert_eq!(r.applications(), eids(&[1]));
        assert!(r.after.registry.is_empty());
    }
}

// REQ-EVENT-001, CR 704.7, 614.6, 616.1f (J1 c): as (a) plus e2 (one-use, any ZoneChange to
// exile -> hand): the child goes through the bus itself, so its FINAL destination (hand) is
// used: the card ends in its owner's hand, applications [e1, e2], one move.
#[test]
fn req_event_001_r4_j1_instead_child_final_destination_used() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![
            instead_move(1, x, ZoneKind::Exile),
            on_moves_to(2, ZoneKind::Exile, set_dest(ZoneKind::Hand)),
        ],
    );
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Hand(P1));
        assert_eq!(r.applications(), eids(&[1, 2]));
        assert!(r.after.registry.is_empty());
    }
}

// REQ-EVENT-001, CR 704.3, 704.7, 701.8b, 122.1c (J1 d): the counter-removal path is kept: the
// destroy shield on X (one counter), [Destroy, StateBasedAction]: no error, the counter is
// removed, the card ends in the graveyard, one application, one move; the RemoveCounters and the
// move are in ONE executed batch (the removal first) and no other executed batch exists.
#[test]
fn req_event_001_r4_j1_shield_counter_removal_and_move_one_batch() {
    let (mut w, x) = one_permanent();
    w.put_counters(x, "shield", 1);
    let base = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), x)],
    );
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Graveyard(P1));
        let new = moved_to(r, x);
        assert_eq!(
            r.world().counter(new, "shield"),
            0,
            "the counter is removed"
        );
        assert_eq!(r.world().counter(x, "shield"), 0, "the counter is removed");
        assert_removal_then_move_in_one_batch(r, x, 1);
        assert_eq!(r.nonempty_batches().len(), 1, "no other executed batch");
        let logged = r.new_log().iter().filter(|b| !b.events.is_empty()).count();
        assert_eq!(logged, 1, "the log gains one non-empty batch");
    }
}

// REQ-EVENT-001, CR 704.3, 704.7, 614.6, 614.7 (J1 e): the Instead child (X to exile) is itself
// dropped by a one-use Skip on moves to exile; nothing else moves X but the surviving
// StateBasedAction member, which still moves it to the graveyard: no error, one move.
#[test]
fn req_event_001_r4_j1_skipped_child_sba_member_still_moves() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![
            instead_move(1, x, ZoneKind::Exile),
            on_moves_to(2, ZoneKind::Exile, Action::Skip),
        ],
    );
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Graveyard(P1));
    }
}

// ---------------------------------------------------------------------------------------------
// J2: last known information of the shielded object (thread r4631818358)
// ---------------------------------------------------------------------------------------------

/// The J2 world: X with `shields` shield counters and the destroy shield (e1), plus, if
/// `with_y`, a second permanent Y. Returns the bus, X and Y.
fn j2_base(shields: i64, with_y: bool) -> (Bus, ObjectId, Option<ObjectId>) {
    let (mut w, x) = one_permanent();
    w.put_counters(x, "shield", shields);
    let y = with_y.then(|| w.create(ZoneKind::Battlefield, P1));
    let bus = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), x)],
    );
    (bus, x, y)
}

/// The J2 assertions common to every variant, for a run where X had `shields` counters.
fn assert_j2(r: &Run, x: ObjectId, shields: i64) {
    assert!(r.result.is_ok(), "{:?}", r.result);
    // CR 704.8: the LKI is the state before any of the group's events, the counters included.
    assert_eq!(lki_shields(r, x), Some(shields), "LKI shield counters");
    assert_eq!(moves_of(r, x), 1);
    assert_card_in(r, x, ZoneId::Graveyard(P1));
    assert_eq!(r.applications(), eids(&[1]));
    // CR 122.1c: the shield removes one counter, in the same executed batch as the move.
    assert_removal_then_move_in_one_batch(r, x, 1);
}

// REQ-EVENT-001, CR 704.8, 704.3, 704.7, 122.1c (J2 a, d): X alone, one shield counter,
// [Destroy, StateBasedAction]: X's LKI holds ("shield", 1); both submission orders.
#[test]
fn req_event_001_r4_j2_lki_keeps_shield_counter() {
    let (base, x, _) = j2_base(1, false);
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert_j2(r, x, 1);
    }
}

/// The J2 Y-variant assertions: X as in [`assert_j2`], and Y moved once to the graveyard with an
/// LKI record.
fn assert_j2_with_y(r: &Run, x: ObjectId, y: ObjectId) {
    assert_j2(r, x, 1);
    assert!(r.world().lki.get(y).is_some(), "Y's LKI exists");
    assert_eq!(moves_of(r, y), 1);
    assert_card_in(r, y, ZoneId::Graveyard(P1));
}

// REQ-EVENT-001, CR 704.8, 704.3, 704.7, 122.1c (J2 b, d): a second permanent Y moved by an
// unrelated StateBasedAction in the same batch: Y's LKI exists and X's still holds the counter.
// Every placement of Y and both orders of X's members are run. Different objects are processed in
// submission order, so whole-run equality is asserted only between the two orders of X's members
// with Y at the same position (the X group itself is order independent); the J2 assertions are
// checked for every order.
#[test]
fn req_event_001_r4_j2_lki_with_unrelated_sba_object() {
    let (base, x, y) = j2_base(1, true);
    let y = y.expect("Y");
    let d = death(x, ZoneChangeCause::Destroy);
    let s = death(x, ZoneChangeCause::StateBasedAction);
    let yy = death(y, ZoneChangeCause::StateBasedAction);
    let pairs = [
        (
            vec![d.clone(), s.clone(), yy.clone()],
            vec![s.clone(), d.clone(), yy.clone()],
        ),
        (
            vec![yy.clone(), d.clone(), s.clone()],
            vec![yy.clone(), s.clone(), d.clone()],
        ),
        (vec![d.clone(), yy.clone(), s.clone()], vec![s, yy, d]),
    ];
    for (first, second) in pairs {
        let a = run_preferring(base.clone(), Call::batch(first.clone(), true), &[]);
        let b = run_preferring(base.clone(), Call::batch(second, true), &[]);
        assert_j2_with_y(&a, x, y);
        assert_j2_with_y(&b, x, y);
        assert_same(&a, &b, "the X group depends on its members' order");
        let again = run_preferring(base.clone(), Call::batch(first, true), &[]);
        assert_same(&a, &again, "a re-run differs");
    }
}

// REQ-EVENT-001, CR 704.8, 704.3, 704.7, 122.1c (J2 c, d): two shield counters on X: the shield
// removes one, and the LKI (the pre-SBA state) shows ("shield", 2); both submission orders.
#[test]
fn req_event_001_r4_j2_lki_two_shield_counters() {
    let (base, x, _) = j2_base(2, false);
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert_j2(r, x, 2);
    }
}

// ---------------------------------------------------------------------------------------------
// J3: a modification that does not change the destination (thread r4631818367)
// ---------------------------------------------------------------------------------------------

/// Runs the J3 group on `base` for each answer preference, in both submission orders; asserts
/// the common ground: no error, at most one request, the card in `zone`, one move, the registry
/// empty.
fn assert_j3(base: &Bus, x: ObjectId, zone: ZoneId, ids: [u64; 2]) {
    for prefer in [[ids[0], ids[1]], [ids[1], ids[0]]] {
        let (a, b) = both_orders(base, group(x), &prefer);
        for r in [&a, &b] {
            assert!(r.result.is_ok(), "{prefer:?}: {:?}", r.result);
            assert!(r.requests.len() <= 1, "{prefer:?}: at most one request");
            assert_eq!(moves_of(r, x), 1, "{prefer:?}");
            assert_card_in(r, x, zone);
            let mut applied = r.applications();
            applied.sort();
            assert_eq!(applied, eids(&ids), "{prefer:?}: both applied once");
            assert!(r.after.registry.is_empty(), "{prefer:?}: both consumed");
        }
    }
}

// REQ-EVENT-001, CR 704.7, 616.1f (J3): e1 redirects the Destroy member to exile; e2 sets the
// StateBasedAction member's face-down flag to false (it already is): both members are modified
// but only the redirect changes the destination, so the card ends in exile, whatever the answer.
#[test]
fn req_event_001_r4_j3_face_down_noop_never_beats_redirect() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![
            one_use(1, ZoneChangeCause::Destroy, set_dest(ZoneKind::Exile)),
            one_use(
                2,
                ZoneChangeCause::StateBasedAction,
                Action::Modify(Modification::SetFaceDown(false)),
            ),
        ],
    );
    assert_j3(&base, x, ZoneId::Exile, [1, 2]);
}

// REQ-EVENT-001, CR 704.7 (J3 control): e2 alone (face-down false on the StateBasedAction member):
// the card ends in the graveyard, one move, both submission orders.
#[test]
fn req_event_001_r4_j3_control_face_down_alone_graveyard() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![one_use(
            2,
            ZoneChangeCause::StateBasedAction,
            Action::Modify(Modification::SetFaceDown(false)),
        )],
    );
    let (a, b) = both_orders(&base, group(x), &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(moves_of(r, x), 1);
        assert_card_in(r, x, ZoneId::Graveyard(P1));
    }
}

// REQ-EVENT-001, CR 704.7, 616.1f (J3 variant): e1 redirects the Destroy member to exile; e2 sets
// the controller of the StateBasedAction member to player 1 (a modification that leaves the
// destination alone): the card ends in exile, whatever the answer.
#[test]
fn req_event_001_r4_j3_set_controller_never_beats_redirect() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![
            one_use(1, ZoneChangeCause::Destroy, set_dest(ZoneKind::Exile)),
            one_use(
                2,
                ZoneChangeCause::StateBasedAction,
                Action::Modify(Modification::SetController(P1)),
            ),
        ],
    );
    assert_j3(&base, x, ZoneId::Exile, [1, 2]);
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.7 (J3, two redirects): the Destroy member to exile and the
// StateBasedAction member to hand: exactly one move, the card in exile or in its owner's hand, no
// error; for the same answer both submission orders give the same zone, report, registry bytes
// and state hash.
#[test]
fn req_event_001_r4_j3_two_redirects_order_independent() {
    let (w, x) = one_permanent();
    let base = Bus::new(
        w,
        vec![
            one_use(1, ZoneChangeCause::Destroy, set_dest(ZoneKind::Exile)),
            one_use(
                2,
                ZoneChangeCause::StateBasedAction,
                set_dest(ZoneKind::Hand),
            ),
        ],
    );
    for prefer in [[1u64, 2], [2, 1]] {
        let (a, b) = both_orders(&base, group(x), &prefer);
        let zone = |r: &Run| r.world().zone_of(moved_to(r, x));
        for r in [&a, &b] {
            assert!(r.result.is_ok(), "{prefer:?}: {:?}", r.result);
            assert_eq!(moves_of(r, x), 1, "{prefer:?}");
            assert!(
                matches!(zone(r), Some(ZoneKind::Exile | ZoneKind::Hand)),
                "{prefer:?}: {:?}",
                zone(r)
            );
        }
        assert_eq!(
            zone(&a),
            zone(&b),
            "{prefer:?}: the same zone in both orders"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// J4: the entry-copy plan (thread r4631818376)
// ---------------------------------------------------------------------------------------------

const C: u64 = 1;

/// The J4 world: a card in player 1's hand, the copied object on the battlefield, and the one-use
/// copy effect C on every ZoneChange to the battlefield. Returns the bus, the card and the copied
/// object.
fn j4_base() -> (Bus, ObjectId, ObjectId) {
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
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
    (Bus::new(w, vec![effect]), card, copied)
}

/// The identical entry of `card` (hand to battlefield).
fn entry(card: ObjectId) -> GameEvent {
    zone_change(
        card,
        ZoneKind::Battlefield,
        ZoneChangeCause::StateBasedAction,
    )
}

/// Runs the two instances (pre-applied sets `first`, `second`) as one SBA batch in both orders;
/// asserts the two runs give the same output, registry, world and hash, and re-running is
/// identical. Returns the two runs.
fn j4_both_orders(base: &Bus, card: ObjectId, first: &[u64], second: &[u64]) -> (Run, Run) {
    let call = |a: &[u64], b: &[u64]| {
        Call::Batch(
            EventBatch::new(vec![
                instance_with(entry(card), a),
                instance_with(entry(card), b),
            ]),
            true,
        )
    };
    let x = run_preferring(base.clone(), call(first, second), &[C]);
    let y = run_preferring(base.clone(), call(second, first), &[C]);
    assert_same(&x, &y, "the output depends on submission order");
    let again = run_preferring(base.clone(), call(first, second), &[C]);
    assert_same(&x, &again, "a re-run differs");
    (x, y)
}

/// The J4 assertions: no error, the card enters the battlefield exactly once, as a copy of
/// `copied` iff `as_copy`, and no BecomeCopy event is executed or logged (CR 707.5).
fn assert_j4(r: &Run, card: ObjectId, copied: ObjectId, as_copy: bool) {
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(moves_of(r, card), 1, "the card enters exactly once");
    assert_card_in(r, card, ZoneId::Battlefield);
    let entered = moved_to(r, card);
    let expected = as_copy.then_some(copied);
    assert_eq!(r.world().events.markers().copy_of(entered), expected);
    let no_copy_event = |e: &fineprint_core::events::ExecutedEvent| {
        !matches!(e.instance.event(), GameEvent::BecomeCopy { .. })
    };
    assert!(r.executed().iter().all(no_copy_event), "no BecomeCopy");
    assert!(
        r.new_log()
            .iter()
            .flat_map(|b| b.events.iter())
            .all(no_copy_event),
        "no BecomeCopy logged"
    );
}

fn applied(r: &Run) -> Vec<EffectId> {
    r.applications()
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.7, 707.5, 616.1f (J4): two identical entries, A with C
// already applied and B without: C applies to B only; the member with the copy plan wins (the
// brief's reading), so in both orders the card enters once, as a copy, with no BecomeCopy event.
#[test]
fn req_event_001_r4_j4_member_with_copy_plan_wins() {
    let (base, card, copied) = j4_base();
    let (a, b) = j4_both_orders(&base, card, &[C], &[]);
    for r in [&a, &b] {
        assert_j4(r, card, copied, true);
        assert_eq!(applied(r), eids(&[C]));
        assert!(r.after.registry.is_empty(), "C is consumed");
    }
}

// REQ-EVENT-001, CR 704.7, 707.5 (J4 control): both instances with an empty applied set: one
// application of C, the card enters once, as a copy.
#[test]
fn req_event_001_r4_j4_control_both_empty_enters_as_copy() {
    let (base, card, copied) = j4_base();
    let (a, b) = j4_both_orders(&base, card, &[], &[]);
    for r in [&a, &b] {
        assert_j4(r, card, copied, true);
        assert_eq!(applied(r), eids(&[C]));
    }
}

// REQ-EVENT-001, CR 704.7, 614.5 (J4 control): both instances with C in the applied set: C does
// not apply; the card enters once, NOT as a copy.
#[test]
fn req_event_001_r4_j4_control_both_applied_not_a_copy() {
    let (base, card, copied) = j4_base();
    let (a, b) = j4_both_orders(&base, card, &[C], &[C]);
    for r in [&a, &b] {
        assert_j4(r, card, copied, false);
        assert!(applied(r).is_empty());
        assert!(r.after.registry.get(eid(C)).is_some(), "C is not consumed");
    }
}
