// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Last known information: capture, storage and the choice between current information and
//! LKI (REQ-CORE-003).

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::encode::{CanonicalEncode, put_bytes, put_variant};
use fineprint_base::ids::{CardId, ObjectId, PlayerId};
use serde::Serialize;

use super::error::MemoryError;
use super::record::{Memory, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::{Permanent, Permanents};
use crate::zones::{MoveRequest, ObjectRecord, ZoneId, ZoneKind, Zones};

/// The source of the fully evaluated state of objects that LKI capture snapshots
/// (REQ-CORE-003, REQ-LAYER-001).
///
/// A rules caller MUST supply the characteristics evaluated through layers 1a-7d (CR 613.1-613.4,
/// CR 608.2h), for example [`LayeredProvider`](crate::layers::LayeredProvider) over a snapshot
/// of the game state (the recipe is in the [module documentation](crate::memory)). An
/// implementation that returns base characteristics is for unit fakes only.
pub trait CharacteristicsProvider {
    /// The fully evaluated characteristics of `id` as of now (CR 608.2h); `None` means unknown.
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics>;

    /// The counters on `id`: counter kind to amount, amounts at least 1 (CR 608.2h). Default:
    /// none.
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        let _ = id;
        BTreeMap::new()
    }

    /// The objects attached to `id` (CR 608.2h). Default: none.
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        let _ = id;
        Vec::new()
    }

    /// The controller of `id` after layer 2 (CR 613.1b) as of now (REQ-LAYER-001). `None`
    /// means the provider names none, and the caller falls back to the stored controller (the
    /// permanent's or the zone record's). Default: `None`, so a base-only provider keeps the
    /// stored-controller behaviour.
    fn controller(&self, id: ObjectId) -> Option<PlayerId> {
        let _ = id;
        None
    }
}

/// The last known information of one object: the object as it last existed in the zone it
/// left, captured immediately before the event (REQ-CORE-003, CR 603.10, CR 608.2h, glossary
/// "Last Known Information").
///
/// Immutable: every field is private, there are only getters, and later changes to the zones,
/// the permanents, the provider or the Memory table never change a captured record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LkiRecord {
    object: ObjectId,
    card: CardId,
    owner: PlayerId,
    controller: Option<PlayerId>,
    zone: ZoneId,
    face_down: bool,
    characteristics: Characteristics,
    counters: BTreeMap<String, i64>,
    attached_here: Vec<ObjectId>,
    permanent: Option<Permanent>,
    memory: Memory,
}

impl LkiRecord {
    /// The id the object had in the zone it left (CR 400.7).
    #[must_use]
    pub fn object(&self) -> ObjectId {
        self.object
    }

    /// The physical card.
    #[must_use]
    pub fn card(&self) -> CardId {
        self.card
    }

    /// The owner.
    #[must_use]
    pub fn owner(&self) -> PlayerId {
        self.owner
    }

    /// The rules-level controller it had (CR 109.4): `None` outside the battlefield and the
    /// stack. On the battlefield it is the controller after layer 2 (CR 613.1b) as it last
    /// existed there, as the provider evaluated it at capture time, else the stored controller;
    /// this is the controller a look-back trigger reads (CR 603.10a, CR 603.3a, CR 701.21a). On
    /// the stack it is the stored controller.
    #[must_use]
    pub fn controller(&self) -> Option<PlayerId> {
        self.controller
    }

    /// The zone it was in (CR 608.2h).
    #[must_use]
    pub fn zone(&self) -> ZoneId {
        self.zone
    }

    /// Whether it was face down.
    #[must_use]
    pub fn face_down(&self) -> bool {
        self.face_down
    }

    /// Its fully evaluated characteristics (CR 608.2h).
    #[must_use]
    pub fn characteristics(&self) -> &Characteristics {
        &self.characteristics
    }

    /// Its counters: kind to amount, ascending kind, amounts at least 1 (CR 608.2h).
    #[must_use]
    pub fn counters(&self) -> &BTreeMap<String, i64> {
        &self.counters
    }

    /// The objects that were attached to it, ascending (CR 608.2h).
    #[must_use]
    pub fn attached_here(&self) -> &[ObjectId] {
        &self.attached_here
    }

