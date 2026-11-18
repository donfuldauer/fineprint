// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::zones`. Black-box: written from the work-item brief only.
//!
//! Zone order: library, graveyard and stack order preserved through moves, positional
//! insertion, unordered zones listed by ascending id, and the graveyard-reorder format flag
//! (REQ-ZONE-001 items 1, 2, 4, 5, 6; REQ-CORE-002).

use std::collections::BTreeMap;

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

fn two() -> Zones {
    Zones::new(&[pid(1), pid(2)], ZonesConfig::default())
}

fn two_reorderable() -> Zones {
    Zones::new(
        &[pid(1), pid(2)],
        ZonesConfig {
            free_graveyard_reorder: true,
        },
    )
}

/// Fills `zone_kind` of player 1 with `n` objects (cards `first_card..`), each created with
/// `position`; returns the ids in creation order.
fn fill(
    z: &mut Zones,
    g: &mut IdGenerator<ObjectId>,
    kind: ZoneKind,
    n: u64,
    first_card: u64,
    position: fn() -> Position,
) -> Vec<ObjectId> {
    (0..n)
        .map(|i| {
            z.create_object(g, cid(first_card + i), pid(1), req_pos(kind, position()))
                .unwrap()
        })
        .collect()
}

fn bottom() -> Position {
    Position::Bottom
}

fn top() -> Position {
    Position::Top
}

fn default_pos() -> Position {
    MoveRequest::new(ZoneKind::Library).position
}

// ---------------------------------------------------------------------------------------------
// Item 4: ordered zones.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 item 1, CR 400.5, 401.2: creating with Bottom gives creation order top to bottom;
// creating with Top gives the reverse.
#[test]
fn library_creation_with_bottom_and_top() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let ids = fill(&mut z, &mut g, ZoneKind::Library, 6, 1, bottom);
    assert_eq!(z.contents(ZoneId::Library(pid(1))), ids);
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let ids = fill(&mut z, &mut g, ZoneKind::Library, 6, 1, top);
    let mut rev = ids.clone();
    rev.reverse();
    assert_eq!(z.contents(ZoneId::Library(pid(1))), rev);
}

/// Checks, for one ordered zone kind, that moving a middle object out keeps the remaining
/// order and that moving it back lands exactly at Top / Bottom / Index(k) / oversized index.
fn check_out_and_back(kind: ZoneKind) {
    let zone = ZoneId::of(kind, pid(1));
    let n = 7_usize;
    let positions: Vec<(Position, Option<usize>)> = vec![
        (Position::Top, Some(0)),
        (Position::Bottom, None),
        (Position::Index(0), Some(0)),
        (Position::Index(1), Some(1)),
        (Position::Index(3), Some(3)),
        (Position::Index(n - 1), None), // == remaining len: bottom
        (Position::Index(n), None),
        (Position::Index(n + 5), None),
        (Position::Index(usize::MAX), None),
    ];
    for (pos, expected_index) in positions {
        let mut z = two();
        let mut g = IdGenerator::<ObjectId>::new();
        let ids = fill(&mut z, &mut g, kind, n as u64, 1, bottom);
        assert_eq!(z.contents(zone), ids, "{kind:?} fill");
        // Move the middle object to exile.
        let mid = ids[3];
        let out = z.move_object(&mut g, mid, req(ZoneKind::Exile)).unwrap();
        let mut remaining = ids.clone();
        remaining.remove(3);
        assert_eq!(z.contents(zone), remaining, "{kind:?} after removal");
        // Untouched objects keep their ids.
        for id in &remaining {
            assert!(z.object(*id).is_some());
        }
        // Move it back.
        let back = z
            .move_object(&mut g, out.new, req_pos(kind, pos))
            .unwrap()
            .new;
        let mut expected = remaining.clone();
        match expected_index {
            Some(i) => expected.insert(i, back),
            None => expected.push(back),
        }
        assert_eq!(
            z.contents(zone),
            expected,
            "{kind:?} back at {expected_index:?}"
        );
    }
}

// REQ-ZONE-001 items 1, 4, CR 400.5, 401.2, 401.7: library order through moves; Index(0) is
// the top, Index(len) and larger is the bottom.
#[test]
fn library_order_preserved_and_positions_exact() {
    check_out_and_back(ZoneKind::Library);
}

