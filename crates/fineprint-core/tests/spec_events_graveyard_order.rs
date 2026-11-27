// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the owner's choice of order when two or more cards enter one graveyard at the
//! same time (REQ-ZONE-001, REQ-EVENT-001, SRS 15.3 `ORDER_CARDS`; CR 404.3, 404.1, 404.2,
//! 400.3, 400.5, 111.6, 111.7, 108.2b, 614.6, 704.3). Black-box: written from the work-item
//! brief only. Every card is synthetic (R5).

#[path = "spec_events_graveyard_order_support.rs"]
mod support;

use fineprint_base::hash::Digest;
use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType, Interrupt,
    OrderDestination, PendingChoiceRequest, RequestId, Status,
};
use fineprint_core::events::{
    EventBatch, EventError, EventInstance, ExecutedBatch, GameEvent, GraveyardOrderGroup,
    ZoneChangeCause,
};
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, BusReport, EventMatcher, Modification,
    ReplacementEffect, ReplacementRegistry, process_batch,
};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{Position, ZoneId, ZoneKind};
use support::*;

/// Orders given as `(owner, old ids top to bottom)`.
type OrderList = Vec<(PlayerId, Vec<ObjectId>)>;

/// The one group of `groups`, asserting there is exactly one.
fn only(groups: &[GraveyardOrderGroup]) -> &GraveyardOrderGroup {
    assert_eq!(groups.len(), 1, "expected exactly one group: {groups:?}");
    &groups[0]
}

/// Runs `batch` on a clone of `base` with `orders` (each given as old ids, top to bottom) and
/// returns the world and the executed batch.
fn ordered(
    base: &World,
    batch: &EventBatch,
    orders: &[(PlayerId, Vec<ObjectId>)],
) -> (World, ExecutedBatch) {
    let mut w = base.clone();
    let list: Vec<_> = orders.iter().map(|(p, o)| order(*p, o)).collect();
    let done = match w.run_ordered(batch, &list) {
        Ok(d) => d,
        Err(e) => panic!("execute_batch_ordered({list:?}) failed: {e:?}"),
    };
    (w, done)
}

/// Runs `batch` on a clone of `base` with `execute_batch`.
fn plain(base: &World, batch: &EventBatch) -> (World, ExecutedBatch) {
    let mut w = base.clone();
    let done = w.run_plain(batch);
    (w, done)
}

// =============================================================================================
// Deliverable 1: two cards, both orders, replay
// =============================================================================================

// REQ-ZONE-001, REQ-EVENT-001, CR 404.3, 704.3: two nontoken cards of one owner dying in one
// state-based batch form one group; both orders are reachable through the ORDER_CARDS choice
// (request, answer, `order_from_response`, `execute_batch_ordered`); replaying the recorded
// answers gives an identical state hash, identical zones and an identical canonical encoding of
// the executed batch.
#[test]
fn req_zone_001_two_cards_both_orders_reachable_and_replay_identical() {
    let mut base = World::new();
    let ab = base.creatures(P1, 2);
    let (a, b) = (ab[0], ab[1]);
    let batch = all_die(&ab);

    let mut g = base.clone();
    let groups = g.groups(&batch);
    let group = only(&groups);
    assert_eq!(group.owner, P1);
    assert_eq!(group.cards, sorted(&ab));
    // Last submitted ends on top.
    assert_eq!(group.default_order, vec![b, a]);

    let mut finals: Vec<Vec<ObjectId>> = Vec::new();
    let mut hashes: Vec<Digest> = Vec::new();
    for perm in [vec![a, b], vec![b, a]] {
        let run = drive(&base, &batch, |_, _| Some(gy_answer(&perm)));
        assert_eq!(run.requests.len(), 1, "one ORDER_CARDS request");
        assert_eq!(run.requests[0].choice_type, ChoiceType::OrderCards);
        assert_eq!(run.requests[0].chooser, P1);
        let gy = run.state.graveyard(P1);
        assert_eq!(gy, new_ids(&run.output, &perm), "chosen order {perm:?}");
        assert_eq!(olds_of(&run.output, &gy), perm);

        // Replay with the recorded answers (REQ-EVENT-001, REQ-NET-003 replay).
        let again = replay(&base, &batch, &run.answers);
        assert_eq!(again.hash, run.hash, "state hash");
        assert_eq!(
            again.state.snap(),
            run.state.snap(),
            "zones and every component"
        );
        assert_eq!(bytes(&again.state.zones), bytes(&run.state.zones));
        assert_eq!(bytes(&again.output), bytes(&run.output), "canonical batch");
        assert_eq!(again.output, run.output);

        // The same through the direct API.
        let mut d = base.clone();
        let groups = d.groups(&batch);
        let req = only(&groups).request(RequestId::new(0));
        assert_eq!(req.validate(&gy_answer(&perm)), Ok(()));
        let o = only(&groups)
            .order_from_response(&gy_answer(&perm))
            .expect("valid answer");
        assert_eq!(o.owner, P1);
        assert_eq!(o.top_to_bottom, perm);
        let done = d.run_ordered(&batch, &[o]).expect("ordered batch");
        assert_eq!(d.snap(), run.state.snap(), "direct API equals the session");
        assert_eq!(bytes(&done), bytes(&run.output));

        finals.push(perm);
        hashes.push(run.hash);
    }
    assert_ne!(finals[0], finals[1]);
    assert_ne!(
        hashes[0], hashes[1],
        "different orders give different states"
    );
}

