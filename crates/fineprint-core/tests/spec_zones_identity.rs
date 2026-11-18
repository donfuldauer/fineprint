// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::zones`. Black-box: written from the work-item brief only.
//!
//! Object identity: every zone change (and a same-zone move into exile or the command zone)
//! creates a new ObjectId and leaves no reference to the old one, a same-zone move elsewhere
//! keeps the id, CardId persistence, owner's-zone routing, error atomicity and snapshot / canonical
//! encoding behaviour (REQ-CORE-002, REQ-ZONE-001).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::zones::{
    MoveRequest, OutsidePool, Position, ZoneError, ZoneId, ZoneKind, Zones, ZonesConfig,
};

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn cid(n: u64) -> CardId {
    CardId::new(n)
}

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn req(kind: ZoneKind) -> MoveRequest {
    MoveRequest::new(kind)
}

fn req_pos(kind: ZoneKind, position: Position) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.position = position;
    r
}

fn req_ctrl(kind: ZoneKind, controller: PlayerId) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.controller = Some(controller);
    r
}

fn two() -> Zones {
    Zones::new(&[pid(1), pid(2)], ZonesConfig::default())
}

const POOLS: [OutsidePool; 6] = [
    OutsidePool::Sideboard,
    OutsidePool::Companion,
    OutsidePool::Lesson,
    OutsidePool::AttractionDeck,
    OutsidePool::ContraptionDeck,
    OutsidePool::Spellbook,
];

fn zone_ids(players: &[PlayerId]) -> Vec<ZoneId> {
    let mut out = Vec::new();
    for &p in players {
        out.push(ZoneId::Library(p));
        out.push(ZoneId::Hand(p));
        out.push(ZoneId::Graveyard(p));
    }
    out.extend([
        ZoneId::Battlefield,
        ZoneId::Stack,
        ZoneId::Exile,
        ZoneId::Command,
    ]);
    out
}

/// True if `id` is referenced by any public query of the container.
fn referenced(z: &Zones, id: ObjectId, players: &[PlayerId]) -> bool {
    z.object(id).is_some()
        || z.is_public_info(id).is_some()
        || z.all_object_ids().contains(&id)
        || zone_ids(players)
            .into_iter()
            .any(|zone| z.contents(zone).contains(&id))
}

/// A populated container with background objects in every zone of both players.
fn populated() -> (Zones, IdGenerator<ObjectId>) {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut card = 1000;
    for p in [pid(1), pid(2)] {
        for kind in ZoneKind::ALL {
            for _ in 0..2 {
                card += 1;
                z.create_object(&mut g, cid(card), p, req_pos(kind, Position::Bottom))
                    .unwrap();
            }
        }
    }
    (z, g)
}

// ---------------------------------------------------------------------------------------------
// Item 1: every move creates a new ObjectId.
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 400.7, 400.8, 406.7, 400.10, 400.5: for every ordered pair of the seven zone
// kinds (same-kind included). A cross-kind move, and a same-kind move into exile or the command
// zone, issues exactly one fresh id, the old id is gone from every query, the CardId and owner
// persist, and Moved.from / Moved.to are right. A same-kind move into the library, hand,
// graveyard, battlefield or stack is not a zone change: the id is kept and no id is issued.
// R10 correction for finding F1 (CodeRabbit comment 4621941423): the same-kind pairs used to
// assert a new id for every kind.
#[test]
fn every_move_between_every_pair_of_kinds_creates_a_new_object() {
    let players = [pid(1), pid(2)];
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            let (mut z, mut g) = populated();
            let card = cid(7);
            let old = z.create_object(&mut g, card, pid(2), req(from)).unwrap();
            assert_eq!(old.get(), g.last_issued());
            let ids_before = z.all_object_ids();
            let others: Vec<ObjectId> = ids_before.iter().copied().filter(|i| *i != old).collect();
            let last = g.last_issued();
            let moved = z.move_object(&mut g, old, req(to)).unwrap();
            assert_eq!(moved.old, old);
            assert_eq!(moved.from, ZoneId::of(from, pid(2)));
            assert_eq!(moved.to, ZoneId::of(to, pid(2)));
            let keeps_id = from == to && !matches!(to, ZoneKind::Exile | ZoneKind::Command);
            if keeps_id {
                assert_eq!(g.last_issued(), last, "{from:?} -> {to:?}");
                assert_eq!(moved.new, old, "{from:?} -> {to:?}");
                assert_eq!(z.all_object_ids(), ids_before);
            } else {
                assert_eq!(g.last_issued(), last + 1, "{from:?} -> {to:?}");
                assert_eq!(moved.new.get(), last + 1);
                assert_ne!(moved.new, old);
                assert!(ids_before.iter().all(|i| moved.new > *i));
                assert!(!referenced(&z, old, &players), "{from:?} -> {to:?}");
                // Every other object is untouched.
                let mut expected = others.clone();
                expected.push(moved.new);
                expected.sort();
                assert_eq!(z.all_object_ids(), expected);
            }
            assert_eq!(z.object_of_card(card), Some(moved.new));
            let rec = z.object(moved.new).unwrap();
            assert_eq!(rec.id, moved.new);
            assert_eq!(rec.card, card);
            assert_eq!(rec.owner, pid(2));
            assert_eq!(rec.zone, ZoneId::of(to, pid(2)));
            assert!(z.contents(ZoneId::of(to, pid(2))).contains(&moved.new));
        }
    }
}

