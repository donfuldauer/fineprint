// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::knowledge` spec tests (REQ-CORE-004, REQ-ZONE-001).
//! Black-box: written from the work-item brief only.
//!
//! Each `spec_knowledge_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card here is a synthetic id (R5).

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::knowledge::{
    HandView, Knowledge, LibraryView, ObjectView, PlayerZonesView, ZonesView,
};
use fineprint_core::zones::{
    MoveRequest, Moved, ObjectRecord, OutsidePool, Position, ZoneId, ZoneKind, Zones, ZonesConfig,
};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
/// A player who is not in any test game.
pub const NOBODY: PlayerId = PlayerId::new(99);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn cid(n: u64) -> CardId {
    CardId::new(n)
}

pub fn set<T: Ord + Copy>(items: &[T]) -> BTreeSet<T> {
    items.iter().copied().collect()
}

// ---------------------------------------------------------------------------------------------
// world
// ---------------------------------------------------------------------------------------------

/// A test world: zones, the one id generator of those zones, and the knowledge under test.
#[derive(Clone, Debug)]
pub struct KWorld {
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub k: Knowledge,
    next_card: u64,
}

impl KWorld {
    pub fn two() -> KWorld {
        KWorld::with_players(&[P1, P2])
    }

    pub fn three() -> KWorld {
        KWorld::with_players(&[P1, P2, P3])
    }

    pub fn with_players(players: &[PlayerId]) -> KWorld {
        KWorld {
            zones: Zones::new(players, ZonesConfig::default()),
            ids: IdGenerator::new(),
            k: Knowledge::new(),
            next_card: 1,
        }
    }

    pub fn players(&self) -> Vec<PlayerId> {
        self.zones.players()
    }

    /// A CardId not present in any zone of this world.
    pub fn fresh_card(&mut self) -> CardId {
        loop {
            let c = cid(self.next_card);
            self.next_card += 1;
            if self.zones.object_of_card(c).is_none() {
                return c;
            }
        }
    }

    /// Creates a face-up object owned by `owner` in a zone of kind `kind` (on top).
    pub fn create(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        self.create_req(owner, MoveRequest::new(kind))
    }

    /// Creates a face-down object owned by `owner` in a zone of kind `kind`.
    pub fn create_fd(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let mut req = MoveRequest::new(kind);
        req.face_down = true;
        self.create_req(owner, req)
    }

    /// Creates a face-down object on the battlefield or stack controlled by `controller`.
    pub fn create_fd_ctl(&mut self, kind: ZoneKind, owner: PlayerId, ctl: PlayerId) -> ObjectId {
        let req = MoveRequest {
            to: kind,
            position: Position::Top,
            face_down: true,
            controller: Some(ctl),
        };
        self.create_req(owner, req)
    }

    /// Creates a face-up object at the bottom of an ordered zone.
    pub fn create_bottom(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let mut req = MoveRequest::new(kind);
        req.position = Position::Bottom;
        self.create_req(owner, req)
    }

    pub fn create_req(&mut self, owner: PlayerId, req: MoveRequest) -> ObjectId {
        let card = self.fresh_card();
        self.zones
            .create_object(&mut self.ids, card, owner, req)
            .expect("create succeeds")
    }

    /// Creates an object for the given synthetic `card` (for twin worlds with equal cards).
    pub fn create_card(&mut self, card: CardId, owner: PlayerId, req: MoveRequest) -> ObjectId {
        self.zones
            .create_object(&mut self.ids, card, owner, req)
            .expect("create succeeds")
    }

