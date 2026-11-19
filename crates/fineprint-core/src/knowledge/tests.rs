// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the knowledge module: REQ-CORE-004 (identity and position knowledge, shuffles,
// the projection V(S, P)), REQ-ZONE-001 (visibility rules), REQ-NFR-006 (canonical encoding).
// Synthetic ids only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::{CanonicalEncode, encode_to_vec};
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};

use super::*;
use crate::zones::{
    MoveRequest, Moved, ObjectRecord, OutsidePool, Position, ZoneId, ZoneKind, Zones, ZonesConfig,
};

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn c(n: u64) -> CardId {
    CardId::new(n)
}

const P1: PlayerId = PlayerId::new(1);
const P2: PlayerId = PlayerId::new(2);
const NOBODY: PlayerId = PlayerId::new(99);

/// A two-player game with its knowledge and id generator.
#[derive(Clone)]
struct World {
    z: Zones,
    k: Knowledge,
    ids: IdGenerator<ObjectId>,
}

impl World {
    fn new() -> World {
        World {
            z: Zones::new(&[P1, P2], ZonesConfig::default()),
            k: Knowledge::new(),
            ids: IdGenerator::new(),
        }
    }

    fn create(&mut self, card: u64, owner: PlayerId, req: MoveRequest) -> ObjectId {
        self.z
            .create_object(&mut self.ids, c(card), owner, req)
            .unwrap()
    }

    /// Creates cards `first..first+n` in `owner`'s library, top first in creation order.
    fn fill_library(&mut self, owner: PlayerId, first: u64, n: u64) -> Vec<ObjectId> {
        (first..first + n)
            .map(|k| self.create(k, owner, at(ZoneKind::Library, Position::Bottom)))
            .collect()
    }

    /// Moves `id` and applies `on_move`; returns the move.
    fn mv(&mut self, id: ObjectId, req: MoveRequest) -> Moved {
        let before = *self.z.object(id).unwrap();
        let moved = self.z.move_object(&mut self.ids, id, req).unwrap();
        self.k.on_move(&self.z, &before, &moved).unwrap();
        moved
    }

    /// Moves `id` and applies `on_move_observed` with `observers`; returns the move.
    fn mv_obs(&mut self, id: ObjectId, req: MoveRequest, observers: &[PlayerId]) -> Moved {
        let before = *self.z.object(id).unwrap();
        let moved = self.z.move_object(&mut self.ids, id, req).unwrap();
        self.k
            .on_move_observed(&self.z, &before, &moved, observers)
            .unwrap();
        moved
    }

    /// Moves `id` and applies `on_move_indexed` with `observers` and `index_known_by`.
    fn mv_idx(
        &mut self,
        id: ObjectId,
        req: MoveRequest,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> Moved {
        let before = *self.z.object(id).unwrap();
        let moved = self.z.move_object(&mut self.ids, id, req).unwrap();
        self.k
            .on_move_indexed(&self.z, &before, &moved, observers, index_known_by)
            .unwrap();
        moved
    }

    /// The object currently holding `card`.
    fn obj(&self, card: u64) -> ObjectId {
        self.z.object_of_card(c(card)).unwrap()
    }

    fn can(&self, viewer: PlayerId, id: ObjectId) -> bool {
        self.k.can_identify(&self.z, viewer, id).unwrap()
    }

    fn view(&self, viewer: PlayerId) -> ZonesView {
        self.k.project(&self.z, viewer).unwrap()
    }
}

fn req(to: ZoneKind) -> MoveRequest {
    MoveRequest::new(to)
}

fn at(to: ZoneKind, position: Position) -> MoveRequest {
    MoveRequest {
        position,
        ..MoveRequest::new(to)
    }
}

fn face_down(to: ZoneKind) -> MoveRequest {
    MoveRequest {
        face_down: true,
        ..MoveRequest::new(to)
    }
}

fn set(players: &[PlayerId]) -> BTreeSet<PlayerId> {
    players.iter().copied().collect()
}

fn cards(ids: &[u64]) -> BTreeSet<CardId> {
    ids.iter().map(|&k| c(k)).collect()
}

// ---------------------------------------------------------------------------------------------
// Basics
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004: a new state is empty and equals the default; add_known and known_to.
#[test]
fn new_is_default_and_add_known_records_player() {
    let mut w = World::new();
    assert_eq!(Knowledge::new(), Knowledge::default());
    let h = w.create(1, P1, req(ZoneKind::Hand));
    assert_eq!(w.k.known_to(&w.z, h).unwrap(), BTreeSet::new());
    w.k.add_known(&w.z, h, P2).unwrap();
    assert_eq!(w.k.known_to(&w.z, h).unwrap(), set(&[P2]));
    assert!(w.can(P2, h));
}

/// REQ-CORE-004, CR 701.20a, CR 701.20b: a reveal adds every player and the object stays put.
#[test]
fn reveal_to_all_adds_every_player_without_moving() {
    let mut w = World::new();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    assert!(!w.can(P2, h));
    w.k.reveal_to_all(&w.z, h).unwrap();
    assert_eq!(w.k.known_to(&w.z, h).unwrap(), set(&[P1, P2]));
    assert_eq!(w.z.object(h).unwrap().zone, ZoneId::Hand(P1));
    assert!(w.can(P2, h));
}

/// REQ-CORE-004, REQ-ZONE-001, CR 400.2, CR 402.3: implicit entitlements are not stored.
#[test]
fn entitlement_rule_implicit_cases() {
    let mut w = World::new();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    let l = w.create(2, P1, req(ZoneKind::Library));
    let b = w.create(3, P1, req(ZoneKind::Battlefield));
    let g = w.create(4, P2, req(ZoneKind::Graveyard));
    // CR 402.3: own hand only.
    assert!(w.can(P1, h));
    assert!(!w.can(P2, h));
    // CR 400.2: the library is hidden, even to the owner.
    assert!(!w.can(P1, l));
    assert!(!w.can(P2, l));
    // Public zones.
    assert!(w.can(P2, b) && w.can(P1, b) && w.can(P1, g));
    for id in [h, l, b, g] {
        assert!(w.k.known_to(&w.z, id).unwrap().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Test 1: look-two-return-two
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 701.24a: look at the top two, then a shuffle clears positions for every
/// viewer and keeps the set knowledge.
#[test]
fn look_two_return_two_then_shuffle() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 5);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    // Another viewer knows a position too (the bottom card).
    w.k.set_position_known(&w.z, P2, P1, 4, lib[4]).unwrap();
    let known = w.k.position_known(&w.z, P1, P1);
    assert_eq!(known, BTreeMap::from([(0, lib[0]), (1, lib[1])]));
    assert_eq!(
        w.view(P1).players[&P1].library.known_cards,
        cards(&[10, 11])
    );

    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
    assert!(w.k.known_to(&w.z, lib[0]).unwrap().contains(&P1));
    assert!(w.k.known_to(&w.z, lib[1]).unwrap().contains(&P1));
    let v1 = w.view(P1);
    assert_eq!(v1.players[&P1].library.known_cards, cards(&[10, 11]));
    assert_eq!(v1.players[&P1].library.count, 5);
    let v2 = w.view(P2);
    assert_eq!(v2.players[&P1].library.known_cards, cards(&[14]));
    assert_eq!(v2.players[&P1].library.count, 5);
}

/// REQ-CORE-004, CR 701.24a: the other player sees no known cards after the viewer's look.
#[test]
fn look_two_return_two_other_player_knows_nothing() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    assert!(w.view(P2).players[&P1].library.known_cards.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Test 2: reveal then shuffle in
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 701.20a, CR 701.24a: a revealed card shuffled into the library while every
/// player watches stays known as a set member to everyone; no one knows its position. Without
/// observers the opponent cannot tell which hand card went in and forgets it (step 0).
#[test]
fn reveal_then_shuffle_in() {
    let mut w = World::new();
    w.fill_library(P1, 10, 3);
    let h = w.create(1, P1, req(ZoneKind::Hand));
    w.k.reveal_to_all(&w.z, h).unwrap();
    let mut unobserved = w.clone();
    let m = w.mv_obs(h, req(ZoneKind::Library), &[P1, P2]);
    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    for v in [P1, P2] {
        assert!(w.can(v, m.new));
        assert_eq!(w.view(v).players[&P1].library.known_cards, cards(&[1]));
        assert!(w.k.position_known(&w.z, v, P1).is_empty());
    }
    let m = unobserved.mv(h, req(ZoneKind::Library));
    unobserved
        .k
        .shuffle_library(&unobserved.z, P1, &[])
        .unwrap();
    assert!(unobserved.can(P1, m.new));
    assert!(!unobserved.can(P2, m.new));
    assert!(
        unobserved.view(P2).players[&P1]
            .library
            .known_cards
            .is_empty()
    );
}

// ---------------------------------------------------------------------------------------------
// Test 3: CR 701.24b
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 701.24b: for viewers who know which card is excluded, an excluded found
/// card keeps its entry while still at its index; non-excluded entries are cleared. Contrast:
/// the shorthand `shuffle_library` (nobody told) clears every entry, set knowledge stays.
#[test]
fn shuffle_excluded_card_keeps_position_while_in_place() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 4);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P1, 2, lib[2]).unwrap();
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    let mut shorthand = w.clone();
    w.k.shuffle_library_disclosed(&w.z, P1, &[lib[0]], &[P1, P2])
        .unwrap();
    assert_eq!(
        w.k.position_known(&w.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );
    assert_eq!(
        w.k.position_known(&w.z, P2, P1),
        BTreeMap::from([(0, lib[0])])
    );

    shorthand
        .k
        .shuffle_library(&shorthand.z, P1, &[lib[0]])
        .unwrap();
    assert!(shorthand.k.positions.is_empty());
    assert!(shorthand.can(P2, lib[0]));
    assert!(shorthand.can(P1, lib[2]));
}

/// REQ-CORE-004, CR 701.24b: an excluded card no longer at its recorded index loses its entry.
#[test]
fn shuffle_excluded_card_moved_loses_position() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 4);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    // Reposition without telling the knowledge (the stale entry stays stored).
    w.z.move_object(
        &mut w.ids,
        lib[0],
        at(ZoneKind::Library, Position::Index(2)),
    )
    .unwrap();
    w.k.shuffle_library(&w.z, P1, &[lib[0]]).unwrap();
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    assert!(w.k.positions.is_empty());
    // Set knowledge survives.
    assert!(w.can(P1, lib[0]));
}

/// REQ-CORE-004, CR 701.24b: an excluded id not in that library is rejected, state unchanged.
#[test]
fn shuffle_excluded_not_in_library_errors() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 2);
    let other = w.fill_library(P2, 20, 1);
    let h = w.create(1, P1, req(ZoneKind::Hand));
    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    let snapshot = w.k.clone();
    for bad in [other[0], h, ObjectId::new(777)] {
        assert_eq!(
            w.k.shuffle_library(&w.z, P1, &[lib[0], bad]),
            Err(KnowledgeError::NotInLibrary {
                owner: P1,
                object: bad
            })
        );
    }
    assert_eq!(w.k, snapshot);
    assert_eq!(
        w.k.shuffle_library(&w.z, NOBODY, &[]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
}

/// REQ-CORE-004, CR 701.24a: a shuffle of one library does not touch another library's entries.
#[test]
fn shuffle_only_affects_that_library() {
    let mut w = World::new();
    let a = w.fill_library(P1, 10, 2);
    let b = w.fill_library(P2, 20, 2);
    w.k.set_position_known(&w.z, P1, P1, 0, a[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P2, 1, b[1]).unwrap();
    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    assert_eq!(
        w.k.position_known(&w.z, P1, P2),
        BTreeMap::from([(1, b[1])])
    );
}

// ---------------------------------------------------------------------------------------------
// Position knowledge
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 401.4: set_position_known replaces an entry for the same object or index.
#[test]
fn set_position_known_replaces_same_object_or_index() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    // A manual reposition leaves a stale entry; setting the new index replaces it.
    w.z.move_object(&mut w.ids, lib[1], at(ZoneKind::Library, Position::Top))
        .unwrap();
    w.k.set_position_known(&w.z, P1, P1, 0, lib[1]).unwrap();
    assert_eq!(w.k.positions[&(P1, P1)], BTreeMap::from([(0, lib[1])]));
    // Same index, another object replaces it.
    w.z.move_object(&mut w.ids, lib[0], at(ZoneKind::Library, Position::Top))
        .unwrap();
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    assert_eq!(w.k.positions[&(P1, P1)], BTreeMap::from([(0, lib[0])]));
    assert!(w.k.known_to(&w.z, lib[0]).unwrap().contains(&P1));
}

/// REQ-CORE-004: position_known filters lazily against the live library; unknown players give
/// an empty map.
#[test]
fn position_known_is_lazily_filtered() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    // A card put on top without on_move shifts the index.
    w.create(50, P1, req(ZoneKind::Library));
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    assert!(w.k.position_known(&w.z, NOBODY, P1).is_empty());
    assert!(w.k.position_known(&w.z, P1, NOBODY).is_empty());
}

