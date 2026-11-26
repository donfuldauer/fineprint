// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of simultaneous graveyard entry (#208 FP-154): REQ-ZONE-001 (graveyard order is
// preserved and chosen per the CR), REQ-EVENT-001 (one batch, all-or-nothing, replacements
// applied before the order choice), SRS 15.3 ORDER_CARDS. CR 404.3, CR 404.1, CR 400.3,
// CR 111.7, CR 111.6, CR 108.2b. Synthetic ids only.

use fineprint_base::encode::encode_to_vec;

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceType, Interrupt, OrderDestination, RequestId, Status, ValidatorRule,
};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

/// A world with `n` permanents of `owner` on the battlefield, cards `first..first + n`.
fn creatures(w: &mut World, owner: u64, first: u64, n: u64) -> Vec<ObjectId> {
    (first..first + n)
        .map(|card| w.create(card, owner, ZoneKind::Battlefield))
        .collect()
}

fn token(w: &mut World, owner: u64, card: u64) -> ObjectId {
    let ex = w
        .exec(GameEvent::CreateToken {
            owner: p(owner),
            card: c(card),
            controller: None,
        })
        .unwrap()
        .unwrap();
    match ex.outcome {
        Outcome::TokenCreated { object } => object,
        other => panic!("expected a token, got {other:?}"),
    }
}

fn to_gy_at(object: ObjectId, position: Position) -> GameEvent {
    let mut request = MoveRequest::new(ZoneKind::Graveyard);
    request.position = position;
    GameEvent::ZoneChange {
        object,
        request,
        cause: ZoneChangeCause::Other,
    }
}

fn batch_of(events: Vec<GameEvent>) -> EventBatch {
    EventBatch::from_events(events)
}

fn groups(w: &mut World, batch: &EventBatch) -> Vec<GraveyardOrderGroup> {
    w.ctx().graveyard_order_groups(batch).unwrap()
}

fn graveyard(w: &World, owner: u64) -> Vec<ObjectId> {
    w.zones.contents(ZoneId::Graveyard(p(owner)))
}

/// The cards (CardIds) of `owner`'s graveyard, top first.
fn graveyard_cards(w: &World, owner: u64) -> Vec<CardId> {
    graveyard(w, owner)
        .into_iter()
        .map(|id| w.zones.object(id).unwrap().card)
        .collect()
}

/// The card each pre-move object of `w` stands for.
fn card_of(w: &World, id: ObjectId) -> CardId {
    w.zones.object(id).unwrap().card
}

fn ordering(graveyard: Vec<ObjectId>) -> ChoiceResponse {
    ChoiceResponse::SubmitOrdering {
        top: Vec::new(),
        bottom: Vec::new(),
        graveyard,
    }
}

/// Every permutation of `items`, in lexicographic order of positions.
fn permutations(items: &[ObjectId]) -> Vec<Vec<ObjectId>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head);
            out.push(tail);
        }
    }
    out
}

/// The CR 404.3 procedure as a caller wires it: groups of the final batch, one ORDER_CARDS ask
/// per owner in ascending PlayerId order, then the ordered execution.
fn ordered_procedure(
    batch: EventBatch,
) -> impl Fn(&mut World, &mut ChoiceContext<'_>) -> Result<ExecutedBatch, Interrupt> {
    move |w: &mut World, ch: &mut ChoiceContext<'_>| {
        let fail = |e: EventError| Interrupt::Error(ChoiceError::Procedure(e.to_string()));
        let groups = w.ctx().graveyard_order_groups(&batch).map_err(fail)?;
        let mut orders = Vec::new();
        for group in &groups {
            let answer = ch.ask(group.request(ch.next_request_id()))?;
            orders.push(group.order_from_response(&answer).map_err(fail)?);
        }
        w.ctx().execute_batch_ordered(&batch, &orders).map_err(fail)
    }
}

