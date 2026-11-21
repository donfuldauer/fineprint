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
    ChoiceContext, ChoiceResponse, ChoiceSession, Interrupt, PendingChoiceRequest, Status,
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
use fineprint_core::permanent::Permanents;
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

/// Every SBA entry id of this unit, in registry order (CR 704.5a-704.5i).
pub const ENTRY_IDS: [&str; 9] = [
    "704.5a", "704.5b", "704.5c", "704.5d", "704.5e", "704.5f", "704.5g", "704.5h", "704.5i",
];

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
#[derive(Clone, Debug, Default)]
pub struct TestProvider {
    fallback: RefCell<Option<Characteristics>>,
    chars: RefCell<BTreeMap<ObjectId, Characteristics>>,
    counters: RefCell<BTreeMap<ObjectId, BTreeMap<String, i64>>>,
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
        let gone: Vec<ObjectId> = self
            .layer
            .bases
            .keys()
            .copied()
            .filter(|id| self.zones.object(*id).is_none())
            .collect();
        for id in gone {
            self.layer.bases.remove(&id);
            self.layer.counters.remove(&id);
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