/// REQ-CORE-004: a library move through on_move prunes entries whose index shifted and drops
/// entries naming a card that left (decision 4).
#[test]
fn on_move_prunes_shifted_and_departed_positions() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 4);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P1, 3, lib[3]).unwrap();
    // Draw the top card: lib[0] leaves, lib[3] shifts to index 2.
    w.mv(lib[0], req(ZoneKind::Hand));
    assert!(w.k.positions.is_empty());
    // The owner still knows the bottom card as a set member.
    assert_eq!(w.view(P1).players[&P1].library.known_cards, cards(&[13]));
}

/// REQ-CORE-004: a same-zone reposition in a library drops the entry naming the object.
#[test]
fn on_move_reposition_in_library_drops_entry() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    let m = w.mv(lib[0], at(ZoneKind::Library, Position::Top));
    assert_eq!(m.old, m.new);
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
}

/// REQ-CORE-004: a card put on the bottom leaves an entry above it untouched for a viewer who
/// knows the insertion index. Contrast: with the shorthand (index undisclosed) the entry goes.
#[test]
fn on_move_keeps_entries_that_still_match() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    let mut shorthand = w.clone();
    w.mv_idx(h, at(ZoneKind::Library, Position::Bottom), &[], &[P1]);
    assert_eq!(
        w.k.position_known(&w.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );
    shorthand.mv(h, at(ZoneKind::Library, Position::Bottom));
    assert!(shorthand.k.positions.is_empty());
    assert!(shorthand.can(P1, lib[0]));
}

// ---------------------------------------------------------------------------------------------
// Test 4: CR 708.5 / CR 708.9
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 708.5: the controller of a face-down permanent can identify it; others
/// (including a non-controlling owner) cannot.
#[test]
fn face_down_permanent_controller_only() {
    let mut w = World::new();
    let fd = w.create(1, P1, face_down(ZoneKind::Battlefield));
    assert!(w.can(P1, fd));
    assert!(!w.can(P2, fd));
    w.z.set_controller(fd, P2).unwrap();
    assert!(w.can(P2, fd));
    assert!(!w.can(P1, fd));
    assert!(w.k.known_to(&w.z, fd).unwrap().is_empty());
}

/// REQ-CORE-004, CR 708.9: a face-down permanent moved to a hand is revealed to all (through
/// KnownTo).
#[test]
fn face_down_permanent_to_hand_revealed() {
    let mut w = World::new();
    let fd = w.create(1, P1, face_down(ZoneKind::Battlefield));
    let m = w.mv(fd, req(ZoneKind::Hand));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1, P2]));
    assert!(w.can(P2, m.new));
    assert_eq!(
        w.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 1,
            known_cards: cards(&[1])
        }
    );
}

/// REQ-CORE-004, CR 708.5, CR 708.9: a face-down spell stack -> battlefield is not revealed;
/// the controller keeps knowing it.
#[test]
fn face_down_spell_to_battlefield_not_revealed() {
    let mut w = World::new();
    let s = w.create(1, P1, face_down(ZoneKind::Stack));
    let m = w.mv(s, face_down(ZoneKind::Battlefield));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1]));
    assert!(!w.can(P2, m.new));
    assert!(w.can(P1, m.new));
}

/// REQ-CORE-004, CR 708.9: a face-down spell stack -> graveyard is revealed.
#[test]
fn face_down_spell_to_graveyard_revealed() {
    let mut w = World::new();
    let s = w.create(1, P1, face_down(ZoneKind::Stack));
    let m = w.mv(s, face_down(ZoneKind::Graveyard));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1, P2]));
    assert!(w.can(P2, m.new));
}

/// REQ-CORE-004, CR 708.9: a face-down spell moved from the stack into exile face down is
/// revealed (the stack to a zone other than the battlefield).
#[test]
fn face_down_spell_to_exile_revealed() {
    let mut w = World::new();
    let s = w.create(1, P2, face_down(ZoneKind::Stack));
    let m = w.mv(s, face_down(ZoneKind::Exile));
    assert!(w.can(P1, m.new) && w.can(P2, m.new));
}

/// REQ-CORE-004, CR 708.9: a face-down permanent -> library is known to all as a set member
/// with no position.
#[test]
fn face_down_permanent_to_library_known_as_set() {
    let mut w = World::new();
    w.fill_library(P1, 10, 2);
    let fd = w.create(1, P1, face_down(ZoneKind::Battlefield));
    let m = w.mv(fd, at(ZoneKind::Library, Position::Index(1)));
    for v in [P1, P2] {
        assert!(w.can(v, m.new));
        assert_eq!(w.view(v).players[&P1].library.known_cards, cards(&[1]));
        assert!(w.k.position_known(&w.z, v, P1).is_empty());
    }
}

/// REQ-CORE-004, CR 708.5, CR 708.9: a same-zone reposition of a face-down permanent is not a
/// reveal.
#[test]
fn face_down_same_zone_reposition_not_revealed() {
    let mut w = World::new();
    let fd = w.create(1, P1, face_down(ZoneKind::Battlefield));
    let m = w.mv(fd, face_down(ZoneKind::Battlefield));
    assert_eq!(m.from, m.to);
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1]));
    assert!(!w.can(P2, m.new));
}

/// REQ-CORE-004, CR 708.9: a leaving player's face-down permanents and spells are revealed;
/// another player's are not, nor face-down exiled cards.
#[test]
fn on_player_leaves_reveals_owned_face_down() {
    let mut w = World::new();
    let a = w.create(1, P1, face_down(ZoneKind::Battlefield));
    let b = w.create(2, P1, face_down(ZoneKind::Stack));
    let other = w.create(3, P2, face_down(ZoneKind::Battlefield));
    let ex = w.create(4, P1, face_down(ZoneKind::Exile));
    w.k.on_player_leaves(&w.z, P1).unwrap();
    assert!(w.can(P2, a) && w.can(P2, b));
    assert!(!w.can(P1, other));
    assert!(!w.can(P2, ex));
    assert_eq!(
        w.k.on_player_leaves(&w.z, NOBODY),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
}

/// REQ-CORE-004, CR 708.9: at the end of the game every face-down permanent and spell is
/// revealed.
#[test]
fn on_game_end_reveals_all_face_down() {
    let mut w = World::new();
    let a = w.create(1, P1, face_down(ZoneKind::Battlefield));
    let b = w.create(2, P2, face_down(ZoneKind::Stack));
    let up = w.create(3, P2, req(ZoneKind::Battlefield));
    w.k.on_game_end(&w.z);
    assert!(w.can(P2, a) && w.can(P1, b));
    assert_eq!(w.k.known_to(&w.z, a).unwrap(), set(&[P1, P2]));
    assert!(w.k.known_to(&w.z, up).unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Test 5: CR 406.3
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 406.3, CR 708.5: a face-down exiled card is hidden from everyone, including
/// its owner; a look permission is per player and persists.
#[test]
fn face_down_exile_and_grant_look() {
    let mut w = World::new();
    let ex = w.create(1, P1, face_down(ZoneKind::Exile));
    assert!(!w.can(P1, ex) && !w.can(P2, ex));
    w.k.grant_look(&w.z, ex, P1).unwrap();
    assert!(w.can(P1, ex));
    assert!(!w.can(P2, ex));
    // Other state changes do not end the permission.
    let other = w.create(2, P2, req(ZoneKind::Hand));
    w.mv(other, req(ZoneKind::Graveyard));
    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    assert!(w.can(P1, ex));
    let v = w.view(P2);
    assert_eq!(v.exile.len(), 1);
    assert_eq!(v.exile[0].card, None);
    assert_eq!(w.view(P1).exile[0].card, Some(c(1)));
}

/// REQ-CORE-004, CR 406.3: grant_look on a face-up card or a face-down card outside exile
/// fails; unknown ids fail; the state stays unchanged.
#[test]
fn grant_look_requires_face_down_exile() {
    let mut w = World::new();
    let up = w.create(1, P1, req(ZoneKind::Exile));
    let fd_bf = w.create(2, P1, face_down(ZoneKind::Battlefield));
    let hand = w.create(3, P1, req(ZoneKind::Hand));
    for id in [up, fd_bf, hand] {
        assert_eq!(
            w.k.grant_look(&w.z, id, P2),
            Err(KnowledgeError::NotFaceDown(id))
        );
    }
    let ex = w.create(4, P1, face_down(ZoneKind::Exile));
    assert_eq!(
        w.k.grant_look(&w.z, ex, NOBODY),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    let missing = ObjectId::new(500);
    assert_eq!(
        w.k.grant_look(&w.z, missing, P1),
        Err(KnowledgeError::UnknownObject(missing))
    );
    assert_eq!(w.k, Knowledge::new());
}

/// REQ-CORE-004, CR 701.24a, CR 406.3: shuffling a face-down pile ends look permissions.
#[test]
fn shuffle_face_down_pile_clears_look() {
    let mut w = World::new();
    let a = w.create(1, P1, face_down(ZoneKind::Exile));
    let b = w.create(2, P1, face_down(ZoneKind::Exile));
    let keep = w.create(3, P1, face_down(ZoneKind::Exile));
    w.k.grant_look(&w.z, a, P1).unwrap();
    w.k.grant_look(&w.z, b, P2).unwrap();
    w.k.grant_look(&w.z, keep, P2).unwrap();
    w.k.shuffle_face_down_pile(&w.z, &[a, b]).unwrap();
    assert!(!w.can(P1, a) && !w.can(P2, b));
    assert!(w.k.known_to(&w.z, a).unwrap().is_empty());
    assert!(w.can(P2, keep));
}

/// REQ-CORE-004, CR 701.24a: shuffle_face_down_pile validates every id first.
#[test]
fn shuffle_face_down_pile_errors_leave_state_unchanged() {
    let mut w = World::new();
    let a = w.create(1, P1, face_down(ZoneKind::Exile));
    let up = w.create(2, P1, req(ZoneKind::Exile));
    w.k.grant_look(&w.z, a, P1).unwrap();
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.z, &[a, up]),
        Err(KnowledgeError::NotFaceDown(up))
    );
    let missing = ObjectId::new(321);
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.z, &[a, missing]),
        Err(KnowledgeError::UnknownObject(missing))
    );
    assert_eq!(w.k, snapshot);
    // An empty pile is a no-op.
    w.k.shuffle_face_down_pile(&w.z, &[]).unwrap();
    assert_eq!(w.k, snapshot);
}

/// REQ-CORE-004, CR 406.3, CR 701.24a (F2): a face-down pile is exile-only. A face-down library
/// card or a face-down permanent is NotFaceDown, every id is checked before any change, and the
/// library card stays in the library's known cards and keeps its position entry.
#[test]
fn shuffle_face_down_pile_rejects_non_exile_face_down() {
    let mut w = World::new();
    let fd_lib = w.create(1, P1, face_down(ZoneKind::Library));
    let fd_bf = w.create(2, P1, face_down(ZoneKind::Battlefield));
    let ex = w.create(3, P1, face_down(ZoneKind::Exile));
    w.k.set_position_known(&w.z, P2, P1, 0, fd_lib).unwrap();
    w.k.add_known(&w.z, fd_bf, P2).unwrap();
    w.k.grant_look(&w.z, ex, P2).unwrap();
    let snapshot = w.k.clone();
    for bad in [fd_lib, fd_bf] {
        assert_eq!(
            w.k.shuffle_face_down_pile(&w.z, &[ex, bad]),
            Err(KnowledgeError::NotFaceDown(bad))
        );
        assert_eq!(w.k, snapshot);
    }
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[1]));
    assert_eq!(
        w.k.position_known(&w.z, P2, P1),
        BTreeMap::from([(0, fd_lib)])
    );
    // The valid exile pile behaves as before.
    w.k.shuffle_face_down_pile(&w.z, &[ex]).unwrap();
    assert!(!w.can(P2, ex));
    assert!(w.can(P2, fd_lib) && w.can(P2, fd_bf));
}

