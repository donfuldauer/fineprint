// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the 616.1 precedence tiers of the replacement bus and the `tier_from_ability`
//! mapping (REQ-EVENT-001, REQ-DSL-009; CR 616.1, 616.1a-e, 616.2, 614.15). Black-box: written
//! from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, AmountOp, BusEventKind, EventMatcher, Modification, ReplacementEffect, Tier,
    tier_from_ability,
};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::ability::TierHint;
use support::*;

/// Every tier in precedence order (first is highest).
const TIERS: [Tier; 5] = [
    Tier::SelfReplacement,
    Tier::ControlEtb,
    Tier::CopyEtb,
    Tier::BackFaceEtb,
    Tier::Other,
];

/// The 10 (higher, lower) tier pairs.
fn pairs() -> Vec<(Tier, Tier)> {
    let mut out = Vec::new();
    for (i, &hi) in TIERS.iter().enumerate() {
        for &lo in &TIERS[i + 1..] {
            out.push((hi, lo));
        }
    }
    out
}

/// A damage-to-player-2 effect of `tier` that adds `add`.
fn add_fx(n: u64, tier: Tier, add: i64) -> ReplacementEffect {
    fx(
        n,
        m_dmg_player(P2),
        Action::Modify(Modification::Amount(AmountOp::Add(add))),
    )
    .with_tier(tier)
}

// REQ-EVENT-001, CR 616.1: the derive order of `Tier` is the precedence order.
#[test]
fn req_event_001_tier_order_is_precedence() {
    for w in TIERS.windows(2) {
        assert!(w[0] < w[1], "{:?} must precede {:?}", w[0], w[1]);
    }
}

// REQ-EVENT-001, CR 616.1a-e: with one effect in a higher tier and one in a lower tier, the
// higher one applies first, without a request (each is alone in its tier), then the lower one;
// both EffectId orders.
#[test]
fn req_event_001_each_tier_beats_every_later_tier_single() {
    for (hi, lo) in pairs() {
        for (hi_id, lo_id) in [(1, 2), (2, 1)] {
            let bus = Bus::new(
                World::new(),
                vec![add_fx(hi_id, hi, 1), add_fx(lo_id, lo, 2)],
            );
            let r = run_event(bus, dmg_player(P2, 1), &[]);
            assert_eq!(
                r.applications(),
                eids(&[hi_id, lo_id]),
                "{hi:?} (id {hi_id}) must apply before {lo:?} (id {lo_id})"
            );
            assert_eq!(r.world().life(P2), STARTING_LIFE - 4);
        }
    }
}

// REQ-EVENT-001, CR 616.1, 616.1a-e: with two effects in each of two tiers, the request lists
// only the higher tier's descriptions (ascending EffectId); after the answer the remaining
// higher-tier effect applies alone without a request; then the lower tier is offered. Every pair,
// three id orders.
#[test]
fn req_event_001_each_tier_beats_every_later_tier_with_requests() {
    let orders: [([u64; 2], [u64; 2]); 3] = [([1, 2], [3, 4]), ([3, 4], [1, 2]), ([1, 3], [2, 4])];
    for (hi, lo) in pairs() {
        for (his, los) in orders {
            let effects = vec![
                add_fx(his[0], hi, 1),
                add_fx(his[1], hi, 1),
                add_fx(los[0], lo, 1),
                add_fx(los[1], lo, 1),
            ];
            let bus = Bus::new(World::new(), effects);
            // Pick index 0 in the first request, index 1 in the second.
            let r = run_batch(bus, vec![dmg_player(P2, 1)], false, &[0, 1]);
            assert_eq!(r.offered(0), descs(&his), "{hi:?} over {lo:?}");
            assert_eq!(r.offered(1), descs(&los), "{hi:?} over {lo:?}");
            assert_eq!(
                r.applications(),
                eids(&[his[0], his[1], los[1], los[0]]),
                "{hi:?} over {lo:?}, ids {his:?} / {los:?}"
            );
            assert_eq!(r.world().life(P2), STARTING_LIFE - 5);
        }
    }
}