// REQ-EVENT-001, SRS 15.3 ORDER_CARDS, CR 404.3: the request's fields (brief clause b), for an
// owner who is not the active player.
#[test]
fn req_event_001_request_fields() {
    let mut w = World::new();
    let cards = w.creatures(P2, 3);
    let batch = all_die(&cards);
    let groups = w.groups(&batch);
    let g = only(&groups);
    assert_eq!(g.owner, P2);
    assert_eq!(g.cards, sorted(&cards));
    for id in [0, 4] {
        let req = g.request(RequestId::new(id));
        assert_eq!(req.request_id, RequestId::new(id));
        assert_eq!(req.chooser, P2, "the owner chooses (CR 404.3)");
        assert_eq!(req.choice_type, ChoiceType::OrderCards);
        assert_eq!(req.reason.cr, "CR 404.3");
        assert_eq!(req.min, 3);
        assert_eq!(req.max, 3);
        assert!(!req.hidden, "a graveyard is face up (CR 404.2)");
        assert_eq!(
            req.payload,
            ChoicePayload::OrderCards {
                cards: sorted(&cards),
                destinations: vec![OrderDestination::Graveyard],
            }
        );
        assert_eq!(req.default_on_timeout, gy_answer(&g.default_order));
        assert_eq!(req.validate(&req.default_on_timeout), Ok(()));
    }
}

// REQ-EVENT-001, SRS 15.3 validation, CR 404.3: every permutation validates; a non-permutation,
// a non-empty `top` or `bottom`, a missing, duplicated or foreign card, or another response type
// does not; `order_from_response` agrees.
#[test]
fn req_event_001_request_validation() {
    let mut w = World::new();
    let cards = w.creatures(P1, 3);
    let other = w.create(ZoneKind::Battlefield, P1);
    let batch = all_die(&cards);
    let groups = w.groups(&batch);
    let g = only(&groups);
    let req = g.request(RequestId::new(0));
    for perm in permutations(&cards) {
        assert_eq!(req.validate(&gy_answer(&perm)), Ok(()), "{perm:?}");
        let o = g.order_from_response(&gy_answer(&perm)).expect("valid");
        assert_eq!(o.owner, P1);
        assert_eq!(o.top_to_bottom, perm);
    }
    let (a, b, c) = (cards[0], cards[1], cards[2]);
    let bad = vec![
        gy_answer(&[]),
        gy_answer(&[a, b]),
        gy_answer(&[a, a, b]),
        gy_answer(&[a, b, c, c]),
        gy_answer(&[a, b, other]),
        gy_answer(&[a, b, c, other]),
        gy_answer(&[a, b, GHOST]),
        ChoiceResponse::SubmitOrdering {
            top: vec![a],
            bottom: Vec::new(),
            graveyard: vec![b, c],
        },
        ChoiceResponse::SubmitOrdering {
            top: Vec::new(),
            bottom: vec![c],
            graveyard: vec![a, b],
        },
        ChoiceResponse::SubmitOrdering {
            top: vec![a, b, c],
            bottom: Vec::new(),
            graveyard: Vec::new(),
        },
        ChoiceResponse::SubmitOrdering {
            top: Vec::new(),
            bottom: vec![a, b, c],
            graveyard: Vec::new(),
        },
        ChoiceResponse::Yes,
    ];
    for resp in bad {
        assert!(req.validate(&resp).is_err(), "{resp:?} must be rejected");
        assert!(
            g.order_from_response(&resp).is_err(),
            "order_from_response must reject {resp:?}"
        );
    }
}

// =============================================================================================
// Deliverable 2: three cards, six permutations
// =============================================================================================

// REQ-ZONE-001, REQ-EVENT-001, CR 404.3: with three cards of one owner, all six permutations are
// reachable and each yields exactly that top-to-bottom order; the six states differ.
#[test]
fn req_zone_001_three_cards_all_six_permutations() {
    let mut base = World::new();
    let cards = base.creatures(P1, 3);
    let batch = all_die(&cards);
    let perms = permutations(&cards);
    assert_eq!(perms.len(), 6);
    let mut hashes = Vec::new();
    for perm in &perms {
        let run = drive(&base, &batch, |_, _| Some(gy_answer(perm)));
        assert_eq!(run.requests.len(), 1);
        let gy = run.state.graveyard(P1);
        assert_eq!(gy, new_ids(&run.output, perm), "{perm:?}");
        assert_eq!(olds_of(&run.output, &gy), *perm);

        let (w, done) = ordered(&base, &batch, &[(P1, perm.clone())]);
        assert_eq!(w.snap(), run.state.snap());
        assert_eq!(olds_of(&done, &w.graveyard(P1)), *perm);
        hashes.push(run.hash);
    }
    for i in 0..hashes.len() {
        for j in i + 1..hashes.len() {
            assert_ne!(hashes[i], hashes[j], "perm {i} vs {j}");
        }
    }
}

// =============================================================================================
// Deliverable 3: grouping by owner
// =============================================================================================

// REQ-ZONE-001, CR 404.3: a single card entering a graveyard forms no group.
#[test]
fn req_zone_001_single_card_no_group() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let batch = all_die(&[a]);
    assert!(w.groups(&batch).is_empty());
    let run = drive(&w, &batch, |i, r| panic!("unexpected request {i}: {r:?}"));
    assert_eq!(olds_of(&run.output, &run.state.graveyard(P1)), vec![a]);
}

// REQ-ZONE-001, CR 404.3, 400.3: one card each of two owners goes to two different graveyards:
// no group.
#[test]
fn req_zone_001_two_owners_one_card_each_no_group() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P2);
    let batch = all_die(&[a, b]);
    assert!(w.groups(&batch).is_empty());
    let (after, done) = plain(&w, &batch);
    assert_eq!(olds_of(&done, &after.graveyard(P1)), vec![a]);
    assert_eq!(olds_of(&done, &after.graveyard(P2)), vec![b]);
}

