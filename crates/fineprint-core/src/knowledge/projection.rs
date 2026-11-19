// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The projection `V(S, P)`: the zones as one player may know them (REQ-CORE-004).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, ObjectId, PlayerId};

use super::{Knowledge, KnowledgeError, require_player};
use crate::zones::{ObjectRecord, ZoneId, Zones};

/// One object in a zone whose objects are visible as objects: the viewer's own hand, the
/// graveyards, the battlefield, the stack, exile and the command zone (REQ-CORE-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectView {
    /// The object's id (the object's existence is public, even when it is face down).
    pub id: ObjectId,
    /// The object's owner.
    pub owner: PlayerId,
    /// The controller the zone container stores (the owner outside the battlefield and the
    /// stack).
    pub controller: PlayerId,
    /// Whether the object is face down.
    pub face_down: bool,
    /// The card's identity, present iff the viewer may identify the object
    /// ([`Knowledge::can_identify`]).
    pub card: Option<CardId>,
}

/// A library as the viewer may know it: the number of cards and the set of card identities the
/// viewer knows are in it. Never a position or an object id (REQ-CORE-004, SRS 2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryView {
    /// The number of cards in the library.
    pub count: usize,
    /// The cards in the library the viewer can identify, ordered by card id only.
    pub known_cards: BTreeSet<CardId>,
}

/// A hand as the viewer may know it (REQ-CORE-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandView {
    /// The viewer's own hand: every object, ascending object id, each with its card.
    Own(Vec<ObjectView>),
    /// Another player's hand: the number of cards and the card identities the viewer knows are
    /// in it (for example revealed ones); no object ids.
    Opponent {
        /// The number of cards in the hand.
        count: usize,
        /// The cards in the hand the viewer can identify, ordered by card id only.
        known_cards: BTreeSet<CardId>,
    },
}

/// One player's library, hand and graveyard as the viewer may know them (REQ-CORE-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerZonesView {
    /// The player's library.
    pub library: LibraryView,
    /// The player's hand.
    pub hand: HandView,
    /// The player's graveyard, top first.
    pub graveyard: Vec<ObjectView>,
}

/// The redacted projection `V(S, P)` of the zones for one viewer: exactly the information the
/// viewer is entitled to (REQ-CORE-004). Safe to hand to that viewer; the wire schema is later
/// work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonesView {
    /// The player this view is for.
    pub viewer: PlayerId,
    /// Every player's own zones, by player.
    pub players: BTreeMap<PlayerId, PlayerZonesView>,
    /// The battlefield, ascending object id.
    pub battlefield: Vec<ObjectView>,
    /// The stack, top first.
    pub stack: Vec<ObjectView>,
    /// The exile zone, ascending object id.
    pub exile: Vec<ObjectView>,
    /// The command zone, ascending object id.
    pub command: Vec<ObjectView>,
}

impl Knowledge {
    /// The projection `V(S, viewer)` of `zones` (REQ-CORE-004): every player's library as a
    /// count and the identifiable card set (CR 401.3), the viewer's hand in full and other hands
    /// as a count and the identifiable card set (CR 402.3), and one [`ObjectView`] per object in
    /// the public zones in contents order, with its card iff the viewer can identify it
    /// (CR 400.2). Nothing the viewer cannot know is in the result, and changes to such
    /// information do not change it. Does not mutate anything.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]`(viewer)`.
    pub fn project(&self, zones: &Zones, viewer: PlayerId) -> Result<ZonesView, KnowledgeError> {
        require_player(zones, viewer)?;
        let players = zones
            .players()
            .into_iter()
            .map(|p| (p, self.project_player(zones, viewer, p)))
            .collect();
        Ok(ZonesView {
            viewer,
            players,
            battlefield: self.objects(zones, viewer, ZoneId::Battlefield),
            stack: self.objects(zones, viewer, ZoneId::Stack),
            exile: self.objects(zones, viewer, ZoneId::Exile),
            command: self.objects(zones, viewer, ZoneId::Command),
        })
    }

    fn project_player(&self, zones: &Zones, viewer: PlayerId, player: PlayerId) -> PlayerZonesView {
        // CR 401.2, CR 401.3: a library is a face-down pile anyone may count.
        let library = LibraryView {
            count: zones.len(ZoneId::Library(player)),
            known_cards: self.known_cards(zones, viewer, ZoneId::Library(player)),
        };
        // CR 402.3: a player may look at their own hand; others may count it.
        let hand = if player == viewer {
            HandView::Own(self.objects(zones, viewer, ZoneId::Hand(player)))
        } else {
            HandView::Opponent {
                count: zones.len(ZoneId::Hand(player)),
                known_cards: self.known_cards(zones, viewer, ZoneId::Hand(player)),
            }
        };
        PlayerZonesView {
            library,
            hand,
            graveyard: self.objects(zones, viewer, ZoneId::Graveyard(player)),
        }
    }

    /// The identifiable cards of the hidden `zone`, as a set ordered by card id only.
    fn known_cards(&self, zones: &Zones, viewer: PlayerId, zone: ZoneId) -> BTreeSet<CardId> {
        zones
            .contents(zone)
            .into_iter()
            .filter_map(|id| zones.object(id).copied())
            .filter(|r| self.identifies(r, viewer))
            .map(|r| r.card)
            .collect()
    }

    /// One view per object of `zone`, in contents order.
    fn objects(&self, zones: &Zones, viewer: PlayerId, zone: ZoneId) -> Vec<ObjectView> {
        zones
            .contents(zone)
            .into_iter()
            .filter_map(|id| zones.object(id).copied())
            .map(|r| self.object_view(&r, viewer))
            .collect()
    }

    fn object_view(&self, record: &ObjectRecord, viewer: PlayerId) -> ObjectView {
        ObjectView {
            id: record.id,
            owner: record.owner,
            controller: record.controller,
            face_down: record.face_down,
            card: self.identifies(record, viewer).then_some(record.card),
        }
    }
}
