// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for child events of the replacement bus: events created by replacement and
//! prevention effects go through the bus themselves (REQ-EVENT-001; SRS 8.1, CR 614.5, 614.6,
//! 615.5, 616.1c). An `Instead` child is a modified event replacing its parent and inherits the
//! parent's applied set (CR 614.5); a `Prevent { then }` follow-up is a new event with an empty
//! applied set (CR 615.5). Black-box: written from the work-item brief only.

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

// REQ-EVENT-001, SRS 8.1 (child events), CR 614.5, 614.6: an `Instead` replaces the draw with
// a life gain; the child is executed and is itself modified by another effect. The child is a
// modified event replacing the draw, so its executed applied set holds the parent's effect too.
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
    assert_eq!(applied_of(&executed[0]), eids(&[1, 2]));
}

/// A Tap effect on `obj` (while it has a charge counter) replaced by removing a charge counter
/// and tapping it.
fn tap_again_effect(obj: fineprint_base::ids::ObjectId) -> ReplacementEffect {
    fx(
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
    )
}

// REQ-EVENT-001, CR 614.5, SRS 8.1: an `Instead` child is a modified event replacing its parent
// and inherits the parent's applied set, so the effect that replaced the parent does not
// replace the child (it had its one opportunity).
#[test]
fn req_event_001_parent_effect_does_not_apply_to_instead_child() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "charge", 1);
    let r = run_event(
        Bus::new(w, vec![tap_again_effect(obj)]),
        GameEvent::Tap { object: obj },
        &[],
    );
    // Depth 0: replaced. Depth 1: the child Tap (applied set {1}) and the RemoveCounters
    // execute together.
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.world().tapped(obj));
    assert_eq!(r.world().counter(obj, "charge"), 0);
    let tap = r
        .executed()
        .into_iter()
        .find(|e| matches!(e.instance.event(), GameEvent::Tap { .. }))
        .expect("the child Tap executed");
    assert_eq!(applied_of(&tap), eids(&[1]));
}