// REQ-ZONE-001, CR 400.3, 404.3: a card controlled by another player goes to its owner's
// graveyard and is grouped with the owner's other cards; the controller gets no group.
#[test]
fn req_zone_001_group_by_owner_not_controller() {
    let mut w = World::new();
    let mine = w.create(ZoneKind::Battlefield, P1);
    let borrowed = w.create_controlled(ZoneKind::Battlefield, P1, Some(P2));
    let theirs = w.create(ZoneKind::Battlefield, P2);
    let batch = all_die(&[mine, borrowed, theirs]);
    let groups = w.groups(&batch);
    let g = only(&groups);
    assert_eq!(g.owner, P1);
    assert_eq!(g.cards, sorted(&[mine, borrowed]));
    let (after, done) = ordered(&w, &batch, &[(P1, vec![mine, borrowed])]);
    assert_eq!(olds_of(&done, &after.graveyard(P1)), vec![mine, borrowed]);
    assert_eq!(olds_of(&done, &after.graveyard(P2)), vec![theirs]);
}

// REQ-ZONE-001, CR 404.3, 400.3: owner A two cards plus owner B one card: one group for A only;
// B's card goes on top of B's graveyard (above B's existing card), unaffected by A's choice.
#[test]
fn req_zone_001_two_plus_one_groups_only_the_pair() {
    let mut w = World::new();
    let old_b = w.create(ZoneKind::Graveyard, P2);
    let a1 = w.create(ZoneKind::Battlefield, P1);
    let b1 = w.create(ZoneKind::Battlefield, P2);
    let a2 = w.create(ZoneKind::Battlefield, P1);
    let batch = all_die(&[a1, b1, a2]);
    let groups = w.groups(&batch);
    let g = only(&groups);
    assert_eq!(g.owner, P1);
    assert_eq!(g.cards, sorted(&[a1, a2]));
    assert_eq!(g.default_order, vec![a2, a1]);
    for perm in permutations(&[a1, a2]) {
        let run = drive(&w, &batch, |_, _| Some(gy_answer(&perm)));
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.requests[0].chooser, P1);
        assert_eq!(olds_of(&run.output, &run.state.graveyard(P1)), perm);
        assert_eq!(
            olds_of(&run.output, &run.state.graveyard(P2)),
            vec![b1, old_b]
        );
    }
}

// REQ-ZONE-001, CR 404.3: owners A and B with two cards each: two groups ascending by player id
// (whatever the submission order), each owner chooses independently; all four combinations.
#[test]
fn req_zone_001_two_owners_two_groups_independent() {
    let mut w = World::new();
    let b1 = w.create(ZoneKind::Battlefield, P2);
    let a1 = w.create(ZoneKind::Battlefield, P1);
    let b2 = w.create(ZoneKind::Battlefield, P2);
    let a2 = w.create(ZoneKind::Battlefield, P1);
    // P2's cards are submitted first.
    let batch = all_die(&[b1, b2, a1, a2]);
    let groups = w.clone().groups(&batch);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].owner, P1);
    assert_eq!(groups[0].cards, sorted(&[a1, a2]));
    assert_eq!(groups[0].default_order, vec![a2, a1]);
    assert_eq!(groups[1].owner, P2);
    assert_eq!(groups[1].cards, sorted(&[b1, b2]));
    assert_eq!(groups[1].default_order, vec![b2, b1]);

    let mut hashes = Vec::new();
    for pa in permutations(&[a1, a2]) {
        for pb in permutations(&[b1, b2]) {
            let run = drive(&w, &batch, |i, req| {
                let (owner, perm) = if i == 0 { (P1, &pa) } else { (P2, &pb) };
                assert_eq!(req.chooser, owner, "request {i}");
                Some(gy_answer(perm))
            });
            assert_eq!(run.requests.len(), 2);
            assert_eq!(olds_of(&run.output, &run.state.graveyard(P1)), pa);
            assert_eq!(olds_of(&run.output, &run.state.graveyard(P2)), pb);

            // Given in either order in the list, the orders mean the same.
            let (x, _) = ordered(&w, &batch, &[(P1, pa.clone()), (P2, pb.clone())]);
            let (y, _) = ordered(&w, &batch, &[(P2, pb.clone()), (P1, pa.clone())]);
            assert_eq!(x.snap(), y.snap());
            assert_eq!(x.snap(), run.state.snap());
            hashes.push(run.hash);
        }
    }
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 4, "four distinct final states");
}

// REQ-ZONE-001, CR 404.3: with three players, groups come ascending by player id and a player
// with a single card gets none.
#[test]
fn req_zone_001_three_players_groups_ascending() {
    let mut w = World::with_players(3);
    let c3 = w.creatures(P3, 2);
    let c2 = w.creatures(P2, 1);
    let c1 = w.creatures(P1, 3);
    let mut all = c3.clone();
    all.extend(&c2);
    all.extend(&c1);
    let batch = all_die(&all);
    let groups = w.groups(&batch);
    let owners: Vec<_> = groups.iter().map(|g| g.owner).collect();
    assert_eq!(owners, vec![P1, P3]);
    assert_eq!(groups[0].cards, sorted(&c1));
    assert_eq!(groups[1].cards, sorted(&c3));
}

