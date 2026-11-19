// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for same-result replacement merging of state-based-action batches
//! (REQ-EVENT-001; CR 704.3, 704.7, 616.1). With `sba = true`, only ZoneChange members merge,
//! and only when they affect the SAME object with the same destination (the same result): such
//! a group makes one choice, one application, consumes one shield use and executes as one
//! event. Members for different objects never merge, whatever their kind, candidate set or
//! chooser: each gets its own choice and application, in APNAP then submission order. Coalescing
//! of same-object, same-destination moves does not depend on submission order (review round 1
//! finding F2, round 2).
//! Black-box: written from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield,
};
use fineprint_core::zones::ZoneKind;
use support::*;

fn dies() -> EventMatcher {
    EventMatcher {
        from: Some(ZoneKind::Battlefield),
        to: Some(ZoneKind::Graveyard),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

fn redirect(n: u64, to: ZoneKind) -> ReplacementEffect {
    fx(n, dies(), Action::Modify(Modification::SetDestination(to)))
}

/// A world with battlefield objects controlled by the given players (owned by player 1).
fn world(controllers: &[PlayerId]) -> (World, Vec<ObjectId>) {
    let mut w = World::new();
    let objs = controllers
        .iter()
        .map(|&c| w.create_controlled(ZoneKind::Battlefield, P1, Some(c)))
        .collect();
    (w, objs)
}

fn sba_deaths(objs: &[ObjectId]) -> Vec<GameEvent> {
    objs.iter()
        .map(|&o| zone_change(o, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction))
        .collect()
}

/// The zone kind each executed ZoneChange moved its object to, in execution order.
fn destinations(r: &Run) -> Vec<ZoneKind> {
    r.executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::ZoneChange { .. }))
        .map(|e| r.world().zone_of(new_id(&e.outcome)).expect("in a zone"))
        .collect()
}

// REQ-EVENT-001, CR 704.7, 616.1 (F5): three DIFFERENT permanents dying to SBAs with the same
// candidate set and the same chooser are distinct results: three requests, one per object,
// three applications, each answer applied to its own object. Both id orders, both answers.
#[test]
fn req_event_001_sba_same_result_merges_into_one_choice() {
    for (exile, hand) in [(1u64, 2u64), (2, 1)] {
        for pick in [exile, hand] {
            let (w, objs) = world(&[P1, P1, P1]);
            let bus = Bus::new(
                w,
                vec![
                    redirect(exile, ZoneKind::Exile),
                    redirect(hand, ZoneKind::Hand),
                ],
            );
            let r = drive(bus, Call::batch(sba_deaths(&objs), true), |_, req| {
                index_of(req, pick)
            });
            assert_eq!(r.requests.len(), 3, "one request per object");
            for (i, req) in r.requests.iter().enumerate() {
                assert_eq!(req.chooser, P1);
                assert_eq!(r.offered(i), descs(&[1, 2]));
                // Submission order inside one chooser.
                assert_eq!(req.reason.source, Some(objs[i]));
            }
            assert_eq!(r.applications(), eids(&[pick, pick, pick]));
            let to = if pick == exile {
                ZoneKind::Exile
            } else {
                ZoneKind::Hand
            };
            assert_eq!(destinations(&r), vec![to; 3]);
            for e in r.executed() {
                assert_eq!(applied_of(&e), eids(&[pick]));
            }
        }
    }
}

// REQ-EVENT-001, CR 704.7 (F5): a `Uses(1)` shield is consumed by the first of three different
// permanents (APNAP then submission order); the other two are not covered by it.
#[test]
fn req_event_001_sba_merge_consumes_one_shield_use() {
    let (w, objs) = world(&[P1, P1, P1]);
    let bus = Bus::new(
        w,
        vec![
            redirect(1, ZoneKind::Exile).with_shield(Shield::Uses(1)),
            redirect(2, ZoneKind::Hand),
        ],
    );
    // First object: both effects apply, pick the shielded one. Then only effect 2 is left
    // and applies without a choice.
    let r = run_batch(bus, sba_deaths(&objs), true, &[0]);
    assert_eq!(r.requests[0].reason.source, Some(objs[0]));
    assert_eq!(r.applications(), eids(&[1, 2, 2]));
    assert_eq!(
        destinations(&r),
        vec![ZoneKind::Exile, ZoneKind::Hand, ZoneKind::Hand]
    );
    assert!(r.after.registry.get(eid(1)).is_none());
    assert!(r.after.registry.get(eid(2)).is_some());
}

