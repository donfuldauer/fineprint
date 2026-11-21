// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::sba` spec tests (REQ-SBA-001, REQ-TEST-001,
//! REQ-TURN-001). Black-box: written from the work-item brief only.
//!
//! Each `spec_sba_*.rs` test file includes this file with `#[path]`; on its own it is an empty
//! test target. Every card, object and effect here is synthetic and neutrally named (R5).
//!
//! A [`Sim`] is the whole input of one SBA check: the event-layer world (an `EventContext`
//! over it), the layer environment and continuous effects the SBA module evaluates toughness and
//! loyalty with, the test [`SbaEnvironment`] and the replacement registry. The test keeps the
//! zones, the layer environment and the characteristics provider consistent: every object in a
//! zone is described by the layer environment (with `on_battlefield` matching its zone) and by
//! the provider. `perform_all` needs a `ChoiceContext`, which only a `ChoiceSession` hands out,
//! so every pass runs inside a session ([`run_sba`]).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_core::choice::{
    ChoiceContext, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType, Interrupt,
    PendingChoiceRequest, Status,
};
use fineprint_core::events::{
    CounterTarget, DamageTarget, EventContext, EventState, ExecutedBatch, ExecutedEvent, GameEvent,
    Outcome, ZoneChangeCause,
};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment,
    LayerModification, MatchContext, ObjectBase,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::{AttachTarget, Permanents};
use fineprint_core::replacement::{BusError, BusInterrupt, ReplacementEffect, ReplacementRegistry};
use fineprint_core::sba::{
    GameVariant, LOST_THE_GAME, SbaEnvironment, SbaFinding, SbaOutcome, SbaRegistry, SbaSubject,
    check, perform_all,
};
use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind, Zones, ZonesConfig};
pub use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::ObjectSelectorAST;

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);

pub const STARTING_LIFE: i64 = 20;

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn cid(n: u64) -> CardId {
    CardId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

/// Every SBA entry id, in registry order (CR 704.5a-704.5i, 704.5j, 704.5k, 704.5m, 704.5n,
/// 704.5p, 704.5q, 704.5r, 704.5s, 704.5t). The CR has no 704.5l and no 704.5o.
pub const ENTRY_IDS: [&str; 18] = [
    "704.5a", "704.5b", "704.5c", "704.5d", "704.5e", "704.5f", "704.5g", "704.5h", "704.5i",
    "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r", "704.5s", "704.5t",
];

/// The entry ids added for CR 704.5j-704.5t, in registry order.
pub const NEW_ENTRY_IDS: [&str; 9] = [
    "704.5j", "704.5k", "704.5m", "704.5n", "704.5p", "704.5q", "704.5r", "704.5s", "704.5t",
];

/// The registry position of `entry_id` (panics on an unknown id).
pub fn entry_index(entry_id: &str) -> usize {
    ENTRY_IDS
        .iter()
        .position(|e| *e == entry_id)
        .unwrap_or_else(|| panic!("unknown entry id {entry_id:?}"))
}

// ---------------------------------------------------------------------------------------------
// DSL placeholder values (as in the layers spec-test helpers)
// ---------------------------------------------------------------------------------------------

fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec_named(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

fn try_via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(
    value: &In,
) -> Option<Out> {
    let bytes = rmp_serde::to_vec_named(value).ok()?;
    rmp_serde::from_slice(&bytes).ok()
}

/// `ValueExpressionAST::Literal(n)` in the placeholder form held by `Characteristics`.
pub fn lit(n: i64) -> ValueExpressionAST {
    via_msgpack(&fineprint_dsl::expr::ValueExpressionAST::Literal(n))
}

/// Reads a literal placeholder value (`{"LITERAL": n}` or a bare integer).
pub fn literal_of(v: &ValueExpressionAST) -> Option<i64> {
    if let Some(fineprint_dsl::expr::ValueExpressionAST::Literal(n)) =
        try_via_msgpack::<_, fineprint_dsl::expr::ValueExpressionAST>(v)
    {
        return Some(n);
    }
    try_via_msgpack::<_, i64>(v)
}

/// A keyword ability with the given ability id and keyword.
pub fn kw_ability(id: &str, kw: &str) -> AbilityAST {
    let mut m: BTreeMap<&str, &str> = BTreeMap::new();
    m.insert("kind", "KEYWORD");
    m.insert("abilityId", id);
    m.insert("keyword", kw);
    via_msgpack(&m)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// An object with the given name and card types and nothing else.
pub fn blank(name: &str, types: &[&str]) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(types),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic creature with literal power and toughness.
pub fn creature_chars(name: &str, p: i64, t: i64) -> Characteristics {
    Characteristics {
        power: Some(lit(p)),
        toughness: Some(lit(t)),
        ..blank(name, &["Creature"])
    }
}

/// A synthetic planeswalker (no power or toughness).
pub fn walker_chars(name: &str) -> Characteristics {
    blank(name, &["Planeswalker"])
}

/// `c` with the keyword `kw` as a base ability.
pub fn with_keyword(mut c: Characteristics, kw: &str) -> Characteristics {
    c.abilities.push(kw_ability(&format!("test.base:{kw}"), kw));
    c
}

// ---------------------------------------------------------------------------------------------
// the layer environment
// ---------------------------------------------------------------------------------------------

/// A synthetic `LayerEnvironment` over ordered maps. No selector matches (effects in these tests
/// use locked sets, `Affected::Objects`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerEnv {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl LayerEnvironment for LayerEnv {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, _id: ObjectId, _kind: &str) -> Option<Timestamp> {
        None
    }

    fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
        false
    }
}

