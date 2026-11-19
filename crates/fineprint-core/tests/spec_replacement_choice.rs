// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the replacement bus's chooser, choice requests, APNAP order, suspension and
//! resumption through `ChoiceSession`, the interrupt conversions, the registry, the event kinds
//! and the matcher (REQ-EVENT-001; CR 616.1, 614.5, SRS 15.3). Black-box: written from the
//! work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceError, ChoiceSession, ChoiceType, Interrupt, RequestId, Status,
};
use fineprint_core::events::{
    CounterTarget, DamageTarget, GameEvent, Phase, Step, ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::replacement::{
    Action, AmountOp, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification,
    ReplacementEffect, ReplacementRegistry, Shield,
};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::ZoneKind;
use support::*;

/// Two Skip effects (ids 1 and 2) on `matcher`, so the event needs a choice.
fn two_skips(matcher: EventMatcher) -> Vec<ReplacementEffect> {
    vec![
        fx(1, matcher.clone(), Action::Skip),
        fx(2, matcher, Action::Skip),
    ]
}

/// The single request of processing `event` against two Skip effects on `matcher`.
fn request_for(
    w: World,
    matcher: EventMatcher,
    event: GameEvent,
) -> fineprint_core::choice::PendingChoiceRequest {
    let r = run_event(Bus::new(w, two_skips(matcher)), event, &[0]);
    assert_eq!(r.applications(), eids(&[1]));
    r.requests[0].clone()
}

fn kind(k: BusEventKind) -> EventMatcher {
    EventMatcher::new(k)
}

// ---------------------------------------------------------------------------------------------
// chooser
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 616.1: the chooser for an object on the battlefield is its controller
// (here not its owner).
#[test]
fn req_event_001_chooser_battlefield_controller() {
    let mut w = World::new();
    let obj = w.create_controlled(ZoneKind::Battlefield, P1, Some(P2));
    let req = request_for(
        w.clone(),
        kind(BusEventKind::Tap),
        GameEvent::Tap { object: obj },
    );
    assert_eq!(req.chooser, P2);
    let req = request_for(w, kind(BusEventKind::Damage), dmg_object(obj, 1));
    assert_eq!(req.chooser, P2);
}

// REQ-EVENT-001, CR 616.1: the chooser for an object off the battlefield is its owner.
#[test]
fn req_event_001_chooser_graveyard_owner() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Graveyard, P2);
    let req = request_for(
        w,
        kind(BusEventKind::ZoneChange),
        zone_change(obj, ZoneKind::Exile, ZoneChangeCause::Exile),
    );
    assert_eq!(req.chooser, P2);
}

// REQ-EVENT-001, CR 616.1: the chooser for an event affecting a player is that player.
#[test]
fn req_event_001_chooser_affected_player() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P2);
    for (k, ev) in [
        (BusEventKind::Draw, GameEvent::Draw { player: P2 }),
        (BusEventKind::LifeChange, life_change(P2, -2)),
        (BusEventKind::Damage, dmg_player(P2, 2)),
    ] {
        let req = request_for(w.clone(), kind(k), ev);
        assert_eq!(req.chooser, P2, "{k:?}");
    }
}

// REQ-EVENT-001, CR 616.1: the chooser for a CreateToken is its controller if set, else its
// owner.
#[test]
fn req_event_001_chooser_create_token() {
    let mut w = World::new();
    let c1 = w.fresh_card();
    let c2 = w.fresh_card();
    let req = request_for(
        w.clone(),
        kind(BusEventKind::CreateToken),
        GameEvent::CreateToken {
            owner: P1,
            card: c1,
            controller: Some(P2),
        },
    );
    assert_eq!(req.chooser, P2);
    let req = request_for(
        w,
        kind(BusEventKind::CreateToken),
        GameEvent::CreateToken {
            owner: P2,
            card: c2,
            controller: None,
        },
    );
    assert_eq!(req.chooser, P2);
}

