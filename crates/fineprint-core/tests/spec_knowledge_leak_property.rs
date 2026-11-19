// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The leak / non-interference property of the redacted projection V(S, P) over generated
//! states and knowledge sets (REQ-CORE-004 acceptance criterion 1; REQ-ZONE-001; CR 400.2,
//! 400.7, 401.2, 401.3, 402.3, 406.3, 701.24a, 701.24b, 708.5, 708.9). Plain loops driven by a
//! seeded LCG (R3): 8 seeds x 2 player counts x 250 steps.
//! L3b (hidden-move twins) has no observation condition based on position knowledge; it also
//! covers removals from a hidden zone (`send_outside` + `on_removed_from_hidden_zone`, PR #174
//! review finding F2). Insertions (`enter_from_outside` / `create_object` +
//! `on_library_changed[_indexed]`) stay position-only. Every library-changing operation draws
//! random `observers` and an independent random `index_known_by` / `known_by`; L3b also has the
//! class "same disclosure, different hidden index" (PR #174 review round 4: insertion, removal,
//! move out, move in, reposition, shuffle with a different excluded card): a player who does
//! not know the index of a library change never learns it from `position_known` or `project`
//! (the brief's index-disclosure rule, a design decision). Black-box: written from the
//! work-item brief only.
//!
//! The oracle is the independent `Shadow` model of the support file: it applies the brief's
//! rules to every generated operation and never asks the implementation for the expected
//! answer. After every step the implementation's `project`, `known_to`, `can_identify` and
//! `position_known` must equal the model's for every player and object (L1).

#[path = "spec_knowledge_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::knowledge::{KnowledgeError, ZonesView};
use fineprint_core::zones::{
    MoveRequest, Moved, ObjectRecord, OutsidePool, Position, ZoneId, ZoneKind,
};
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

/// The world under test plus the independent shadow model kept alongside it.
#[derive(Clone, Debug)]
struct World {
    w: KWorld,
    m: Shadow,
}

impl World {
    fn new(players: &[PlayerId]) -> World {
        World {
            w: KWorld::with_players(players),
            m: Shadow::new(),
        }
    }
}

/// The classes of the "same disclosure, different hidden index" twins (L3b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdxClass {
    Insertion = 0,
    Removal = 1,
    MoveOut = 2,
    MoveIn = 3,
    Reposition = 4,
    Shuffle = 5,
}

impl IdxClass {
    const ALL: [IdxClass; 6] = [
        IdxClass::Insertion,
        IdxClass::Removal,
        IdxClass::MoveOut,
        IdxClass::MoveIn,
        IdxClass::Reposition,
        IdxClass::Shuffle,
    ];
}