    /// Its permanent state (status, designations, attachment, combat): `Some` only if it was on
    /// the battlefield and known to the permanent container (CR 608.2h).
    #[must_use]
    pub fn permanent(&self) -> Option<&Permanent> {
        self.permanent.as_ref()
    }

    /// A snapshot of its Memory; empty if it had none (REQ-CORE-003, REQ-CORE-007).
    #[must_use]
    pub fn memory(&self) -> &Memory {
        &self.memory
    }
}

/// Appends a deterministic encoding of a serde value: tag 0 and its RFC 8785 canonical JSON;
/// if that is impossible (for example an integer outside the JSON safe range), tag 1 and its
/// MessagePack form; tag 2 if neither exists. Equal values give equal bytes.
fn put_serde<T: Serialize>(out: &mut Vec<u8>, value: &T) {
    if let Ok(bytes) = fineprint_dsl::canonical::to_canonical_vec(value) {
        put_variant(out, 0);
        put_bytes(out, &bytes);
    } else if let Ok(bytes) = rmp_serde::to_vec(value) {
        put_variant(out, 1);
        put_bytes(out, &bytes);
    } else {
        put_variant(out, 2);
    }
}

/// Appends the canonical encoding of `c` (decision "#24 FP-023 — Characteristics encoding"):
/// each field in declaration order; sets and integers through their own canonical encoding,
/// the mana cost as its canonical text, color masks as their bits, and the placeholder DSL
/// trees (rules text, abilities, power, toughness) through [`put_serde`].
fn encode_characteristics(c: &Characteristics, out: &mut Vec<u8>) {
    c.names.encode_canonical(out);
    c.mana_cost
        .as_ref()
        .map(ToString::to_string)
        .encode_canonical(out);
    c.color_indicator.map(|m| m.bits()).encode_canonical(out);
    c.colors.bits().encode_canonical(out);
    c.supertypes.encode_canonical(out);
    c.card_types.encode_canonical(out);
    c.subtypes.encode_canonical(out);
    put_serde(out, &c.rules_text);
    put_serde(out, &c.abilities);
    put_serde(out, &c.power);
    put_serde(out, &c.toughness);
    c.loyalty.encode_canonical(out);
    c.defense.encode_canonical(out);
    c.hand_modifier.encode_canonical(out);
    c.life_modifier.encode_canonical(out);
}

/// Appends the canonical encoding of a zone id: the zone kind tag, then the owner of a
/// per-player zone (`Option`).
fn encode_zone_id(zone: ZoneId, out: &mut Vec<u8>) {
    put_variant(out, zone.kind().tag());
    zone.owner().encode_canonical(out);
}

impl CanonicalEncode for LkiRecord {
    /// The object id, card, owner, controller (`Option`), zone (kind tag and `Option` owner),
    /// face-down flag, characteristics (see the "Characteristics encoding" decision), counters,
    /// attached objects, permanent state (`Option`) and Memory, in that order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.object.encode_canonical(out);
        self.card.encode_canonical(out);
        self.owner.encode_canonical(out);
        self.controller.encode_canonical(out);
        encode_zone_id(self.zone, out);
        self.face_down.encode_canonical(out);
        encode_characteristics(&self.characteristics, out);
        self.counters.encode_canonical(out);
        self.attached_here.encode_canonical(out);
        self.permanent.encode_canonical(out);
        self.memory.encode_canonical(out);
    }
}

/// Every captured LKI record, keyed by the id the object had when it left its zone
/// (REQ-CORE-003). Records are never overwritten or removed here: an object id names one zone
/// residency and is never reused (CR 400.7).
///
/// Cloning is a cheap snapshot (`Arc` + `Arc::make_mut`); mutating a clone never changes the
/// original.
///
/// **Omniscient state.** Records can describe objects that went to a hidden zone and face-down
/// objects; the store stays behind the redacted per-player view (REQ-CORE-004).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LkiStore {
    records: Arc<BTreeMap<ObjectId, Arc<LkiRecord>>>,
}