    /// Builds `owner`'s (empty) library from the synthetic cards `top_first` (top first). The
    /// cards are created in ascending card order, each at the index that gives the final order,
    /// so twin worlds with the same card set (and the same earlier objects) give every card the
    /// same ObjectId whatever the order. Returns the library ids, top first.
    pub fn build_library(&mut self, owner: PlayerId, top_first: &[u64]) -> Vec<ObjectId> {
        assert!(
            self.library(owner).is_empty(),
            "build_library: empty library"
        );
        let rank = |c: u64| top_first.iter().position(|x| *x == c).expect("in order");
        let mut sorted = top_first.to_vec();
        sorted.sort_unstable();
        let mut placed: Vec<u64> = Vec::new();
        for c in sorted {
            let idx = placed.iter().filter(|p| rank(**p) < rank(c)).count();
            self.create_card(
                cid(c),
                owner,
                KWorld::req_at(ZoneKind::Library, Position::Index(idx)),
            );
            placed.push(c);
        }
        let lib = self.library(owner);
        let cards: Vec<u64> = lib.iter().map(|id| self.card(*id).get()).collect();
        assert_eq!(cards, top_first, "build_library order");
        lib
    }

    /// The object of the synthetic card number `card`.
    pub fn id_of(&self, card: u64) -> ObjectId {
        self.zones
            .object_of_card(cid(card))
            .expect("card in a zone")
    }

    pub fn card(&self, id: ObjectId) -> CardId {
        self.zones.object(id).expect("object present").card
    }

    pub fn rec(&self, id: ObjectId) -> ObjectRecord {
        *self.zones.object(id).expect("object present")
    }

    /// Moves with `Zones::move_object` then informs the knowledge with `on_move`, using the
    /// record copied before the move (the documented protocol).
    pub fn mv(&mut self, id: ObjectId, req: MoveRequest) -> Moved {
        let before = self.rec(id);
        let moved = self
            .zones
            .move_object(&mut self.ids, id, req)
            .expect("move succeeds");
        self.k
            .on_move(&self.zones, &before, &moved)
            .expect("on_move succeeds");
        moved
    }

    /// Moves with `Zones::move_object` then informs the knowledge with `on_move_observed`:
    /// `observers` watched the move and know which card moved.
    pub fn mv_obs(&mut self, id: ObjectId, req: MoveRequest, observers: &[PlayerId]) -> Moved {
        let before = self.rec(id);
        let moved = self
            .zones
            .move_object(&mut self.ids, id, req)
            .expect("move succeeds");
        self.k
            .on_move_observed(&self.zones, &before, &moved, observers)
            .expect("on_move_observed succeeds");
        moved
    }