// REQ-CORE-002, CR 400.8, 406.7, 400.10: exile to exile and command to command issue a fresh id;
// the old id is absent from all_object_ids, object and every zone's contents (finding F1).
#[test]
fn exile_and_command_same_zone_moves_issue_a_fresh_id() {
    let players = [pid(1), pid(2)];
    for kind in [ZoneKind::Exile, ZoneKind::Command] {
        let (mut z, mut g) = populated();
        let old = z.create_object(&mut g, cid(7), pid(1), req(kind)).unwrap();
        let m = z.move_object(&mut g, old, req(kind)).unwrap();
        assert_eq!(m.new.get(), g.last_issued());
        assert!(m.new > old);
        assert_eq!(
            (m.from, m.to),
            (ZoneId::of(kind, pid(1)), ZoneId::of(kind, pid(1)))
        );
        assert!(!z.all_object_ids().contains(&old));
        assert_eq!(z.object(old), None);
        assert!(
            zone_ids(&players)
                .into_iter()
                .all(|zone| !z.contents(zone).contains(&old))
        );
        assert_eq!(z.object_of_card(cid(7)), Some(m.new));
    }
}

// REQ-CORE-002, REQ-ZONE-001, CR 400.7, 400.5, 109.4, 110.2a: a battlefield-to-battlefield move
// with a controller change keeps the id and issues none (finding F1).
#[test]
fn battlefield_to_battlefield_with_controller_change_keeps_the_id() {
    let (mut z, mut g) = populated();
    let a = z
        .create_object(&mut g, cid(7), pid(1), req(ZoneKind::Battlefield))
        .unwrap();
    let last = g.last_issued();
    let m = z
        .move_object(&mut g, a, req_ctrl(ZoneKind::Battlefield, pid(2)))
        .unwrap();
    assert_eq!((m.old, m.new), (a, a));
    assert_eq!(g.last_issued(), last);
    let rec = z.object(a).unwrap();
    assert_eq!((rec.owner, rec.controller), (pid(1), pid(2)));
    assert_eq!(rec.rules_controller(), Some(pid(2)));
}

// REQ-CORE-002, CR 400.7: a chain of moves gives a strictly increasing sequence of ids, none of
// which survives its successor; the CardId stays the same throughout.
#[test]
fn chained_moves_leave_no_stale_ids() {
    let players = [pid(1), pid(2)];
    let (mut z, mut g) = populated();
    let mut id = z
        .create_object(&mut g, cid(5), pid(1), req(ZoneKind::Library))
        .unwrap();
    let mut history = vec![id];
    for i in 0..50 {
        let to = ZoneKind::ALL[(i * 3 + 1) % 7];
        let m = z.move_object(&mut g, id, req(to)).unwrap();
        assert!(m.new > id);
        assert_eq!(m.old, id);
        for old in &history {
            assert!(!referenced(&z, *old, &players));
            assert_eq!(
                z.move_object(&mut g, *old, req(to)).map(|x| x.new),
                Err(ZoneError::UnknownObject(*old))
            );
        }
        assert_eq!(z.object_of_card(cid(5)), Some(m.new));
        assert_eq!(z.object(m.new).unwrap().card, cid(5));
        id = m.new;
        history.push(id);
    }
}