impl LkiStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> LkiStore {
        LkiStore::default()
    }

    /// The record captured for `id`, if any (CR 608.2h).
    #[must_use]
    pub fn get(&self, id: ObjectId) -> Option<&LkiRecord> {
        self.records.get(&id).map(AsRef::as_ref)
    }

    /// The number of records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the store holds no record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The ids with a record, ascending.
    #[must_use]
    pub fn ids(&self) -> Vec<ObjectId> {
        self.records.keys().copied().collect()
    }
}

impl CanonicalEncode for LkiStore {
    /// The number of records, then each id ascending with its record (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        fineprint_base::encode::put_len(out, self.records.len());
        for (id, record) in self.records.iter() {
            id.encode_canonical(out);
            record.encode_canonical(out);
        }
    }
}

/// Captures the last known information of `id` before `req` moves it (REQ-CORE-003,
/// CR 603.10, CR 608.2h). Returns whether a record was stored.
///
/// **Call this BEFORE [`Zones::move_object`]**: afterwards the old object is gone. A record is
/// captured iff the request makes the object leave its zone: the destination
/// (`ZoneId::of(req.to, owner)`) differs from its zone, or the move is exile to exile or command
/// to command, where it becomes a new object (CR 400.8, CR 406.7, CR 400.10). A same-zone
/// reposition in a library, hand, graveyard, the battlefield or the stack is not a zone change
/// (CR 400.7): `Ok(false)`, nothing stored. Every zone exit is covered, including a move from a
/// public zone to a hidden zone (SRS 2.3).
///
/// The record holds, all as of now: the characteristics, counters and attached objects from
/// `provider`, the rules-level controller (see below), zone and face-down flag from `zones`, the permanent
/// state from `permanents` (battlefield only), and a snapshot of the object's Memory from
/// `memories` (empty if none).
///
/// **Controller** (REQ-CORE-003, CR 613.1b, CR 603.10a, CR 701.21a, CR 603.3a): for an object
/// leaving the battlefield, the layer-evaluated controller from
/// [`CharacteristicsProvider::controller`], falling back to the zone record's controller when
/// the provider names none; for any other zone, the zone record's rules-level controller
/// (CR 109.4: the stack object's controller, `None` elsewhere). The destination stays
/// owner-based (`ZoneId::of(req.to, owner)`, CR 400.3, CR 108.4a) whoever controlled it.
///
/// **Simultaneous departures** (CR 704.8: when state-based actions make several permanents
/// leave at once, their LKI comes from the state before any of those actions): the caller
/// captures every departing object first, and only then moves them.
///
/// # Errors
/// [`MemoryError::UnknownObject`] if `id` is not in a zone, [`MemoryError::AlreadyCaptured`] if
/// a record for `id` exists (records are never overwritten), [`MemoryError::NoCharacteristics`]
/// if `provider` knows no characteristics for `id`. The store is then unchanged.
pub fn capture_lki_on_move(
    zones: &Zones,
    permanents: &Permanents,
    memories: &MemoryTable,
    provider: &dyn CharacteristicsProvider,
    lki: &mut LkiStore,
    id: ObjectId,
    req: &MoveRequest,
) -> Result<bool, MemoryError> {
    let record = zones.object(id).ok_or(MemoryError::UnknownObject(id))?;
    let dest = ZoneId::of(req.to, record.owner);
    // CR 400.7: only a move from one zone to another; CR 400.8, CR 406.7, CR 400.10: exile and
    // the command zone make a new object even without a zone change.
    let leaves = dest != record.zone || matches!(dest, ZoneId::Exile | ZoneId::Command);
    if !leaves {
        return Ok(false);
    }
    if lki.records.contains_key(&id) {
        return Err(MemoryError::AlreadyCaptured(id));
    }
    // CR 608.2h: the object as it last existed in its zone.
    let characteristics = provider
        .characteristics(id)
        .ok_or(MemoryError::NoCharacteristics(id))?;
    let counters = provider
        .counters(id)
        .into_iter()
        .filter(|&(_, n)| n >= 1)
        .collect();
    let mut attached_here = provider.attached_here(id);
    attached_here.sort_unstable();
    attached_here.dedup();
    let permanent = if record.zone == ZoneId::Battlefield {
        permanents.get(id).cloned()
    } else {
        None
    };
    // CR 613.1b, CR 603.10a, CR 701.21a: a permanent's controller as it last existed on the
    // battlefield is the one after layer 2; the stored controller is the fallback. Any other
    // zone keeps the rules-level controller (CR 109.4).
    let controller = if record.zone == ZoneId::Battlefield {
        provider
            .controller(id)
            .or_else(|| record.rules_controller())
    } else {
        record.rules_controller()
    };
    let lki_record = LkiRecord {
        object: id,
        card: record.card,
        owner: record.owner,
        controller,
        zone: record.zone,
        face_down: record.face_down,
        characteristics,
        counters,
        attached_here,
        permanent,
        memory: memories.get(id).cloned().unwrap_or_default(),
    };
    Arc::make_mut(&mut lki.records).insert(id, Arc::new(lki_record));
    Ok(true)
}

