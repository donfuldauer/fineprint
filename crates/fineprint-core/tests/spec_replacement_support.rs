// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::replacement` spec tests (REQ-EVENT-001,
//! REQ-DSL-009). Black-box: written from the work-item brief only (SRS 8.1).
//!
//! Each `spec_replacement_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card, object and effect here is synthetic and neutrally named (R5).
//!
//! The bus is always driven through a [`ChoiceSession`] over [`Bus`] (the world components plus
//! the replacement registry), since a `ChoiceContext` is only handed out by a session. A bus
//! error is turned into a `Done` output (`Err(BusError)`) so a test can compare the final state
//! with the base state (all-or-nothing, brief clause k).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceTarget, Interrupt,
    PendingChoiceRequest, Status,
};
use fineprint_core::events::{
    CounterTarget, DamageTarget, EventBatch, EventContext, EventInstance, EventState,
    ExecutedBatch, ExecutedEvent, GameEvent, Outcome, ZoneChangeCause,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, BusReport, EventMatcher, ReplacementEffect,
    ReplacementRegistry, process_batch, process_event,
};
use fineprint_core::state::GameState;
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream, ValueExpressionAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);

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

pub fn eids(ns: &[u64]) -> Vec<EffectId> {
    ns.iter().map(|&n| eid(n)).collect()
}

