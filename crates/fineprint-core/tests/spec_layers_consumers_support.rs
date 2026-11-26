// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the "rules consumers read evaluated characteristics" spec tests
//! (#184 FP-145; REQ-LAYER-001, REQ-CORE-003, REQ-EVENT-001, REQ-EVENT-002; CR 608.2h,
//! 603.10a, 613.1-613.4, 614.12, 616.1). Black-box: written from the work-item brief only.
//!
//! Each `spec_layers_consumers_*.rs` test file includes this file with
//! `mod spec_layers_consumers_support;`; on its own it is an empty test target. Every object
//! and ability here is synthetic and neutrally named (R5); nothing reads a clock or an
//! environment variable (R3, R7).
//!
//! Wiring (from the brief): the event context holds `&mut GameState`, so the evaluated
//! provider is built over a SNAPSHOT of the game taken before the context:
//! `let snap = t.game.clone(); let env = GameEnvironment::new(&snap, &objs);
//! let provider = LayeredProvider::new(snap.effects(), &env);`. The base-only control is
//! [`BaseProvider`], which answers with the [`Objects`] world's base characteristics.

#![allow(dead_code)]

#[path = "typed_ast_support/mod.rs"]
mod typed_ast_support;
use typed_ast_support::synthetic_ability;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::SplitMix64;
use fineprint_core::choice::{ChoiceContext, ChoiceResponse, ChoiceSession, Interrupt, Status};
use fineprint_core::events::{
    CounterTarget, EventBatch, EventContext, EventState, ExecutedBatch, ExecutedEvent, GameEvent,
    Outcome, ZoneChangeCause,
};
use fineprint_core::layers::{
    Affected, ContinuousEffect, EffectOrigin, LayerModification, MatchContext, ObjectBase,
    literal_of,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiRecord, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::replacement::{
    BusError, BusInterrupt, BusReport, ReplacementRegistry, process_event,
};
use fineprint_core::state::{GameState, GeneratingAbility, ObjectWorld};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
pub use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, ReferenceAST};

// ---------------------------------------------------------------------------------------------
// ids and small values
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);

/// The card type every granting effect here adds (CR 613.1d, layer 4).
pub const GRANTED: &str = "Artifact";

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A literal printed power/toughness value `Literal(n)` (#232 FP-164: formerly the bare JSON
/// integer placeholder).
pub fn printed(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

/// A synthetic ability identified by value (#232 FP-164: a valid inert KEYWORD ability instead
/// of the former free-form `{"synthetic": name}` placeholder).
pub fn ability(name: &str) -> AbilityAST {
    synthetic_ability(name)
}

/// A synthetic creature with the given name and printed power/toughness, no abilities.
pub fn creature(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        power: Some(printed(power)),
        toughness: Some(printed(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// `c` with one more ability.
pub fn with_ability(mut c: Characteristics, a: AbilityAST) -> Characteristics {
    c.abilities.push(a);
    c
}

/// The literal power and toughness of `c` (`None` for a missing or non-literal value).
pub fn pt(c: &Characteristics) -> (Option<i64>, Option<i64>) {
    (
        c.power.as_ref().and_then(literal_of),
        c.toughness.as_ref().and_then(literal_of),
    )
}

/// `(Some(p), Some(t))`.
pub fn spt(p: i64, t: i64) -> (Option<i64>, Option<i64>) {
    (Some(p), Some(t))
}

/// Whether `c` has the card type `ty`.
pub fn has_type(c: &Characteristics, ty: &str) -> bool {
    c.card_types.contains(ty)
}

// ---------------------------------------------------------------------------------------------
// selectors (opaque keys answered by `Objects::matches`)
// ---------------------------------------------------------------------------------------------

pub fn sel(slot: u32) -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::Target { slot })
}

/// Matches objects that are creatures as evaluated so far.
pub const SEL_CREATURE: u32 = 4;
/// Matches objects that have the [`GRANTED`] card type as evaluated so far.
pub const SEL_GRANTED: u32 = 5;

// ---------------------------------------------------------------------------------------------
// the object world (the layer system's view of objects) and the base-only control provider
// ---------------------------------------------------------------------------------------------

/// A synthetic [`ObjectWorld`] over ordered maps (R3: no hash-ordered iteration).
#[derive(Clone, Debug, Default)]
pub struct Objects {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl Objects {
    pub fn new() -> Objects {
        Objects::default()
    }

    /// A battlefield permanent controlled by `controller`.
    pub fn put(&mut self, id: ObjectId, c: Characteristics, controller: PlayerId) {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(controller),
                on_battlefield: true,
            },
        );
    }

    pub fn counter(&mut self, id: ObjectId, kind: &str, amount: i64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_string(), amount);
    }

    /// The base characteristics of `id`; panics if unknown.
    pub fn base_chars(&self, id: ObjectId) -> &Characteristics {
        &self.bases.get(&id).expect("known object").characteristics
    }
}

impl ObjectWorld for Objects {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        let c = &ctx.candidate_view.characteristics;
        if *selector == sel(SEL_CREATURE) {
            c.card_types.contains("Creature")
        } else if *selector == sel(SEL_GRANTED) {
            c.card_types.contains(GRANTED)
        } else {
            false
        }
    }
}