    /// Moves with `Zones::move_object` then informs the knowledge with `on_move_indexed`:
    /// `observers` know which card moved, `index_known_by` know the library index(es) of the
    /// change (the brief's index-disclosure rule, a design decision).
    pub fn mv_idx(
        &mut self,
        id: ObjectId,
        req: MoveRequest,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> Moved {
        let before = self.rec(id);
        let moved = self
            .zones
            .move_object(&mut self.ids, id, req)
            .expect("move succeeds");
        self.k
            .on_move_indexed(&self.zones, &before, &moved, observers, index_known_by)
            .expect("on_move_indexed succeeds");
        moved
    }

    /// Removes `id` from its zone to its owner's sideboard with `Zones::send_outside`, then
    /// informs the knowledge with `on_removed_from_hidden_zone`, using the record copied BEFORE
    /// the removal (the documented caller contract). `observers` watched which card was removed.
    /// Returns that record and the removed card.
    pub fn send_out(&mut self, id: ObjectId, observers: &[PlayerId]) -> (ObjectRecord, CardId) {
        let before = self.rec(id);
        let card = self
            .zones
            .send_outside(id, OutsidePool::Sideboard)
            .expect("send_outside succeeds");
        self.k
            .on_removed_from_hidden_zone(&self.zones, &before, observers)
            .expect("on_removed_from_hidden_zone succeeds");
        (before, card)
    }

    /// As [`KWorld::send_out`], through `on_removed_from_hidden_zone_indexed`: `index_known_by`
    /// know the index the card was removed from.
    pub fn send_out_idx(
        &mut self,
        id: ObjectId,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> (ObjectRecord, CardId) {
        let before = self.rec(id);
        let card = self
            .zones
            .send_outside(id, OutsidePool::Sideboard)
            .expect("send_outside succeeds");
        self.k
            .on_removed_from_hidden_zone_indexed(&self.zones, &before, observers, index_known_by)
            .expect("on_removed_from_hidden_zone_indexed succeeds");
        (before, card)
    }

    /// Brings a fresh synthetic card from `owner`'s sideboard into the zone `req` names with
    /// `Zones::enter_from_outside`; for a library it then calls `on_library_changed` (an
    /// insertion: position-only). Returns the new object.
    pub fn enter_fresh(&mut self, owner: PlayerId, req: MoveRequest) -> ObjectId {
        let card = self.fresh_card();
        self.enter_card(owner, card, req, None)
    }

    /// Brings `card` from `owner`'s sideboard into the zone `req` names with
    /// `Zones::enter_from_outside`. For a library it then calls `on_library_changed_indexed`
    /// with `index_known_by` when that is `Some`, the shorthand `on_library_changed` when it is
    /// `None`. Returns the new object.
    pub fn enter_card(
        &mut self,
        owner: PlayerId,
        card: CardId,
        req: MoveRequest,
        index_known_by: Option<&[PlayerId]>,
    ) -> ObjectId {
        self.zones
            .add_outside_card(owner, OutsidePool::Sideboard, card)
            .expect("add_outside_card succeeds");
        let id = self
            .zones
            .enter_from_outside(&mut self.ids, owner, card, OutsidePool::Sideboard, req)
            .expect("enter_from_outside succeeds");
        if req.to == ZoneKind::Library {
            self.library_changed(owner, index_known_by);
        }
        id
    }

    /// Creates `card` in the zone `req` names with `Zones::create_object` (an insertion); for a
    /// library it then informs the knowledge as [`KWorld::enter_card`] does.
    pub fn insert_card(
        &mut self,
        owner: PlayerId,
        card: CardId,
        req: MoveRequest,
        index_known_by: Option<&[PlayerId]>,
    ) -> ObjectId {
        let id = self.create_card(card, owner, req);
        if req.to == ZoneKind::Library {
            self.library_changed(owner, index_known_by);
        }
        id
    }

    /// `on_library_changed_indexed(owner, ikb)` for `Some(ikb)`, `on_library_changed(owner)`
    /// for `None`.
    pub fn library_changed(&mut self, owner: PlayerId, index_known_by: Option<&[PlayerId]>) {
        match index_known_by {
            Some(ikb) => self
                .k
                .on_library_changed_indexed(&self.zones, owner, ikb)
                .expect("on_library_changed_indexed succeeds"),
            None => self
                .k
                .on_library_changed(&self.zones, owner)
                .expect("on_library_changed succeeds"),
        }
    }

    pub fn mv_to(&mut self, id: ObjectId, kind: ZoneKind) -> Moved {
        self.mv(id, MoveRequest::new(kind))
    }

    /// A move the knowledge is not told about (an omniscient-only change).
    pub fn raw_mv(&mut self, id: ObjectId, req: MoveRequest) -> Moved {
        self.zones
            .move_object(&mut self.ids, id, req)
            .expect("move succeeds")
    }

    pub fn known_to(&self, id: ObjectId) -> BTreeSet<PlayerId> {
        self.k.known_to(&self.zones, id).expect("known_to succeeds")
    }

    pub fn can(&self, viewer: PlayerId, id: ObjectId) -> bool {
        self.k
            .can_identify(&self.zones, viewer, id)
            .expect("can_identify succeeds")
    }

    pub fn view(&self, viewer: PlayerId) -> ZonesView {
        self.k
            .project(&self.zones, viewer)
            .expect("project succeeds")
    }

    pub fn library(&self, owner: PlayerId) -> Vec<ObjectId> {
        self.zones.contents(ZoneId::Library(owner))
    }

    pub fn positions(&self, viewer: PlayerId, owner: PlayerId) -> BTreeMap<usize, ObjectId> {
        self.k.position_known(&self.zones, viewer, owner)
    }

    pub fn req_at(kind: ZoneKind, position: Position) -> MoveRequest {
        let mut req = MoveRequest::new(kind);
        req.position = position;
        req
    }
}

// ---------------------------------------------------------------------------------------------
// the projection rules and the independent shadow model of the knowledge
// ---------------------------------------------------------------------------------------------

/// Entitlement rules 1-3 of the brief, evaluated on a record only (no stored knowledge):
/// public face-up (CR 400.2), in the viewer's own hand (CR 402.3), face down on the
/// battlefield or stack and controlled by the viewer (CR 708.5).
pub fn implicit_can(r: &ObjectRecord, viewer: PlayerId) -> bool {
    // (1) public info, CR 400.2
    let public = r.zone.kind().is_public() && !r.face_down;
    // (2) own hand, CR 402.3
    let own_hand = r.zone == ZoneId::Hand(viewer);
    // (3) face-down permanent or spell the viewer controls, CR 708.5
    let fd_ctl = r.face_down
        && matches!(r.zone, ZoneId::Battlefield | ZoneId::Stack)
        && r.controller == viewer;
    public || own_hand || fd_ctl
}

/// The projection rules of the brief (REQ-CORE-004; CR 400.2, 401.2, 401.3, 402.3), with the
/// per-object identification decided by `can`: libraries and opponent hands show a count and
/// the set of identifiable cards, every other zone shows its objects in container order.
pub fn project_with(zones: &Zones, viewer: PlayerId, can: &dyn Fn(ObjectId) -> bool) -> ZonesView {
    let object_view = |id: ObjectId| -> ObjectView {
        let r = zones.object(id).expect("object present");
        ObjectView {
            id,
            owner: r.owner,
            controller: r.controller,
            face_down: r.face_down,
            card: can(id).then_some(r.card),
        }
    };
    let ov = |zone: ZoneId| -> Vec<ObjectView> {
        zones.contents(zone).into_iter().map(object_view).collect()
    };
    let cards = |zone: ZoneId| -> BTreeSet<CardId> {
        zones
            .contents(zone)
            .into_iter()
            .filter(|id| can(*id))
            .map(|id| zones.object(id).expect("object present").card)
            .collect()
    };
    let mut players = BTreeMap::new();
    for p in zones.players() {
        let library = LibraryView {
            count: zones.len(ZoneId::Library(p)),
            known_cards: cards(ZoneId::Library(p)),
        };
        let hand = if p == viewer {
            HandView::Own(ov(ZoneId::Hand(p)))
        } else {
            HandView::Opponent {
                count: zones.len(ZoneId::Hand(p)),
                known_cards: cards(ZoneId::Hand(p)),
            }
        };
        players.insert(
            p,
            PlayerZonesView {
                library,
                hand,
                graveyard: ov(ZoneId::Graveyard(p)),
            },
        );
    }
    ZonesView {
        viewer,
        players,
        battlefield: ov(ZoneId::Battlefield),
        stack: ov(ZoneId::Stack),
        exile: ov(ZoneId::Exile),
        command: ov(ZoneId::Command),
    }
}

/// A consistency projection: the projection rules applied to the implementation's own
/// `known_to`. It checks that `project` agrees with `known_to`; it is NOT an independent
/// oracle (the [`Shadow`] model is).
pub fn consistency_project(zones: &Zones, k: &Knowledge, viewer: PlayerId) -> ZonesView {
    project_with(zones, viewer, &|id| {
        let r = zones.object(id).expect("object present");
        implicit_can(r, viewer)
            || k.known_to(zones, id)
                .expect("known_to succeeds")
                .contains(&viewer)
    })
}

/// An independent model of the knowledge, written from the brief's text only. It never calls
/// the implementation; the physical state comes from `Zones` (REQ-CORE-004).
///
/// Identity knowledge is stored per card (it follows the card across zone changes); position
/// entries are stored per (viewer, library owner).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shadow {
    known: BTreeMap<CardId, BTreeSet<PlayerId>>,
    pos: BTreeMap<(PlayerId, PlayerId), BTreeMap<usize, ObjectId>>,
}

impl Shadow {
    pub fn new() -> Shadow {
        Shadow::default()
    }