// REQ-EVENT-001, CR 616.1: the chooser for PhaseBegins and StepBegins is the active player.
#[test]
fn req_event_001_chooser_phase_and_step_active_player() {
    for active in [P1, P2] {
        let mut w = World::new();
        w.game
            .set_active_player(Some(active))
            .expect("known player");
        let req = request_for(
            w.clone(),
            kind(BusEventKind::PhaseBegins),
            GameEvent::PhaseBegins {
                phase: Phase::Combat,
            },
        );
        assert_eq!(req.chooser, active);
        let req = request_for(
            w,
            kind(BusEventKind::StepBegins),
            GameEvent::StepBegins { step: Step::Upkeep },
        );
        assert_eq!(req.chooser, active);
    }
}

// ---------------------------------------------------------------------------------------------
// request fields
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 616.1, SRS 15.3: the request's type, id, reason and payload.
#[test]
fn req_event_001_request_fields() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.create(ZoneKind::Library, P1);
    let mut effects = vec![
        fx(4, kind(BusEventKind::Tap), Action::Skip),
        fx(2, kind(BusEventKind::Tap), Action::Skip),
        fx(9, kind(BusEventKind::Tap), Action::Skip),
    ];
    effects.push(fx(5, kind(BusEventKind::Draw), Action::Skip));
    effects.push(fx(6, kind(BusEventKind::Draw), Action::Skip));
    let r = run_batch(
        Bus::new(w, effects),
        vec![
            GameEvent::Tap { object: obj },
            GameEvent::Draw { player: P1 },
        ],
        false,
        &[2, 0],
    );
    let tap = &r.requests[0];
    assert_eq!(tap.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(tap.request_id, RequestId::new(0));
    assert_eq!(tap.chooser, P1);
    assert_eq!(tap.reason.cr, "CR 616.1");
    assert_eq!(tap.reason.source, Some(obj));
    assert_eq!(payload(tap), ("TAP".to_owned(), descs(&[2, 4, 9])));
    let draw = &r.requests[1];
    assert_eq!(draw.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(draw.request_id, RequestId::new(1));
    assert_eq!(draw.reason.cr, "CR 616.1");
    assert_eq!(draw.reason.source, None);
    assert_eq!(payload(draw), ("DRAW".to_owned(), descs(&[5, 6])));
    // Index 2 picked effect 9 for the Tap; index 0 picked effect 5 for the Draw.
    assert_eq!(r.applications(), eids(&[9, 5]));
}

// ---------------------------------------------------------------------------------------------
// APNAP
// ---------------------------------------------------------------------------------------------

fn draw_skips(p: PlayerId, base: u64) -> Vec<ReplacementEffect> {
    let m = EventMatcher {
        affected_player: Some(p),
        ..kind(BusEventKind::Draw)
    };
    vec![
        fx(base, m.clone(), Action::Skip),
        fx(base + 1, m, Action::Skip),
    ]
}

// REQ-EVENT-001, CR 616.1 (simultaneous choosers -> APNAP): the active player is asked first,
// whatever the submission order; then the other player.
#[test]
fn req_event_001_apnap_order_of_requests() {
    for active in [P1, P2] {
        for order in [[P2, P1], [P1, P2]] {
            let mut w = World::new();
            w.game
                .set_active_player(Some(active))
                .expect("known player");
            w.create(ZoneKind::Library, P1);
            w.create(ZoneKind::Library, P2);
            let mut effects = draw_skips(P1, 1);
            effects.extend(draw_skips(P2, 3));
            let events = order
                .iter()
                .map(|&p| GameEvent::Draw { player: p })
                .collect();
            let r = run_batch(Bus::new(w, effects), events, false, &[0, 0]);
            let other = if active == P1 { P2 } else { P1 };
            assert_eq!(
                r.requests[0].chooser, active,
                "active {active:?}, order {order:?}"
            );
            assert_eq!(r.requests[1].chooser, other);
            let first = if active == P1 { 1 } else { 3 };
            let second = if active == P1 { 3 } else { 1 };
            assert_eq!(r.applications(), eids(&[first, second]));
        }
    }
}

// REQ-EVENT-001, CR 616.1: events of one chooser are processed in submission order.
#[test]
fn req_event_001_one_chooser_submission_order() {
    for draw_first in [true, false] {
        let mut w = World::new();
        w.create(ZoneKind::Library, P1);
        let mut effects = draw_skips(P1, 1);
        let lm = EventMatcher {
            affected_player: Some(P1),
            ..kind(BusEventKind::LifeChange)
        };
        effects.push(fx(3, lm.clone(), Action::Skip));
        effects.push(fx(4, lm, Action::Skip));
        let mut events = vec![GameEvent::Draw { player: P1 }, life_change(P1, 1)];
        if !draw_first {
            events.reverse();
        }
        let r = run_batch(Bus::new(w, effects), events, false, &[0, 0]);
        let names: Vec<String> = r.requests.iter().map(|q| payload(q).0).collect();
        let mut expected = vec!["DRAW".to_owned(), "LIFE_CHANGE".to_owned()];
        if !draw_first {
            expected.reverse();
        }
        assert_eq!(names, expected);
    }
}

// ---------------------------------------------------------------------------------------------
// session
// ---------------------------------------------------------------------------------------------

/// A battlefield object and three effects sending it to exile (1), hand (2) or library (3)
/// instead of the graveyard.
fn redirect_bus() -> (Bus, ObjectId) {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let m = EventMatcher {
        to: Some(ZoneKind::Graveyard),
        ..m_zc_object(obj)
    };
    let effects = [ZoneKind::Exile, ZoneKind::Hand, ZoneKind::Library]
        .iter()
        .enumerate()
        .map(|(i, &z)| {
            fx(
                i as u64 + 1,
                m.clone(),
                Action::Modify(Modification::SetDestination(z)),
            )
        })
        .collect();
    (Bus::new(w, effects), obj)
}

fn destroy(obj: ObjectId) -> Call {
    Call::Event(zone_change(
        obj,
        ZoneKind::Graveyard,
        ZoneChangeCause::Destroy,
    ))
}

// REQ-EVENT-001, CR 616.1, SRS 15.3: through a `ChoiceSession`: start -> pending (base state
// unchanged) -> an out-of-range index is rejected and leaves it pending -> each valid index
// completes with the matching effect applied; replay reproduces output, hash and bytes.
#[test]
fn req_event_001_session_suspend_resume_replay() {
    let dests = [ZoneKind::Exile, ZoneKind::Hand, ZoneKind::Library];
    for (i, dest) in dests.iter().enumerate() {
        let (bus, obj) = redirect_bus();
        let base_snap = bus.snap();
        let base_hash = bus.state_hash();
        let mut s = ChoiceSession::start(bus.clone(), procedure(destroy(obj))).expect("start");
        let req = s.pending().cloned().expect("pending");
        assert_eq!(offered(&req), descs(&[1, 2, 3]));
        assert_eq!(s.base().snap(), base_snap);
        assert_eq!(s.state_hash(), base_hash);

        let bad = s.submit(req.request_id, select(3));
        assert!(matches!(bad, Err(ChoiceError::Invalid { .. })), "{bad:?}");
        assert_eq!(s.pending(), Some(&req));
        assert!(s.answers().is_empty());
        assert_eq!(s.base().snap(), base_snap);

        let index = u32::try_from(i).expect("fits");
        s.submit(req.request_id, select(index)).expect("valid");
        let (state, output) = match s.status() {
            Status::Done { state, output } => (state.clone(), output.clone()),
            Status::Pending(p) => panic!("still pending: {p:?}"),
        };
        let report = output.clone().expect("no bus error");
        assert_eq!(report.applications, eids(&[i as u64 + 1]));
        let moved = new_id(&report.executed[0].events[0].outcome);
        assert_eq!(state.world.zone_of(moved), Some(*dest));
        // The base is still the pre-call state.
        assert_eq!(s.base().snap(), base_snap);

        let replayed =
            ChoiceSession::replay(bus, procedure(destroy(obj)), s.answers()).expect("replay");
        match replayed.status() {
            Status::Done {
                state: st2,
                output: out2,
            } => {
                assert_eq!(out2, &output);
                assert_eq!(st2.canonical(), state.canonical());
                assert_eq!(st2.snap(), state.snap());
            }
            Status::Pending(p) => panic!("replay pending: {p:?}"),
        }
        assert_eq!(replayed.state_hash(), s.state_hash());
        assert_ne!(s.state_hash(), base_hash);
    }
}

// REQ-EVENT-001: `From<Interrupt> for BusInterrupt` and `From<BusInterrupt> for Interrupt`.
#[test]
fn req_event_001_interrupt_conversions() {
    let (bus, obj) = redirect_bus();
    let r = run(bus, destroy(obj), &[0]);
    let req = r.requests[0].clone();

    match BusInterrupt::from(Interrupt::Pending(Box::new(req.clone()))) {
        BusInterrupt::Pending(p) => assert_eq!(*p, req),
        BusInterrupt::Error(e) => panic!("expected Pending, got {e:?}"),
    }
    match BusInterrupt::from(Interrupt::Error(ChoiceError::NotPending)) {
        BusInterrupt::Error(e) => assert_eq!(e, BusError::Choice(ChoiceError::NotPending)),
        BusInterrupt::Pending(_) => panic!("expected Error"),
    }
    assert_eq!(
        Interrupt::from(BusInterrupt::Pending(Box::new(req.clone()))),
        Interrupt::Pending(Box::new(req))
    );
    for e in [BusError::RepeatLimit, BusError::DepthExceeded] {
        assert_eq!(
            Interrupt::from(BusInterrupt::Error(e.clone())),
            Interrupt::Error(ChoiceError::Procedure(e.to_string()))
        );
    }
}

// ---------------------------------------------------------------------------------------------
// registry
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: adding an effect id twice is `DuplicateEffect`; the registry is unchanged.
#[test]
fn req_event_001_registry_duplicate() {
    let first = fx(1, kind(BusEventKind::Tap), Action::Skip);
    let mut reg = registry(vec![first.clone()]);
    let again = fx(1, kind(BusEventKind::Untap), Action::Skip);
    assert_eq!(reg.add(again), Err(BusError::DuplicateEffect(eid(1))));
    assert_eq!(reg.len(), 1);
    assert_eq!(reg.get(eid(1)), Some(&first));
}

fn amount(op: AmountOp) -> Action {
    Action::Modify(Modification::Amount(op))
}

// REQ-EVENT-001: every InvalidEffect case of `add`; the registry stays empty.
#[test]
fn req_event_001_registry_invalid_effects() {
    let obj = oid(1);
    let prevent = || Action::Prevent { then: Vec::new() };
    let cases: Vec<(&str, ReplacementEffect)> = vec![
        ("Prevent on Tap", fx(1, kind(BusEventKind::Tap), prevent())),
        (
            "Prevent on LifeChange",
            fx(1, kind(BusEventKind::LifeChange), prevent()),
        ),
        (
            "Pool on non-Damage",
            fx(1, kind(BusEventKind::Tap), prevent()).with_shield(Shield::Pool(2)),
        ),
        (
            "Pool without Prevent",
            fx(1, kind(BusEventKind::Damage), Action::Skip).with_shield(Shield::Pool(2)),
        ),
        (
            "Pool with Modify",
            fx(1, kind(BusEventKind::Damage), amount(AmountOp::Add(1)))
                .with_shield(Shield::Pool(2)),
        ),
        (
            "SetDestination on Damage",
            fx(
                1,
                kind(BusEventKind::Damage),
                Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
            ),
        ),
        (
            "SetDestination on CreateToken",
            fx(
                1,
                kind(BusEventKind::CreateToken),
                Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
            ),
        ),
        (
            "SetFaceDown on CreateToken",
            fx(
                1,
                kind(BusEventKind::CreateToken),
                Action::Modify(Modification::SetFaceDown(true)),
            ),
        ),
        (
            "EntersAsCopyOf on Damage",
            fx(
                1,
                kind(BusEventKind::Damage),
                Action::Modify(Modification::EntersAsCopyOf(obj)),
            ),
        ),
        (
            "SetController on Draw",
            fx(
                1,
                kind(BusEventKind::Draw),
                Action::Modify(Modification::SetController(P2)),
            ),
        ),
        (
            "Amount on ZoneChange",
            fx(1, kind(BusEventKind::ZoneChange), amount(AmountOp::Add(1))),
        ),
        (
            "Amount on Tap",
            fx(1, kind(BusEventKind::Tap), amount(AmountOp::Set(1))),
        ),
        (
            "matcher kind None",
            fx(1, EventMatcher::default(), Action::Skip),
        ),
        (
            "Uses(0)",
            fx(1, kind(BusEventKind::Damage), prevent()).with_shield(Shield::Uses(0)),
        ),
        (
            "Pool(0)",
            fx(1, kind(BusEventKind::Damage), prevent()).with_shield(Shield::Pool(0)),
        ),
        (
            "Pool(-1)",
            fx(1, kind(BusEventKind::Damage), prevent()).with_shield(Shield::Pool(-1)),
        ),
        (
            "Mul(-1)",
            fx(1, kind(BusEventKind::Damage), amount(AmountOp::Mul(-1))),
        ),
    ];
    for (name, effect) in cases {
        let mut reg = ReplacementRegistry::new();
        let got = reg.add(effect);
        assert!(
            matches!(got, Err(BusError::InvalidEffect(_))),
            "{name}: {got:?}"
        );
        assert!(reg.is_empty(), "{name}");
    }
}

// REQ-EVENT-001: the valid counterparts of the InvalidEffect cases are accepted.
#[test]
fn req_event_001_registry_valid_effects() {
    let obj = oid(1);
    let prevent = || Action::Prevent { then: Vec::new() };
    let mut n = 0u64;
    let mut next = || {
        n += 1;
        n
    };
    let effects = vec![
        fx(next(), kind(BusEventKind::Damage), prevent()),
        fx(next(), kind(BusEventKind::Damage), prevent()).with_shield(Shield::Pool(1)),
        fx(next(), kind(BusEventKind::Damage), prevent()).with_shield(Shield::Uses(1)),
        fx(next(), kind(BusEventKind::Tap), Action::Skip).with_shield(Shield::Uses(3)),
        fx(
            next(),
            kind(BusEventKind::ZoneChange),
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        ),
        fx(
            next(),
            kind(BusEventKind::ZoneChange),
            Action::Modify(Modification::SetFaceDown(true)),
        ),
        fx(
            next(),
            kind(BusEventKind::ZoneChange),
            Action::Modify(Modification::SetController(P2)),
        ),
        fx(
            next(),
            kind(BusEventKind::CreateToken),
            Action::Modify(Modification::SetController(P2)),
        ),
        fx(
            next(),
            kind(BusEventKind::ZoneChange),
            Action::Modify(Modification::EntersAsCopyOf(obj)),
        ),
        fx(
            next(),
            kind(BusEventKind::CreateToken),
            Action::Modify(Modification::EntersAsCopyOf(obj)),
        ),
        fx(next(), kind(BusEventKind::Damage), amount(AmountOp::Mul(0))),
        fx(
            next(),
            kind(BusEventKind::LifeChange),
            amount(AmountOp::Add(1)),
        ),
        fx(
            next(),
            kind(BusEventKind::AddCounters),
            amount(AmountOp::Mul(2)),
        ),
        fx(
            next(),
            kind(BusEventKind::RemoveCounters),
            amount(AmountOp::Set(0)),
        ),
        fx(
            next(),
            kind(BusEventKind::Draw),
            Action::Instead(vec![life_change(P1, 1)]),
        ),
    ];
    let count = effects.len();
    let reg = registry(effects);
    assert_eq!(reg.len(), count);
}

// REQ-EVENT-001: `ids()` is ascending whatever the insertion order; `remove` returns the effect.
#[test]
fn req_event_001_registry_ids_and_remove() {
    let e = |n| fx(n, kind(BusEventKind::Tap), Action::Skip);
    let mut reg = registry(vec![e(5), e(2), e(9)]);
    assert_eq!(reg.ids(), eids(&[2, 5, 9]));
    assert_eq!(reg.len(), 3);
    assert!(!reg.is_empty());
    assert_eq!(reg.remove(eid(5)), Some(e(5)));
    assert_eq!(reg.ids(), eids(&[2, 9]));
    assert_eq!(reg.remove(eid(5)), None);
    assert_eq!(reg.get(eid(5)), None);
    assert_eq!(reg.get(eid(9)), Some(&e(9)));
}

// REQ-EVENT-001, CR 615: canonical bytes are equal for equal registries regardless of insertion
// order, and change after a shield use.
#[test]
fn req_event_001_registry_canonical_encoding() {
    let a =
        fx(1, m_dmg_player(P2), Action::Prevent { then: Vec::new() }).with_shield(Shield::Uses(2));
    let b = fx(2, kind(BusEventKind::Tap), Action::Skip);
    let r1 = registry(vec![a.clone(), b.clone()]);
    let r2 = registry(vec![b, a]);
    assert_eq!(bytes(&r1), bytes(&r2));
    assert_eq!(r1, r2);
    assert_eq!(bytes(&r1.clone()), bytes(&r1));

    let bus = Bus {
        world: World::new(),
        registry: r1.clone(),
    };
    let r = run_event(bus, dmg_player(P2, 1), &[]);
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.after.registry.get(eid(1)).is_some());
    assert_ne!(bytes(&r.after.registry), bytes(&r1));
}