// REQ-CORE-002, CR 400.7: create_object issues exactly one fresh id per call.
#[test]
fn create_object_issues_exactly_one_id() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::from_last_issued(9);
    for (i, kind) in ZoneKind::ALL.into_iter().enumerate() {
        let id = z
            .create_object(&mut g, cid(i as u64), pid(1), req(kind))
            .unwrap();
        assert_eq!(id, oid(10 + i as u64));
        assert_eq!(g.last_issued(), 10 + i as u64);
        assert_eq!(z.object_of_card(cid(i as u64)), Some(id));
    }
}

// ---------------------------------------------------------------------------------------------
// Item 3: owner's zones.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, CR 400.3: an object controlled by another player goes to its owner's library,
// hand or graveyard; shared destinations are the shared zones.
#[test]
fn per_player_destinations_use_the_owner() {
    for from in [ZoneKind::Battlefield, ZoneKind::Stack] {
        for to in ZoneKind::ALL {
            let mut z = two();
            let mut g = IdGenerator::<ObjectId>::new();
            let a = z
                .create_object(&mut g, cid(1), pid(1), req_ctrl(from, pid(2)))
                .unwrap();
            assert_eq!(z.object(a).unwrap().controller, pid(2));
            let m = z.move_object(&mut g, a, req(to)).unwrap();
            let expected = ZoneId::of(to, pid(1));
            assert_eq!(m.to, expected);
            assert_eq!(z.object(m.new).unwrap().zone, expected);
            assert_eq!(z.contents(expected), vec![m.new]);
            for zone in zone_ids(&[pid(1), pid(2)]) {
                if zone != expected {
                    assert!(z.contents(zone).is_empty(), "{from:?}->{to:?} {zone:?}");
                }
            }
            if !to.is_per_player() {
                assert_eq!(expected.owner(), None);
            }
        }
    }
}

