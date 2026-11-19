// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for child events of the replacement bus: events created by replacement and
//! prevention effects go through the bus themselves (REQ-EVENT-001; SRS 8.1, CR 614.5, 615,
//! 616.1c). Black-box: written from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::encode::CanonicalEncode;
use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, AmountOp, BusError, BusEventKind, EventMatcher, Modification, ReplacementEffect,
    Shield, Tier,
};
use fineprint_core::zones::ZoneKind;
use support::*;

fn life_matcher(p: fineprint_base::ids::PlayerId) -> EventMatcher {
    EventMatcher {
        affected_player: Some(p),
        ..EventMatcher::new(BusEventKind::LifeChange)
    }
}

fn draw_matcher(p: fineprint_base::ids::PlayerId) -> EventMatcher {
    EventMatcher {
        affected_player: Some(p),
        ..EventMatcher::new(BusEventKind::Draw)
    }
}

// REQ-EVENT-001, SRS 8.1 (child events): an `Instead` replaces the draw with a life gain; the
// child is executed and is itself modified by another effect.
#[test]
fn req_event_001_instead_children_execute_and_can_be_replaced() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    let bus = Bus::new(
        w,
        vec![
            fx(
                1,
                draw_matcher(P1),
                Action::Instead(vec![life_change(P1, 3)]),
            ),
            fx(
                2,
                life_matcher(P1),
                Action::Modify(Modification::Amount(AmountOp::Add(2))),
            ),
        ],
    );
    let r = run_event(bus, GameEvent::Draw { player: P1 }, &[]);
    assert_eq!(r.applications(), eids(&[1, 2]));
    assert_eq!(r.world().life(P1), STARTING_LIFE + 5);
    assert!(
        r.world()
            .zones
            .is_empty(fineprint_core::zones::ZoneId::Hand(P1))
    );
    let executed = r.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].instance.event(), &life_change(P1, 5));
    assert_eq!(applied_of(&executed[0]), eids(&[2]));
}

// REQ-EVENT-001, CR 614.5, SRS 8.1: a child event has a fresh applied set, so the effect that
// replaced the parent can replace the child too.
#[test]
fn req_event_001_parent_effect_applies_again_to_child() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "charge", 1);
    let effect = fx(
        1,
        EventMatcher {
            affected_object: Some(obj),
            requires_counter: Some((obj, "charge".to_owned())),
            ..EventMatcher::new(BusEventKind::Tap)
        },
        Action::Instead(vec![
            remove_counters(obj, "charge", 1),
            GameEvent::Tap { object: obj },
        ]),
    );
    let r = run_event(
        Bus::new(w, vec![effect]),
        GameEvent::Tap { object: obj },
        &[],
    );
    // Depth 0: replaced. Depth 1: the child Tap is replaced again (the counter is still there
    // while the child batch is processed), the RemoveCounters executes. Depth 2: the Tap
    // executes.
    assert_eq!(r.applications(), eids(&[1, 1]));
    assert!(r.world().tapped(obj));
    assert_eq!(r.world().counter(obj, "charge"), 0);
}

// REQ-EVENT-001, CR 615, SRS 8.1: children of an event that is then dropped (here: a partial
// prevention with a follow-up, then a Skip) are still processed.
#[test]
fn req_event_001_children_of_dropped_event_are_processed() {
    for (pool_id, skip_id) in [(1u64, 2u64), (2, 1)] {
        let effects = vec![
            fx(
                pool_id,
                m_dmg_player(P2),
                Action::Prevent {
                    then: vec![life_change(P1, 1)],
                },
            )
            .with_shield(Shield::Pool(2)),
            fx(skip_id, m_dmg_player(P2), Action::Skip),
        ];
        let bus = Bus::new(World::new(), effects);
        let r = drive(bus, Call::Event(dmg_player(P2, 5)), |_, req| {
            index_of(req, pool_id)
        });
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.applications(), eids(&[pool_id, skip_id]));
        assert_eq!(r.report().prevented, 2);
        assert_eq!(r.world().life(P2), STARTING_LIFE);
        assert_eq!(r.world().life(P1), STARTING_LIFE + 1);
        assert_eq!(r.executed().len(), 1);
        assert_eq!(r.executed()[0].instance.event(), &life_change(P1, 1));
    }
    // A full prevention with a follow-up: the damage is dropped, the child is executed.
    let bus = Bus::new(
        World::new(),
        vec![fx(
            1,
            m_dmg_player(P2),
            Action::Prevent {
                then: vec![life_change(P2, 2)],
            },
        )],
    );
    let r = run_event(bus, dmg_player(P2, 3), &[]);
    assert_eq!(r.world().life(P2), STARTING_LIFE + 2);
    assert_eq!(r.report().prevented, 3);
}

// REQ-EVENT-001, CR 616.1c: `EntersAsCopyOf` on a ZoneChange to the battlefield yields a child
// BecomeCopy of the entered object's NEW id.
#[test]
fn req_event_001_enters_as_copy_zone_change_uses_new_id() {
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    let original = w.create(ZoneKind::Battlefield, P2);
    let effect = fx(
        1,
        EventMatcher {
            to: Some(ZoneKind::Battlefield),
            ..m_zc_object(card)
        },
        Action::Modify(Modification::EntersAsCopyOf(original)),
    )
    .with_tier(Tier::CopyEtb);
    let r = run_event(
        Bus::new(w, vec![effect]),
        zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other),
        &[],
    );
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 2);
    let entered = match batches[0].events[0].outcome {
        fineprint_core::events::Outcome::ZoneChange(m) => m.new,
        other => panic!("expected a ZoneChange outcome, got {other:?}"),
    };
    assert_ne!(entered, card);
    assert_eq!(
        batches[1].events[0].instance.event(),
        &GameEvent::BecomeCopy {
            object: entered,
            copied: original
        }
    );
    assert_eq!(r.world().events.markers().copy_of(entered), Some(original));
}