/// Counters that keep the hidden-move twin check (L3b) from being vacuous.
#[derive(Debug, Default)]
struct Stats {
    /// (move, non-observer) pairs whose views were compared across hidden-move twins.
    twin_compared: usize,
    /// Hidden-move twins where an observer's views were shown to differ (the control).
    twin_control: usize,
    /// Compared non-observers who held, before the move, a position entry on the moved or on
    /// the alternative card (the case of the removed position-entry observation condition).
    twin_pos_holder_compared: usize,
    /// `send_outside` operations followed by the removal hook (any source zone).
    removals: usize,
    /// Of those, removals from a library or a hand.
    hidden_removals: usize,
    /// (removal, non-observer) pairs whose views were compared across removal twins.
    removal_compared: usize,
    /// Compared non-observers who held, before the removal, a position entry or stored KnownTo
    /// on either removed candidate.
    removal_holder_compared: usize,
    /// Removal twins where an observer's views were shown to differ (the control).
    removal_control: usize,
    /// `enter_from_outside` / `create_object` insertions of `do_insert` (library or hand).
    insertions: usize,
    /// Of those, insertions into a library (followed by `on_library_changed[_indexed]`).
    library_insertions: usize,
    /// Per [`IdxClass`]: "same disclosure, different hidden index" twins built.
    idx_twins: [usize; 6],
    /// Per [`IdxClass`]: (twin, player NOT in the disclosure set) pairs compared.
    idx_compared: [usize; 6],
    /// Per [`IdxClass`]: compared pairs whose player held a position entry in the affected
    /// library before the change.
    idx_holder_compared: [usize; 6],
    /// Per [`IdxClass`]: (twin, player in the disclosure set) pairs whose `position_known`
    /// differed across the twins (the control: L3b is not vacuous).
    idx_control: [usize; 6],
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
// one generated step (valid calls, plus some invalid ones that must be rejected unchanged);
// every valid call is applied to the implementation and to the shadow model
// ---------------------------------------------------------------------------------------------

/// A random player set for `observers`, `index_known_by` or `known_by`: empty, a random subset
/// or every player (each drawn independently).
fn random_set(rng: &mut Lcg, players: &[PlayerId]) -> Vec<PlayerId> {
    match rng.below(3) {
        0 => Vec::new(),
        1 => players
            .iter()
            .copied()
            .filter(|_| rng.chance(1, 2))
            .collect(),
        _ => players.to_vec(),
    }
}

fn step(s: &mut World, rng: &mut Lcg, stats: &mut Stats) {
    let players = s.w.players();
    let all = s.w.zones.all_object_ids();
    let roll = rng.below(100);
    if all.is_empty() || roll < 18 {
        let mut req = random_request(rng, &players);
        if rng.chance(1, 3) {
            // keep libraries large enough for the index twins
            req = KWorld::req_at(ZoneKind::Library, random_position(rng));
        }
        let owner = rng.pick(&players);
        let card = s.w.fresh_card();
        let ikb = random_set(rng, &players);
        let shorthand = ikb.is_empty() && rng.chance(1, 2);
        // an insertion outside on_move (CR 401.2); for a library it is followed by
        // `on_library_changed[_indexed]`
        insert(s, owner, card, req, &ikb, shorthand, false);
    } else if roll < 23 {
        do_insert(s, rng, &players, stats);
    } else if roll < 46 {
        do_move(s, rng, &players, &all, stats);
    } else if roll < 51 {
        do_remove(s, rng, &players, &all, stats);
    } else if roll < 55 {
        let id = rng.pick(&all);
        s.w.k.reveal_to_all(&s.w.zones, id).expect("reveal");
        s.m.reveal_to_all(&s.w.zones, id);
    } else if roll < 65 {
        let id = rng.pick(&all);
        let p = rng.pick(&players);
        s.w.k.add_known(&s.w.zones, id, p).expect("add_known");
        s.m.add_known(&s.w.zones, id, p);
    } else if roll < 70 {
        let fds = face_down_in(&s.w, &[ZoneKind::Exile]);
        if !fds.is_empty() {
            let id = rng.pick(&fds);
            let p = rng.pick(&players);
            s.w.k.grant_look(&s.w.zones, id, p).expect("grant");
            s.m.grant_look(&s.w.zones, id, p);
        }
    } else if roll < 80 {
        do_set_position(s, rng, &players);
    } else if roll < 86 {
        do_shuffle_library(s, rng, &players, stats);
    } else if roll < 89 {
        // CR 406.3, 701.24a: only face-down exiled cards form a pile
        let fds = face_down_in(&s.w, &[ZoneKind::Exile]);
        let pile: Vec<ObjectId> = fds.into_iter().filter(|_| rng.chance(1, 2)).collect();
        s.w.k
            .shuffle_face_down_pile(&s.w.zones, &pile)
            .expect("pile");
        s.m.shuffle_face_down_pile(&s.w.zones, &pile);
    } else if roll < 96 {
        invalid_call(s, rng, &players, &all);
    } else if roll < 98 {
        // CR 708.9
        let p = rng.pick(&players);
        s.w.k.on_player_leaves(&s.w.zones, p).expect("leave");
        s.m.on_player_leaves(&s.w.zones, p);
    } else if rng.chance(1, 4) {
        // CR 708.9
        s.w.k.on_game_end(&s.w.zones);
        s.m.on_game_end(&s.w.zones);
    }
}

/// Moves `id` in the Zones and informs the implementation and the model. The implementation
/// gets `on_move` when `plain` (observers and `index_known_by` empty), `on_move_observed` when
/// `index_known_by` is empty, `on_move_indexed` otherwise; the model always gets the indexed
/// operation (the shorthands are the empty disclosure). With `inject`, rejected calls are tried
/// first. Returns the move and the model's observed set (step 0).
fn apply_move(
    s: &mut World,
    id: ObjectId,
    req: MoveRequest,
    observers: &[PlayerId],
    ikb: &[PlayerId],
    plain: bool,
    inject: Option<&mut Lcg>,
) -> (Moved, Option<BTreeSet<PlayerId>>) {
    let before = s.w.rec(id);
    let moved = s.w.raw_mv(id, req);
    if let Some(rng) = inject {
        inject_move_errors(s, &before, &moved, observers, ikb, rng);
    }
    if plain {
        assert!(observers.is_empty() && ikb.is_empty());
        s.w.k.on_move(&s.w.zones, &before, &moved).expect("on_move");
    } else if ikb.is_empty() {
        s.w.k
            .on_move_observed(&s.w.zones, &before, &moved, observers)
            .expect("on_move_observed");
    } else {
        s.w.k
            .on_move_indexed(&s.w.zones, &before, &moved, observers, ikb)
            .expect("on_move_indexed");
    }
    let observed =
        s.m.on_move_indexed(&s.w.zones, &before, &moved, observers, ikb);
    (moved, observed)
}

/// R12: an unknown moved object is `UnknownObject` (checked first), then an unknown observer
/// `UnknownPlayer` (the first one), then an unknown `index_known_by` player `UnknownPlayer`
/// (the first one); the knowledge is unchanged.
fn inject_move_errors(
    s: &mut World,
    before: &ObjectRecord,
    moved: &Moved,
    observers: &[PlayerId],
    ikb: &[PlayerId],
    rng: &mut Lcg,
) {
    let snapshot = s.w.k.clone();
    let mut bad = observers.to_vec();
    bad.insert(rng.idx(bad.len() + 1), NOBODY);
    let ghost = PlayerId::new(3000 + rng.below(1000));
    let mut bad_ikb = ikb.to_vec();
    bad_ikb.insert(rng.idx(bad_ikb.len() + 1), ghost);
    assert_eq!(
        s.w.k.on_move_observed(&s.w.zones, before, moved, &bad),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        s.w.k
            .on_move_indexed(&s.w.zones, before, moved, &bad, &bad_ikb),
        Err(KnowledgeError::UnknownPlayer(NOBODY)),
        "the observer error comes before the index_known_by error"
    );
    assert_eq!(
        s.w.k
            .on_move_indexed(&s.w.zones, before, moved, observers, &bad_ikb),
        Err(KnowledgeError::UnknownPlayer(ghost))
    );
    let fake = Moved {
        new: ObjectId::new(u64::MAX - rng.below(1000)),
        ..*moved
    };
    assert_eq!(
        s.w.k.on_move_observed(&s.w.zones, before, &fake, &bad),
        Err(KnowledgeError::UnknownObject(fake.new))
    );
    assert_eq!(
        s.w.k
            .on_move_indexed(&s.w.zones, before, &fake, &bad, &bad_ikb),
        Err(KnowledgeError::UnknownObject(fake.new))
    );
    assert_eq!(
        s.w.k.on_move(&s.w.zones, before, &fake),
        Err(KnowledgeError::UnknownObject(fake.new))
    );
    assert_eq!(s.w.k, snapshot, "a rejected move report changes nothing");
}

/// A random move with random observers and an independent random `index_known_by`. For a move
/// that changes a library (out of, into, or within a library), half the time the "same
/// disclosure, different hidden index" twin is built ([`index_twin_move`]). Otherwise, for a
/// hidden-source move to another zone, a twin world moves a DIFFERENT object of the same source
/// zone with the same request, observers and `index_known_by` (L3b). Observation (design rule
/// of the brief, no CR) depends only on the observers list, the source zone and the destination
/// record, which are identical in both worlds by construction, so the model's observed set must
/// be the same in both worlds (the structural guard against a condition that depends on which
/// card moved). Every player outside that common observed set must then have equal whole views
/// and equal `position_known` for every library in both worlds, including a player holding a
/// position entry on the moved or on the alternative card (a known position does not establish
/// which position was selected).
fn do_move(
    s: &mut World,
    rng: &mut Lcg,
    players: &[PlayerId],
    all: &[ObjectId],
    stats: &mut Stats,
) {
    let hidden: Vec<ObjectId> = all
        .iter()
        .copied()
        .filter(|o| matches!(s.w.rec(*o).zone.kind(), ZoneKind::Library | ZoneKind::Hand))
        .collect();
    // bias toward hidden-source moves (they exercise step 0 and L3b)
    let from_hidden = !hidden.is_empty() && rng.chance(1, 2);
    let id = if from_hidden {
        rng.pick(&hidden)
    } else {
        rng.pick(all)
    };
    let mut req = random_request(rng, players);
    if from_hidden && rng.chance(1, 2) {
        // a destination that hides the card from most players
        req.to = rng.pick(&[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Exile]);
        req.face_down = req.to == ZoneKind::Exile;
        req.controller = None;
    }
    if rng.chance(1, 5) {
        // bias toward same-zone repositioning
        req.to = s.w.rec(id).zone.kind();
        req.face_down = req.to.is_public() && s.w.rec(id).face_down;
        if !matches!(req.to, ZoneKind::Battlefield | ZoneKind::Stack) {
            req.controller = None;
        }
    }
    let observers: Vec<PlayerId> = if rng.chance(1, 2) {
        players
            .iter()
            .copied()
            .filter(|_| rng.chance(1, 3))
            .collect()
    } else {
        Vec::new()
    };
    let ikb = random_set(rng, players);
    let plain = observers.is_empty() && ikb.is_empty() && rng.chance(1, 2);
    let inject = rng.chance(1, 8);
    let before = s.w.rec(id);
    let dest = ZoneId::of(req.to, before.owner);
    let class = match (before.zone, dest) {
        (ZoneId::Library(_), d) if d == before.zone => Some(IdxClass::Reposition),
        (ZoneId::Library(_), _) => Some(IdxClass::MoveOut),
        (_, ZoneId::Library(_)) => Some(IdxClass::MoveIn),
        _ => None,
    };
    if let Some(class) = class
        && rng.chance(1, 2)
    {
        let mv = IdxMove {
            id,
            req,
            observers: &observers,
            ikb: &ikb,
            plain,
            class,
        };
        if index_twin_move(s, rng, players, &mv, inject, stats) {
            return;
        }
    }
    let hidden_source = matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand);
    let mut twin = None;
    let mut pos_holders: BTreeSet<PlayerId> = BTreeSet::new();
    if hidden_source && dest != before.zone {
        let alts: Vec<ObjectId> =
            s.w.zones
                .contents(before.zone)
                .into_iter()
                .filter(|o| *o != id)
                .collect();
        if !alts.is_empty() {
            let alt = rng.pick(&alts);
            if let ZoneId::Library(owner) = before.zone {
                // make position knowledge common in the source library, on both candidates and
                // on random other cards, so a position-based leak is reachable (CR 401.2)
                if rng.chance(2, 3) {
                    seed_positions(s, rng, players, owner, &[id, alt]);
                }
                for p in players {
                    let held = s.m.positions(&s.w.zones, *p, owner);
                    if held.values().any(|o| *o == id || *o == alt) {
                        pos_holders.insert(*p);
                    }
                }
            }
            let mut t = s.clone();
            let (tm, tobs) = apply_move(&mut t, alt, req, &observers, &ikb, plain, None);
            twin = Some((t, tm, tobs));
        }
    }
    let (moved, observed) = apply_move(s, id, req, &observers, &ikb, plain, inject.then_some(rng));
    let Some((t, tmoved, tobserved)) = twin else {
        return;
    };
    check_model(&t, "hidden-move twin");
    assert_eq!(moved.new, tmoved.new, "twins issue the same new id");
    assert_eq!(moved.from, tmoved.from, "twins share the source zone");
    assert_eq!(moved.to, tmoved.to, "twins share the destination zone");
    let obs = observed.expect("step 0 applies to a hidden-source move");
    let tobs = tobserved.expect("step 0 applies to a hidden-source move");
    // structural guard: observation never depends on which card moved
    assert_eq!(
        obs, tobs,
        "L3b: observation must be world-independent ({before:?} -> {moved:?}, observers {observers:?})"
    );
    for p in players {
        if obs.contains(p) {
            continue;
        }
        assert_eq!(
            s.w.view(*p),
            t.w.view(*p),
            "L3b: {p:?} did not observe {before:?} -> {moved:?} (observers {observers:?}, ikb {ikb:?})"
        );
        for owner in players {
            assert_eq!(
                s.w.positions(*p, *owner),
                t.w.positions(*p, *owner),
                "L3b position_known: {p:?} on {owner:?}'s library after {before:?} -> {moved:?}"
            );
        }
        stats.twin_compared += 1;
        if pos_holders.contains(p) {
            stats.twin_pos_holder_compared += 1;
        }
    }
    // control: the owner of the destination hand sees which card arrived (CR 402.3)
    if let ZoneId::Hand(owner) = moved.to {
        assert!(obs.contains(&owner));
        assert_ne!(
            s.w.view(owner),
            t.w.view(owner),
            "L3b control: views differ"
        );
        stats.twin_control += 1;
    }
}