fn done(status: &Status<World, ExecutedBatch>) -> (&World, &ExecutedBatch) {
    match status {
        Status::Done { state, output } => (state, output),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Grouping (CR 404.3, CR 400.3, CR 111.7)
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. Two nontoken cards of one owner put into that owner's
// graveyard in one batch form one group; the default is the execute_batch result (the
// last-submitted card on top).
#[test]
fn req_zone_001_cr_404_3_two_cards_form_one_group() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 2);
    let batch = batch_of(vec![destroy(ids[0]), destroy(ids[1])]);
    let before = w.parts();
    let g = groups(&mut w, &batch);
    // graveyard_order_groups leaves every component unchanged.
    assert!(w.parts() == before);
    assert_eq!(
        g,
        vec![GraveyardOrderGroup {
            owner: p(1),
            cards: vec![ids[0], ids[1]],
            default_order: vec![ids[1], ids[0]],
        }]
    );
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. A single card raises no choice.
#[test]
fn req_zone_001_cr_404_3_single_card_no_group() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 1);
    assert!(groups(&mut w, &batch_of(vec![destroy(ids[0])])).is_empty());
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3, CR 400.3. Cards of different owners go to separate
// graveyards (each one card): no choice. Two cards each: two groups, ascending owner.
#[test]
fn req_zone_001_cr_404_3_separate_graveyards() {
    let mut w = World::new();
    let a = creatures(&mut w, 2, 1, 1);
    let b = creatures(&mut w, 1, 2, 1);
    assert!(groups(&mut w, &batch_of(vec![destroy(a[0]), destroy(b[0])])).is_empty());

    let mut w = World::new();
    let two = creatures(&mut w, 2, 1, 2);
    let one = creatures(&mut w, 1, 3, 2);
    let g = groups(
        &mut w,
        &batch_of(vec![
            destroy(two[0]),
            destroy(one[0]),
            destroy(two[1]),
            destroy(one[1]),
        ]),
    );
    assert_eq!(g.len(), 2);
    assert_eq!((g[0].owner, g[1].owner), (p(1), p(2)));
    assert_eq!(g[0].cards, one);
    assert_eq!(g[1].cards, two);
}

// REQ-ZONE-001, REQ-EVENT-001: CR 400.3. A card controlled by another player goes to its
// owner's graveyard, so it joins its owner's group.
#[test]
fn req_zone_001_cr_400_3_owner_not_controller_groups() {
    let mut w = World::new();
    let mine = creatures(&mut w, 1, 1, 1);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(p(2));
    req.position = Position::Bottom;
    let theirs = w.zones.create_object(&mut w.ids, c(2), p(1), req).unwrap();
    w.permanents.enter(theirs, p(2), 1).unwrap();
    let g = groups(&mut w, &batch_of(vec![destroy(mine[0]), destroy(theirs)]));
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].owner, p(1));
    assert_eq!(g[0].cards, vec![mine[0], theirs]);
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3, CR 111.7, CR 111.6, CR 108.2b. A token is not a card:
// token plus two cards forms a group of the two cards; token plus one card forms none.
#[test]
fn req_zone_001_cr_404_3_cr_111_7_tokens_excluded() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 2);
    let t = token(&mut w, 1, 50);
    let g = groups(
        &mut w,
        &batch_of(vec![destroy(ids[0]), destroy(t), destroy(ids[1])]),
    );
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].cards, ids);
    assert_eq!(g[0].default_order, vec![ids[1], ids[0]]);

    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 1);
    let t = token(&mut w, 1, 50);
    assert!(groups(&mut w, &batch_of(vec![destroy(t), destroy(ids[0])])).is_empty());
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. A move with Position::Bottom or Position::Index keeps
// its requested position and takes no part; a graveyard-to-graveyard reposition is not "put
// into" a graveyard; a draw is not a graveyard move.
#[test]
fn req_zone_001_cr_404_3_non_top_and_repositions_not_grouped() {
    // One Top and one Bottom: no group.
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 2);
    assert!(
        groups(
            &mut w,
            &batch_of(vec![destroy(ids[0]), to_gy_at(ids[1], Position::Bottom)])
        )
        .is_empty()
    );
    // Two Top and one Index: a group of the two Top cards.
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 3);
    let g = groups(
        &mut w,
        &batch_of(vec![
            destroy(ids[0]),
            to_gy_at(ids[1], Position::Index(1)),
            destroy(ids[2]),
        ]),
    );
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].cards, vec![ids[0], ids[2]]);
    // One card entering plus one graveyard card repositioned on top: no group.
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 1);
    let old = w.create(9, 1, ZoneKind::Graveyard);
    assert!(groups(&mut w, &batch_of(vec![destroy(ids[0]), destroy(old)])).is_empty());
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3, CR 614.6. The batch reaching the executor is final:
// after a replacement sent one of two dying cards to exile, only one card enters the graveyard
// and no choice is raised.
#[test]
fn req_event_001_cr_404_3_replacement_reduced_batch_no_group() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 2);
    let batch = batch_of(vec![destroy(ids[0]), zc(ids[1], ZoneKind::Exile)]);
    assert!(groups(&mut w, &batch).is_empty());
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. A card put into the graveyard and moved out again by a
// later event of the same batch is not part of the arrangement.
#[test]
fn req_zone_001_cr_404_3_card_leaving_again_not_grouped() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 2);
    // Predict the graveyard id of the first card: the next id the generator issues.
    let batch_plain = batch_of(vec![destroy(ids[0])]);
    let mut probe = w.clone();
    let first_new = moved_of(&probe.ctx().execute_batch(&batch_plain).unwrap().events[0]).new;
    let batch = batch_of(vec![
        destroy(ids[0]),
        zc(first_new, ZoneKind::Exile),
        destroy(ids[1]),
    ]);
    assert!(groups(&mut w, &batch).is_empty());
}

