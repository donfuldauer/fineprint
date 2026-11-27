// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the graveyard-order choice (REQ-ZONE-001, REQ-EVENT-001, SRS 15.3
//! `ORDER_CARDS`; CR 404.3, 404.1, 400.3, 111.6, 108.2b). Random worlds (fixed seeds, local
//! deterministic generator as in the events property tests; R3): random owners, card counts,
//! source zones, token mixes, existing graveyard contents, `Bottom` moves and submission orders;
//! random permutations. Checked: the groups match the brief's rule (a); the resulting
//! top-to-bottom order is the chosen permutation, above the existing contents; two runs agree;
//! every permutation validates against `request()`; non-permutations are rejected with nothing
//! changed. Black-box: written from the work-item brief only.

#[path = "spec_events_graveyard_order_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{ChoiceResponse, RequestId};
use fineprint_core::events::{EventBatch, EventError, GameEvent, ZoneChangeCause};
use fineprint_core::zones::{Position, ZoneKind};
use support::*;

const SEEDS: u64 = 256;

/// One random case: the base world, the batch, and what the brief says about it.
struct Case {
    base: World,
    batch: EventBatch,
    players: Vec<PlayerId>,
    /// Per owner, the nontoken cards moved to the top of that owner's graveyard, in submission
    /// order.
    top_cards: BTreeMap<PlayerId, Vec<ObjectId>>,
    /// Per owner, the cards moved to the bottom.
    bottom_cards: BTreeMap<PlayerId, Vec<ObjectId>>,
    /// Per owner, the graveyard before the batch, top first.
    existing: BTreeMap<PlayerId, Vec<ObjectId>>,
}

fn random_case(seed: u64) -> Case {
    let mut rng = Lcg::new(seed);
    let n_players = 2 + rng.below(2) as usize;
    let mut w = World::with_players(n_players);
    let players: Vec<PlayerId> = (1..=n_players as u64).map(PlayerId::new).collect();
    let mut events: Vec<GameEvent> = Vec::new();
    let mut top_ids: Vec<(PlayerId, ObjectId)> = Vec::new();
    let mut bottom_cards: BTreeMap<PlayerId, Vec<ObjectId>> = BTreeMap::new();
    for &p in &players {
        for _ in 0..rng.below(3) {
            w.create(ZoneKind::Graveyard, p);
        }
        // Nontoken cards moving to the top, from a random zone (CR 404.1).
        for _ in 0..rng.below(5) {
            let (kind, cause) = match rng.below(3) {
                0 => (ZoneKind::Battlefield, ZoneChangeCause::StateBasedAction),
                1 => (ZoneKind::Hand, ZoneChangeCause::Discard),
                _ => (ZoneKind::Library, ZoneChangeCause::Mill),
            };
            let controller = if kind == ZoneKind::Battlefield && rng.below(3) == 0 {
                // CR 400.3: controlled by someone else, still goes to its owner's graveyard.
                Some(players[rng.below(players.len() as u64) as usize])
            } else {
                None
            };
            let id = w.create_controlled(kind, p, controller);
            events.push(zc(id, ZoneKind::Graveyard, cause));
            top_ids.push((p, id));
        }
        // Tokens dying (CR 111.6, 108.2b: not cards).
        for _ in 0..rng.below(3) {
            let t = w.token(p);
            events.push(dies(t));
        }
        // Bottom moves (not grouped).
        if rng.below(3) == 0 {
            let id = w.create(ZoneKind::Battlefield, p);
            events.push(zc_at(id, ZoneKind::Graveyard, Position::Bottom));
            bottom_cards.entry(p).or_default().push(id);
        }
        // A creature that does something else (not grouped).
        if rng.below(3) == 0 {
            let id = w.create(ZoneKind::Battlefield, p);
            events.push(zc(id, ZoneKind::Exile, ZoneChangeCause::Exile));
        }
    }
    let events = rng.shuffled(&events);
    // Submission order of the top-moving cards follows the shuffled events.
    let mut top_cards: BTreeMap<PlayerId, Vec<ObjectId>> = BTreeMap::new();
    for e in &events {
        if let GameEvent::ZoneChange { object, .. } = e
            && let Some(&(p, _)) = top_ids.iter().find(|(_, id)| id == object)
        {
            top_cards.entry(p).or_default().push(*object);
        }
    }
    let existing = players.iter().map(|&p| (p, w.graveyard(p))).collect();
    Case {
        base: w,
        batch: EventBatch::from_events(events),
        players,
        top_cards,
        bottom_cards,
        existing,
    }
}

/// REQ-ZONE-001, CR 404.3, rule (a): the expected groups: per owner (ascending), the nontoken
/// cards moved to the top when there are two or more, ascending, with the default order the
/// reverse of submission order (last submitted on top).
fn expected_groups(case: &Case) -> Vec<(PlayerId, Vec<ObjectId>, Vec<ObjectId>)> {
    case.top_cards
        .iter()
        .filter(|(_, cards)| cards.len() >= 2)
        .map(|(&p, cards)| (p, sorted(cards), reversed(cards)))
        .collect()
}

