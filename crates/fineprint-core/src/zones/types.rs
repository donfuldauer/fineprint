// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Zone kinds, zone identifiers, move requests and object records (REQ-ZONE-001,
//! REQ-CORE-002).

use fineprint_base::ids::{CardId, ObjectId, PlayerId};

/// The kind of a zone (REQ-ZONE-001, CR 400.1). Outside the game is not a zone (CR 400.11) and
/// has no kind; see [`OutsidePool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneKind {
    /// A player's library (CR 401): ordered, hidden, per player.
    Library,
    /// A player's hand (CR 402): unordered, hidden, per player.
    Hand,
    /// The battlefield (CR 403): unordered, public, shared.
    Battlefield,
    /// A player's graveyard (CR 404): ordered, public, per player.
    Graveyard,
    /// The stack (CR 405): ordered, public, shared.
    Stack,
    /// The exile zone (CR 406): unordered, public, shared.
    Exile,
    /// The command zone (CR 408): unordered, public, shared.
    Command,
}

impl ZoneKind {
    /// Every zone kind, in declaration order.
    pub const ALL: [ZoneKind; 7] = [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Battlefield,
        ZoneKind::Graveyard,
        ZoneKind::Stack,
        ZoneKind::Exile,
        ZoneKind::Command,
    ];

    /// True for the zones each player has their own of: library, hand and graveyard
    /// (REQ-ZONE-001, CR 400.1).
    #[must_use]
    pub fn is_per_player(self) -> bool {
        // CR 400.1
        matches!(
            self,
            ZoneKind::Library | ZoneKind::Hand | ZoneKind::Graveyard
        )
    }

    /// True for public zones; false for the hidden library and hand (REQ-ZONE-001, CR 400.2).
    #[must_use]
    pub fn is_public(self) -> bool {
        // CR 400.2
        !matches!(self, ZoneKind::Library | ZoneKind::Hand)
    }

    /// True for zones whose order matters and can't be changed except when effects or rules
    /// allow it: library, graveyard and stack (REQ-ZONE-001, CR 400.5).
    #[must_use]
    pub fn is_ordered(self) -> bool {
        // CR 400.5
        matches!(
            self,
            ZoneKind::Library | ZoneKind::Graveyard | ZoneKind::Stack
        )
    }

    /// The canonical encoding tag of this kind.
    pub(crate) fn tag(self) -> u8 {
        match self {
            ZoneKind::Library => 0,
            ZoneKind::Hand => 1,
            ZoneKind::Battlefield => 2,
            ZoneKind::Graveyard => 3,
            ZoneKind::Stack => 4,
            ZoneKind::Exile => 5,
            ZoneKind::Command => 6,
        }
    }
}

/// One specific zone: a per-player zone together with its player, or a shared zone
/// (REQ-ZONE-001, CR 400.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneId {
    /// The library of the player.
    Library(PlayerId),
    /// The hand of the player.
    Hand(PlayerId),
    /// The graveyard of the player.
    Graveyard(PlayerId),
    /// The shared battlefield.
    Battlefield,
    /// The shared stack.
    Stack,
    /// The shared exile zone.
    Exile,
    /// The shared command zone.
    Command,
}

impl ZoneId {
    /// The kind of this zone.
    #[must_use]
    pub fn kind(self) -> ZoneKind {
        match self {
            ZoneId::Library(_) => ZoneKind::Library,
            ZoneId::Hand(_) => ZoneKind::Hand,
            ZoneId::Graveyard(_) => ZoneKind::Graveyard,
            ZoneId::Battlefield => ZoneKind::Battlefield,
            ZoneId::Stack => ZoneKind::Stack,
            ZoneId::Exile => ZoneKind::Exile,
            ZoneId::Command => ZoneKind::Command,
        }
    }

    /// The player whose zone this is, for per-player zones; `None` for shared zones.
    #[must_use]
    pub fn owner(self) -> Option<PlayerId> {
        match self {
            ZoneId::Library(p) | ZoneId::Hand(p) | ZoneId::Graveyard(p) => Some(p),
            ZoneId::Battlefield | ZoneId::Stack | ZoneId::Exile | ZoneId::Command => None,
        }
    }

