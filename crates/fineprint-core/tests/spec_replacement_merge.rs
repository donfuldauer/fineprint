// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for same-result replacement merging of state-based-action batches
//! (REQ-EVENT-001; CR 704.7, 616.1). With `sba = true`, events of one kind with the same
//! first-round candidate set and the same chooser make one choice, applied to all of them;
//! choosers that differ are asked separately in APNAP order. Black-box: written from the
//! work-item brief only.

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

// REQ-EVENT-001, CR 704.7: three same-kind SBA events with the same candidate set and the
// same chooser make ONE request; the answer applies to all three; one application. Both id
// orders and both answers.
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
            assert_eq!(r.requests.len(), 1, "one merged request");
            assert_eq!(r.requests[0].chooser, P1);
            assert_eq!(r.offered(0), descs(&[1, 2]));
            assert_eq!(r.applications(), eids(&[pick]));
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

// REQ-EVENT-001, CR 704.7, 615: the merged application consumes ONE use of a `Uses(1)` shield
// and still covers all three events.
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
    let r = run_batch(bus, sba_deaths(&objs), true, &[0]);
    assert_eq!(r.applications(), eids(&[1]));
    assert_eq!(destinations(&r), vec![ZoneKind::Exile; 3]);
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

// REQ-EVENT-001, CR 704.7, 616.1 (amendment): same kind and same candidate set but different
// choosers do not merge: two requests, player 1 (active) first, each answer applied to its
// own event. Both submission orders.
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

// REQ-EVENT-001, CR 704.7: in an SBA batch, events of a different kind or with a different
// candidate set do not merge: one request per distinct (kind, candidate set) group.
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
    assert_eq!(r.requests.len(), 3);
    let mut offered: Vec<Vec<String>> = (0..3).map(|i| r.offered(i)).collect();
    offered.sort();
    let mut expected = vec![descs(&[1, 2]), descs(&[1, 2, 3]), descs(&[4, 5])];
    expected.sort();
    assert_eq!(offered, expected);
    let mut apps = r.applications();
    apps.sort();
    assert_eq!(apps, eids(&[1, 1, 4]));
    assert_eq!(r.report().prevented, 5);
    assert_eq!(r.world().life(P1), STARTING_LIFE);
    assert_eq!(destinations(&r), vec![ZoneKind::Exile; 3]);
}