// REQ-EVENT-001, CR 616.1e: one applicable effect is applied without a request.
#[test]
fn req_event_001_single_effect_applies_without_request() {
    for tier in TIERS {
        let bus = Bus::new(World::new(), vec![add_fx(7, tier, 2)]);
        let r = run_event(bus, dmg_player(P2, 1), &[]);
        assert_eq!(r.applications(), eids(&[7]));
        assert_eq!(r.world().life(P2), STARTING_LIFE - 3);
    }
}

// REQ-EVENT-001, CR 616.1f, 616.2, 616.1a: after an application, collection restarts on the
// modified event and a higher-tier effect that newly applies is considered before a lower-tier
// one that also newly applies (so no request is needed).
#[test]
fn req_event_001_newly_applicable_higher_tier_first() {
    for order in [[1u64, 2, 3], [3, 2, 1], [2, 3, 1]] {
        let [first, selfr, other] = order;
        let mut w = World::new();
        let obj = w.create(ZoneKind::Battlefield, P1);
        let to = |z: ZoneKind| EventMatcher {
            to: Some(z),
            ..m_zc_object(obj)
        };
        let effects = vec![
            fx(
                first,
                to(ZoneKind::Graveyard),
                Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
            ),
            fx(
                selfr,
                to(ZoneKind::Exile),
                Action::Modify(Modification::SetDestination(ZoneKind::Hand)),
            )
            .with_tier(Tier::SelfReplacement),
            fx(
                other,
                to(ZoneKind::Exile),
                Action::Modify(Modification::SetDestination(ZoneKind::Library)),
            ),
        ];
        let bus = Bus::new(w, effects);
        let r = run_event(
            bus,
            zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            &[],
        );
        assert_eq!(r.applications(), eids(&[first, selfr]));
        let ev = &r.executed()[0];
        assert_eq!(r.world().zone_of(new_id(&ev.outcome)), Some(ZoneKind::Hand));
    }
}

// REQ-DSL-009, CR 616.1a-e, 614.15: `tier_from_ability` maps the DSL fields to a tier;
// `Some(true)` (self-replacement) wins over any hint.
#[test]
fn req_dsl_009_tier_from_ability_table() {
    let hints: [(Option<TierHint>, Tier); 4] = [
        (None, Tier::Other),
        (Some(TierHint::ControlEtb), Tier::ControlEtb),
        (Some(TierHint::CopyEtb), Tier::CopyEtb),
        (Some(TierHint::BackFaceEtb), Tier::BackFaceEtb),
    ];
    for (hint, expected) in hints {
        assert_eq!(tier_from_ability(None, hint), expected, "None, {hint:?}");
        assert_eq!(
            tier_from_ability(Some(false), hint),
            expected,
            "Some(false), {hint:?}"
        );
        assert_eq!(
            tier_from_ability(Some(true), hint),
            Tier::SelfReplacement,
            "Some(true), {hint:?}"
        );
    }
}

/// A hand-to-battlefield world: the entering object (owned by player 1) and a battlefield
/// object to copy.
fn etb_world() -> (World, Pair) {
    let mut w = World::new();
    let entering = w.create(ZoneKind::Hand, P1);
    let original = w.create(ZoneKind::Battlefield, P1);
    (w, Pair { entering, original })
}

struct Pair {
    entering: ObjectId,
    original: ObjectId,
}

fn etb_matcher(obj: ObjectId) -> EventMatcher {
    EventMatcher {
        to: Some(ZoneKind::Battlefield),
        ..m_zc_object(obj)
    }
}