/// A resolved layer 7c `ModifyPt` effect on the locked set `targets` (CR 613.4c).
pub fn modify_pt_effect(targets: &[ObjectId], p: i64, t: i64, ts: u64) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: P1,
        timestamp: Timestamp::new(ts),
        layer: Layer::L7c,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected: Affected::Objects(targets.iter().copied().collect()),
        modification: LayerModification::ModifyPt {
            power: p,
            toughness: t,
        },
    }
}

// ---------------------------------------------------------------------------------------------
// the characteristics provider (for LKI capture)
// ---------------------------------------------------------------------------------------------

/// A characteristics provider with per-object characteristics and counters and a fallback.
/// A retired id (one [`TestProvider::forget`] dropped: it left its zone or ceased to exist,
/// CR 400.7) is described by nothing, not even the fallback.
#[derive(Clone, Debug, Default)]
pub struct TestProvider {
    fallback: RefCell<Option<Characteristics>>,
    chars: RefCell<BTreeMap<ObjectId, Characteristics>>,
    counters: RefCell<BTreeMap<ObjectId, BTreeMap<String, i64>>>,
    retired: RefCell<BTreeSet<ObjectId>>,
}

impl TestProvider {
    pub fn with_fallback(fallback: Characteristics) -> TestProvider {
        let p = TestProvider::default();
        *p.fallback.borrow_mut() = Some(fallback);
        p
    }

    pub fn set_chars(&self, id: ObjectId, c: Characteristics) {
        self.chars.borrow_mut().insert(id, c);
    }

    /// Drops the fallback: an object with no characteristics of its own is then undescribed.
    pub fn clear_fallback(&self) {
        *self.fallback.borrow_mut() = None;
    }

    /// Retires `id`: drops its characteristics and counters; afterwards `characteristics(id)` is
    /// `None` and `counters(id)` is empty.
    pub fn forget(&self, id: ObjectId) {
        self.chars.borrow_mut().remove(&id);
        self.counters.borrow_mut().remove(&id);
        self.retired.borrow_mut().insert(id);
    }

    /// Every id with its own characteristics or counters, in id order.
    pub fn described_ids(&self) -> BTreeSet<ObjectId> {
        let mut ids: BTreeSet<ObjectId> = self.chars.borrow().keys().copied().collect();
        ids.extend(self.counters.borrow().keys().copied());
        ids
    }

    pub fn set_counter(&self, id: ObjectId, kind: &str, n: i64) {
        self.counters
            .borrow_mut()
            .entry(id)
            .or_default()
            .insert(kind.to_string(), n);
    }
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        if self.retired.borrow().contains(&id) {
            return None;
        }
        if let Some(c) = self.chars.borrow().get(&id) {
            return Some(c.clone());
        }
        self.fallback.borrow().clone()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.borrow().get(&id).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// the SBA environment
// ---------------------------------------------------------------------------------------------

/// A test `SbaEnvironment` over ordered sets; counts `clear_deathtouch_damaged` calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestSbaEnv {
    pub variant: GameVariant,
    pub spell_copies: BTreeSet<ObjectId>,
    pub card_copies: BTreeSet<ObjectId>,
    pub exempt: BTreeSet<ObjectId>,
    pub deathtouch: BTreeSet<ObjectId>,
    pub clear_calls: u64,
    /// CR 704.5k: the stamp at which each object gained the world supertype (larger = later
    /// = shorter time); missing = 0.
    pub world_since: BTreeMap<ObjectId, u64>,
    /// CR 704.5m, 303.4c: `(aura, target)` pairs its enchant ability makes illegal.
    pub illegal_aura_targets: BTreeSet<(ObjectId, AttachTarget)>,
    /// CR 704.5r: per object, the "can't have more than N counters of kind" limits.
    pub counter_limits: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    /// CR 704.5s, 715.2d: final chapter numbers of objects with chapter abilities.
    pub saga_final_chapter: BTreeMap<ObjectId, i64>,
    /// CR 704.5s: Sagas that are the source of a triggered chapter ability not yet gone.
    pub chapter_pending: BTreeSet<ObjectId>,
    /// CR 704.5t: dungeons whose venture marker is on the bottommost room.
    pub bottommost_dungeons: BTreeSet<ObjectId>,
    /// CR 704.5t: dungeons that are the source of a triggered room ability not yet gone.
    pub room_pending: BTreeSet<ObjectId>,
}

impl Default for TestSbaEnv {
    fn default() -> TestSbaEnv {
        TestSbaEnv {
            variant: GameVariant::Standard,
            spell_copies: BTreeSet::new(),
            card_copies: BTreeSet::new(),
            exempt: BTreeSet::new(),
            deathtouch: BTreeSet::new(),
            clear_calls: 0,
            world_since: BTreeMap::new(),
            illegal_aura_targets: BTreeSet::new(),
            counter_limits: BTreeMap::new(),
            saga_final_chapter: BTreeMap::new(),
            chapter_pending: BTreeSet::new(),
            bottommost_dungeons: BTreeSet::new(),
            room_pending: BTreeSet::new(),
        }
    }
}