// REQ-ZONE-001, CR 404.1, 404.3: cards entering from other zones (discarded from hand, milled
// from library) and dying from the battlefield in one batch are one group.
#[test]
fn req_zone_001_cards_from_any_zone_group() {
    let mut w = World::new();
    let h = w.create(ZoneKind::Hand, P1);
    let l = w.create(ZoneKind::Library, P1);
    let f = w.create(ZoneKind::Battlefield, P1);
    let batch = EventBatch::from_events(vec![
        zc(h, ZoneKind::Graveyard, ZoneChangeCause::Discard),
        zc(l, ZoneKind::Graveyard, ZoneChangeCause::Mill),
        zc(f, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
    ]);
    let groups = w.groups(&batch);
    let g = only(&groups);
    assert_eq!(g.cards, sorted(&[h, l, f]));
    assert_eq!(g.default_order, vec![f, l, h]);
    let (after, done) = ordered(&w, &batch, &[(P1, vec![l, f, h])]);
    assert_eq!(olds_of(&done, &after.graveyard(P1)), vec![l, f, h]);
}

// =============================================================================================
// Deliverable 4: tokens
// =============================================================================================

// REQ-ZONE-001, CR 111.6, 108.2b, 111.7, 404.3: a token dying with two nontoken cards of the same
// owner is not a card: the group is exactly the two cards (not in `cards`, `default_order` or
// the request); once the token ceases to exist (the CR 111.7 state-based action), the cards are
// in the chosen order on top of the graveyard.
#[test]
fn req_zone_001_token_not_in_group() {
    let mut w = World::new();
    let old = w.create(ZoneKind::Graveyard, P1);
    let a = w.create(ZoneKind::Battlefield, P1);
    let t = w.token(P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    assert!(w.is_token(t));
    for submission in [vec![a, t, b], vec![t, a, b], vec![a, b, t]] {
        let batch = all_die(&submission);
        let groups = w.clone().groups(&batch);
        let g = only(&groups);
        assert_eq!(g.owner, P1);
        assert_eq!(g.cards, sorted(&[a, b]));
        assert!(!g.default_order.contains(&t));
        let mut nontoken: Vec<_> = submission.iter().copied().filter(|&x| x != t).collect();
        nontoken.reverse();
        assert_eq!(g.default_order, nontoken);
        let req = g.request(RequestId::new(0));
        match &req.payload {
            ChoicePayload::OrderCards { cards, .. } => assert_eq!(cards, &sorted(&[a, b])),
            other => panic!("unexpected payload {other:?}"),
        }
        assert_eq!(req.min, 2);
        assert_eq!(req.max, 2);
        assert!(req.validate(&gy_answer(&[a, t, b])).is_err());
        assert!(req.validate(&gy_answer(&[a, b])).is_ok());

        for perm in permutations(&[a, b]) {
            let run = drive(&w, &batch, |_, _| Some(gy_answer(&perm)));
            assert_eq!(run.requests.len(), 1);
            let mut after = run.state.clone();
            let t_new = new_ids(&run.output, &[t])[0];
            assert!(after.is_token(t_new));
            // CR 111.7: the token ceases to exist.
            after.zones.cease_to_exist(t_new).expect("token ceases");
            assert!(after.zones.object(t_new).is_none());
            let gy = after.graveyard(P1);
            let mut expected = new_ids(&run.output, &perm);
            expected.push(old);
            assert_eq!(gy, expected, "{submission:?} {perm:?}");
            assert_eq!(after.graveyard_cards(P1), expected);
        }
    }
}

// REQ-ZONE-001, CR 111.6, 108.2b, 404.3: tokens do not count toward two: a token and one card,
// or two tokens and one card, form no group; two tokens alone form none either.
#[test]
fn req_zone_001_tokens_do_not_count_toward_two() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let t1 = w.token(P1);
    let t2 = w.token(P1);
    for objs in [vec![a, t1], vec![t1, a], vec![t1, a, t2], vec![t1, t2]] {
        let batch = all_die(&objs);
        assert!(w.clone().groups(&batch).is_empty(), "{objs:?}");
        let run = drive(&w, &batch, |i, r| panic!("unexpected request {i}: {r:?}"));
        let (p, _) = plain(&w, &batch);
        assert_eq!(run.state.snap(), p.snap());
    }
}

// =============================================================================================
// Deliverable 5: replacement first (CR 614.6)
// =============================================================================================

// REQ-EVENT-001, CR 614.6, 404.3: a batch already reduced by replacement (one card to the
// graveyard and one to exile, or only one card left entering) raises no group.
#[test]
fn req_event_001_post_replacement_batch_by_hand_no_group() {
    let mut w = World::new();
    let ab = w.creatures(P1, 2);
    let (a, b) = (ab[0], ab[1]);
    let one_exiled = EventBatch::from_events(vec![
        dies(a),
        zc(b, ZoneKind::Exile, ZoneChangeCause::StateBasedAction),
    ]);
    let one_left = EventBatch::from_events(vec![dies(a)]);
    let to_hand = EventBatch::from_events(vec![
        zc(a, ZoneKind::Hand, ZoneChangeCause::StateBasedAction),
        dies(b),
    ]);
    for batch in [one_exiled, one_left, to_hand] {
        assert!(w.clone().groups(&batch).is_empty(), "{batch:?}");
        let run = drive(&w, &batch, |i, r| panic!("unexpected request {i}: {r:?}"));
        let (p, _) = plain(&w, &batch);
        assert_eq!(run.state.snap(), p.snap());
    }
}

/// The state a replacement-bus session runs over.
#[derive(Clone)]
struct BusWorld {
    world: World,
    registry: ReplacementRegistry,
}

impl Transactional for BusWorld {
    fn state_hash(&self) -> Digest {
        let mut bytes_all = self.world.canonical();
        bytes_all.extend(bytes(&self.registry));
        fineprint_base::hash::sha256(&bytes_all)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(P1)
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// Runs `batch` through the replacement bus with `effects`, answering no request; returns the
/// requests the bus published (expected none), the report and the final world.
fn through_bus(
    base: &World,
    effects: Vec<ReplacementEffect>,
    batch: &EventBatch,
) -> (
    Vec<PendingChoiceRequest>,
    Result<BusReport, BusError>,
    World,
) {
    let mut registry = ReplacementRegistry::new();
    for e in effects {
        registry.add(e).expect("effect accepted");
    }
    let state = BusWorld {
        world: base.clone(),
        registry,
    };
    let b = batch.clone();
    let proc = move |s: &mut BusWorld, c: &mut ChoiceContext<'_>| {
        let BusWorld { world, registry } = s;
        let mut ctx = world.ctx();
        match process_batch(&mut ctx, registry, &b, true, c) {
            Ok(r) => Ok(Ok(r)),
            Err(BusInterrupt::Pending(p)) => Err(Interrupt::Pending(p)),
            Err(BusInterrupt::Error(e)) => Ok(Err(e)),
        }
    };
    let session = ChoiceSession::start(state, proc).expect("bus session starts");
    let pending: Vec<_> = session.pending().cloned().into_iter().collect();
    match session.status() {
        Status::Done { state, output } => (pending, output.clone(), state.world.clone()),
        Status::Pending(r) => panic!("the bus asked {r:?}"),
    }
}

// REQ-EVENT-001, CR 614.6, 404.3, 704.3: through the replacement bus, a state-based batch of two
// cards dying where a replacement sends one to exile instead: the bus asks no ORDER_CARDS
// question, the final (replaced) events raise no group, and one card reaches the graveyard.
#[test]
fn req_event_001_post_replacement_batch_through_bus_no_group() {
    let mut w = World::new();
    let ab = w.creatures(P1, 2);
    let (a, b) = (ab[0], ab[1]);
    let batch = all_die(&ab);
    let effect = ReplacementEffect::new(
        EffectId::new(1),
        EventMatcher {
            affected_object: Some(b),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
    );
    let (requests, report, after) = through_bus(&w, vec![effect], &batch);
    assert!(
        requests
            .iter()
            .all(|r| r.choice_type != ChoiceType::OrderCards),
        "no ORDER_CARDS request: {requests:?}"
    );
    assert!(requests.is_empty(), "nothing to choose: {requests:?}");
    let report = report.expect("bus succeeds");
    let executed: Vec<EventInstance> = report
        .executed
        .iter()
        .flat_map(|x| x.events.iter().map(|e| e.instance.clone()))
        .collect();
    let replaced = EventBatch::new(executed);
    assert_eq!(replaced.len(), 2);
    assert!(w.clone().groups(&replaced).is_empty(), "{replaced:?}");
    let gy = after.graveyard(P1);
    assert_eq!(gy.len(), 1);
    assert_eq!(after.zones.contents(ZoneId::Exile).len(), 1);
    let done = &report.executed[0];
    assert_eq!(olds_of(done, &gy), vec![a]);

    // Control: with no replacement, the same bus batch's final events form the group.
    let (_, report, _) = through_bus(&w, Vec::new(), &batch);
    let report = report.expect("bus succeeds");
    let executed: Vec<EventInstance> = report
        .executed
        .iter()
        .flat_map(|x| x.events.iter().map(|e| e.instance.clone()))
        .collect();
    let groups = w.clone().groups(&EventBatch::new(executed));
    assert_eq!(only(&groups).cards, sorted(&ab));
}

// =============================================================================================
// Deliverable 6: existing contents and explicit positions
// =============================================================================================

// REQ-ZONE-001, CR 404.1, 400.5: cards already in the graveyard stay below the batch's cards,
// in their order, whatever order is chosen.
#[test]
fn req_zone_001_existing_contents_stay_below_in_order() {
    let mut w = World::new();
    for _ in 0..3 {
        w.create(ZoneKind::Graveyard, P1);
    }
    let before = w.graveyard(P1);
    let cards = w.creatures(P1, 3);
    let batch = all_die(&cards);
    for perm in permutations(&cards) {
        let (after, done) = ordered(&w, &batch, &[(P1, perm.clone())]);
        let mut expected = new_ids(&done, &perm);
        expected.extend(&before);
        assert_eq!(after.graveyard(P1), expected, "{perm:?}");
    }
}

// REQ-ZONE-001, CR 404.3, 400.5: moves with `Position::Bottom` are not grouped and keep their
// requested position (the bottom), below the existing contents.
#[test]
fn req_zone_001_bottom_moves_not_grouped() {
    let mut w = World::new();
    let g1 = w.create(ZoneKind::Graveyard, P1);
    let abc = w.creatures(P1, 3);
    let (a, b, c) = (abc[0], abc[1], abc[2]);

    // Two Bottom moves alone: no group.
    let bottoms = EventBatch::from_events(vec![
        zc_at(a, ZoneKind::Graveyard, Position::Bottom),
        zc_at(b, ZoneKind::Graveyard, Position::Bottom),
    ]);
    assert!(w.clone().groups(&bottoms).is_empty());
    let (after, done) = plain(&w, &bottoms);
    let gy = olds_of(&done, &after.graveyard(P1));
    assert_eq!(gy[0], g1);
    assert_eq!(sorted(&gy[1..]), sorted(&[a, b]));

    // Two Top moves and one Bottom move: the group is the Top pair; the Bottom card is last.
    let mixed = EventBatch::from_events(vec![
        dies(a),
        zc_at(c, ZoneKind::Graveyard, Position::Bottom),
        dies(b),
    ]);
    let groups = w.clone().groups(&mixed);
    let g = only(&groups);
    assert_eq!(g.cards, sorted(&[a, b]));
    assert_eq!(g.default_order, vec![b, a]);
    for perm in permutations(&[a, b]) {
        let (after, done) = ordered(&w, &mixed, &[(P1, perm.clone())]);
        let mut expected = perm.clone();
        expected.extend([g1, c]);
        assert_eq!(olds_of(&done, &after.graveyard(P1)), expected);
    }
}

// REQ-ZONE-001, CR 404.3, 400.5: moves with `Position::Index` are not grouped and keep their
// requested position as `execute_batch` places them.
#[test]
fn req_zone_001_index_moves_not_grouped() {
    let mut w = World::new();
    for _ in 0..3 {
        w.create(ZoneKind::Graveyard, P1);
    }
    let abc = w.creatures(P1, 3);
    let (a, b, c) = (abc[0], abc[1], abc[2]);
    let indexed = EventBatch::from_events(vec![
        zc_at(a, ZoneKind::Graveyard, Position::Index(1)),
        zc_at(b, ZoneKind::Graveyard, Position::Index(2)),
    ]);
    assert!(w.clone().groups(&indexed).is_empty());
    let (p, pd) = plain(&w, &indexed);
    let (o, od) = ordered(&w, &indexed, &[]);
    assert_eq!(o.snap(), p.snap());
    assert_eq!(od, pd);
    let gy = p.graveyard(P1);
    assert_ne!(gy[0], new_ids(&pd, &[a])[0], "not moved to the top");

    let mixed = EventBatch::from_events(vec![
        dies(a),
        zc_at(c, ZoneKind::Graveyard, Position::Index(1)),
        dies(b),
    ]);
    let groups = w.clone().groups(&mixed);
    let g = only(&groups);
    assert_eq!(g.cards, sorted(&[a, b]));
    let (p, _) = plain(&w, &mixed);
    let (o, _) = ordered(&w, &mixed, &[(P1, g.default_order.clone())]);
    assert_eq!(o.snap(), p.snap());
}

// REQ-ZONE-001, CR 400.5, 404.3: a graveyard-to-graveyard reposition is not a card entering the
// graveyard: it neither forms nor joins a group.
#[test]
fn req_zone_001_graveyard_reposition_not_grouped() {
    let mut w = World::new();
    let g1 = w.create(ZoneKind::Graveyard, P1);
    let g2 = w.create(ZoneKind::Graveyard, P1);
    let ab = w.creatures(P1, 2);
    let (a, b) = (ab[0], ab[1]);

    let one = EventBatch::from_events(vec![dies(g1), dies(a)]);
    assert!(w.clone().groups(&one).is_empty());

    let two = EventBatch::from_events(vec![dies(a), dies(g1), dies(b)]);
    let groups = w.clone().groups(&two);
    let g = only(&groups);
    assert_eq!(g.cards, sorted(&[a, b]));
    assert!(!g.default_order.contains(&g1));

    let repos_only = EventBatch::from_events(vec![dies(g1), dies(g2)]);
    assert!(w.clone().groups(&repos_only).is_empty());
}

// =============================================================================================
// Deliverable 7: default and timeout
// =============================================================================================

// REQ-EVENT-001, CR 404.3: `default_on_timeout` is `default_order`; answering every request by
// `submit_default` (the caller's timeout) gives the same final state as `execute_batch`.
#[test]
fn req_event_001_timeout_default_equals_execute_batch() {
    let mut w = World::new();
    w.create(ZoneKind::Graveyard, P2);
    let c1 = w.creatures(P1, 3);
    let c2 = w.creatures(P2, 2);
    let mut all = Vec::new();
    for i in 0..3 {
        all.push(c1[i]);
        if i < 2 {
            all.push(c2[i]);
        }
    }
    let batch = all_die(&all);
    let groups = w.clone().groups(&batch);
    assert_eq!(groups.len(), 2);
    for g in &groups {
        let req = g.request(RequestId::new(0));
        assert_eq!(req.default_on_timeout, gy_answer(&g.default_order));
    }
    let run = drive(&w, &batch, |_, _| None);
    assert_eq!(run.requests.len(), 2);
    for (req, g) in run.requests.iter().zip(&groups) {
        assert_eq!(req.default_on_timeout, gy_answer(&g.default_order));
    }
    let (p, pd) = plain(&w, &batch);
    assert_eq!(run.state.snap(), p.snap());
    assert_eq!(run.output, pd);
    for g in &groups {
        let gy = olds_of(&pd, &p.graveyard(g.owner));
        assert_eq!(&gy[..g.default_order.len()], &g.default_order[..]);
    }
}

// =============================================================================================
// Deliverable 8: invalid orders
// =============================================================================================

// REQ-EVENT-001, CR 404.3: an entry for an owner with no group, an owner given twice, or a list
// that is not a permutation of the group's cards is `InvalidGraveyardOrder { owner }` and the
// whole context (zones, ids, log, every component) is unchanged.
#[test]
fn req_event_001_invalid_orders_rejected_and_nothing_changes() {
    let mut w = World::new();
    w.create(ZoneKind::Graveyard, P1);
    let c1 = w.creatures(P1, 3);
    let c2 = w.creatures(P2, 2);
    let lone = w.create(ZoneKind::Battlefield, P1);
    let (a, b, c) = (c1[0], c1[1], c1[2]);
    let (x, y) = (c2[0], c2[1]);
    let mut all = c1.clone();
    all.extend(&c2);
    let batch = all_die(&all);

    let cases: Vec<(OrderList, PlayerId)> = vec![
        // Owner with no group (a known player and an unknown one).
        (vec![(NOBODY, vec![a, b, c])], NOBODY),
        (vec![(P1, vec![a, b, c]), (NOBODY, vec![])], NOBODY),
        // Owner given twice.
        (vec![(P1, vec![a, b, c]), (P1, vec![a, b, c])], P1),
        (
            vec![(P1, vec![c, b, a]), (P2, vec![x, y]), (P1, vec![a, b, c])],
            P1,
        ),
        // Not a permutation.
        (vec![(P1, vec![])], P1),
        (vec![(P1, vec![a, b])], P1),
        (vec![(P1, vec![a, a, b])], P1),
        (vec![(P1, vec![a, b, c, c])], P1),
        (vec![(P1, vec![a, b, lone])], P1),
        (vec![(P1, vec![a, b, c, lone])], P1),
        (vec![(P1, vec![a, b, GHOST])], P1),
        (vec![(P1, vec![a, b, x])], P1),
        (vec![(P1, vec![a, b, c]), (P2, vec![x])], P2),
        (vec![(P1, vec![a, b, c]), (P2, vec![x, a])], P2),
    ];
    let before = w.snap();
    for (orders, owner) in &cases {
        let list: Vec<_> = orders.iter().map(|(p, o)| order(*p, o)).collect();
        assert_eq!(
            w.run_ordered(&batch, &list),
            Err(EventError::InvalidGraveyardOrder { owner: *owner }),
            "{list:?}"
        );
        assert_eq!(w.snap(), before, "context changed by {list:?}");
    }

    // A batch with no group at all: any entry is rejected.
    let single = all_die(&[lone]);
    for p in [P1, P2] {
        assert_eq!(
            w.run_ordered(&single, &[order(p, &[lone])]),
            Err(EventError::InvalidGraveyardOrder { owner: p })
        );
        assert_eq!(w.snap(), before);
    }
    // The group of a card not in this batch's group (P2 has none here).
    let only_p1 = all_die(&c1);
    assert_eq!(
        w.run_ordered(&only_p1, &[order(P1, &[a, b, c]), order(P2, &[x, y])]),
        Err(EventError::InvalidGraveyardOrder { owner: P2 })
    );
    assert_eq!(w.snap(), before);
    // The display names something.
    assert!(
        !EventError::InvalidGraveyardOrder { owner: P1 }
            .to_string()
            .is_empty()
    );
}

// =============================================================================================
// Deliverable 9 and clause d: existing behaviour unchanged
// =============================================================================================

/// A set of batches covering groups, no groups, tokens and positions.
fn sample_batches(w: &mut World) -> Vec<EventBatch> {
    w.create(ZoneKind::Graveyard, P1);
    w.create(ZoneKind::Graveyard, P2);
    let c1 = w.creatures(P1, 3);
    let c2 = w.creatures(P2, 2);
    let t = w.token(P1);
    let h = w.create(ZoneKind::Hand, P2);
    vec![
        all_die(&c1),
        all_die(&[c1[0]]),
        all_die(&[c1[0], c2[0]]),
        all_die(&[c2[1], c1[2], t, c1[0], c2[0]]),
        EventBatch::from_events(vec![
            dies(c1[1]),
            zc_at(c1[0], ZoneKind::Graveyard, Position::Bottom),
            zc(h, ZoneKind::Graveyard, ZoneChangeCause::Discard),
            dies(c2[0]),
            dies(c1[2]),
            zc(c2[1], ZoneKind::Exile, ZoneChangeCause::Exile),
        ]),
        EventBatch::from_events(vec![
            GameEvent::LifeChange {
                player: P1,
                delta: -2,
            },
            dies(c1[0]),
            dies(c1[1]),
        ]),
    ]
}

// REQ-EVENT-001, CR 404.3: `execute_batch` equals `execute_batch_ordered(&batch, &[])` and
// `execute_batch_ordered` with every group's `default_order`: the same executed batch and the
// same state; and its graveyard order is `default_order`.
#[test]
fn req_event_001_execute_batch_equals_ordered_with_defaults() {
    let mut w = World::new();
    for batch in sample_batches(&mut w) {
        let mut probe = w.clone();
        let groups = probe.groups(&batch);
        assert_eq!(
            probe.snap(),
            w.snap(),
            "graveyard_order_groups changes nothing"
        );

        let (p, pd) = plain(&w, &batch);
        let (o, od) = ordered(&w, &batch, &[]);
        assert_eq!(od, pd, "{batch:?}");
        assert_eq!(o.snap(), p.snap());
        let defaults: Vec<_> = groups
            .iter()
            .map(|g| (g.owner, g.default_order.clone()))
            .collect();
        let (d, dd) = ordered(&w, &batch, &defaults);
        assert_eq!(dd, pd);
        assert_eq!(d.snap(), p.snap());
        for g in &groups {
            let gy = olds_of(&pd, &p.graveyard_cards(g.owner));
            assert_eq!(&gy[..g.default_order.len()], &g.default_order[..]);
        }
    }
}

// REQ-EVENT-001, CR 404.3, 111.7 (brief clause d): with a token submitted between the two cards
// of a group, `execute_batch_ordered` with the group's `default_order` given explicitly yields
// the same state as `execute_batch` and as `execute_batch_ordered(&batch, &[])` (a group with no
// entry uses `default_order`), the token's position included.
#[test]
fn req_event_001_explicit_default_with_interleaved_token_equals_execute_batch() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let t = w.token(P1);
    let b = w.create(ZoneKind::Battlefield, P1);
    let batch = all_die(&[a, t, b]);
    let groups = w.clone().groups(&batch);
    let g = only(&groups);
    assert_eq!(g.default_order, vec![b, a]);
    let (p, pd) = plain(&w, &batch);
    let (n, nd) = ordered(&w, &batch, &[]);
    let (d, dd) = ordered(&w, &batch, &[(P1, g.default_order.clone())]);
    assert_eq!(nd, pd);
    assert_eq!(n.snap(), p.snap());
    assert_eq!(dd, pd);
    assert_eq!(
        olds_of(&dd, &d.graveyard(P1)),
        olds_of(&pd, &p.graveyard(P1)),
        "explicit default_order must give execute_batch's graveyard"
    );
    assert_eq!(d.snap(), p.snap());
}

// REQ-EVENT-001, CR 404.1: executing the moves one by one with `execute` puts them on top in
// order too: the result is the batch's `default_order`.
#[test]
fn req_event_001_execute_one_by_one_matches_default_order() {
    let mut w = World::new();
    let cards = w.creatures(P1, 3);
    let groups = w.clone().groups(&all_die(&cards));
    let mut seq = w.clone();
    let mut new = Vec::new();
    for &c in &cards {
        let done = seq.ok(dies(c));
        match done.outcome {
            fineprint_core::events::Outcome::ZoneChange(m) => new.push((m.new, m.old)),
            other => panic!("unexpected {other:?}"),
        }
    }
    let gy: Vec<ObjectId> = seq
        .graveyard(P1)
        .iter()
        .map(|id| new.iter().find(|(n, _)| n == id).expect("moved").1)
        .collect();
    assert_eq!(gy, only(&groups).default_order);
}

// REQ-EVENT-001, clause c: `ExecutedBatch.events` keeps submission order with the same events
// `execute_batch` logs (new ids aside) for every chosen order; LKI is captured for every mover.
#[test]
fn req_event_001_ordered_batch_logs_the_same_events() {
    let mut w = World::new();
    for batch in sample_batches(&mut w) {
        let groups = w.clone().groups(&batch);
        let (p, pd) = plain(&w, &batch);
        let orders: Vec<_> = groups
            .iter()
            .map(|g| (g.owner, reversed(&g.default_order)))
            .collect();
        let (o, od) = ordered(&w, &batch, &orders);
        assert_eq!(shape(&od), shape(&pd));
        for (e, i) in od.events.iter().zip(batch.events()) {
            assert_eq!(&e.instance, i, "submission order kept");
        }
        assert_eq!(od.index, pd.index);
        assert_eq!(od.turn, pd.turn);
        let o_log = o.events.log().batches();
        let p_log = p.events.log().batches();
        assert_eq!(o_log.len(), p_log.len());
        if let Some(last) = o_log.last() {
            assert_eq!(shape(last), shape(&pd));
            assert_eq!(last, &od);
        }
        for g in &groups {
            let gy = olds_of(&od, &o.graveyard_cards(g.owner));
            let want = reversed(&g.default_order);
            assert_eq!(&gy[..want.len()], &want[..]);
        }
    }
}

// =============================================================================================
// Clause g: determinism
// =============================================================================================

// REQ-EVENT-001, REQ-ZONE-001, CR 404.3: the same batch with the same orders on clones of one
// base gives identical zones, identical canonical encodings and identical results over ten
// runs; through a session, identical state hashes.
#[test]
fn req_event_001_ten_runs_identical() {
    let mut base = World::new();
    base.create(ZoneKind::Graveyard, P1);
    let c1 = base.creatures(P1, 3);
    let c2 = base.creatures(P2, 2);
    let t = base.token(P2);
    let mut all = c2.clone();
    all.push(t);
    all.extend(&c1);
    let batch = all_die(&all);
    let orders = vec![(P1, vec![c1[1], c1[2], c1[0]]), (P2, vec![c2[0], c2[1]])];
    let (w0, d0) = ordered(&base, &batch, &orders);
    let r0 = drive(&base, &batch, |i, _| Some(gy_answer(&orders[i].1)));
    assert_eq!(r0.state.snap(), w0.snap());
    for _ in 0..10 {
        let (w, d) = ordered(&base, &batch, &orders);
        assert_eq!(w.snap(), w0.snap());
        assert_eq!(bytes(&w.zones), bytes(&w0.zones));
        assert_eq!(bytes(&d), bytes(&d0));
        assert_eq!(d, d0);
        let r = drive(&base, &batch, |i, _| Some(gy_answer(&orders[i].1)));
        assert_eq!(r.hash, r0.hash);
        assert_eq!(r.state.state_hash(), r0.state.state_hash());
        assert_eq!(bytes(&r.output), bytes(&r0.output));
    }
}

// =============================================================================================
// Gate 06-turn F7: the preflight captures last known information
// =============================================================================================

/// A provider that cannot evaluate one object and otherwise answers as the world's provider.
struct MissingOne<'a> {
    inner: &'a TestProvider,
    missing: ObjectId,
}

impl fineprint_core::memory::CharacteristicsProvider for MissingOne<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<fineprint_core::objects::Characteristics> {
        if id == self.missing {
            None
        } else {
            self.inner.characteristics(id)
        }
    }
}

