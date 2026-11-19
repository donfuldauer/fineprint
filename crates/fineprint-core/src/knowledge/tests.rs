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
    MoveRequest, Moved, ObjectRecord, Position, ZoneId, ZoneKind, Zones, ZonesConfig,
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

/// REQ-CORE-004, CR 701.20a, CR 701.24a: a revealed card shuffled into the library stays known
/// as a set member to everyone; no one knows its position.
#[test]
fn reveal_then_shuffle_in() {
    let mut w = World::new();
    w.fill_library(P1, 10, 3);
    let h = w.create(1, P1, req(ZoneKind::Hand));
    w.k.reveal_to_all(&w.z, h).unwrap();
    let m = w.mv(h, req(ZoneKind::Library));
    w.k.shuffle_library(&w.z, P1, &[]).unwrap();
    for v in [P1, P2] {
        assert!(w.can(v, m.new));
        assert_eq!(w.view(v).players[&P1].library.known_cards, cards(&[1]));
        assert!(w.k.position_known(&w.z, v, P1).is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Test 3: CR 701.24b
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-004, CR 701.24b: an excluded found card keeps its entry while still at its index;
/// non-excluded entries are cleared.
#[test]
fn shuffle_excluded_card_keeps_position_while_in_place() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 4);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    w.k.set_position_known(&w.z, P1, P1, 2, lib[2]).unwrap();
    w.k.set_position_known(&w.z, P2, P1, 0, lib[0]).unwrap();
    w.k.shuffle_library(&w.z, P1, &[lib[0]]).unwrap();
    assert_eq!(
        w.k.position_known(&w.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );
    assert_eq!(
        w.k.position_known(&w.z, P2, P1),
        BTreeMap::from([(0, lib[0])])
    );
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

/// REQ-CORE-004: a card put on the bottom leaves an entry above it untouched.
#[test]
fn on_move_keeps_entries_that_still_match() {
    let mut w = World::new();
    let lib = w.fill_library(P1, 10, 3);
    w.k.set_position_known(&w.z, P1, P1, 0, lib[0]).unwrap();
    let h = w.create(1, P1, req(ZoneKind::Hand));
    w.mv(h, at(ZoneKind::Library, Position::Bottom));
    assert_eq!(
        w.k.position_known(&w.z, P1, P1),
        BTreeMap::from([(0, lib[0])])
    );
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

/// REQ-CORE-004, CR 701.24a: a face-down pile shuffle also drops position entries naming the
/// shuffled objects (a face-down library card).
#[test]
fn shuffle_face_down_pile_drops_positions() {
    let mut w = World::new();
    let fd = w.create(1, P1, face_down(ZoneKind::Library));
    w.k.set_position_known(&w.z, P1, P1, 0, fd).unwrap();
    w.k.shuffle_face_down_pile(&w.z, &[fd]).unwrap();
    assert!(w.k.position_known(&w.z, P1, P1).is_empty());
    assert_eq!(w.k, Knowledge::new());
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

/// Every ObjectId and CardId in a view (walks every field).
fn collect_ids(v: &ZonesView) -> (BTreeSet<ObjectId>, BTreeSet<CardId>) {
    let mut objects = BTreeSet::new();
    let mut card_ids = BTreeSet::new();
    let mut take = |ovs: &[ObjectView]| {
        for o in ovs {
            objects.insert(o.id);
            if let Some(card) = o.card {
                card_ids.insert(card);
            }
        }
    };
    for pz in v.players.values() {
        take(&pz.graveyard);
        if let HandView::Own(h) = &pz.hand {
            take(h);
        }
    }
    for zone in [&v.battlefield, &v.stack, &v.exile, &v.command] {
        take(zone);
    }
    for pz in v.players.values() {
        card_ids.extend(pz.library.known_cards.iter().copied());
        if let HandView::Opponent { known_cards, .. } = &pz.hand {
            card_ids.extend(known_cards.iter().copied());
        }
    }
    (objects, card_ids)
}

/// REQ-CORE-004: no CardId the viewer cannot identify and no ObjectId of a hidden object the
/// viewer may not see; every identifiable card is present; counts match.
fn assert_no_leak(z: &Zones, k: &Knowledge, viewer: PlayerId) {
    let v = k.project(z, viewer).unwrap();
    assert_eq!(v.viewer, viewer);
    assert_eq!(v.players.keys().copied().collect::<Vec<_>>(), z.players());
    let (objects, card_ids) = collect_ids(&v);
    for id in &objects {
        let r = z.object(*id).expect("view names a live object");
        let allowed = r.zone.kind().is_public() || r.zone == ZoneId::Hand(viewer);
        assert!(allowed, "hidden object id {id:?} leaked to {viewer:?}");
    }
    for card in &card_ids {
        let id = z.object_of_card(*card).expect("view names a live card");
        assert!(
            k.can_identify(z, viewer, id).unwrap(),
            "card {card:?} leaked to {viewer:?}"
        );
    }
    for id in z.all_object_ids() {
        let r = z.object(id).unwrap();
        if k.can_identify(z, viewer, id).unwrap() {
            assert!(card_ids.contains(&r.card));
        }
    }
    for pl in z.players() {
        let pz = &v.players[&pl];
        assert_eq!(pz.library.count, z.len(ZoneId::Library(pl)));
        match &pz.hand {
            HandView::Own(h) => {
                assert_eq!(pl, viewer);
                assert_eq!(h.len(), z.len(ZoneId::Hand(pl)));
                assert!(h.iter().all(|o| o.card.is_some()));
            }
            HandView::Opponent { count, .. } => {
                assert_ne!(pl, viewer);
                assert_eq!(*count, z.len(ZoneId::Hand(pl)));
            }
        }
    }
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

    assert_no_leak(&w.z, &w.k, P1);
    assert_no_leak(&w.z, &w.k, P2);
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

/// Every entry position_known reports matches the live library.
fn assert_positions_live(z: &Zones, k: &Knowledge) {
    for v in z.players() {
        for o in z.players() {
            let live = z.contents(ZoneId::Library(o));
            for (i, id) in k.position_known(z, v, o) {
                assert_eq!(live.get(i), Some(&id));
            }
        }
    }
}

/// REQ-CORE-004 (R12, R-e), CR 400.2, CR 402.3, CR 708.5, CR 708.9, CR 701.24a, CR 701.24b:
/// random creates, moves, reveals, looks and shuffles never make a view leak, and position
/// knowledge always matches the live library.
#[test]
fn property_random_sequences_never_leak() {
    let players = [P1, P2, p(3)];
    for seed in 0..200u64 {
        let mut rng = Lcg(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xDEAD_BEEF);
        let mut z = Zones::new(&players, ZonesConfig::default());
        let mut k = Knowledge::new();
        let mut ids: IdGenerator<ObjectId> = IdGenerator::new();
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
                        let before = *z.object(id).unwrap();
                        let moved = z.move_object(&mut ids, id, r).unwrap();
                        k.on_move(&z, &before, &moved).unwrap();
                    }
                }
                4 => {
                    if let Some(id) = rng.pick(&objects) {
                        if rng.below(2) == 0 {
                            k.reveal_to_all(&z, id).unwrap();
                        } else {
                            k.add_known(&z, id, players[rng.below(3)]).unwrap();
                        }
                    }
                }
                5 => {
                    if let Some(id) = rng.pick(&objects) {
                        let r = k.grant_look(&z, id, players[rng.below(3)]);
                        let rec = z.object(id).unwrap();
                        assert_eq!(r.is_ok(), rec.face_down && rec.zone == ZoneId::Exile);
                    }
                }
                6 => {
                    let owner = players[rng.below(3)];
                    let lib = z.contents(ZoneId::Library(owner));
                    if !lib.is_empty() {
                        let i = rng.below(lib.len());
                        let viewer = players[rng.below(3)];
                        k.set_position_known(&z, viewer, owner, i, lib[i]).unwrap();
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
                        let before = *z.object(id).unwrap();
                        let r = at(ZoneKind::Library, Position::Index(rng.below(lib.len())));
                        let moved = z.move_object(&mut ids, id, r).unwrap();
                        k.on_move(&z, &before, &moved).unwrap();
                    }
                    k.shuffle_library(&z, owner, &excluded).unwrap();
                    for v in players {
                        for (_, id) in k.position_known(&z, v, owner) {
                            assert!(excluded.contains(&id));
                        }
                    }
                }
                _ => {
                    let pile: Vec<ObjectId> = objects
                        .iter()
                        .copied()
                        .filter(|id| z.object(*id).is_some_and(|r| r.face_down))
                        .filter(|_| rng.below(2) == 0)
                        .collect();
                    k.shuffle_face_down_pile(&z, &pile).unwrap();
                    for id in pile {
                        assert!(k.known_to(&z, id).unwrap().is_empty());
                    }
                }
            }
            for v in players {
                assert_no_leak(&z, &k, v);
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