// REQ-EVENT-001: graveyard_order_groups reports a failing batch's error and changes nothing.
#[test]
fn req_event_001_groups_of_failing_batch_errs_unchanged() {
    let mut w = World::new();
    let ids = creatures(&mut w, 1, 1, 1);
    let before = w.parts();
    let err = w
        .ctx()
        .graveyard_order_groups(&batch_of(vec![
            destroy(ids[0]),
            destroy(ObjectId::new(999)),
        ]))
        .unwrap_err();
    assert_eq!(err, EventError::UnknownObject(ObjectId::new(999)));
    assert!(w.parts() == before);
}

// ---------------------------------------------------------------------------------------------
// The ORDER_CARDS request (SRS 15.3)
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, REQ-EVENT-001: SRS 15.3 ORDER_CARDS, CR 404.3. The request's fields, its
// default and every permutation validate; anything else is rejected by validate and by
// order_from_response.
#[test]
fn req_zone_001_cr_404_3_request_shape() {
    let group = GraveyardOrderGroup {
        owner: p(2),
        cards: vec![ObjectId::new(3), ObjectId::new(5), ObjectId::new(8)],
        default_order: vec![ObjectId::new(8), ObjectId::new(5), ObjectId::new(3)],
    };
    let r = group.request(RequestId::new(4));
    assert_eq!(r.request_id, RequestId::new(4));
    assert_eq!(r.chooser, p(2));
    assert_eq!(r.choice_type, ChoiceType::OrderCards);
    assert_eq!(r.reason.cr, "CR 404.3");
    assert_eq!(r.reason.source, None);
    assert_eq!((r.min, r.max), (3, 3));
    assert!(!r.hidden);
    assert!(r.reveal_to.is_empty());
    assert_eq!(r.default_on_timeout, ordering(group.default_order.clone()));
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(
            group
                .cards
                .iter()
                .map(|&o| ChoiceOption::Object(o))
                .collect()
        )
    );
    assert_eq!(
        r.payload,
        ChoicePayload::OrderCards {
            cards: group.cards.clone(),
            destinations: vec![OrderDestination::Graveyard],
        }
    );
    // Pure: the same group and id give the same request (replay digest).
    assert_eq!(r.digest(), group.request(RequestId::new(4)).digest());
    assert!(r.validate(&r.default_on_timeout).is_ok());
    for perm in permutations(&group.cards) {
        let answer = ordering(perm.clone());
        assert!(r.validate(&answer).is_ok());
        assert_eq!(
            group.order_from_response(&answer).unwrap(),
            GraveyardOrder {
                owner: p(2),
                top_to_bottom: perm,
            }
        );
    }
    let bad = [
        ChoiceResponse::SubmitOrdering {
            top: group.cards.clone(),
            bottom: Vec::new(),
            graveyard: Vec::new(),
        },
        ChoiceResponse::SubmitOrdering {
            top: Vec::new(),
            bottom: vec![ObjectId::new(3)],
            graveyard: vec![ObjectId::new(5), ObjectId::new(8)],
        },
        ordering(vec![ObjectId::new(3), ObjectId::new(5)]),
        ordering(vec![ObjectId::new(3), ObjectId::new(5), ObjectId::new(5)]),
        ordering(vec![ObjectId::new(3), ObjectId::new(5), ObjectId::new(9)]),
        ChoiceResponse::No,
    ];
    for b in &bad {
        assert!(r.validate(b).is_err(), "{b:?}");
        assert_eq!(
            group.order_from_response(b),
            Err(EventError::InvalidGraveyardOrder { owner: p(2) })
        );
    }
}

