// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::events` spec tests (REQ-EVENT-001, REQ-DSL-003).
//! Black-box: written from the work-item brief only.
//!
//! Each `spec_events_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card here is synthetic and neutrally named (R5).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::events::{
    AppliedEffects, EventBatch, EventContext, EventError, EventInstance, EventState, ExecutedBatch,
    ExecutedEvent, GameEvent,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::state::GameState;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream, ValueExpressionAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
/// A player id no world knows.
pub const NOBODY: PlayerId = PlayerId::new(9);
/// An object id no world ever issues in these tests.
pub const GHOST: ObjectId = ObjectId::new(999_999);

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

/// An `AppliedEffects` set holding the given effect ids.
pub fn applied(ids: &[u64]) -> AppliedEffects {
    let mut set = AppliedEffects::new();
    for &n in ids {
        set.insert(eid(n));
    }
    set
}

// ---------------------------------------------------------------------------------------------
// the world
// ---------------------------------------------------------------------------------------------

/// A test world: a two-player game at turn 1 (player 1 active) plus the standalone components
/// an `EventContext` borrows.
#[derive(Clone)]
pub struct World {
    pub game: GameState,
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Permanents,
    pub memories: MemoryTable,
    pub lki: LkiStore,
    pub events: EventState,
    pub provider: TestProvider,
    next_card: u64,
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

impl World {
    pub fn new() -> World {
        let mut game = GameState::new(0x5EED_0027, 2, STARTING_LIFE).expect("valid players");
        game.set_turn_number(1);
        game.set_active_player(Some(P1)).expect("known player");
        World {
            game,
            zones: Zones::new(&[P1, P2], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            provider: TestProvider::with_fallback(chars("test.vanilla_1_1", 1, 1)),
            next_card: 1,
        }
    }

    /// A fresh synthetic card id never used by this world before.
    pub fn fresh_card(&mut self) -> CardId {
        let c = cid(self.next_card);
        self.next_card += 1;
        c
    }

    /// Test setup (not through the event layer): a new object owned by `owner` in a zone of
    /// kind `kind`; a battlefield object is also entered into the permanents under `owner` on
    /// the current turn.
    pub fn create(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        self.create_controlled(kind, owner, None)
    }

    /// As [`World::create`], with an explicit controller (battlefield only).
    pub fn create_controlled(
        &mut self,
        kind: ZoneKind,
        owner: PlayerId,
        controller: Option<PlayerId>,
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
        id
    }

    /// The event context over every component of this world.
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

    /// `EventContext::execute`.
    pub fn exec(
        &mut self,
        event: impl Into<EventInstance>,
    ) -> Result<Option<ExecutedEvent>, EventError> {
        self.ctx().execute(event)
    }

    /// `EventContext::execute`, expecting an event that occurred.
    pub fn ok(&mut self, event: GameEvent) -> ExecutedEvent {
        match self.exec(event.clone()) {
            Ok(Some(done)) => done,
            other => panic!("expected {event:?} to occur, got {other:?}"),
        }
    }

    /// `EventContext::execute_batch` of the given events.
    pub fn batch(&mut self, events: Vec<GameEvent>) -> Result<ExecutedBatch, EventError> {
        let batch = EventBatch::from_events(events);
        self.ctx().execute_batch(&batch)
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
        }
    }

    pub fn life(&self, p: PlayerId) -> i64 {
        self.game.player(p).expect("known player").life()
    }

    pub fn turn(&self) -> u64 {
        self.game.turn().turn_number()
    }
}

/// A comparable snapshot of a whole world (game by value, the rest by canonical bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snap {
    pub game: GameState,
    pub zones: Vec<u8>,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Vec<u8>,
    pub memories: Vec<u8>,
    pub lki: Vec<u8>,
    pub events: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// characteristics
// ---------------------------------------------------------------------------------------------

/// Round-trips a serialisable value through MessagePack into one of the DSL placeholder types.
fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

fn printed(n: i64) -> ValueExpressionAST {
    via_msgpack(&n)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A synthetic creature `Characteristics` with the given name, power and toughness.
pub fn chars(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::new(0).expect("valid mask"),
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(printed(power)),
        toughness: Some(printed(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

// ---------------------------------------------------------------------------------------------
// provider
// ---------------------------------------------------------------------------------------------

/// A characteristics provider that knows characteristics for every object (a fallback), with
/// per-object overrides, counters and attached objects that a test may set.
#[derive(Clone, Debug, Default)]
pub struct TestProvider {
    fallback: RefCell<Option<Characteristics>>,
    chars: RefCell<BTreeMap<ObjectId, Characteristics>>,
    counters: RefCell<BTreeMap<ObjectId, BTreeMap<String, i64>>>,
    attached: RefCell<BTreeMap<ObjectId, Vec<ObjectId>>>,
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

    pub fn set_attached(&self, id: ObjectId, attached: Vec<ObjectId>) {
        self.attached.borrow_mut().insert(id, attached);
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

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.borrow().get(&id).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// encoding
// ---------------------------------------------------------------------------------------------

/// The canonical bytes of `v`.
pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
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

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = usize::try_from(self.below(items.len() as u64)).expect("fits");
        items[i]
    }
}