/// The discriminating control: a provider that answers with the BASE characteristics of the
/// [`Objects`] world (what a consumer reading the base definition would see), no counters and
/// no attachments.
pub struct BaseProvider<'a>(pub &'a Objects);

impl CharacteristicsProvider for BaseProvider<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.0.bases.get(&id).map(|b| b.characteristics.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// A resolved continuous effect affecting the locked set `objects`.
pub fn resolved(
    timestamp: Timestamp,
    layer: Layer,
    objects: &[ObjectId],
    modification: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: P1,
        timestamp,
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected: Affected::Objects(objects.iter().copied().collect()),
        modification,
    }
}

/// A static effect of `source`'s own (printed) ability, applying by selector.
pub fn static_sel(
    source: ObjectId,
    timestamp: Timestamp,
    layer: Layer,
    slot: u32,
    modification: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        controller: P1,
        timestamp,
        layer,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected: Affected::Selector(sel(slot)),
        modification,
    }
}

/// Layer 4 (CR 613.1d): adds the card type `ty`.
pub fn grant_type(ty: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(&[ty]),
        subtypes: BTreeSet::new(),
    }
}

/// Layer 7b (CR 613.4b): sets power and toughness.
pub fn set_pt(p: i64, t: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(p),
        toughness: Some(t),
    }
}

/// Layer 7c (CR 613.4c): modifies power and toughness.
pub fn modify_pt(p: i64, t: i64) -> LayerModification {
    LayerModification::ModifyPt {
        power: p,
        toughness: t,
    }
}

/// Registers a resolved effect through `GameState::register_effect` with a fresh resolved
/// timestamp; returns the effect id.
pub fn register_resolved(
    game: &mut GameState,
    layer: Layer,
    objects: &[ObjectId],
    modification: LayerModification,
) -> EffectId {
    let t = game.stamp_resolved_effect().expect("timestamp");
    register_at(game, t, layer, objects, modification)
}

/// Registers a resolved effect with the given timestamp.
pub fn register_at(
    game: &mut GameState,
    t: Timestamp,
    layer: Layer,
    objects: &[ObjectId],
    modification: LayerModification,
) -> EffectId {
    game.register_effect(resolved(t, layer, objects, modification), None)
        .expect("register_effect accepts the effect")
}

/// Registers a static effect of `source`'s ability `own`, linked to it.
pub fn register_static(
    game: &mut GameState,
    source: ObjectId,
    own: AbilityAST,
    layer: Layer,
    slot: u32,
    modification: LayerModification,
) -> EffectId {
    let t = game.stamp_resolved_effect().expect("timestamp");
    game.register_effect(
        static_sel(source, t, layer, slot, modification),
        Some(GeneratingAbility::new(source, own)),
    )
    .expect("register_effect accepts the effect")
}

