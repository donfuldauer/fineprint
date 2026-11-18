// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The zone container and the zone-move primitive (REQ-ZONE-001, REQ-CORE-002).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_len, put_variant};
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};

use super::error::ZoneError;
use super::types::{
    MoveRequest, Moved, ObjectRecord, OutsidePool, Position, ZoneId, ZoneKind, ZonesConfig,
};

/// The zones of one player and their outside-the-game pools.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PlayerZones {
    /// Top first (CR 401.2).
    library: Arc<Vec<ObjectId>>,
    /// Unordered (CR 402.3); ascending ObjectId.
    hand: Arc<BTreeSet<ObjectId>>,
    /// Top first (CR 404.1).
    graveyard: Arc<Vec<ObjectId>>,
    /// Outside the game (CR 400.11). Empty pools are never stored, so equal contents are equal
    /// values.
    outside: Arc<BTreeMap<OutsidePool, BTreeSet<CardId>>>,
}

/// The zone containers of a game (REQ-ZONE-001, REQ-CORE-002).
///
/// Per player: a library, a hand and a graveyard (CR 400.1); shared: the battlefield, the
/// stack, the exile zone and the command zone; plus per-player outside-the-game card pools
/// (CR 400.11). Every object currently in a zone has an [`ObjectRecord`]; every card is in at
/// most one place.
///
/// Cloning is a cheap snapshot with structural sharing: each component sits behind an `Arc` and
/// is written with `Arc::make_mut`. Mutating one copy never affects another. Equality compares
/// values.
///
/// **Omniscient state.** `Zones` is the omniscient engine state. [`Zones::contents`],
/// [`Zones::object`], [`Zones::object_of_card`], [`Zones::all_object_ids`] and the canonical
/// encoding expose hidden information: library order, hand membership and the `CardId` of
/// face-down objects. It must stay behind the redacted per-player view and the determinized-fork
/// boundary (REQ-CORE-004, REQ-API-004) and is never handed to a client directly.
///
/// **Stack scope.** In this unit the stack holds only card-backed objects (spells, and copies
/// that have a `CardId`); every object is created with a `CardId` ([`Zones::create_object`]).
/// Cardless stack objects (abilities, which go on the stack without a card, CR 405.1, CR 109.1;
/// copies of spells that are not cards) are out of scope and are the job of the later units that
/// own them: #31 FP-030 (putting triggered abilities on the stack), #58 FP-056 (resolution of
/// spells and abilities) and #66 FP-064 (copies of spells and abilities), with activated
/// abilities in a later stack/ability unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zones {
    config: ZonesConfig,
    players: Arc<BTreeMap<PlayerId, PlayerZones>>,
    /// Unordered; ascending ObjectId.
    battlefield: Arc<BTreeSet<ObjectId>>,
    /// Top first (CR 405.2).
    stack: Arc<Vec<ObjectId>>,
    /// Unordered; ascending ObjectId.
    exile: Arc<BTreeSet<ObjectId>>,
    /// Unordered; ascending ObjectId.
    command: Arc<BTreeSet<ObjectId>>,
    /// The record of every object in a zone.
    objects: Arc<BTreeMap<ObjectId, ObjectRecord>>,
    /// The current object of every card in a zone.
    cards: Arc<BTreeMap<CardId, ObjectId>>,
    /// Where every outside-the-game card is.
    outside_index: Arc<BTreeMap<CardId, (PlayerId, OutsidePool)>>,
    /// The highest `ObjectId` raw value this container has ever issued or accepted (0 when none):
    /// the high-water mark that keeps retired ids from being issued again (REQ-CORE-002). Part
    /// of equality and of the canonical encoding.
    max_issued: u64,
}

/// Inserts `id` into the top-first sequence `seq` at `position`; an index past the bottom means
/// the bottom (CR 401.7).
fn insert_ordered(seq: &mut Vec<ObjectId>, id: ObjectId, position: Position) {
    let at = match position {
        Position::Top => 0,
        Position::Bottom => seq.len(),
        // CR 401.7: fewer than N objects puts it on the bottom.
        Position::Index(n) => n.min(seq.len()),
    };
    seq.insert(at, id);
}