/// The information an effect uses about an object (REQ-CORE-003, CR 608.2h).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Information<'a> {
    /// The object's current information: it is still in the zone the effect expects.
    Current(&'a ObjectRecord),
    /// The object's last known information.
    Lki(&'a LkiRecord),
}

/// Who asks for an object's information (REQ-CORE-003, CR 604.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InformationUser {
    /// A spell or a non-static ability, which can use last known information (CR 608.2h).
    SpellOrAbility,
    /// A static ability, which can't use last known information (CR 604.7).
    StaticAbility,
}

/// The information to use about the object `id` that an effect holds (REQ-CORE-003,
/// CR 608.2h, CR 113.7a, CR 608.2b).
///
/// `Current` iff `id` is in a zone of kind `expected` (an object that moved has a new id, so
/// the old id is no longer current, CR 400.7). Otherwise the LKI of `id` if one was captured and
/// `user` is [`InformationUser::SpellOrAbility`]; a static ability never gets LKI (CR 604.7).
/// `None` if neither applies.
///
/// **Omniscient state.** The result can expose hidden information; callers stay behind the
/// redacted view (REQ-CORE-004).
#[must_use]
pub fn information_for<'a>(
    zones: &'a Zones,
    lki: &'a LkiStore,
    id: ObjectId,
    expected: ZoneKind,
    user: InformationUser,
) -> Option<Information<'a>> {
    // CR 608.2h: current information while it is in the public zone it was expected to be in.
    if let Some(record) = zones.object(id).filter(|r| r.zone.kind() == expected) {
        return Some(Information::Current(record));
    }
    match user {
        // CR 608.2h, CR 113.7a, CR 608.2b: otherwise its last known information.
        InformationUser::SpellOrAbility => lki.get(id).map(Information::Lki),
        // CR 604.7: static abilities can't use last known information.
        InformationUser::StaticAbility => None,
    }
}

#[cfg(test)]
mod evaluated_controller_tests {
    // Unit tests of the evaluated controller in LKI capture: REQ-CORE-003, REQ-LAYER-001
    // (CR 613.1b, CR 603.10a, CR 701.21a, CR 603.3a, CR 400.3, CR 108.4a). Synthetic ids and
    // synthetic characteristics only.

    use std::collections::{BTreeMap, BTreeSet};

    use fineprint_base::encode::encode_to_vec;
    use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
    use fineprint_dsl::ability::Layer;
    use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
    use fineprint_dsl::expr::ObjectSelectorAST;

    use super::*;
    use crate::layers::{
        Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment,
        LayerModification, LayeredProvider, MatchContext, ObjectBase,
    };
    use crate::objects::Characteristics;
    use crate::permanent::Permanents;
    use crate::zones::{MoveRequest, ZoneId, ZoneKind, Zones, ZonesConfig};

    fn p(n: u64) -> PlayerId {
        PlayerId::new(n)
    }

    fn chars() -> Characteristics {
        Characteristics {
            names: BTreeSet::from(["test.saga_like_0_0".to_owned()]),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::from(["Enchantment".to_owned()]),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream::default(),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        }
    }

    /// A fake provider whose controller answer is fixed per object.
    #[derive(Default)]
    struct Fake {
        controllers: BTreeMap<ObjectId, PlayerId>,
    }

    impl CharacteristicsProvider for Fake {
        fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
            Some(chars())
        }