// REQ-EVENT-001, CR 704.7, 616.1: with `sba = false` there is no merging: three requests,
// three applications, each answer applied to its own event.
#[test]
fn req_event_001_no_merge_without_sba() {
    let (w, objs) = world(&[P1, P1, P1]);
    let bus = Bus::new(
        w,
        vec![redirect(1, ZoneKind::Exile), redirect(2, ZoneKind::Hand)],
    );
    let r = run_batch(bus, sba_deaths(&objs), false, &[0, 1, 0]);
    assert_eq!(r.applications(), eids(&[1, 2, 1]));
    assert_eq!(
        destinations(&r),
        vec![ZoneKind::Exile, ZoneKind::Hand, ZoneKind::Exile]
    );
}

// REQ-EVENT-001, CR 704.7, 616.1: different objects never merge (here they also have different
// choosers): two requests, player 1 (active) first (APNAP), each answer applied to its own
// event. Both submission orders.
#[test]
fn req_event_001_sba_different_choosers_do_not_merge() {
    for controllers in [[P1, P2], [P2, P1]] {
        let (w, objs) = world(&controllers);
        let bus = Bus::new(
            w,
            vec![redirect(1, ZoneKind::Exile), redirect(2, ZoneKind::Hand)],
        );
        // Player 1 picks exile, player 2 picks hand.
        let r = drive(bus, Call::batch(sba_deaths(&objs), true), |_, req| {
            if req.chooser == P1 { 0 } else { 1 }
        });
        assert_eq!(r.requests.len(), 2);
        assert_eq!(r.requests[0].chooser, P1);
        assert_eq!(r.requests[1].chooser, P2);
        assert_eq!(r.applications(), eids(&[1, 2]));
        let executed = r.executed();
        assert_eq!(executed.len(), 2);
        for (i, e) in executed.iter().enumerate() {
            // Execution is in submission order.
            assert_eq!(
                e.instance.event(),
                &zone_change(
                    objs[i],
                    ZoneKind::Graveyard,
                    ZoneChangeCause::StateBasedAction
                )
                .with_dest_for(controllers[i])
            );
        }
    }
}

trait WithDest {
    fn with_dest_for(self, chooser: PlayerId) -> GameEvent;
}

impl WithDest for GameEvent {
    /// The expected final event of the different-choosers test: exile for player 1's object,
    /// hand for player 2's.
    fn with_dest_for(self, chooser: PlayerId) -> GameEvent {
        match self {
            GameEvent::ZoneChange {
                object,
                mut request,
                cause,
            } => {
                request.to = if chooser == P1 {
                    ZoneKind::Exile
                } else {
                    ZoneKind::Hand
                };
                GameEvent::ZoneChange {
                    object,
                    request,
                    cause,
                }
            }
            other => other,
        }
    }
}

