// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for review round 6 of the replacement bus (REQ-EVENT-001, REQ-ARCH-002,
//! REQ-CORE-001; CR 101.3, 101.4, 616.1, 614.6, 614.7, 400.7, 608.2h, 701.3b, 701.26a, 615.5).
//!
//! * D1 (thread r4632671472; CR 101.4, 616.1, SRS 8.1 [2]): the events of a batch and every
//!   `Instead` descendant of them are resolved one at a time in APNAP order of the event's
//!   chooser, then submission position; a descendant takes the position of the event it replaces
//!   and is resolved as soon as it is the earliest unresolved one after that event.
//!   Recursively (thread r4633092145): a descendant's position is its parent's position followed
//!   by its index in the parent's `Instead` list, so lineages expand depth-first.
//! * D2 (thread r4632671485; CR 101.3, 614.6, 400.7, 701.3b, 608.2h): in a later batch of the
//!   same call an event is ignored if it names, as its affected object, an `Attach` host or a
//!   `BecomeCopy.copied` object, an object an earlier executed batch moved; the rest of that
//!   batch still executes; a damage source that moved is not such a role; a never-issued id
//!   still fails with `UnknownObject` and the whole call rolls back.
//! * D6 (thread r4632818842; CR 701.26a, 614.7): a `Tap` of a tapped permanent would not happen:
//!   it offers no effect and changes nothing.
//!
//! Every new call is run twice and the canonical outputs compared (REQ-CORE-001).
//!
//! Black-box: written from the work-item brief only. Every object and effect is synthetic (R5).

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{DamageTarget, EventError, GameEvent, Outcome, ZoneChangeCause};
use fineprint_core::memory::MemoryError;
use fineprint_core::permanent::{AttachTarget, PermanentError};
use fineprint_core::replacement::{
    Action, AmountOp, BusError, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield,
};
use fineprint_core::zones::{ZoneError, ZoneId, ZoneKind};
use support::*;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn object_matcher(kind: BusEventKind, o: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(o),
        ..EventMatcher::new(kind)
    }
}

/// "doubler": effect 9, a one-use doubling of any LifeChange (any player).
fn doubler() -> ReplacementEffect {
    fx(
        9,
        m(BusEventKind::LifeChange),
        Action::Modify(Modification::Amount(AmountOp::Mul(2))),
    )
    .with_shield(Shield::Uses(1))
}

/// "tap_to_gain(n, t, p, d)": a shield-less effect replacing a Tap of `t` by a gain of `d` for `p`.
fn tap_to_gain(n: u64, t: ObjectId, p: PlayerId, d: i64) -> ReplacementEffect {
    fx(
        n,
        object_matcher(BusEventKind::Tap, t),
        Action::Instead(vec![life_change(p, d)]),
    )
}

fn tap(object: ObjectId) -> GameEvent {
    GameEvent::Tap { object }
}

/// Asserts that two runs of one call agree on the report, the registry bytes, the world and the
/// state hash (REQ-CORE-001).
fn assert_same(a: &Run, b: &Run, what: &str) {
    assert_eq!(a.result, b.result, "{what}: report");
    assert_eq!(a.answers, b.answers, "{what}: answers");
    assert_eq!(
        bytes(&a.after.registry),
        bytes(&b.after.registry),
        "{what}: registry"
    );
    assert_eq!(a.after.snap(), b.after.snap(), "{what}: world");
    assert_eq!(a.after.canonical(), b.after.canonical(), "{what}: state");
    assert_eq!(a.hash, b.hash, "{what}: state hash");
}

/// `run(base, call, answers)` twice; asserts both runs agree and returns the first.
fn run_twice(base: &Bus, call: Call, answers: &[u32]) -> Run {
    let a = run(base.clone(), call.clone(), answers);
    let b = run(base.clone(), call, answers);
    assert_same(&a, &b, "a re-run differs");
    a
}

/// `run_batch` twice (determinism, REQ-CORE-001).
fn batch_twice(base: &Bus, events: Vec<GameEvent>, sba: bool) -> Run {
    run_twice(base, Call::batch(events, sba), &[])
}