    fn rec(zones: &Zones, id: ObjectId) -> ObjectRecord {
        *zones.object(id).expect("shadow: object present")
    }

    fn add(&mut self, card: CardId, p: PlayerId) {
        self.known.entry(card).or_default().insert(p);
    }

    fn remove(&mut self, card: CardId, p: PlayerId) {
        if let Some(s) = self.known.get_mut(&card) {
            s.remove(&p);
        }
    }

    /// The stored KnownTo of `id` (what `known_to` must report).
    pub fn stored(&self, zones: &Zones, id: ObjectId) -> BTreeSet<PlayerId> {
        let card = Shadow::rec(zones, id).card;
        self.known.get(&card).cloned().unwrap_or_default()
    }

    /// Entitlement rules 1-4 (CR 400.2, 402.3, 708.5; stored KnownTo).
    pub fn can(&self, zones: &Zones, viewer: PlayerId, id: ObjectId) -> bool {
        implicit_can(&Shadow::rec(zones, id), viewer) || self.stored(zones, id).contains(&viewer)
    }

    /// The expected `project(zones, viewer)`.
    pub fn project(&self, zones: &Zones, viewer: PlayerId) -> ZonesView {
        project_with(zones, viewer, &|id| self.can(zones, viewer, id))
    }

    /// The expected `position_known`: stored entries that match the live library (CR 401.2).
    pub fn positions(
        &self,
        zones: &Zones,
        viewer: PlayerId,
        owner: PlayerId,
    ) -> BTreeMap<usize, ObjectId> {
        let lib = zones.contents(ZoneId::Library(owner));
        self.pos
            .get(&(viewer, owner))
            .map(|e| {
                e.iter()
                    .filter(|(i, o)| lib.get(**i) == Some(*o))
                    .map(|(i, o)| (*i, *o))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn add_known(&mut self, zones: &Zones, id: ObjectId, p: PlayerId) {
        self.add(Shadow::rec(zones, id).card, p);
    }

    /// CR 701.20a: every player of the game.
    pub fn reveal_to_all(&mut self, zones: &Zones, id: ObjectId) {
        let card = Shadow::rec(zones, id).card;
        for p in zones.players() {
            self.add(card, p);
        }
    }

    /// CR 406.3, 406.3b: the viewer is stored in KnownTo.
    pub fn grant_look(&mut self, zones: &Zones, id: ObjectId, p: PlayerId) {
        self.add_known(zones, id, p);
    }

    /// CR 401.2: replaces an entry for the same object and for the same index; adds the viewer
    /// to KnownTo.
    pub fn set_position_known(
        &mut self,
        zones: &Zones,
        viewer: PlayerId,
        owner: PlayerId,
        index: usize,
        id: ObjectId,
    ) {
        let e = self.pos.entry((viewer, owner)).or_default();
        e.retain(|_, o| *o != id);
        e.insert(index, id);
        self.add_known(zones, id, viewer);
    }

    /// CR 701.24a, 701.24b, SRS 2.4: a shuffle clears every position entry of that library for
    /// every viewer, excluded found cards included. KnownTo is never touched.
    pub fn shuffle_library(&mut self, _zones: &Zones, owner: PlayerId, _excluded: &[ObjectId]) {
        for ((_, o), e) in &mut self.pos {
            if *o == owner {
                e.clear();
            }
        }
    }

    /// CR 406.3, 701.24a: the KnownTo of every card of the pile is cleared.
    pub fn shuffle_face_down_pile(&mut self, zones: &Zones, ids: &[ObjectId]) {
        for id in ids {
            let card = Shadow::rec(zones, *id).card;
            self.known.remove(&card);
        }
    }

    /// Drops every entry for `owner`'s library that does not match the live library (CR 401.2).
    fn prune(&mut self, zones: &Zones, owner: PlayerId) {
        let lib = zones.contents(ZoneId::Library(owner));
        for ((_, o), e) in &mut self.pos {
            if *o == owner {
                e.retain(|i, obj| lib.get(*i) == Some(obj));
            }
        }
    }

    /// The brief's index-disclosure rule (a design decision, no CR) for one affected library:
    /// every viewer NOT in `index_known_by` loses all of its entries for `owner`'s library,
    /// without looking at where the change happened; the entries of the viewers in it are then
    /// pruned against the live library (CR 401.2).
    fn index_rule(&mut self, zones: &Zones, owner: PlayerId, index_known_by: &[PlayerId]) {
        for ((v, o), e) in &mut self.pos {
            if *o == owner && !index_known_by.contains(v) {
                e.clear();
            }
        }
        self.prune(zones, owner);
    }

    /// `on_library_changed` (the no-disclosure shorthand): every viewer loses its entries for
    /// that library.
    pub fn on_library_changed(&mut self, zones: &Zones, owner: PlayerId) {
        self.library_changed_indexed(zones, owner, &[]);
    }

    /// `on_library_changed_indexed`: the index-disclosure rule for `owner`'s library; KnownTo is
    /// never touched.
    pub fn library_changed_indexed(
        &mut self,
        zones: &Zones,
        owner: PlayerId,
        index_known_by: &[PlayerId],
    ) {
        self.index_rule(zones, owner, index_known_by);
    }

    /// Whether step 0 of the hidden-source identity rule applies to a move.
    pub fn step0_applies(before: &ObjectRecord, moved: &Moved) -> bool {
        matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand) && moved.from != moved.to
    }

    /// The players who observed a hidden-source move, by (i)-(iii) of the brief; `None` when
    /// step 0 does not apply. `zones` is the state after the move.
    ///
    /// Observation is a design rule of the brief (no CR): (i) listed in `observers`, (ii)
    /// implicitly entitled to the NEW record, (iii) the source is the player's own hand. Holding
    /// a position entry naming the moved card does NOT make a player an observer: knowing where
    /// a card is does not reveal which position was selected. The model never looks at the
    /// stored knowledge or at which object moved to decide observation.
    pub fn observed(
        &self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
        observers: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        if !Shadow::step0_applies(before, moved) {
            return None;
        }
        let new = Shadow::rec(zones, moved.new);
        let mut out = BTreeSet::new();
        for v in zones.players() {
            let i = observers.contains(&v);
            let ii = implicit_can(&new, v);
            let iii = before.zone == ZoneId::Hand(v);
            if i || ii || iii {
                out.insert(v);
            }
        }
        Some(out)
    }

    /// `on_move_observed` (and `on_move` with no observers): `on_move_indexed` with nobody
    /// knowing the index.
    pub fn on_move(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
        observers: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        self.on_move_indexed(zones, before, moved, observers, &[])
    }

    /// `on_move_indexed`: step 0 (with `observers`), rules (a)-(c), then the index-disclosure
    /// rule for every affected library (moved.from and moved.to when they are libraries), with
    /// effect (d) for the viewers in `index_known_by`. Returns the observed set when step 0
    /// applied.
    pub fn on_move_indexed(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        let players = zones.players();
        let card = before.card;
        // step 0 (design rule of the brief, not CR-derived)
        let observed = self.observed(zones, before, moved, observers);
        if let Some(obs) = &observed {
            let mut source: Vec<CardId> = zones
                .contents(moved.from)
                .into_iter()
                .map(|id| Shadow::rec(zones, id).card)
                .collect();
            source.push(card);
            for v in players.iter().filter(|v| !obs.contains(v)) {
                for c in &source {
                    self.remove(*c, *v);
                }
                if let ZoneId::Library(owner) = moved.from {
                    self.pos.remove(&(*v, owner));
                }
            }
        }
        // (a) CR 400.2
        if before.zone.kind().is_public() && !before.face_down {
            for p in &players {
                self.add(card, *p);
            }
        }
        // (b) CR 402.3
        if before.zone == ZoneId::Hand(before.owner) {
            self.add(card, before.owner);
        }
        // (c) CR 708.5, 708.9
        if before.face_down && matches!(before.zone, ZoneId::Battlefield | ZoneId::Stack) {
            self.add(card, before.controller);
            let reveal = moved.from != moved.to
                && (before.zone == ZoneId::Battlefield || moved.to != ZoneId::Battlefield);
            if reveal {
                for p in &players {
                    self.add(card, *p);
                }
            }
        }
        // (d) CR 401.2
        if let ZoneId::Library(owner) = before.zone {
            for ((_, o), e) in &mut self.pos {
                if *o == owner {
                    e.retain(|_, obj| *obj != moved.old);
                }
            }
        }
        // index-disclosure rule of the brief (design decision): only the viewers who know the
        // index keep (pruned) entries for an affected library
        for z in [moved.from, moved.to] {
            if let ZoneId::Library(owner) = z {
                self.index_rule(zones, owner, index_known_by);
            }
        }
        observed
    }

    /// The players who observed a removal from a hidden zone (`on_removed_from_hidden_zone`),
    /// by the brief's design rule (no CR): (i) listed in `observers`, or (iii) the source is the
    /// player's own hand. Condition (ii) does not apply (there is no new object). `None` when the
    /// source is not a library or a hand (the hook does nothing). Never depends on which object
    /// was removed or on the stored knowledge.
    pub fn removal_observed(
        zones: &Zones,
        before: &ObjectRecord,
        observers: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        if !matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand) {
            return None;
        }
        Some(
            zones
                .players()
                .into_iter()
                .filter(|v| observers.contains(v) || before.zone == ZoneId::Hand(*v))
                .collect(),
        )
    }

    /// The model of `on_removed_from_hidden_zone`, called after `Zones::send_outside` with the
    /// record copied before the removal. Every non-observer forgets every card now in the source
    /// zone and the removed card; for a library source the non-observer's position entries for
    /// that library are dropped, and (nobody knowing the index) so are everyone else's. A public
    /// source is a no-op. Returns the observed set (`None` for a public source).
    pub fn removed_from_hidden_zone(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        observers: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        self.removed_from_hidden_zone_indexed(zones, before, observers, &[])
    }

    /// `on_removed_from_hidden_zone_indexed`: identity as [`Shadow::removed_from_hidden_zone`],
    /// then for a library source the index-disclosure rule (only `index_known_by` keep their
    /// pruned entries for that library).
    pub fn removed_from_hidden_zone_indexed(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> Option<BTreeSet<PlayerId>> {
        let observed = Shadow::removal_observed(zones, before, observers)?;
        let mut source: Vec<CardId> = zones
            .contents(before.zone)
            .into_iter()
            .map(|id| Shadow::rec(zones, id).card)
            .collect();
        source.push(before.card);
        for v in zones.players().iter().filter(|v| !observed.contains(v)) {
            for c in &source {
                self.remove(*c, *v);
            }
            if let ZoneId::Library(owner) = before.zone {
                self.pos.remove(&(*v, owner));
            }
        }
        if let ZoneId::Library(owner) = before.zone {
            self.index_rule(zones, owner, index_known_by);
        }
        Some(observed)
    }

    /// CR 708.9: the face-down permanents and spells `p` owns are revealed to all.
    pub fn on_player_leaves(&mut self, zones: &Zones, p: PlayerId) {
        self.reveal_face_down(zones, Some(p));
    }

    /// CR 708.9: every face-down permanent and spell is revealed to all.
    pub fn on_game_end(&mut self, zones: &Zones) {
        self.reveal_face_down(zones, None);
    }

    fn reveal_face_down(&mut self, zones: &Zones, owner: Option<PlayerId>) {
        for id in zones.all_object_ids() {
            let r = Shadow::rec(zones, id);
            let fd = r.face_down && matches!(r.zone, ZoneId::Battlefield | ZoneId::Stack);
            if fd && owner.is_none_or(|o| o == r.owner) {
                self.reveal_to_all(zones, id);
            }
        }
    }
}

/// Asserts that every player of `players` has equal whole views and equal `position_known`
/// for every library in `a` and `b` (the twin-world non-interference check, REQ-CORE-004).
pub fn assert_twins_equal_for(a: &KWorld, b: &KWorld, players: &[PlayerId], ctx: &str) {
    for p in players {
        assert_eq!(a.view(*p), b.view(*p), "{ctx}: view of {p:?}");
        for owner in a.players() {
            assert_eq!(
                a.positions(*p, owner),
                b.positions(*p, owner),
                "{ctx}: position_known of {p:?} on {owner:?}'s library"
            );
        }
    }
}

/// Every ObjectView in a view (hand Own, graveyards and the shared zones).
pub fn object_views(v: &ZonesView) -> Vec<ObjectView> {
    let mut out = Vec::new();
    for pz in v.players.values() {
        if let HandView::Own(objs) = &pz.hand {
            out.extend(objs.iter().cloned());
        }
        out.extend(pz.graveyard.iter().cloned());
    }
    out.extend(v.battlefield.iter().cloned());
    out.extend(v.stack.iter().cloned());
    out.extend(v.exile.iter().cloned());
    out.extend(v.command.iter().cloned());
    out
}

/// Every CardId present anywhere in a view.
pub fn view_cards(v: &ZonesView) -> Vec<CardId> {
    let mut out: Vec<CardId> = object_views(v).iter().filter_map(|o| o.card).collect();
    for pz in v.players.values() {
        out.extend(pz.library.known_cards.iter().copied());
        if let HandView::Opponent { known_cards, .. } = &pz.hand {
            out.extend(known_cards.iter().copied());
        }
    }
    out
}

/// The library `known_cards` of `owner` in `v`.
pub fn lib_known(v: &ZonesView, owner: PlayerId) -> BTreeSet<CardId> {
    v.players[&owner].library.known_cards.clone()
}

/// The opponent-hand `known_cards` of `owner` in `v` (panics on an own hand).
pub fn opp_hand_known(v: &ZonesView, owner: PlayerId) -> BTreeSet<CardId> {
    match &v.players[&owner].hand {
        HandView::Opponent { known_cards, .. } => known_cards.clone(),
        HandView::Own(_) => panic!("expected an opponent hand view"),
    }
}

/// The ObjectView of `id` in the shared zones / graveyards / own hand of `v`, if any.
pub fn find_view(v: &ZonesView, id: ObjectId) -> Option<ObjectView> {
    object_views(v).into_iter().find(|o| o.id == id)
}

// ---------------------------------------------------------------------------------------------
// encoding
// ---------------------------------------------------------------------------------------------

pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// Asserts that encoding appends to a non-empty buffer without clearing it.
pub fn assert_appends<T: CanonicalEncode + ?Sized>(v: &T) {
    let prefix = vec![0xA5u8, 0x5A, 0x01];
    let mut out = prefix.clone();
    v.encode_canonical(&mut out);
    assert!(out.starts_with(&prefix), "prefix kept");
    assert_eq!(
        &out[prefix.len()..],
        bytes(v).as_slice(),
        "suffix is the encoding"
    );
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers for property tests (R3: seeded, no system source)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator.
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// A value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn idx(&mut self, n: usize) -> usize {
        usize::try_from(self.below(n as u64)).expect("fits")
    }

    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.idx(items.len())]
    }
}