// REQ-ZONE-001, CR 400.3: creating and entering from outside route per-player kinds to the
// owner's zone.
#[test]
fn creation_routes_to_owner_zone() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(&mut g, cid(1), pid(2), req(ZoneKind::Graveyard))
        .unwrap();
    assert_eq!(z.contents(ZoneId::Graveyard(pid(2))), vec![a]);
    assert!(z.contents(ZoneId::Graveyard(pid(1))).is_empty());
    z.add_outside_card(pid(2), OutsidePool::Sideboard, cid(2))
        .unwrap();
    let b = z
        .enter_from_outside(
            &mut g,
            pid(2),
            cid(2),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand),
        )
        .unwrap();
    assert_eq!(z.contents(ZoneId::Hand(pid(2))), vec![b]);
    assert!(z.contents(ZoneId::Hand(pid(1))).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Item 2: property test over random operation sequences.
// ---------------------------------------------------------------------------------------------

fn pick(rng: &mut Pcg64Dxsm, n: usize) -> usize {
    usize::try_from(rng.next_bounded(n as u64).unwrap()).unwrap()
}

fn random_request(rng: &mut Pcg64Dxsm, players: &[PlayerId]) -> MoveRequest {
    let to = ZoneKind::ALL[pick(rng, 7)];
    let mut r = req(to);
    r.position = match pick(rng, 4) {
        0 => Position::Top,
        1 => Position::Bottom,
        _ => Position::Index(pick(rng, 10)),
    };
    r.face_down = pick(rng, 3) == 0;
    if matches!(to, ZoneKind::Battlefield | ZoneKind::Stack) && pick(rng, 2) == 0 {
        r.controller = Some(players[pick(rng, players.len())]);
    }
    r
}

/// Checks the identity invariants of `z` against the tracked cards.
fn check_identity(
    z: &Zones,
    players: &[PlayerId],
    cards: &BTreeMap<CardId, PlayerId>,
    retired: &BTreeSet<ObjectId>,
    ctx: &str,
) {
    let all = z.all_object_ids();
    // Ascending and unique.
    assert!(all.windows(2).all(|w| w[0] < w[1]), "{ctx}: {all:?}");
    for id in &all {
        assert!(!retired.contains(id), "{ctx}: retired id {id:?} reappeared");
        assert_eq!(z.object(*id).unwrap().id, *id);
    }
    // Zone contents partition all_object_ids.
    let mut in_zones: Vec<ObjectId> = zone_ids(players)
        .into_iter()
        .flat_map(|zone| z.contents(zone))
        .collect();
    in_zones.sort();
    assert_eq!(in_zones, all, "{ctx}");
    // Each card is held by exactly one object or one pool.
    let mut holders = 0;
    for (card, owner) in cards {
        let in_obj = z.object_of_card(*card);
        let mut pools = 0;
        for p in players {
            for pool in POOLS {
                pools += z
                    .outside_cards(*p, pool)
                    .iter()
                    .filter(|c| *c == card)
                    .count();
            }
        }
        match in_obj {
            Some(id) => {
                assert_eq!(pools, 0, "{ctx}: {card:?} both in zone and pool");
                let rec = z.object(id).unwrap();
                assert_eq!(rec.card, *card);
                assert_eq!(rec.owner, *owner, "{ctx}: owner changed");
            }
            None => {
                assert_eq!(pools, 1, "{ctx}: {card:?} held {pools} times outside");
                assert!(
                    POOLS
                        .iter()
                        .any(|pool| z.outside_cards(*owner, *pool).contains(card))
                );
            }
        }
        holders += 1;
    }
    let mut pool_total = 0;
    for p in players {
        for pool in POOLS {
            pool_total += z.outside_cards(*p, pool).len();
        }
    }
    assert_eq!(all.len() + pool_total, holders, "{ctx}: untracked holders");
}

// REQ-CORE-002, REQ-ZONE-001 item 9, CR 400.7, 400.8, 406.7, 400.10, 400.11: over many seeds and
// 2-4 players, random creates, moves, outside round trips and failing calls never duplicate an
// id, never revive a replaced id, keep each CardId in exactly one place and issue strictly
// increasing ids. A same-zone move into the library, hand, graveyard, battlefield or stack keeps
// the id and issues none, so it replaces nothing (finding F1, comment 4621941423).
#[test]
fn property_ids_are_fresh_and_never_reused() {
    let mut repositions = 0_usize;
    let mut fail_checks = 0_usize;
    let mut fresh_checks = 0_usize;
    for seed in 0..64_u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x1d_0000 + seed);
        let n_players = 2 + pick(&mut rng, 3);
        let players: Vec<PlayerId> = (1..=n_players as u64).map(pid).collect();
        let mut z = Zones::new(&players, ZonesConfig::default());
        let mut g = IdGenerator::<ObjectId>::new();
        let mut cards: BTreeMap<CardId, PlayerId> = BTreeMap::new();
        let mut retired: BTreeSet<ObjectId> = BTreeSet::new();
        let mut max_issued = 0_u64;
        let mut next_card = 1_u64;
        for step in 0..300 {
            let ctx = format!("seed {seed} step {step}");
            let live = z.all_object_ids();
            let mut fresh: Option<ObjectId> = None;
            match pick(&mut rng, 12) {
                0 | 1 => {
                    let owner = players[pick(&mut rng, players.len())];
                    let card = cid(next_card);
                    next_card += 1;
                    let r = random_request(&mut rng, &players);
                    fresh = Some(z.create_object(&mut g, card, owner, r).unwrap());
                    cards.insert(card, owner);
                }
                2 => {
                    let owner = players[pick(&mut rng, players.len())];
                    let card = cid(next_card);
                    next_card += 1;
                    z.add_outside_card(owner, POOLS[pick(&mut rng, 6)], card)
                        .unwrap();
                    cards.insert(card, owner);
                }
                3 if !live.is_empty() => {
                    let id = live[pick(&mut rng, live.len())];
                    let card = z.object(id).unwrap().card;
                    assert_eq!(z.send_outside(id, POOLS[pick(&mut rng, 6)]), Ok(card));
                    retired.insert(id);
                }
                4 => {
                    // Enter some outside card, if any.
                    let mut choices = Vec::new();
                    for p in &players {
                        for pool in POOLS {
                            for c in z.outside_cards(*p, pool) {
                                choices.push((*p, pool, c));
                            }
                        }
                    }
                    if !choices.is_empty() {
                        let (p, pool, c) = choices[pick(&mut rng, choices.len())];
                        let r = random_request(&mut rng, &players);
                        fresh = Some(z.enter_from_outside(&mut g, p, c, pool, r).unwrap());
                    }
                }
                5 => {
                    // A failing call: unknown, retired or duplicate; nothing changes.
                    let before = z.clone();
                    let last = g.last_issued();
                    let bad = retired.iter().next_back().copied().unwrap_or(oid(u64::MAX));
                    assert!(z.move_object(&mut g, bad, req(ZoneKind::Hand)).is_err());
                    if let Some((card, owner)) = cards.iter().next() {
                        fail_checks += 1;
                        assert_eq!(
                            z.create_object(&mut g, *card, *owner, req(ZoneKind::Exile)),
                            Err(ZoneError::CardAlreadyPresent(*card))
                        );
                    }
                    assert_eq!(z, before, "{ctx}");
                    assert_eq!(g.last_issued(), last);
                }
                _ if !live.is_empty() => {
                    let id = live[pick(&mut rng, live.len())];
                    let r = random_request(&mut rng, &players);
                    let rec = *z.object(id).unwrap();
                    let m = z.move_object(&mut g, id, r).unwrap();
                    assert_eq!(m.old, id);
                    assert_eq!(m.from, rec.zone);
                    if m.to == rec.zone && !matches!(m.to, ZoneId::Exile | ZoneId::Command) {
                        assert_eq!(m.new, id, "{ctx}: reposition changed the id");
                        repositions += 1;
                    } else {
                        retired.insert(id);
                        fresh = Some(m.new);
                    }
                }
                _ => {}
            }
            if let Some(id) = fresh {
                fresh_checks += 1;
                assert!(id.get() > max_issued, "{ctx}: ids not increasing");
                assert_eq!(id.get(), g.last_issued());
                assert!(!retired.contains(&id));
                max_issued = id.get();
            }
            assert_eq!(g.last_issued(), max_issued, "{ctx}: unexpected id issued");
            check_identity(&z, &players, &cards, &retired, &ctx);
        }
    }
    // Neither the reposition path, the fresh-id checks nor the duplicate-card check is vacuous.
    assert!(repositions > 0);
    assert!(fail_checks > 0);
    assert!(fresh_checks > 0);
}