// ---------------------------------------------------------------------------------------------
// Test 6: decision 1 (KnownTo is player memory)
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 400.2: a public face-up object moved to a library is known to everyone.
#[test]
fn public_to_library_known_to_all() {
    let mut w = World::new();
    let g = w.create(1, P1, req(ZoneKind::Graveyard));
    let m = w.mv(g, req(ZoneKind::Library));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1, P2]));
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[1]));
}

/// REQ-CORE-004, CR 402.3: a hand card moved to the library is known to its owner only.
#[test]
fn hand_to_library_known_to_owner_only() {
    let mut w = World::new();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    let m = w.mv(h, req(ZoneKind::Library));
    assert!(w.can(P1, m.new));
    assert!(!w.can(P2, m.new));
    assert_eq!(w.view(P1).players[&P1].library.known_cards, cards(&[1]));
    assert!(w.view(P2).players[&P1].library.known_cards.is_empty());
}

/// REQ-CORE-004, CR 402.3: library -> hand -> library: the owner remembers; the opponent never
/// knew.
#[test]
fn library_to_hand_and_back() {
    let mut w = World::new();
    let lib = w.fill_library(P2, 10, 2);
    let m = w.mv(lib[0], req(ZoneKind::Hand));
    assert!(w.can(P2, m.new) && !w.can(P1, m.new));
    let m2 = w.mv(m.new, at(ZoneKind::Library, Position::Bottom));
    assert!(w.can(P2, m2.new) && !w.can(P1, m2.new));
}

/// REQ-CORE-004 (R12): on_move with an absent new object errors and leaves the state unchanged.
#[test]
fn on_move_unknown_new_object() {
    let mut w = World::new();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    let before: ObjectRecord = *w.z.object(h).unwrap();
    let ghost = ObjectId::new(4242);
    let moved = Moved {
        old: h,
        new: ghost,
        from: ZoneId::Hand(P1),
        to: ZoneId::Graveyard(P1),
    };
    assert_eq!(
        w.k.on_move(&w.z, &before, &moved),
        Err(KnowledgeError::UnknownObject(ghost))
    );
    assert_eq!(w.k, Knowledge::new());
}

// ---------------------------------------------------------------------------------------------
// Test 7: projection shapes
// ---------------------------------------------------------------------------------------------

/// The test's own record of `KnownTo` per card: what the test itself revealed, granted or let a
/// player see, updated by the rules of the brief, never read back from the implementation.
type Told = BTreeMap<CardId, BTreeSet<PlayerId>>;

fn tell(told: &mut Told, card: CardId, players: impl IntoIterator<Item = PlayerId>) {
    told.entry(card).or_default().extend(players);
}

fn told_set(told: &Told, card: CardId) -> BTreeSet<PlayerId> {
    told.get(&card).cloned().unwrap_or_default()
}

/// The implicit entitlements, from the zone and face-down state only: public face up
/// (CR 400.2), the viewer's own hand (CR 402.3), a face-down permanent or spell the viewer
/// controls (CR 708.5).
fn implicit(r: &ObjectRecord, viewer: PlayerId) -> bool {
    (r.zone.kind().is_public() && !r.face_down)
        || r.zone == ZoneId::Hand(viewer)
        || (r.face_down
            && matches!(r.zone, ZoneId::Battlefield | ZoneId::Stack)
            && r.controller == viewer)
}

/// Whether the test's own model entitles `viewer` to the identity of `r`.
fn entitled(told: &Told, r: &ObjectRecord, viewer: PlayerId) -> bool {
    implicit(r, viewer) || told.get(&r.card).is_some_and(|s| s.contains(&viewer))
}

/// REQ-CORE-004: walks every field of the view. No ObjectId of a hidden object; every object
/// view carries its card iff the test's model (`told` plus the zone and face-down state)
/// entitles the viewer; every hidden-zone card set is exactly the entitled cards of that zone;
/// counts match. Entitlement is never taken from the implementation under test.
fn assert_no_leak(z: &Zones, k: &Knowledge, told: &Told, viewer: PlayerId) {
    let v = k.project(z, viewer).unwrap();
    assert_eq!(v.viewer, viewer);
    assert_eq!(v.players.keys().copied().collect::<Vec<_>>(), z.players());
    let expect_cards = |zone: ZoneId| -> BTreeSet<CardId> {
        z.contents(zone)
            .into_iter()
            .map(|id| *z.object(id).unwrap())
            .filter(|r| entitled(told, r, viewer))
            .map(|r| r.card)
            .collect()
    };
    let check_objects = |ovs: &[ObjectView], zone: ZoneId| {
        let ids: Vec<ObjectId> = ovs.iter().map(|o| o.id).collect();
        assert_eq!(ids, z.contents(zone), "{zone:?} for {viewer:?}");
        for o in ovs {
            let r = z.object(o.id).expect("view names a live object");
            assert!(
                r.zone.kind().is_public() || r.zone == ZoneId::Hand(viewer),
                "hidden object id {:?} leaked to {viewer:?}",
                o.id
            );
            assert_eq!(
                o.card,
                entitled(told, r, viewer).then_some(r.card),
                "card of {:?} for {viewer:?}",
                o.id
            );
            assert_eq!(
                (o.owner, o.controller, o.face_down),
                (r.owner, r.controller, r.face_down)
            );
        }
    };
    for pl in z.players() {
        let pz = &v.players[&pl];
        assert_eq!(pz.library.count, z.len(ZoneId::Library(pl)));
        assert_eq!(
            pz.library.known_cards,
            expect_cards(ZoneId::Library(pl)),
            "library of {pl:?} for {viewer:?}"
        );
        match &pz.hand {
            HandView::Own(h) => {
                assert_eq!(pl, viewer);
                check_objects(h, ZoneId::Hand(pl));
            }
            HandView::Opponent { count, known_cards } => {
                assert_ne!(pl, viewer);
                assert_eq!(*count, z.len(ZoneId::Hand(pl)));
                assert_eq!(
                    *known_cards,
                    expect_cards(ZoneId::Hand(pl)),
                    "hand of {pl:?} for {viewer:?}"
                );
            }
        }
        check_objects(&pz.graveyard, ZoneId::Graveyard(pl));
    }
    check_objects(&v.battlefield, ZoneId::Battlefield);
    check_objects(&v.stack, ZoneId::Stack);
    check_objects(&v.exile, ZoneId::Exile);
    check_objects(&v.command, ZoneId::Command);
}

/// REQ-CORE-004, REQ-ZONE-001, CR 400.2, CR 401.3, CR 402.3: the shapes R-a..R-d.
#[test]
fn project_shapes() {
    let mut w = World::new();
    let lib1 = w.fill_library(P1, 10, 3);
    w.fill_library(P2, 20, 4);
    let h1 = w.create(1, P1, req(ZoneKind::Hand));
    let h1b = w.create(2, P1, req(ZoneKind::Hand));
    w.create(3, P2, req(ZoneKind::Hand));
    let g2 = w.create(4, P2, req(ZoneKind::Graveyard));
    let g2b = w.create(5, P2, req(ZoneKind::Graveyard));
    let bf = w.create(6, P1, req(ZoneKind::Battlefield));
    let fd = w.create(7, P2, face_down(ZoneKind::Battlefield));
    let st = w.create(8, P1, req(ZoneKind::Stack));
    let ex = w.create(9, P2, req(ZoneKind::Exile));
    let cmd = w.create(30, P1, req(ZoneKind::Command));
    w.k.set_position_known(&w.z, P1, P1, 0, lib1[0]).unwrap();

    let v = w.view(P1);
    assert_eq!(v.viewer, P1);
    assert_eq!(v.players.len(), 2);
    let own = &v.players[&P1];
    assert_eq!(own.library.count, 3);
    assert_eq!(own.library.known_cards, cards(&[10]));
    let ov = |id: ObjectId, owner: PlayerId, fd: bool, card: Option<u64>| ObjectView {
        id,
        owner,
        controller: owner,
        face_down: fd,
        card: card.map(c),
    };
    assert_eq!(
        own.hand,
        HandView::Own(vec![
            ov(h1, P1, false, Some(1)),
            ov(h1b, P1, false, Some(2))
        ])
    );
    let opp = &v.players[&P2];
    assert_eq!(opp.library.count, 4);
    assert!(opp.library.known_cards.is_empty());
    assert_eq!(
        opp.hand,
        HandView::Opponent {
            count: 1,
            known_cards: BTreeSet::new()
        }
    );
    // Graveyard top first.
    assert_eq!(
        opp.graveyard,
        vec![ov(g2b, P2, false, Some(5)), ov(g2, P2, false, Some(4))]
    );
    assert_eq!(
        v.battlefield,
        vec![ov(bf, P1, false, Some(6)), ov(fd, P2, true, None)]
    );
    assert_eq!(v.stack, vec![ov(st, P1, false, Some(8))]);
    assert_eq!(v.exile, vec![ov(ex, P2, false, Some(9))]);
    assert_eq!(v.command, vec![ov(cmd, P1, false, Some(30))]);

    let v2 = w.view(P2);
    assert_eq!(v2.battlefield[1].card, Some(c(7)));
    assert_eq!(v2.players[&P1].library.count, v.players[&P1].library.count);
    assert_eq!(v2.players[&P2].library.count, v.players[&P2].library.count);
    assert!(matches!(v2.players[&P2].hand, HandView::Own(_)));
    assert!(v2.players[&P1].library.known_cards.is_empty());

    // The test itself told P1 about card 10 (the position look); nothing else.
    let mut told = Told::new();
    tell(&mut told, c(10), [P1]);
    assert_no_leak(&w.z, &w.k, &told, P1);
    assert_no_leak(&w.z, &w.k, &told, P2);
}

/// REQ-CORE-004, REQ-ZONE-001: the controller field of a view follows the record.
#[test]
fn project_uses_record_controller() {
    let mut w = World::new();
    let bf = w.create(1, P1, req(ZoneKind::Battlefield));
    w.z.set_controller(bf, P2).unwrap();
    let v = w.view(P1);
    assert_eq!(v.battlefield[0].controller, P2);
    assert_eq!(v.battlefield[0].owner, P1);
}

// ---------------------------------------------------------------------------------------------
// Test 8: non-interference
// ---------------------------------------------------------------------------------------------

/// A world whose information hidden from P1 is parameterised: P2's hand card ids
/// (`hand_base`), P2's library card ids (`lib_base`) and order (`reverse`), and P2's own
/// knowledge (`p2_knows`).
fn hidden_world(hand_base: u64, lib_base: u64, reverse: bool, p2_knows: bool) -> World {
    let mut w = World::new();
    let mine = w.fill_library(P1, 1, 3);
    let theirs: Vec<u64> = if reverse {
        (lib_base..lib_base + 4).rev().collect()
    } else {
        (lib_base..lib_base + 4).collect()
    };
    let lib2: Vec<ObjectId> = theirs
        .iter()
        .map(|&k| w.create(k, P2, at(ZoneKind::Library, Position::Bottom)))
        .collect();
    w.create(hand_base, P2, req(ZoneKind::Hand));
    w.create(hand_base + 1, P2, req(ZoneKind::Hand));
    w.create(500, P1, req(ZoneKind::Hand));
    w.create(501, P2, face_down(ZoneKind::Battlefield));
    let ex = w.create(502, P2, face_down(ZoneKind::Exile));
    w.create(503, P1, req(ZoneKind::Graveyard));
    w.k.set_position_known(&w.z, P1, P1, 0, mine[0]).unwrap();
    if p2_knows {
        w.k.set_position_known(&w.z, P2, P2, 0, lib2[0]).unwrap();
        w.k.add_known(&w.z, mine[2], P2).unwrap();
        w.k.grant_look(&w.z, ex, P2).unwrap();
    }
    w
}