// ---------------------------------------------------------------------------------------------
// the world (copied from the events spec-test helpers, not included, per the brief)
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
        let mut game = GameState::new(0x5EED_0028, 2, STARTING_LIFE).expect("valid players");
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
    /// kind `kind`; a battlefield object also enters the permanents under `owner`.
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

    /// Test setup through the plain event layer (no bus): executes `event`, which must occur.
    pub fn ok(&mut self, event: GameEvent) -> ExecutedEvent {
        match self.ctx().execute(event.clone()) {
            Ok(Some(done)) => done,
            other => panic!("expected {event:?} to occur, got {other:?}"),
        }
    }

    /// Test setup: puts `n` counters of `kind` on `object` (through `AddCounters`).
    pub fn put_counters(&mut self, object: ObjectId, kind: &str, n: i64) {
        self.ok(add_counters(object, kind, n));
    }

    /// The number of counters of `kind` on `object` (`ObjectMarkers::counter`).
    pub fn counter(&self, object: ObjectId, kind: &str) -> i64 {
        self.events.markers().counter(object, kind)
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

    /// The zone kind `object` is in, or `None` if no zone holds it.
    pub fn zone_of(&self, object: ObjectId) -> Option<ZoneKind> {
        self.zones.object(object).map(|r| r.zone.kind())
    }

    /// The number of logged executed batches.
    pub fn log_len(&self) -> usize {
        self.events.log().batches().len()
    }

    /// Whether the battlefield permanent `object` is tapped.
    pub fn tapped(&self, object: ObjectId) -> bool {
        self.permanents
            .get(object)
            .expect("a permanent")
            .status()
            .tapped
    }

    /// The player active right now.
    pub fn active(&self) -> PlayerId {
        self.game.turn().active_player().expect("an active player")
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
// characteristics and provider
// ---------------------------------------------------------------------------------------------

fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

/// A printed value `Literal(n)` (#232 FP-164: formerly the bare JSON integer placeholder).
fn printed(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
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

/// A characteristics provider with a fallback for every object.
#[derive(Clone, Debug, Default)]
pub struct TestProvider {
    fallback: RefCell<Option<Characteristics>>,
}

impl TestProvider {
    pub fn with_fallback(fallback: Characteristics) -> TestProvider {
        let p = TestProvider::default();
        *p.fallback.borrow_mut() = Some(fallback);
        p
    }
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        self.fallback.borrow().clone()
    }

    fn counters(&self, _id: ObjectId) -> BTreeMap<String, i64> {
        BTreeMap::new()
    }

    fn attached_here(&self, _id: ObjectId) -> Vec<ObjectId> {
        Vec::new()
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

pub fn dmg_player(p: PlayerId, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Player(p),
        amount,
        combat: false,
    }
}

pub fn dmg_object(o: ObjectId, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(o),
        amount,
        combat: false,
    }
}

/// Damage from source object `source` to player `p` (no source controller).
pub fn dmg_from(source: ObjectId, p: PlayerId, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: Some(source),
        source_controller: None,
        target: DamageTarget::Player(p),
        amount,
        combat: false,
    }
}

pub fn zone_change(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

pub fn life_change(player: PlayerId, delta: i64) -> GameEvent {
    GameEvent::LifeChange { player, delta }
}

pub fn add_counters(object: ObjectId, kind: &str, amount: i64) -> GameEvent {
    GameEvent::AddCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount,
    }
}

pub fn remove_counters(object: ObjectId, kind: &str, amount: i64) -> GameEvent {
    GameEvent::RemoveCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount,
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// The neutral, distinct description of effect `n`.
pub fn desc(n: u64) -> String {
    format!("test.effect_{n}")
}

pub fn descs(ns: &[u64]) -> Vec<String> {
    ns.iter().map(|&n| desc(n)).collect()
}

/// `ReplacementEffect::new(eid(n), matcher, action)` described as [`desc`]`(n)`.
pub fn fx(n: u64, matcher: EventMatcher, action: Action) -> ReplacementEffect {
    ReplacementEffect::new(eid(n), matcher, action).with_description(desc(n))
}

/// A matcher of `kind` (every other field `None`).
pub fn m(kind: BusEventKind) -> EventMatcher {
    EventMatcher::new(kind)
}

/// A Damage matcher for damage dealt to player `p`.
pub fn m_dmg_player(p: PlayerId) -> EventMatcher {
    EventMatcher {
        affected_player: Some(p),
        ..EventMatcher::new(BusEventKind::Damage)
    }
}

/// A ZoneChange matcher for `object`.
pub fn m_zc_object(object: ObjectId) -> EventMatcher {
    EventMatcher {
        affected_object: Some(object),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    }
}

/// A registry holding `effects`, added in the given order; panics if one is rejected.
pub fn registry(effects: Vec<ReplacementEffect>) -> ReplacementRegistry {
    let mut r = ReplacementRegistry::new();
    for e in effects {
        let id = e.id;
        if let Err(err) = r.add(e) {
            panic!("effect {id:?} rejected: {err:?}");
        }
    }
    r
}

// ---------------------------------------------------------------------------------------------
// the session state
// ---------------------------------------------------------------------------------------------

/// The state a bus session runs over: the world and the registry.
#[derive(Clone)]
pub struct Bus {
    pub world: World,
    pub registry: ReplacementRegistry,
}

/// A comparable snapshot of a [`Bus`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusSnap {
    pub world: Snap,
    pub registry: Vec<u8>,
}

impl Bus {
    pub fn new(world: World, effects: Vec<ReplacementEffect>) -> Bus {
        Bus {
            world,
            registry: registry(effects),
        }
    }

    pub fn snap(&self) -> BusSnap {
        BusSnap {
            world: self.world.snap(),
            registry: bytes(&self.registry),
        }
    }

    /// Every component's canonical bytes, concatenated.
    pub fn canonical(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(self.world.game.state_hash().as_bytes());
        out.extend(self.world.ids.last_issued().to_le_bytes());
        for part in [
            bytes(&self.world.zones),
            bytes(&self.world.permanents),
            bytes(&self.world.memories),
            bytes(&self.world.lki),
            bytes(&self.world.events),
            bytes(&self.registry),
        ] {
            out.extend((part.len() as u64).to_le_bytes());
            out.extend(part);
        }
        out
    }
}

impl Transactional for Bus {
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

/// What one bus call is.
#[derive(Clone, Debug)]
pub enum Call {
    /// `process_batch(batch, sba)`.
    Batch(EventBatch, bool),
    /// `process_event(event)`.
    Event(GameEvent),
}

impl Call {
    pub fn batch(events: Vec<GameEvent>, sba: bool) -> Call {
        Call::Batch(EventBatch::from_events(events), sba)
    }
}

/// A procedure's output: the bus result, with a bus error kept as data.
pub type BusOut = Result<BusReport, BusError>;

/// One run of the bus on `s`. A pending choice suspends the session; a bus error becomes the
/// output so the post-call state can be inspected.
pub fn call_bus(s: &mut Bus, call: &Call, c: &mut ChoiceContext<'_>) -> Result<BusOut, Interrupt> {
    let Bus { world, registry } = s;
    let mut ctx = world.ctx();
    let r = match call {
        Call::Batch(b, sba) => process_batch(&mut ctx, registry, b, *sba, c),
        Call::Event(e) => process_event(&mut ctx, registry, e.clone(), c),
    };
    match r {
        Ok(rep) => Ok(Ok(rep)),
        Err(BusInterrupt::Pending(p)) => Err(Interrupt::Pending(p)),
        Err(BusInterrupt::Error(e)) => Ok(Err(e)),
    }
}

/// The session procedure for `call`.
pub fn procedure(
    call: Call,
) -> impl Fn(&mut Bus, &mut ChoiceContext<'_>) -> Result<BusOut, Interrupt> + Clone {
    move |s: &mut Bus, c: &mut ChoiceContext<'_>| call_bus(s, &call, c)
}

/// The result of driving one bus call to completion.
pub struct Run {
    pub before: Bus,
    pub after: Bus,
    /// Every request the session published, in order.
    pub requests: Vec<PendingChoiceRequest>,
    pub answers: Vec<ChoiceResponse>,
    pub result: BusOut,
    pub hash: Digest,
}

impl Run {
    /// The report; panics on a bus error.
    pub fn report(&self) -> &BusReport {
        match &self.result {
            Ok(r) => r,
            Err(e) => panic!("expected a report, got {e:?}"),
        }
    }

    /// The bus error; panics on success.
    pub fn error(&self) -> &BusError {
        match &self.result {
            Ok(r) => panic!("expected a bus error, got {r:?}"),
            Err(e) => e,
        }
    }

    pub fn world(&self) -> &World {
        &self.after.world
    }

    /// `BusReport::applications`.
    pub fn applications(&self) -> Vec<EffectId> {
        self.report().applications.clone()
    }

    /// Every executed event of every executed batch, in order.
    pub fn executed(&self) -> Vec<ExecutedEvent> {
        self.report()
            .executed
            .iter()
            .flat_map(|b| b.events.iter().cloned())
            .collect()
    }

    /// The executed batches that hold at least one event.
    pub fn nonempty_batches(&self) -> Vec<ExecutedBatch> {
        self.report()
            .executed
            .iter()
            .filter(|b| !b.events.is_empty())
            .cloned()
            .collect()
    }

    /// The effect descriptions offered by request `i`.
    pub fn offered(&self, i: usize) -> Vec<String> {
        offered(&self.requests[i])
    }

    /// The logged batches added by this call.
    pub fn new_log(&self) -> Vec<ExecutedBatch> {
        let before = self.before.world.log_len();
        self.after.world.events.log().batches()[before..].to_vec()
    }
}

/// The event name and effect descriptions of a `ChooseReplacementEffect` request.
pub fn payload(req: &PendingChoiceRequest) -> (String, Vec<String>) {
    match &req.payload {
        ChoicePayload::ChooseReplacementEffect { event, effects } => {
            (event.clone(), effects.clone())
        }
        other => panic!("expected a ChooseReplacementEffect payload, got {other:?}"),
    }
}

/// The effect descriptions of a `ChooseReplacementEffect` request.
pub fn offered(req: &PendingChoiceRequest) -> Vec<String> {
    payload(req).1
}

/// The number of options of a `ChooseReplacementEffect` request.
pub fn option_count(req: &PendingChoiceRequest) -> usize {
    offered(req).len()
}

pub fn select(index: u32) -> ChoiceResponse {
    ChoiceResponse::SelectReplacement { index }
}

/// The `(total, recipients)` of a `DivideValue` request (the CR 615.7 pool allocation).
pub fn division(req: &PendingChoiceRequest) -> (u64, Vec<(ChoiceTarget, u64)>) {
    match &req.payload {
        ChoicePayload::DivideValue { total, recipients } => (*total, recipients.clone()),
        other => panic!("expected a DivideValue payload, got {other:?}"),
    }
}

/// `SubmitDivision` giving `amounts[i]` to source object `sources[i]`.
pub fn divide(sources: &[ObjectId], amounts: &[u64]) -> ChoiceResponse {
    assert_eq!(sources.len(), amounts.len());
    ChoiceResponse::SubmitDivision {
        assignments: sources
            .iter()
            .zip(amounts)
            .map(|(&s, &a)| (ChoiceTarget::Object(s), a))
            .collect(),
    }
}

/// The most requests one test run may publish before it is declared non-terminating.
pub const MAX_REQUESTS: usize = 2_000;

/// Drives `call` on `base` to completion, answering each request `req` (the `i`-th, from 0)
/// with `SelectReplacement { index: pick(i, req) }`.
pub fn drive(
    base: Bus,
    call: Call,
    mut pick: impl FnMut(usize, &PendingChoiceRequest) -> u32,
) -> Run {
    drive_with(base, call, |i, req| select(pick(i, req)))
}

/// Drives `call` on `base` to completion, answering each request `req` (the `i`-th, from 0)
/// with `answer(i, req)` (any response type, e.g. a `SubmitDivision`).
pub fn drive_with(
    base: Bus,
    call: Call,
    mut answer: impl FnMut(usize, &PendingChoiceRequest) -> ChoiceResponse,
) -> Run {
    let before = base.clone();
    let mut session = match ChoiceSession::start(base, procedure(call)) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(
            requests.len() < MAX_REQUESTS,
            "the bus did not terminate within {MAX_REQUESTS} requests"
        );
        let response = answer(requests.len(), &req);
        if let Err(e) = session.submit(req.request_id, response.clone()) {
            panic!("answer {response:?} to {req:?} rejected: {e:?}");
        }
        requests.push(req);
    }
    let hash = session.state_hash();
    let answers = session.answers().to_vec();
    match session.status() {
        Status::Done { state, output } => Run {
            before,
            after: state.clone(),
            requests,
            answers,
            result: output.clone(),
            hash,
        },
        Status::Pending(_) => unreachable!("loop ends only when done"),
    }
}

/// Drives `call`, answering the requests with `answers` in order; panics unless exactly
/// `answers.len()` requests are published.
pub fn run(base: Bus, call: Call, answers: &[u32]) -> Run {
    let r = drive(base, call, |i, req| match answers.get(i) {
        Some(&a) => a,
        None => panic!("unexpected request #{i}: {req:?}"),
    });
    assert_eq!(
        r.requests.len(),
        answers.len(),
        "expected exactly {} requests",
        answers.len()
    );
    r
}

/// `run` of a `process_batch` call.
pub fn run_batch(base: Bus, events: Vec<GameEvent>, sba: bool, answers: &[u32]) -> Run {
    run(base, Call::batch(events, sba), answers)
}

/// `run` of a `process_event` call.
pub fn run_event(base: Bus, event: GameEvent, answers: &[u32]) -> Run {
    run(base, Call::Event(event), answers)
}

/// The index of description `desc(n)` in request `req`; panics if it is not offered.
pub fn index_of(req: &PendingChoiceRequest, n: u64) -> u32 {
    let d = desc(n);
    let i = offered(req)
        .iter()
        .position(|x| *x == d)
        .unwrap_or_else(|| panic!("{d} not offered by {req:?}"));
    u32::try_from(i).expect("fits")
}

/// Drives `call`, always choosing the effect with the given id order of preference: the first
/// id of `prefer` that a request offers.
pub fn run_preferring(base: Bus, call: Call, prefer: &[u64]) -> Run {
    drive(base, call, |_, req| {
        let names = offered(req);
        for &n in prefer {
            if let Some(i) = names.iter().position(|x| *x == desc(n)) {
                return u32::try_from(i).expect("fits");
            }
        }
        panic!("no preferred effect offered by {req:?}")
    })
}

/// Asserts that the post-call state equals the base state (all-or-nothing, clause k).
pub fn assert_unchanged(r: &Run) {
    assert_eq!(r.after.snap(), r.before.snap(), "world or registry changed");
}

/// The applied-effects set of an executed event, ascending.
pub fn applied_of(e: &ExecutedEvent) -> Vec<EffectId> {
    e.instance.applied_effects().iter().collect()
}

/// The new object id of a ZoneChange or TokenCreated outcome.
pub fn new_id(outcome: &Outcome) -> ObjectId {
    match outcome {
        Outcome::ZoneChange(moved) => moved.new,
        Outcome::TokenCreated { object } => *object,
        other => panic!("expected a ZoneChange or TokenCreated outcome, got {other:?}"),
    }
}

/// `EventInstance` with the given pre-applied effects.
pub fn instance_with(event: GameEvent, applied: &[u64]) -> EventInstance {
    let mut set = fineprint_core::events::AppliedEffects::new();
    for &n in applied {
        set.insert(eid(n));
    }
    EventInstance::new(event).with_applied(set)
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

    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = usize::try_from(self.below(items.len() as u64)).expect("fits");
        items[i]
    }
}
