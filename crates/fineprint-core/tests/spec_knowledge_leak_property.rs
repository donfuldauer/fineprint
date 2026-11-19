// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The leak / non-interference property of the redacted projection V(S, P) over generated
//! states and knowledge sets (REQ-CORE-004 acceptance criterion 1; REQ-ZONE-001; CR 400.2,
//! 400.7, 401.2, 401.3, 402.3, 406.3, 701.24a, 701.24b, 708.5, 708.9). Plain loops driven by a
//! seeded LCG (R3): 8 seeds x 2 player counts x 250 steps. Black-box: written from the
//! work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::knowledge::{KnowledgeError, ZonesView};
use fineprint_core::zones::{MoveRequest, Position, ZoneId, ZoneKind};
use support::*;

const STEPS: usize = 250;
const SEEDS: [u64; 8] = [1, 2, 3, 17, 99, 1234, 0xDEAD_BEEF, u64::MAX];
/// CardIds of relabelled hidden cards in a twin world start here (far above generated ones).
const TWIN_CARD_BASE: u64 = 1_000_000_000;

fn random_position(rng: &mut Lcg) -> Position {
    match rng.below(3) {
        0 => Position::Top,
        1 => Position::Bottom,
        _ => Position::Index(rng.idx(6)),
    }
}

fn random_request(rng: &mut Lcg, players: &[PlayerId]) -> MoveRequest {
    let to = rng.pick(&ZoneKind::ALL);
    let face_down = to.is_public() && rng.chance(1, 2);
    let controller = if matches!(to, ZoneKind::Battlefield | ZoneKind::Stack) && rng.chance(1, 2) {
        Some(rng.pick(players))
    } else {
        None
    };
    MoveRequest {
        to,
        position: random_position(rng),
        face_down,
        controller,
    }
}