// ---------------------------------------------------------------------------------------------
// L3b "same disclosure, different hidden index" (the brief's index-disclosure rule, a design
// decision; no CR): twin worlds S and S' with the same players, the same card set per zone,
// the same observers and disclosure set and the same announced card, but a different hidden
// index of the library change. Every position entry that exists before the change sits at the
// same index in both worlds (S and S' are indistinguishable to every player before the change).
// Every player NOT in the disclosure set must have equal whole views and equal position_known
// for every library afterwards; for players in it nothing is asserted except the per-class
// control count (their position_known CAN differ).
// ---------------------------------------------------------------------------------------------

/// A library-changing move for an index twin.
struct IdxMove<'a> {
    id: ObjectId,
    req: MoveRequest,
    observers: &'a [PlayerId],
    ikb: &'a [PlayerId],
    plain: bool,
    class: IdxClass,
}

/// The index an insertion with `position` lands at in a sequence of `n` other cards.
fn landing_index(position: Position, n: usize) -> usize {
    match position {
        Position::Top => 0,
        Position::Bottom => n,
        Position::Index(k) => k.min(n),
    }
}

/// A landing index in `0..=n` different from `i` (`n` > 0).
fn other_index(rng: &mut Lcg, i: usize, n: usize) -> usize {
    let j = rng.idx(n);
    if j >= i { j + 1 } else { j }
}

/// The players who hold a live position entry for `owner`'s library (model).
fn pos_holders(s: &World, players: &[PlayerId], owner: PlayerId) -> BTreeSet<PlayerId> {
    players
        .iter()
        .copied()
        .filter(|p| !s.m.positions(&s.w.zones, *p, owner).is_empty())
        .collect()
}

/// Gives random players position entries on one to `max` random cards of `owner`'s library
/// other than `except` (CR 401.2), in the implementation and in the model.
fn seed_positions_except(
    s: &mut World,
    rng: &mut Lcg,
    players: &[PlayerId],
    owner: PlayerId,
    except: Option<ObjectId>,
    max: u64,
) {
    let lib = s.w.library(owner);
    let cands: Vec<usize> = (0..lib.len()).filter(|i| Some(lib[*i]) != except).collect();
    if cands.is_empty() {
        return;
    }
    for _ in 0..=rng.below(max) {
        let idx = rng.pick(&cands);
        let viewer = rng.pick(players);
        s.w.k
            .set_position_known(&s.w.zones, viewer, owner, idx, lib[idx])
            .expect("pos");
        s.m.set_position_known(&s.w.zones, viewer, owner, idx, lib[idx]);
    }
}

/// A random card of `owner`'s library that no player holds a live position entry on (model),
/// if any.
fn unpinned_card(
    s: &World,
    rng: &mut Lcg,
    players: &[PlayerId],
    owner: PlayerId,
) -> Option<ObjectId> {
    let lib = s.w.library(owner);
    let pinned: BTreeSet<ObjectId> = players
        .iter()
        .flat_map(|v| s.m.positions(&s.w.zones, *v, owner).into_values())
        .collect();
    let free: Vec<ObjectId> = lib.into_iter().filter(|o| !pinned.contains(o)).collect();
    (!free.is_empty()).then(|| rng.pick(&free))
}

/// The order of `owner`'s library with `x` at a different index: every index holding a live
/// position entry of any player (model) keeps its card, the other cards keep their relative
/// order and `x` takes another free slot. `None` when `x` itself is pinned or there is no other
/// free slot.
fn moved_elsewhere(
    s: &World,
    rng: &mut Lcg,
    players: &[PlayerId],
    owner: PlayerId,
    x: ObjectId,
) -> Option<Vec<ObjectId>> {
    let lib = s.w.library(owner);
    let pinned: BTreeSet<usize> = players
        .iter()
        .flat_map(|v| s.m.positions(&s.w.zones, *v, owner).into_keys())
        .collect();
    let i = lib.iter().position(|o| *o == x)?;
    if pinned.contains(&i) {
        return None;
    }
    let free: Vec<usize> = (0..lib.len()).filter(|k| !pinned.contains(k)).collect();
    if free.len() < 2 {
        return None;
    }
    let ki = free.iter().position(|f| *f == i).expect("x is free");
    let k = other_index(rng, ki, free.len() - 1);
    let mut seq: Vec<ObjectId> = free.iter().map(|f| lib[*f]).filter(|o| *o != x).collect();
    seq.insert(k, x);
    let mut target = lib.clone();
    for (slot, obj) in free.iter().zip(seq) {
        target[*slot] = obj;
    }
    Some(target)
}