// REQ-ZONE-001, REQ-EVENT-001, CR 404.3, 400.3, 111.6, 108.2b: the groups match the rule, leave
// the state unchanged, and their default order is what `execute_batch` produces.
#[test]
fn req_zone_001_property_groups_match_rule() {
    for seed in 0..SEEDS {
        let case = random_case(seed);
        let mut w = case.base.clone();
        let before = w.snap();
        let groups = w.groups(&case.batch);
        assert_eq!(w.snap(), before, "seed {seed}: groups changed the state");
        let got: Vec<_> = groups
            .iter()
            .map(|g| (g.owner, g.cards.clone(), g.default_order.clone()))
            .collect();
        assert_eq!(got, expected_groups(&case), "seed {seed}");

        let (after, done) = {
            let mut p = case.base.clone();
            let d = p.run_plain(&case.batch);
            (p, d)
        };
        for g in &groups {
            let gy = olds_of(&done, &after.graveyard_cards(g.owner));
            assert_eq!(
                &gy[..g.default_order.len()],
                &g.default_order[..],
                "seed {seed}: default order is execute_batch's"
            );
        }
    }
}

// REQ-ZONE-001, REQ-EVENT-001, CR 404.3, 404.1: for a random permutation per group, the owner's
// graveyard (tokens aside) is that permutation on top, then the existing contents in order,
// then the bottom moves; two runs on clones agree on every component and on the canonical
// encoding of the executed batch; a different permutation gives a different order.
#[test]
fn req_zone_001_property_chosen_permutation_is_the_result() {
    for seed in 0..SEEDS {
        let case = random_case(seed);
        let mut rng = Lcg::new(seed ^ 0xA5A5);
        let groups = case.base.clone().groups(&case.batch);
        let chosen: Vec<_> = groups
            .iter()
            .map(|g| order(g.owner, &rng.shuffled(&g.cards)))
            .collect();
        let mut x = case.base.clone();
        let mut y = case.base.clone();
        let dx = x
            .run_ordered(&case.batch, &chosen)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        let dy = y
            .run_ordered(&case.batch, &chosen)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        assert_eq!(x.snap(), y.snap(), "seed {seed}: runs agree");
        assert_eq!(bytes(&dx), bytes(&dy), "seed {seed}");
        assert_eq!(dx, dy, "seed {seed}");
        for (e, i) in dx.events.iter().zip(case.batch.events()) {
            assert_eq!(&e.instance, i, "seed {seed}: submission order kept");
        }
        assert_eq!(dx.events.len(), case.batch.len(), "seed {seed}");

        for &p in &case.players {
            let gy = olds_of(&dx, &x.graveyard_cards(p));
            let mut cursor = 0;
            if let Some(o) = chosen.iter().find(|o| o.owner == p) {
                assert_eq!(
                    &gy[..o.top_to_bottom.len()],
                    &o.top_to_bottom[..],
                    "seed {seed}: {p:?} chosen order"
                );
                cursor = o.top_to_bottom.len();
            } else if let Some(cards) = case.top_cards.get(&p) {
                // A single card (no group) is still on top.
                assert_eq!(&gy[..cards.len()], &cards[..], "seed {seed}: {p:?}");
                cursor = cards.len();
            }
            let existing = &case.existing[&p];
            assert_eq!(
                &gy[cursor..cursor + existing.len()],
                &existing[..],
                "seed {seed}: {p:?} existing contents stay below, in order"
            );
            cursor += existing.len();
            let bottoms = case.bottom_cards.get(&p).cloned().unwrap_or_default();
            assert_eq!(
                sorted(&gy[cursor..]),
                sorted(&bottoms),
                "seed {seed}: {p:?} bottom moves last"
            );
        }

        // A different permutation of one group gives a different order.
        if let Some(g) = groups.first() {
            let first = &chosen[0].top_to_bottom;
            let other = permutations(&g.cards)
                .into_iter()
                .find(|q| q != first)
                .expect("two or more cards");
            let mut alt = chosen.clone();
            alt[0] = order(g.owner, &other);
            let mut z = case.base.clone();
            let dz = z.run_ordered(&case.batch, &alt).expect("valid");
            let gz = olds_of(&dz, &z.graveyard_cards(g.owner));
            assert_eq!(&gz[..other.len()], &other[..], "seed {seed}");
            assert_ne!(z.snap().zones, x.snap().zones, "seed {seed}");
        }
    }
}