/// Removes `id` from the top-first sequence `seq`, keeping the order of the rest (CR 400.5).
fn remove_ordered(seq: &mut Vec<ObjectId>, id: ObjectId) {
    seq.retain(|&x| x != id);
}

impl Zones {
    /// An empty container with one library, hand and graveyard per listed player (duplicates
    /// ignored); every zone and pool is empty (REQ-ZONE-001, CR 400.1).
    #[must_use]
    pub fn new(players: &[PlayerId], config: ZonesConfig) -> Zones {
        let players = players
            .iter()
            .map(|&p| (p, PlayerZones::default()))
            .collect();
        Zones {
            config,
            players: Arc::new(players),
            battlefield: Arc::default(),
            stack: Arc::default(),
            exile: Arc::default(),
            command: Arc::default(),
            objects: Arc::default(),
            cards: Arc::default(),
            outside_index: Arc::default(),
            max_issued: 0,
        }
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> ZonesConfig {
        self.config
    }

    /// The players, ascending.
    #[must_use]
    pub fn players(&self) -> Vec<PlayerId> {
        self.players.keys().copied().collect()
    }

    fn require_player(&self, player: PlayerId) -> Result<(), ZoneError> {
        if self.players.contains_key(&player) {
            Ok(())
        } else {
            Err(ZoneError::UnknownPlayer(player))
        }
    }

    /// True when `card` is in a zone or in any outside-the-game pool.
    fn card_present(&self, card: CardId) -> bool {
        self.cards.contains_key(&card) || self.outside_index.contains_key(&card)
    }

    /// The controller the object gets: the request's (battlefield and stack only, CR 109.4,
    /// CR 110.2a, CR 405.4) or the owner (the CR 108.4a fallback). An explicit controller
    /// elsewhere is rejected as a container rule: emblems (CR 109.4c) and variant cards are not
    /// modelled yet.
    fn resolve_controller(
        &self,
        req: &MoveRequest,
        owner: PlayerId,
    ) -> Result<PlayerId, ZoneError> {
        match req.controller {
            None => Ok(owner),
            Some(p) => {
                if !matches!(req.to, ZoneKind::Battlefield | ZoneKind::Stack) {
                    return Err(ZoneError::ControllerNotApplicable);
                }
                self.require_player(p)?;
                Ok(p)
            }
        }
    }

    /// Removes `id` from the container of `zone` (not from the record index).
    fn detach(&mut self, id: ObjectId, zone: ZoneId) {
        match zone {
            ZoneId::Library(p) | ZoneId::Hand(p) | ZoneId::Graveyard(p) => {
                let players = Arc::make_mut(&mut self.players);
                if let Some(pz) = players.get_mut(&p) {
                    match zone.kind() {
                        ZoneKind::Library => remove_ordered(Arc::make_mut(&mut pz.library), id),
                        ZoneKind::Graveyard => {
                            remove_ordered(Arc::make_mut(&mut pz.graveyard), id);
                        }
                        _ => {
                            Arc::make_mut(&mut pz.hand).remove(&id);
                        }
                    }
                }
            }
            ZoneId::Battlefield => {
                Arc::make_mut(&mut self.battlefield).remove(&id);
            }
            ZoneId::Stack => remove_ordered(Arc::make_mut(&mut self.stack), id),
            ZoneId::Exile => {
                Arc::make_mut(&mut self.exile).remove(&id);
            }
            ZoneId::Command => {
                Arc::make_mut(&mut self.command).remove(&id);
            }
        }
    }

    /// Puts `id` into the container of `zone` at `position` (ordered zones only).
    fn attach(&mut self, id: ObjectId, zone: ZoneId, position: Position) {
        match zone {
            ZoneId::Library(p) | ZoneId::Hand(p) | ZoneId::Graveyard(p) => {
                let players = Arc::make_mut(&mut self.players);
                if let Some(pz) = players.get_mut(&p) {
                    match zone.kind() {
                        ZoneKind::Library => {
                            insert_ordered(Arc::make_mut(&mut pz.library), id, position);
                        }
                        ZoneKind::Graveyard => {
                            insert_ordered(Arc::make_mut(&mut pz.graveyard), id, position);
                        }
                        _ => {
                            Arc::make_mut(&mut pz.hand).insert(id);
                        }
                    }
                }
            }
            ZoneId::Battlefield => {
                Arc::make_mut(&mut self.battlefield).insert(id);
            }
            // CR 405.2: the default request puts the object on top.
            ZoneId::Stack => insert_ordered(Arc::make_mut(&mut self.stack), id, position),
            ZoneId::Exile => {
                Arc::make_mut(&mut self.exile).insert(id);
            }
            ZoneId::Command => {
                Arc::make_mut(&mut self.command).insert(id);
            }
        }
    }

    /// Issues the next id from a copy of `ids` and rejects it unless it is above the high-water
    /// mark, so an id at or below it (live, retired or never held) is never issued
    /// (REQ-CORE-002, CR 400.7). Returns the advanced copy, to be written back (with
    /// [`Zones::commit_issued`]) only once the operation cannot fail any more, and the id.
    fn issue_fresh(
        &self,
        ids: &IdGenerator<ObjectId>,
    ) -> Result<(IdGenerator<ObjectId>, ObjectId), ZoneError> {
        let mut probe = ids.clone();
        let id = probe.issue().map_err(|_| ZoneError::IdExhausted)?;
        if id.get() <= self.max_issued {
            return Err(ZoneError::IdCollision(id));
        }
        Ok((probe, id))
    }

    /// Writes back the advanced generator and raises the high-water mark to `id`.
    fn commit_issued(
        &mut self,
        ids: &mut IdGenerator<ObjectId>,
        probe: IdGenerator<ObjectId>,
        id: ObjectId,
    ) {
        *ids = probe;
        self.max_issued = id.get();
    }

    /// Creates a new object for `card` (owned by `owner`) in the zone `req` names; the object
    /// gets a fresh id from `ids` (REQ-CORE-002, CR 400.7). Used for cards entering a zone from
    /// no zone, such as game setup. Every object needs a `CardId`: cardless stack objects
    /// (abilities, CR 405.1) cannot be created here (see [`Zones`], "Stack scope").
    ///
    /// A per-player destination is `owner`'s zone (CR 400.3). `ids` must be the generator that
    /// issued every id this container ever held; a generator that is behind it (stale, saved or
    /// fresh) is rejected.
    ///
    /// # Errors
    /// [`ZoneError::UnknownPlayer`] if `owner` (or the requested controller) is unknown,
    /// [`ZoneError::CardAlreadyPresent`] if `card` is in a zone or pool,
    /// [`ZoneError::ControllerNotApplicable`] if a controller is requested outside the
    /// battlefield and the stack, [`ZoneError::IdExhausted`] if `ids` is exhausted,
    /// [`ZoneError::IdCollision`] if `ids` issues an id that is not above the highest id this
    /// container has issued or accepted (a high-water rejection: it can reject an id the
    /// container never held). The container and `ids` are then unchanged.
    pub fn create_object(
        &mut self,
        ids: &mut IdGenerator<ObjectId>,
        card: CardId,
        owner: PlayerId,
        req: MoveRequest,
    ) -> Result<ObjectId, ZoneError> {
        self.require_player(owner)?;
        if self.card_present(card) {
            return Err(ZoneError::CardAlreadyPresent(card));
        }
        let controller = self.resolve_controller(&req, owner)?;
        let (probe, id) = self.issue_fresh(ids)?;
        self.commit_issued(ids, probe, id);
        self.place(id, card, owner, controller, &req);
        Ok(id)
    }

    /// Records and attaches a new object (all checks done).
    fn place(
        &mut self,
        id: ObjectId,
        card: CardId,
        owner: PlayerId,
        controller: PlayerId,
        req: &MoveRequest,
    ) -> ZoneId {
        // CR 400.3: per-player destinations are always the owner's.
        let zone = ZoneId::of(req.to, owner);
        let record = ObjectRecord {
            id,
            card,
            owner,
            controller,
            face_down: req.face_down,
            zone,
        };
        Arc::make_mut(&mut self.objects).insert(id, record);
        Arc::make_mut(&mut self.cards).insert(card, id);
        self.attach(id, zone, req.position);
        zone
    }

    /// Removes the object of `record` and its record entirely.
    fn remove_object(&mut self, record: ObjectRecord) {
        self.detach(record.id, record.zone);
        Arc::make_mut(&mut self.objects).remove(&record.id);
        Arc::make_mut(&mut self.cards).remove(&record.card);
    }

    fn record(&self, id: ObjectId) -> Result<ObjectRecord, ZoneError> {
        self.objects
            .get(&id)
            .copied()
            .ok_or(ZoneError::UnknownObject(id))
    }

    /// The zone-move primitive (REQ-CORE-002, REQ-ZONE-001, CR 400.7).
    ///
    /// Moves the object `id` to the zone `req` names. The destination `ZoneId` is
    /// `ZoneId::of(req.to, owner)`: a per-player destination is always the owner's zone
    /// (CR 400.3). Three cases:
    ///
    /// - **Zone change** (the destination is another zone): the object becomes a new object
    ///   (CR 400.7). Exactly one fresh id is issued from `ids`, the old object is removed
    ///   entirely, and the new one, with the same card and owner, is inserted at `req.position`.
    ///   No reference to the old id remains anywhere in the container; the caller may hand both
    ///   ids from the returned [`Moved`] to the exception registry (CR 400.7a-m).
    /// - **Same zone, exile or command zone**: the object doesn't change zones but becomes a new
    ///   object (CR 400.8, CR 406.7 for exile; CR 400.10 for the command zone), handled exactly
    ///   like a zone change: a fresh id, the old id forgotten.
    /// - **Same zone, library, hand, graveyard, battlefield or stack**: not a zone change
    ///   (CR 400.7 applies to moves "from one zone to another"), so the object keeps its id and
    ///   `ids` is not touched. It is repositioned within the zone: in an ordered zone it is
    ///   removed and re-inserted at `req.position` counted in the remaining pile (CR 400.5);
    ///   in an unordered zone the position is ignored. The returned [`Moved`] has `old == new`.
    ///   The mark and `ids` are untouched.
    ///
    /// In every case:
    /// - Ordered zones keep the relative order of the other objects (CR 400.5); `Index(n)` past
    ///   the bottom means the bottom (CR 401.7). The default request puts the object on top of
    ///   the stack (CR 405.2) or graveyard (CR 404.1).
    /// - `face_down` comes from the request, never from the old object (CR 406.3).
    /// - The controller is the requested one (battlefield and stack only, CR 109.4, CR 110.2a,
    ///   CR 405.4) or the owner (CR 108.4a fallback).
    ///
    /// `ids` must be the generator that issued every id this container ever held; a generator
    /// that is behind it (stale, saved or fresh) is rejected.
    ///
    /// # Errors
    /// [`ZoneError::UnknownObject`], [`ZoneError::ControllerNotApplicable`],
    /// [`ZoneError::UnknownPlayer`] (requested controller), and, when a new id is needed,
    /// [`ZoneError::IdExhausted`] and [`ZoneError::IdCollision`] (the issued id is not above the
    /// highest id this container has issued or accepted: a high-water rejection, which can
    /// reject an id the container never held). The container and `ids` are then unchanged.
    pub fn move_object(
        &mut self,
        ids: &mut IdGenerator<ObjectId>,
        id: ObjectId,
        req: MoveRequest,
    ) -> Result<Moved, ZoneError> {
        let record = self.record(id)?;
        let controller = self.resolve_controller(&req, record.owner)?;
        // CR 400.3: a per-player destination is the owner's zone.
        let dest = ZoneId::of(req.to, record.owner);
        if dest == record.zone && !matches!(dest, ZoneId::Exile | ZoneId::Command) {
            // CR 400.7 (no zone change), CR 400.5: reposition, same object.
            self.detach(id, dest);
            self.attach(id, dest, req.position);
            if let Some(r) = Arc::make_mut(&mut self.objects).get_mut(&id) {
                r.face_down = req.face_down;
                r.controller = controller;
            }
            return Ok(Moved {
                old: id,
                new: id,
                from: dest,
                to: dest,
            });
        }
        // CR 400.7 (zone change), CR 400.8, CR 406.7, CR 400.10 (same-zone exile or command):
        // the moved object is a new object with a new id.
        let (probe, new) = self.issue_fresh(ids)?;
        self.commit_issued(ids, probe, new);
        self.remove_object(record);
        let to = self.place(new, record.card, record.owner, controller, &req);
        Ok(Moved {
            old: id,
            new,
            from: record.zone,
            to,
        })
    }

    /// Removes the object `id` from the game: the object ceases to exist and its card goes to
    /// its owner's outside-the-game `pool` (REQ-ZONE-001 item 9, CR 400.11). Returns the card.
    ///
    /// # Errors
    /// [`ZoneError::UnknownObject`]; the container is then unchanged.
    pub fn send_outside(&mut self, id: ObjectId, pool: OutsidePool) -> Result<CardId, ZoneError> {
        let record = self.record(id)?;
        self.remove_object(record);
        self.insert_outside(record.owner, pool, record.card);
        Ok(record.card)
    }

    fn insert_outside(&mut self, owner: PlayerId, pool: OutsidePool, card: CardId) {
        let players = Arc::make_mut(&mut self.players);
        if let Some(pz) = players.get_mut(&owner) {
            Arc::make_mut(&mut pz.outside)
                .entry(pool)
                .or_default()
                .insert(card);
            Arc::make_mut(&mut self.outside_index).insert(card, (owner, pool));
        }
    }

    /// Brings `card` from `owner`'s outside-the-game `pool` into the zone `req` names as a new
    /// object with a fresh id (REQ-ZONE-001 item 9, CR 400.11, CR 400.7).
    ///
    /// `ids` must be the generator that issued every id this container ever held; a generator
    /// that is behind it (stale, saved or fresh) is rejected.
    ///
    /// # Errors
    /// [`ZoneError::UnknownPlayer`], [`ZoneError::NoSuchOutsideCard`] if the card is not in that
    /// pool, [`ZoneError::ControllerNotApplicable`], [`ZoneError::IdExhausted`],
    /// [`ZoneError::IdCollision`] if `ids` issues an id that is not above the highest id this
    /// container has issued or accepted (a high-water rejection: it can reject an id the
    /// container never held). The container and `ids` are then unchanged.
    pub fn enter_from_outside(
        &mut self,
        ids: &mut IdGenerator<ObjectId>,
        owner: PlayerId,
        card: CardId,
        pool: OutsidePool,
        req: MoveRequest,
    ) -> Result<ObjectId, ZoneError> {
        self.require_player(owner)?;
        if self.outside_index.get(&card) != Some(&(owner, pool)) {
            return Err(ZoneError::NoSuchOutsideCard(card));
        }
        let controller = self.resolve_controller(&req, owner)?;
        let (probe, id) = self.issue_fresh(ids)?;
        self.commit_issued(ids, probe, id);
        let players = Arc::make_mut(&mut self.players);
        if let Some(pz) = players.get_mut(&owner) {
            let pools = Arc::make_mut(&mut pz.outside);
            if let Some(set) = pools.get_mut(&pool) {
                set.remove(&card);
                if set.is_empty() {
                    pools.remove(&pool);
                }
            }
        }
        Arc::make_mut(&mut self.outside_index).remove(&card);
        self.place(id, card, owner, controller, &req);
        Ok(id)
    }

    /// Puts `card` into `owner`'s outside-the-game `pool` during setup (REQ-ZONE-001 item 9,
    /// CR 400.11a).
    ///
    /// # Errors
    /// [`ZoneError::UnknownPlayer`], [`ZoneError::CardAlreadyPresent`] if the card is in any
    /// zone or pool. The container is then unchanged.
    pub fn add_outside_card(
        &mut self,
        owner: PlayerId,
        pool: OutsidePool,
        card: CardId,
    ) -> Result<(), ZoneError> {
        self.require_player(owner)?;
        if self.card_present(card) {
            return Err(ZoneError::CardAlreadyPresent(card));
        }
        self.insert_outside(owner, pool, card);
        Ok(())
    }

    /// The cards in `owner`'s outside-the-game `pool`, ascending; empty if none or the player is
    /// unknown (REQ-ZONE-001 item 9).
    #[must_use]
    pub fn outside_cards(&self, owner: PlayerId, pool: OutsidePool) -> Vec<CardId> {
        self.players
            .get(&owner)
            .and_then(|pz| pz.outside.get(&pool))
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Sets the controller of the object `id` on the battlefield or the stack (REQ-ZONE-001,
    /// CR 109.4). Elsewhere it is rejected as a container rule (emblems, CR 109.4c, are not
    /// modelled yet).
    ///
    /// # Errors
    /// [`ZoneError::UnknownObject`], [`ZoneError::ControllerNotApplicable`] if the object is in
    /// another zone, [`ZoneError::UnknownPlayer`]. The container is then unchanged.
    pub fn set_controller(&mut self, id: ObjectId, controller: PlayerId) -> Result<(), ZoneError> {
        let record = self.record(id)?;
        if !matches!(record.zone, ZoneId::Battlefield | ZoneId::Stack) {
            return Err(ZoneError::ControllerNotApplicable);
        }
        self.require_player(controller)?;
        if let Some(r) = Arc::make_mut(&mut self.objects).get_mut(&id) {
            r.controller = controller;
        }
        Ok(())
    }

    /// Replaces the order of `player`'s graveyard with `new_top_first` (REQ-ZONE-001 item 4),
    /// allowed only when [`ZonesConfig::free_graveyard_reorder`] is set (the CR 404.2
    /// tournament option; otherwise CR 400.5 forbids it). No new object ids are created.
    ///
    /// # Errors
    /// [`ZoneError::ReorderNotAllowed`] if the flag is off (checked first, even for an
    /// unchanged order), [`ZoneError::UnknownPlayer`], [`ZoneError::InvalidOrder`] if
    /// `new_top_first` is not a permutation of exactly the current contents. The container is
    /// then unchanged.
    pub fn reorder_graveyard(
        &mut self,
        player: PlayerId,
        new_top_first: Vec<ObjectId>,
    ) -> Result<(), ZoneError> {
        // CR 404.2, CR 400.5
        if !self.config.free_graveyard_reorder {
            return Err(ZoneError::ReorderNotAllowed);
        }
        let current = self
            .players
            .get(&player)
            .ok_or(ZoneError::UnknownPlayer(player))?;
        let mut a: Vec<ObjectId> = current.graveyard.to_vec();
        let mut b = new_top_first.clone();
        a.sort_unstable();
        b.sort_unstable();
        if a != b {
            return Err(ZoneError::InvalidOrder);
        }
        let players = Arc::make_mut(&mut self.players);
        if let Some(pz) = players.get_mut(&player) {
            pz.graveyard = Arc::new(new_top_first);
        }
        Ok(())
    }

    /// The record of the object `id`, if it is in a zone.
    #[must_use]
    pub fn object(&self, id: ObjectId) -> Option<&ObjectRecord> {
        self.objects.get(&id)
    }

    /// The current object of `card`, if the card is in a zone (REQ-CORE-002).
    #[must_use]
    pub fn object_of_card(&self, card: CardId) -> Option<ObjectId> {
        self.cards.get(&card).copied()
    }

    /// The objects in `zone`: top first for ordered zones (CR 400.5), ascending ObjectId for
    /// unordered zones; empty for an unknown player's zone (REQ-ZONE-001).
    #[must_use]
    pub fn contents(&self, zone: ZoneId) -> Vec<ObjectId> {
        match zone {
            ZoneId::Library(p) => self
                .players
                .get(&p)
                .map(|pz| pz.library.to_vec())
                .unwrap_or_default(),
            ZoneId::Hand(p) => self
                .players
                .get(&p)
                .map(|pz| pz.hand.iter().copied().collect())
                .unwrap_or_default(),
            ZoneId::Graveyard(p) => self
                .players
                .get(&p)
                .map(|pz| pz.graveyard.to_vec())
                .unwrap_or_default(),
            ZoneId::Battlefield => self.battlefield.iter().copied().collect(),
            ZoneId::Stack => self.stack.to_vec(),
            ZoneId::Exile => self.exile.iter().copied().collect(),
            ZoneId::Command => self.command.iter().copied().collect(),
        }
    }

    /// The number of objects in `zone`; any player may count any zone (REQ-ZONE-001, CR 401.3,
    /// CR 402.3).
    #[must_use]
    pub fn len(&self, zone: ZoneId) -> usize {
        match zone {
            ZoneId::Library(p) => self.players.get(&p).map_or(0, |pz| pz.library.len()),
            ZoneId::Hand(p) => self.players.get(&p).map_or(0, |pz| pz.hand.len()),
            ZoneId::Graveyard(p) => self.players.get(&p).map_or(0, |pz| pz.graveyard.len()),
            ZoneId::Battlefield => self.battlefield.len(),
            ZoneId::Stack => self.stack.len(),
            ZoneId::Exile => self.exile.len(),
            ZoneId::Command => self.command.len(),
        }
    }

    /// True when `zone` holds no object.
    #[must_use]
    pub fn is_empty(&self, zone: ZoneId) -> bool {
        self.len(zone) == 0
    }

    /// Every ObjectId held anywhere in the container (records, zone contents and the card
    /// index), ascending and without duplicates (REQ-CORE-002).
    #[must_use]
    pub fn all_object_ids(&self) -> Vec<ObjectId> {
        let mut all: BTreeSet<ObjectId> = self.objects.keys().copied().collect();
        all.extend(self.cards.values().copied());
        for pz in self.players.values() {
            all.extend(pz.library.iter().copied());
            all.extend(pz.hand.iter().copied());
            all.extend(pz.graveyard.iter().copied());
        }
        all.extend(self.battlefield.iter().copied());
        all.extend(self.stack.iter().copied());
        all.extend(self.exile.iter().copied());
        all.extend(self.command.iter().copied());
        all.into_iter().collect()
    }

    /// Whether the object `id` is public information: its zone is public and it is not face
    /// down (REQ-ZONE-001, CR 400.2, CR 406.3). `None` if the object is unknown.
    #[must_use]
    pub fn is_public_info(&self, id: ObjectId) -> Option<bool> {
        self.objects
            .get(&id)
            .map(|r| r.zone.kind().is_public() && !r.face_down)
    }

    /// Encodes the records of `ids` (in that order) with a length prefix.
    fn encode_zone<'a>(
        &self,
        kind: ZoneKind,
        ids: impl ExactSizeIterator<Item = &'a ObjectId>,
        out: &mut Vec<u8>,
    ) {
        put_variant(out, kind.tag());
        put_len(out, ids.len());
        for id in ids {
            id.encode_canonical(out);
            if let Some(r) = self.objects.get(id) {
                r.card.encode_canonical(out);
                r.owner.encode_canonical(out);
                r.controller.encode_canonical(out);
                r.face_down.encode_canonical(out);
            }
        }
    }
}

impl CanonicalEncode for Zones {
    /// The configuration flag; then each player in ascending order (id, library, hand,
    /// graveyard, outside pools); then the battlefield, stack, exile and command zones. Each
    /// zone is a kind tag, a length and, in contents order, each object's id, card, owner,
    /// controller and face-down flag. Each player's pools are a count and, in ascending pool
    /// order, a pool tag and the ascending card set; last the id high-water mark as 8 bytes
    /// little-endian (REQ-NFR-006, REQ-CORE-002).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.config.free_graveyard_reorder.encode_canonical(out);
        put_len(out, self.players.len());
        for (player, pz) in self.players.iter() {
            player.encode_canonical(out);
            self.encode_zone(ZoneKind::Library, pz.library.iter(), out);
            self.encode_zone(ZoneKind::Hand, pz.hand.iter(), out);
            self.encode_zone(ZoneKind::Graveyard, pz.graveyard.iter(), out);
            put_len(out, pz.outside.len());
            for (pool, cards) in pz.outside.iter() {
                put_variant(out, pool.tag());
                cards.encode_canonical(out);
            }
        }
        self.encode_zone(ZoneKind::Battlefield, self.battlefield.iter(), out);
        self.encode_zone(ZoneKind::Stack, self.stack.iter(), out);
        self.encode_zone(ZoneKind::Exile, self.exile.iter(), out);
        self.encode_zone(ZoneKind::Command, self.command.iter(), out);
        self.max_issued.encode_canonical(out);
    }
}
