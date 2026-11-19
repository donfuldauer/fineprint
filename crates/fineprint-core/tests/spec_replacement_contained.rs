// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for contained events in the replacement bus (REQ-EVENT-001; CR 616.1g, 616.1).
//! A contained effect (one on an event contained in the processed event, such as "enters"
//! inside "create a token") is not choosable while a non-contained effect applies; this filter
//! comes before the tier restriction. Black-box: written from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::GameEvent;
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, Tier,
};
use fineprint_core::zones::ZoneKind;
use support::*;

/// A world with two battlefield objects to copy; returns the world and the objects.
fn world() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    (w, a, b)
}

/// A CreateToken event for a token owned by player 1.
fn token(w: &mut World) -> GameEvent {
    GameEvent::CreateToken {
        owner: P1,
        card: w.fresh_card(),
        controller: None,
    }
}

fn token_matcher() -> EventMatcher {
    EventMatcher {
        affected_player: Some(P1),
        ..EventMatcher::new(BusEventKind::CreateToken)
    }
}

/// A non-contained "it's created under player 2's control" effect.
fn control(n: u64) -> ReplacementEffect {
    fx(
        n,
        token_matcher(),
        Action::Modify(Modification::SetController(P2)),
    )
}

/// A contained "it enters as a copy of `of`" effect of `tier`.
fn contained_copy(n: u64, of: ObjectId, tier: Tier) -> ReplacementEffect {
    fx(
        n,
        token_matcher(),
        Action::Modify(Modification::EntersAsCopyOf(of)),
    )
    .with_contained(true)
    .with_tier(tier)
}

// REQ-EVENT-001, SRS regression "the 616.1g nested event", CR 616.1g: on a CreateToken event, a
// contained effect is applied only after the non-contained effect, whatever the id order and
// even when the contained effect is in a higher tier (the contained filter comes first).
#[test]
fn req_event_001_regression_contained_after_non_contained() {
    for tier in [Tier::Other, Tier::CopyEtb, Tier::SelfReplacement] {
        for (n, c) in [(1u64, 2u64), (2, 1)] {
            let (mut w, a, _) = world();
            let ev = token(&mut w);
            let bus = Bus::new(w, vec![control(n), contained_copy(c, a, tier)]);
            let r = run_event(bus, ev, &[]);
            assert_eq!(
                r.applications(),
                eids(&[n, c]),
                "tier {tier:?}, ids {n}/{c}"
            );
            let created = new_id(&r.nonempty_batches()[0].events[0].outcome);
            let w = r.world();
            assert_eq!(w.zones.object(created).expect("present").controller, P2);
            assert_eq!(w.events.markers().copy_of(created), Some(a));
        }
    }
}

// REQ-EVENT-001, CR 616.1g: while non-contained effects apply, the request offers only them;
// the contained effect is applied after both non-contained effects. Every id order.
#[test]
fn req_event_001_contained_not_offered_while_non_contained_applies() {
    for ids in [[1u64, 2, 3], [3, 2, 1], [1, 3, 2], [2, 1, 3]] {
        let [n1, n2, c] = ids;
        let (mut w, a, _) = world();
        let ev = token(&mut w);
        let bus = Bus::new(
            w,
            vec![control(n1), control(n2), contained_copy(c, a, Tier::Other)],
        );
        let lo = n1.min(n2);
        let hi = n1.max(n2);
        let r = run_event(bus, ev, &[0]);
        assert_eq!(r.offered(0), descs(&[lo, hi]), "ids {ids:?}");
        assert_eq!(r.applications(), eids(&[lo, hi, c]), "ids {ids:?}");
    }
}

// REQ-EVENT-001, CR 616.1g: a contained effect alone is applied at once, without a request.
#[test]
fn req_event_001_contained_alone_applies_immediately() {
    let (mut w, a, _) = world();
    let ev = token(&mut w);
    let bus = Bus::new(w, vec![contained_copy(4, a, Tier::Other)]);
    let r = run_event(bus, ev, &[]);
    assert_eq!(r.applications(), eids(&[4]));
    let created = new_id(&r.nonempty_batches()[0].events[0].outcome);
    assert_eq!(r.world().events.markers().copy_of(created), Some(a));
}

// REQ-EVENT-001, CR 616.1g, 616.1: two contained effects and one non-contained effect: the
// non-contained one applies first without a request, then the two contained effects are
// offered together (ascending id), asked of the token's controller after the change.
#[test]
fn req_event_001_two_contained_offered_together_after_non_contained() {
    for ids in [[1u64, 2, 3], [3, 1, 2], [2, 3, 1]] {
        let [n, c1, c2] = ids;
        let (mut w, a, b) = world();
        let ev = token(&mut w);
        let bus = Bus::new(
            w,
            vec![
                control(n),
                contained_copy(c1, a, Tier::Other),
                contained_copy(c2, b, Tier::Other),
            ],
        );
        let lo = c1.min(c2);
        let hi = c1.max(c2);
        let r = run_event(bus, ev, &[1]);
        assert_eq!(r.offered(0), descs(&[lo, hi]), "ids {ids:?}");
        // CR 616.1: the chooser of a CreateToken is its controller once one is set.
        assert_eq!(r.requests[0].chooser, P2);
        assert_eq!(r.applications(), eids(&[n, hi, lo]), "ids {ids:?}");
    }
}

// REQ-EVENT-001, CR 616.1g, 616.1 (amendment): when every effect of the best tier is contained
// and lower-tier non-contained effects apply, the non-contained effects are offered first
// (never an empty choice).
#[test]
fn req_event_001_contained_best_tier_does_not_block_lower_non_contained() {
    // One lower-tier non-contained effect: applied without a request.
    for (n, c1, c2) in [(1u64, 2u64, 3u64), (3, 1, 2)] {
        let (mut w, a, b) = world();
        let ev = token(&mut w);
        let bus = Bus::new(
            w,
            vec![
                control(n),
                contained_copy(c1, a, Tier::CopyEtb),
                contained_copy(c2, b, Tier::CopyEtb),
            ],
        );
        let r = run_event(bus, ev, &[0]);
        assert_eq!(r.offered(0), descs(&[c1.min(c2), c1.max(c2)]));
        assert_eq!(r.applications()[0], eid(n));
        assert_eq!(r.applications().len(), 3);
    }
    // Two lower-tier non-contained effects: the first request offers exactly those two.
    for ids in [[1u64, 2, 3, 4], [4, 3, 2, 1], [1, 3, 2, 4]] {
        let [n1, n2, c1, c2] = ids;
        let (mut w, a, b) = world();
        let ev = token(&mut w);
        let bus = Bus::new(
            w,
            vec![
                control(n1),
                control(n2),
                contained_copy(c1, a, Tier::CopyEtb),
                contained_copy(c2, b, Tier::CopyEtb),
            ],
        );
        let r = run_event(bus, ev, &[0, 0]);
        assert_eq!(
            r.offered(0),
            descs(&[n1.min(n2), n1.max(n2)]),
            "ids {ids:?}"
        );
        assert_eq!(
            r.offered(1),
            descs(&[c1.min(c2), c1.max(c2)]),
            "ids {ids:?}"
        );
        assert_eq!(
            r.applications(),
            eids(&[n1.min(n2), n1.max(n2), c1.min(c2), c1.max(c2)]),
            "ids {ids:?}"
        );
    }
}
