// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the allocation of a limited prevention pool among simultaneous damage from
//! two or more sources (REQ-EVENT-001; CR 615.7, review round 1 finding F8). When the eligible
//! damage of a batch comes from at least two sources and exceeds the pool, the shielded player
//! is asked once, before anything else of the batch, with a `DIVIDE_VALUE` request; otherwise
//! the pool is drawn on automatically in submission order. Black-box: written from the
//! work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::choice::{
    ChoiceError, ChoiceOption, ChoiceOptions, ChoiceSession, ChoiceTarget, ChoiceType, Status,
};
use fineprint_core::events::GameEvent;
use fineprint_core::replacement::{Action, BusEventKind, EventMatcher, ReplacementEffect, Shield};
use fineprint_core::zones::ZoneKind;
use support::*;

/// The pool effect id used throughout.
const POOL: u64 = 5;

/// A prevention effect on damage to player 2 with a pool of `n`.
fn pool(n: i64) -> ReplacementEffect {
    fx(POOL, m_dmg_player(P2), Action::Prevent { then: Vec::new() }).with_shield(Shield::Pool(n))
}

/// A world with two battlefield source objects A < B (ascending ids).
fn sources() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    assert!(a < b);
    (w, a, b)
}

/// Drives `call`, answering the `i`-th request with `answers[i]`; panics on more requests.
fn run_answers(base: Bus, call: Call, answers: &[fineprint_core::choice::ChoiceResponse]) -> Run {
    let r = drive_with(base, call, |i, req| match answers.get(i) {
        Some(a) => a.clone(),
        None => panic!("unexpected request #{i}: {req:?}"),
    });
    assert_eq!(r.requests.len(), answers.len());
    r
}

/// Damage dealt: `(source, amount)` per executed Damage event.
type Dealt = Vec<(Option<ObjectId>, i64)>;

/// The executed Damage events of a run as `(source, amount)`, in execution order.
fn dealt(r: &Run) -> Dealt {
    r.executed()
        .iter()
        .filter_map(|e| match e.instance.event() {
            GameEvent::Damage { source, amount, .. } => Some((*source, *amount)),
            _ => None,
        })
        .collect()
}

// REQ-EVENT-001, CR 615.7 (F8): pool 3 and 3 damage from each of two sources: exactly one
// DIVIDE_VALUE request to player 2, total 3, recipients A and B ascending with minimum 0,
// reason CR 615.7; no CHOOSE_REPLACEMENT_EFFECT request.
#[test]
fn req_event_001_r1_f8_allocation_request_shape() {
    let (w, a, b) = sources();
    let r = run_answers(
        Bus::new(w, vec![pool(3)]),
        Call::batch(vec![dmg_from(a, P2, 3), dmg_from(b, P2, 3)], false),
        &[divide(&[a, b], &[3, 0])],
    );
    assert_eq!(r.requests.len(), 1);
    let req = &r.requests[0];
    assert_eq!(req.choice_type, ChoiceType::DivideValue);
    assert_ne!(req.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(req.chooser, P2);
    assert_eq!(req.reason.cr, "CR 615.7");
    assert_eq!(req.reason.source, None);
    assert_eq!(
        division(req),
        (
            3,
            vec![(ChoiceTarget::Object(a), 0), (ChoiceTarget::Object(b), 0)]
        )
    );
    assert_eq!((req.min, req.max), (2, 2));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Object(a), ChoiceOption::Object(b)])
    );
    assert!(!req.hidden);
    assert_eq!(req.default_on_timeout, divide(&[a, b], &[3, 0]));
}

// REQ-EVENT-001, CR 615.7 (F8): the answer decides which damage is prevented: (A:3, B:0) deals
// B's damage; (A:0, B:3) deals A's; (A:1, B:2) prevents 3 in total and deals 3 in total. The
// pool is drained and removed in every case; the same answer gives identical results.
#[test]
fn req_event_001_r1_f8_answer_decides_prevented_damage() {
    let (w, a, b) = sources();
    let call = Call::batch(vec![dmg_from(a, P2, 3), dmg_from(b, P2, 3)], false);
    let base = Bus::new(w, vec![pool(3)]);
    let cases: [([u64; 2], Dealt); 3] = [
        ([3, 0], vec![(Some(b), 3)]),
        ([0, 3], vec![(Some(a), 3)]),
        ([1, 2], vec![(Some(a), 2), (Some(b), 1)]),
    ];
    for (split, expected) in cases {
        let answer = [divide(&[a, b], &split)];
        let r = run_answers(base.clone(), call.clone(), &answer);
        assert_eq!(r.report().prevented, 3, "{split:?}");
        assert_eq!(r.world().life(P2), STARTING_LIFE - 3, "{split:?}");
        assert_eq!(dealt(&r), expected, "{split:?}");
        assert!(r.after.registry.is_empty(), "{split:?}");
        assert!(r.applications().contains(&eid(POOL)));
        match split {
            [3, 0] => assert_eq!(r.applications(), eids(&[POOL])),
            [1, 2] => assert_eq!(r.applications(), eids(&[POOL, POOL])),
            _ => {}
        }
        let again = run_answers(base.clone(), call.clone(), &answer);
        assert_eq!(again.result, r.result);
        assert_eq!(again.after.canonical(), r.after.canonical());
        assert_eq!(again.hash, r.hash);
    }
}

