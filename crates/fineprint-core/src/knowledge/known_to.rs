// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Identity knowledge (`KnownTo`), the entitlement rule and the knowledge effects of zone
//! changes and game events (REQ-CORE-004).

use std::collections::BTreeSet;
use std::sync::Arc;

use fineprint_base::ids::{CardId, ObjectId, PlayerId};

use super::{Knowledge, KnowledgeError, record_of, require_player};
use crate::zones::{Moved, ObjectRecord, ZoneId, ZoneKind, Zones};

impl Knowledge {
    /// Adds every player of `players` to the stored `KnownTo` of `card` (no validation).
    pub(super) fn add_players(
        &mut self,
        card: CardId,
        players: impl IntoIterator<Item = PlayerId>,
    ) {
        let mut players = players.into_iter().peekable();
        if players.peek().is_none() {
            return;
        }
        Arc::make_mut(&mut self.known)
            .entry(card)
            .or_default()
            .extend(players);
    }

    /// The stored `KnownTo` of `card` (empty if none).
    pub(super) fn known_set(&self, card: CardId) -> BTreeSet<PlayerId> {
        self.known.get(&card).cloned().unwrap_or_default()
    }

    /// The stored `KnownTo` set of the card behind `id`: the players who know its identity
    /// because it was revealed to them, they looked at it, or they saw it in a public zone and
    /// tracked it since (REQ-CORE-004, SRS 2.4). Empty if none. It does NOT include the
    /// implicit entitlements of [`Knowledge::can_identify`] (public information, the owner of a
    /// hand card, the controller of a face-down permanent or spell).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`] if `id` is not in a zone.
    pub fn known_to(
        &self,
        zones: &Zones,
        id: ObjectId,
    ) -> Result<BTreeSet<PlayerId>, KnowledgeError> {
        let record = record_of(zones, id)?;
        Ok(self.known_set(record.card))
    }

    /// Adds `player` to the `KnownTo` of the card behind `id`: a reveal to one player, or that
    /// player looking at the card (REQ-CORE-004).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`], then [`KnowledgeError::UnknownPlayer`]. The state is
    /// then unchanged.
    pub fn add_known(
        &mut self,
        zones: &Zones,
        id: ObjectId,
        player: PlayerId,
    ) -> Result<(), KnowledgeError> {
        let record = record_of(zones, id)?;
        require_player(zones, player)?;
        self.add_players(record.card, [player]);
        Ok(())
    }

    /// Reveals the object `id` to every player of `zones.players()`: they all know its
    /// identity from now on. The object does not move (REQ-CORE-004).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`]; the state is then unchanged.
    pub fn reveal_to_all(&mut self, zones: &Zones, id: ObjectId) -> Result<(), KnowledgeError> {
        // CR 701.20a: show the card to all players. CR 701.20b: it does not leave its zone.
        let record = record_of(zones, id)?;
        self.add_players(record.card, zones.players());
        Ok(())
    }

    /// Grants `player` permission to look at the face-down exiled card `id` (REQ-CORE-004). The
    /// effect is that of [`Knowledge::add_known`]; the permission persists after the granting
    /// effect ends, until the card leaves exile or is shuffled into a face-down pile
    /// ([`Knowledge::shuffle_face_down_pile`]).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`], then [`KnowledgeError::UnknownPlayer`], then
    /// [`KnowledgeError::NotFaceDown`] if the object is face up or not in exile. The state is
    /// then unchanged.
    pub fn grant_look(
        &mut self,
        zones: &Zones,
        id: ObjectId,
        player: PlayerId,
    ) -> Result<(), KnowledgeError> {
        // CR 406.3: once a player is allowed to look at a card exiled face down, that player may
        // continue to look at it, even if the instruction allowing it no longer applies.
        let record = record_of(zones, id)?;
        require_player(zones, player)?;
        if !(record.face_down && record.zone == ZoneId::Exile) {
            return Err(KnowledgeError::NotFaceDown(id));
        }
        self.add_players(record.card, [player]);
        Ok(())
    }