impl SbaEnvironment for TestSbaEnv {
    fn variant(&self) -> GameVariant {
        self.variant
    }

    fn is_spell_copy(&self, object: ObjectId) -> bool {
        self.spell_copies.contains(&object)
    }

    fn is_card_copy(&self, object: ObjectId) -> bool {
        self.card_copies.contains(&object)
    }

    fn exempt_from_copy_sba(&self, object: ObjectId) -> bool {
        self.exempt.contains(&object)
    }

    fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
        self.deathtouch.clone()
    }

    fn clear_deathtouch_damaged(&mut self) {
        self.deathtouch.clear();
        self.clear_calls += 1;
    }

    fn world_supertype_since(&self, object: ObjectId) -> u64 {
        self.world_since.get(&object).copied().unwrap_or(0)
    }

    fn aura_can_enchant(&self, aura: ObjectId, target: AttachTarget) -> bool {
        !self.illegal_aura_targets.contains(&(aura, target))
    }

    fn counter_limits(&self, object: ObjectId) -> BTreeMap<String, i64> {
        self.counter_limits
            .get(&object)
            .cloned()
            .unwrap_or_default()
    }

    fn saga_final_chapter(&self, object: ObjectId) -> Option<i64> {
        self.saga_final_chapter.get(&object).copied()
    }

    fn chapter_ability_pending(&self, object: ObjectId) -> bool {
        self.chapter_pending.contains(&object)
    }

    fn dungeons_at_bottommost_room(&self) -> BTreeSet<ObjectId> {
        self.bottommost_dungeons.clone()
    }

    fn room_ability_pending(&self, dungeon: ObjectId) -> bool {
        self.room_pending.contains(&dungeon)
    }
}