    /// The zone of kind `kind`: `owner`'s zone for per-player kinds; the shared zone (ignoring
    /// `owner`) otherwise (REQ-ZONE-001, CR 400.1).
    #[must_use]
    pub fn of(kind: ZoneKind, owner: PlayerId) -> ZoneId {
        match kind {
            ZoneKind::Library => ZoneId::Library(owner),
            ZoneKind::Hand => ZoneId::Hand(owner),
            ZoneKind::Graveyard => ZoneId::Graveyard(owner),
            ZoneKind::Battlefield => ZoneId::Battlefield,
            ZoneKind::Stack => ZoneId::Stack,
            ZoneKind::Exile => ZoneId::Exile,
            ZoneKind::Command => ZoneId::Command,
        }
    }
}

/// Where an object goes in an ordered zone (REQ-ZONE-001, CR 400.5). Ignored for unordered
/// zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    /// On top (index 0).
    Top,
    /// On the bottom (the last index).
    Bottom,
    /// At index `n` (0 is the top); an index past the bottom means the bottom (the CR 401.7
    /// treatment, applied to every ordered zone).
    Index(usize),
}

/// How an object enters a zone (REQ-ZONE-001, REQ-CORE-002).
///
/// There is no destination player: a per-player destination is always the object owner's zone
/// (CR 400.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveRequest {
    /// The kind of the destination zone.
    pub to: ZoneKind,
    /// The position in an ordered destination; ignored for unordered zones.
    pub position: Position,
    /// Whether the object is face down in the destination (CR 406.3); never carried over.
    pub face_down: bool,
    /// The controller in the destination. Only allowed for the battlefield and the stack;
    /// `None` means the owner.
    pub controller: Option<PlayerId>,
}

impl MoveRequest {
    /// A request to enter a zone of kind `to` on top, face up, controlled by the owner.
    #[must_use]
    pub fn new(to: ZoneKind) -> MoveRequest {
        MoveRequest {
            to,
            position: Position::Top,
            face_down: false,
            controller: None,
        }
    }
}

/// The record of one object in a zone (REQ-ZONE-001, REQ-CORE-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectRecord {
    /// The object's id; a new one on every zone change (CR 400.7).
    pub id: ObjectId,
    /// The physical card the object represents; persists across zone changes.
    pub card: CardId,
    /// The object's owner; never changed by a move.
    pub owner: PlayerId,
    /// The object's controller; equal to the owner outside the battlefield and the stack.
    pub controller: PlayerId,
    /// Whether the object is face down (CR 406.3).
    pub face_down: bool,
    /// The zone the object is in.
    pub zone: ZoneId,
}

/// The result of a zone move: the old and the new object id and both zones (REQ-CORE-002,
/// CR 400.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    /// The id the object had before the move; no longer held anywhere in the container.
    pub old: ObjectId,
    /// The new object's id.
    pub new: ObjectId,
    /// The zone the object left.
    pub from: ZoneId,
    /// The zone the new object entered.
    pub to: ZoneId,
}

/// Configuration of the zone container (REQ-ZONE-001). Stand-in for the format configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ZonesConfig {
    /// Allow players to reorder their graveyards freely (REQ-ZONE-001 item 4, the CR 404.2
    /// tournament option). Default `false`.
    pub free_graveyard_reorder: bool,
}

/// An outside-the-game card pool of a player (REQ-ZONE-001 item 9). Outside the game is not a
/// zone (CR 400.11); pools hold card ids, never object ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum OutsidePool {
    /// The sideboard (CR 400.11a).
    Sideboard,
    /// A companion set aside outside the game.
    Companion,
    /// Lesson cards outside the game.
    Lesson,
    /// The Attraction deck.
    AttractionDeck,
    /// The Contraption deck.
    ContraptionDeck,
    /// A spellbook.
    Spellbook,
}

impl OutsidePool {
    /// The canonical encoding tag of this pool.
    pub(crate) fn tag(self) -> u8 {
        match self {
            OutsidePool::Sideboard => 0,
            OutsidePool::Companion => 1,
            OutsidePool::Lesson => 2,
            OutsidePool::AttractionDeck => 3,
            OutsidePool::ContraptionDeck => 4,
            OutsidePool::Spellbook => 5,
        }
    }
}