        fn controller(&self, id: ObjectId) -> Option<PlayerId> {
            self.controllers.get(&id).copied()
        }
    }

    /// A provider using every default, `controller` included.
    struct BaseOnly;

    impl CharacteristicsProvider for BaseOnly {
        fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
            Some(chars())
        }
    }

    struct World {
        zones: Zones,
        ids: IdGenerator<ObjectId>,
        permanents: Permanents,
        memories: MemoryTable,
        lki: LkiStore,
    }

    impl World {
        fn new() -> World {
            World {
                zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
                ids: IdGenerator::new(),
                permanents: Permanents::new(),
                memories: MemoryTable::new(),
                lki: LkiStore::new(),
            }
        }

        /// Card `card` owned by `owner` in a zone of `kind`; on the battlefield and the stack
        /// stored as controlled by `controller`.
        fn create(&mut self, card: u64, owner: u64, controller: u64, kind: ZoneKind) -> ObjectId {
            let mut req = MoveRequest::new(kind);
            if matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack) {
                req.controller = Some(p(controller));
            }
            let id = self
                .zones
                .create_object(&mut self.ids, CardId::new(card), p(owner), req)
                .unwrap();
            if kind == ZoneKind::Battlefield {
                self.permanents.enter(id, p(controller), 1).unwrap();
            }
            id
        }

        fn capture(
            &mut self,
            provider: &dyn CharacteristicsProvider,
            id: ObjectId,
            to: ZoneKind,
        ) -> Result<bool, MemoryError> {
            capture_lki_on_move(
                &self.zones,
                &self.permanents,
                &self.memories,
                provider,
                &mut self.lki,
                id,
                &MoveRequest::new(to),
            )
        }
    }

    /// REQ-CORE-003, REQ-LAYER-001; CR 613.1b, CR 603.10a, CR 701.21a: a permanent stored as
    /// controlled by P1 whose evaluated controller is P2 is recorded with controller P2; owner
    /// and zone are unchanged.
    #[test]
    fn req_core_003_battlefield_lki_uses_evaluated_controller() {
        let mut w = World::new();
        let id = w.create(1, 1, 1, ZoneKind::Battlefield);
        let fake = Fake {
            controllers: BTreeMap::from([(id, p(2))]),
        };
        assert_eq!(w.capture(&fake, id, ZoneKind::Graveyard), Ok(true));
        let rec = w.lki.get(id).unwrap();
        assert_eq!(rec.controller(), Some(p(2)));
        assert_eq!(rec.owner(), p(1));
        assert_eq!(rec.zone(), ZoneId::Battlefield);
        // The permanent snapshot keeps its stored state unchanged.
        assert_eq!(rec.permanent().map(Permanent::controller), Some(p(1)));
    }

    /// REQ-CORE-003, REQ-LAYER-001; CR 613.1b: with no evaluated controller (a fake naming
    /// none, or a provider using the default) the stored controller is recorded.
    #[test]
    fn req_core_003_battlefield_lki_falls_back_to_stored_controller() {
        let mut w = World::new();
        let a = w.create(1, 2, 1, ZoneKind::Battlefield);
        let b = w.create(2, 2, 1, ZoneKind::Battlefield);
        assert_eq!(
            w.capture(&Fake::default(), a, ZoneKind::Graveyard),
            Ok(true)
        );
        assert_eq!(w.capture(&BaseOnly, b, ZoneKind::Exile), Ok(true));
        assert_eq!(w.lki.get(a).unwrap().controller(), Some(p(1)));
        assert_eq!(w.lki.get(b).unwrap().controller(), Some(p(1)));
        assert_eq!(w.lki.get(a).unwrap().owner(), p(2));
        assert_eq!(BaseOnly.controller(a), None);
    }

    /// REQ-CORE-003; CR 603.10a: an object leaving a hand or a graveyard has no controller in
    /// its LKI even if a provider names one; a stack object keeps its stored controller.
    #[test]
    fn req_core_003_non_battlefield_lki_ignores_provider_controller() {
        let mut w = World::new();
        let hand = w.create(1, 1, 1, ZoneKind::Hand);
        let yard = w.create(2, 1, 1, ZoneKind::Graveyard);
        let spell = w.create(3, 1, 2, ZoneKind::Stack);
        let fake = Fake {
            controllers: BTreeMap::from([(hand, p(2)), (yard, p(2)), (spell, p(1))]),
        };
        assert_eq!(w.capture(&fake, hand, ZoneKind::Graveyard), Ok(true));
        assert_eq!(w.capture(&fake, yard, ZoneKind::Exile), Ok(true));
        assert_eq!(w.capture(&fake, spell, ZoneKind::Graveyard), Ok(true));
        assert_eq!(w.lki.get(hand).unwrap().controller(), None);
        assert_eq!(w.lki.get(yard).unwrap().controller(), None);
        assert_eq!(w.lki.get(spell).unwrap().controller(), Some(p(2)));
    }

    /// REQ-CORE-003; CR 400.3, CR 108.4a, CR 701.21a: a permanent owned by P1 under P2's
    /// evaluated control is recorded with controller P2, and its move still lands in P1's
    /// graveyard.
    #[test]
    fn req_core_003_destination_stays_owner_based() {
        let mut w = World::new();
        let id = w.create(1, 1, 1, ZoneKind::Battlefield);
        let fake = Fake {
            controllers: BTreeMap::from([(id, p(2))]),
        };
        let r = MoveRequest::new(ZoneKind::Graveyard);
        assert_eq!(w.capture(&fake, id, ZoneKind::Graveyard), Ok(true));
        let moved = w.zones.move_object(&mut w.ids, id, r).unwrap();
        assert_eq!(moved.to, ZoneId::Graveyard(p(1)));
        assert_eq!(w.lki.get(id).unwrap().controller(), Some(p(2)));
        // A same-zone reposition is still no capture, whatever the provider says.
        let mut w = World::new();
        let id = w.create(1, 1, 1, ZoneKind::Battlefield);
        assert_eq!(w.capture(&fake, id, ZoneKind::Battlefield), Ok(false));
        assert!(w.lki.is_empty());
    }

    /// A layer environment of battlefield objects with a base controller each.
    struct Env {
        bases: BTreeMap<ObjectId, ObjectBase>,
    }

    impl LayerEnvironment for Env {
        fn objects(&self) -> Vec<ObjectId> {
            self.bases.keys().copied().collect()
        }

        fn base(&self, id: ObjectId) -> Option<ObjectBase> {
            self.bases.get(&id).cloned()
        }

        fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
            false
        }
    }

    /// REQ-CORE-003, REQ-LAYER-001; CR 613.1b, CR 701.21a, CR 603.3a: through the
    /// [`LayeredProvider`], a permanent under a layer-2 control effect is recorded with the
    /// evaluated controller (the one a sacrifice trigger reads), and the capture is
    /// deterministic: repeated runs from the same state give byte-identical records (R3).
    #[test]
    fn req_core_003_layered_provider_capture_and_determinism() {
        let run = || {
            let mut w = World::new();
            let id = w.create(1, 1, 1, ZoneKind::Battlefield);
            let env = Env {
                bases: BTreeMap::from([(
                    id,
                    ObjectBase {
                        characteristics: chars(),
                        controller: Some(p(1)),
                        on_battlefield: true,
                    },
                )]),
            };
            let mut effects = ContinuousEffects::new();
            effects
                .register(
                    EffectId::new(1),
                    ContinuousEffect {
                        source: None,
                        controller: p(2),
                        timestamp: Timestamp::new(1),
                        layer: Layer::L2,
                        is_cda: false,
                        origin: EffectOrigin::Resolved,
                        affected: Affected::Objects([id].into_iter().collect()),
                        modification: LayerModification::SetController(p(2)),
                    },
                )
                .unwrap();
            let provider = LayeredProvider::new(&effects, &env);
            assert_eq!(w.capture(&provider, id, ZoneKind::Graveyard), Ok(true));
            assert_eq!(w.lki.get(id).unwrap().controller(), Some(p(2)));
            assert_eq!(w.lki.get(id).unwrap().owner(), p(1));
            encode_to_vec(&w.lki)
        };
        let first = run();
        for _ in 0..4 {
            assert_eq!(run(), first);
        }
    }
}