// REQ-ZONE-001 item 4, CR 400.5, 404.1, 404.2: graveyard order through moves.
#[test]
fn graveyard_order_preserved_and_positions_exact() {
    check_out_and_back(ZoneKind::Graveyard);
}

// REQ-ZONE-001, CR 400.5, 405.2: stack order through moves.
#[test]
fn stack_order_preserved_and_positions_exact() {
    check_out_and_back(ZoneKind::Stack);
}

// REQ-ZONE-001 item 4, CR 404.1: new graveyard entries default to the top.
#[test]
fn graveyard_entries_default_to_top() {
    assert!(matches!(default_pos(), Position::Top));
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    let b = z
        .create_object(&mut g, cid(2), pid(1), req(ZoneKind::Battlefield))
        .unwrap();
    let c = z
        .create_object(&mut g, cid(3), pid(1), req(ZoneKind::Hand))
        .unwrap();
    let b2 = z
        .move_object(&mut g, b, req(ZoneKind::Graveyard))
        .unwrap()
        .new;
    let c2 = z
        .move_object(&mut g, c, req(ZoneKind::Graveyard))
        .unwrap()
        .new;
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), vec![c2, b2, a]);
}

// REQ-ZONE-001, CR 405.2, 405.5: each object put on the stack goes on top; the top is listed
// first.
#[test]
fn stack_entries_default_to_top() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Stack))
        .unwrap();
    let b = z
        .create_object(&mut g, cid(2), pid(2), req(ZoneKind::Stack))
        .unwrap();
    let h = z
        .create_object(&mut g, cid(3), pid(1), req(ZoneKind::Hand))
        .unwrap();
    let c = z.move_object(&mut g, h, req(ZoneKind::Stack)).unwrap().new;
    assert_eq!(z.contents(ZoneId::Stack), vec![c, b, a]);
    // Resolving the top (moving it away) exposes the next one.
    z.move_object(&mut g, c, req(ZoneKind::Graveyard)).unwrap();
    assert_eq!(z.contents(ZoneId::Stack), vec![b, a]);
}

// REQ-ZONE-001 item 1, CR 400.5, 401.2, 400.7: moving an object within its own library is not a
// zone change: the same object (same id) is re-inserted at the requested position of the
// remaining pile (R10 correction for finding F1, CodeRabbit comment 4621941423).
#[test]
fn library_to_library_move_reinserts_at_position() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let ids = fill(&mut z, &mut g, ZoneKind::Library, 5, 1, bottom);
    let m = z
        .move_object(&mut g, ids[0], req_pos(ZoneKind::Library, Position::Bottom))
        .unwrap();
    assert_eq!(m.new, ids[0]);
    assert_eq!(g.last_issued(), 5);
    assert_eq!(
        z.contents(ZoneId::Library(pid(1))),
        vec![ids[1], ids[2], ids[3], ids[4], m.new]
    );
    let m2 = z
        .move_object(
            &mut g,
            ids[3],
            req_pos(ZoneKind::Library, Position::Index(1)),
        )
        .unwrap();
    assert_eq!(m2.new, ids[3]);
    assert_eq!(
        z.contents(ZoneId::Library(pid(1))),
        vec![ids[1], m2.new, ids[2], ids[4], m.new]
    );
}

// REQ-ZONE-001 items 1, 4, CR 400.3, 400.5: moving objects in other zones and other players'
// zones never perturbs an untouched library, graveyard or stack.
#[test]
fn moves_elsewhere_do_not_perturb_ordered_zones() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let lib = fill(&mut z, &mut g, ZoneKind::Library, 5, 1, bottom);
    let gy = fill(&mut z, &mut g, ZoneKind::Graveyard, 5, 100, bottom);
    let st = fill(&mut z, &mut g, ZoneKind::Stack, 5, 200, bottom);
    let mut other = Vec::new();
    for (i, kind) in ZoneKind::ALL.into_iter().enumerate() {
        other.push(
            z.create_object(&mut g, cid(300 + i as u64), pid(2), req(kind))
                .unwrap(),
        );
    }
    for (i, id) in other.into_iter().enumerate() {
        let to = ZoneKind::ALL[(i + 3) % 7];
        z.move_object(&mut g, id, req_pos(to, Position::Index(2)))
            .unwrap();
        assert_eq!(z.contents(ZoneId::Library(pid(1))), lib);
        assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), gy);
    }
    // Stack objects owned by player 2 are on top of player 1's; player 1's stay in order.
    let st_now: Vec<ObjectId> = z
        .contents(ZoneId::Stack)
        .into_iter()
        .filter(|id| st.contains(id))
        .collect();
    assert_eq!(st_now, st);
}