// REQ-EVENT-001, CR 704.7, 616.1 (F5): in an SBA batch, members for different objects and
// non-ZoneChange members never merge: one request per object and one per damage event.
#[test]
fn req_event_001_sba_different_kind_or_candidates_do_not_merge() {
    let (w, objs) = world(&[P1, P1, P1]);
    let only_third = fx(
        3,
        EventMatcher {
            affected_object: Some(objs[2]),
            ..dies()
        },
        Action::Modify(Modification::SetDestination(ZoneKind::Library)),
    );
    let prevent = |n| fx(n, m_dmg_player(P1), Action::Prevent { then: Vec::new() });
    let bus = Bus::new(
        w,
        vec![
            redirect(1, ZoneKind::Exile),
            redirect(2, ZoneKind::Hand),
            only_third,
            prevent(4),
            prevent(5),
        ],
    );
    let mut events = sba_deaths(&objs);
    events.push(dmg_player(P1, 2));
    events.push(dmg_player(P1, 3));
    let r = run_preferring(bus, Call::batch(events, true), &[1, 4]);
    assert_eq!(r.requests.len(), 5);
    let mut offered: Vec<Vec<String>> = (0..5).map(|i| r.offered(i)).collect();
    offered.sort();
    let mut expected = vec![
        descs(&[1, 2]),
        descs(&[1, 2]),
        descs(&[1, 2, 3]),
        descs(&[4, 5]),
        descs(&[4, 5]),
    ];
    expected.sort();
    assert_eq!(offered, expected);
    let mut apps = r.applications();
    apps.sort();
    assert_eq!(apps, eids(&[1, 1, 1, 4, 4]));
    assert_eq!(r.report().prevented, 5);
    assert_eq!(r.world().life(P1), STARTING_LIFE);
    assert_eq!(destinations(&r), vec![ZoneKind::Exile; 3]);
}

// ---------------------------------------------------------------------------------------------
// review round 1 regressions
// ---------------------------------------------------------------------------------------------

/// Two identical SBA deaths of `obj` (e.g. lethal damage and a second destroying SBA).
fn duplicate_death(obj: ObjectId) -> Vec<GameEvent> {
    sba_deaths(&[obj, obj])
}

/// The executed ZoneChange events of a run.
fn zone_changes(r: &Run) -> usize {
    r.executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::ZoneChange { .. }))
        .count()
}

// REQ-EVENT-001, CR 704.3, 704.5g, 704.5h, 704.7, 701.8a (F2): two SBA moves of the SAME object
// to the graveyard are one result: with no effect, the object moves once, one executed
// ZoneChange, no error.
#[test]
fn req_event_001_r1_f2_same_object_duplicates_coalesce() {
    let (w, objs) = world(&[P1]);
    let r = run_batch(Bus::new(w, Vec::new()), duplicate_death(objs[0]), true, &[]);
    assert!(r.applications().is_empty());
    assert_eq!(zone_changes(&r), 1);
    assert_eq!(destinations(&r), vec![ZoneKind::Graveyard]);
    assert_eq!(r.world().zone_of(objs[0]), None);
    assert_eq!(
        r.world()
            .zones
            .len(fineprint_core::zones::ZoneId::Graveyard(P1)),
        1
    );
}

// REQ-EVENT-001, CR 704.7 (F2): with one exile redirect, the duplicate moves are replaced once:
// exiled once, one application; as `Uses(1)` it is consumed once and gone.
#[test]
fn req_event_001_r1_f2_same_object_duplicates_one_application() {
    for shielded in [false, true] {
        let (w, objs) = world(&[P1]);
        let effect = if shielded {
            redirect(1, ZoneKind::Exile).with_shield(Shield::Uses(1))
        } else {
            redirect(1, ZoneKind::Exile)
        };
        let r = run_batch(
            Bus::new(w, vec![effect]),
            duplicate_death(objs[0]),
            true,
            &[],
        );
        assert_eq!(r.applications(), eids(&[1]), "shielded {shielded}");
        assert_eq!(zone_changes(&r), 1);
        assert_eq!(destinations(&r), vec![ZoneKind::Exile]);
        assert_eq!(r.after.registry.get(eid(1)).is_none(), shielded);
        assert_eq!(r.world().zones.len(fineprint_core::zones::ZoneId::Exile), 1);
    }
}

// REQ-EVENT-001, CR 704.7, 616.1 (F2): with two candidate effects the duplicate moves make ONE
// request; the answer applies once. Both answers.
#[test]
fn req_event_001_r1_f2_same_object_duplicates_one_request() {
    for pick in [0u32, 1] {
        let (w, objs) = world(&[P1]);
        let bus = Bus::new(
            w,
            vec![redirect(1, ZoneKind::Exile), redirect(2, ZoneKind::Hand)],
        );
        let r = run_batch(bus, duplicate_death(objs[0]), true, &[pick]);
        assert_eq!(r.requests[0].reason.source, Some(objs[0]));
        assert_eq!(r.applications(), eids(&[u64::from(pick) + 1]));
        assert_eq!(zone_changes(&r), 1);
        let to = if pick == 0 {
            ZoneKind::Exile
        } else {
            ZoneKind::Hand
        };
        assert_eq!(destinations(&r), vec![to]);
    }
}