/// REQ-CORE-004 (R-e): worlds that differ only in hidden information give equal views for P1.
#[test]
fn non_interference_hidden_differences() {
    let base = hidden_world(100, 200, false, false);
    let v = base.view(P1);
    for (hb, lb, rev, kn) in [
        (300, 200, false, false),
        (100, 400, false, false),
        (100, 200, true, false),
        (100, 200, false, true),
        (300, 400, true, true),
    ] {
        let other = hidden_world(hb, lb, rev, kn);
        assert_eq!(other.view(P1), v, "case {hb} {lb} {rev} {kn}");
    }
    // The opponent's views differ, so the worlds really differ.
    assert_ne!(hidden_world(300, 200, false, false).view(P2), base.view(P2));
}

/// REQ-CORE-004 (R-e): projection is deterministic and does not mutate.
#[test]
fn projection_deterministic_and_pure() {
    let w = hidden_world(100, 200, false, true);
    let (z0, k0) = (w.z.clone(), w.k.clone());
    let a = w.view(P1);
    let b = w.view(P1);
    assert_eq!(a, b);
    assert_eq!(w.z, z0);
    assert_eq!(w.k, k0);
    assert_eq!(encode_to_vec(&w.k), encode_to_vec(&k0));
}

// ---------------------------------------------------------------------------------------------
// Test 9: error paths
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004 (R12): every KnowledgeError variant, unknown players and objects, an index out
/// of range, a hand card given as a library card.
#[test]
fn error_paths() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 2);
    let h = w.create(1, P1, req(ZoneKind::Hand));
    let missing = ObjectId::new(999);
    let unknown = Err(KnowledgeError::UnknownObject(missing));
    let nobody = Err(KnowledgeError::UnknownPlayer(NOBODY));

    assert_eq!(
        w.k.known_to(&w.z, missing),
        unknown.clone().map(|()| BTreeSet::new())
    );
    assert_eq!(w.k.add_known(&w.z, missing, P1), unknown);
    assert_eq!(w.k.add_known(&w.z, h, NOBODY), nobody);
    assert_eq!(w.k.reveal_to_all(&w.z, missing), unknown);
    assert_eq!(
        w.k.can_identify(&w.z, P1, missing),
        Err(KnowledgeError::UnknownObject(missing))
    );
    assert_eq!(
        w.k.can_identify(&w.z, NOBODY, h),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.k.project(&w.z, NOBODY),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );

    // set_position_known
    assert_eq!(w.k.set_position_known(&w.z, NOBODY, P1, 0, lib[0]), nobody);
    assert_eq!(w.k.set_position_known(&w.z, P1, NOBODY, 0, lib[0]), nobody);
    assert_eq!(w.k.set_position_known(&w.z, P1, P1, 0, missing), unknown);
    assert_eq!(
        w.k.set_position_known(&w.z, P1, P1, 0, h),
        Err(KnowledgeError::NotInLibrary {
            owner: P1,
            object: h
        })
    );
    assert_eq!(
        w.k.set_position_known(&w.z, P1, P2, 0, lib[0]),
        Err(KnowledgeError::NotInLibrary {
            owner: P2,
            object: lib[0]
        })
    );
    assert_eq!(
        w.k.set_position_known(&w.z, P1, P1, 1, lib[0]),
        Err(KnowledgeError::IndexMismatch {
            owner: P1,
            index: 1,
            object: lib[0]
        })
    );
    assert_eq!(
        w.k.set_position_known(&w.z, P1, P1, usize::MAX, lib[1]),
        Err(KnowledgeError::IndexMismatch {
            owner: P1,
            index: usize::MAX,
            object: lib[1]
        })
    );
    assert_eq!(w.k, Knowledge::new());
}

/// REQ-CORE-004: Display and std::error::Error for every variant.
#[test]
fn error_display() {
    let o = ObjectId::new(7);
    let errs = [
        KnowledgeError::UnknownObject(o),
        KnowledgeError::UnknownPlayer(p(3)),
        KnowledgeError::NotInLibrary {
            owner: p(3),
            object: o,
        },
        KnowledgeError::IndexMismatch {
            owner: p(3),
            index: 4,
            object: o,
        },
        KnowledgeError::NotFaceDown(o),
    ];
    let texts: Vec<String> = errs.iter().map(ToString::to_string).collect();
    assert_eq!(texts[0], "unknown object 7");
    assert_eq!(texts[1], "unknown player 3");
    assert!(texts[2].contains("not in the library of player 3"));
    assert!(texts[3].contains("index 4"));
    assert!(texts[4].contains("object 7"));
    let e: &dyn std::error::Error = &errs[0];
    assert!(e.source().is_none());
}

// ---------------------------------------------------------------------------------------------
// Test 10: canonical encoding and snapshots
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, REQ-NFR-006: encoding is deterministic, appends, distinguishes states;
/// clones are independent.
#[test]
fn canonical_encoding_and_clone_independence() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 2);
    let mut buf = vec![0xAB, 0xCD];
    Knowledge::new().encode_canonical(&mut buf);
    assert_eq!(&buf[..2], &[0xAB, 0xCD]);
    assert_eq!(buf.len(), 2 + 8);

    w.k.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    let a = encode_to_vec(&w.k);
    assert_eq!(a, encode_to_vec(&w.k.clone()));

    let mut clone = w.k.clone();
    clone.add_known(&w.z, lib[0], P2).unwrap();
    assert_ne!(clone, w.k);
    assert_ne!(encode_to_vec(&clone), a);
    assert_eq!(encode_to_vec(&w.k), a);
    assert!(!w.k.known_to(&w.z, lib[0]).unwrap().contains(&P2));

    // The same knowledge reached in a different order gives equal bytes.
    let mut x = Knowledge::new();
    x.add_known(&w.z, lib[0], P2).unwrap();
    x.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    let mut y = Knowledge::new();
    y.set_position_known(&w.z, P1, P1, 1, lib[1]).unwrap();
    y.add_known(&w.z, lib[0], P2).unwrap();
    assert_eq!(x, y);
    assert_eq!(encode_to_vec(&x), encode_to_vec(&y));
    assert_eq!(encode_to_vec(&x), encode_to_vec(&clone));

    // Same KnownTo, no position knowledge: different bytes.
    let mut z = Knowledge::new();
    z.add_known(&w.z, lib[1], P1).unwrap();
    z.add_known(&w.z, lib[0], P2).unwrap();
    assert_ne!(encode_to_vec(&z), encode_to_vec(&x));
}

// ---------------------------------------------------------------------------------------------
// Test 11: property loop
// ---------------------------------------------------------------------------------------------

/// A 64-bit LCG (R3, R7: seeded, no clock).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
        if xs.is_empty() {
            None
        } else {
            Some(xs[self.below(xs.len())])
        }
    }
}

/// Every stored position entry (not only what position_known reports) matches the live
/// library: the callers below honour the on_move / on_library_changed contract.
fn assert_positions_live(z: &Zones, k: &Knowledge) {
    for (&(_, owner), entries) in k.positions.iter() {
        let live = z.contents(ZoneId::Library(owner));
        for (&i, id) in entries {
            assert_eq!(live.get(i), Some(id), "stale entry {i} of {owner:?}");
        }
    }
    for v in z.players() {
        for o in z.players() {
            let live = z.contents(ZoneId::Library(o));
            for (i, id) in k.position_known(z, v, o) {
                assert_eq!(live.get(i), Some(&id));
            }
        }
    }
}

/// The brief's on_move_observed rules applied to the test's own model `told`; `z` is the
/// container after the move. Knowing the moved card's position is not observation: only
/// `observers` and the implicit rules on the new record and the source hand count.
fn told_move(
    told: &mut Told,
    z: &Zones,
    before: &ObjectRecord,
    moved: &Moved,
    observers: &[PlayerId],
) {
    let new = *z.object(moved.new).unwrap();
    let players = z.players();
    // Step 0: a player who did not observe a move out of a hidden zone forgets the candidates.
    if matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand) && moved.from != moved.to {
        let mut candidates: Vec<CardId> = z
            .contents(moved.from)
            .into_iter()
            .map(|id| z.object(id).unwrap().card)
            .collect();
        candidates.push(new.card);
        for &v in &players {
            let observed =
                observers.contains(&v) || implicit(&new, v) || before.zone == ZoneId::Hand(v);
            if !observed {
                for card in &candidates {
                    if let Some(s) = told.get_mut(card) {
                        s.remove(&v);
                    }
                }
            }
        }
    }
    // (a) CR 400.2.
    if before.zone.kind().is_public() && !before.face_down {
        tell(told, new.card, players.iter().copied());
    }
    // (b) CR 402.3.
    if let ZoneId::Hand(owner) = before.zone {
        tell(told, new.card, [owner]);
    }
    // (c) CR 708.5, CR 708.9.
    if before.face_down && matches!(before.zone, ZoneId::Battlefield | ZoneId::Stack) {
        tell(told, new.card, [before.controller]);
        if moved.from != moved.to
            && (moved.from == ZoneId::Battlefield
                || (moved.from == ZoneId::Stack && moved.to != ZoneId::Battlefield))
        {
            tell(told, new.card, players.iter().copied());
        }
    }
}

/// Moves `id` in the property loop: applies on_move_indexed and the model. Position entries of
/// an affected library survive only for players in `index_known_by` (round 4 rule).
#[allow(clippy::too_many_arguments)]
fn prop_move(
    z: &mut Zones,
    k: &mut Knowledge,
    ids: &mut IdGenerator<ObjectId>,
    told: &mut Told,
    id: ObjectId,
    r: MoveRequest,
    observers: &[PlayerId],
    index_known_by: &[PlayerId],
) {
    let before = *z.object(id).unwrap();
    let moved = z.move_object(ids, id, r).unwrap();
    k.on_move_indexed(z, &before, &moved, observers, index_known_by)
        .unwrap();
    told_move(told, z, &before, &moved, observers);
    for zone in [moved.from, moved.to] {
        if let ZoneId::Library(owner) = zone {
            assert_only_index_known(k, owner, index_known_by);
        }
    }
}

/// No player outside `index_known_by` holds a stored position entry for `owner`'s library.
fn assert_only_index_known(k: &Knowledge, owner: PlayerId, index_known_by: &[PlayerId]) {
    for &(v, o) in k.positions.keys() {
        assert!(
            o != owner || index_known_by.contains(&v),
            "{v:?} kept {o:?}"
        );
    }
}