// ---------------------------------------------------------------------------------------------
// Item 4 / 5 property test: a reference model of every zone.
// ---------------------------------------------------------------------------------------------

const POOLS: [OutsidePool; 6] = [
    OutsidePool::Sideboard,
    OutsidePool::Companion,
    OutsidePool::Lesson,
    OutsidePool::AttractionDeck,
    OutsidePool::ContraptionDeck,
    OutsidePool::Spellbook,
];

fn kind_index(kind: ZoneKind) -> usize {
    ZoneKind::ALL.iter().position(|k| *k == kind).unwrap()
}

/// Model zone key: (kind index, owner raw for per-player kinds, else 0).
fn key(kind: ZoneKind, owner: PlayerId) -> (usize, u64) {
    let o = if kind.is_per_player() { owner.get() } else { 0 };
    (kind_index(kind), o)
}

#[derive(Default)]
struct Model {
    /// Zone contents; ordered zones top first, unordered in insertion order (compared sorted).
    zones: BTreeMap<(usize, u64), Vec<ObjectId>>,
    /// Live object -> (card, owner, zone key).
    objects: BTreeMap<ObjectId, (CardId, PlayerId, (usize, u64))>,
    /// Outside card -> (owner, pool index).
    outside: BTreeMap<CardId, (PlayerId, usize)>,
}

impl Model {
    fn insert(&mut self, id: ObjectId, card: CardId, owner: PlayerId, to: ZoneKind, p: Pos) {
        let k = key(to, owner);
        let v = self.zones.entry(k).or_default();
        if to.is_ordered() {
            let at = match p {
                Pos::Top => 0,
                Pos::Bottom => v.len(),
                Pos::Index(i) => i.min(v.len()),
            };
            v.insert(at, id);
        } else {
            v.push(id);
        }
        self.objects.insert(id, (card, owner, k));
    }

    /// The zone the model holds `id` in.
    fn zone_of(&self, id: ObjectId) -> ZoneId {
        let (_, owner, (kind_ix, _)) = self.objects[&id];
        let kind = ZoneKind::ALL
            .into_iter()
            .find(|k| kind_index(*k) == kind_ix)
            .unwrap();
        ZoneId::of(kind, owner)
    }

    fn remove(&mut self, id: ObjectId) -> (CardId, PlayerId) {
        let (card, owner, k) = self.objects.remove(&id).unwrap();
        let v = self.zones.get_mut(&k).unwrap();
        let at = v.iter().position(|x| *x == id).unwrap();
        v.remove(at);
        (card, owner)
    }
}

fn pick(rng: &mut Pcg64Dxsm, n: usize) -> usize {
    usize::try_from(rng.next_bounded(n as u64).unwrap()).unwrap()
}

/// A copyable mirror of `Position` for the model.
#[derive(Clone, Copy, Debug)]
enum Pos {
    Top,
    Bottom,
    Index(usize),
}

impl Pos {
    fn position(self) -> Position {
        match self {
            Pos::Top => Position::Top,
            Pos::Bottom => Position::Bottom,
            Pos::Index(i) => Position::Index(i),
        }
    }
}

fn random_pos(rng: &mut Pcg64Dxsm) -> Pos {
    match pick(rng, 5) {
        0 => Pos::Top,
        1 => Pos::Bottom,
        2 => Pos::Index(usize::MAX),
        _ => Pos::Index(pick(rng, 12)),
    }
}

fn request(to: ZoneKind, p: Pos) -> MoveRequest {
    req_pos(to, p.position())
}

