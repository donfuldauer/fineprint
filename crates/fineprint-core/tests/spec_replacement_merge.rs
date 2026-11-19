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
//! finding F2, round 2). Review round 3 (H2): such a group is handled member-wise: the candidates
//! are the effects eligible for at least one member; the chosen effect is applied once to every
//! member it matches; unmatched members still move the object; the object moves at most once, and
//! where the surviving destinations differ a member modified by an application wins.
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

// REQ-EVENT-001, CR 704.3, 704.7, 701.8b, 122.1c (F2, corrected in round 3 by H2): an object
// with a shield counter and the destroy shield registered, moved to the graveyard twice in one SBA
// batch with different causes (Destroy, StateBasedAction): the result is identical for both
// submission orders, no error. The shield matches only the Destroy member (CR 122.1c: destroyed
// as the result of an effect): it is applied once and removes the counter; the unmatched
// StateBasedAction member still moves the object to the graveyard, exactly once.
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
    assert_eq!(a.applications(), eids(&[1]));
    assert_eq!(a.world().counter(x, "shield"), 0);
    assert_eq!(moves_of(&a, x), 1);
    assert_eq!(destinations(&a), vec![ZoneKind::Graveyard]);
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
    assert_eq!(moves_of(&a, x), 1);
    assert_eq!(destinations(&a), vec![ZoneKind::Exile]);
}

// REQ-EVENT-001, CR 704.3, 704.7 (F2, round 3 H2): the same pair with no effect: identical for
// both orders; the members keep the same destination, so they are one move: the object moves to
// the graveyard exactly once, nothing applied.
#[test]
fn req_event_001_r2_f2_mixed_causes_order_independent_no_effect() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let (a, b) = both_orders(&Bus::new(w, Vec::new()), x, true);
    assert_coalesced(&a, x);
    assert_coalesced(&b, x);
    assert!(a.applications().is_empty());
    assert_eq!(zone_changes(&a), 1);
    assert_eq!(moves_of(&a, x), 1);
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

// ---------------------------------------------------------------------------------------------
// review round 3 (H2): member-wise SBA groups
// ---------------------------------------------------------------------------------------------

/// The number of executed ZoneChange events of `obj` in a run.
fn moves_of(r: &Run, obj: ObjectId) -> usize {
    r.executed()
        .iter()
        .filter(|e| matches!(e.instance.event(), GameEvent::ZoneChange { object, .. } if *object == obj))
        .count()
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

fn to_exile() -> Action {
    Action::Modify(Modification::SetDestination(ZoneKind::Exile))
}

/// A move of `obj` to the graveyard with `cause`.
fn death(obj: ObjectId, cause: ZoneChangeCause) -> GameEvent {
    zone_change(obj, ZoneKind::Graveyard, cause)
}

/// Runs `events` and their reverse as one SBA batch on `base`, answering each
/// CHOOSE_REPLACEMENT_EFFECT request by effect id preference `prefer`, and asserts the two runs
/// give the same output (report or error), registry bytes, world snapshot and state hash.
fn h2_both_orders(base: &Bus, events: Vec<GameEvent>, prefer: &[u64]) -> (Run, Run) {
    let mut reversed = events.clone();
    reversed.reverse();
    let a = run_preferring(base.clone(), Call::batch(events, true), prefer);
    let b = run_preferring(base.clone(), Call::batch(reversed, true), prefer);
    assert_eq!(a.result, b.result, "the output depends on submission order");
    assert_eq!(bytes(&a.after.registry), bytes(&b.after.registry));
    assert_eq!(a.after.snap(), b.after.snap());
    assert_eq!(a.after.canonical(), b.after.canonical());
    assert_eq!(a.hash, b.hash);
    assert_eq!(a.requests.len(), b.requests.len());
    (a, b)
}

// REQ-EVENT-001, CR 704.3, 704.7 (H2 a): one permanent, [Destroy, StateBasedAction] to the
// graveyard, a one-use exile redirect matching only the StateBasedAction cause: the effect is
// offered for the group, applied once, and the object ends in exile, moved exactly once. Both
// submission orders.
#[test]
fn req_event_001_r3_h2_effect_matching_only_sba_member_applies() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(
        w,
        vec![one_use(1, ZoneChangeCause::StateBasedAction, to_exile())],
    );
    let events = vec![
        death(x, ZoneChangeCause::Destroy),
        death(x, ZoneChangeCause::StateBasedAction),
    ];
    let (a, b) = h2_both_orders(&base, events, &[]);
    for r in [&a, &b] {
        assert!(r.requests.is_empty());
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(destinations(r), vec![ZoneKind::Exile]);
        assert_eq!(r.world().zone_of(x), None);
        assert!(r.after.registry.is_empty());
    }
}