/// A clone of `s` whose `owner` library is physically re-ordered to `target` (same-zone moves
/// the knowledge is not told about: the twin's state before the change). `None` when the clone
/// is not indistinguishable from `s` for every player (views and position_known).
fn rearranged(
    s: &World,
    players: &[PlayerId],
    owner: PlayerId,
    target: &[ObjectId],
) -> Option<World> {
    let mut t = s.clone();
    for (i, id) in target.iter().enumerate() {
        let m =
            t.w.raw_mv(*id, KWorld::req_at(ZoneKind::Library, Position::Index(i)));
        assert_eq!(m.old, m.new, "a same-zone library move keeps the id");
    }
    assert_eq!(t.w.library(owner), target);
    let same = players.iter().all(|p| {
        s.w.view(*p) == t.w.view(*p)
            && players
                .iter()
                .all(|o| s.w.positions(*p, *o) == t.w.positions(*p, *o))
    });
    if !same {
        return None;
    }
    check_model(&t, "index twin before the change");
    Some(t)
}

/// Compares index twins after the change (see the section comment) and counts.
fn compare_index_twins(
    s: &World,
    t: &World,
    class: IdxClass,
    disclosed: &[PlayerId],
    holders: &BTreeSet<PlayerId>,
    stats: &mut Stats,
) {
    let players = s.w.players();
    let ctx = format!("index twin {class:?} disclosed to {disclosed:?}");
    check_model(t, &ctx);
    let c = class as usize;
    stats.idx_twins[c] += 1;
    for p in &players {
        if disclosed.contains(p) {
            if players
                .iter()
                .any(|o| s.w.positions(*p, *o) != t.w.positions(*p, *o))
            {
                stats.idx_control[c] += 1;
            }
            continue;
        }
        assert_eq!(
            s.w.view(*p),
            t.w.view(*p),
            "L3b {ctx}: view of {p:?} depends on the hidden index"
        );
        for owner in &players {
            assert_eq!(
                s.w.positions(*p, *owner),
                t.w.positions(*p, *owner),
                "L3b {ctx}: position_known of {p:?} on {owner:?}'s library depends on the hidden index"
            );
        }
        stats.idx_compared[c] += 1;
        if holders.contains(p) {
            stats.idx_holder_compared[c] += 1;
        }
    }
}

/// The index twin of a move out of, into, or within a library. MoveOut: S' has the moved card
/// at another index of the source library ([`moved_elsewhere`]); MoveIn and Reposition: S' is
/// S, and the card lands at another index. For MoveOut the moved card is replaced by a card of
/// the same library whose position no player holds. Position entries are seeded on random
/// library cards (not on the moved card for MoveOut) in most worlds. Returns false (nothing done) when no twin
/// can be built.
fn index_twin_move(
    s: &mut World,
    rng: &mut Lcg,
    players: &[PlayerId],
    mv: &IdxMove,
    inject: bool,
    stats: &mut Stats,
) -> bool {
    let owner = s.w.rec(mv.id).owner;
    let mut id = mv.id;
    let (mut t, treq) = match mv.class {
        IdxClass::MoveOut => {
            // the announced card: one whose position no player holds (any card of that
            // library makes the same kind of move)
            let Some(x) = unpinned_card(s, rng, players, owner) else {
                return false;
            };
            id = x;
            if rng.chance(4, 5) {
                seed_positions_except(s, rng, players, owner, Some(id), 1);
            }
            let Some(target) = moved_elsewhere(s, rng, players, owner, id) else {
                return false;
            };
            let Some(t) = rearranged(s, players, owner, &target) else {
                return false;
            };
            (t, mv.req)
        }
        IdxClass::MoveIn | IdxClass::Reposition => {
            let len = s.w.library(owner).len();
            let n = if mv.class == IdxClass::Reposition {
                len - 1
            } else {
                len
            };
            if n == 0 {
                return false;
            }
            if rng.chance(4, 5) {
                seed_positions_except(s, rng, players, owner, None, 3);
            }
            let i = landing_index(mv.req.position, n);
            let j = other_index(rng, i, n);
            let treq = MoveRequest {
                position: Position::Index(j),
                ..mv.req
            };
            (s.clone(), treq)
        }
        _ => unreachable!("not a move class"),
    };
    let holders = pos_holders(s, players, owner);
    let (tm, tobs) = apply_move(&mut t, id, treq, mv.observers, mv.ikb, mv.plain, None);
    let (m, obs) = apply_move(
        s,
        id,
        mv.req,
        mv.observers,
        mv.ikb,
        mv.plain,
        inject.then_some(rng),
    );
    assert_eq!(m, tm, "index twins: same move record");
    assert_eq!(obs, tobs, "index twins: same observed set");
    compare_index_twins(s, &t, mv.class, mv.ikb, &holders, stats);
    true
}

/// Removes `id` with `Zones::send_outside`, then informs the implementation (the shorthand
/// `on_removed_from_hidden_zone` when `shorthand`, `on_removed_from_hidden_zone_indexed`
/// otherwise, with the record copied BEFORE the removal) and the model. With `inject`, rejected
/// hook calls are tried first. Returns the record and the model's observed set (`None` for a
/// public source).
fn apply_removal(
    s: &mut World,
    id: ObjectId,
    observers: &[PlayerId],
    ikb: &[PlayerId],
    shorthand: bool,
    inject: Option<&mut Lcg>,
) -> (ObjectRecord, Option<BTreeSet<PlayerId>>) {
    let before = s.w.rec(id);
    let pool = if id.get().is_multiple_of(2) {
        OutsidePool::Sideboard
    } else {
        OutsidePool::Lesson
    };
    let card = s.w.zones.send_outside(id, pool).expect("send_outside");
    assert_eq!(card, before.card);
    if let Some(rng) = inject {
        inject_removal_errors(s, &before, observers, ikb, rng);
    }
    if shorthand {
        assert!(ikb.is_empty());
        s.w.k
            .on_removed_from_hidden_zone(&s.w.zones, &before, observers)
            .expect("on_removed_from_hidden_zone");
    } else {
        s.w.k
            .on_removed_from_hidden_zone_indexed(&s.w.zones, &before, observers, ikb)
            .expect("on_removed_from_hidden_zone_indexed");
    }
    let observed =
        s.m.removed_from_hidden_zone_indexed(&s.w.zones, &before, observers, ikb);
    (before, observed)
}

/// R12: the source library or hand of an unknown player is `UnknownPlayer(owner)` (checked
/// first), then an unknown observer `UnknownPlayer` of the first unknown one, then an unknown
/// `index_known_by` player; nothing changes.
fn inject_removal_errors(
    s: &mut World,
    before: &ObjectRecord,
    observers: &[PlayerId],
    ikb: &[PlayerId],
    rng: &mut Lcg,
) {
    let snapshot = s.w.k.clone();
    let mut bad = observers.to_vec();
    bad.insert(rng.idx(bad.len() + 1), NOBODY);
    let ghost_ikb = PlayerId::new(3000 + rng.below(1000));
    let mut bad_ikb = ikb.to_vec();
    bad_ikb.insert(rng.idx(bad_ikb.len() + 1), ghost_ikb);
    let public = !matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand);
    if !public {
        assert_eq!(
            s.w.k.on_removed_from_hidden_zone(&s.w.zones, before, &bad),
            Err(KnowledgeError::UnknownPlayer(NOBODY))
        );
        assert_eq!(
            s.w.k
                .on_removed_from_hidden_zone_indexed(&s.w.zones, before, &bad, &bad_ikb),
            Err(KnowledgeError::UnknownPlayer(NOBODY)),
            "the observer error comes before the index_known_by error"
        );
        assert_eq!(
            s.w.k
                .on_removed_from_hidden_zone_indexed(&s.w.zones, before, observers, &bad_ikb),
            Err(KnowledgeError::UnknownPlayer(ghost_ikb))
        );
    }
    let ghost = PlayerId::new(1000 + rng.below(1000));
    let zone = if rng.chance(1, 2) {
        ZoneId::Library(ghost)
    } else {
        ZoneId::Hand(ghost)
    };
    let fake = ObjectRecord { zone, ..*before };
    assert_eq!(
        s.w.k.on_removed_from_hidden_zone(&s.w.zones, &fake, &bad),
        Err(KnowledgeError::UnknownPlayer(ghost)),
        "the owner error comes first"
    );
    assert_eq!(
        s.w.k
            .on_removed_from_hidden_zone_indexed(&s.w.zones, &fake, &bad, &bad_ikb),
        Err(KnowledgeError::UnknownPlayer(ghost)),
        "the owner error comes first"
    );
    assert_eq!(s.w.k, snapshot, "a rejected removal report changes nothing");
}