impl TestSbaEnv {
    fn canonical(&self, out: &mut Vec<u8>) {
        let variant: u8 = match self.variant {
            GameVariant::Standard => 0,
            GameVariant::TwoHeadedGiant => 1,
        };
        out.push(variant);
        for s in [
            &self.spell_copies,
            &self.card_copies,
            &self.exempt,
            &self.deathtouch,
        ] {
            out.extend((s.len() as u64).to_le_bytes());
            for id in s {
                out.extend(id.get().to_le_bytes());
            }
        }
        out.extend(self.clear_calls.to_le_bytes());
        out.extend((self.world_since.len() as u64).to_le_bytes());
        for (id, since) in &self.world_since {
            out.extend(id.get().to_le_bytes());
            out.extend(since.to_le_bytes());
        }
        out.extend((self.illegal_aura_targets.len() as u64).to_le_bytes());
        for (aura, target) in &self.illegal_aura_targets {
            out.extend(aura.get().to_le_bytes());
            out.extend(bytes(target));
        }
        out.extend((self.counter_limits.len() as u64).to_le_bytes());
        for (id, limits) in &self.counter_limits {
            out.extend(id.get().to_le_bytes());
            out.extend((limits.len() as u64).to_le_bytes());
            for (kind, n) in limits {
                out.extend((kind.len() as u64).to_le_bytes());
                out.extend(kind.as_bytes());
                out.extend(n.to_le_bytes());
            }
        }
        out.extend((self.saga_final_chapter.len() as u64).to_le_bytes());
        for (id, n) in &self.saga_final_chapter {
            out.extend(id.get().to_le_bytes());
            out.extend(n.to_le_bytes());
        }
        for s in [
            &self.chapter_pending,
            &self.bottommost_dungeons,
            &self.room_pending,
        ] {
            out.extend((s.len() as u64).to_le_bytes());
            for id in s {
                out.extend(id.get().to_le_bytes());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the simulation
// ---------------------------------------------------------------------------------------------

/// The whole input of one SBA check (see the module documentation).
#[derive(Clone)]
pub struct Sim {
    pub game: GameState,
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Permanents,
    pub memories: MemoryTable,
    pub lki: LkiStore,
    pub events: EventState,
    pub provider: TestProvider,
    pub layer: LayerEnv,
    pub effects: ContinuousEffects,
    pub env: TestSbaEnv,
    pub replacements: ReplacementRegistry,
    next_card: u64,
}

impl Default for Sim {
    fn default() -> Sim {
        Sim::new()
    }
}

/// A comparable snapshot of a whole [`Sim`] (except the immutable layer inputs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snap {
    pub game: GameState,
    pub zones: Vec<u8>,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Vec<u8>,
    pub memories: Vec<u8>,
    pub lki: Vec<u8>,
    pub events: Vec<u8>,
    pub env: TestSbaEnv,
    pub replacements: Vec<u8>,
    pub empty_draws: Vec<PlayerId>,
}

/// The canonical bytes of `v`.
pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

impl Sim {
    /// A two-player game at turn 1, player 1 active.
    pub fn new() -> Sim {
        Sim::with_players(2)
    }

    /// A game with `n` players at turn 1, player 1 active.
    pub fn with_players(n: usize) -> Sim {
        let mut game = GameState::new(0x5EED_0040, n, STARTING_LIFE).expect("valid players");
        game.set_turn_number(1);
        game.set_active_player(Some(P1)).expect("known player");
        let players: Vec<PlayerId> = game.turn_order().to_vec();
        Sim {
            game,
            zones: Zones::new(&players, ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            provider: TestProvider::with_fallback(creature_chars("test.vanilla_1_1", 1, 1)),
            layer: LayerEnv::default(),
            effects: ContinuousEffects::new(),
            env: TestSbaEnv::default(),
            replacements: ReplacementRegistry::new(),
            next_card: 1,
        }
    }

    /// The event context over the world components.
    pub fn ctx(&mut self) -> EventContext<'_> {
        EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &self.provider,
        }
    }

    /// Test setup through the plain event layer: executes `event`, which must occur.
    pub fn ok(&mut self, event: GameEvent) -> ExecutedEvent {
        match self.ctx().execute(event.clone()) {
            Ok(Some(done)) => done,
            other => panic!("expected {event:?} to occur, got {other:?}"),
        }
    }

    pub fn fresh_card(&mut self) -> CardId {
        let c = cid(self.next_card);
        self.next_card += 1;
        c
    }

    /// Registers `id` with the layer environment and the provider.
    pub fn describe(&mut self, id: ObjectId, c: Characteristics) {
        let rec = self.zones.object(id).expect("in a zone");
        let on_battlefield = rec.zone == ZoneId::Battlefield;
        let controller = Some(rec.controller);
        self.layer.bases.insert(
            id,
            ObjectBase {
                characteristics: c.clone(),
                controller,
                on_battlefield,
            },
        );
        self.provider.set_chars(id, c);
    }

    /// Test setup (not through the event layer): a new object with characteristics `c`, owned
    /// by `owner`, in a zone of kind `kind`; a battlefield object also enters the permanents.
    pub fn create(&mut self, kind: ZoneKind, owner: PlayerId, c: Characteristics) -> ObjectId {
        self.create_controlled(kind, owner, None, c)
    }

    /// As [`Sim::create`], with an explicit controller (battlefield only).
    pub fn create_controlled(
        &mut self,
        kind: ZoneKind,
        owner: PlayerId,
        controller: Option<PlayerId>,
        c: Characteristics,
    ) -> ObjectId {
        let card = self.fresh_card();
        let mut req = MoveRequest::new(kind);
        req.controller = controller;
        let id = self
            .zones
            .create_object(&mut self.ids, card, owner, req)
            .expect("create succeeds");
        if kind == ZoneKind::Battlefield {
            let turn = self.game.turn().turn_number();
            let ctrl = self.zones.object(id).expect("present").controller;
            self.permanents
                .enter(id, ctrl, turn)
                .expect("enter succeeds");
        }
        self.describe(id, c);
        id
    }

    /// A battlefield creature `name` with base `p`/`t`, owned by `owner`.
    pub fn creature(&mut self, owner: PlayerId, name: &str, p: i64, t: i64) -> ObjectId {
        self.create(ZoneKind::Battlefield, owner, creature_chars(name, p, t))
    }

    /// A battlefield planeswalker owned by `owner` with `loyalty` loyalty counters.
    pub fn walker(&mut self, owner: PlayerId, loyalty: i64) -> ObjectId {
        let id = self.create(
            ZoneKind::Battlefield,
            owner,
            walker_chars(&format!("test.walker_{loyalty}")),
        );
        if loyalty > 0 {
            self.counters(id, "loyalty", loyalty);
        }
        id
    }

    /// Puts `n` counters of `kind` on `id`: in the markers (through `AddCounters`), the layer
    /// environment and the provider.
    pub fn counters(&mut self, id: ObjectId, kind: &str, n: i64) {
        self.ok(GameEvent::AddCounters {
            target: CounterTarget::Object(id),
            kind: kind.to_owned(),
            amount: n,
        });
        let total = self.events.markers().counter(id, kind);
        self.layer
            .counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), total);
        self.provider.set_counter(id, kind, total);
    }

    /// Marks `n` damage on `id` (an unsourced, noncombat `Damage` event).
    pub fn damage(&mut self, id: ObjectId, n: i64) {
        self.ok(GameEvent::Damage {
            source: None,
            source_controller: None,
            target: DamageTarget::Object(id),
            amount: n,
            combat: false,
        });
    }

    /// Moves `id` to a zone of kind `kind` through the event layer (cause `Other`) and
    /// describes the new object as the old one was; returns the new id.
    pub fn move_to(&mut self, id: ObjectId, kind: ZoneKind) -> ObjectId {
        let c = self.layer.bases[&id].characteristics.clone();
        let done = self.ok(GameEvent::ZoneChange {
            object: id,
            request: MoveRequest::new(kind),
            cause: ZoneChangeCause::Other,
        });
        let new = new_id(&done.outcome);
        self.layer.bases.remove(&id);
        self.layer.counters.remove(&id);
        self.describe(new, c);
        new
    }

    /// A 1/1 creature token created by `owner` (through `CreateToken`), then moved to a zone
    /// of kind `kind` if that is not the battlefield; returns its current id.
    pub fn token(&mut self, owner: PlayerId, kind: ZoneKind) -> ObjectId {
        let card = self.fresh_card();
        let done = self.ok(GameEvent::CreateToken {
            owner,
            card,
            controller: None,
        });
        let id = new_id(&done.outcome);
        self.describe(id, creature_chars("test.token_1_1", 1, 1));
        if kind == ZoneKind::Battlefield {
            id
        } else {
            self.move_to(id, kind)
        }
    }