// REQ-EVENT-001, SRS regression "a 616.1 tier test (copy ETB versus control ETB)",
// CR 616.1b, 616.1c: the control-ETB effect applies first, then the copy-ETB effect; the object
// enters under the new controller and, after the child BecomeCopy, is a copy. Both id orders.
#[test]
fn req_event_001_regression_control_etb_before_copy_etb() {
    for (control, copy) in [(1u64, 2u64), (2, 1)] {
        let (w, p) = etb_world();
        let effects = vec![
            fx(
                control,
                etb_matcher(p.entering),
                Action::Modify(Modification::SetController(P2)),
            )
            .with_tier(Tier::ControlEtb),
            fx(
                copy,
                etb_matcher(p.entering),
                Action::Modify(Modification::EntersAsCopyOf(p.original)),
            )
            .with_tier(Tier::CopyEtb),
        ];
        let bus = Bus::new(w, effects);
        let r = run_event(
            bus,
            zone_change(p.entering, ZoneKind::Battlefield, ZoneChangeCause::Other),
            &[],
        );
        assert_eq!(r.applications(), eids(&[control, copy]));
        let batches = r.nonempty_batches();
        assert_eq!(
            batches.len(),
            2,
            "the entering batch, then the BecomeCopy batch"
        );
        let entered = new_id(&batches[0].events[0].outcome);
        let w = r.world();
        assert_eq!(w.zone_of(entered), Some(ZoneKind::Battlefield));
        assert_eq!(w.zones.object(entered).expect("present").controller, P2);
        assert_eq!(
            w.permanents.get(entered).expect("permanent").controller(),
            P2
        );
        assert_eq!(
            batches[1].events[0].instance.event(),
            &GameEvent::BecomeCopy {
                object: entered,
                copied: p.original
            }
        );
        assert_eq!(w.events.markers().copy_of(entered), Some(p.original));
    }
}

// REQ-EVENT-001, CR 616.1b, 616.1c: with two control-ETB effects and one copy-ETB effect, the
// control tier is asked first (copy not offered), then the copy effect applies.
#[test]
fn req_event_001_regression_control_etb_asked_before_copy_etb() {
    for ids in [[1u64, 2, 3], [3, 2, 1], [1, 3, 2]] {
        let [c1, c2, copy] = ids;
        let (w, p) = etb_world();
        let effects = vec![
            fx(
                c1,
                etb_matcher(p.entering),
                Action::Modify(Modification::SetController(P2)),
            )
            .with_tier(Tier::ControlEtb),
            fx(
                c2,
                etb_matcher(p.entering),
                Action::Modify(Modification::SetController(P2)),
            )
            .with_tier(Tier::ControlEtb),
            fx(
                copy,
                etb_matcher(p.entering),
                Action::Modify(Modification::EntersAsCopyOf(p.original)),
            )
            .with_tier(Tier::CopyEtb),
        ];
        let bus = Bus::new(w, effects);
        let lo = c1.min(c2);
        let hi = c1.max(c2);
        let r = run_preferring(
            bus,
            Call::Event(zone_change(
                p.entering,
                ZoneKind::Battlefield,
                ZoneChangeCause::Other,
            )),
            &[hi],
        );
        assert_eq!(r.requests.len(), 1);
        assert_eq!(r.offered(0), descs(&[lo, hi]));
        assert_eq!(r.applications(), eids(&[hi, lo, copy]));
        let entered = new_id(&r.nonempty_batches()[0].events[0].outcome);
        assert_eq!(
            r.world().zones.object(entered).expect("present").controller,
            P2
        );
        assert_eq!(
            r.world().events.markers().copy_of(entered),
            Some(p.original)
        );
    }
}

// REQ-EVENT-001, CR 616.1: the request kind and names are those of the bus event kind.
#[test]
fn req_event_001_tier_request_names_damage() {
    let bus = Bus::new(
        World::new(),
        vec![add_fx(1, Tier::Other, 1), add_fx(2, Tier::Other, 1)],
    );
    let r = run_event(bus, dmg_player(P2, 1), &[0]);
    assert_eq!(payload(&r.requests[0]).0, BusEventKind::Damage.name());
    assert_eq!(payload(&r.requests[0]).0, "DAMAGE");
}