// REQ-EVENT-001, CR 704.7, 616.1 (F5): three different permanents and one `Uses(1)` exile
// redirect: the first (APNAP then submission order) is exiled, one application, the effect is
// gone, the other two go to the graveyard. Checked with player 2 controlling the first
// submitted object, so APNAP puts player 1's objects first.
#[test]
fn req_event_001_r1_f5_distinct_objects_one_use_shield() {
    for controllers in [[P1, P1, P1], [P2, P1, P1]] {
        let (w, objs) = world(&controllers);
        let bus = Bus::new(
            w,
            vec![redirect(1, ZoneKind::Exile).with_shield(Shield::Uses(1))],
        );
        let r = run_batch(bus, sba_deaths(&objs), true, &[]);
        assert_eq!(r.applications(), eids(&[1]));
        assert!(r.after.registry.is_empty());
        let first = if controllers[0] == P1 { 0 } else { 1 };
        let exiled: Vec<ObjectId> = r
            .executed()
            .iter()
            .filter(|e| r.world().zone_of(new_id(&e.outcome)) == Some(ZoneKind::Exile))
            .map(|e| match e.instance.event() {
                GameEvent::ZoneChange { object, .. } => *object,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(exiled, vec![objs[first]], "{controllers:?}");
        let mut dests = destinations(&r);
        dests.sort();
        let mut expected = vec![ZoneKind::Exile, ZoneKind::Graveyard, ZoneKind::Graveyard];
        expected.sort();
        assert_eq!(dests, expected);
    }
}

// REQ-EVENT-001, CR 704.7 (F5): an unlimited redirect applies once per object: three
// applications, three exiled.
#[test]
fn req_event_001_r1_f5_distinct_objects_unlimited_redirect() {
    let (w, objs) = world(&[P1, P1, P1]);
    let r = run_batch(
        Bus::new(w, vec![redirect(1, ZoneKind::Exile)]),
        sba_deaths(&objs),
        true,
        &[],
    );
    assert_eq!(r.applications(), eids(&[1, 1, 1]));
    assert_eq!(destinations(&r), vec![ZoneKind::Exile; 3]);
}

// REQ-EVENT-001, CR 704.7, 616.1 (F2, F5): different objects and a same-object duplicate in one
// SBA batch: one request per distinct object (the duplicate shares its object's request).
#[test]
fn req_event_001_r1_f5_mixed_duplicates_and_distinct_objects() {
    let (w, objs) = world(&[P1, P1]);
    let bus = Bus::new(
        w,
        vec![redirect(1, ZoneKind::Exile), redirect(2, ZoneKind::Hand)],
    );
    let events = sba_deaths(&[objs[0], objs[1], objs[0]]);
    let r = run_batch(bus, events, true, &[0, 1]);
    let sources: Vec<_> = r.requests.iter().map(|q| q.reason.source).collect();
    assert_eq!(sources, vec![Some(objs[0]), Some(objs[1])]);
    assert_eq!(r.applications(), eids(&[1, 2]));
    assert_eq!(zone_changes(&r), 2);
    assert_eq!(destinations(&r), vec![ZoneKind::Exile, ZoneKind::Hand]);
}

// REQ-EVENT-001, CR 704.7 (F2): without `sba` equal duplicate moves are not coalesced: the
// second fails and the whole call is rolled back.
#[test]
fn req_event_001_r1_f2_no_coalescing_without_sba() {
    let (w, objs) = world(&[P1]);
    let r = run_batch(
        Bus::new(w, Vec::new()),
        duplicate_death(objs[0]),
        false,
        &[],
    );
    assert!(r.result.is_err(), "{:?}", r.result);
    assert_unchanged(&r);
}

/// Two moves of `obj` to the graveyard with different causes (Destroy, StateBasedAction), in the
/// given submission order.
fn mixed_cause_deaths(obj: ObjectId, destroy_first: bool) -> Vec<GameEvent> {
    let destroy = zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::Destroy);
    let sba = zone_change(obj, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction);
    if destroy_first {
        vec![destroy, sba]
    } else {
        vec![sba, destroy]
    }
}

/// Runs the mixed-cause pair on `base` (with `sba`) in both submission orders and asserts the two
/// runs are identical: the same output (report or error), the same registry bytes, the same
/// world and the same state hash. Returns the two runs.
fn both_orders(base: &Bus, obj: ObjectId, sba: bool) -> (Run, Run) {
    let a = run_batch(base.clone(), mixed_cause_deaths(obj, true), sba, &[]);
    let b = run_batch(base.clone(), mixed_cause_deaths(obj, false), sba, &[]);
    assert_eq!(a.result, b.result, "the output depends on submission order");
    assert_eq!(bytes(&a.after.registry), bytes(&b.after.registry));
    assert_eq!(a.after.snap(), b.after.snap());
    assert_eq!(a.after.canonical(), b.after.canonical());
    assert_eq!(a.hash, b.hash);
    (a, b)
}

/// The coalescing invariants of one sba run: success, at most one executed move of `obj`.
fn assert_coalesced(r: &Run, obj: ObjectId) {
    assert!(r.result.is_ok(), "{:?}", r.result);
    let moves = r
        .executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::ZoneChange { object, .. } if *object == obj))
        .count();
    assert!(moves <= 1, "{moves} executed moves of the object");
}