fn check_against_model(z: &Zones, m: &Model, players: &[PlayerId], seed: u64, step: usize) {
    for kind in ZoneKind::ALL {
        let owners: Vec<PlayerId> = if kind.is_per_player() {
            players.to_vec()
        } else {
            vec![pid(0)]
        };
        for owner in owners {
            let zone = ZoneId::of(kind, owner);
            let mut expected = m.zones.get(&key(kind, owner)).cloned().unwrap_or_default();
            if !kind.is_ordered() {
                expected.sort();
            }
            assert_eq!(
                z.contents(zone),
                expected,
                "seed {seed} step {step} zone {zone:?}"
            );
            assert_eq!(z.len(zone), expected.len());
        }
    }
    let live: Vec<ObjectId> = m.objects.keys().copied().collect();
    assert_eq!(z.all_object_ids(), live, "seed {seed} step {step}");
    for (id, (card, owner, _)) in &m.objects {
        let rec = z.object(*id).unwrap();
        assert_eq!((rec.card, rec.owner), (*card, *owner));
    }
}

// REQ-ZONE-001 items 1, 2, 4, 5, 6, REQ-CORE-002, CR 400.3, 400.5, 400.7, 401.7, 404.1, 405.2: over
// many seeds, random creates, moves (every kind and position) and outside round trips match a
// reference model of every zone: ordered zones keep relative order and honour positions;
// unordered zones list ascending ids; untouched objects are never perturbed.
#[test]
fn property_zone_order_matches_reference_model() {
    for seed in 0..48_u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x5eed_0000 + seed);
        let n_players = 2 + pick(&mut rng, 3);
        let players: Vec<PlayerId> = (1..=n_players as u64).map(pid).collect();
        let mut z = Zones::new(&players, ZonesConfig::default());
        let mut g = IdGenerator::<ObjectId>::new();
        let mut m = Model::default();
        let mut next_card = 1_u64;
        for step in 0..300 {
            let live: Vec<ObjectId> = m.objects.keys().copied().collect();
            let action = pick(&mut rng, 10);
            if live.is_empty() || action < 2 {
                let owner = players[pick(&mut rng, players.len())];
                let to = ZoneKind::ALL[pick(&mut rng, 7)];
                let p = random_pos(&mut rng);
                let mut r = request(to, p);
                r.face_down = pick(&mut rng, 2) == 0;
                let card = cid(next_card);
                next_card += 1;
                let id = z.create_object(&mut g, card, owner, r).unwrap();
                m.insert(id, card, owner, to, p);
            } else if action < 8 {
                let id = live[pick(&mut rng, live.len())];
                let to = ZoneKind::ALL[pick(&mut rng, 7)];
                let p = random_pos(&mut rng);
                let from = z.object(id).unwrap().zone;
                let owner_before = m.objects[&id].1;
                // Expected endpoints from the model's record and the request (CR 400.3).
                let expected_from = m.zone_of(id);
                let expected_to = ZoneId::of(to, owner_before);
                assert_eq!(expected_from, from, "seed {seed} step {step}: model drift");
                let moved = z.move_object(&mut g, id, request(to, p)).unwrap();
                assert_eq!(moved.from, expected_from, "seed {seed} step {step}");
                assert_eq!(moved.to, expected_to, "seed {seed} step {step}");
                // CR 400.7, 400.8, 406.7, 400.10 (finding F1): only a same-zone move outside exile
                // and the command zone keeps the id.
                let keeps = expected_to == expected_from
                    && !matches!(to, ZoneKind::Exile | ZoneKind::Command);
                assert_eq!(moved.new == id, keeps, "seed {seed} step {step}");
                let (card, owner) = m.remove(id);
                m.insert(moved.new, card, owner, to, p);
            } else if action == 8 {
                let id = live[pick(&mut rng, live.len())];
                let pool = pick(&mut rng, POOLS.len());
                let card = z.send_outside(id, POOLS[pool]).unwrap();
                let (mcard, owner) = m.remove(id);
                assert_eq!(card, mcard);
                m.outside.insert(card, (owner, pool));
            } else {
                let outside: Vec<CardId> = m.outside.keys().copied().collect();
                if outside.is_empty() {
                    continue;
                }
                let card = outside[pick(&mut rng, outside.len())];
                let (owner, pool) = m.outside.remove(&card).unwrap();
                let to = ZoneKind::ALL[pick(&mut rng, 7)];
                let p = random_pos(&mut rng);
                let id = z
                    .enter_from_outside(&mut g, owner, card, POOLS[pool], request(to, p))
                    .unwrap();
                m.insert(id, card, owner, to, p);
            }
            check_against_model(&z, &m, &players, seed, step);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Item 9: graveyard reorder flag.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 item 4, CR 400.5, 404.2: with the default config, reordering is not allowed and
// nothing changes.
#[test]
fn reorder_graveyard_not_allowed_by_default() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    assert!(!z.config().free_graveyard_reorder);
    let ids = fill(&mut z, &mut g, ZoneKind::Graveyard, 3, 1, bottom);
    let before = z.clone();
    let mut perm = ids.clone();
    perm.reverse();
    assert_eq!(
        z.reorder_graveyard(pid(1), perm),
        Err(ZoneError::ReorderNotAllowed)
    );
    assert_eq!(
        z.reorder_graveyard(pid(1), ids.clone()),
        Err(ZoneError::ReorderNotAllowed)
    );
    assert_eq!(z, before);
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), ids);
}