// REQ-EVENT-001, CR 616.1c: `EntersAsCopyOf` on a CreateToken yields a child BecomeCopy of the
// token's object id.
#[test]
fn req_event_001_enters_as_copy_create_token() {
    let mut w = World::new();
    let original = w.create(ZoneKind::Battlefield, P1);
    let card = w.fresh_card();
    let effect = fx(
        1,
        EventMatcher::new(BusEventKind::CreateToken),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    );
    let r = run_event(
        Bus::new(w, vec![effect]),
        GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        },
        &[],
    );
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 2);
    let token = match batches[0].events[0].outcome {
        fineprint_core::events::Outcome::TokenCreated { object } => object,
        other => panic!("expected TokenCreated, got {other:?}"),
    };
    assert_eq!(
        batches[1].events[0].instance.event(),
        &GameEvent::BecomeCopy {
            object: token,
            copied: original
        }
    );
    assert_eq!(r.world().events.markers().copy_of(token), Some(original));
}

// REQ-EVENT-001, CR 616.1c: if the final event does not enter the battlefield, `EntersAsCopyOf`
// does nothing.
#[test]
fn req_event_001_enters_as_copy_without_entering_does_nothing() {
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    let original = w.create(ZoneKind::Battlefield, P1);
    let etb = EventMatcher {
        to: Some(ZoneKind::Battlefield),
        ..m_zc_object(card)
    };
    let effects = vec![
        fx(
            1,
            etb.clone(),
            Action::Modify(Modification::EntersAsCopyOf(original)),
        )
        .with_tier(Tier::CopyEtb),
        fx(
            2,
            etb,
            Action::Modify(Modification::SetDestination(ZoneKind::Graveyard)),
        ),
    ];
    let r = run_event(
        Bus::new(w, effects),
        zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other),
        &[],
    );
    assert_eq!(r.applications(), eids(&[1, 2]));
    let executed = r.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(
        r.world().zone_of(new_id(&executed[0].outcome)),
        Some(ZoneKind::Graveyard)
    );
    assert!(
        executed
            .iter()
            .all(|e| !matches!(e.instance.event(), GameEvent::BecomeCopy { .. }))
    );
}

// REQ-EVENT-001, SRS 8.1: an `Instead` chain that re-creates itself forever is `DepthExceeded`;
// the registry and every world component are unchanged (child batches that executed are
// rolled back, a consumed shield is restored).
#[test]
fn req_event_001_endless_children_are_depth_exceeded() {
    let mut w = World::new();
    w.ok(life_change(P2, -1));
    let effects = vec![
        fx(
            1,
            life_matcher(P1),
            Action::Instead(vec![life_change(P1, 1), life_change(P2, -1)]),
        ),
        fx(
            2,
            life_matcher(P2),
            Action::Modify(Modification::Amount(AmountOp::Add(1))),
        )
        .with_shield(Shield::Uses(1)),
    ];
    let r = run_event(Bus::new(w, effects), life_change(P1, 1), &[]);
    assert_eq!(r.error(), &BusError::DepthExceeded);
    assert_unchanged(&r);
    assert_eq!(fineprint_core::replacement::MAX_CHILD_DEPTH, 64);
}

// REQ-EVENT-001, SRS 8.1: executed batches are reported parent first, then child batches, and
// match the logged batches.
#[test]
fn req_event_001_executed_batches_parent_then_children() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.create(ZoneKind::Library, P1);
    let effects: Vec<ReplacementEffect> = vec![
        fx(
            1,
            m_dmg_player(P2),
            Action::Instead(vec![life_change(P1, 1), GameEvent::Draw { player: P1 }]),
        ),
        fx(
            2,
            EventMatcher {
                affected_player: Some(P1),
                ..EventMatcher::new(BusEventKind::Draw)
            },
            Action::Instead(vec![life_change(P2, -1)]),
        ),
    ];
    let r = run_batch(
        Bus::new(w, effects),
        vec![dmg_player(P2, 4), GameEvent::Tap { object: obj }],
        false,
        &[],
    );
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 3);
    assert_eq!(
        batches[0].events[0].instance.event(),
        &GameEvent::Tap { object: obj }
    );
    assert_eq!(batches[1].events.len(), 1);
    assert_eq!(batches[1].events[0].instance.event(), &life_change(P1, 1));
    assert_eq!(batches[2].events[0].instance.event(), &life_change(P2, -1));
    let log = r.new_log();
    assert_eq!(log.len(), 3);
    for (a, b) in log.iter().zip(batches.iter()) {
        let (mut x, mut y) = (Vec::new(), Vec::new());
        a.encode_canonical(&mut x);
        b.encode_canonical(&mut y);
        assert_eq!(x, y);
    }
    assert!(batches[0].index < batches[1].index && batches[1].index < batches[2].index);
    assert_eq!(r.world().life(P2), STARTING_LIFE - 1);
}