// ---------------------------------------------------------------------------------------------
// the event-layer table
// ---------------------------------------------------------------------------------------------

/// The components an `EventContext` borrows (everything except the provider, which a test
/// chooses per call).
#[derive(Clone)]
pub struct Table {
    pub game: GameState,
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Permanents,
    pub memories: MemoryTable,
    pub lki: LkiStore,
    pub events: EventState,
    next_card: u64,
}

impl Default for Table {
    fn default() -> Table {
        Table::new()
    }
}

impl Table {
    pub fn new() -> Table {
        let mut game = GameState::new(0x5EED_0184, 2, 20).expect("valid players");
        game.set_turn_number(1);
        game.set_active_player(Some(P1)).expect("known player");
        Table {
            game,
            zones: Zones::new(&[P1, P2], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            next_card: 1,
        }
    }

    /// Test setup: a new battlefield object owned and controlled by `owner`.
    pub fn create(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let card = CardId::new(self.next_card);
        self.next_card += 1;
        let id = self
            .zones
            .create_object(&mut self.ids, card, owner, MoveRequest::new(kind))
            .expect("create succeeds");
        if kind == ZoneKind::Battlefield {
            let turn = self.game.turn().turn_number();
            self.permanents
                .enter(id, owner, turn)
                .expect("enter succeeds");
        }
        id
    }

    /// The event context over this table with `provider`.
    pub fn ctx<'a>(&'a mut self, provider: &'a dyn CharacteristicsProvider) -> EventContext<'a> {
        EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider,
        }
    }

    /// `EventContext::execute` of `event`, which must occur.
    pub fn exec(
        &mut self,
        provider: &dyn CharacteristicsProvider,
        event: GameEvent,
    ) -> ExecutedEvent {
        match self.ctx(provider).execute(event.clone()) {
            Ok(Some(done)) => done,
            other => panic!("expected {event:?} to occur, got {other:?}"),
        }
    }

    /// `EventContext::execute_batch` of `events` as one simultaneous batch.
    pub fn batch(
        &mut self,
        provider: &dyn CharacteristicsProvider,
        events: Vec<GameEvent>,
    ) -> ExecutedBatch {
        let b = EventBatch::from_events(events);
        self.ctx(provider)
            .execute_batch(&b)
            .expect("batch executes")
    }

    /// The LKI record of `id`; panics if none was captured.
    pub fn lki_of(&self, id: ObjectId) -> &LkiRecord {
        self.lki.get(id).expect("LKI captured")
    }

    /// The zone kind `object` is in now.
    pub fn zone_of(&self, object: ObjectId) -> Option<ZoneKind> {
        self.zones.object(object).map(|r| r.zone.kind())
    }

    /// Every component's canonical bytes, concatenated.
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
        ] {
            out.extend((part.len() as u64).to_le_bytes());
            out.extend(part);
        }
        out
    }
}

/// The canonical bytes of `v`.
pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------------------------

pub fn zone_change(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

pub fn add_counters(object: ObjectId, kind: &str, amount: i64) -> GameEvent {
    GameEvent::AddCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount,
    }
}