// REQ-EVENT-001, CR 614.5, 903.9b, SRS 8.1: with `overrides_614_5` the effect that replaced the
// parent may replace the `Instead` child too.
#[test]
fn req_event_001_override_effect_applies_again_to_instead_child() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    w.put_counters(obj, "charge", 1);
    let r = run_event(
        Bus::new(w, vec![tap_again_effect(obj).with_overrides_614_5(true)]),
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

// REQ-EVENT-001, CR 615.5 (the prevention's follow-up), CR 614.6 (the Skip), SRS 8.1: children
// of an event that is then dropped (here: a partial prevention with a follow-up, then a Skip)
// are still processed.
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

// REQ-EVENT-001, CR 903.9b, SRS 8.1: an overriding `Instead` chain that re-creates itself
// forever (one new matching event per level) is `DepthExceeded`; the registry and every world
// component are unchanged (child batches that executed are rolled back, a consumed shield is
// restored).
#[test]
fn req_event_001_endless_children_are_depth_exceeded() {
    let mut w = World::new();
    w.ok(life_change(P2, -1));
    let effects = vec![
        fx(
            1,
            life_matcher(P1),
            Action::Instead(vec![life_change(P1, 1), life_change(P2, -1)]),
        )
        .with_overrides_614_5(true),
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

// ---------------------------------------------------------------------------------------------
// review round 1 regressions
// ---------------------------------------------------------------------------------------------

/// A = Draw -> Instead[LifeChange +1] (id `a`), B = LifeChange -> Instead[Draw] (id `b`), both
/// for player 1, neither overriding CR 614.5.
fn draw_life_pair(a: u64, b: u64) -> Vec<ReplacementEffect> {
    vec![
        fx(
            a,
            draw_matcher(P1),
            Action::Instead(vec![life_change(P1, 1)]),
        ),
        fx(
            b,
            life_matcher(P1),
            Action::Instead(vec![GameEvent::Draw { player: P1 }]),
        ),
    ]
}

// REQ-EVENT-001, CR 614.5, 614.6 (F1): a Draw replaced by a life gain that is replaced by a Draw:
// the final Draw is a modified event of the original draw, A already had its one opportunity,
// so the Draw executes (one card drawn), applications [A, B], no DepthExceeded, and the
// executed Draw's applied set holds {A, B}. Both EffectId orders.
#[test]
fn req_event_001_r1_f1_instead_lineage_keeps_applied_effects() {
    for (a, b) in [(1u64, 2u64), (2, 1)] {
        let mut w = World::new();
        w.create(ZoneKind::Library, P1);
        w.create(ZoneKind::Library, P1);
        let hand_before = w.zones.len(fineprint_core::zones::ZoneId::Hand(P1));
        let r = run_event(
            Bus::new(w, draw_life_pair(a, b)),
            GameEvent::Draw { player: P1 },
            &[],
        );
        assert_eq!(r.applications(), eids(&[a, b]), "ids {a}, {b}");
        assert_eq!(
            r.world().zones.len(fineprint_core::zones::ZoneId::Hand(P1)),
            hand_before + 1
        );
        assert_eq!(r.world().life(P1), STARTING_LIFE);
        let executed = r.executed();
        assert_eq!(executed.len(), 1);
        assert_eq!(
            executed[0].instance.event(),
            &GameEvent::Draw { player: P1 }
        );
        let mut both = eids(&[a, b]);
        both.sort();
        assert_eq!(applied_of(&executed[0]), both);
    }
}

// REQ-EVENT-001, CR 614.5 (F1): applied history is per batch member: two Draws in one batch
// are each replaced by A (Draw -> Instead[LifeChange +1]).
#[test]
fn req_event_001_r1_f1_history_is_per_batch_member() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    w.create(ZoneKind::Library, P1);
    let effect = fx(
        1,
        draw_matcher(P1),
        Action::Instead(vec![life_change(P1, 1)]),
    );
    let r = run_batch(
        Bus::new(w, vec![effect]),
        vec![
            GameEvent::Draw { player: P1 },
            GameEvent::Draw { player: P1 },
        ],
        false,
        &[],
    );
    assert_eq!(r.applications(), eids(&[1, 1]));
    assert_eq!(r.world().life(P1), STARTING_LIFE + 2);
    assert!(
        r.world()
            .zones
            .is_empty(fineprint_core::zones::ZoneId::Hand(P1))
    );
    for e in r.executed() {
        assert_eq!(e.instance.event(), &life_change(P1, 1));
        assert_eq!(applied_of(&e), eids(&[1]));
    }
}

// REQ-EVENT-001, CR 615.5, 614.5 (F1): a `Prevent { then }` follow-up is a new event with an
// empty applied set, so the effect that prevented its parent can prevent it too.
#[test]
fn req_event_001_r1_f1_prevent_follow_up_has_fresh_applied_set() {
    let effect = fx(
        1,
        m_dmg_player(P2),
        Action::Prevent {
            then: vec![dmg_player(P2, 1)],
        },
    )
    .with_shield(Shield::Uses(2));
    let r = run_event(Bus::new(World::new(), vec![effect]), dmg_player(P2, 3), &[]);
    // Depth 0: 3 prevented, follow-up 1 damage. Depth 1: the follow-up is prevented (second
    // use; the effect is gone), follow-up 1 damage. Depth 2: dealt.
    assert_eq!(r.applications(), eids(&[1, 1]));
    assert_eq!(r.report().prevented, 4);
    assert_eq!(r.world().life(P2), STARTING_LIFE - 1);
    assert!(r.after.registry.is_empty());
    let executed = r.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].instance.event(), &dmg_player(P2, 1));
    assert!(applied_of(&executed[0]).is_empty());
}

/// A Skip effect (id `n`) on every BecomeCopy.
fn skip_become_copy(n: u64) -> ReplacementEffect {
    fx(n, EventMatcher::new(BusEventKind::BecomeCopy), Action::Skip)
}