/// Drives `call` answering 0 to every request, twice; asserts both runs agree.
fn drive_zero_twice(base: &Bus, call: Call) -> Run {
    let a = drive(base.clone(), call.clone(), |_, _| 0);
    let b = drive(base.clone(), call, |_, _| 0);
    assert_same(&a, &b, "a re-run differs");
    assert_eq!(
        a.requests.iter().map(|r| r.chooser).collect::<Vec<_>>(),
        b.requests.iter().map(|r| r.chooser).collect::<Vec<_>>(),
        "a re-run asks differently"
    );
    a
}

/// Whether `e` is a ZoneChange of `obj` (by its id at submission).
fn is_move_of(e: &GameEvent, obj: ObjectId) -> bool {
    matches!(e, GameEvent::ZoneChange { object, .. } if *object == obj)
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

/// Asserts that `obj`'s single executed move put its card in `zone` (CR 400.7: a new object of
/// the same card; the old id is in no zone).
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

// ---------------------------------------------------------------------------------------------
// D1: descendants are resolved in APNAP order at their parent's position (thread r4632671472)
// ---------------------------------------------------------------------------------------------

/// T on the battlefield, owned by player 1 and controlled by `controller`; effects
/// tap_to_gain(1, T, P1, 1) and the doubler.
fn d1_base(controller: PlayerId) -> (Bus, ObjectId) {
    let mut w = World::new();
    let t = w.create_controlled(ZoneKind::Battlefield, P1, Some(controller));
    let bus = Bus::new(w, vec![tap_to_gain(1, t, P1, 1), doubler()]);
    (bus, t)
}

// REQ-EVENT-001, REQ-CORE-001, CR 101.4, 616.1, SRS 8.1 [2] (D1 a, thread r4632671472): T is
// player 1's; its Tap becomes player 1's gain of 1 (e1). That descendant and player 2's gain of 1
// are simultaneous: the one-use doubler goes to the active player's gain whether the Tap was
// submitted first or second. P1 ends at +2, P2 at +1, applications [e1, e9], e9 used up; a
// control with both gains submitted directly gives the same life totals.
#[test]
fn req_event_001_r6_d1_descendant_gain_gets_the_apnap_use() {
    let (base, t) = d1_base(P1);
    for (i, events) in [
        vec![tap(t), life_change(P2, 1)],
        vec![life_change(P2, 1), tap(t)],
    ]
    .into_iter()
    .enumerate()
    {
        let r = batch_twice(&base, events, false);
        assert!(r.result.is_ok(), "order {i}: {:?}", r.result);
        assert_eq!(r.world().life(P1), STARTING_LIFE + 2, "order {i}: P1");
        assert_eq!(r.world().life(P2), STARTING_LIFE + 1, "order {i}: P2");
        assert_eq!(r.applications(), eids(&[1, 9]), "order {i}");
        assert!(
            r.after.registry.get(eid(9)).is_none(),
            "order {i}: e9 used up"
        );
    }
    let control = batch_twice(&base, vec![life_change(P1, 1), life_change(P2, 1)], false);
    assert!(control.result.is_ok(), "{:?}", control.result);
    assert_eq!(control.world().life(P1), STARTING_LIFE + 2, "control: P1");
    assert_eq!(control.world().life(P2), STARTING_LIFE + 1, "control: P2");
}

// REQ-EVENT-001, REQ-CORE-001, CR 101.4, 616.1, SRS 8.1 [2] (D1 b, thread r4632671472): T2 is
// controlled by player 2, so its Tap's chooser is player 2; its descendant is player 1's gain,
// whose chooser is player 1 (active): once the Tap has resolved, the descendant is the earliest
// unresolved event in APNAP order and takes the doubler. P1 +2, P2 +1.
#[test]
fn req_event_001_r6_d1_p2_event_descendant_with_p1_chooser() {
    let (base, t2) = d1_base(P2);
    let r = batch_twice(&base, vec![tap(t2), life_change(P2, 1)], false);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().life(P1), STARTING_LIFE + 2, "P1");
    assert_eq!(r.world().life(P2), STARTING_LIFE + 1, "P2");
}