/// A random `send_outside` of a library or hand object (sometimes of any object) with random
/// observers and an independent random `index_known_by` (each: none, a random subset, every
/// player), followed by the hook. For a library source, half the time the "same disclosure,
/// different hidden index" twin is built (the announced card removed from another index).
/// Otherwise, for a hidden source, a twin world removes a DIFFERENT object of the same source
/// zone with the same observers and `index_known_by` (L3b for removals, PR #174 review finding
/// F2). Observation (design rule of the brief, no CR) depends only on the observers list and the
/// source zone, so the model's observed set must be the same in both worlds; every player
/// outside it must then have equal whole views and equal `position_known` for every library in
/// both worlds, including a player holding a position entry or set knowledge on either removed
/// candidate.
fn do_remove(
    s: &mut World,
    rng: &mut Lcg,
    players: &[PlayerId],
    all: &[ObjectId],
    stats: &mut Stats,
) {
    let hidden: Vec<ObjectId> = all
        .iter()
        .copied()
        .filter(|o| matches!(s.w.rec(*o).zone.kind(), ZoneKind::Library | ZoneKind::Hand))
        .collect();
    let id = if !hidden.is_empty() && rng.chance(4, 5) {
        rng.pick(&hidden)
    } else {
        rng.pick(all)
    };
    let observers = random_set(rng, players);
    let ikb = random_set(rng, players);
    let shorthand = ikb.is_empty() && rng.chance(1, 2);
    let inject = rng.chance(1, 6);
    let mut id = id;
    if let ZoneId::Library(owner) = s.w.rec(id).zone
        && rng.chance(2, 3)
        && let Some(x) = unpinned_card(s, rng, players, owner)
    {
        // the announced card of an index twin: no player holds its position
        id = x;
        if rng.chance(4, 5) {
            seed_positions_except(s, rng, players, owner, Some(id), 1);
        }
        if let Some(target) = moved_elsewhere(s, rng, players, owner, id)
            && let Some(mut t) = rearranged(s, players, owner, &target)
        {
            let holders = pos_holders(s, players, owner);
            let (_, tobs) = apply_removal(&mut t, id, &observers, &ikb, shorthand, None);
            let (_, obs) = apply_removal(s, id, &observers, &ikb, shorthand, inject.then_some(rng));
            assert_eq!(obs, tobs, "index twins: same observed set");
            stats.removals += 1;
            stats.hidden_removals += 1;
            compare_index_twins(s, &t, IdxClass::Removal, &ikb, &holders, stats);
            return;
        }
    }
    let before = s.w.rec(id);
    let mut twin = None;
    let mut holders: BTreeSet<PlayerId> = BTreeSet::new();
    let hidden_source = matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand);
    if hidden_source {
        let alts: Vec<ObjectId> =
            s.w.zones
                .contents(before.zone)
                .into_iter()
                .filter(|o| *o != id)
                .collect();
        if !alts.is_empty() {
            let alt = rng.pick(&alts);
            if let ZoneId::Library(owner) = before.zone
                && rng.chance(2, 3)
            {
                seed_positions(s, rng, players, owner, &[id, alt]);
            }
            if rng.chance(1, 2) {
                // set knowledge on both candidates for a random player
                let p = rng.pick(players);
                for o in [id, alt] {
                    s.w.k.add_known(&s.w.zones, o, p).expect("add_known");
                    s.m.add_known(&s.w.zones, o, p);
                }
            }
            for p in players {
                let pos_holder = match before.zone {
                    ZoneId::Library(owner) => {
                        s.m.positions(&s.w.zones, *p, owner)
                            .values()
                            .any(|o| *o == id || *o == alt)
                    }
                    _ => false,
                };
                let set_holder = s.m.stored(&s.w.zones, id).contains(p)
                    || s.m.stored(&s.w.zones, alt).contains(p);
                if pos_holder || set_holder {
                    holders.insert(*p);
                }
            }
            let mut t = s.clone();
            let (_, tobs) = apply_removal(&mut t, alt, &observers, &ikb, shorthand, None);
            twin = Some((t, tobs));
        }
    }
    let (_, observed) = apply_removal(s, id, &observers, &ikb, shorthand, inject.then_some(rng));
    stats.removals += 1;
    if hidden_source {
        stats.hidden_removals += 1;
        assert!(observed.is_some(), "the hook applies to a hidden source");
    } else {
        assert!(observed.is_none(), "a public source is a no-op");
    }
    let Some((t, tobserved)) = twin else {
        return;
    };
    check_model(&t, "removal twin");
    let obs = observed.expect("hidden source");
    let tobs = tobserved.expect("hidden source");
    // structural guard: observation never depends on which card was removed
    assert_eq!(
        obs, tobs,
        "L3b removal: observation must be world-independent ({before:?}, observers {observers:?})"
    );
    for p in players {
        if obs.contains(p) {
            continue;
        }
        assert_eq!(
            s.w.view(*p),
            t.w.view(*p),
            "L3b removal: {p:?} did not observe the removal from {before:?} (observers {observers:?}, ikb {ikb:?})"
        );
        for owner in players {
            assert_eq!(
                s.w.positions(*p, *owner),
                t.w.positions(*p, *owner),
                "L3b removal position_known: {p:?} on {owner:?}'s library after removal from {before:?}"
            );
        }
        stats.removal_compared += 1;
        if holders.contains(p) {
            stats.removal_holder_compared += 1;
        }
    }
    // control: the owner of the source hand sees which card left (CR 402.3)
    if let ZoneId::Hand(owner) = before.zone {
        assert!(obs.contains(&owner));
        assert_ne!(
            s.w.view(owner),
            t.w.view(owner),
            "L3b removal control: views differ"
        );
        stats.removal_control += 1;
    }
}

/// Inserts `card` into the zone `req` names (`Zones::enter_from_outside` when `via_enter`,
/// `Zones::create_object` otherwise); for a library the implementation is told with the
/// shorthand `on_library_changed` when `shorthand`, `on_library_changed_indexed(ikb)` otherwise,
/// and the model with `library_changed_indexed(ikb)` (CR 401.2: position-only).
fn insert(
    s: &mut World,
    owner: PlayerId,
    card: CardId,
    req: MoveRequest,
    ikb: &[PlayerId],
    shorthand: bool,
    via_enter: bool,
) -> ObjectId {
    if shorthand {
        assert!(ikb.is_empty());
    }
    let disclosure = (!shorthand).then_some(ikb);
    let id = if via_enter {
        s.w.enter_card(owner, card, req, disclosure)
    } else {
        s.w.insert_card(owner, card, req, disclosure)
    };
    if req.to == ZoneKind::Library {
        s.m.library_changed_indexed(&s.w.zones, owner, ikb);
    }
    id
}