// REQ-EVENT-001, CR 615.7 (F8): the order of submission does not decide: submitting B's damage
// first gives the same request (recipients still ascending) and the same result for the same
// answer.
#[test]
fn req_event_001_r1_f8_submission_order_does_not_decide() {
    let (w, a, b) = sources();
    let base = Bus::new(w, vec![pool(3)]);
    for split in [[3u64, 0], [0, 3], [1, 2], [2, 1]] {
        let answer = [divide(&[a, b], &split)];
        let ab = run_answers(
            base.clone(),
            Call::batch(vec![dmg_from(a, P2, 3), dmg_from(b, P2, 3)], false),
            &answer,
        );
        let ba = run_answers(
            base.clone(),
            Call::batch(vec![dmg_from(b, P2, 3), dmg_from(a, P2, 3)], false),
            &answer,
        );
        assert_eq!(ab.requests, ba.requests, "{split:?}");
        assert_eq!(ab.report().prevented, ba.report().prevented);
        assert_eq!(ab.world().life(P2), ba.world().life(P2));
        assert_eq!(bytes(&ab.after.registry), bytes(&ba.after.registry));
        let mut x = dealt(&ab);
        let mut y = dealt(&ba);
        x.sort();
        y.sort();
        assert_eq!(x, y, "{split:?}");
        // The damage each source dealt is its 3 minus its allotment.
        let expected: Dealt = [(a, split[0]), (b, split[1])]
            .iter()
            .filter(|(_, n)| *n < 3)
            .map(|&(s, n)| (Some(s), 3 - i64::try_from(n).expect("fits")))
            .collect();
        assert_eq!(x, expected, "{split:?}");
    }
}

// REQ-EVENT-001, CR 615.7 (F8), SRS 15.3: the session rejects a division that does not sum to
// the pool and stays pending with its base unchanged; a valid answer completes; replaying the
// recorded answers gives an identical state.
#[test]
fn req_event_001_r1_f8_invalid_division_rejected_and_replay() {
    let (w, a, b) = sources();
    let base = Bus::new(w, vec![pool(3)]);
    let call = Call::batch(vec![dmg_from(a, P2, 3), dmg_from(b, P2, 3)], false);
    let base_snap = base.snap();
    let mut s = ChoiceSession::start(base.clone(), procedure(call.clone())).expect("start");
    let req = s.pending().cloned().expect("pending");
    for bad in [[1u64, 1], [3, 3], [0, 0], [4, 0]] {
        let got = s.submit(req.request_id, divide(&[a, b], &bad));
        assert!(
            matches!(got, Err(ChoiceError::Invalid { .. })),
            "{bad:?}: {got:?}"
        );
        assert_eq!(s.pending(), Some(&req));
        assert!(s.answers().is_empty());
        assert_eq!(s.base().snap(), base_snap);
    }
    s.submit(req.request_id, divide(&[a, b], &[2, 1]))
        .expect("valid division");
    let (state, output) = match s.status() {
        Status::Done { state, output } => (state.clone(), output.clone()),
        Status::Pending(p) => panic!("still pending: {p:?}"),
    };
    let report = output.clone().expect("no bus error");
    assert_eq!(report.prevented, 3);
    assert_eq!(state.world.life(P2), STARTING_LIFE - 3);
    let replayed = ChoiceSession::replay(base, procedure(call), s.answers()).expect("replay");
    match replayed.status() {
        Status::Done {
            state: st2,
            output: out2,
        } => {
            assert_eq!(out2, &output);
            assert_eq!(st2.canonical(), state.canonical());
        }
        Status::Pending(p) => panic!("replay pending: {p:?}"),
    }
    assert_eq!(replayed.state_hash(), s.state_hash());
}