/// `EventContext::graveyard_order_groups` over `w` with a provider that cannot evaluate
/// `missing`.
fn groups_missing(
    w: &mut World,
    batch: &EventBatch,
    missing: ObjectId,
) -> Result<Vec<GraveyardOrderGroup>, EventError> {
    let inner = w.provider.clone();
    let provider = MissingOne {
        inner: &inner,
        missing,
    };
    let mut ctx = fineprint_core::events::EventContext {
        game: &mut w.game,
        zones: &mut w.zones,
        ids: &mut w.ids,
        permanents: &mut w.permanents,
        memories: &mut w.memories,
        lki: &mut w.lki,
        events: &mut w.events,
        provider: &provider,
    };
    ctx.graveyard_order_groups(batch)
}

// REQ-ZONE-001, REQ-EVENT-001, REQ-CORE-003, CR 404.3, CR 704.8, CR 603.10a (Gate 06-turn F7):
// two nontoken cards of one owner die together, but the provider cannot evaluate one of them,
// so the batch's LKI capture fails. `graveyard_order_groups` returns that error
// (`NoCharacteristics`) instead of offering a group that every answer to `execute_batch_ordered`
// would fail, and leaves every component unchanged; the same batch with a provider that can
// evaluate both still yields its group, with the same cards and default order.
#[test]
fn req_zone_001_preflight_lki_failure_offers_no_group() {
    let mut base = World::new();
    let ab = base.creatures(P1, 2);
    let batch = all_die(&ab);
    for &missing in &ab {
        let mut w = base.clone();
        let before = w.snap();
        assert_eq!(
            groups_missing(&mut w, &batch, missing),
            Err(EventError::Memory(
                fineprint_core::memory::MemoryError::NoCharacteristics(missing)
            )),
            "no group for a batch whose LKI capture fails"
        );
        assert_eq!(w.snap(), before, "every component unchanged");
    }
    // Control: the evaluable batch keeps its group exactly.
    let mut w = base.clone();
    let before = w.snap();
    let groups = w.groups(&batch);
    let group = only(&groups);
    assert_eq!(group.owner, P1);
    assert_eq!(group.cards, sorted(&ab));
    assert_eq!(group.default_order, vec![ab[1], ab[0]]);
    assert_eq!(w.snap(), before, "every component unchanged");
}