// REQ-ZONE-001: SRS 15.3, CR 404.3. At or above ENUMERATION_LIMIT orders (8! = 40320) the
// request carries a Permutation validator contract, like a generated ORDER choice.
#[test]
fn req_zone_001_cr_404_3_large_group_uses_validator() {
    let cards: Vec<ObjectId> = (1..=8).map(ObjectId::new).collect();
    let mut default_order = cards.clone();
    default_order.reverse();
    let group = GraveyardOrderGroup {
        owner: p(1),
        cards: cards.clone(),
        default_order,
    };
    let r = group.request(RequestId::new(0));
    match &r.options {
        ChoiceOptions::Validator(v) => {
            assert_eq!((v.min, v.max), (8, 8));
            assert_eq!(v.rule, ValidatorRule::Permutation { items: cards });
        }
        other => panic!("expected a validator, got {other:?}"),
    }
    assert!(r.validate(&r.default_on_timeout).is_ok());
    // 7 cards: 5040 orders, still enumerated.
    let small = GraveyardOrderGroup {
        owner: p(1),
        cards: (1..=7).map(ObjectId::new).collect(),
        default_order: (1..=7).rev().map(ObjectId::new).collect(),
    };
    assert!(matches!(
        small.request(RequestId::new(0)).options,
        ChoiceOptions::Enumerated(_)
    ));
}

