// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the applied-effects set of the replacement bus and its per-effect exception
//! (REQ-EVENT-001; CR 614.5, 903.9b, 616.1f). Black-box: written from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_core::events::{EventBatch, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, AmountOp, BusError, EventMatcher, MAX_REPEATS, Modification, ReplacementEffect,
};
use fineprint_core::zones::ZoneKind;
use support::*;

fn add_one(n: u64, below: Option<i64>, overrides: bool) -> ReplacementEffect {
    fx(
        n,
        EventMatcher {
            amount_below: below,
            ..m_dmg_player(P2)
        },
        Action::Modify(Modification::Amount(AmountOp::Add(1))),
    )
    .with_overrides_614_5(overrides)
}

// REQ-EVENT-001, CR 614.5: an effect that keeps matching applies only once to the same event.
#[test]
fn req_event_001_effect_never_applies_twice() {
    let bus = Bus::new(World::new(), vec![add_one(3, None, false)]);
    let r = run_event(bus, dmg_player(P2, 1), &[]);
    assert_eq!(r.applications(), eids(&[3]));
    assert_eq!(r.world().life(P2), STARTING_LIFE - 2);
    assert_eq!(applied_of(&r.executed()[0]), eids(&[3]));
}

// REQ-EVENT-001, CR 614.5: the executed event's applied set lists every applied effect,
// ascending, whatever order they were applied in.
#[test]
fn req_event_001_applied_set_lists_every_effect_ascending() {
    let bus = Bus::new(
        World::new(),
        vec![
            add_one(3, None, false),
            add_one(1, None, false),
            add_one(2, None, false),
        ],
    );
    // Always pick the last (highest-id) option: applied 3, then 2, then 1 (alone).
    let r = drive(bus, Call::Event(dmg_player(P2, 1)), |_, req| {
        u32::try_from(option_count(req) - 1).expect("fits")
    });
    assert_eq!(r.requests.len(), 2);
    assert_eq!(r.applications(), eids(&[3, 2, 1]));
    let executed = r.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(applied_of(&executed[0]), eids(&[1, 2, 3]));
    assert_eq!(r.world().life(P2), STARTING_LIFE - 4);
}

// REQ-EVENT-001, CR 614.5: an effect already in the event's applied set does not apply.
#[test]
fn req_event_001_effect_in_incoming_applied_set_is_excluded() {
    let bus = Bus::new(World::new(), vec![add_one(5, None, false)]);
    let batch = EventBatch::new(vec![instance_with(dmg_player(P2, 1), &[5])]);
    let r = run(bus, Call::Batch(batch, false), &[]);
    assert!(r.applications().is_empty());
    assert_eq!(r.world().life(P2), STARTING_LIFE - 1);
    assert!(applied_of(&r.executed()[0]).contains(&eid(5)));
}

// REQ-EVENT-001, CR 903.9b, 614.5: an effect with the override flag applies again and again
// while it matches (1 -> 5 damage, four applications); without the flag it applies once.
#[test]
fn req_event_001_override_applies_until_matcher_fails() {
    let bus = Bus::new(World::new(), vec![add_one(4, Some(5), true)]);
    let r = run_event(bus, dmg_player(P2, 1), &[]);
    assert_eq!(r.applications(), eids(&[4, 4, 4, 4]));
    assert_eq!(r.world().life(P2), STARTING_LIFE - 5);
    assert_eq!(applied_of(&r.executed()[0]), eids(&[4]));

    let bus = Bus::new(World::new(), vec![add_one(4, Some(5), false)]);
    let r = run_event(bus, dmg_player(P2, 1), &[]);
    assert_eq!(r.applications(), eids(&[4]));
    assert_eq!(r.world().life(P2), STARTING_LIFE - 2);
}

// REQ-EVENT-001, CR 903.9b, 614.5: an override effect that always matches exceeds
// MAX_REPEATS: `RepeatLimit`, and the registry and world are unchanged.
#[test]
fn req_event_001_unbounded_override_is_repeat_limit() {
    let mut w = World::new();
    // Earlier history, so "unchanged" covers a non-empty log.
    w.ok(life_change(P1, -1));
    // A shield on another effect that applies first, so the registry would change.
    let spent = fx(
        1,
        m_dmg_player(P2),
        Action::Modify(Modification::Amount(AmountOp::Add(1))),
    )
    .with_tier(fineprint_core::replacement::Tier::SelfReplacement)
    .with_shield(fineprint_core::replacement::Shield::Uses(1));
    let bus = Bus::new(w, vec![spent, add_one(2, None, true)]);
    let r = run_batch(bus, vec![dmg_player(P2, 1), life_change(P1, 2)], false, &[]);
    assert_eq!(r.error(), &BusError::RepeatLimit);
    assert_unchanged(&r);
    assert_eq!(MAX_REPEATS, 256);
}

// REQ-EVENT-001, SRS regression "903.9b repeated application", CR 903.9b, 614.5, 616.1f: a
// ZoneChange override effect (to hand -> command zone) applies again after another effect
// sends the object back toward the hand; the non-override effect does not apply twice.
#[test]
fn req_event_001_regression_903_9b_repeated_zone_change() {
    for (ov, other) in [(1u64, 2u64), (2, 1)] {
        let mut w = World::new();
        let obj = w.create(ZoneKind::Battlefield, P1);
        let to = |z: ZoneKind| EventMatcher {
            to: Some(z),
            ..m_zc_object(obj)
        };
        let effects = vec![
            fx(
                ov,
                to(ZoneKind::Hand),
                Action::Modify(Modification::SetDestination(ZoneKind::Command)),
            )
            .with_overrides_614_5(true),
            fx(
                other,
                to(ZoneKind::Command),
                Action::Modify(Modification::SetDestination(ZoneKind::Hand)),
            ),
        ];
        let bus = Bus::new(w.clone(), effects.clone());
        let r = run_event(
            bus,
            zone_change(obj, ZoneKind::Hand, ZoneChangeCause::Other),
            &[],
        );
        assert_eq!(r.applications(), eids(&[ov, other, ov]));
        let ev = &r.executed()[0];
        assert_eq!(
            r.world().zone_of(new_id(&ev.outcome)),
            Some(ZoneKind::Command)
        );
        let mut both = eids(&[ov, other]);
        both.sort();
        assert_eq!(applied_of(ev), both);

        // Without the flag: applied once each, and the object ends in the hand.
        let mut plain = effects;
        plain[0] = plain[0].clone().with_overrides_614_5(false);
        let r = run_event(
            Bus::new(w, plain),
            zone_change(obj, ZoneKind::Hand, ZoneChangeCause::Other),
            &[],
        );
        assert_eq!(r.applications(), eids(&[ov, other]));
        let ev = &r.executed()[0];
        assert_eq!(r.world().zone_of(new_id(&ev.outcome)), Some(ZoneKind::Hand));
    }
}

// REQ-EVENT-001, SRS regression "903.9b repeated application", CR 903.9b: a ZoneChange override
// effect that keeps matching forever is `RepeatLimit`, with nothing changed.
#[test]
fn req_event_001_regression_903_9b_zone_change_keeps_matching() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let effect = fx(
        1,
        m_zc_object(obj),
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
    )
    .with_overrides_614_5(true);
    let r = run_event(
        Bus::new(w, vec![effect]),
        zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        &[],
    );
    assert_eq!(r.error(), &BusError::RepeatLimit);
    assert_unchanged(&r);
}