// REQ-EVENT-001, CR 704.3 (H2 b, control): the StateBasedAction move alone with the same effect
// is redirected to exile, one application.
#[test]
fn req_event_001_r3_h2_control_sba_move_alone() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(
        w,
        vec![one_use(1, ZoneChangeCause::StateBasedAction, to_exile())],
    );
    let r = run_batch(
        base,
        vec![death(x, ZoneChangeCause::StateBasedAction)],
        true,
        &[],
    );
    assert_eq!(r.applications(), eids(&[1]));
    assert_eq!(moves_of(&r, x), 1);
    assert_eq!(destinations(&r), vec![ZoneKind::Exile]);
}

// REQ-EVENT-001, CR 704.3, 704.7 (H2 c): a one-use Skip matching only the StateBasedAction cause
// drops that member; the unmatched Destroy member still moves the object to the graveyard, once.
// Both submission orders.
#[test]
fn req_event_001_r3_h2_skip_of_one_member_still_moves_object() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(
        w,
        vec![one_use(1, ZoneChangeCause::StateBasedAction, Action::Skip)],
    );
    let events = vec![
        death(x, ZoneChangeCause::Destroy),
        death(x, ZoneChangeCause::StateBasedAction),
    ];
    let (a, b) = h2_both_orders(&base, events, &[]);
    for r in [&a, &b] {
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(destinations(r), vec![ZoneKind::Graveyard]);
        assert_eq!(r.world().zone_of(x), None);
    }
}

// REQ-EVENT-001, CR 704.3, 704.7, 701.8b, 122.1c (H2 d): a shield counter and the destroy shield,
// [Destroy, StateBasedAction]: the shield (Destroy only) is applied once and the counter removed;
// the StateBasedAction member is not matched by it and moves the object to the graveyard, once.
// Both submission orders.
#[test]
fn req_event_001_r3_h2_shield_counter_with_sba_member() {
    let (mut w, objs) = world(&[P1]);
    let x = objs[0];
    w.put_counters(x, "shield", 1);
    let base = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), x)],
    );
    let events = vec![
        death(x, ZoneChangeCause::Destroy),
        death(x, ZoneChangeCause::StateBasedAction),
    ];
    let (a, b) = h2_both_orders(&base, events, &[]);
    for r in [&a, &b] {
        assert!(r.result.is_ok(), "{:?}", r.result);
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(r.world().counter(x, "shield"), 0);
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(destinations(r), vec![ZoneKind::Graveyard]);
        assert_eq!(r.world().zone_of(x), None);
    }
}

// REQ-EVENT-001, CR 701.8b, 122.1c (H2, shield counter): the destroy shield does not match a
// StateBasedAction move on its own: nothing applied, the counter stays on the object until it
// moves, the object goes to the graveyard.
#[test]
fn req_event_001_r3_h2_shield_counter_ignores_sba_cause() {
    let (mut w, objs) = world(&[P1]);
    let x = objs[0];
    w.put_counters(x, "shield", 1);
    let base = Bus::new(
        w,
        vec![ReplacementEffect::shield_counter_destroy(eid(1), x)],
    );
    let r = run_batch(
        base,
        vec![death(x, ZoneChangeCause::StateBasedAction)],
        true,
        &[],
    );
    assert!(r.applications().is_empty());
    assert_eq!(moves_of(&r, x), 1);
    assert_eq!(destinations(&r), vec![ZoneKind::Graveyard]);
    assert!(r.after.registry.get(eid(1)).is_some());
}