// ---------------------------------------------------------------------------------------------
// Ordered execution
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. Two cards of one owner: both orders are reachable
// through the choice protocol, and replaying the same answers on a clone of the base gives
// identical zones, an identical state hash and an identical ExecutedBatch encoding.
#[test]
fn req_zone_001_req_event_001_cr_404_3_two_cards_both_orders_and_replay() {
    let mut base = World::new();
    let ids = creatures(&mut base, 1, 1, 2);
    let cards = [card_of(&base, ids[0]), card_of(&base, ids[1])];
    let batch = batch_of(vec![destroy(ids[0]), destroy(ids[1])]);
    let mut seen = BTreeSet::new();
    for answer in [vec![ids[0], ids[1]], vec![ids[1], ids[0]]] {
        let mut session =
            ChoiceSession::start(base.clone(), ordered_procedure(batch.clone())).unwrap();
        let pending = session.pending().unwrap().clone();
        assert_eq!(pending.chooser, p(1));
        assert_eq!(pending.choice_type, ChoiceType::OrderCards);
        session
            .submit(pending.request_id, ordering(answer.clone()))
            .unwrap();
        let (state, output) = done(session.status());
        let expected: Vec<CardId> = answer
            .iter()
            .map(|id| cards[usize::from(*id == ids[1])])
            .collect();
        assert_eq!(graveyard_cards(state, 1), expected);
        seen.insert(graveyard_cards(state, 1));
        // The log is the execute_batch log: submission order.
        assert_eq!(output.events.len(), 2);
        assert_eq!(moved_of(&output.events[0]).old, ids[0]);
        assert_eq!(moved_of(&output.events[1]).old, ids[1]);
        assert_eq!(state.events.log().batches().last(), Some(output));

        // Replay on a clone of the base with the same answer list.
        let replay = ChoiceSession::replay(
            base.clone(),
            ordered_procedure(batch.clone()),
            session.answers(),
        )
        .unwrap();
        let (rstate, routput) = done(replay.status());
        assert!(rstate.zones == state.zones);
        assert_eq!(replay.state_hash(), session.state_hash());
        assert_eq!(encode_to_vec(routput), encode_to_vec(output));
    }
    assert_eq!(seen.len(), 2);
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3. Three cards: all six orders are reachable and land as
// chosen, contiguous on top of the cards already in the graveyard.
#[test]
fn req_zone_001_cr_404_3_three_cards_all_permutations() {
    let mut base = World::new();
    let below = base.create(90, 1, ZoneKind::Graveyard);
    let ids = creatures(&mut base, 1, 1, 3);
    let batch = batch_of(ids.iter().map(|&id| destroy(id)).collect());
    let g = groups(&mut base, &batch);
    assert_eq!(g.len(), 1);
    let mut results = BTreeSet::new();
    for perm in permutations(&ids) {
        let mut w = base.clone();
        let order = GraveyardOrder {
            owner: p(1),
            top_to_bottom: perm.clone(),
        };
        w.ctx().execute_batch_ordered(&batch, &[order]).unwrap();
        let mut expected: Vec<CardId> = perm.iter().map(|&id| card_of(&base, id)).collect();
        expected.push(card_of(&base, below));
        assert_eq!(graveyard_cards(&w, 1), expected);
        results.insert(expected);
    }
    assert_eq!(results.len(), 6);
}

// REQ-EVENT-001, REQ-ZONE-001: CR 404.3. With no orders, and with every group's default order,
// execute_batch_ordered equals execute_batch: same components, same ExecutedBatch.
#[test]
fn req_event_001_cr_404_3_default_equals_execute_batch() {
    let mut base = World::new();
    base.create(90, 2, ZoneKind::Graveyard);
    let a = creatures(&mut base, 1, 1, 3);
    let b = creatures(&mut base, 2, 4, 2);
    let t = token(&mut base, 1, 50);
    let batch = batch_of(vec![
        destroy(b[0]),
        destroy(a[0]),
        destroy(t),
        destroy(a[1]),
        destroy(b[1]),
        destroy(a[2]),
    ]);
    let mut plain = base.clone();
    let expected = plain.ctx().execute_batch(&batch).unwrap();

    let mut none = base.clone();
    let got = none.ctx().execute_batch_ordered(&batch, &[]).unwrap();
    assert_eq!(encode_to_vec(&got), encode_to_vec(&expected));
    assert!(none.parts() == plain.parts());

    let mut defaults = base.clone();
    let orders: Vec<GraveyardOrder> = groups(&mut defaults, &batch)
        .into_iter()
        .rev()
        .map(|g| GraveyardOrder {
            owner: g.owner,
            top_to_bottom: g.default_order,
        })
        .collect();
    assert_eq!(orders.len(), 2);
    let got = defaults
        .ctx()
        .execute_batch_ordered(&batch, &orders)
        .unwrap();
    assert_eq!(encode_to_vec(&got), encode_to_vec(&expected));
    assert!(defaults.parts() == plain.parts());
}

// REQ-EVENT-001, REQ-ZONE-001: CR 404.3. Answering every request with its timeout default
// (submit_default) gives the default_order, which is the execute_batch result.
#[test]
fn req_event_001_cr_404_3_submit_default_is_default_order() {
    let mut base = World::new();
    let a = creatures(&mut base, 1, 1, 3);
    let b = creatures(&mut base, 2, 4, 2);
    let batch = batch_of(vec![
        destroy(a[2]),
        destroy(b[1]),
        destroy(a[0]),
        destroy(b[0]),
        destroy(a[1]),
    ]);
    let g = groups(&mut base.clone(), &batch);
    let mut session = ChoiceSession::start(base.clone(), ordered_procedure(batch.clone())).unwrap();
    // CR 404.3 gives no sequencing: one request per owner, ascending PlayerId.
    let mut asked = Vec::new();
    while let Some(r) = session.pending().cloned() {
        asked.push(r.chooser);
        let ChoiceResponse::SubmitOrdering { graveyard, .. } = &r.default_on_timeout else {
            panic!("unexpected default");
        };
        assert_eq!(*graveyard, g[asked.len() - 1].default_order);
        session.submit_default(r.request_id).unwrap();
    }
    assert_eq!(asked, vec![p(1), p(2)]);
    let mut plain = base.clone();
    let expected = plain.ctx().execute_batch(&batch).unwrap();
    let (state, output) = done(session.status());
    assert_eq!(encode_to_vec(output), encode_to_vec(&expected));
    assert!(state.parts() == plain.parts());
    // The default puts the last-submitted card on top.
    assert_eq!(g[0].default_order, vec![a[1], a[0], a[2]]);
    assert_eq!(g[1].default_order, vec![b[0], b[1]]);
}

// REQ-EVENT-001: invalid orders (owner without a group, an owner twice, not a permutation) are
// rejected with InvalidGraveyardOrder and change nothing (all-or-nothing).
#[test]
fn req_event_001_cr_404_3_invalid_orders_rejected_atomically() {
    let mut w = World::new();
    let a = creatures(&mut w, 1, 1, 2);
    let lone = creatures(&mut w, 2, 3, 1);
    let batch = batch_of(vec![destroy(a[0]), destroy(a[1]), destroy(lone[0])]);
    let good = GraveyardOrder {
        owner: p(1),
        top_to_bottom: vec![a[0], a[1]],
    };
    let cases: Vec<(Vec<GraveyardOrder>, u64)> = vec![
        // Player 2 has one card: no group.
        (
            vec![GraveyardOrder {
                owner: p(2),
                top_to_bottom: vec![lone[0]],
            }],
            2,
        ),
        // An unknown player.
        (
            vec![GraveyardOrder {
                owner: p(7),
                top_to_bottom: Vec::new(),
            }],
            7,
        ),
        // An owner twice.
        (vec![good.clone(), good.clone()], 1),
        // Missing a card.
        (
            vec![GraveyardOrder {
                owner: p(1),
                top_to_bottom: vec![a[0]],
            }],
            1,
        ),
        // A card twice.
        (
            vec![GraveyardOrder {
                owner: p(1),
                top_to_bottom: vec![a[0], a[0]],
            }],
            1,
        ),
        // A card outside the group.
        (
            vec![GraveyardOrder {
                owner: p(1),
                top_to_bottom: vec![a[0], a[1], lone[0]],
            }],
            1,
        ),
        // A valid entry plus an invalid one, in either entry order: the lowest bad owner.
        (
            vec![
                good.clone(),
                GraveyardOrder {
                    owner: p(2),
                    top_to_bottom: vec![lone[0]],
                },
            ],
            2,
        ),
        (
            vec![
                GraveyardOrder {
                    owner: p(2),
                    top_to_bottom: vec![lone[0]],
                },
                good.clone(),
            ],
            2,
        ),
    ];
    for (orders, owner) in cases {
        let before = w.parts();
        let err = w.ctx().execute_batch_ordered(&batch, &orders).unwrap_err();
        assert_eq!(err, EventError::InvalidGraveyardOrder { owner: p(owner) });
        assert!(w.parts() == before, "{orders:?}");
        assert!(err.to_string().contains("invalid graveyard order"));
    }
    // A failing batch with valid orders changes nothing either.
    let before = w.parts();
    let failing = batch_of(vec![
        destroy(a[0]),
        destroy(a[1]),
        destroy(ObjectId::new(999)),
    ]);
    assert_eq!(
        w.ctx()
            .execute_batch_ordered(&failing, std::slice::from_ref(&good))
            .unwrap_err(),
        EventError::UnknownObject(ObjectId::new(999))
    );
    assert!(w.parts() == before);
    // The valid order alone executes.
    w.ctx().execute_batch_ordered(&batch, &[good]).unwrap();
    assert_eq!(
        graveyard_cards(&w, 1),
        vec![c(1), c(2)],
        "a[0] (card 1) on top as chosen"
    );
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3, CR 404.1, CR 111.7. A batch mixing a group with other
// moves into the same graveyard: the group's cards take their own execute_batch slots in the
// chosen order; every other object keeps its place (a token that landed between group cards,
// a later Top arrival outside the group, a Bottom move, the cards already there).
#[test]
fn req_zone_001_cr_404_3_mixed_batch_placement() {
    let mut base = World::new();
    base.create(90, 1, ZoneKind::Graveyard);
    let ids = creatures(&mut base, 1, 1, 3);
    let t1 = token(&mut base, 1, 50);
    let t2 = token(&mut base, 1, 51);
    let batch = batch_of(vec![
        destroy(ids[0]),
        destroy(t1),
        destroy(ids[1]),
        to_gy_at(ids[2], Position::Bottom),
        destroy(t2),
    ]);
    let mut plain = base.clone();
    let expected_log = plain.ctx().execute_batch(&batch).unwrap();
    // Default: t2, ids1, t1, ids0, old, ids2.
    assert_eq!(
        graveyard_cards(&plain, 1),
        vec![c(51), c(2), c(50), c(1), c(90), c(3)]
    );
    let g = groups(&mut base, &batch);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].cards, vec![ids[0], ids[1]]);
    assert_eq!(g[0].default_order, vec![ids[1], ids[0]]);

    let mut w = base.clone();
    let got = w
        .ctx()
        .execute_batch_ordered(
            &batch,
            &[GraveyardOrder {
                owner: p(1),
                top_to_bottom: vec![ids[0], ids[1]],
            }],
        )
        .unwrap();
    assert_eq!(
        graveyard_cards(&w, 1),
        vec![c(51), c(1), c(50), c(2), c(90), c(3)]
    );
    // The log and the object ids are those of execute_batch; only the graveyard order differs.
    assert_eq!(encode_to_vec(&got), encode_to_vec(&expected_log));
    let mut sorted_a = graveyard(&w, 1);
    let mut sorted_b = graveyard(&plain, 1);
    sorted_a.sort_unstable();
    sorted_b.sort_unstable();
    assert_eq!(sorted_a, sorted_b);
    assert!(w.events == plain.events && w.lki == plain.lki && w.ids == plain.ids);
}