/// A card from outside the game enters a library (random position, `create_object` or
/// `enter_from_outside`, then `on_library_changed[_indexed]` with a random `index_known_by`:
/// position-only, CR 401.2) or a hand (no knowledge call), in the implementation and in the
/// model. Only cards never seen in the game enter (the brief does not say what a returning
/// card's knowledge is). Stored KnownTo is never touched by an insertion. For most library
/// insertions the "same disclosure, different hidden index" twin inserts the same card at
/// another index.
fn do_insert(s: &mut World, rng: &mut Lcg, players: &[PlayerId], stats: &mut Stats) {
    let owner = rng.pick(players);
    let to_library = rng.chance(2, 3);
    let req = if to_library {
        KWorld::req_at(ZoneKind::Library, random_position(rng))
    } else {
        MoveRequest::new(ZoneKind::Hand)
    };
    let ikb = random_set(rng, players);
    let shorthand = ikb.is_empty() && rng.chance(1, 2);
    let via_enter = rng.chance(1, 2);
    let card = s.w.fresh_card();
    let mut twin = None;
    if to_library {
        if rng.chance(4, 5) {
            seed_positions_except(s, rng, players, owner, None, 3);
        }
        let n = s.w.library(owner).len();
        if n > 0 && rng.chance(3, 4) {
            let j = other_index(rng, landing_index(req.position, n), n);
            let holders = pos_holders(s, players, owner);
            let mut t = s.clone();
            let treq = KWorld::req_at(ZoneKind::Library, Position::Index(j));
            insert(&mut t, owner, card, treq, &ikb, shorthand, via_enter);
            twin = Some((t, holders));
        }
    }
    let known_before: Vec<(ObjectId, BTreeSet<PlayerId>)> =
        s.w.zones
            .all_object_ids()
            .into_iter()
            .map(|id| (id, s.w.known_to(id)))
            .collect();
    let new = insert(s, owner, card, req, &ikb, shorthand, via_enter);
    if to_library {
        stats.library_insertions += 1;
    }
    stats.insertions += 1;
    assert!(
        s.w.known_to(new).is_empty(),
        "a fresh card is known to nobody"
    );
    for (id, k) in known_before {
        assert_eq!(s.w.known_to(id), k, "an insertion never changes KnownTo");
    }
    if let Some((t, holders)) = twin {
        compare_index_twins(s, &t, IdxClass::Insertion, &ikb, &holders, stats);
    }
}

/// Gives random players position entries in `owner`'s library on every card of `must` and on
/// some random other cards (CR 401.2), in the implementation and in the model.
fn seed_positions(
    s: &mut World,
    rng: &mut Lcg,
    players: &[PlayerId],
    owner: PlayerId,
    must: &[ObjectId],
) {
    let lib = s.w.library(owner);
    let mut targets: Vec<usize> = must
        .iter()
        .map(|o| {
            lib.iter()
                .position(|x| x == o)
                .expect("candidate in library")
        })
        .collect();
    for _ in 0..rng.below(3) {
        targets.push(rng.idx(lib.len()));
    }
    for idx in targets {
        let viewer = rng.pick(players);
        s.w.k
            .set_position_known(&s.w.zones, viewer, owner, idx, lib[idx])
            .expect("pos");
        s.m.set_position_known(&s.w.zones, viewer, owner, idx, lib[idx]);
    }
}

fn do_set_position(s: &mut World, rng: &mut Lcg, players: &[PlayerId]) {
    let owner = rng.pick(players);
    let lib = s.w.library(owner);
    if lib.is_empty() {
        return;
    }
    let viewer = rng.pick(players);
    let idx = rng.idx(lib.len());
    s.w.k
        .set_position_known(&s.w.zones, viewer, owner, idx, lib[idx])
        .expect("pos");
    s.m.set_position_known(&s.w.zones, viewer, owner, idx, lib[idx]);
}

/// The shorthand `shuffle_library` when `shorthand` (`known_by` empty), otherwise
/// `shuffle_library_disclosed`, in the implementation; the model always gets the disclosed
/// operation.
fn shuffle(
    s: &mut World,
    owner: PlayerId,
    excluded: &[ObjectId],
    known_by: &[PlayerId],
    shorthand: bool,
) {
    if shorthand {
        assert!(known_by.is_empty());
        s.w.k
            .shuffle_library(&s.w.zones, owner, excluded)
            .expect("shuffle");
    } else {
        s.w.shuffle_disclosed(owner, excluded, known_by);
    }
    s.m.shuffle_library_disclosed(&s.w.zones, owner, excluded, known_by);
}

/// Physically reorders a library (same-zone moves told to the knowledge with a random
/// `index_known_by`), then shuffles the knowledge with a random excluded subset and a random
/// `known_by` (CR 701.24a, 701.24b). When some card is excluded and some is not, the "same
/// disclosure, different hidden index" twin excludes another card in place of one excluded
/// card (A vs X); a random player often holds an entry on A.
fn do_shuffle_library(s: &mut World, rng: &mut Lcg, players: &[PlayerId], stats: &mut Stats) {
    let owner = rng.pick(players);
    let lib = s.w.library(owner);
    for _ in 0..rng.below(3) {
        if lib.is_empty() {
            break;
        }
        let id = rng.pick(&lib);
        let req = KWorld::req_at(ZoneKind::Library, random_position(rng));
        let ikb = random_set(rng, players);
        let plain = ikb.is_empty();
        apply_move(s, id, req, &[], &ikb, plain, None);
    }
    let lib = s.w.library(owner);
    if rng.chance(4, 5) {
        seed_positions_except(s, rng, players, owner, None, 3);
    }
    let excluded: Vec<ObjectId> = lib.iter().copied().filter(|_| rng.chance(1, 3)).collect();
    let known_by = random_set(rng, players);
    let shorthand = known_by.is_empty() && rng.chance(1, 2);
    let rest: Vec<ObjectId> = lib
        .iter()
        .copied()
        .filter(|o| !excluded.contains(o))
        .collect();
    let mut twin = None;
    if !excluded.is_empty() && !rest.is_empty() {
        let k = rng.idx(excluded.len());
        if rng.chance(1, 2) {
            // a random player holds an entry on the excluded card A (CR 401.2)
            let a = excluded[k];
            let idx = lib.iter().position(|o| *o == a).expect("in library");
            let viewer = rng.pick(players);
            s.w.k
                .set_position_known(&s.w.zones, viewer, owner, idx, a)
                .expect("pos");
            s.m.set_position_known(&s.w.zones, viewer, owner, idx, a);
        }
        let mut alt = excluded.clone();
        alt[k] = rng.pick(&rest);
        let holders = pos_holders(s, players, owner);
        let mut t = s.clone();
        shuffle(&mut t, owner, &alt, &known_by, shorthand);
        twin = Some((t, holders));
    }
    let known_before: Vec<BTreeSet<PlayerId>> = lib.iter().map(|id| s.w.known_to(*id)).collect();
    shuffle(s, owner, &excluded, &known_by, shorthand);
    let known_after: Vec<BTreeSet<PlayerId>> = lib.iter().map(|id| s.w.known_to(*id)).collect();
    assert_eq!(known_before, known_after, "a shuffle never touches KnownTo");
    if let Some((t, holders)) = twin {
        compare_index_twins(s, &t, IdxClass::Shuffle, &known_by, &holders, stats);
    }
}