    /// A copy of a spell (`is_spell_copy`) in a zone of kind `kind`.
    pub fn spell_copy(&mut self, owner: PlayerId, kind: ZoneKind) -> ObjectId {
        let id = self.create(kind, owner, blank("test.sorcery_copy", &["Sorcery"]));
        self.env.spell_copies.insert(id);
        id
    }

    /// A copy of a card (`is_card_copy`) in a zone of kind `kind` (a 2/2 creature card).
    pub fn card_copy(&mut self, owner: PlayerId, kind: ZoneKind) -> ObjectId {
        let id = self.create(kind, owner, creature_chars("test.card_copy_2_2", 2, 2));
        self.env.card_copies.insert(id);
        id
    }

    /// `player` attempts to draw from an empty library (the library must be empty).
    pub fn empty_draw(&mut self, player: PlayerId) {
        let r = self.ctx().execute(GameEvent::Draw { player });
        assert_eq!(r, Ok(None), "a draw from an empty library is no event");
        assert!(self.events.empty_library_draws().contains(&player));
    }

    pub fn set_life(&mut self, p: PlayerId, life: i64) {
        self.game
            .player_mut(p)
            .expect("known player")
            .set_life(life);
    }

    pub fn set_poison(&mut self, p: PlayerId, n: i64) {
        self.game
            .player_mut(p)
            .expect("known player")
            .set_counter("poison", n);
    }

    pub fn mark_lost(&mut self, p: PlayerId) {
        self.game
            .player_mut(p)
            .expect("known player")
            .set_designation(LOST_THE_GAME, DesignationValue::Flag);
    }

    pub fn lost(&self, p: PlayerId) -> bool {
        self.game
            .player(p)
            .expect("known player")
            .designation(LOST_THE_GAME)
            == Some(&DesignationValue::Flag)
    }

    pub fn life(&self, p: PlayerId) -> i64 {
        self.game.player(p).expect("known player").life()
    }

    /// The zone kind `id` is in, or `None` if no zone holds it.
    pub fn zone_of(&self, id: ObjectId) -> Option<ZoneKind> {
        self.zones.object(id).map(|r| r.zone.kind())
    }

    pub fn on_battlefield(&self, id: ObjectId) -> bool {
        self.zone_of(id) == Some(ZoneKind::Battlefield)
    }

    pub fn add_replacement(&mut self, effect: ReplacementEffect) {
        let id = effect.id;
        if let Err(e) = self.replacements.add(effect) {
            panic!("effect {id:?} rejected: {e:?}");
        }
    }

    pub fn add_layer_effect(&mut self, id: u64, effect: ContinuousEffect) {
        self.effects
            .register(eid(id), effect)
            .expect("register succeeds");
    }

    /// `sba::check` with the standard registry.
    pub fn check(&mut self) -> Vec<SbaFinding> {
        let registry = SbaRegistry::standard();
        let Sim {
            game,
            zones,
            ids,
            permanents,
            memories,
            lki,
            events,
            provider,
            layer,
            effects,
            env,
            ..
        } = self;
        let ctx = EventContext {
            game,
            zones,
            ids,
            permanents,
            memories,
            lki,
            events,
            provider,
        };
        check(&registry, &ctx, effects, layer, env)
    }

    /// The entry ids of `check`, in order.
    pub fn check_ids(&mut self) -> Vec<&'static str> {
        self.check().iter().map(|f| f.entry_id).collect()
    }

    /// Every component in comparable form.
    pub fn snap(&self) -> Snap {
        Snap {
            game: self.game.clone(),
            zones: bytes(&self.zones),
            ids: self.ids.clone(),
            permanents: bytes(&self.permanents),
            memories: bytes(&self.memories),
            lki: bytes(&self.lki),
            events: bytes(&self.events),
            env: self.env.clone(),
            replacements: bytes(&self.replacements),
            empty_draws: self.events.empty_library_draws(),
        }
    }

    /// Every mutable component's canonical bytes, concatenated.
    pub fn canonical(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(self.game.state_hash().as_bytes());
        out.extend(self.ids.last_issued().to_le_bytes());
        for part in [
            bytes(&self.zones),
            bytes(&self.permanents),
            bytes(&self.memories),
            bytes(&self.lki),
            bytes(&self.events),
            bytes(&self.replacements),
        ] {
            out.extend((part.len() as u64).to_le_bytes());
            out.extend(part);
        }
        self.env.canonical(&mut out);
        out
    }

    /// After a pass: drops layer/provider descriptions of objects no zone holds any more and
    /// describes each object a pass moved (old id -> new id) as the old one was, with
    /// `on_battlefield` matching its new zone; counters and damage do not follow (CR 400.7).
    pub fn sync_after(&mut self, outcome: &SbaOutcome) {
        for b in &outcome.executed {
            for e in &b.events {
                if let Outcome::ZoneChange(moved) = &e.outcome
                    && let Some(base) = self.layer.bases.get(&moved.old).cloned()
                    && self.zones.object(moved.new).is_some()
                {
                    self.describe(moved.new, base.characteristics);
                }
            }
        }
        let mut known: BTreeSet<ObjectId> = self.layer.bases.keys().copied().collect();
        known.extend(self.layer.counters.keys().copied());
        known.extend(self.provider.described_ids());
        let gone: Vec<ObjectId> = known
            .into_iter()
            .filter(|id| self.zones.object(*id).is_none())
            .collect();
        for id in gone {
            self.layer.bases.remove(&id);
            self.layer.counters.remove(&id);
            self.provider.forget(id);
        }
    }
}

