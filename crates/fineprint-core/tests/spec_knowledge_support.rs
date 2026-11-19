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
    MoveRequest, Moved, ObjectRecord, Position, ZoneId, ZoneKind, Zones, ZonesConfig,
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
// the independent oracle (entitlement rules 1-4 of the brief)
// ---------------------------------------------------------------------------------------------

/// Whether `viewer` may identify object `id`, computed from the Zones and the stored KnownTo
/// only (REQ-CORE-004; CR 400.2, CR 402.3, CR 708.5).
pub fn oracle_can(zones: &Zones, k: &Knowledge, viewer: PlayerId, id: ObjectId) -> bool {
    let r = zones.object(id).expect("object present");
    // (1) public info, CR 400.2
    let public = r.zone.kind().is_public() && !r.face_down;
    // (2) own hand, CR 402.3
    let own_hand = r.zone == ZoneId::Hand(viewer);
    // (3) face-down permanent or spell the viewer controls, CR 708.5
    let fd_ctl = r.face_down
        && matches!(r.zone, ZoneId::Battlefield | ZoneId::Stack)
        && r.controller == viewer;
    // (4) stored KnownTo
    let known = k
        .known_to(zones, id)
        .expect("known_to succeeds")
        .contains(&viewer);
    public || own_hand || fd_ctl || known
}

fn oracle_object_view(zones: &Zones, k: &Knowledge, viewer: PlayerId, id: ObjectId) -> ObjectView {
    let r = zones.object(id).expect("object present");
    ObjectView {
        id,
        owner: r.owner,
        controller: r.controller,
        face_down: r.face_down,
        card: if oracle_can(zones, k, viewer, id) {
            Some(r.card)
        } else {
            None
        },
    }
}

fn oracle_cards(zones: &Zones, k: &Knowledge, viewer: PlayerId, zone: ZoneId) -> BTreeSet<CardId> {
    zones
        .contents(zone)
        .into_iter()
        .filter(|id| oracle_can(zones, k, viewer, *id))
        .map(|id| zones.object(id).expect("object present").card)
        .collect()
}

/// The projection the brief specifies, built independently of `Knowledge::project`
/// (REQ-CORE-004).
pub fn oracle_project(zones: &Zones, k: &Knowledge, viewer: PlayerId) -> ZonesView {
    let ov = |zone: ZoneId| -> Vec<ObjectView> {
        zones
            .contents(zone)
            .into_iter()
            .map(|id| oracle_object_view(zones, k, viewer, id))
            .collect()
    };
    let mut players = BTreeMap::new();
    for p in zones.players() {
        let library = LibraryView {
            count: zones.len(ZoneId::Library(p)),
            known_cards: oracle_cards(zones, k, viewer, ZoneId::Library(p)),
        };
        let hand = if p == viewer {
            HandView::Own(ov(ZoneId::Hand(p)))
        } else {
            HandView::Opponent {
                count: zones.len(ZoneId::Hand(p)),
                known_cards: oracle_cards(zones, k, viewer, ZoneId::Hand(p)),
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