// REQ-EVENT-001, CR 614.1c, 603.6d, 616.1c (F6): `EntersAsCopyOf` on a ZoneChange to the
// battlefield makes the permanent enter as the copy: a Skip effect on BecomeCopy is not applied
// and the new permanent is a copy; the copy is its own executed batch after the entry,
// carrying the new id.
#[test]
fn req_event_001_r1_f6_enters_as_copy_zone_change_ignores_become_copy_effects() {
    for (copy_id, skip_id) in [(1u64, 2u64), (2, 1)] {
        let mut w = World::new();
        let card = w.create(ZoneKind::Hand, P1);
        let original = w.create(ZoneKind::Battlefield, P2);
        let effects = vec![
            fx(
                copy_id,
                EventMatcher {
                    to: Some(ZoneKind::Battlefield),
                    ..m_zc_object(card)
                },
                Action::Modify(Modification::EntersAsCopyOf(original)),
            )
            .with_tier(Tier::CopyEtb),
            skip_become_copy(skip_id),
        ];
        let r = run_event(
            Bus::new(w, effects),
            zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other),
            &[],
        );
        assert_eq!(r.applications(), eids(&[copy_id]));
        let batches = r.nonempty_batches();
        assert_eq!(batches.len(), 2);
        let entered = new_id(&batches[0].events[0].outcome);
        assert_eq!(
            batches[1].events[0].instance.event(),
            &GameEvent::BecomeCopy {
                object: entered,
                copied: original
            }
        );
        assert_eq!(r.world().events.markers().copy_of(entered), Some(original));
        assert_eq!(r.world().zone_of(entered), Some(ZoneKind::Battlefield));
    }
}

// REQ-EVENT-001, CR 614.1c, 603.6d, 616.1c (F6): the same for a CreateToken.
#[test]
fn req_event_001_r1_f6_enters_as_copy_token_ignores_become_copy_effects() {
    let mut w = World::new();
    let original = w.create(ZoneKind::Battlefield, P1);
    let card = w.fresh_card();
    let effects = vec![
        fx(
            1,
            EventMatcher::new(BusEventKind::CreateToken),
            Action::Modify(Modification::EntersAsCopyOf(original)),
        ),
        skip_become_copy(2),
    ];
    let r = run_event(
        Bus::new(w, effects),
        GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        },
        &[],
    );
    assert_eq!(r.applications(), eids(&[1]));
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 2);
    let token = new_id(&batches[0].events[0].outcome);
    assert_eq!(
        batches[1].events[0].instance.event(),
        &GameEvent::BecomeCopy {
            object: token,
            copied: original
        }
    );
    assert_eq!(r.world().events.markers().copy_of(token), Some(original));
}

// REQ-EVENT-001, CR 614.6 (F6): a BecomeCopy submitted on its own through the bus is still
// subject to replacement: the Skip effect applies and nothing becomes a copy.
#[test]
fn req_event_001_r1_f6_standalone_become_copy_is_replaceable() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let original = w.create(ZoneKind::Battlefield, P2);
    let r = run_event(
        Bus::new(w, vec![skip_become_copy(1)]),
        GameEvent::BecomeCopy {
            object: obj,
            copied: original,
        },
        &[],
    );
    assert_eq!(r.applications(), eids(&[1]));
    assert!(r.executed().is_empty());
    assert!(r.new_log().is_empty());
    assert_eq!(r.world().events.markers().copy_of(obj), None);
}