impl Transactional for Sim {
    fn state_hash(&self) -> Digest {
        sha256(&self.canonical())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(P1)
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// The new object id of a ZoneChange or TokenCreated outcome.
pub fn new_id(outcome: &Outcome) -> ObjectId {
    match outcome {
        Outcome::ZoneChange(moved) => moved.new,
        Outcome::TokenCreated { object } => *object,
        other => panic!("expected a ZoneChange or TokenCreated outcome, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// running a pass inside a session
// ---------------------------------------------------------------------------------------------

/// A pass's output: the outcome, with a bus error kept as data.
pub type SbaOut = Result<SbaOutcome, BusError>;

/// One `perform_all` with the standard registry over `s`. Atomicity check (brief: on
/// `Err(BusInterrupt::Pending)` the context, the replacement registry and the env flags are
/// unchanged): the state on a pending interrupt must equal the state before the call.
pub fn call_sba(s: &mut Sim, c: &mut ChoiceContext<'_>) -> Result<SbaOut, Interrupt> {
    let registry = SbaRegistry::standard();
    let before = s.snap();
    let Sim {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
        layer,
        effects,
        env,
        replacements,
        ..
    } = s;
    let mut ctx = EventContext {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider,
    };
    let r = perform_all(&registry, &mut ctx, replacements, effects, layer, env, c);
    match r {
        Ok(o) => Ok(Ok(o)),
        Err(BusInterrupt::Pending(p)) => {
            assert_eq!(
                s.snap(),
                before,
                "perform_all must leave every component unchanged on a pending choice"
            );
            Err(Interrupt::Pending(p))
        }
        Err(BusInterrupt::Error(e)) => Ok(Err(e)),
    }
}

/// The result of driving one pass to completion.
pub struct SbaRun {
    pub before: Sim,
    pub after: Sim,
    pub requests: Vec<PendingChoiceRequest>,
    pub answers: Vec<ChoiceResponse>,
    pub result: SbaOut,
    pub hash: Digest,
}

impl SbaRun {
    /// The outcome; panics on a bus error.
    pub fn outcome(&self) -> &SbaOutcome {
        match &self.result {
            Ok(o) => o,
            Err(e) => panic!("expected an SBA outcome, got bus error {e:?}"),
        }
    }

    /// The executed batches holding at least one event.
    pub fn nonempty_batches(&self) -> Vec<ExecutedBatch> {
        self.outcome()
            .executed
            .iter()
            .filter(|b| !b.events.is_empty())
            .cloned()
            .collect()
    }

    /// Every executed ZoneChange as `(old id, new id, destination kind, cause)`, in order.
    pub fn moves(&self) -> Vec<(ObjectId, ObjectId, ZoneKind, ZoneChangeCause)> {
        self.outcome()
            .executed
            .iter()
            .flat_map(|b| b.events.iter())
            .filter_map(|e| match (e.instance.event(), &e.outcome) {
                (GameEvent::ZoneChange { cause, .. }, Outcome::ZoneChange(m)) => {
                    Some((m.old, m.new, m.to.kind(), *cause))
                }
                _ => None,
            })
            .collect()
    }

    /// The new id `old` was moved to by this pass, if it was.
    pub fn moved_to(&self, old: ObjectId) -> Option<ObjectId> {
        self.moves()
            .into_iter()
            .find(|(o, _, _, _)| *o == old)
            .map(|(_, n, _, _)| n)
    }

    /// The entry ids of the findings, in order.
    pub fn finding_ids(&self) -> Vec<&'static str> {
        self.outcome().findings.iter().map(|f| f.entry_id).collect()
    }
}

/// The most requests one pass may publish in a test.
pub const MAX_REQUESTS: usize = 200;

/// Runs one pass over `base`, answering the `i`-th request with `answer(i, req)`.
pub fn drive_sba(
    base: Sim,
    mut answer: impl FnMut(usize, &PendingChoiceRequest) -> ChoiceResponse,
) -> SbaRun {
    let before = base.clone();
    let mut session = match ChoiceSession::start(base, call_sba) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < MAX_REQUESTS, "the pass did not terminate");
        let response = answer(requests.len(), &req);
        if let Err(e) = session.submit(req.request_id, response.clone()) {
            panic!("answer {response:?} to {req:?} rejected: {e:?}");
        }
        requests.push(req);
    }
    let hash = session.state_hash();
    let answers = session.answers().to_vec();
    match session.status() {
        Status::Done { state, output } => SbaRun {
            before,
            after: state.clone(),
            requests,
            answers,
            result: output.clone(),
            hash,
        },
        Status::Pending(_) => unreachable!("the loop ends only when done"),
    }
}

/// Runs one pass that must publish no choice request.
pub fn run_sba(base: Sim) -> SbaRun {
    drive_sba(base, |i, req| panic!("unexpected request #{i}: {req:?}"))
}

/// `SelectReplacement { index }`.
pub fn select(index: u32) -> ChoiceResponse {
    ChoiceResponse::SelectReplacement { index }
}

/// A finding shorthand.
pub fn finding(entry_id: &'static str, subject: SbaSubject) -> SbaFinding {
    SbaFinding { entry_id, subject }
}

pub fn obj(id: ObjectId) -> SbaSubject {
    SbaSubject::Object(id)
}

pub fn player(p: PlayerId) -> SbaSubject {
    SbaSubject::Player(p)
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers for property tests (R3: seeded, no system source)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator (Knuth MMIX constants).
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

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = u64::try_from(hi - lo + 1).expect("lo <= hi");
        lo + i64::try_from(self.below(span)).expect("fits")
    }

    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = usize::try_from(self.below(items.len() as u64)).expect("fits");
        items[i]
    }
}