// ---------------------------------------------------------------------------------------------
// event kinds and matcher
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001: `BusEventKind::of` and `name()` for all 17 kinds.
#[test]
fn req_event_001_bus_event_kind_of_and_name() {
    let o = oid(1);
    let cases: Vec<(GameEvent, BusEventKind, &str)> = vec![
        (
            zone_change(o, ZoneKind::Exile, ZoneChangeCause::Other),
            BusEventKind::ZoneChange,
            "ZONE_CHANGE",
        ),
        (GameEvent::Draw { player: P1 }, BusEventKind::Draw, "DRAW"),
        (dmg_player(P1, 1), BusEventKind::Damage, "DAMAGE"),
        (life_change(P1, 1), BusEventKind::LifeChange, "LIFE_CHANGE"),
        (
            add_counters(o, "charge", 1),
            BusEventKind::AddCounters,
            "ADD_COUNTERS",
        ),
        (
            remove_counters(o, "charge", 1),
            BusEventKind::RemoveCounters,
            "REMOVE_COUNTERS",
        ),
        (GameEvent::Tap { object: o }, BusEventKind::Tap, "TAP"),
        (GameEvent::Untap { object: o }, BusEventKind::Untap, "UNTAP"),
        (
            GameEvent::CreateToken {
                owner: P1,
                card: cid(1),
                controller: None,
            },
            BusEventKind::CreateToken,
            "CREATE_TOKEN",
        ),
        (
            GameEvent::TurnBegins { player: P1 },
            BusEventKind::TurnBegins,
            "TURN_BEGINS",
        ),
        (
            GameEvent::PhaseBegins {
                phase: Phase::Ending,
            },
            BusEventKind::PhaseBegins,
            "PHASE_BEGINS",
        ),
        (
            GameEvent::StepBegins { step: Step::End },
            BusEventKind::StepBegins,
            "STEP_BEGINS",
        ),
        (
            GameEvent::Attach {
                attachment: o,
                target: AttachTarget::Player(P1),
            },
            BusEventKind::Attach,
            "ATTACH",
        ),
        (
            GameEvent::Unattach { attachment: o },
            BusEventKind::Unattach,
            "UNATTACH",
        ),
        (
            GameEvent::BecomeCopy {
                object: o,
                copied: oid(2),
            },
            BusEventKind::BecomeCopy,
            "BECOME_COPY",
        ),
        (
            GameEvent::TurnFaceUp { object: o },
            BusEventKind::TurnFaceUp,
            "TURN_FACE_UP",
        ),
        (
            GameEvent::TurnFaceDown { object: o },
            BusEventKind::TurnFaceDown,
            "TURN_FACE_DOWN",
        ),
    ];
    assert_eq!(cases.len(), 17);
    for (ev, k, name) in cases {
        assert_eq!(BusEventKind::of(&ev), k, "{ev:?}");
        assert_eq!(k.name(), name);
    }
}