// REQ-EVENT-001, REQ-CORE-001, CR 101.4, 616.1, SRS 8.1 [2] (D1 c, thread r4632671472): both
// gains are player 1's, so submission position decides; the descendant takes the Tap's position.
// [Tap, gain 3]: the descendant gain of 1 is doubled, P1 ends at +5; [gain 3, Tap]: the gain of 3
// is doubled, P1 ends at +7.
#[test]
fn req_event_001_r6_d1_descendant_takes_parent_position() {
    let (base, t) = d1_base(P1);
    let r = batch_twice(&base, vec![tap(t), life_change(P1, 3)], false);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(
        r.world().life(P1),
        STARTING_LIFE + 5,
        "the Tap's descendant first"
    );
    let r = batch_twice(&base, vec![life_change(P1, 3), tap(t)], false);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().life(P1), STARTING_LIFE + 7, "the gain of 3 first");
}

// REQ-EVENT-001, REQ-CORE-001, CR 101.4, 616.1, SRS 8.1 [2], SRS 15.3 (D1 d, thread
// r4632671472): two shield-less LifeChange effects e3, e4 make every LifeChange ask once
// (CHOOSE_REPLACEMENT_EFFECT); the Tap of player 1's T becomes player 1's gain. With [Tap, P2's
// gain] and answer 0 to every request, the first request's chooser (`PendingChoiceRequest::
// chooser`) is player 1, the second's player 2.
#[test]
fn req_event_001_r6_d1_requests_in_apnap_order_for_descendants() {
    let mut w = World::new();
    let t = w.create(ZoneKind::Battlefield, P1);
    let add_one = |n| {
        fx(
            n,
            m(BusEventKind::LifeChange),
            Action::Modify(Modification::Amount(AmountOp::Add(1))),
        )
    };
    let base = Bus::new(w, vec![tap_to_gain(1, t, P1, 1), add_one(3), add_one(4)]);
    let r = drive_zero_twice(&base, Call::batch(vec![tap(t), life_change(P2, 1)], false));
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert!(
        r.requests.len() >= 2,
        "at least two requests: {:?}",
        r.requests
    );
    assert_eq!(r.requests[0].chooser, P1, "the descendant's request first");
    assert_eq!(r.requests[1].chooser, P2, "then player 2's");
}

// REQ-EVENT-001, REQ-CORE-001, CR 101.4, 616.1, SRS 8.1 [2] (D1 e, thread r4632671472): without
// `Instead` the order is unchanged: [P2's gain, P1's gain] with the doubler: player 1 (active)
// takes the use, P1 +2, P2 +1.
#[test]
fn req_event_001_r6_d1_direct_submission_order_unchanged() {
    let base = Bus::new(World::new(), vec![doubler()]);
    let r = batch_twice(&base, vec![life_change(P2, 1), life_change(P1, 1)], false);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.world().life(P1), STARTING_LIFE + 2, "P1");
    assert_eq!(r.world().life(P2), STARTING_LIFE + 1, "P2");
}