// ---------------------------------------------------------------------------------------------
// Item 10: error atomicity.
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, REQ-ZONE-001, CR 400.7, 109.4: failing calls (including the container's
// controller-eligibility errors) leave the container and the generator unchanged.
#[test]
fn failed_calls_change_nothing() {
    let (mut z, mut g) = populated();
    z.add_outside_card(pid(1), OutsidePool::Sideboard, cid(1))
        .unwrap();
    let live = z.all_object_ids()[0];
    let live_card = z.object(live).unwrap().card;
    let before = z.clone();
    let last = g.last_issued();
    let check = |z: &Zones, g: &IdGenerator<ObjectId>| {
        assert_eq!(*z, before);
        assert_eq!(g.last_issued(), last);
    };
    // Unknown object.
    assert_eq!(
        z.move_object(&mut g, oid(9999), req(ZoneKind::Hand))
            .map(|m| m.new),
        Err(ZoneError::UnknownObject(oid(9999)))
    );
    check(&z, &g);
    assert_eq!(
        z.send_outside(oid(9999), OutsidePool::Lesson),
        Err(ZoneError::UnknownObject(oid(9999)))
    );
    check(&z, &g);
    // Unknown owner.
    assert_eq!(
        z.create_object(&mut g, cid(2), pid(7), req(ZoneKind::Library)),
        Err(ZoneError::UnknownPlayer(pid(7)))
    );
    check(&z, &g);
    assert!(
        z.enter_from_outside(
            &mut g,
            pid(7),
            cid(1),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand)
        )
        .is_err()
    );
    check(&z, &g);
    // Duplicate card (in a zone, in a pool).
    assert_eq!(
        z.create_object(&mut g, live_card, pid(1), req(ZoneKind::Exile)),
        Err(ZoneError::CardAlreadyPresent(live_card))
    );
    check(&z, &g);
    assert_eq!(
        z.create_object(&mut g, cid(1), pid(1), req(ZoneKind::Exile)),
        Err(ZoneError::CardAlreadyPresent(cid(1)))
    );
    check(&z, &g);
    // Controller errors.
    assert_eq!(
        z.move_object(&mut g, live, req_ctrl(ZoneKind::Graveyard, pid(2)))
            .map(|m| m.new),
        Err(ZoneError::ControllerNotApplicable)
    );
    check(&z, &g);
    assert_eq!(
        z.move_object(&mut g, live, req_ctrl(ZoneKind::Battlefield, pid(8)))
            .map(|m| m.new),
        Err(ZoneError::UnknownPlayer(pid(8)))
    );
    check(&z, &g);
    assert_eq!(
        z.enter_from_outside(
            &mut g,
            pid(1),
            cid(1),
            OutsidePool::Sideboard,
            req_ctrl(ZoneKind::Exile, pid(2))
        ),
        Err(ZoneError::ControllerNotApplicable)
    );
    check(&z, &g);
    assert_eq!(
        z.enter_from_outside(
            &mut g,
            pid(1),
            cid(1),
            OutsidePool::Sideboard,
            req_ctrl(ZoneKind::Stack, pid(8))
        ),
        Err(ZoneError::UnknownPlayer(pid(8)))
    );
    check(&z, &g);
    // Missing outside card.
    assert_eq!(
        z.enter_from_outside(
            &mut g,
            pid(1),
            cid(3),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand)
        ),
        Err(ZoneError::NoSuchOutsideCard(cid(3)))
    );
    check(&z, &g);
}