/// REQ-CORE-004 (R12, R-e), CR 400.2, CR 402.3, CR 406.3, CR 708.5, CR 708.9, CR 701.24a,
/// CR 701.24b: random creates, moves (observed by random players or not), reveals, looks and
/// shuffles never make a view leak against the test's own model of what each player was told,
/// the stored KnownTo equals that model, and position knowledge always matches the live library.
#[test]
fn property_random_sequences_never_leak() {
    let players = [P1, P2, p(3)];
    for seed in 0..200u64 {
        let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xDEAD_BEEF);
        let mut z = Zones::new(&players, ZonesConfig::default());
        let mut k = Knowledge::new();
        let mut ids: IdGenerator<ObjectId> = IdGenerator::new();
        let mut told = Told::new();
        let mut next_card = 1u64;
        for _step in 0..60 {
            let objects = z.all_object_ids();
            match rng.below(9) {
                0 | 1 => {
                    let owner = players[rng.below(3)];
                    let kind = ZoneKind::ALL[rng.below(ZoneKind::ALL.len())];
                    let r = MoveRequest {
                        to: kind,
                        position: Position::Index(rng.below(4)),
                        face_down: rng.below(3) == 0,
                        controller: None,
                    };
                    z.create_object(&mut ids, c(next_card), owner, r).unwrap();
                    next_card += 1;
                    if kind == ZoneKind::Library {
                        // The caller contract for insertions outside on_move.
                        let index_known_by: Vec<PlayerId> = players
                            .iter()
                            .copied()
                            .filter(|_| rng.below(2) == 0)
                            .collect();
                        k.on_library_changed_indexed(&z, owner, &index_known_by)
                            .unwrap();
                        assert_only_index_known(&k, owner, &index_known_by);
                    }
                }
                2 | 3 => {
                    if let Some(id) = rng.pick(&objects) {
                        let kind = ZoneKind::ALL[rng.below(ZoneKind::ALL.len())];
                        let controller = if matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack)
                            && rng.below(2) == 0
                        {
                            Some(players[rng.below(3)])
                        } else {
                            None
                        };
                        let r = MoveRequest {
                            to: kind,
                            position: Position::Index(rng.below(5)),
                            face_down: rng.below(3) == 0,
                            controller,
                        };
                        let observers: Vec<PlayerId> = players
                            .iter()
                            .copied()
                            .filter(|_| rng.below(3) == 0)
                            .collect();
                        let index_known_by: Vec<PlayerId> = players
                            .iter()
                            .copied()
                            .filter(|_| rng.below(2) == 0)
                            .collect();
                        prop_move(
                            &mut z,
                            &mut k,
                            &mut ids,
                            &mut told,
                            id,
                            r,
                            &observers,
                            &index_known_by,
                        );
                    }
                }
                4 => {
                    if let Some(id) = rng.pick(&objects) {
                        let card = z.object(id).unwrap().card;
                        if rng.below(2) == 0 {
                            k.reveal_to_all(&z, id).unwrap();
                            tell(&mut told, card, players);
                        } else {
                            let pl = players[rng.below(3)];
                            k.add_known(&z, id, pl).unwrap();
                            tell(&mut told, card, [pl]);
                        }
                    }
                }
                5 => {
                    if let Some(id) = rng.pick(&objects) {
                        let pl = players[rng.below(3)];
                        let r = k.grant_look(&z, id, pl);
                        let rec = *z.object(id).unwrap();
                        assert_eq!(r.is_ok(), rec.face_down && rec.zone == ZoneId::Exile);
                        if r.is_ok() {
                            tell(&mut told, rec.card, [pl]);
                        }
                    }
                }
                6 => {
                    let owner = players[rng.below(3)];
                    let lib = z.contents(ZoneId::Library(owner));
                    if !lib.is_empty() {
                        let i = rng.below(lib.len());
                        let viewer = players[rng.below(3)];
                        k.set_position_known(&z, viewer, owner, i, lib[i]).unwrap();
                        tell(&mut told, z.object(lib[i]).unwrap().card, [viewer]);
                        // A wrong index is rejected.
                        let wrong = (i + 1) % (lib.len() + 1);
                        assert!(
                            k.set_position_known(&z, viewer, owner, wrong, lib[i])
                                .is_err()
                        );
                    }
                }
                7 => {
                    // A shuffle: reposition the non-excluded cards at random, then clear positions.
                    let owner = players[rng.below(3)];
                    let lib = z.contents(ZoneId::Library(owner));
                    let excluded: Vec<ObjectId> =
                        lib.iter().copied().filter(|_| rng.below(4) == 0).collect();
                    for &id in &lib {
                        if excluded.contains(&id) {
                            continue;
                        }
                        let r = at(ZoneKind::Library, Position::Index(rng.below(lib.len())));
                        prop_move(&mut z, &mut k, &mut ids, &mut told, id, r, &[], &players);
                    }
                    let known_by: Vec<PlayerId> = players
                        .iter()
                        .copied()
                        .filter(|_| rng.below(2) == 0)
                        .collect();
                    k.shuffle_library_disclosed(&z, owner, &excluded, &known_by)
                        .unwrap();
                    for v in players {
                        for (_, id) in k.position_known(&z, v, owner) {
                            assert!(excluded.contains(&id));
                        }
                    }
                    assert_only_index_known(&k, owner, &known_by);
                }
                _ => {
                    // CR 406.3: only face-down exiled cards form a pile; any other face-down
                    // object is rejected without a change.
                    let in_pile = |id: &ObjectId| {
                        z.object(*id)
                            .is_some_and(|r| r.face_down && r.zone == ZoneId::Exile)
                    };
                    let pile: Vec<ObjectId> = objects
                        .iter()
                        .copied()
                        .filter(in_pile)
                        .filter(|_| rng.below(2) == 0)
                        .collect();
                    let outsider = objects
                        .iter()
                        .copied()
                        .find(|id| z.object(*id).is_some_and(|r| r.face_down) && !in_pile(id));
                    if let Some(bad) = outsider {
                        let snapshot = k.clone();
                        let mut with_bad = pile.clone();
                        with_bad.push(bad);
                        assert_eq!(
                            k.shuffle_face_down_pile(&z, &with_bad),
                            Err(KnowledgeError::NotFaceDown(bad))
                        );
                        assert_eq!(k, snapshot);
                    }
                    k.shuffle_face_down_pile(&z, &pile).unwrap();
                    for id in pile {
                        assert!(k.known_to(&z, id).unwrap().is_empty());
                        told.remove(&z.object(id).unwrap().card);
                    }
                }
            }
            for id in z.all_object_ids() {
                let card = z.object(id).unwrap().card;
                assert_eq!(k.known_to(&z, id).unwrap(), told_set(&told, card));
            }
            for v in players {
                assert_no_leak(&z, &k, &told, v);
            }
            assert_positions_live(&z, &k);
        }
        // CR 708.9: game end reveals every face-down permanent and spell.
        k.on_game_end(&z);
        for zone in [ZoneId::Battlefield, ZoneId::Stack] {
            for id in z.contents(zone) {
                for v in players {
                    assert!(k.can_identify(&z, v, id).unwrap());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Test 12: unobserved moves out of a hidden zone (step 0 of on_move_observed)
// ---------------------------------------------------------------------------------------------

/// Cards 1 (A) and 2 (B) are known to P2 as members of P1's library; 3 (X) and 4 are unknown.
/// World `a_on_top` has library A, B, X, 4 (top first); the twin has X, B, A, 4. Nothing else
/// differs. Returns the world and the top object.
fn draw_twin(a_on_top: bool) -> (World, ObjectId) {
    let mut w = World::new();
    let order: [u64; 4] = if a_on_top { [1, 2, 3, 4] } else { [3, 2, 1, 4] };
    let lib: Vec<ObjectId> = order
        .iter()
        .map(|&k| w.create(k, P1, at(ZoneKind::Library, Position::Bottom)))
        .collect();
    for card in [1, 2] {
        let id = w.obj(card);
        w.k.add_known(&w.z, id, P2).unwrap();
    }
    (w, lib[0])
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2, CR 402.3 (F1 twin worlds): an unrevealed draw of A
/// and an unrevealed draw of the unknown X give P2 equal views and equal position knowledge.
/// Controls: P1's own view differs; an observed draw and a reveal both make P2's views differ.
#[test]
fn twin_worlds_unobserved_draw_give_equal_views() {
    let (mut w1, top1) = draw_twin(true);
    let (mut w2, top2) = draw_twin(false);
    // Indistinguishable for P2 before the draw.
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(w1.view(P2).players[&P1].library.known_cards, cards(&[1, 2]));

    let (mut o1, mut o2) = (w1.clone(), w2.clone());
    let m1 = w1.mv(top1, req(ZoneKind::Hand));
    let m2 = w2.mv(top2, req(ZoneKind::Hand));
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    // Control: the worlds really differ (P1's own hand shows A vs X).
    assert_ne!(w1.view(P1), w2.view(P1));
    assert_eq!(
        w1.view(P1).players[&P1].hand,
        HandView::Own(vec![ObjectView {
            id: m1.new,
            owner: P1,
            controller: P1,
            face_down: false,
            card: Some(c(1)),
        }])
    );
    // Control: a reveal of the drawn card distinguishes the worlds for P2.
    let (mut r1, mut r2) = (w1.clone(), w2.clone());
    r1.k.reveal_to_all(&r1.z, m1.new).unwrap();
    r2.k.reveal_to_all(&r2.z, m2.new).unwrap();
    assert_ne!(r1.view(P2), r2.view(P2));

    // Control: P2 watches the draw, so P2 knows which card moved.
    let n1 = o1.mv_obs(top1, req(ZoneKind::Hand), &[P2]);
    o2.mv_obs(top2, req(ZoneKind::Hand), &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
    assert!(o1.can(P2, n1.new));
    assert_eq!(
        o1.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 1,
            known_cards: cards(&[1])
        }
    );
    assert_eq!(o1.view(P2).players[&P1].library.known_cards, cards(&[2]));
    assert_eq!(o2.view(P2).players[&P1].library.known_cards, cards(&[1, 2]));
}

/// P1 holds A (card 1) and B (card 2), both known to P2; the library holds 3 and 4. P1 puts A
/// (`put_a`) or B on top of the library. Returns the world before the move and the moved object.
fn put_back_twin(put_a: bool) -> (World, ObjectId) {
    let mut w = World::new();
    w.fill_library(P1, 3, 2);
    let a = w.create(1, P1, req(ZoneKind::Hand));
    let b = w.create(2, P1, req(ZoneKind::Hand));
    w.k.add_known(&w.z, a, P2).unwrap();
    w.k.add_known(&w.z, b, P2).unwrap();
    (w, if put_a { a } else { b })
}

/// REQ-CORE-004, REQ-TEST-001, CR 402.3 (F1 twin worlds): P1 puts one of two hand cards P2
/// knows into its library unobserved; P2's views are equal whichever card it was. Control:
/// with P2 observing, the views differ.
#[test]
fn twin_worlds_unobserved_hand_to_library_give_equal_views() {
    let (mut w1, a) = put_back_twin(true);
    let (mut w2, b) = put_back_twin(false);
    assert_eq!(w1.view(P2), w2.view(P2));
    let (mut o1, mut o2) = (w1.clone(), w2.clone());
    w1.mv(a, at(ZoneKind::Library, Position::Top));
    w2.mv(b, at(ZoneKind::Library, Position::Top));
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert_ne!(w1.view(P1), w2.view(P1));

    o1.mv_obs(a, at(ZoneKind::Library, Position::Top), &[P2]);
    o2.mv_obs(b, at(ZoneKind::Library, Position::Top), &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
    assert_eq!(o1.view(P2).players[&P1].library.known_cards, cards(&[1]));
    assert_eq!(
        o1.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 1,
            known_cards: cards(&[2])
        }
    );
}

/// REQ-CORE-004, CR 401.2: a player who did not observe a move out of a library forgets every
/// card of that library and the moved card, and loses every position entry for it, including
/// entries the move did not shift (the bottom card leaves, the top entry stays live).
#[test]
fn step0_unobserved_forgets_candidates_and_positions() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 4);
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    w.k.add_known(&w.z, lib[3], P2).unwrap();
    // P1 knows its own top card; P1 sees the moved card arrive in its hand and knows the
    // index it left (P1 is in index_known_by; P2 is in neither set).
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    let mut shorthand = w.clone();
    let m = w.mv_idx(lib[3], req(ZoneKind::Hand), &[], &[P1]);
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
    assert!(!w.can(P2, m.new));
    assert!(w.view(P2).players[&P1].library.known_cards.is_empty());
    for id in [lib[0], lib[1]] {
        assert!(!w.k.known_to(&w.z, id).unwrap().contains(&P2));
    }
    // P1 observed (the card is in P1's hand): P1's knowledge is untouched.
    assert_eq!(
        w.k.position_known(&w.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );
    assert_eq!(w.k.known_to(&w.z, lib[0]).unwrap(), set(&[P1]));
    // The stored map holds no empty set.
    assert!(w.k.known.values().all(|s| !s.is_empty()));
    assert!(!w.k.known.contains_key(&c(2)));

    // Contrast: the shorthand (index undisclosed) also drops P1's entry; identity is the same.
    shorthand.mv(lib[3], req(ZoneKind::Hand));
    assert!(shorthand.k.positions.is_empty());
    assert_eq!(shorthand.k.known, w.k.known);
}

/// REQ-CORE-004 step 0 (i): a player in `observers` keeps every piece of knowledge.
#[test]
fn step0_observer_keeps_knowledge() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.add_known(&w.z, lib[0], P2).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    w.k.set_position_known(&w.z, P2, P1, 2, lib[2]).unwrap();
    let m = w.mv_obs(lib[0], req(ZoneKind::Hand), &[P2]);
    assert!(w.can(P2, m.new));
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[2, 3]));
    // The pruning of (d) still applies: lib[2] shifted to index 1.
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
}

/// REQ-CORE-004, CR 400.2, step 0 (ii): a move from a library or a hand to a public zone face
/// up is seen by everyone; P2 keeps its set knowledge of the source zone.
#[test]
fn step0_public_face_up_destination_is_observed() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    let m = w.mv(lib[0], req(ZoneKind::Graveyard));
    assert!(w.can(P2, m.new));
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[2]));
    let m = w.mv(lib[2], req(ZoneKind::Battlefield));
    assert!(w.can(P2, m.new));
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[2]));

    let h1 = w.create(10, P1, req(ZoneKind::Hand));
    let h2 = w.create(11, P1, req(ZoneKind::Hand));
    w.k.add_known(&w.z, h2, P2).unwrap();
    w.mv(h1, req(ZoneKind::Battlefield));
    assert!(w.can(P2, h2));
    // A face-down destination is not public face up: P2 forgets; the owner (iii, b) keeps it.
    let m = w.mv(h2, face_down(ZoneKind::Exile));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1]));
    assert!(!w.can(P2, m.new));
}