// ---------------------------------------------------------------------------------------------
// CR 704.5j-704.5t: synthetic permanents, attachments, counters, Sagas and dungeons
// ---------------------------------------------------------------------------------------------

/// Characteristics with the given names, supertypes, card types and subtypes (no P/T).
pub fn typed(
    names: &[&str],
    supertypes: &[&str],
    types: &[&str],
    subtypes: &[&str],
) -> Characteristics {
    Characteristics {
        names: set(names),
        supertypes: set(supertypes),
        subtypes: set(subtypes),
        ..blank("test.placeholder", types)
    }
}

/// A creature with the given names, supertypes and subtypes and literal P/T.
pub fn typed_creature(
    names: &[&str],
    supertypes: &[&str],
    subtypes: &[&str],
    p: i64,
    t: i64,
) -> Characteristics {
    Characteristics {
        power: Some(lit(p)),
        toughness: Some(lit(t)),
        ..typed(names, supertypes, &["Creature"], subtypes)
    }
}

/// A legendary 2/2 creature named `name` (CR 704.5j).
pub fn legend_chars(name: &str) -> Characteristics {
    typed_creature(&[name], &["Legendary"], &[], 2, 2)
}

/// A world enchantment named `name` (CR 704.5k).
pub fn world_chars(name: &str) -> Characteristics {
    typed(&[name], &["World"], &["Enchantment"], &[])
}

/// An Aura enchantment named `name` (CR 704.5m).
pub fn aura_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Enchantment"], &["Aura"])
}

/// An Equipment artifact named `name` (CR 704.5n).
pub fn equipment_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Artifact"], &["Equipment"])
}

/// A Fortification artifact named `name` (CR 704.5n).
pub fn fortification_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Artifact"], &["Fortification"])
}

/// A land named `name`.
pub fn land_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Land"], &[])
}

/// A plain (non-Equipment, non-Fortification) artifact named `name`.
pub fn artifact_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Artifact"], &[])
}

/// A battle named `name` (CR 704.5p, 310.10).
pub fn battle_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Battle"], &[])
}

/// A Saga enchantment named `name` (CR 704.5s).
pub fn saga_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Enchantment"], &["Saga"])
}

/// A dungeon card named `name` (CR 704.5t, 309.2c).
pub fn dungeon_chars(name: &str) -> Characteristics {
    typed(&[name], &[], &["Dungeon"], &[])
}

impl Sim {
    /// A battlefield permanent with characteristics `c`, owned by `owner`, controlled by
    /// `controller`.
    pub fn permanent(
        &mut self,
        owner: PlayerId,
        controller: PlayerId,
        c: Characteristics,
    ) -> ObjectId {
        self.create_controlled(ZoneKind::Battlefield, owner, Some(controller), c)
    }

    /// A legendary 2/2 creature named `name` owned by `owner`, controlled by `controller`.
    pub fn legend(&mut self, owner: PlayerId, controller: PlayerId, name: &str) -> ObjectId {
        self.permanent(owner, controller, legend_chars(name))
    }

    /// A world enchantment owned by `owner`, controlled by `controller`, that gained the world
    /// supertype at stamp `since` (`None`: no stamp recorded, read as 0).
    pub fn world(
        &mut self,
        owner: PlayerId,
        controller: PlayerId,
        name: &str,
        since: Option<u64>,
    ) -> ObjectId {
        let id = self.permanent(owner, controller, world_chars(name));
        if let Some(stamp) = since {
            self.env.world_since.insert(id, stamp);
        }
        id
    }

    /// Attaches permanent `id` to `target` in the permanents container (test setup).
    pub fn attach(&mut self, id: ObjectId, target: AttachTarget) {
        self.permanents.attach(id, target).expect("attach succeeds");
        assert_eq!(self.permanents.attached_to(id), Some(target));
    }

    /// Attaches permanent `id` to permanent `target`.
    pub fn attach_to(&mut self, id: ObjectId, target: ObjectId) {
        self.attach(id, AttachTarget::Permanent(target));
    }

    /// Phases permanent `id` out (CR 702.26b).
    pub fn phase_out(&mut self, id: ObjectId) {
        self.permanents
            .set_phased_out(id, true)
            .expect("a permanent on the battlefield");
    }