// REQ-ZONE-001 item 4, REQ-CORE-002, CR 404.2: with the flag, a valid permutation reorders the
// graveyard top-first and keeps the same ObjectIds (no zone change).
#[test]
fn reorder_graveyard_with_flag_applies_permutation() {
    let mut z = two_reorderable();
    let mut g = IdGenerator::<ObjectId>::new();
    let ids = fill(&mut z, &mut g, ZoneKind::Graveyard, 4, 1, bottom);
    let other = fill(&mut z, &mut g, ZoneKind::Library, 2, 50, bottom);
    let last = g.last_issued();
    let perm = vec![ids[2], ids[0], ids[3], ids[1]];
    z.reorder_graveyard(pid(1), perm.clone()).unwrap();
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), perm);
    for id in &ids {
        assert_eq!(z.object(*id).unwrap().zone, ZoneId::Graveyard(pid(1)));
    }
    assert_eq!(z.contents(ZoneId::Library(pid(1))), other);
    assert_eq!(g.last_issued(), last);
    // Identity permutation is fine too.
    z.reorder_graveyard(pid(1), perm.clone()).unwrap();
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), perm);
    // Empty graveyard, empty list.
    z.reorder_graveyard(pid(2), Vec::new()).unwrap();
    assert!(z.contents(ZoneId::Graveyard(pid(2))).is_empty());
    // Later entries still go on top of the reordered pile (CR 404.1).
    let h = z
        .create_object(&mut g, cid(99), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    let mut expected = vec![h];
    expected.extend(perm);
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), expected);
}

// REQ-ZONE-001 item 4, CR 404.2: missing, extra, duplicated or foreign ids give InvalidOrder;
// unknown player gives UnknownPlayer; state unchanged.
#[test]
fn reorder_graveyard_rejects_invalid_orders() {
    let mut z = two_reorderable();
    let mut g = IdGenerator::<ObjectId>::new();
    let ids = fill(&mut z, &mut g, ZoneKind::Graveyard, 3, 1, bottom);
    let mine_in_library = z
        .create_object(&mut g, cid(10), pid(1), req(ZoneKind::Library))
        .unwrap();
    let theirs = z
        .create_object(&mut g, cid(11), pid(2), req(ZoneKind::Graveyard))
        .unwrap();
    let before = z.clone();
    let bad: Vec<Vec<ObjectId>> = vec![
        vec![ids[0], ids[1]],                          // missing
        vec![ids[0], ids[1], ids[2], oid(500)],        // extra unknown
        vec![ids[0], ids[1], ids[2], ids[2]],          // duplicate + length
        vec![ids[0], ids[1], ids[1]],                  // duplicate, right length
        vec![ids[0], ids[1], mine_in_library],         // other zone
        vec![ids[0], ids[1], theirs],                  // other player's graveyard
        vec![ids[0], ids[1], ids[2], theirs],          // extra foreign
        vec![],                                        // empty
        vec![ids[0], ids[1], ids[2], mine_in_library], // extra other zone
    ];
    for order in bad {
        assert_eq!(
            z.reorder_graveyard(pid(1), order.clone()),
            Err(ZoneError::InvalidOrder),
            "{order:?}"
        );
        assert_eq!(z, before);
    }
    assert_eq!(
        z.reorder_graveyard(pid(3), Vec::new()),
        Err(ZoneError::UnknownPlayer(pid(3)))
    );
    assert_eq!(
        z.reorder_graveyard(pid(3), ids.clone()),
        Err(ZoneError::UnknownPlayer(pid(3)))
    );
    assert_eq!(z, before);
    assert_eq!(z.contents(ZoneId::Graveyard(pid(1))), ids);
}