/// REQ-CORE-004, CR 402.3, step 0 (iii): the hand owner chose the card and keeps its stored
/// knowledge of the other hand cards; the opponent forgets them.
#[test]
fn step0_hand_owner_observes() {
    let mut w = World::new();
    w.fill_library(P1, 1, 2);
    let h1 = w.create(10, P1, req(ZoneKind::Hand));
    let h2 = w.create(11, P1, req(ZoneKind::Hand));
    w.k.add_known(&w.z, h2, P1).unwrap();
    w.k.add_known(&w.z, h2, P2).unwrap();
    let m = w.mv(h1, req(ZoneKind::Library));
    assert_eq!(w.k.known_to(&w.z, h2).unwrap(), set(&[P1]));
    assert_eq!(w.k.known_to(&w.z, m.new).unwrap(), set(&[P1]));
}

/// REQ-CORE-004, CR 401.2, CR 402.3: knowing the moved card's position is not observation. P2
/// knows A is on top; an unobserved draw of A makes P2 forget A and every library card, and
/// P2's position entries for that library are gone. The same draw with P2 passed as an
/// observer (the selected position is public) keeps A identifiable to P2.
#[test]
fn step0_position_knowledge_is_not_observation() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P2, P1, 2, lib[2]).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    let mut o = w.clone();

    let m = w.mv(lib[0], req(ZoneKind::Hand));
    assert!(!w.can(P2, m.new));
    assert!(!w.k.known_to(&w.z, m.new).unwrap().contains(&P2));
    for &id in &lib[1..] {
        assert!(!w.k.known_to(&w.z, id).unwrap().contains(&P2));
    }
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
    assert!(!w.k.positions.contains_key(&(P2, P1)));
    assert_eq!(
        w.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 1,
            known_cards: BTreeSet::new()
        }
    );
    assert!(w.view(P2).players[&P1].library.known_cards.is_empty());

    // Control: the caller passes P2 as an observer of the public top-card draw.
    let n = o.mv_obs(lib[0], req(ZoneKind::Hand), &[P2]);
    assert!(o.can(P2, n.new));
    assert_eq!(
        o.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 1,
            known_cards: cards(&[1])
        }
    );
    assert_eq!(o.view(P2).players[&P1].library.known_cards, cards(&[2, 3]));
}

/// P1's library is A (card 1), B (card 2), X (card 3), card 4 (top first). P2 knows A's
/// position (index 0) and B as a set member; X and card 4 are unknown to P2.
fn selection_twin() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 4);
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    (w, lib[0], lib[2])
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2, CR 402.3 (CodeRabbit round 2, thread 4625001805): an
/// undisclosed selection of a library card. P2 knows A's position, but P1 secretly selects a
/// card: in world 1 A, in world 2 X, by the same request. P2's views and position knowledge
/// are equal in both worlds. Controls: P1's views differ; with P2 passed as an observer in both
/// worlds, P2's views differ.
#[test]
fn twin_worlds_undisclosed_selection_give_equal_views() {
    let (mut w1, a, _) = selection_twin();
    let (mut w2, _, x) = selection_twin();
    assert_eq!(w1.view(P2), w2.view(P2));
    let (mut o1, mut o2) = (w1.clone(), w2.clone());

    w1.mv(a, req(ZoneKind::Hand));
    w2.mv(x, req(ZoneKind::Hand));
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
    assert!(w2.k.position_known(&w2.z, P2, P1).is_empty());
    // Control: the worlds really differ (P1's own hand shows A vs X).
    assert_ne!(w1.view(P1), w2.view(P1));

    // Control: P2 watches the selection in both worlds, so P2 sees which card was taken.
    o1.mv_obs(a, req(ZoneKind::Hand), &[P2]);
    o2.mv_obs(x, req(ZoneKind::Hand), &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
}

/// REQ-CORE-004: a same-zone reposition (from == to) skips step 0; a hand reposition too.
#[test]
fn step0_skipped_for_same_zone_reposition() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.add_known(&w.z, lib[0], P2).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    let m = w.mv(lib[0], at(ZoneKind::Library, Position::Index(2)));
    assert_eq!(m.from, m.to);
    assert_eq!(w.view(P2).players[&P1].library.known_cards, cards(&[1, 2]));
    let h = w.create(10, P1, req(ZoneKind::Hand));
    w.k.add_known(&w.z, h, P2).unwrap();
    let m = w.mv(h, req(ZoneKind::Hand));
    assert_eq!(m.from, m.to);
    assert!(w.can(P2, m.new));
}

/// REQ-CORE-004 (R12): on_move_observed reports UnknownObject before UnknownPlayer and leaves
/// the state unchanged; on_move equals on_move_observed with no observers.
#[test]
fn on_move_observed_errors_and_equivalence() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 2);
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    let before = *w.z.object(lib[0]).unwrap();
    let moved =
        w.z.move_object(&mut w.ids, lib[0], req(ZoneKind::Hand))
            .unwrap();
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.on_move_observed(&w.z, &before, &moved, &[P2, NOBODY]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.k, snapshot);
    let ghost = Moved {
        new: ObjectId::new(4242),
        ..moved
    };
    assert_eq!(
        w.k.on_move_observed(&w.z, &before, &ghost, &[NOBODY]),
        Err(KnowledgeError::UnknownObject(ObjectId::new(4242)))
    );
    assert_eq!(w.k, snapshot);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.on_move(&w.z, &before, &moved).unwrap();
    b.on_move_observed(&w.z, &before, &moved, &[]).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, snapshot);
}

// ---------------------------------------------------------------------------------------------
// Test 13: on_library_changed (F4)
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 401.2: the viewer knows A at index 0; X is created on top; after
/// on_library_changed, drawing X does not bring the stale entry for A back. Without the call
/// the coincidence would restore it (control).
#[test]
fn on_library_changed_prevents_coincidental_revival() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    let x = w.create(9, P1, at(ZoneKind::Library, Position::Top));
    let mut control = w.clone();
    let mut shorthand = w.clone();
    // P1 knows both indexes (the insertion and the draw of the top card).
    w.k.on_library_changed_indexed(&w.z, P1, &[P1]).unwrap();
    w.mv_idx(x, req(ZoneKind::Hand), &[], &[P1]);
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    // Set knowledge of A is kept.
    assert!(w.can(P1, lib[0]));

    control.mv_idx(x, req(ZoneKind::Hand), &[], &[P1]);
    assert_eq!(
        control.k.position_known(&control.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );

    // Contrast: the shorthand drops every entry at once; set knowledge stays.
    shorthand.k.on_library_changed(&shorthand.z, P1).unwrap();
    assert!(shorthand.k.positions.is_empty());
    assert!(shorthand.can(P1, lib[0]));
}

/// REQ-CORE-004: for a viewer who knows the insertion index n, entries below n are kept and
/// entries at or after n are dropped; another library is untouched; an unknown owner is an
/// error with the state unchanged. Contrast: the shorthand drops every entry of that library.
#[test]
fn on_library_changed_keeps_entries_above_insertion() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 4);
    let other = w.fill_library(P2, 20, 2);
    for (i, &id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.z, P2, P1, i, id).unwrap();
    }
    w.k.set_position_known(&w.z, P1, P2, 1, other[1]).unwrap();
    w.create(9, P1, at(ZoneKind::Library, Position::Index(2)));
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.on_library_changed(&w.z, NOBODY),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.k, snapshot);
    let mut shorthand = w.clone();
    w.k.on_library_changed_indexed(&w.z, P1, &[P2]).unwrap();
    assert_eq!(
        w.k.positions[&(P2, P1)],
        BTreeMap::from([(0, lib[0]), (1, lib[1])])
    );
    shorthand.k.on_library_changed(&shorthand.z, P1).unwrap();
    assert!(!shorthand.k.positions.contains_key(&(P2, P1)));
    assert_eq!(
        shorthand.k.position_known(&shorthand.z, P1, P2),
        BTreeMap::from([(1, other[1])])
    );
    assert_eq!(shorthand.k.known, w.k.known);
    assert_eq!(
        w.k.position_known(&w.z, P1, P2),
        BTreeMap::from([(1, other[1])])
    );
    // Unchanged library, index known to the viewer: a no-op.
    let snapshot = w.k.clone();
    w.k.on_library_changed_indexed(&w.z, P2, &[P1]).unwrap();
    assert_eq!(w.k, snapshot);
    // The shorthand drops P1's entries for that library.
    w.k.on_library_changed(&w.z, P2).unwrap();
    assert!(!w.k.positions.contains_key(&(P1, P2)));
    assert_eq!(w.k.known, snapshot.known);
}

// ---------------------------------------------------------------------------------------------
// Test 14: on_removed_from_hidden_zone (PR #174 round 3, threads 4625229511 and 4625229503)
// ---------------------------------------------------------------------------------------------

impl World {
    /// Removes `id` from the game with `Zones::send_outside` and applies
    /// `on_removed_from_hidden_zone` with `observers`; returns the record before the removal.
    fn remove(&mut self, id: ObjectId, observers: &[PlayerId]) -> ObjectRecord {
        let before = *self.z.object(id).unwrap();
        self.z.send_outside(id, OutsidePool::Sideboard).unwrap();
        self.k
            .on_removed_from_hidden_zone(&self.z, &before, observers)
            .unwrap();
        before
    }

    /// Like [`World::remove`] with `on_removed_from_hidden_zone_indexed`.
    fn remove_idx(
        &mut self,
        id: ObjectId,
        observers: &[PlayerId],
        index_known_by: &[PlayerId],
    ) -> ObjectRecord {
        let before = *self.z.object(id).unwrap();
        self.z.send_outside(id, OutsidePool::Sideboard).unwrap();
        self.k
            .on_removed_from_hidden_zone_indexed(&self.z, &before, observers, index_known_by)
            .unwrap();
        before
    }
}

