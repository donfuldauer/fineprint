// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the graveyard-order spec tests (REQ-ZONE-001, REQ-EVENT-001, SRS 15.3
//! `ORDER_CARDS`; CR 404.3). Black-box: written from the work-item brief only.
//!
//! Each `spec_events_graveyard_order*.rs` test file includes this file with `#[path]`; on its
//! own it is an empty test target. The world is copied from the events spec-test helpers (not
//! included, per the brief) and made `Transactional` so a [`ChoiceSession`] can drive it. Every
//! card here is synthetic and neutrally named (R5).

#![allow(dead_code)]

use std::collections::BTreeMap;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceResponse, ChoiceSession, Interrupt, PendingChoiceRequest,
    Status,
};
use fineprint_core::events::{
    EventBatch, EventContext, EventError, EventState, ExecutedBatch, ExecutedEvent, GameEvent,
    GraveyardOrder, GraveyardOrderGroup, Outcome, ZoneChangeCause,
};
use fineprint_core::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::state::GameState;
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, Moved, Position, ZoneId, ZoneKind, Zones, ZonesConfig};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream, ValueExpressionAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
/// A player id no world knows.
pub const NOBODY: PlayerId = PlayerId::new(9);
/// An object id no world ever issues in these tests.
pub const GHOST: ObjectId = ObjectId::new(999_999);

pub const STARTING_LIFE: i64 = 20;

pub fn cid(n: u64) -> CardId {
    CardId::new(n)
}

// ---------------------------------------------------------------------------------------------
// the world
// ---------------------------------------------------------------------------------------------