/// The new object id of a ZoneChange outcome.
pub fn new_id(outcome: &Outcome) -> ObjectId {
    match outcome {
        Outcome::ZoneChange(moved) => moved.new,
        other => panic!("expected a ZoneChange outcome, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// the canonical fixture: one battlefield creature with a granted type and a P/T change
// ---------------------------------------------------------------------------------------------

/// The base of the fixture creature: `test.lki_granted_2_2`, a 2/2 creature.
pub const SUBJECT: &str = "test.lki_granted_2_2";

/// A table with one battlefield creature `a` (base 2/2 creature, see [`SUBJECT`]) and, through
/// `GameState::register_effect`, a layer-4 effect granting [`GRANTED`] and a layer-7c +1/+2
/// effect on it. Evaluated: a 3/4 artifact creature.
pub struct Fixture {
    pub table: Table,
    pub objects: Objects,
    pub a: ObjectId,
    pub grant: EffectId,
    pub pump: EffectId,
}

pub fn fixture() -> Fixture {
    let mut table = Table::new();
    let a = table.create(ZoneKind::Battlefield, P1);
    let mut objects = Objects::new();
    objects.put(a, creature(SUBJECT, 2, 2), P1);
    let grant = register_resolved(&mut table.game, Layer::L4, &[a], grant_type(GRANTED));
    let pump = register_resolved(&mut table.game, Layer::L7c, &[a], modify_pt(1, 2));
    Fixture {
        table,
        objects,
        a,
        grant,
        pump,
    }
}

// ---------------------------------------------------------------------------------------------
// replacement-bus session (a ChoiceContext is only handed out by a ChoiceSession)
// ---------------------------------------------------------------------------------------------

/// The state a bus session runs over.
#[derive(Clone)]
pub struct Bus {
    pub table: Table,
    pub registry: ReplacementRegistry,
}

impl Transactional for Bus {
    fn state_hash(&self) -> Digest {
        let mut out = self.table.canonical();
        out.extend(bytes(&self.registry));
        sha256(&out)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(P1)
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// The finished bus run: the state after it and the bus result.
pub struct BusRun {
    pub after: Bus,
    pub result: Result<BusReport, BusError>,
    /// How many replacement-effect choices were requested.
    pub requests: usize,
}

impl BusRun {
    pub fn report(&self) -> &BusReport {
        match &self.result {
            Ok(r) => r,
            Err(e) => panic!("expected a report, got {e:?}"),
        }
    }

    /// Every executed event of every executed batch, in order.
    pub fn executed(&self) -> Vec<ExecutedEvent> {
        self.report()
            .executed
            .iter()
            .flat_map(|b| b.events.iter().cloned())
            .collect()
    }
}

/// Runs `replacement::process_event(event)` on `base` with `provider` in the event context,
/// answering the i-th replacement choice with `answers[i]` (`SelectReplacement { index }`).
pub fn run_bus(
    base: Bus,
    provider: &dyn CharacteristicsProvider,
    event: GameEvent,
    answers: &[u32],
) -> BusRun {
    let procedure = move |s: &mut Bus,
                          c: &mut ChoiceContext<'_>|
          -> Result<Result<BusReport, BusError>, Interrupt> {
        let Bus { table, registry } = s;
        let mut ctx = table.ctx(provider);
        match process_event(&mut ctx, registry, event.clone(), c) {
            Ok(rep) => Ok(Ok(rep)),
            Err(BusInterrupt::Pending(p)) => Err(Interrupt::Pending(p)),
            Err(BusInterrupt::Error(e)) => Ok(Err(e)),
        }
    };
    let mut session = match ChoiceSession::start(base, procedure) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    let mut requests = 0usize;
    while let Some(req) = session.pending().cloned() {
        let index = *answers
            .get(requests)
            .unwrap_or_else(|| panic!("unexpected request #{requests}: {req:?}"));
        if let Err(e) = session.submit(req.request_id, ChoiceResponse::SelectReplacement { index })
        {
            panic!("answer {index} rejected: {e:?}");
        }
        requests += 1;
    }
    assert_eq!(
        requests,
        answers.len(),
        "expected exactly {} requests",
        answers.len()
    );
    match session.status() {
        Status::Done { state, output } => BusRun {
            after: state.clone(),
            result: output.clone(),
            requests,
        },
        Status::Pending(_) => unreachable!("the loop ends only when done"),
    }
}

// ---------------------------------------------------------------------------------------------
// deterministic test PRNG (fixed seeds only, R3)
// ---------------------------------------------------------------------------------------------

pub struct Rng(SplitMix64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(SplitMix64::new(seed))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// A value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = u64::try_from(hi - lo + 1).expect("non-empty range");
        lo + i64::try_from(self.below(span)).expect("fits")
    }
}