/// Invalid calls: each is an error and leaves the knowledge unchanged (R12).
fn invalid_call(s: &mut World, rng: &mut Lcg, players: &[PlayerId], all: &[ObjectId]) {
    let snapshot = s.w.k.clone();
    let id = rng.pick(all);
    let r = s.w.rec(id);
    let p = rng.pick(players);
    let w = &mut s.w;
    match rng.below(9) {
        0 => {
            let res = w.k.grant_look(&w.zones, id, p);
            if r.face_down && r.zone == ZoneId::Exile {
                assert_eq!(res, Ok(()));
                s.m.grant_look(&s.w.zones, id, p);
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
            // NotInLibrary comes before an unknown known_by player
            assert_eq!(
                w.k.shuffle_library_disclosed(&w.zones, p, &[id], &[NOBODY]),
                Err(KnowledgeError::NotInLibrary {
                    owner: p,
                    object: id
                })
            );
        }
        5 => {
            // CR 406.3, 701.24a: face up, or face down outside exile, is NotFaceDown
            if r.face_down && r.zone == ZoneId::Exile {
                return;
            }
            let mut pile = face_down_in(w, &[ZoneKind::Exile]);
            pile.insert(rng.idx(pile.len() + 1), id);
            assert_eq!(
                w.k.shuffle_face_down_pile(&w.zones, &pile),
                Err(KnowledgeError::NotFaceDown(id))
            );
        }
        6 => {
            let mut ikb = random_set(rng, players);
            ikb.insert(rng.idx(ikb.len() + 1), NOBODY);
            assert_eq!(
                w.k.on_library_changed_indexed(&w.zones, p, &ikb),
                Err(KnowledgeError::UnknownPlayer(NOBODY))
            );
            assert_eq!(
                w.k.on_library_changed_indexed(&w.zones, pid(5000), &ikb),
                Err(KnowledgeError::UnknownPlayer(pid(5000))),
                "the owner error comes first"
            );
        }
        7 => {
            let lib = w.library(p);
            let excluded: Vec<ObjectId> =
                lib.iter().copied().filter(|_| rng.chance(1, 2)).collect();
            let mut known_by = random_set(rng, players);
            known_by.insert(rng.idx(known_by.len() + 1), NOBODY);
            assert_eq!(
                w.k.shuffle_library_disclosed(&w.zones, p, &excluded, &known_by),
                Err(KnowledgeError::UnknownPlayer(NOBODY))
            );
            assert_eq!(
                w.k.shuffle_library_disclosed(&w.zones, pid(5000), &[id], &known_by),
                Err(KnowledgeError::UnknownPlayer(pid(5000))),
                "the owner error comes first"
            );
        }
        _ => assert_eq!(
            w.k.on_library_changed(&w.zones, NOBODY),
            Err(KnowledgeError::UnknownPlayer(NOBODY))
        ),
    }
    assert_eq!(s.w.k, snapshot, "a rejected call changes nothing");
}

// ---------------------------------------------------------------------------------------------
// the checks L1-L5
// ---------------------------------------------------------------------------------------------

/// L1: the implementation agrees with the shadow model for every player and object (whole
/// views, stored KnownTo, can_identify, live positions).
fn check_model(s: &World, ctx: &str) {
    let w = &s.w;
    let players = w.players();
    for id in w.zones.all_object_ids() {
        assert_eq!(
            w.known_to(id),
            s.m.stored(&w.zones, id),
            "L1 known_to {ctx} {:?}",
            w.rec(id)
        );
        for p in &players {
            assert_eq!(
                w.can(*p, id),
                s.m.can(&w.zones, *p, id),
                "L1 can_identify {ctx} {p:?} {:?}",
                w.rec(id)
            );
        }
    }
    for p in &players {
        assert_eq!(
            w.view(*p),
            s.m.project(&w.zones, *p),
            "L1 project {ctx} {p:?}"
        );
        for owner in &players {
            assert_eq!(
                w.positions(*p, *owner),
                s.m.positions(&w.zones, *p, *owner),
                "L1 position_known {ctx} viewer {p:?} owner {owner:?}"
            );
        }
    }
}

/// The CardIds `viewer` is entitled to (model, rules 1-4).
fn entitled_cards(s: &World, viewer: PlayerId) -> BTreeSet<CardId> {
    s.w.zones
        .all_object_ids()
        .into_iter()
        .filter(|id| s.m.can(&s.w.zones, viewer, *id))
        .map(|id| s.w.card(id))
        .collect()
}

/// L2: structural walk of the view for one viewer.
fn check_l2(s: &World, viewer: PlayerId, ctx: &str) -> ZonesView {
    let w = &s.w;
    let v = w.view(viewer);
    for o in object_views(&v) {
        let r = w.zones.object(o.id).expect("L2: view ids are live objects");
        let allowed = r.zone.kind().is_public() || r.zone == ZoneId::Hand(viewer);
        assert!(allowed, "L2 {ctx}: leaked ObjectId of {r:?}");
        if let Some(c) = o.card {
            assert_eq!(c, r.card, "L2 {ctx}: card belongs to the object");
        }
    }
    let entitled = entitled_cards(s, viewer);
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

/// Builds a world that differs from `s` only in what `viewer` cannot know: CardIds of objects
/// the viewer cannot identify (by the model) are relabelled, every library is re-ordered, and
/// other players' KnownTo and position knowledge are replaced by random ones. `relabel_visible`
/// additionally relabels one object the viewer CAN identify (for the mutation self-check).
fn twin(s: &World, viewer: PlayerId, rng: &mut Lcg, relabel_visible: Option<ObjectId>) -> World {
    let w = &s.w;
    let players = w.players();
    let mut t = World::new(&players);
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
        let visible = s.m.can(&w.zones, viewer, id);
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
        t.w.ids = IdGenerator::from_last_issued(id.get() - 1);
        let new =
            t.w.zones
                .create_object(&mut t.w.ids, card, r.owner, req)
                .expect("twin create");
        assert_eq!(new, id, "twin keeps object ids");
        if s.m.stored(&w.zones, id).contains(&viewer) {
            t.w.k.add_known(&t.w.zones, id, viewer).expect("twin add");
            t.m.add_known(&t.w.zones, id, viewer);
        }
    }
    for zone in order.keys() {
        if !matches!(zone, ZoneId::Library(_)) {
            assert_eq!(
                t.w.zones.contents(*zone),
                w.zones.contents(*zone),
                "twin order"
            );
        }
    }
    // random knowledge for the other players
    let others: Vec<PlayerId> = players.iter().copied().filter(|p| *p != viewer).collect();
    if !others.is_empty() {
        for id in t.w.zones.all_object_ids() {
            if rng.chance(1, 3) {
                let p = rng.pick(&others);
                t.w.k.add_known(&t.w.zones, id, p).expect("twin other add");
                t.m.add_known(&t.w.zones, id, p);
            }
        }
        for _ in 0..rng.below(4) {
            let owner = rng.pick(&players);
            let lib = t.w.library(owner);
            if lib.is_empty() {
                continue;
            }
            let v = rng.pick(&others);
            let i = rng.idx(lib.len());
            t.w.k
                .set_position_known(&t.w.zones, v, owner, i, lib[i])
                .expect("twin pos");
            t.m.set_position_known(&t.w.zones, v, owner, i, lib[i]);
        }
    }
    t
}

/// L3a non-interference (relabelled hidden cards), plus the deliberate-mutation self-checks.
fn check_l3(s: &World, viewer: PlayerId, v: &ZonesView, rng: &mut Lcg, ctx: &str) {
    let t = twin(s, viewer, rng, None);
    assert_eq!(
        &t.w.view(viewer),
        v,
        "L3 {ctx}: hidden information changed V"
    );
    assert_eq!(&t.m.project(&t.w.zones, viewer), v, "L3 {ctx}: model twin");
    // self-check 1: revealing a hidden card to the viewer changes V
    let hidden: Vec<ObjectId> =
        t.w.zones
            .all_object_ids()
            .into_iter()
            .filter(|id| !t.m.can(&t.w.zones, viewer, *id))
            .collect();
    if !hidden.is_empty() {
        let mut m = t.w.clone();
        let id = rng.pick(&hidden);
        m.k.add_known(&m.zones, id, viewer).expect("add");
        assert_ne!(
            &m.view(viewer),
            v,
            "L3 self-check {ctx}: reveal must change V"
        );
    }
    // self-check 2: a new card in the viewer's hand changes V
    let mut m = t.w.clone();
    m.create(ZoneKind::Hand, viewer);
    assert_ne!(
        &m.view(viewer),
        v,
        "L3 self-check {ctx}: hand change must change V"
    );
    // self-check 3: relabelling a card the viewer can identify changes V
    let visible: Vec<ObjectId> =
        s.w.zones
            .all_object_ids()
            .into_iter()
            .filter(|id| s.m.can(&s.w.zones, viewer, *id))
            .collect();
    if !visible.is_empty() {
        let id = rng.pick(&visible);
        let m = twin(s, viewer, rng, Some(id));
        assert_ne!(
            &m.w.view(viewer),
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

fn run(seed: u64, players: &[PlayerId], stats: &mut Stats) -> (Vec<ZonesView>, Vec<u8>) {
    let mut rng = Lcg::new(seed);
    let mut twin_rng = Lcg::new(seed.wrapping_add(0x5151));
    let mut s = World::new(players);
    for i in 0..STEPS {
        step(&mut s, &mut rng, stats);
        let ctx = format!("seed {seed} players {} step {i}", players.len());
        check_model(&s, &ctx);
        check_positions_live(&s.w, &ctx);
        for p in players {
            let v = check_l2(&s, *p, &ctx);
            check_l3(&s, *p, &v, &mut twin_rng, &ctx);
        }
    }
    let views = players.iter().map(|p| s.w.view(*p)).collect();
    (views, bytes(&s.w.k))
}

fn run_all(players: &[PlayerId]) {
    let mut stats = Stats::default();
    for seed in SEEDS {
        run(seed, players, &mut stats);
    }
    assert!(stats.twin_compared > 0, "L3b compared nothing: {stats:?}");
    assert!(stats.twin_control > 0, "L3b control never ran: {stats:?}");
    assert!(
        stats.twin_pos_holder_compared > 0,
        "L3b never compared a position-holding non-observer: {stats:?}"
    );
    assert!(stats.removals > 0, "no send_outside: {stats:?}");
    assert!(
        stats.hidden_removals > 0,
        "no hidden-zone send_outside: {stats:?}"
    );
    assert!(
        stats.removal_compared > 0,
        "L3b compared no removal non-observer: {stats:?}"
    );
    assert!(
        stats.removal_holder_compared > 0,
        "L3b never compared a knowledge-holding removal non-observer: {stats:?}"
    );
    assert!(
        stats.removal_control > 0,
        "L3b removal control never ran: {stats:?}"
    );
    assert!(stats.insertions > 0, "no enter_from_outside: {stats:?}");
    assert!(
        stats.library_insertions > 0,
        "no enter_from_outside into a library: {stats:?}"
    );
    for class in IdxClass::ALL {
        let c = class as usize;
        assert!(
            stats.idx_compared[c] > 0,
            "L3b index twins compared no non-disclosed player for {class:?}: {stats:?}"
        );
        assert!(
            stats.idx_holder_compared[c] > 0,
            "L3b index twins never compared a position holder for {class:?}: {stats:?}"
        );
        assert!(
            stats.idx_control[c] > 0,
            "L3b index-twin control never differed for {class:?}: {stats:?}"
        );
    }
    eprintln!("L3b stats {} players: {stats:?}", players.len());
}

/// REQ-CORE-004 (acceptance criterion 1), CR 400.2, 401.2, 402.3, 406.3, 708.5, 708.9: over
/// generated two-player states, V(S, P) contains exactly what P is entitled to by the
/// independent shadow model (L1, L2); relabelled hidden cards never change it (L3a); a
/// different hidden card selected for the same unobserved hidden move never changes a
/// non-observer's view (L3b, the hidden-source identity rule of the brief); positions are
/// always live (L5).
#[test]
fn leak_property_two_players() {
    run_all(&[P1, P2]);
}

/// REQ-CORE-004 (acceptance criterion 1), CR 400.2, 401.2, 402.3, 406.3, 708.5, 708.9: the
/// same property over generated three-player states.
#[test]
fn leak_property_three_players() {
    run_all(&[P1, P2, P3]);
}

/// REQ-CORE-004, REQ-CORE-001 (L5): the same seed gives the same final views and the same
/// canonical bytes of the Knowledge.
#[test]
fn leak_property_deterministic() {
    let mut stats = Stats::default();
    for seed in [SEEDS[0], SEEDS[6]] {
        for players in [vec![P1, P2], vec![P1, P2, P3]] {
            assert_eq!(
                run(seed, &players, &mut stats),
                run(seed, &players, &mut stats),
                "seed {seed}"
            );
        }
    }
}

/// REQ-CORE-004 (hidden-source identity rule), CR 402.3: a deterministic L3b instance and its
/// control. P2 knows the whole set {A, B} of P1's library; the twins draw A and B respectively
/// (unobserved): P2's views are equal, P1's differ. With P2 as an observer, P2's views differ.
#[test]
fn hidden_move_twin_fixed_instance() {
    let build = || -> World {
        let mut s = World::new(&[P1, P2]);
        for c in [10, 11] {
            let id =
                s.w.create_card(cid(c), P1, MoveRequest::new(ZoneKind::Library));
            s.w.k.add_known(&s.w.zones, id, P2).expect("add");
            s.m.add_known(&s.w.zones, id, P2);
        }
        s
    };
    for (observers, plain) in [(vec![], true), (vec![], false), (vec![P2], false)] {
        let mut a = build();
        let mut b = build();
        let lib = a.w.library(P1);
        apply_move(
            &mut a,
            lib[0],
            MoveRequest::new(ZoneKind::Hand),
            &observers,
            &[],
            plain,
            None,
        );
        apply_move(
            &mut b,
            lib[1],
            MoveRequest::new(ZoneKind::Hand),
            &observers,
            &[],
            plain,
            None,
        );
        check_model(&a, "fixed a");
        check_model(&b, "fixed b");
        assert_ne!(
            a.w.view(P1),
            b.w.view(P1),
            "control: the drawer sees the card"
        );
        if observers.is_empty() {
            assert_eq!(a.w.view(P2), b.w.view(P2), "no leak to P2");
            assert!(lib_known(&a.w.view(P2), P1).is_empty());
        } else {
            assert_ne!(a.w.view(P2), b.w.view(P2), "the observer can tell");
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