// REQ-EVENT-001, CR 704.7 (H2 e): identical StateBasedAction duplicates and a one-use exile
// redirect on that cause: exactly one application, one move, exile.
#[test]
fn req_event_001_r3_h2_same_cause_duplicates_one_application() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(
        w,
        vec![one_use(1, ZoneChangeCause::StateBasedAction, to_exile())],
    );
    let events = vec![
        death(x, ZoneChangeCause::StateBasedAction),
        death(x, ZoneChangeCause::StateBasedAction),
    ];
    let (a, b) = h2_both_orders(&base, events, &[]);
    for r in [&a, &b] {
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(destinations(r), vec![ZoneKind::Exile]);
    }
}

// REQ-EVENT-001, CR 704.3, 704.7 (H2 f): a one-use exile redirect matching only the Destroy cause:
// applied once; the modified member wins over the unmodified StateBasedAction member, so the
// object ends in exile, moved once. Both submission orders.
#[test]
fn req_event_001_r3_h2_modified_member_wins() {
    let (w, objs) = world(&[P1]);
    let x = objs[0];
    let base = Bus::new(w, vec![one_use(1, ZoneChangeCause::Destroy, to_exile())]);
    let events = vec![
        death(x, ZoneChangeCause::Destroy),
        death(x, ZoneChangeCause::StateBasedAction),
    ];
    let (a, b) = h2_both_orders(&base, events, &[]);
    for r in [&a, &b] {
        assert_eq!(r.applications(), eids(&[1]));
        assert_eq!(moves_of(r, x), 1);
        assert_eq!(destinations(r), vec![ZoneKind::Exile]);
        assert_eq!(r.world().zone_of(x), None);
    }
}

// REQ-EVENT-001, CR 704.7, 616.1, 616.1f (H2 g): e1 (Destroy only, exile) and e2
// (StateBasedAction only, skip), [Destroy, StateBasedAction]: the chooser is asked once with both
// effects offered. Common ground only: one request, no error, at most one move, `applications`
// starts with the chosen effect, and for the same answer both submission orders agree.
#[test]
fn req_event_001_r3_h2_two_cause_specific_effects_one_request() {
    for pick in [1u64, 2] {
        let (w, objs) = world(&[P1]);
        let x = objs[0];
        let base = Bus::new(
            w,
            vec![
                one_use(1, ZoneChangeCause::Destroy, to_exile()),
                one_use(2, ZoneChangeCause::StateBasedAction, Action::Skip),
            ],
        );
        let events = vec![
            death(x, ZoneChangeCause::Destroy),
            death(x, ZoneChangeCause::StateBasedAction),
        ];
        let (a, b) = h2_both_orders(&base, events, &[pick]);
        for r in [&a, &b] {
            assert!(r.result.is_ok(), "pick {pick}: {:?}", r.result);
            assert_eq!(r.requests.len(), 1, "pick {pick}");
            assert_eq!(
                r.requests[0].choice_type,
                fineprint_core::choice::ChoiceType::ChooseReplacementEffect
            );
            let mut offered = r.offered(0);
            offered.sort();
            assert_eq!(offered, descs(&[1, 2]), "pick {pick}");
            assert!(moves_of(r, x) <= 1, "pick {pick}");
            assert_eq!(r.applications().first(), Some(&eid(pick)), "pick {pick}");
        }
    }
}