/// Whether a Skip effect on `matcher` replaces `event` in `w`.
fn matches(w: &World, matcher: EventMatcher, event: GameEvent) -> bool {
    let r = run_event(
        Bus::new(w.clone(), vec![fx(1, matcher, Action::Skip)]),
        event,
        &[],
    );
    !r.applications().is_empty()
}

// REQ-EVENT-001, CR 614.5 (brief: a matcher field that does not apply to the event's kind makes
// the matcher not match): wrong-kind fields never match; the right-kind fields do.
#[test]
fn req_event_001_matcher_fields_on_wrong_kind_do_not_match() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let host = w.create(ZoneKind::Battlefield, P1);
    w.create(ZoneKind::Library, P1);
    let k = kind;
    let wrong: Vec<(&str, EventMatcher, GameEvent)> = vec![
        (
            "from on Damage",
            EventMatcher {
                from: Some(ZoneKind::Battlefield),
                ..k(BusEventKind::Damage)
            },
            dmg_object(obj, 1),
        ),
        (
            "to on Tap",
            EventMatcher {
                to: Some(ZoneKind::Battlefield),
                ..k(BusEventKind::Tap)
            },
            GameEvent::Tap { object: obj },
        ),
        (
            "cause on Damage",
            EventMatcher {
                cause: Some(ZoneChangeCause::Other),
                ..k(BusEventKind::Damage)
            },
            dmg_player(P2, 1),
        ),
        (
            "combat on Tap",
            EventMatcher {
                combat: Some(false),
                ..k(BusEventKind::Tap)
            },
            GameEvent::Tap { object: obj },
        ),
        (
            "combat on LifeChange",
            EventMatcher {
                combat: Some(false),
                ..k(BusEventKind::LifeChange)
            },
            life_change(P1, 1),
        ),
        (
            "counter_kind on ZoneChange",
            EventMatcher {
                counter_kind: Some("charge".to_owned()),
                ..k(BusEventKind::ZoneChange)
            },
            zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Other),
        ),
        (
            "amount_below on ZoneChange",
            EventMatcher {
                amount_below: Some(100),
                ..k(BusEventKind::ZoneChange)
            },
            zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Other),
        ),
        (
            "affected_object on Draw",
            EventMatcher {
                affected_object: Some(obj),
                ..k(BusEventKind::Draw)
            },
            GameEvent::Draw { player: P1 },
        ),
        (
            "affected_player on Tap",
            EventMatcher {
                affected_player: Some(P1),
                ..k(BusEventKind::Tap)
            },
            GameEvent::Tap { object: obj },
        ),
        (
            "affected_player on PhaseBegins",
            EventMatcher {
                affected_player: Some(P1),
                ..k(BusEventKind::PhaseBegins)
            },
            GameEvent::PhaseBegins {
                phase: Phase::Combat,
            },
        ),
        (
            "affected_object on CreateToken",
            EventMatcher {
                affected_object: Some(obj),
                ..k(BusEventKind::CreateToken)
            },
            GameEvent::CreateToken {
                owner: P1,
                card: cid(500),
                controller: None,
            },
        ),
        (
            "Tap matcher on Untap",
            EventMatcher {
                affected_object: Some(obj),
                ..k(BusEventKind::Tap)
            },
            GameEvent::Untap { object: obj },
        ),
    ];
    for (name, matcher, ev) in wrong {
        assert!(!matches(&w, matcher, ev), "{name} must not match");
    }

    let right: Vec<(&str, EventMatcher, GameEvent)> = vec![
        (
            "combat on Damage",
            EventMatcher {
                combat: Some(false),
                ..k(BusEventKind::Damage)
            },
            dmg_player(P2, 1),
        ),
        (
            "amount_below on LifeChange (|delta|)",
            EventMatcher {
                amount_below: Some(4),
                ..k(BusEventKind::LifeChange)
            },
            life_change(P1, -3),
        ),
        (
            "to Battlefield on CreateToken",
            EventMatcher {
                to: Some(ZoneKind::Battlefield),
                affected_player: Some(P1),
                ..k(BusEventKind::CreateToken)
            },
            GameEvent::CreateToken {
                owner: P1,
                card: cid(501),
                controller: None,
            },
        ),
        (
            "from/to/cause on ZoneChange",
            EventMatcher {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                cause: Some(ZoneChangeCause::Sacrifice),
                affected_object: Some(obj),
                ..k(BusEventKind::ZoneChange)
            },
            zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Sacrifice),
        ),
        (
            "affected_object on Attach is the attachment",
            EventMatcher {
                affected_object: Some(obj),
                ..k(BusEventKind::Attach)
            },
            GameEvent::Attach {
                attachment: obj,
                target: AttachTarget::Permanent(host),
            },
        ),
        (
            "counter_kind and amount_below on AddCounters",
            EventMatcher {
                counter_kind: Some("charge".to_owned()),
                amount_below: Some(3),
                affected_object: Some(obj),
                ..k(BusEventKind::AddCounters)
            },
            add_counters(obj, "charge", 2),
        ),
        (
            "affected_player on a counter event on a player",
            EventMatcher {
                affected_player: Some(P2),
                ..k(BusEventKind::AddCounters)
            },
            GameEvent::AddCounters {
                target: CounterTarget::Player(P2),
                kind: "poison".to_owned(),
                amount: 1,
            },
        ),
        (
            "affected_object on Damage to an object",
            EventMatcher {
                affected_object: Some(obj),
                ..k(BusEventKind::Damage)
            },
            GameEvent::Damage {
                source: None,
                source_controller: None,
                target: DamageTarget::Object(obj),
                amount: 1,
                combat: true,
            },
        ),
    ];
    for (name, matcher, ev) in right {
        assert!(matches(&w, matcher, ev), "{name} must match");
    }

    // Boundaries: amount_below is strict, and a different value of a field does not match.
    assert!(!matches(
        &w,
        EventMatcher {
            amount_below: Some(3),
            ..k(BusEventKind::LifeChange)
        },
        life_change(P1, -3)
    ));
    assert!(!matches(
        &w,
        EventMatcher {
            combat: Some(true),
            ..k(BusEventKind::Damage)
        },
        dmg_player(P2, 1)
    ));
    assert!(!matches(
        &w,
        EventMatcher {
            cause: Some(ZoneChangeCause::Destroy),
            ..k(BusEventKind::ZoneChange)
        },
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Sacrifice)
    ));
}