// REQ-EVENT-001, SRS 15.3 ORDER_CARDS validation, CR 404.3: every permutation (all of them for
// up to four cards) and the default validate against `request()` and convert back to the same
// order; random non-permutations are rejected by `validate`, by `order_from_response` and by
// `execute_batch_ordered` (with `InvalidGraveyardOrder { owner }` and nothing changed).
#[test]
fn req_event_001_property_validation() {
    for seed in 0..SEEDS {
        let case = random_case(seed);
        let mut rng = Lcg::new(seed ^ 0x5A5A);
        let groups = case.base.clone().groups(&case.batch);
        for g in &groups {
            let req = g.request(RequestId::new(rng.below(4)));
            assert_eq!(req.chooser, g.owner);
            assert_eq!(req.min, g.cards.len() as u64);
            assert_eq!(req.max, g.cards.len() as u64);
            assert_eq!(req.default_on_timeout, gy_answer(&g.default_order));
            assert_eq!(req.validate(&req.default_on_timeout), Ok(()), "seed {seed}");
            for perm in permutations(&g.cards) {
                assert_eq!(req.validate(&gy_answer(&perm)), Ok(()), "seed {seed}");
                let o = g
                    .order_from_response(&gy_answer(&perm))
                    .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
                assert_eq!(o, order(g.owner, &perm));
            }

            // Random non-permutations.
            let perm = rng.shuffled(&g.cards);
            let mut bad: Vec<Vec<ObjectId>> = Vec::new();
            let mut dropped = perm.clone();
            dropped.remove(rng.below(perm.len() as u64) as usize);
            bad.push(dropped);
            let mut dup = perm.clone();
            let i = rng.below(perm.len() as u64) as usize;
            let j = (i + 1) % perm.len();
            dup[j] = dup[i];
            bad.push(dup);
            let mut extra = perm.clone();
            extra.insert(rng.below(perm.len() as u64 + 1) as usize, GHOST);
            bad.push(extra);
            let mut swapped = perm.clone();
            swapped[rng.below(perm.len() as u64) as usize] = GHOST;
            bad.push(swapped);
            bad.push(Vec::new());
            for list in &bad {
                assert!(req.validate(&gy_answer(list)).is_err(), "seed {seed}");
                assert!(g.order_from_response(&gy_answer(list)).is_err());
                let mut w = case.base.clone();
                let before = w.snap();
                let others: Vec<_> = groups
                    .iter()
                    .filter(|h| h.owner != g.owner)
                    .map(|h| order(h.owner, &h.default_order))
                    .collect();
                let mut orders = others;
                orders.push(order(g.owner, list));
                assert_eq!(
                    w.run_ordered(&case.batch, &orders),
                    Err(EventError::InvalidGraveyardOrder { owner: g.owner }),
                    "seed {seed}: {list:?}"
                );
                assert_eq!(w.snap(), before, "seed {seed}: nothing changed");
            }
            // Cards in `top` or `bottom` are not accepted.
            let split = 1 + rng.below(perm.len() as u64 - 1) as usize;
            for resp in [
                ChoiceResponse::SubmitOrdering {
                    top: perm[..split].to_vec(),
                    bottom: Vec::new(),
                    graveyard: perm[split..].to_vec(),
                },
                ChoiceResponse::SubmitOrdering {
                    top: Vec::new(),
                    bottom: perm[..split].to_vec(),
                    graveyard: perm[split..].to_vec(),
                },
            ] {
                assert!(req.validate(&resp).is_err(), "seed {seed}: {resp:?}");
                assert!(g.order_from_response(&resp).is_err(), "seed {seed}");
            }
        }

        // An entry for an owner without a group is rejected.
        if let Some(&p) = case
            .players
            .iter()
            .find(|p| !groups.iter().any(|g| g.owner == **p))
        {
            let mut w = case.base.clone();
            let before = w.snap();
            assert_eq!(
                w.run_ordered(&case.batch, &[order(p, &[])]),
                Err(EventError::InvalidGraveyardOrder { owner: p }),
                "seed {seed}"
            );
            assert_eq!(w.snap(), before);
        }
    }
}

// REQ-EVENT-001, CR 404.3: through a `ChoiceSession`, random answers give the same final state
// as the direct call, and replaying the recorded answers reproduces the same state hash.
#[test]
fn req_event_001_property_session_replay() {
    for seed in 0..64 {
        let case = random_case(seed);
        let mut rng = Lcg::new(seed ^ 0x3C3C);
        let groups = case.base.clone().groups(&case.batch);
        let perms: Vec<Vec<ObjectId>> = groups.iter().map(|g| rng.shuffled(&g.cards)).collect();
        let run = drive(&case.base, &case.batch, |i, req| {
            assert_eq!(
                req.chooser, groups[i].owner,
                "seed {seed}: ascending owners"
            );
            Some(gy_answer(&perms[i]))
        });
        assert_eq!(run.requests.len(), groups.len(), "seed {seed}");
        let orders: Vec<_> = groups
            .iter()
            .zip(&perms)
            .map(|(g, p)| order(g.owner, p))
            .collect();
        let mut direct = case.base.clone();
        let done = direct.run_ordered(&case.batch, &orders).expect("valid");
        assert_eq!(run.state.snap(), direct.snap(), "seed {seed}");
        assert_eq!(bytes(&run.output), bytes(&done), "seed {seed}");
        let again = replay(&case.base, &case.batch, &run.answers);
        assert_eq!(again.hash, run.hash, "seed {seed}");
        assert_eq!(again.state.snap(), run.state.snap(), "seed {seed}");
    }
}