// REQ-EVENT-001, CR 704.3, 704.7, 122.1c (F2): an object with a shield counter and the destroy
// shield registered, moved to the graveyard twice in one SBA batch with different causes
// (Destroy, StateBasedAction): the result is identical for both submission orders (which cause
// survives is the engine's content-based choice and is not asserted); never two moves, no error.
#[test]
fn req_event_001_r2_f2_mixed_causes_order_independent_shield_counter() {
    let (mut w, objs) = world(&[P1]);
    let x = objs[0];
    w.put_counters(x, "shield", 1);
    let base = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), x)],
    );
    let (a, b) = both_orders(&base, x, true);
    assert_coalesced(&a, x);
    assert_coalesced(&b, x);
}

// REQ-EVENT-001, CR 704.3, 704.7 (F2): the same pair with a one-use exile redirect matching any
// cause: identical for both orders, at most one move, no error.
#[test]
fn req_event_001_r2_f2_mixed_causes_order_independent_redirect() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(
        w,
        vec![redirect(1, ZoneKind::Exile).with_shield(Shield::Uses(1))],
    );
    let (a, b) = both_orders(&base, x, true);
    assert_coalesced(&a, x);
    assert_coalesced(&b, x);
    assert_eq!(a.applications(), eids(&[1]));
    assert_eq!(destinations(&a), vec![ZoneKind::Exile]);
}

// REQ-EVENT-001, CR 704.3, 704.7 (F2): the same pair with no effect: identical for both orders;
// the object moves to the graveyard exactly once.
#[test]
fn req_event_001_r2_f2_mixed_causes_order_independent_no_effect() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let (a, b) = both_orders(&Bus::new(w, Vec::new()), x, true);
    assert_coalesced(&a, x);
    assert_coalesced(&b, x);
    assert_eq!(zone_changes(&a), 1);
    assert_eq!(destinations(&a), vec![ZoneKind::Graveyard]);
}

// REQ-EVENT-001, CR 704.3 (F2): without `sba` the mixed-cause duplicate pair still fails and
// rolls back, in both submission orders.
#[test]
fn req_event_001_r2_f2_mixed_causes_without_sba_fail() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(w, Vec::new());
    for destroy_first in [true, false] {
        let r = run_batch(
            base.clone(),
            mixed_cause_deaths(x, destroy_first),
            false,
            &[],
        );
        assert!(r.result.is_err(), "{destroy_first}: {:?}", r.result);
        assert_unchanged(&r);
    }
}