// REQ-CORE-002, CR 400.7: an exhausted generator gives IdExhausted and nothing changes.
#[test]
fn id_exhaustion_changes_nothing() {
    let (mut z, _) = populated();
    z.add_outside_card(pid(2), OutsidePool::Companion, cid(1))
        .unwrap();
    let live = z.all_object_ids()[3];
    let mut ex = IdGenerator::<ObjectId>::from_last_issued(u64::MAX);
    let before = z.clone();
    assert_eq!(
        z.create_object(&mut ex, cid(2), pid(1), req(ZoneKind::Hand)),
        Err(ZoneError::IdExhausted)
    );
    assert_eq!(z, before);
    assert_eq!(ex.last_issued(), u64::MAX);
    let live_zone = z.object(live).unwrap().zone;
    for to in ZoneKind::ALL {
        if ZoneId::of(to, pid(1)) == live_zone {
            // R10 correction for finding F1 (comment 4621941423): a same-zone move in a hand is
            // a reposition that needs no id, so an exhausted generator does not matter.
            assert_eq!(
                z.move_object(&mut ex, live, req(to)).map(|m| m.new),
                Ok(live)
            );
        } else {
            assert_eq!(
                z.move_object(&mut ex, live, req(to)).map(|m| m.new),
                Err(ZoneError::IdExhausted)
            );
        }
        assert_eq!(z, before);
        assert!(z.object(live).is_some());
    }
    assert_eq!(
        z.enter_from_outside(
            &mut ex,
            pid(2),
            cid(1),
            OutsidePool::Companion,
            req(ZoneKind::Exile)
        ),
        Err(ZoneError::IdExhausted)
    );
    assert_eq!(z, before);
    assert_eq!(
        z.outside_cards(pid(2), OutsidePool::Companion),
        vec![cid(1)]
    );
    assert_eq!(ex.last_issued(), u64::MAX);
    // The last id itself can still be issued from u64::MAX - 1.
    let mut nearly = IdGenerator::<ObjectId>::from_last_issued(u64::MAX - 1);
    let m = z
        .move_object(&mut nearly, live, req(ZoneKind::Exile))
        .unwrap();
    assert_eq!(m.new, oid(u64::MAX));
    assert_eq!(z.object(oid(u64::MAX)).unwrap().zone, ZoneId::Exile);
}