// REQ-EVENT-001, REQ-CORE-001, CR 616.1, 101.4 (D1, thread r4633092145): a descendant takes the
// exact position of the event it replaces, recursively (its parent's position followed by its
// index in the parent's `Instead` list). T, U are player 1's; e1: Tap T -> Instead[Tap U, P1
// gains 3]; e2: Tap U -> Instead[P1 gains 1]; e9: a one-use doubling of a player-1 gain. The
// grandchild gain of 1 sits at Tap U's position, before the gain of 3, so it is doubled: P1 ends
// at +5 (2 + 3), applications [e1, e2, e9]. Control with e1's list [P1 gains 3, Tap U]: the gain
// of 3 is doubled, +7, applications [e1, e9, e2]. A gain of player 2 submitted before or after
// Tap T changes neither result.
#[test]
fn req_event_001_r6_lineage_grandchild_takes_parent_position() {
    let base = |tap_u_first: bool| {
        let mut w = World::new();
        let t = w.create(ZoneKind::Battlefield, P1);
        let u = w.create(ZoneKind::Battlefield, P1);
        let list = if tap_u_first {
            vec![tap(u), life_change(P1, 3)]
        } else {
            vec![life_change(P1, 3), tap(u)]
        };
        let p1_doubler = fx(
            9,
            EventMatcher {
                affected_player: Some(P1),
                ..EventMatcher::new(BusEventKind::LifeChange)
            },
            Action::Modify(Modification::Amount(AmountOp::Mul(2))),
        )
        .with_shield(Shield::Uses(1));
        let effects = vec![
            fx(
                1,
                object_matcher(BusEventKind::Tap, t),
                Action::Instead(list),
            ),
            tap_to_gain(2, u, P1, 1),
            p1_doubler,
        ];
        (Bus::new(w, effects), t)
    };
    for (tap_u_first, p1, apps) in [(true, 5, [1, 2, 9]), (false, 7, [1, 9, 2])] {
        let (bus, t) = base(tap_u_first);
        for events in [
            vec![tap(t)],
            vec![tap(t), life_change(P2, 1)],
            vec![life_change(P2, 1), tap(t)],
        ] {
            let what = format!("Tap U first: {tap_u_first}, {events:?}");
            let p2 = i64::from(events.len() == 2);
            let r = batch_twice(&bus, events, false);
            assert!(r.result.is_ok(), "{what}: {:?}", r.result);
            assert_eq!(r.world().life(P1), STARTING_LIFE + p1, "{what}: P1");
            assert_eq!(r.world().life(P2), STARTING_LIFE + p2, "{what}: P2");
            assert_eq!(r.applications(), eids(&apps), "{what}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// D2: impossible instructions in later batches (thread r4632671485)
// ---------------------------------------------------------------------------------------------

/// The D2 world: X and Z on the battlefield (player 1), A on the battlefield (player 2).
struct D2 {
    bus: Bus,
    x: ObjectId,
    z: ObjectId,
    a: ObjectId,
}

/// How the shape's follow-up list reaches the later batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Via {
    /// e1's `Prevent { then }` is the shape's list.
    Direct,
    /// e1's `then` is [P1 gains 1]; e2 (one use) replaces that gain by the shape's list (with a
    /// gain of 2 in place of a gain of 1).
    Instead,
}

const VIAS: [Via; 2] = [Via::Direct, Via::Instead];

/// Builds the D2 world; `list(x, z, gain)` is the shape's follow-up list, `gain` being the
/// shape's player-1 gain (1 directly, 2 via Instead).
fn d2(via: Via, list: impl Fn(ObjectId, ObjectId, GameEvent) -> Vec<GameEvent>) -> D2 {
    let mut w = World::new();
    let x = w.create(ZoneKind::Battlefield, P1);
    let z = w.create(ZoneKind::Battlefield, P1);
    let a = w.create(ZoneKind::Battlefield, P2);
    let prevent = |then| {
        fx(
            1,
            object_matcher(BusEventKind::Damage, a),
            Action::Prevent { then },
        )
    };
    let effects = match via {
        Via::Direct => vec![prevent(list(x, z, life_change(P1, 1)))],
        Via::Instead => vec![
            prevent(vec![life_change(P1, 1)]),
            fx(
                2,
                m(BusEventKind::LifeChange),
                Action::Instead(list(x, z, life_change(P1, 2))),
            )
            .with_shield(Shield::Uses(1)),
        ],
    };
    D2 {
        bus: Bus::new(w, effects),
        x,
        z,
        a,
    }
}

/// The D2 submission (`sba = true`): 1 damage to A and X's StateBasedAction move to the graveyard.
fn d2_run(s: &D2) -> Run {
    batch_twice(
        &s.bus,
        vec![
            dmg_object(s.a, 1),
            zone_change(s.x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        ],
        true,
    )
}

/// The common expectations of a D2 shape that succeeds: Ok; 1 damage prevented; X's card in the
/// graveyard; the first executed batch (holding X's move) kept.
fn assert_d2_ok(r: &Run, s: &D2, via: Via) {
    assert!(r.result.is_ok(), "{via:?}: {:?}", r.result);
    assert_eq!(r.report().prevented, 1, "{via:?}: prevented");
    assert_card_in(r, s.x, ZoneId::Graveyard(P1));
    let batches = r.nonempty_batches();
    assert!(
        batches[0]
            .events
            .iter()
            .any(|e| is_move_of(e.instance.event(), s.x)),
        "{via:?}: the first executed batch (X's move) is kept"
    );
}

/// The player-1 gain a D2 shape ends with: 1 directly, 2 via Instead.
fn gain(via: Via) -> i64 {
    match via {
        Via::Direct => 1,
        Via::Instead => 2,
    }
}

// REQ-EVENT-001, REQ-ARCH-002, REQ-CORE-001, CR 101.3, 614.6, 400.7, 701.3b (D2 a, thread
// r4632671485): the later batch attaches Z to X, which moved to the graveyard in the first batch
// (CR 400.7: X is a new object), and gives player 1 life. The Attach is an impossible instruction
// and is ignored (Z does not move, CR 701.3b); the rest of the batch is still carried out (CR
// 101.3). Ok; Z stays on the battlefield attached to nothing; no Attach executed; P1 +1 (via
// Instead +2). Directly and via Instead.
#[test]
fn req_event_001_r6_d2_attach_onto_moved_host_is_ignored() {
    for via in VIAS {
        let s = d2(via, |x, z, g| {
            vec![
                GameEvent::Attach {
                    attachment: z,
                    target: AttachTarget::Permanent(x),
                },
                g,
            ]
        });
        let r = d2_run(&s);
        assert_d2_ok(&r, &s, via);
        assert_eq!(
            r.world().zone_of(s.z),
            Some(ZoneKind::Battlefield),
            "{via:?}: Z stays"
        );
        assert!(
            r.world().permanents.attached_to(s.z).is_none(),
            "{via:?}: Z is attached to nothing"
        );
        assert!(
            r.executed()
                .iter()
                .all(|e| !matches!(e.instance.event(), GameEvent::Attach { .. })),
            "{via:?}: no Attach executed"
        );
        assert_eq!(r.world().life(P1), STARTING_LIFE + gain(via), "{via:?}: P1");
    }
}

// REQ-EVENT-001, REQ-ARCH-002, CR 101.3, 615.5 (D2 b, thread r4632671485): an Attach onto an
// object id that was never issued is not an impossible instruction but a caller error: the call
// fails with UnknownObject for that id and everything is rolled back. Directly and via Instead.
#[test]
fn req_event_001_r6_d2_attach_onto_never_issued_host_fails() {
    let ghost = oid(9_999);
    for via in VIAS {
        let s = d2(via, |_, z, _| {
            vec![GameEvent::Attach {
                attachment: z,
                target: AttachTarget::Permanent(ghost),
            }]
        });
        let r = d2_run(&s);
        assert!(unknown_object(r.error(), ghost), "{via:?}: {:?}", r.error());
        assert_unchanged(&r);
    }
}

// REQ-EVENT-001, REQ-ARCH-002, CR 608.2h, 615.5 (D2 c, thread r4632671485): the later batch deals
// 1 damage from X, which moved in the first batch, to player 2. The source is not a role of the
// impossible-instruction check: damage from a source that has moved is still dealt (the object as
// it most recently existed deals it, CR 608.2h). Ok; P2 at -1; the damage executes in a batch
// after the first. Directly and via Instead.
#[test]
fn req_event_001_r6_d2_damage_from_moved_source_is_dealt() {
    for via in VIAS {
        let s = d2(via, |x, _, _| {
            vec![GameEvent::Damage {
                source: Some(x),
                source_controller: Some(P1),
                target: DamageTarget::Player(P2),
                amount: 1,
                combat: false,
            }]
        });
        let r = d2_run(&s);
        assert_d2_ok(&r, &s, via);
        assert_eq!(r.world().life(P2), STARTING_LIFE - 1, "{via:?}: P2");
        let batches = r.nonempty_batches();
        let at = batches
            .iter()
            .position(|b| {
                b.events.iter().any(|e| {
                    matches!(
                        e.instance.event(),
                        GameEvent::Damage {
                            target: DamageTarget::Player(p),
                            ..
                        } if *p == P2
                    )
                })
            })
            .unwrap_or_else(|| panic!("{via:?}: the damage to P2 is executed: {batches:?}"));
        assert!(at > 0, "{via:?}: the damage executes after the first batch");
    }
}

// REQ-EVENT-001, REQ-ARCH-002, REQ-CORE-001, CR 101.3, 614.6, 400.7 (D2 d, thread r4632671485):
// the later batch makes Z a copy of X, which moved in the first batch, and gives player 1 life.
// The BecomeCopy is ignored (an engine limitation recorded in DECISIONS: the copied object's role
// is checked like an affected object's); the gain is still carried out. Ok; Z is no copy; no
// BecomeCopy executed; P1 +1 (via Instead +2). Directly and via Instead.
#[test]
fn req_event_001_r6_d2_copy_of_moved_object_is_ignored() {
    for via in VIAS {
        let s = d2(via, |x, z, g| {
            vec![
                GameEvent::BecomeCopy {
                    object: z,
                    copied: x,
                },
                g,
            ]
        });
        let r = d2_run(&s);
        assert_d2_ok(&r, &s, via);
        assert_eq!(
            r.world().events.markers().copy_of(s.z),
            None,
            "{via:?}: Z is no copy"
        );
        assert!(
            r.executed()
                .iter()
                .all(|e| !matches!(e.instance.event(), GameEvent::BecomeCopy { .. })),
            "{via:?}: no BecomeCopy executed"
        );
        assert_eq!(r.world().life(P1), STARTING_LIFE + gain(via), "{via:?}: P1");
    }
}

// ---------------------------------------------------------------------------------------------
// D6: a Tap of a tapped permanent (thread r4632818842)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-CORE-001, CR 701.26a, 614.7 (D6 a, thread r4632818842): only untapped
// permanents can be tapped, so a Tap of a tapped permanent would not happen and the one-use
// replacement effect on it does nothing: Ok; no application, no request; e1 keeps its use (the
// registry bytes are unchanged); P1 unchanged; nothing executed or logged; obj still tapped; the
// whole state unchanged. Control: the same effect on an untapped permanent applies once (P1 +1,
// obj stays untapped).
#[test]
fn req_event_001_r6_d6_tap_of_tapped_permanent_offers_no_effect() {
    let world = |tapped: bool| {
        let mut w = World::new();
        let obj = w.create(ZoneKind::Battlefield, P1);
        if tapped {
            w.ok(tap(obj));
        }
        let effect = fx(
            1,
            object_matcher(BusEventKind::Tap, obj),
            Action::Instead(vec![life_change(P1, 1)]),
        )
        .with_shield(Shield::Uses(1));
        (Bus::new(w, vec![effect]), obj)
    };

    let (base, obj) = world(true);
    assert!(base.world.tapped(obj), "precondition: obj is tapped");
    let r = run_twice(&base, Call::Event(tap(obj)), &[]);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert!(r.applications().is_empty(), "no application");
    assert!(r.requests.is_empty(), "no request");
    assert_eq!(
        bytes(&r.after.registry),
        bytes(&r.before.registry),
        "e1 keeps its use"
    );
    assert_eq!(r.world().life(P1), STARTING_LIFE);
    assert!(r.executed().is_empty(), "nothing executed");
    assert!(r.new_log().is_empty(), "nothing logged");
    assert!(r.world().tapped(obj), "obj is still tapped");
    assert_unchanged(&r);

    let (base, obj) = world(false);
    let r = run_twice(&base, Call::Event(tap(obj)), &[]);
    assert!(r.result.is_ok(), "{:?}", r.result);
    assert_eq!(r.applications(), eids(&[1]), "control: one application");
    assert_eq!(r.world().life(P1), STARTING_LIFE + 1, "control: P1");
    assert!(!r.world().tapped(obj), "control: obj stays untapped");
}