/// P1's library is A (card 1), B (card 2), X (card 3), card 4 (top first). P2 knows the SET
/// {A, B} (no positions); X and card 4 are unknown to P2. Returns the world and the library.
fn removal_twin() -> (World, Vec<ObjectId>) {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 4);
    w.k.add_known(&w.z, lib[0], P2).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    (w, lib)
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2 (thread 4625229511): an unobserved removal of A and an
/// unobserved removal of the unknown X from P1's library give P2 equal views and equal position
/// knowledge; P2's library knowledge is empty in both. Control: with P2 observing, P2 keeps {B}
/// vs {A, B} (views differ). Control: `on_library_changed` alone is position-only and leaves
/// P2's grant on B.
#[test]
fn twin_worlds_unobserved_removal_give_equal_views() {
    let (mut w1, lib) = removal_twin();
    let (mut w2, _) = removal_twin();
    assert_eq!(w1.view(P2), w2.view(P2));
    let (mut o1, mut o2) = (w1.clone(), w2.clone());
    let mut l1 = w1.clone();

    w1.remove(lib[0], &[]);
    w2.remove(lib[2], &[]);
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert!(w1.view(P2).players[&P1].library.known_cards.is_empty());
    assert!(w2.view(P2).players[&P1].library.known_cards.is_empty());
    // Control: the worlds really differ (P1's library holds B, X, 4 vs A, B, 4).
    assert_ne!(w1.z.object_of_card(c(1)), w2.z.object_of_card(c(1)));

    // Control: P2 watched which card was removed.
    o1.remove(lib[0], &[P2]);
    o2.remove(lib[2], &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
    assert_eq!(o1.view(P2).players[&P1].library.known_cards, cards(&[2]));
    assert_eq!(o2.view(P2).players[&P1].library.known_cards, cards(&[1, 2]));

    // Control: on_library_changed alone only prunes positions; the grant on B stays.
    l1.z.send_outside(lib[0], OutsidePool::Sideboard).unwrap();
    l1.k.on_library_changed(&l1.z, P1).unwrap();
    assert!(l1.k.known_to(&l1.z, lib[1]).unwrap().contains(&P2));
    assert_eq!(l1.view(P2).players[&P1].library.known_cards, cards(&[2]));
}

/// P1's hand holds cards 10 and 11 (known to P2 and stored for P1) and card 12 (unknown to P2);
/// P1's library holds card 1.
fn hand_removal_world() -> (World, [ObjectId; 3]) {
    let mut w = World::new();
    w.fill_library(P1, 1, 1);
    let h = [10, 11, 12].map(|k| w.create(k, P1, req(ZoneKind::Hand)));
    for &id in &h[..2] {
        w.k.add_known(&w.z, id, P2).unwrap();
        w.k.add_known(&w.z, id, P1).unwrap();
    }
    (w, h)
}

/// REQ-CORE-004, REQ-TEST-001, CR 402.3 (thread 4625229511): removing a card from P1's hand
/// unobserved makes P2 forget both cards it knew, whichever card left (twin views equal); the
/// hand owner P1 observed it (iii) and keeps its stored knowledge; with P2 as an observer, P2
/// keeps its knowledge.
#[test]
fn removal_from_hand_forgets_for_non_observers() {
    let (mut w1, h) = hand_removal_world();
    let (mut w2, _) = hand_removal_world();
    let (mut o1, mut o2) = (w1.clone(), w2.clone());
    w1.remove(h[0], &[]);
    w2.remove(h[2], &[]);
    assert_eq!(w1.view(P2), w2.view(P2));
    let empty = HandView::Opponent {
        count: 2,
        known_cards: BTreeSet::new(),
    };
    assert_eq!(w1.view(P2).players[&P1].hand, empty);
    assert_eq!(w1.k.known_to(&w1.z, h[1]).unwrap(), set(&[P1]));
    assert_eq!(w2.k.known_to(&w2.z, h[0]).unwrap(), set(&[P1]));
    assert_eq!(w2.k.known_to(&w2.z, h[1]).unwrap(), set(&[P1]));
    // The removed card's stored KnownTo lost P2 too; P1 keeps it.
    assert_eq!(w1.k.known_set(c(10)), set(&[P1]));
    assert_ne!(w1.view(P1), w2.view(P1));

    o1.remove(h[0], &[P2]);
    o2.remove(h[2], &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
    assert_eq!(o1.k.known_to(&o1.z, h[1]).unwrap(), set(&[P1, P2]));
    assert_eq!(o1.k.known_set(c(10)), set(&[P1, P2]));
    assert_eq!(
        o2.view(P2).players[&P1].hand,
        HandView::Opponent {
            count: 2,
            known_cards: cards(&[10, 11])
        }
    );
}

/// REQ-CORE-004, CR 401.2 (threads 4625229511, 4625492354): an unobserved removal drops every
/// position entry of P2 for that library (other libraries untouched); an observed removal with
/// the index known to P2 keeps the entries above the removal index and drops the removed card's
/// entry and the shifted ones; an observed removal without the index drops every entry but
/// keeps the set knowledge.
#[test]
fn removal_from_library_position_entries() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 5);
    let other = w.fill_library(P2, 20, 2);
    for (i, &id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.z, P2, P1, i, id).unwrap();
    }
    w.k.set_position_known(&w.z, P2, P2, 0, other[0]).unwrap();
    let mut o = w.clone();
    let mut s = w.clone();

    w.remove(lib[2], &[]);
    assert!(w.k.position_known(&w.z, P2, P1).is_empty());
    assert!(!w.k.positions.contains_key(&(P2, P1)));
    assert!(w.view(P2).players[&P1].library.known_cards.is_empty());
    assert_eq!(
        w.k.position_known(&w.z, P2, P2),
        BTreeMap::from([(0, other[0])])
    );

    o.remove_idx(lib[2], &[P2], &[P2]);
    assert_eq!(
        o.k.positions[&(P2, P1)],
        BTreeMap::from([(0, lib[0]), (1, lib[1])])
    );
    assert_eq!(
        o.view(P2).players[&P1].library.known_cards,
        cards(&[1, 2, 4, 5])
    );
    // The removal of the bottom card with its index known keeps every remaining entry.
    o.remove_idx(lib[4], &[P2], &[P2]);
    assert_eq!(
        o.k.positions[&(P2, P1)],
        BTreeMap::from([(0, lib[0]), (1, lib[1])])
    );

    // Contrast: observed (P2 knows which card left) but the index undisclosed.
    s.remove(lib[2], &[P2]);
    assert!(!s.k.positions.contains_key(&(P2, P1)));
    assert_eq!(
        s.view(P2).players[&P1].library.known_cards,
        cards(&[1, 2, 4, 5])
    );
    assert_eq!(
        s.k.position_known(&s.z, P2, P2),
        BTreeMap::from([(0, other[0])])
    );
}