// REQ-EVENT-001, CR 704.7 (H2 h): different objects in one SBA batch with the same cause-specific
// effect never merge: each object is processed on its own (one application each, each exiled
// once). Both submission orders.
#[test]
fn req_event_001_r3_h2_different_objects_do_not_merge() {
    let (w, objs) = world(&[P1, P1]);
    let base = Bus::new(
        w,
        vec![fx(
            1,
            cause_is(ZoneChangeCause::StateBasedAction),
            to_exile(),
        )],
    );
    for order in [[0usize, 1], [1, 0]] {
        let events: Vec<GameEvent> = order
            .iter()
            .map(|&i| death(objs[i], ZoneChangeCause::StateBasedAction))
            .collect();
        let r = run_batch(base.clone(), events, true, &[]);
        assert_eq!(r.applications(), eids(&[1, 1]), "{order:?}");
        for &o in &objs {
            assert_eq!(moves_of(&r, o), 1, "{order:?}");
            assert_eq!(r.world().zone_of(o), None, "{order:?}");
        }
        assert_eq!(destinations(&r), vec![ZoneKind::Exile; 2], "{order:?}");
    }
}

const H2_CAUSES: [ZoneChangeCause; 3] = [
    ZoneChangeCause::Destroy,
    ZoneChangeCause::StateBasedAction,
    ZoneChangeCause::Other,
];

/// Every permutation of `items` (lexicographic over positions; at most 3 items here).
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

// REQ-EVENT-001, REQ-CORE-001, CR 704.3, 704.7, 616.1f (H2 i): for generated groups of 2-3 SBA
// members of one object (causes Destroy, StateBasedAction, Other) and 0-2 cause-specific effects
// (exile redirect or skip, unlimited or one-use), every permutation of the submission order gives
// the same output, registry bytes and state hash, the call succeeds, and the object moves at most
// once. Re-running the first order gives the identical result (determinism).
#[test]
fn req_event_001_r3_h2_property_group_order_independent() {
    let (mut asked, mut applied, mut moved, mut stayed) = (0usize, 0usize, 0usize, 0usize);
    for seed in 0..120u64 {
        let mut rng = Lcg::new(seed ^ 0x0004_0002);
        let (w, objs) = world(&[rng.pick(&[P1, P2])]);
        let x = objs[0];
        let members: Vec<GameEvent> = (0..2 + rng.below(2))
            .map(|_| death(x, rng.pick(&H2_CAUSES)))
            .collect();
        let effects: Vec<ReplacementEffect> = (0..rng.below(3))
            .map(|i| {
                let action = if rng.chance(1, 2) {
                    to_exile()
                } else {
                    Action::Skip
                };
                let e = fx(i + 1, cause_is(rng.pick(&H2_CAUSES)), action);
                if rng.chance(1, 2) {
                    e.with_shield(Shield::Uses(1))
                } else {
                    e
                }
            })
            .collect();
        let base = Bus::new(w, effects);
        // The same answer whatever the option order: prefer effect 1, then effect 2.
        let runs: Vec<Run> = permutations(&members)
            .into_iter()
            .map(|order| run_preferring(base.clone(), Call::batch(order, true), &[1, 2]))
            .collect();
        let first = &runs[0];
        assert!(first.result.is_ok(), "seed {seed}: {:?}", first.result);
        for (k, r) in runs.iter().enumerate() {
            let ctx = format!("seed {seed}, permutation {k}");
            assert_eq!(r.result, first.result, "{ctx}");
            assert_eq!(
                bytes(&r.after.registry),
                bytes(&first.after.registry),
                "{ctx}"
            );
            assert_eq!(r.after.snap(), first.after.snap(), "{ctx}");
            assert_eq!(r.hash, first.hash, "{ctx}");
            assert!(moves_of(r, x) <= 1, "{ctx}");
        }
        let again = run_preferring(base, Call::batch(members, true), &[1, 2]);
        assert_eq!(again.result, first.result, "seed {seed}: rerun");
        assert_eq!(again.hash, first.hash, "seed {seed}: rerun");
        asked += usize::from(!first.requests.is_empty());
        applied += usize::from(!first.applications().is_empty());
        if moves_of(first, x) == 1 {
            moved += 1;
        } else {
            stayed += 1;
        }
    }
    // The generator reaches requests, applications, moved and unmoved outcomes.
    assert!(
        asked > 0 && applied > 0 && moved > 0 && stayed > 0,
        "asked {asked}, applied {applied}, moved {moved}, stayed {stayed}"
    );
}