    /// Whether `viewer` may see the identity ([`CardId`]) of the object `id` now
    /// (REQ-CORE-004). True iff (1) the object is public information (a public zone, not face
    /// down), (2) it is in the viewer's hand, (3) it is a face-down permanent or spell the
    /// viewer controls, or (4) the viewer is in the card's stored `KnownTo`. Everything else is
    /// false: a face-down object in exile, a graveyard or the command zone is identifiable only
    /// through (4), and the owner of a face-down card has no implicit right.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`], then [`KnowledgeError::UnknownPlayer`].
    pub fn can_identify(
        &self,
        zones: &Zones,
        viewer: PlayerId,
        id: ObjectId,
    ) -> Result<bool, KnowledgeError> {
        let record = record_of(zones, id)?;
        require_player(zones, viewer)?;
        Ok(self.identifies(&record, viewer))
    }

    /// The entitlement rule on a validated record.
    pub(super) fn identifies(&self, record: &ObjectRecord, viewer: PlayerId) -> bool {
        implicitly_identifies(record, viewer)
            || self
                .known
                .get(&record.card)
                .is_some_and(|s| s.contains(&viewer))
    }

    /// Shuffle of a face-down pile of the objects `ids` (REQ-CORE-004). Clears the stored
    /// `KnownTo` of their cards, which ends every look permission on them, and removes every
    /// position entry naming them.
    ///
    /// Only face-down cards in exile can form such a pile here: look permissions on face-down
    /// cards exist only in exile (CR 406.3). A face-down card in a library is part of the
    /// library and is shuffled with [`Knowledge::shuffle_library`]; a face-down permanent or
    /// spell is not in a pile.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`] for an unknown id, [`KnowledgeError::NotFaceDown`] for
    /// an object that is face up or is face down in any zone other than exile. Every id is
    /// checked first; the state is then unchanged.
    pub fn shuffle_face_down_pile(
        &mut self,
        zones: &Zones,
        ids: &[ObjectId],
    ) -> Result<(), KnowledgeError> {
        // CR 701.24a: shuffling a face-down pile randomizes it so that no player knows its order.
        // CR 406.3: a player may look at a card exiled face down until it is part of a pile of
        // cards that are shuffled.
        let mut cards = BTreeSet::new();
        for &id in ids {
            let record = record_of(zones, id)?;
            if !(record.face_down && record.zone == ZoneId::Exile) {
                return Err(KnowledgeError::NotFaceDown(id));
            }
            cards.insert(record.card);
        }
        if cards.iter().any(|c| self.known.contains_key(c)) {
            let known = Arc::make_mut(&mut self.known);
            for card in &cards {
                known.remove(card);
            }
        }
        let named: BTreeSet<ObjectId> = ids.iter().copied().collect();
        self.retain_positions(|_, _, object| !named.contains(&object));
        Ok(())
    }

    /// Updates knowledge after a zone move that no player is recorded as having watched
    /// (REQ-CORE-004): exactly [`Knowledge::on_move_observed`] with no observers. Call it AFTER
    /// [`Zones::move_object`] with `before` = the object's record copied before the move,
    /// `moved` = the returned [`Moved`] and `zones` = the container after the move.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`]`(moved.new)` if that object is not in `zones`; the
    /// state is then unchanged.
    pub fn on_move(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
    ) -> Result<(), KnowledgeError> {
        self.on_move_observed(zones, before, moved, &[])
    }