// REQ-CORE-002: ZoneError implements Display (non-empty) and std::error::Error.
#[test]
fn zone_error_is_a_std_error() {
    fn takes_error<E: std::error::Error>(e: &E) -> String {
        e.to_string()
    }
    assert!(!takes_error(&ZoneError::IdExhausted).is_empty());
    assert!(!takes_error(&ZoneError::UnknownObject(oid(1))).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Item 11: snapshots and canonical encoding.
// ---------------------------------------------------------------------------------------------

/// A deterministic operation sequence; `variant` changes one library order.
fn build(variant: bool) -> Zones {
    let mut z = Zones::new(&[pid(1), pid(2), pid(3)], ZonesConfig::default());
    let mut g = IdGenerator::<ObjectId>::new();
    z.create_object(&mut g, cid(1), pid(1), req(ZoneKind::Library))
        .unwrap();
    let pos = if variant {
        Position::Top
    } else {
        Position::Bottom
    };
    z.create_object(&mut g, cid(2), pid(1), req_pos(ZoneKind::Library, pos))
        .unwrap();
    let b = z
        .create_object(
            &mut g,
            cid(3),
            pid(2),
            req_ctrl(ZoneKind::Battlefield, pid(3)),
        )
        .unwrap();
    z.move_object(&mut g, b, req(ZoneKind::Graveyard)).unwrap();
    z.add_outside_card(pid(3), OutsidePool::Spellbook, cid(4))
        .unwrap();
    let mut r = req(ZoneKind::Exile);
    r.face_down = true;
    z.create_object(&mut g, cid(5), pid(3), r).unwrap();
    z
}

// REQ-CORE-002: a clone is an independent snapshot.
#[test]
fn clone_is_independent() {
    let original = build(false);
    let copy = original.clone();
    let mut changed = original.clone();
    let mut g = IdGenerator::<ObjectId>::from_last_issued(100);
    let first = changed.all_object_ids()[0];
    changed
        .move_object(&mut g, first, req(ZoneKind::Exile))
        .unwrap();
    changed
        .add_outside_card(pid(1), OutsidePool::Sideboard, cid(50))
        .unwrap();
    assert_ne!(changed, original);
    assert_eq!(original, copy);
    assert_eq!(encode_to_vec(&original), encode_to_vec(&copy));
    assert!(original.object(first).is_some());
}

// REQ-CORE-002: identical operation sequences give equal containers and identical canonical
// bytes; any difference in content or order changes the bytes; encoding is repeatable.
#[test]
fn canonical_encoding_is_deterministic_and_sensitive() {
    let a = build(false);
    let b = build(false);
    assert_eq!(a, b);
    let bytes = encode_to_vec(&a);
    assert!(!bytes.is_empty());
    assert_eq!(bytes, encode_to_vec(&b));
    for _ in 0..10 {
        assert_eq!(encode_to_vec(&build(false)), bytes);
        assert_eq!(encode_to_vec(&a), bytes);
    }
    // Different library order, same ids and cards.
    let c = build(true);
    assert_ne!(a, c);
    assert_eq!(a.all_object_ids(), c.all_object_ids());
    assert_ne!(encode_to_vec(&c), bytes);
    // Different content.
    let mut d = build(false);
    d.add_outside_card(pid(1), OutsidePool::Lesson, cid(77))
        .unwrap();
    assert_ne!(encode_to_vec(&d), bytes);
    // Different graveyard order.
    let mut e1 = Zones::new(&[pid(1)], ZonesConfig::default());
    let mut e2 = Zones::new(&[pid(1)], ZonesConfig::default());
    let mut g1 = IdGenerator::<ObjectId>::new();
    let mut g2 = IdGenerator::<ObjectId>::new();
    e1.create_object(&mut g1, cid(1), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    e1.create_object(&mut g1, cid(2), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    e2.create_object(&mut g2, cid(1), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    e2.create_object(
        &mut g2,
        cid(2),
        pid(1),
        req_pos(ZoneKind::Graveyard, Position::Bottom),
    )
    .unwrap();
    assert_ne!(e1, e2);
    assert_ne!(encode_to_vec(&e1), encode_to_vec(&e2));
    // Different face-down state.
    let mut f1 = Zones::new(&[pid(1)], ZonesConfig::default());
    let mut f2 = Zones::new(&[pid(1)], ZonesConfig::default());
    let mut h1 = IdGenerator::<ObjectId>::new();
    let mut h2 = IdGenerator::<ObjectId>::new();
    f1.create_object(&mut h1, cid(1), pid(1), req(ZoneKind::Exile))
        .unwrap();
    let mut r = req(ZoneKind::Exile);
    r.face_down = true;
    f2.create_object(&mut h2, cid(1), pid(1), r).unwrap();
    assert_ne!(encode_to_vec(&f1), encode_to_vec(&f2));
}