/// REQ-CORE-004 (R12): a public-zone source is a no-op; an unknown source owner and an unknown
/// observer are UnknownPlayer with the state unchanged; the owner is checked first.
#[test]
fn removal_hook_noops_and_errors() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 2);
    w.k.add_known(&w.z, lib[0], P2).unwrap();
    w.k.set_position_known(&w.z, P2, P1, 1, lib[1]).unwrap();
    let bf = w.create(10, P1, req(ZoneKind::Battlefield));
    let gy = w.create(11, P1, req(ZoneKind::Graveyard));
    for id in [bf, gy] {
        let snapshot = w.k.clone();
        w.remove(id, &[]);
        assert_eq!(w.k, snapshot);
    }
    let snapshot = w.k.clone();
    let before = *w.z.object(lib[0]).unwrap();
    let other = p(98);
    for zone in [ZoneId::Library(NOBODY), ZoneId::Hand(NOBODY)] {
        let ghost = ObjectRecord { zone, ..before };
        assert_eq!(
            w.k.on_removed_from_hidden_zone(&w.z, &ghost, &[other]),
            Err(KnowledgeError::UnknownPlayer(NOBODY))
        );
        assert_eq!(w.k, snapshot);
    }
    assert_eq!(
        w.k.on_removed_from_hidden_zone(&w.z, &before, &[P2, other]),
        Err(KnowledgeError::UnknownPlayer(other))
    );
    assert_eq!(w.k, snapshot);
    // The hook does not depend on whether the object still exists.
    w.k.on_removed_from_hidden_zone(&w.z, &before, &[]).unwrap();
    assert!(!w.k.known_to(&w.z, lib[0]).unwrap().contains(&P2));
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2, CR 406.3 (thread 4625229503, fixed in round 2 by
/// dropping step-0 condition (iv)): P2 knows A's position and B as a set member; P1 secretly
/// exiles A (world 1) or X (world 2) face down. P2's views are equal and P2 cannot identify
/// the exiled object in either world. Control: with P2 observing, P2 identifies A in world 1.
#[test]
fn twin_worlds_undisclosed_selection_to_face_down_exile() {
    let (mut w1, a, _) = selection_twin();
    let (mut w2, _, x) = selection_twin();
    let (mut o1, mut o2) = (w1.clone(), w2.clone());
    let m1 = w1.mv(a, face_down(ZoneKind::Exile));
    let m2 = w2.mv(x, face_down(ZoneKind::Exile));
    assert_eq!(w1.view(P2), w2.view(P2));
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert!(!w1.can(P2, m1.new));
    assert!(!w2.can(P2, m2.new));
    assert_ne!(
        w1.z.object(m1.new).unwrap().card,
        w2.z.object(m2.new).unwrap().card
    );

    let n1 = o1.mv_obs(a, face_down(ZoneKind::Exile), &[P2]);
    let n2 = o2.mv_obs(x, face_down(ZoneKind::Exile), &[P2]);
    assert!(o1.can(P2, n1.new));
    assert!(!o2.can(P2, n2.new));
    assert_ne!(o1.view(P2), o2.view(P2));
}

// ---------------------------------------------------------------------------------------------
// Test 15: position knowledge survives a library change only for viewers who know the index
// (PR #174 round 4, threads 4625492354 and 4625492361)
// ---------------------------------------------------------------------------------------------

/// P1's library in the given card order (top first). P2 knows the position of card 1 (A) and
/// the set membership of card 2 (B). Returns the world and the object of card 3 (X).
fn index_twin(order: &[u64]) -> (World, ObjectId) {
    let mut w = World::new();
    for &k in order {
        w.create(k, P1, at(ZoneKind::Library, Position::Bottom));
    }
    let a = w.obj(1);
    let a_index = order.iter().position(|&k| k == 1).unwrap();
    w.k.set_position_known(&w.z, P2, P1, a_index, a).unwrap();
    let b = w.obj(2);
    w.k.add_known(&w.z, b, P2).unwrap();
    let x = w.obj(3);
    (w, x)
}

/// X is above A (index 0) in world 1 and below A (index 2) in world 2; A is at index 1 in both.
const ABOVE: [u64; 4] = [3, 1, 2, 4];
const BELOW: [u64; 4] = [4, 1, 3, 2];

/// REQ-CORE-004, REQ-TEST-001, CR 401.2 (F1, thread 4625492354): P2 is told which card left
/// (X) but not where it was. Removing X from above A and from below A gives P2 equal position
/// knowledge (empty) and equal views. Control: with the index disclosed to P2, A's entry goes
/// when X was above it and stays when X was below; P1's own view is not changed by the hook.
#[test]
fn twin_worlds_removal_index_undisclosed() {
    let (mut w1, x1) = index_twin(&ABOVE);
    let (mut w2, x2) = index_twin(&BELOW);
    assert_eq!(w1.view(P2), w2.view(P2));
    let (mut c1, mut c2) = (w1.clone(), w2.clone());

    w1.remove(x1, &[P2]);
    w2.remove(x2, &[P2]);
    assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert_eq!(w1.view(P2), w2.view(P2));
    // P2 was an observer: its set knowledge {A, B} stays.
    assert_eq!(w1.view(P2).players[&P1].library.known_cards, cards(&[1, 2]));

    // Control: P2 knows the removal index.
    let a = c1.obj(1);
    c1.remove_idx(x1, &[P2], &[P2]);
    c2.remove_idx(x2, &[P2], &[P2]);
    assert!(c1.k.position_known(&c1.z, P2, P1).is_empty());
    assert_eq!(c2.k.position_known(&c2.z, P2, P1), BTreeMap::from([(1, a)]));

    // P1's own view does not depend on the hook.
    let (mut h, x) = index_twin(&ABOVE);
    let record = *h.z.object(x).unwrap();
    h.z.send_outside(x, OutsidePool::Sideboard).unwrap();
    let before_hook = h.view(P1);
    h.k.on_removed_from_hidden_zone(&h.z, &record, &[P2])
        .unwrap();
    assert_eq!(h.view(P1), before_hook);
}

/// P1's library is B (card 2), A (card 1), card 5; P2 knows A at index 1 and the set {B}.
fn insertion_twin() -> World {
    let (w, _) = index_twin(&[2, 1, 3]);
    w
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2 (F2, thread 4625492361): a card inserted at index 0
/// (above A) or at index 2 (below A) with the index undisclosed leaves P2 no position entries
/// in both worlds, equal views, and P2's set knowledge of A and B intact. Control: with the
/// index disclosed to P2, the insertion at 0 drops A@1 and the insertion at 2 keeps it. Also
/// `enter_from_outside` once.
#[test]
fn twin_worlds_insertion_index_undisclosed() {
    let mut worlds = [insertion_twin(), insertion_twin()];
    let mut controls = worlds.clone();
    for (w, i) in worlds.iter_mut().zip([0, 2]) {
        w.create(9, P1, at(ZoneKind::Library, Position::Index(i)));
        w.k.on_library_changed(&w.z, P1).unwrap();
        assert!(w.k.position_known(&w.z, P2, P1).is_empty());
        for card in [1, 2] {
            assert!(w.k.known_to(&w.z, w.obj(card)).unwrap().contains(&P2));
        }
    }
    assert_eq!(worlds[0].view(P2), worlds[1].view(P2));

    for (w, i) in controls.iter_mut().zip([0, 2]) {
        w.create(9, P1, at(ZoneKind::Library, Position::Index(i)));
        w.k.on_library_changed_indexed(&w.z, P1, &[P2]).unwrap();
    }
    assert!(
        controls[0]
            .k
            .position_known(&controls[0].z, P2, P1)
            .is_empty()
    );
    let a = controls[1].obj(1);
    assert_eq!(
        controls[1].k.position_known(&controls[1].z, P2, P1),
        BTreeMap::from([(1, a)])
    );

    // enter_from_outside below A, index undisclosed: P2 keeps no entry, set knowledge stays.
    let mut e = insertion_twin();
    e.z.add_outside_card(P1, OutsidePool::Sideboard, c(9))
        .unwrap();
    e.z.enter_from_outside(
        &mut e.ids,
        P1,
        c(9),
        OutsidePool::Sideboard,
        at(ZoneKind::Library, Position::Bottom),
    )
    .unwrap();
    let mut ec = e.clone();
    e.k.on_library_changed(&e.z, P1).unwrap();
    assert!(e.k.position_known(&e.z, P2, P1).is_empty());
    assert!(e.can(P2, e.obj(1)));
    ec.k.on_library_changed_indexed(&ec.z, P1, &[P2]).unwrap();
    let a = ec.obj(1);
    assert_eq!(ec.k.position_known(&ec.z, P2, P1), BTreeMap::from([(1, a)]));
}

/// Runs `r` on X in both twin worlds (X above A, X below A) with `on_move_indexed`.
fn move_twin(r: MoveRequest, observers: &[PlayerId], index_known_by: &[PlayerId]) -> [World; 2] {
    [ABOVE, BELOW].map(|order| {
        let (mut w, x) = index_twin(&order);
        w.mv_idx(x, r, observers, index_known_by);
        w
    })
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2, CR 400.2, CR 406.3 (class instance (a)): a move of X
/// out of P1's library from above or below A, index undisclosed, leaves P2 equal (empty)
/// position knowledge: to P1's hand (P2 observing), to the graveyard and to face-down exile.
/// Control: with P2 observing and the index disclosed, a move from above drops A@1 and one
/// from below keeps it.
#[test]
fn twin_worlds_move_out_index_undisclosed() {
    let dests = [
        (req(ZoneKind::Hand), vec![P2]),
        (req(ZoneKind::Graveyard), vec![]),
        (face_down(ZoneKind::Exile), vec![]),
    ];
    for (r, observers) in &dests {
        let [w1, w2] = move_twin(*r, observers, &[]);
        assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
        assert!(w2.k.position_known(&w2.z, P2, P1).is_empty());
        assert_eq!(w1.view(P2), w2.view(P2));
        // The shorthand gives the same result.
        let (mut s, x) = index_twin(&ABOVE);
        let before = *s.z.object(x).unwrap();
        let moved = s.z.move_object(&mut s.ids, x, *r).unwrap();
        s.k.on_move_observed(&s.z, &before, &moved, observers)
            .unwrap();
        assert_eq!(s.k, w1.k);

        let [c1, c2] = move_twin(*r, &[P2], &[P2]);
        assert!(c1.k.position_known(&c1.z, P2, P1).is_empty());
        let a = c2.obj(1);
        assert_eq!(c2.k.position_known(&c2.z, P2, P1), BTreeMap::from([(1, a)]));
    }
    // Knowing the index does not make P2 an observer: an unobserved secret move to face-down
    // exile drops P2's entries at step 0 even with P2 in index_known_by.
    let [_, u] = move_twin(face_down(ZoneKind::Exile), &[], &[P2]);
    assert!(u.k.position_known(&u.z, P2, P1).is_empty());
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2 (class instance (b)): a card moves into P1's library
/// at index 0 (above A) or index 2 (below A), from P1's hand and once from the graveyard.
/// Shorthand: P2 has no position entry in both worlds. Control with the index disclosed to P2:
/// the insertion at 0 drops A@1, the one at 2 keeps it.
#[test]
fn twin_worlds_move_in_index_undisclosed() {
    for source in [ZoneKind::Hand, ZoneKind::Graveyard] {
        let run = |i: usize, index_known_by: &[PlayerId]| {
            let mut w = insertion_twin();
            let h = w.create(7, P1, req(source));
            w.mv_idx(
                h,
                at(ZoneKind::Library, Position::Index(i)),
                &[],
                index_known_by,
            );
            w
        };
        let (w1, w2) = (run(0, &[]), run(2, &[]));
        assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
        assert!(w2.k.position_known(&w2.z, P2, P1).is_empty());
        assert_eq!(
            w1.view(P2).players[&P1].library,
            w2.view(P2).players[&P1].library
        );
        let (c1, c2) = (run(0, &[P2]), run(2, &[P2]));
        assert!(c1.k.position_known(&c1.z, P2, P1).is_empty());
        let a = c2.obj(1);
        assert_eq!(c2.k.position_known(&c2.z, P2, P1), BTreeMap::from([(1, a)]));
    }
}

/// REQ-CORE-004, REQ-TEST-001, CR 401.2 (class instance (c)): P1's library is X, B, card 5, A,
/// card 4 (top first); P2 knows A at index 3. X moves from index 0 to the bottom (A shifts to
/// 2) or to index 2 (A stays at 3). Shorthand: P2 has no entry in either world. Control with
/// the index disclosed: the move to the bottom drops A@3, the move to index 2 keeps it.
#[test]
fn twin_worlds_reposition_index_undisclosed() {
    let run = |position: Position, index_known_by: &[PlayerId]| {
        let (mut w, x) = index_twin(&[3, 2, 5, 1, 4]);
        let m = w.mv_idx(x, at(ZoneKind::Library, position), &[], index_known_by);
        assert_eq!(m.from, m.to);
        w
    };
    let (w1, w2) = (run(Position::Bottom, &[]), run(Position::Index(2), &[]));
    assert_eq!(w2.z.contents(ZoneId::Library(P1))[3], w2.obj(1));
    assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
    assert!(w2.k.position_known(&w2.z, P2, P1).is_empty());
    assert_eq!(w1.view(P2), w2.view(P2));
    let (c1, c2) = (run(Position::Bottom, &[P2]), run(Position::Index(2), &[P2]));
    assert!(c1.k.position_known(&c1.z, P2, P1).is_empty());
    let a = c2.obj(1);
    assert_eq!(c2.k.position_known(&c2.z, P2, P1), BTreeMap::from([(3, a)]));
}

/// REQ-CORE-004, REQ-TEST-001, CR 701.24a, CR 701.24b (class instance (f)): P2 knows A at
/// index 0 and the set {B}. A shuffle that excludes A and one that excludes X give P2 equal
/// (empty) position knowledge. Control: `shuffle_library_disclosed` keeps A@0 for a viewer in
/// `known_by` only; `shuffle_library` never touches KnownTo.
#[test]
fn twin_worlds_shuffle_excluded_undisclosed() {
    let (mut w1, _) = index_twin(&[1, 2, 3, 4]);
    let (mut w2, x) = index_twin(&[1, 2, 3, 4]);
    let a = w1.obj(1);
    let known = w1.k.known.clone();
    let mut c = w1.clone();
    w1.k.shuffle_library(&w1.z, P1, &[a]).unwrap();
    w2.k.shuffle_library(&w2.z, P1, &[x]).unwrap();
    assert!(w1.k.position_known(&w1.z, P2, P1).is_empty());
    assert_eq!(
        w1.k.position_known(&w1.z, P2, P1),
        w2.k.position_known(&w2.z, P2, P1)
    );
    assert_eq!(w1.k, w2.k);
    assert_eq!(w1.k.known, known);

    let mut none = c.clone();
    let mut other = c.clone();
    c.k.shuffle_library_disclosed(&c.z, P1, &[a], &[P2])
        .unwrap();
    assert_eq!(c.k.position_known(&c.z, P2, P1), BTreeMap::from([(0, a)]));
    none.k
        .shuffle_library_disclosed(&none.z, P1, &[a], &[])
        .unwrap();
    assert!(none.k.positions.is_empty());
    // A viewer not in known_by is cleared.
    other
        .k
        .shuffle_library_disclosed(&other.z, P1, &[a], &[P1])
        .unwrap();
    assert!(other.k.position_known(&other.z, P2, P1).is_empty());
    assert_eq!(other.k.known, known);
}

/// REQ-CORE-004 (R12): the four disclosure methods report unknown players in the documented
/// order and leave the state unchanged; each shorthand equals its disclosure form with an
/// empty set.
#[test]
fn index_disclosure_errors_and_shorthands() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 1, 3);
    w.k.set_position_known(&w.z, P2, P1, 2, lib[2]).unwrap();
    w.k.add_known(&w.z, lib[1], P2).unwrap();
    let (q, r) = (p(97), p(98));
    let snapshot = w.k.clone();
    let check = |k: &Knowledge| assert_eq!(*k, snapshot);

    // on_move_indexed: UnknownObject, then observers, then index_known_by.
    let before = *w.z.object(lib[0]).unwrap();
    let moved =
        w.z.move_object(&mut w.ids, lib[0], req(ZoneKind::Hand))
            .unwrap();
    let ghost = Moved {
        new: ObjectId::new(4242),
        ..moved
    };
    assert_eq!(
        w.k.on_move_indexed(&w.z, &before, &ghost, &[q], &[r]),
        Err(KnowledgeError::UnknownObject(ObjectId::new(4242)))
    );
    assert_eq!(
        w.k.on_move_indexed(&w.z, &before, &moved, &[P2, q], &[r]),
        Err(KnowledgeError::UnknownPlayer(q))
    );
    assert_eq!(
        w.k.on_move_indexed(&w.z, &before, &moved, &[P2], &[P1, r]),
        Err(KnowledgeError::UnknownPlayer(r))
    );
    check(&w.k);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.on_move(&w.z, &before, &moved).unwrap();
    b.on_move_indexed(&w.z, &before, &moved, &[], &[]).unwrap();
    assert_eq!(a, b);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.on_move_observed(&w.z, &before, &moved, &[P2]).unwrap();
    b.on_move_indexed(&w.z, &before, &moved, &[P2], &[])
        .unwrap();
    assert_eq!(a, b);

    // on_removed_from_hidden_zone_indexed: zone owner, then observers, then index_known_by.
    let before = *w.z.object(lib[1]).unwrap();
    w.z.send_outside(lib[1], OutsidePool::Sideboard).unwrap();
    let ghost = ObjectRecord {
        zone: ZoneId::Library(NOBODY),
        ..before
    };
    assert_eq!(
        w.k.on_removed_from_hidden_zone_indexed(&w.z, &ghost, &[q], &[r]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.k.on_removed_from_hidden_zone_indexed(&w.z, &before, &[q], &[r]),
        Err(KnowledgeError::UnknownPlayer(q))
    );
    assert_eq!(
        w.k.on_removed_from_hidden_zone_indexed(&w.z, &before, &[P2], &[r]),
        Err(KnowledgeError::UnknownPlayer(r))
    );
    check(&w.k);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.on_removed_from_hidden_zone(&w.z, &before, &[P2]).unwrap();
    b.on_removed_from_hidden_zone_indexed(&w.z, &before, &[P2], &[])
        .unwrap();
    assert_eq!(a, b);

    // on_library_changed_indexed: owner, then index_known_by.
    assert_eq!(
        w.k.on_library_changed_indexed(&w.z, NOBODY, &[r]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.k.on_library_changed_indexed(&w.z, P1, &[P2, r]),
        Err(KnowledgeError::UnknownPlayer(r))
    );
    check(&w.k);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.on_library_changed(&w.z, P1).unwrap();
    b.on_library_changed_indexed(&w.z, P1, &[]).unwrap();
    assert_eq!(a, b);

    // shuffle_library_disclosed: owner, then excluded, then known_by.
    let bad = ObjectId::new(4242);
    assert_eq!(
        w.k.shuffle_library_disclosed(&w.z, NOBODY, &[bad], &[r]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.k.shuffle_library_disclosed(&w.z, P1, &[bad], &[r]),
        Err(KnowledgeError::NotInLibrary {
            owner: P1,
            object: bad
        })
    );
    assert_eq!(
        w.k.shuffle_library_disclosed(&w.z, P1, &[lib[2]], &[P2, r]),
        Err(KnowledgeError::UnknownPlayer(r))
    );
    check(&w.k);
    let mut a = w.k.clone();
    let mut b = w.k.clone();
    a.shuffle_library(&w.z, P1, &[lib[2]]).unwrap();
    b.shuffle_library_disclosed(&w.z, P1, &[lib[2]], &[])
        .unwrap();
    assert_eq!(a, b);
}