// REQ-EVENT-001, CR 616.1c (F6), brief clause k: if the copy step fails (the copied object
// does not exist), the whole call is rolled back, the entry included.
#[test]
fn req_event_001_r1_f6_failing_copy_rolls_back_entry() {
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    let missing = oid(900);
    let effect = fx(
        1,
        EventMatcher {
            to: Some(ZoneKind::Battlefield),
            ..m_zc_object(card)
        },
        Action::Modify(Modification::EntersAsCopyOf(missing)),
    )
    .with_tier(Tier::CopyEtb)
    .with_shield(Shield::Uses(1));
    let r = run_event(
        Bus::new(w, vec![effect]),
        zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other),
        &[],
    );
    assert!(matches!(r.error(), BusError::Event(_)), "{:?}", r.error());
    assert_unchanged(&r);
    assert_eq!(r.world().zone_of(card), Some(ZoneKind::Hand));
}

// REQ-EVENT-001, CR 903.9b (F7): an overriding Draw -> Instead[Draw, Draw] doubles the events
// at every level; the shared work budget stops it with `WorkLimit` before memory runs out, and
// nothing changes.
#[test]
fn req_event_001_r1_f7_doubling_override_is_work_limit() {
    assert_eq!(fineprint_core::replacement::MAX_BUS_EVENTS, 4096);
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    let effect = fx(
        1,
        draw_matcher(P1),
        Action::Instead(vec![
            GameEvent::Draw { player: P1 },
            GameEvent::Draw { player: P1 },
        ]),
    )
    .with_overrides_614_5(true)
    .with_description("test.doubling_draw");
    let effects = vec![
        effect,
        fx(
            2,
            life_matcher(P2),
            Action::Modify(Modification::Amount(AmountOp::Add(1))),
        )
        .with_shield(Shield::Uses(1)),
    ];
    let r = run_batch(
        Bus::new(w, effects),
        vec![life_change(P2, -1), GameEvent::Draw { player: P1 }],
        false,
        &[],
    );
    assert_eq!(r.error(), &BusError::WorkLimit);
    assert_unchanged(&r);
    assert_eq!(r.after.canonical(), r.before.canonical());
}

// REQ-EVENT-001 (F7): the budget counts the submitted members: exactly `MAX_BUS_EVENTS` plain
// events with no effects succeed; one more is `WorkLimit` with nothing changed.
#[test]
fn req_event_001_r1_f7_budget_boundary() {
    use fineprint_core::replacement::MAX_BUS_EVENTS;
    let ok = run_batch(
        Bus::new(World::new(), Vec::new()),
        vec![life_change(P1, 1); MAX_BUS_EVENTS],
        false,
        &[],
    );
    assert_eq!(ok.executed().len(), MAX_BUS_EVENTS);
    assert_eq!(
        ok.world().life(P1),
        STARTING_LIFE + i64::try_from(MAX_BUS_EVENTS).expect("fits")
    );
    let over = run_batch(
        Bus::new(World::new(), Vec::new()),
        vec![life_change(P1, 1); MAX_BUS_EVENTS + 1],
        false,
        &[],
    );
    assert_eq!(over.error(), &BusError::WorkLimit);
    assert_unchanged(&over);
}

// REQ-EVENT-001 (F7): children count against the same budget: `MAX_BUS_EVENTS - 1` plain
// events plus one Draw replaced by one child is `MAX_BUS_EVENTS + 1` events: `WorkLimit`; with
// one plain event fewer it succeeds.
#[test]
fn req_event_001_r1_f7_budget_counts_children() {
    use fineprint_core::replacement::MAX_BUS_EVENTS;
    for (plain, fits) in [(MAX_BUS_EVENTS - 1, false), (MAX_BUS_EVENTS - 2, true)] {
        let effect = fx(
            1,
            draw_matcher(P2),
            Action::Instead(vec![life_change(P2, 1)]),
        );
        let mut events = vec![life_change(P1, 1); plain];
        events.push(GameEvent::Draw { player: P2 });
        let r = run_batch(Bus::new(World::new(), vec![effect]), events, false, &[]);
        if fits {
            assert_eq!(r.applications(), eids(&[1]));
            assert_eq!(r.world().life(P2), STARTING_LIFE + 1);
        } else {
            assert_eq!(r.error(), &BusError::WorkLimit);
            assert_unchanged(&r);
        }
    }
}