// REQ-EVENT-001: CR 704.8, CR 603.10a. The ordered execution captures LKI exactly as
// execute_batch does (before any move of the batch).
#[test]
fn req_event_001_cr_704_8_ordered_lki_unchanged() {
    let mut base = World::new();
    let ids = creatures(&mut base, 1, 1, 2);
    let batch = batch_of(vec![destroy(ids[0]), destroy(ids[1])]);
    let mut plain = base.clone();
    plain.ctx().execute_batch(&batch).unwrap();
    let mut w = base.clone();
    w.ctx()
        .execute_batch_ordered(
            &batch,
            &[GraveyardOrder {
                owner: p(1),
                top_to_bottom: vec![ids[0], ids[1]],
            }],
        )
        .unwrap();
    assert!(w.lki == plain.lki);
    assert!(w.lki.get(ids[0]).is_some() && w.lki.get(ids[1]).is_some());
}

// REQ-ZONE-001, REQ-EVENT-001: CR 404.3 (property). Over random batches of cards of random
// owners (some tokens, some pre-existing graveyard cards) and random chosen permutations: each
// group's cards fill their execute_batch slots in the chosen order (contiguous when nothing
// else landed between them), every other object of each graveyard keeps its position, the log equals execute_batch's, and the result is
// a deterministic function of the inputs.
#[test]
fn req_zone_001_req_event_001_cr_404_3_property_random_orders() {
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut base = World::new();
        let mut card = 1;
        for owner in 1..=2 {
            for _ in 0..rng.below(3) {
                base.create(100 + card, owner, ZoneKind::Graveyard);
                card += 1;
            }
        }
        let mut events = Vec::new();
        for _ in 0..(1 + rng.below(7)) {
            let owner = 1 + rng.below(2);
            let id = if rng.below(5) == 0 {
                token(&mut base, owner, 200 + card)
            } else {
                base.create(card, owner, ZoneKind::Battlefield)
            };
            card += 1;
            events.push(if rng.below(6) == 0 {
                to_gy_at(id, Position::Bottom)
            } else {
                destroy(id)
            });
        }
        let batch = batch_of(events);
        let g = groups(&mut base, &batch);
        let mut orders = Vec::new();
        for group in &g {
            assert!(group.cards.len() >= 2);
            let mut perm = group.cards.clone();
            for i in (1..perm.len()).rev() {
                perm.swap(i, rng.index(i + 1));
            }
            let r = group.request(RequestId::new(0));
            assert!(r.validate(&r.default_on_timeout).is_ok());
            assert!(r.validate(&ordering(perm.clone())).is_ok());
            orders.push(GraveyardOrder {
                owner: group.owner,
                top_to_bottom: perm,
            });
        }
        if rng.below(2) == 0 {
            orders.reverse();
        }
        let mut plain = base.clone();
        let expected = plain.ctx().execute_batch(&batch).unwrap();
        let mut w = base.clone();
        let got = w.ctx().execute_batch_ordered(&batch, &orders).unwrap();
        assert_eq!(encode_to_vec(&got), encode_to_vec(&expected), "seed {seed}");
        let mut again = base.clone();
        again.ctx().execute_batch_ordered(&batch, &orders).unwrap();
        assert!(again.parts() == w.parts(), "seed {seed}");
        let renamed: BTreeMap<ObjectId, ObjectId> = got
            .events
            .iter()
            .map(|e| (moved_of(e).old, moved_of(e).new))
            .collect();
        for owner in 1..=2 {
            let ours = graveyard(&w, owner);
            let theirs = graveyard(&plain, owner);
            let order = orders.iter().find(|o| o.owner == p(owner));
            let block: Vec<ObjectId> = order
                .map(|o| o.top_to_bottom.iter().map(|id| renamed[id]).collect())
                .unwrap_or_default();
            if block.is_empty() {
                assert_eq!(ours, theirs, "seed {seed}");
                continue;
            }
            // The group's cards fill their execute_batch slots in the chosen order; every
            // other object keeps its execute_batch position.
            assert_eq!(ours.len(), theirs.len(), "seed {seed}");
            let mut next = block.iter();
            for (i, id) in theirs.iter().enumerate() {
                if block.contains(id) {
                    assert_eq!(Some(&ours[i]), next.next(), "seed {seed}");
                } else {
                    assert_eq!(ours[i], *id, "seed {seed}");
                }
            }
            // Contiguous when nothing else of the batch landed between the group's cards.
            let first = theirs.iter().position(|id| block.contains(id)).unwrap();
            if theirs[first..first + block.len()]
                .iter()
                .all(|id| block.contains(id))
            {
                assert_eq!(
                    &ours[first..first + block.len()],
                    block.as_slice(),
                    "seed {seed}"
                );
            }
        }
        for k in [
            ZoneId::Battlefield,
            ZoneId::Exile,
            ZoneId::Hand(p(1)),
            ZoneId::Hand(p(2)),
        ] {
            assert_eq!(w.zones.contents(k), plain.zones.contents(k));
        }
    }
}