    /// Like on_move; `observers` are players who watched the move and so know which card moved.
    ///
    /// Call it AFTER [`Zones::move_object`] with `before` = the object's record copied before
    /// the move, `moved` = the returned [`Moved`] and `zones` = the container after the move
    /// (REQ-CORE-004). In order:
    ///
    /// - (0) Only when `before` was in a library or a hand and `moved.from != moved.to`: a
    ///   player V observed the move iff V is in `observers`, or V may see the new object by the
    ///   implicit entitlements of [`Knowledge::can_identify`] (public face up, V's own hand, a
    ///   face-down permanent or spell V controls), or the object left V's hand. Nothing else
    ///   makes V an observer: in particular a viewer who knows the moved card's library
    ///   position is an observer ONLY if the caller passes it in `observers`. The caller must
    ///   do so when the selected position is itself public (for example a draw of the top card
    ///   by a viewer who knows the top card, or a selection announced as "the card at index
    ///   k"); `Knowledge` cannot tell a disclosed-position move from a secret selection.
    ///   Every player who did NOT observe it forgets every card left in `moved.from` and the moved card (is removed
    ///   from their stored `KnownTo`), and, when the source is a library, loses every position
    ///   entry for that library: such a player knows a set but cannot tell which member moved,
    ///   so any of them could be the moved one (a design decision, see `docs/DECISIONS.md`,
    ///   "#25 FP-024: Unobserved hidden moves forget the candidates"). An observer's knowledge
    ///   is kept as it was.
    /// - (a) `before` was in a public zone and face up: every player saw it and keeps tracking
    ///   it (it is revealed to all).
    /// - (b) `before` was in its owner's hand: the owner knows it.
    /// - (c) `before` was face down on the battlefield or the stack: its controller knows it;
    ///   and when it moved from the battlefield to another zone, or from the stack to a zone
    ///   other than the battlefield, it is revealed to all players.
    /// - (d) `before` was in a library: every position entry naming `moved.old` is dropped.
    ///   Position entries of the libraries `moved` left or entered that no longer match the
    ///   live library are pruned too (a known card whose index shifted becomes unknown).
    ///
    /// Apart from (0), `KnownTo` persists across the move: it is stored per card.
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownObject`]`(moved.new)` if that object is not in `zones`, then
    /// [`KnowledgeError::UnknownPlayer`] for the first observer that is not a player of
    /// `zones`. The state is then unchanged.
    pub fn on_move_observed(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
        observers: &[PlayerId],
    ) -> Result<(), KnowledgeError> {
        let record = record_of(zones, moved.new)?;
        for &observer in observers {
            require_player(zones, observer)?;
        }
        let card = record.card;
        // (0) Design decision (not CR-derived): an unobserved move out of a hidden zone makes
        // every candidate of that zone ambiguous to the players who did not watch it.
        if matches!(before.zone.kind(), ZoneKind::Library | ZoneKind::Hand)
            && moved.from != moved.to
        {
            self.forget_unobserved(zones, before, moved, &record, observers);
        }
        // (a) CR 400.2: the face of a face-up card in a public zone was seen by all players.
        if before.zone.kind().is_public() && !before.face_down {
            self.add_players(card, zones.players());
        }
        // (b) CR 402.3: the owner could look at the card in their hand.
        if let ZoneId::Hand(owner) = before.zone {
            self.add_players(card, [owner]);
        }
        // (c) CR 708.5: the controller could look at the face-down permanent or spell.
        if before.face_down && matches!(before.zone, ZoneId::Battlefield | ZoneId::Stack) {
            self.add_players(card, [before.controller]);
            // CR 708.9: a face-down permanent that leaves the battlefield, or a face-down spell
            // that moves from the stack to any zone other than the battlefield, is revealed.
            let leaves = moved.from != moved.to
                && (moved.from == ZoneId::Battlefield
                    || (moved.from == ZoneId::Stack && moved.to != ZoneId::Battlefield));
            if leaves {
                self.add_players(card, zones.players());
            }
        }
        // (d) A card that moved within or out of a library is no longer at its known position.
        if before.zone.kind() == ZoneKind::Library {
            let old = moved.old;
            self.retain_positions(|_, _, object| object != old);
        }
        for zone in [moved.from, moved.to] {
            if let ZoneId::Library(owner) = zone {
                self.prune_library(zones, owner);
            }
        }
        Ok(())
    }

    /// Step (0) of [`Knowledge::on_move_observed`]: every player who did not observe the move
    /// out of a hidden zone forgets the cards of the source zone and the moved card, and their
    /// position entries for a source library.
    fn forget_unobserved(
        &mut self,
        zones: &Zones,
        before: &ObjectRecord,
        moved: &Moved,
        record: &ObjectRecord,
        observers: &[PlayerId],
    ) {
        let source_library = match before.zone {
            ZoneId::Library(owner) => Some(owner),
            _ => None,
        };
        let blind: BTreeSet<PlayerId> = zones
            .players()
            .into_iter()
            .filter(|&v| {
                // Every condition depends only on the observers, the destination record and the
                // source zone, never on which candidate moved: knowing a position is not
                // observation (a secret selection would otherwise leak).
                let observed = observers.contains(&v)
                    || implicitly_identifies(record, v)
                    || before.zone == ZoneId::Hand(v);
                !observed
            })
            .collect();
        if blind.is_empty() {
            return;
        }
        let candidates: BTreeSet<CardId> = zones
            .contents(moved.from)
            .into_iter()
            .filter_map(|id| zones.object(id).map(|r| r.card))
            .chain([record.card])
            .collect();
        let touched = candidates.iter().any(|c| {
            self.known
                .get(c)
                .is_some_and(|s| s.iter().any(|v| blind.contains(v)))
        });
        if touched {
            let known = Arc::make_mut(&mut self.known);
            for c in &candidates {
                if let Some(players) = known.get_mut(c) {
                    players.retain(|v| !blind.contains(v));
                    if players.is_empty() {
                        known.remove(c);
                    }
                }
            }
        }
        if let Some(owner) = source_library {
            self.retain_positions(|v, o, _| o != owner || !blind.contains(&v));
        }
    }

    /// The third sentence of CR 708.9: when `player` leaves the game, every face-down permanent
    /// and face-down spell they own is revealed to all players (REQ-CORE-004).
    ///
    /// # Errors
    /// [`KnowledgeError::UnknownPlayer`]; the state is then unchanged.
    pub fn on_player_leaves(
        &mut self,
        zones: &Zones,
        player: PlayerId,
    ) -> Result<(), KnowledgeError> {
        require_player(zones, player)?;
        // CR 708.9: face-down permanents and face-down spells owned by a leaving player are revealed.
        self.reveal_face_down_in_play(zones, Some(player));
        Ok(())
    }

    /// The fourth sentence of CR 708.9: at the end of the game every face-down permanent and
    /// face-down spell is revealed to all players (REQ-CORE-004).
    pub fn on_game_end(&mut self, zones: &Zones) {
        // CR 708.9: at the end of each game all face-down permanents and spells are revealed.
        self.reveal_face_down_in_play(zones, None);
    }

    /// Reveals to all players every face-down object on the battlefield and the stack, owned by
    /// `owner` when given.
    fn reveal_face_down_in_play(&mut self, zones: &Zones, owner: Option<PlayerId>) {
        let players = zones.players();
        for zone in [ZoneId::Battlefield, ZoneId::Stack] {
            for id in zones.contents(zone) {
                if let Some(record) = zones.object(id).copied()
                    && record.face_down
                    && owner.is_none_or(|o| o == record.owner)
                {
                    self.add_players(record.card, players.iter().copied());
                }
            }
        }
    }
}

/// The implicit entitlements of [`Knowledge::can_identify`] (rules 1-3), ignoring the stored
/// `KnownTo`.
fn implicitly_identifies(record: &ObjectRecord, viewer: PlayerId) -> bool {
    // CR 400.2: public zones show the cards' faces, except face-down cards.
    if record.zone.kind().is_public() && !record.face_down {
        return true;
    }
    // CR 402.3: a player may look at their own hand at any time.
    if record.zone == ZoneId::Hand(viewer) {
        return true;
    }
    // CR 708.5: a face-down spell or permanent may be looked at by its controller, and by no
    // other player; face-down cards in any other zone can't be looked at.
    record.face_down
        && matches!(record.zone, ZoneId::Battlefield | ZoneId::Stack)
        && record.controller == viewer
}