fn face_down_in(w: &KWorld, kinds: &[ZoneKind]) -> Vec<ObjectId> {
    w.zones
        .all_object_ids()
        .into_iter()
        .filter(|id| {
            let r = w.rec(*id);
            r.face_down && kinds.contains(&r.zone.kind())
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// one generated step (valid calls, plus some invalid ones that must be rejected unchanged)
// ---------------------------------------------------------------------------------------------

fn step(w: &mut KWorld, rng: &mut Lcg) {
    let players = w.players();
    let all = w.zones.all_object_ids();
    let roll = rng.below(100);
    if all.is_empty() || roll < 22 {
        let req = random_request(rng, &players);
        let owner = rng.pick(&players);
        w.create_req(owner, req);
    } else if roll < 50 {
        do_move(w, rng, &players, &all);
    } else if roll < 55 {
        let id = rng.pick(&all);
        w.k.reveal_to_all(&w.zones, id).expect("reveal");
    } else if roll < 65 {
        let id = rng.pick(&all);
        let p = rng.pick(&players);
        let before = w.known_to(id);
        w.k.add_known(&w.zones, id, p).expect("add_known");
        let mut expected = before;
        expected.insert(p);
        assert_eq!(w.known_to(id), expected, "add_known stores exactly p");
    } else if roll < 70 {
        let fds = face_down_in(w, &[ZoneKind::Exile]);
        if !fds.is_empty() {
            let id = rng.pick(&fds);
            let p = rng.pick(&players);
            w.k.grant_look(&w.zones, id, p).expect("grant");
            assert!(w.known_to(id).contains(&p));
        }
    } else if roll < 80 {
        do_set_position(w, rng, &players);
    } else if roll < 86 {
        do_shuffle_library(w, rng, &players);
    } else if roll < 89 {
        let fds = face_down_in(w, &[ZoneKind::Exile]);
        let pile: Vec<ObjectId> = fds.into_iter().filter(|_| rng.chance(1, 2)).collect();
        w.k.shuffle_face_down_pile(&w.zones, &pile).expect("pile");
        for id in &pile {
            assert!(w.known_to(*id).is_empty(), "CR 406.3: pile KnownTo cleared");
        }
    } else if roll < 96 {
        invalid_call(w, rng, &players, &all);
    } else if roll < 98 {
        let p = rng.pick(&players);
        w.k.on_player_leaves(&w.zones, p).expect("leave");
        let all_set: BTreeSet<PlayerId> = players.iter().copied().collect();
        for id in face_down_in(w, &[ZoneKind::Battlefield, ZoneKind::Stack]) {
            if w.rec(id).owner == p {
                assert_eq!(w.known_to(id), all_set, "CR 708.9 player leaves");
            }
        }
    } else if rng.chance(1, 4) {
        w.k.on_game_end(&w.zones);
        let all_set: BTreeSet<PlayerId> = players.iter().copied().collect();
        for id in face_down_in(w, &[ZoneKind::Battlefield, ZoneKind::Stack]) {
            assert_eq!(w.known_to(id), all_set, "CR 708.9 game end");
        }
    }
}

/// A move followed by `on_move`, checking rules (a)-(d) exactly.
fn do_move(w: &mut KWorld, rng: &mut Lcg, players: &[PlayerId], all: &[ObjectId]) {
    let id = rng.pick(all);
    let mut req = random_request(rng, players);
    if rng.chance(1, 5) {
        // bias toward same-zone repositioning
        req.to = w.rec(id).zone.kind();
        req.face_down = req.to.is_public() && w.rec(id).face_down;
        if !matches!(req.to, ZoneKind::Battlefield | ZoneKind::Stack) {
            req.controller = None;
        }
    }
    let before = w.rec(id);
    let known_before = w.known_to(id);
    let moved = w.mv(id, req);
    let all_set: BTreeSet<PlayerId> = players.iter().copied().collect();
    let mut expected = known_before;
    // (a) CR 400.2
    if before.zone.kind().is_public() && !before.face_down {
        expected.extend(all_set.iter().copied());
    }
    // (b) CR 402.3
    if before.zone == ZoneId::Hand(before.owner) {
        expected.insert(before.owner);
    }
    // (c) CR 708.5, 708.9
    if before.face_down && matches!(before.zone, ZoneId::Battlefield | ZoneId::Stack) {
        expected.insert(before.controller);
        let to_bf = moved.to == ZoneId::Battlefield;
        let reveal = moved.from != moved.to
            && (before.zone == ZoneId::Battlefield || (before.zone == ZoneId::Stack && !to_bf));
        if reveal {
            expected.extend(all_set.iter().copied());
        }
    }
    assert_eq!(
        w.known_to(moved.new),
        expected,
        "on_move {before:?} -> {moved:?}"
    );
    // (d) CR 401.2
    if let ZoneId::Library(owner) = before.zone {
        for v in players {
            let pos = w.positions(*v, owner);
            assert!(
                !pos.values().any(|o| *o == moved.old),
                "entry naming the moved card dropped"
            );
        }
    }
}

fn do_set_position(w: &mut KWorld, rng: &mut Lcg, players: &[PlayerId]) {
    let owner = rng.pick(players);
    let lib = w.library(owner);
    if lib.is_empty() {
        return;
    }
    let viewer = rng.pick(players);
    let idx = rng.idx(lib.len());
    w.k.set_position_known(&w.zones, viewer, owner, idx, lib[idx])
        .expect("pos");
    assert_eq!(w.positions(viewer, owner).get(&idx), Some(&lib[idx]));
    assert!(w.known_to(lib[idx]).contains(&viewer));
}

/// Physically reorders a library (same-zone moves told to the knowledge), then shuffles the
/// knowledge with a random excluded subset (CR 701.24a, 701.24b).
fn do_shuffle_library(w: &mut KWorld, rng: &mut Lcg, players: &[PlayerId]) {
    let owner = rng.pick(players);
    let lib = w.library(owner);
    for _ in 0..rng.below(3) {
        if lib.is_empty() {
            break;
        }
        let id = rng.pick(&lib);
        w.mv(id, KWorld::req_at(ZoneKind::Library, random_position(rng)));
    }
    let lib = w.library(owner);
    let excluded: Vec<ObjectId> = lib.iter().copied().filter(|_| rng.chance(1, 4)).collect();
    let before: BTreeMap<PlayerId, BTreeMap<usize, ObjectId>> = players
        .iter()
        .map(|v| (*v, w.positions(*v, owner)))
        .collect();
    let known_before: Vec<BTreeSet<PlayerId>> = lib.iter().map(|id| w.known_to(*id)).collect();
    w.k.shuffle_library(&w.zones, owner, &excluded)
        .expect("shuffle");
    for v in players {
        let expected: BTreeMap<usize, ObjectId> = before[v]
            .iter()
            .filter(|(_, o)| excluded.contains(o))
            .map(|(i, o)| (*i, *o))
            .collect();
        assert_eq!(w.positions(*v, owner), expected, "CR 701.24b");
    }
    let known_after: Vec<BTreeSet<PlayerId>> = lib.iter().map(|id| w.known_to(*id)).collect();
    assert_eq!(known_before, known_after, "a shuffle never touches KnownTo");
}

/// Invalid calls: each is an error and leaves the knowledge unchanged (R12).
fn invalid_call(w: &mut KWorld, rng: &mut Lcg, players: &[PlayerId], all: &[ObjectId]) {
    let snapshot = w.k.clone();
    let id = rng.pick(all);
    let r = w.rec(id);
    let p = rng.pick(players);
    match rng.below(6) {
        0 => {
            let res = w.k.grant_look(&w.zones, id, p);
            if r.face_down && r.zone == ZoneId::Exile {
                assert_eq!(res, Ok(()));
                return;
            }
            assert_eq!(res, Err(KnowledgeError::NotFaceDown(id)));
        }
        1 => assert_eq!(
            w.k.add_known(&w.zones, id, NOBODY),
            Err(KnowledgeError::UnknownPlayer(NOBODY))
        ),
        2 => {
            let bad = ObjectId::new(u64::MAX - rng.below(1000));
            assert_eq!(
                w.k.reveal_to_all(&w.zones, bad),
                Err(KnowledgeError::UnknownObject(bad))
            );
        }
        3 => {
            let lib = w.library(p);
            if lib.is_empty() {
                return;
            }
            let i = rng.idx(lib.len());
            let wrong = (i + 1 + rng.idx(5)) % (lib.len() + 5);
            if lib.get(wrong) == Some(&lib[i]) {
                return;
            }
            assert_eq!(
                w.k.set_position_known(&w.zones, p, p, wrong, lib[i]),
                Err(KnowledgeError::IndexMismatch {
                    owner: p,
                    index: wrong,
                    object: lib[i]
                })
            );
        }
        4 => {
            if r.zone == ZoneId::Library(p) {
                return;
            }
            assert_eq!(
                w.k.shuffle_library(&w.zones, p, &[id]),
                Err(KnowledgeError::NotInLibrary {
                    owner: p,
                    object: id
                })
            );
        }
        _ => {
            if r.face_down {
                return;
            }
            let mut pile = face_down_in(w, &[ZoneKind::Exile]);
            pile.push(id);
            assert_eq!(
                w.k.shuffle_face_down_pile(&w.zones, &pile),
                Err(KnowledgeError::NotFaceDown(id))
            );
        }
    }
    assert_eq!(w.k, snapshot, "a rejected call changes nothing");
}

// ---------------------------------------------------------------------------------------------
// the checks L1-L5
// ---------------------------------------------------------------------------------------------

/// The CardIds `viewer` is entitled to (oracle, rules 1-4).
fn entitled_cards(w: &KWorld, viewer: PlayerId) -> BTreeSet<CardId> {
    w.zones
        .all_object_ids()
        .into_iter()
        .filter(|id| oracle_can(&w.zones, &w.k, viewer, *id))
        .map(|id| w.card(id))
        .collect()
}

/// L1 and L2 for one viewer.
fn check_l1_l2(w: &KWorld, viewer: PlayerId, ctx: &str) -> ZonesView {
    let v = w.view(viewer);
    // L1: exact agreement with the independent oracle projection (soundness and completeness)
    assert_eq!(v, oracle_project(&w.zones, &w.k, viewer), "L1 {ctx}");
    for id in w.zones.all_object_ids() {
        assert_eq!(
            w.can(viewer, id),
            oracle_can(&w.zones, &w.k, viewer, id),
            "L1 can_identify {ctx} {:?}",
            w.rec(id)
        );
    }
    // L2: structural walk
    for o in object_views(&v) {
        let r = w.zones.object(o.id).expect("L2: view ids are live objects");
        let allowed = r.zone.kind().is_public() || r.zone == ZoneId::Hand(viewer);
        assert!(allowed, "L2 {ctx}: leaked ObjectId of {r:?}");
        if let Some(c) = o.card {
            assert_eq!(c, r.card, "L2 {ctx}: card belongs to the object");
        }
    }
    let entitled = entitled_cards(w, viewer);
    for c in view_cards(&v) {
        assert!(entitled.contains(&c), "L2 {ctx}: leaked {c:?}");
    }
    // every object of a public zone or of the own hand appears exactly once
    let shown: Vec<ObjectId> = object_views(&v).iter().map(|o| o.id).collect();
    let shown_set: BTreeSet<ObjectId> = shown.iter().copied().collect();
    assert_eq!(shown.len(), shown_set.len(), "L2 {ctx}: duplicate");
    let visible_objects: BTreeSet<ObjectId> = w
        .zones
        .all_object_ids()
        .into_iter()
        .filter(|id| {
            let r = w.rec(*id);
            r.zone.kind().is_public() || r.zone == ZoneId::Hand(viewer)
        })
        .collect();
    assert_eq!(shown_set, visible_objects, "L2 {ctx}");
    v
}

/// Builds a world that differs from `w` only in what `viewer` cannot know: CardIds of objects
/// the viewer cannot identify are relabelled, every library is re-ordered, and other players'
/// KnownTo and position knowledge are replaced by random ones. `relabel_visible` additionally
/// relabels one object the viewer CAN identify (for the mutation self-check).
fn twin(w: &KWorld, viewer: PlayerId, rng: &mut Lcg, relabel_visible: Option<ObjectId>) -> KWorld {
    let players = w.players();
    let mut t = KWorld::with_players(&players);
    // final order of every ordered zone in the twin
    let mut order: BTreeMap<ZoneId, Vec<ObjectId>> = BTreeMap::new();
    for p in &players {
        let mut lib = w.library(*p);
        // Fisher-Yates
        for i in (1..lib.len()).rev() {
            let j = rng.idx(i + 1);
            lib.swap(i, j);
        }
        order.insert(ZoneId::Library(*p), lib);
        order.insert(
            ZoneId::Graveyard(*p),
            w.zones.contents(ZoneId::Graveyard(*p)),
        );
    }
    order.insert(ZoneId::Stack, w.zones.contents(ZoneId::Stack));
    let mut placed: BTreeMap<ZoneId, Vec<ObjectId>> = BTreeMap::new();
    let mut next_hidden = TWIN_CARD_BASE;
    for id in w.zones.all_object_ids() {
        let r = w.rec(id);
        let visible = oracle_can(&w.zones, &w.k, viewer, id);
        let card = if !visible || relabel_visible == Some(id) {
            next_hidden += 1;
            cid(next_hidden)
        } else {
            r.card
        };
        let position = match order.get(&r.zone) {
            Some(final_order) => {
                let rank = |o: &ObjectId| final_order.iter().position(|x| x == o).expect("in");
                let me = rank(&id);
                let before = placed
                    .get(&r.zone)
                    .map_or(0, |v| v.iter().filter(|o| rank(o) < me).count());
                Position::Index(before)
            }
            None => Position::Top,
        };
        placed.entry(r.zone).or_default().push(id);
        let req = MoveRequest {
            to: r.zone.kind(),
            position,
            face_down: r.face_down,
            controller: matches!(r.zone, ZoneId::Battlefield | ZoneId::Stack)
                .then_some(r.controller),
        };
        t.ids = IdGenerator::from_last_issued(id.get() - 1);
        let new = t
            .zones
            .create_object(&mut t.ids, card, r.owner, req)
            .expect("twin create");
        assert_eq!(new, id, "twin keeps object ids");
        if w.known_to(id).contains(&viewer) {
            t.k.add_known(&t.zones, id, viewer).expect("twin add");
        }
    }
    for zone in order.keys() {
        if !matches!(zone, ZoneId::Library(_)) {
            assert_eq!(
                t.zones.contents(*zone),
                w.zones.contents(*zone),
                "twin order"
            );
        }
    }
    // random knowledge for the other players
    let others: Vec<PlayerId> = players.iter().copied().filter(|p| *p != viewer).collect();
    if !others.is_empty() {
        for id in t.zones.all_object_ids() {
            if rng.chance(1, 3) {
                let p = rng.pick(&others);
                t.k.add_known(&t.zones, id, p).expect("twin other add");
            }
        }
        for _ in 0..rng.below(4) {
            let owner = rng.pick(&players);
            let lib = t.library(owner);
            if lib.is_empty() {
                continue;
            }
            let v = rng.pick(&others);
            let i = rng.idx(lib.len());
            t.k.set_position_known(&t.zones, v, owner, i, lib[i])
                .expect("twin pos");
        }
    }
    t
}

/// L3 non-interference, plus the deliberate-mutation self-check.
fn check_l3(w: &KWorld, viewer: PlayerId, v: &ZonesView, rng: &mut Lcg, ctx: &str) {
    let t = twin(w, viewer, rng, None);
    assert_eq!(&t.view(viewer), v, "L3 {ctx}: hidden information changed V");
    // self-check 1: revealing a hidden card to the viewer changes V
    let hidden: Vec<ObjectId> = t
        .zones
        .all_object_ids()
        .into_iter()
        .filter(|id| !oracle_can(&t.zones, &t.k, viewer, *id))
        .collect();
    if !hidden.is_empty() {
        let mut m = t.clone();
        let id = rng.pick(&hidden);
        m.k.add_known(&m.zones, id, viewer).expect("add");
        assert_ne!(
            &m.view(viewer),
            v,
            "L3 self-check {ctx}: reveal must change V"
        );
    }
    // self-check 2: a new card in the viewer's hand changes V
    let mut m = t.clone();
    m.create(ZoneKind::Hand, viewer);
    assert_ne!(
        &m.view(viewer),
        v,
        "L3 self-check {ctx}: hand change must change V"
    );
    // self-check 3: relabelling a card the viewer can identify changes V
    let visible: Vec<ObjectId> = w
        .zones
        .all_object_ids()
        .into_iter()
        .filter(|id| oracle_can(&w.zones, &w.k, viewer, *id))
        .collect();
    if !visible.is_empty() {
        let id = rng.pick(&visible);
        let m = twin(w, viewer, rng, Some(id));
        assert_ne!(
            &m.view(viewer),
            v,
            "L3 self-check {ctx}: known card relabel"
        );
    }
}

/// L5: every reported position matches the live library.
fn check_positions_live(w: &KWorld, ctx: &str) {
    let players = w.players();
    for owner in &players {
        let lib = w.library(*owner);
        for v in &players {
            for (i, id) in w.positions(*v, *owner) {
                assert_eq!(lib.get(i), Some(&id), "L5 {ctx}: stale position reported");
            }
        }
    }
}

fn run(seed: u64, players: &[PlayerId]) -> (Vec<ZonesView>, Vec<u8>) {
    let mut rng = Lcg::new(seed);
    let mut twin_rng = Lcg::new(seed.wrapping_add(0x5151));
    let mut w = KWorld::with_players(players);
    for s in 0..STEPS {
        step(&mut w, &mut rng);
        let ctx = format!("seed {seed} players {} step {s}", players.len());
        check_positions_live(&w, &ctx);
        for p in players {
            let v = check_l1_l2(&w, *p, &ctx);
            check_l3(&w, *p, &v, &mut twin_rng, &ctx);
        }
    }
    let views = players.iter().map(|p| w.view(*p)).collect();
    (views, bytes(&w.k))
}

/// REQ-CORE-004 (acceptance criterion 1), CR 400.2, 401.2, 402.3, 406.3, 708.5, 708.9: over
/// generated two-player states, V(S, P) contains exactly what P is entitled to (L1, L2), and
/// changes to hidden information never change it (L3); positions are always live (L5).
#[test]
fn leak_property_two_players() {
    for seed in SEEDS {
        run(seed, &[P1, P2]);
    }
}

/// REQ-CORE-004 (acceptance criterion 1), CR 400.2, 401.2, 402.3, 406.3, 708.5, 708.9: the
/// same property over generated three-player states.
#[test]
fn leak_property_three_players() {
    for seed in SEEDS {
        run(seed, &[P1, P2, P3]);
    }
}

/// REQ-CORE-004, REQ-CORE-001 (L5): the same seed gives the same final views and the same
/// canonical bytes of the Knowledge.
#[test]
fn leak_property_deterministic() {
    for seed in [SEEDS[0], SEEDS[6]] {
        for players in [vec![P1, P2], vec![P1, P2, P3]] {
            assert_eq!(run(seed, &players), run(seed, &players), "seed {seed}");
        }
    }
}

/// REQ-CORE-004, CR 401.2, 701.24a (L4): library views never expose order; two libraries with
/// the same cards in different order project equally for a viewer who knows only the set.
#[test]
fn library_view_never_exposes_order() {
    let build = |order: &[u64]| -> KWorld {
        let mut w = KWorld::two();
        for n in order {
            let id = w
                .zones
                .create_object(
                    &mut w.ids,
                    cid(*n),
                    P2,
                    KWorld::req_at(ZoneKind::Library, Position::Bottom),
                )
                .expect("create");
            w.k.add_known(&w.zones, id, P1).expect("add");
        }
        w
    };
    let a = build(&[10, 11, 12, 13]);
    let b = build(&[13, 11, 10, 12]);
    for p in [P1, P2] {
        assert_eq!(a.view(p), b.view(p), "{p:?}");
    }
    let v = a.view(P1);
    assert_eq!(
        lib_known(&v, P2),
        set(&[cid(10), cid(11), cid(12), cid(13)])
    );
    assert_eq!(v.players[&P2].library.count, 4);
}