/// A test world: an n-player game at turn 1 (player 1 active) plus the standalone components
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
    /// A two-player world.
    pub fn new() -> World {
        World::with_players(2)
    }

    /// A world with players `1..=n`.
    pub fn with_players(n: usize) -> World {
        let mut game = GameState::new(0x5EED_0404, n, STARTING_LIFE).expect("valid players");
        game.set_turn_number(1);
        game.set_active_player(Some(P1)).expect("known player");
        let players: Vec<PlayerId> = game.turn_order().to_vec();
        World {
            game,
            zones: Zones::new(&players, ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            provider: TestProvider {
                fallback: chars("test.vanilla_1_1", 1, 1),
            },
            next_card: 1,
        }
    }

    /// A fresh synthetic card id never used by this world before.
    pub fn fresh_card(&mut self) -> CardId {
        let c = cid(self.next_card);
        self.next_card += 1;
        c
    }

    /// Test setup (not through the event layer): a new nontoken object owned by `owner` in a
    /// zone of kind `kind` (on top of an ordered zone); a battlefield object is also entered
    /// into the permanents.
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

    /// `n` nontoken cards owned by `owner` on the battlefield.
    pub fn creatures(&mut self, owner: PlayerId, n: usize) -> Vec<ObjectId> {
        (0..n)
            .map(|_| self.create(ZoneKind::Battlefield, owner))
            .collect()
    }

    /// A token owned and controlled by `owner` on the battlefield, created through the event
    /// layer (CR 111.2).
    pub fn token(&mut self, owner: PlayerId) -> ObjectId {
        let card = self.fresh_card();
        let done = self.ok(GameEvent::CreateToken {
            owner,
            card,
            controller: None,
        });
        match done.outcome {
            Outcome::TokenCreated { object } => object,
            other => panic!("expected TokenCreated, got {other:?}"),
        }
    }

    /// Whether the object `id` (in some zone) is a token.
    pub fn is_token(&self, id: ObjectId) -> bool {
        let card = self.zones.object(id).expect("present").card;
        self.events.markers().is_token(card)
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

    /// `EventContext::execute`, expecting an event that occurred.
    pub fn ok(&mut self, event: GameEvent) -> ExecutedEvent {
        match self.ctx().execute(event.clone()) {
            Ok(Some(done)) => done,
            other => panic!("expected {event:?} to occur, got {other:?}"),
        }
    }

    /// `EventContext::graveyard_order_groups`.
    pub fn groups(&mut self, batch: &EventBatch) -> Vec<GraveyardOrderGroup> {
        match self.ctx().graveyard_order_groups(batch) {
            Ok(g) => g,
            Err(e) => panic!("graveyard_order_groups failed: {e:?}"),
        }
    }

    /// `EventContext::execute_batch`, expecting success.
    pub fn run_plain(&mut self, batch: &EventBatch) -> ExecutedBatch {
        match self.ctx().execute_batch(batch) {
            Ok(b) => b,
            Err(e) => panic!("execute_batch failed: {e:?}"),
        }
    }

    /// `EventContext::execute_batch_ordered`.
    pub fn run_ordered(
        &mut self,
        batch: &EventBatch,
        orders: &[GraveyardOrder],
    ) -> Result<ExecutedBatch, EventError> {
        self.ctx().execute_batch_ordered(batch, orders)
    }

    /// The contents of `owner`'s graveyard, top first (CR 404.2).
    pub fn graveyard(&self, owner: PlayerId) -> Vec<ObjectId> {
        self.zones.contents(ZoneId::Graveyard(owner))
    }

    /// The graveyard contents of `owner`, top first, without token objects.
    pub fn graveyard_cards(&self, owner: PlayerId) -> Vec<ObjectId> {
        self.graveyard(owner)
            .into_iter()
            .filter(|&id| !self.is_token(id))
            .collect()
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

impl Transactional for World {
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

/// A printed value `Literal(n)` (#232 FP-164: formerly the bare JSON integer placeholder).
fn printed(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

/// A synthetic creature `Characteristics` with the given name, power and toughness.
pub fn chars(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: [name.to_string()].into_iter().collect(),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::new(0).expect("valid mask"),
        supertypes: Default::default(),
        card_types: ["Creature".to_string()].into_iter().collect(),
        subtypes: Default::default(),
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

/// A provider that describes every object with one synthetic creature.
#[derive(Clone, Debug)]
pub struct TestProvider {
    fallback: Characteristics,
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(self.fallback.clone())
    }

    fn counters(&self, _id: ObjectId) -> BTreeMap<String, i64> {
        BTreeMap::new()
    }

    fn attached_here(&self, _id: ObjectId) -> Vec<ObjectId> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------------------------
// events and batches
// ---------------------------------------------------------------------------------------------

/// A zone change of `object` to the top of a `to` zone.
pub fn zc(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

/// A zone change of `object` to a `to` zone at `position`.
pub fn zc_at(object: ObjectId, to: ZoneKind, position: Position) -> GameEvent {
    let mut request = MoveRequest::new(to);
    request.position = position;
    GameEvent::ZoneChange {
        object,
        request,
        cause: ZoneChangeCause::Other,
    }
}

/// A state-based "dies" move to the top of the owner's graveyard (CR 704.3, 404.1).
pub fn dies(object: ObjectId) -> GameEvent {
    zc(
        object,
        ZoneKind::Graveyard,
        ZoneChangeCause::StateBasedAction,
    )
}

/// One simultaneous batch of every object dying (CR 704.3).
pub fn all_die(objects: &[ObjectId]) -> EventBatch {
    EventBatch::from_events(objects.iter().map(|&o| dies(o)).collect())
}

/// The `Moved` outcomes of an executed batch, old id to `Moved`.
pub fn moves(done: &ExecutedBatch) -> BTreeMap<ObjectId, Moved> {
    done.events
        .iter()
        .filter_map(|e| match &e.outcome {
            Outcome::ZoneChange(m) => Some((m.old, *m)),
            _ => None,
        })
        .collect()
}

/// The new ids of `olds` after `done`, in the order of `olds`.
pub fn new_ids(done: &ExecutedBatch, olds: &[ObjectId]) -> Vec<ObjectId> {
    let m = moves(done);
    olds.iter()
        .map(|o| m.get(o).unwrap_or_else(|| panic!("{o:?} did not move")).new)
        .collect()
}

/// `ids` (objects after `done`) mapped back to the ids they had before it; an object that did
/// not move keeps its id.
pub fn olds_of(done: &ExecutedBatch, ids: &[ObjectId]) -> Vec<ObjectId> {
    let back: BTreeMap<ObjectId, ObjectId> = moves(done).values().map(|m| (m.new, m.old)).collect();
    ids.iter()
        .map(|id| back.get(id).copied().unwrap_or(*id))
        .collect()
}

/// The events of an executed batch with the newly assigned ids erased: each event's instance,
/// LKI flag, and for a zone change its old id and both zones (brief clause c: ids may differ
/// between `execute_batch` and `execute_batch_ordered`).
pub fn shape(done: &ExecutedBatch) -> Vec<String> {
    done.events
        .iter()
        .map(|e| match &e.outcome {
            Outcome::ZoneChange(m) => format!(
                "{:?} lki={} moved {:?} {:?}->{:?}",
                e.instance, e.lki_captured, m.old, m.from, m.to
            ),
            other => format!("{:?} lki={} {other:?}", e.instance, e.lki_captured),
        })
        .collect()
}

/// `SubmitOrdering` with only a graveyard list.
pub fn gy_answer(graveyard: &[ObjectId]) -> ChoiceResponse {
    ChoiceResponse::SubmitOrdering {
        top: Vec::new(),
        bottom: Vec::new(),
        graveyard: graveyard.to_vec(),
    }
}

/// A `GraveyardOrder` for `owner`.
pub fn order(owner: PlayerId, top_to_bottom: &[ObjectId]) -> GraveyardOrder {
    GraveyardOrder {
        owner,
        top_to_bottom: top_to_bottom.to_vec(),
    }
}

/// `ids` sorted ascending.
pub fn sorted(ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut v = ids.to_vec();
    v.sort();
    v
}

/// `ids` reversed (the default order of `Top` moves submitted in `ids` order: the last
/// submitted ends on top).
pub fn reversed(ids: &[ObjectId]) -> Vec<ObjectId> {
    ids.iter().rev().copied().collect()
}

/// Every permutation of `items`, in lexicographic index order.
pub fn permutations<T: Copy>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head);
            out.push(tail);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// choice-session driving (REQ-EVENT-001: replacements first, then the order choice, then the
// one all-or-nothing batch)
// ---------------------------------------------------------------------------------------------

/// The session procedure: compute the groups, ask each owner (ascending) through its
/// `request`, turn each answer into an order, and execute the batch with those orders. An
/// event-layer error becomes `ChoiceError::Procedure`.
pub fn ordering_procedure(
    batch: EventBatch,
) -> impl Fn(&mut World, &mut ChoiceContext<'_>) -> Result<ExecutedBatch, Interrupt> {
    move |w: &mut World, c: &mut ChoiceContext<'_>| {
        let to_err = |e: EventError| Interrupt::Error(ChoiceError::Procedure(format!("{e:?}")));
        let groups = w.ctx().graveyard_order_groups(&batch).map_err(to_err)?;
        let mut orders = Vec::new();
        for g in &groups {
            let answer = c.ask(g.request(c.next_request_id()))?;
            orders.push(g.order_from_response(&answer).map_err(to_err)?);
        }
        w.ctx()
            .execute_batch_ordered(&batch, &orders)
            .map_err(to_err)
    }
}

/// The result of driving one ordered batch through a session.
pub struct SessionRun {
    pub requests: Vec<PendingChoiceRequest>,
    pub answers: Vec<ChoiceResponse>,
    pub state: World,
    pub output: ExecutedBatch,
    pub hash: Digest,
}

/// Drives `batch` on `base` through a [`ChoiceSession`], answering request `i` with
/// `answer(i, req)`.
pub fn drive(
    base: &World,
    batch: &EventBatch,
    mut answer: impl FnMut(usize, &PendingChoiceRequest) -> Option<ChoiceResponse>,
) -> SessionRun {
    let mut session = ChoiceSession::start(base.clone(), ordering_procedure(batch.clone()))
        .unwrap_or_else(|e| panic!("session start failed: {e:?}"));
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < 16, "too many requests");
        let result = match answer(requests.len(), &req) {
            Some(r) => session.submit(req.request_id, r),
            None => session.submit_default(req.request_id),
        };
        if let Err(e) = result {
            panic!("answer to {req:?} rejected: {e:?}");
        }
        requests.push(req);
    }
    let hash = session.state_hash();
    let answers = session.answers().to_vec();
    match session.status() {
        Status::Done { state, output } => SessionRun {
            requests,
            answers,
            state: state.clone(),
            output: output.clone(),
            hash,
        },
        Status::Pending(_) => unreachable!("the loop ends only when done"),
    }
}

/// Replays a recorded answer list in one call.
pub fn replay(base: &World, batch: &EventBatch, answers: &[ChoiceResponse]) -> SessionRun {
    let session = ChoiceSession::replay(base.clone(), ordering_procedure(batch.clone()), answers)
        .unwrap_or_else(|e| panic!("replay failed: {e:?}"));
    let hash = session.state_hash();
    match session.status() {
        Status::Done { state, output } => SessionRun {
            requests: Vec::new(),
            answers: answers.to_vec(),
            state: state.clone(),
            output: output.clone(),
            hash,
        },
        Status::Pending(r) => panic!("replay still pending at {r:?}"),
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

/// A 64-bit linear congruential generator (Knuth MMIX constants), as in the events property
/// tests.
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

    /// A uniformly shuffled copy of `items` (Fisher-Yates).
    pub fn shuffled<T: Clone>(&mut self, items: &[T]) -> Vec<T> {
        let mut v = items.to_vec();
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).expect("fits");
            v.swap(i, j);
        }
        v
    }
}