// REQ-EVENT-001, CR 615.7 (F8): no request when the pool covers all eligible damage: pool 6,
// 3 + 3 from two sources, all prevented.
#[test]
fn req_event_001_r1_f8_pool_covering_all_asks_nothing() {
    let (w, a, b) = sources();
    let r = run_batch(
        Bus::new(w, vec![pool(6)]),
        vec![dmg_from(a, P2, 3), dmg_from(b, P2, 3)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 6);
    assert_eq!(r.world().life(P2), STARTING_LIFE);
    assert!(dealt(&r).is_empty());
    assert_eq!(r.applications(), eids(&[POOL, POOL]));
}

// REQ-EVENT-001, CR 615.7 (F8): no request with a single source: two events of 3 from source A
// and pool 3: the first event is prevented automatically, the second is dealt.
#[test]
fn req_event_001_r1_f8_single_source_is_automatic() {
    let (w, a, _) = sources();
    let r = run_batch(
        Bus::new(w, vec![pool(3)]),
        vec![dmg_from(a, P2, 3), dmg_from(a, P2, 3)],
        false,
        &[],
    );
    assert_eq!(r.report().prevented, 3);
    assert_eq!(r.applications(), eids(&[POOL]));
    assert_eq!(dealt(&r), vec![(Some(a), 3)]);
    assert_eq!(r.world().life(P2), STARTING_LIFE - 3);
    assert!(r.after.registry.is_empty());
}

// REQ-EVENT-001, CR 615.7, 615.8 (F8): a `Uses` shield is unchanged by the allocation protocol:
// two sources, a one-use prevention: no request, the first submitted event is prevented.
#[test]
fn req_event_001_r1_f8_uses_shield_asks_nothing() {
    for first_a in [true, false] {
        let (w, a, b) = sources();
        let effect = fx(POOL, m_dmg_player(P2), Action::Prevent { then: Vec::new() })
            .with_shield(Shield::Uses(1));
        let (x, y) = if first_a { (a, b) } else { (b, a) };
        let r = run_batch(
            Bus::new(w, vec![effect]),
            vec![dmg_from(x, P2, 3), dmg_from(y, P2, 3)],
            false,
            &[],
        );
        assert_eq!(r.report().prevented, 3);
        assert_eq!(dealt(&r), vec![(Some(y), 3)]);
        assert!(r.after.registry.is_empty());
    }
}

// REQ-EVENT-001, CR 615.7, 616.1 (F8): the allocation request is asked before every other
// request of the batch, even one of the active player (player 1) whose event is submitted
// first.
#[test]
fn req_event_001_r1_f8_allocation_request_comes_first() {
    let (mut w, a, b) = sources();
    w.create(ZoneKind::Library, P1);
    let draw = EventMatcher {
        affected_player: Some(P1),
        ..EventMatcher::new(BusEventKind::Draw)
    };
    let effects = vec![
        fx(1, draw.clone(), Action::Skip),
        fx(2, draw, Action::Skip),
        pool(3),
    ];
    let r = drive_with(
        Bus::new(w, effects),
        Call::batch(
            vec![
                GameEvent::Draw { player: P1 },
                dmg_from(a, P2, 3),
                dmg_from(b, P2, 3),
            ],
            false,
        ),
        |i, req| match i {
            0 => divide(&[a, b], &[0, 3]),
            _ => select(index_of(req, 2)),
        },
    );
    assert_eq!(r.requests.len(), 2);
    assert_eq!(r.requests[0].choice_type, ChoiceType::DivideValue);
    assert_eq!(r.requests[0].chooser, P2);
    assert_eq!(
        r.requests[1].choice_type,
        ChoiceType::ChooseReplacementEffect
    );
    assert_eq!(r.requests[1].chooser, P1);
    assert_eq!(dealt(&r), vec![(Some(a), 3)]);
    assert_eq!(r.report().prevented, 3);
    assert!(r.applications().contains(&eid(2)));
}

// REQ-EVENT-001, CR 615.7 (F8): allotment a source cannot use stays in the shield: pool 4,
// 3 from A and 2 from B, answer (A:1, B:3): A deals 2, B's 2 are prevented, 1 is left in the
// pool for later damage.
#[test]
fn req_event_001_r1_f8_unusable_allotment_stays_in_pool() {
    let (w, a, b) = sources();
    let r = run_answers(
        Bus::new(w, vec![pool(4)]),
        Call::batch(vec![dmg_from(a, P2, 3), dmg_from(b, P2, 2)], false),
        &[divide(&[a, b], &[1, 3])],
    );
    assert_eq!(division(&r.requests[0]).0, 4);
    assert_eq!(r.report().prevented, 3);
    assert_eq!(dealt(&r), vec![(Some(a), 2)]);
    assert!(r.after.registry.get(eid(POOL)).is_some());
    // The remaining 1 point prevents 1 of the next damage.
    let next = run_event(r.after.clone(), dmg_from(a, P2, 2), &[]);
    assert_eq!(next.report().prevented, 1);
    assert_eq!(dealt(&next), vec![(Some(a), 1)]);
    assert!(next.after.registry.is_empty());
}