    /// Records a "can't have more than `n` counters of `kind`" limit on `id` (CR 704.5r).
    pub fn limit(&mut self, id: ObjectId, kind: &str, n: i64) {
        self.env
            .counter_limits
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), n);
    }

    /// A Saga owned by `owner`, controlled by `controller`, with final chapter number `last`
    /// (`None`: no chapter abilities) and `lore` lore counters.
    pub fn saga(
        &mut self,
        owner: PlayerId,
        controller: PlayerId,
        last: Option<i64>,
        lore: i64,
    ) -> ObjectId {
        let id = self.permanent(owner, controller, saga_chars("test.saga_a"));
        if let Some(n) = last {
            self.env.saga_final_chapter.insert(id, n);
        }
        if lore > 0 {
            self.counters(id, "lore", lore);
        }
        id
    }

    /// A dungeon card owned by `owner` in the command zone; `bottommost` puts it in
    /// `dungeons_at_bottommost_room`.
    pub fn dungeon(&mut self, owner: PlayerId, bottommost: bool) -> ObjectId {
        let id = self.create(ZoneKind::Command, owner, dungeon_chars("test.dungeon_a"));
        assert_eq!(
            self.zones.object(id).expect("created").zone,
            ZoneId::Command
        );
        if bottommost {
            self.env.bottommost_dungeons.insert(id);
        }
        id
    }

    /// The counter count of `kind` on `id` in the event-layer markers.
    pub fn counter(&self, id: ObjectId, kind: &str) -> i64 {
        self.events.markers().counter(id, kind)
    }

    /// Copies the marker counts of every kind the layer environment knows for each object
    /// still in a zone back into the layer environment and the provider (after a pass removed
    /// counters).
    pub fn sync_counters(&mut self) {
        let ids: Vec<ObjectId> = self.layer.counters.keys().copied().collect();
        for id in ids {
            if self.zones.object(id).is_none() {
                continue;
            }
            let kinds: Vec<String> = self.layer.counters[&id].keys().cloned().collect();
            for kind in kinds {
                let n = self.events.markers().counter(id, &kind);
                self.layer
                    .counters
                    .entry(id)
                    .or_default()
                    .insert(kind.clone(), n);
                self.provider.set_counter(id, &kind, n);
            }
        }
    }
}

/// `SubmitObjectSelection { objects: [id] }`: the answer keeping `id` (CR 704.5j).
pub fn keep(id: ObjectId) -> ChoiceResponse {
    ChoiceResponse::SubmitObjectSelection { objects: vec![id] }
}

/// The candidates of a `SelectObjectsFromZone` request (panics on another payload).
pub fn candidates(req: &PendingChoiceRequest) -> Vec<ObjectId> {
    match &req.payload {
        ChoicePayload::SelectObjectsFromZone { candidates } => candidates.clone(),
        other => panic!("expected a SelectObjectsFromZone payload, got {other:?}"),
    }
}

/// Asserts the shape of a legend-rule request (CR 704.5j): chooser, type, bounds, candidates,
/// reason and default (lowest id).
pub fn assert_legend_request(req: &PendingChoiceRequest, chooser: PlayerId, group: &[ObjectId]) {
    let mut sorted = group.to_vec();
    sorted.sort();
    assert_eq!(req.chooser, chooser, "the group's controller chooses");
    assert_eq!(req.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!(req.min, 1);
    assert_eq!(req.max, 1);
    assert_eq!(req.reason.cr, "CR 704.5j");
    assert_eq!(candidates(req), sorted, "candidates ascending");
    assert_eq!(
        req.default_on_timeout,
        keep(sorted[0]),
        "default keeps the lowest id"
    );
}

/// Every executed `RemoveCounters` on an object, as `(object, kind, amount)`, in order.
pub fn counter_removals(r: &SbaRun) -> Vec<(ObjectId, String, i64)> {
    r.outcome()
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|e| match e.instance.event() {
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(id),
                kind,
                amount,
            } => Some((*id, kind.clone(), *amount)),
            _ => None,
        })
        .collect()
}

/// Asserts that `old` left the battlefield through exactly one move of this pass with `cause`,
/// to its owner's graveyard.
pub fn assert_to_owners_graveyard(r: &SbaRun, old: ObjectId, cause: ZoneChangeCause) {
    let owner = r.before.zones.object(old).expect("existed").owner;
    assert!(r.after.zones.object(old).is_none(), "{old:?} left");
    let moves: Vec<_> = r.moves().into_iter().filter(|m| m.0 == old).collect();
    assert_eq!(moves.len(), 1, "{old:?} moved exactly once");
    let (_, new, kind, c) = moves[0];
    assert_eq!(kind, ZoneKind::Graveyard);
    assert_eq!(c, cause, "{old:?} cause");
    assert_eq!(
        r.after.zones.object(new).expect("moved object").zone,
        ZoneId::Graveyard(owner),
        "{old:?} went to its owner's graveyard"
    );
}

/// Asserts that `id` is still on the battlefield after `r`, unmoved.
pub fn assert_still_on_battlefield(r: &SbaRun, id: ObjectId) {
    assert!(
        r.after.on_battlefield(id),
        "{id:?} stayed on the battlefield"
    );
    assert!(r.moved_to(id).is_none(), "{id:?} did not move");
    assert!(!r.outcome().ceased.contains(&id), "{id:?} did not cease");
}

/// Asserts `findings` are in registry order, then subject order, without duplicates.
pub fn assert_findings_ordered(findings: &[SbaFinding]) {
    let keys: Vec<(usize, SbaSubject)> = findings
        .iter()
        .map(|f| (entry_index(f.entry_id), f.subject))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        keys, sorted,
        "findings in registry order then subject order"
    );
}
