// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the "evaluated (layer-2) controller" spec tests (#216 FP-160;
//! REQ-LAYER-001, REQ-REPL-*, REQ-CORE-003, REQ-EVENT-001; CR 613.1b, 616.1, 108.4a, 110.2,
//! 603.10a, 701.21a, 400.3, 613.7). Black-box: written from the work-item brief and the public
//! API only.
//!
//! `spec_evaluated_controller.rs` and `spec_evaluated_controller_property.rs` include this file
//! with `#[path]`; on its own it is an empty test target. Every object here is synthetic and
//! neutrally named (R5); nothing reads a clock, the environment or a hash-ordered container
//! (R3, R7).

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::SplitMix64;
use fineprint_core::choice::ChoiceContext;
use fineprint_core::choice::{
    ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest,
    Status,
};
use fineprint_core::events::{EventContext, EventState, ExecutedEvent, GameEvent, ZoneChangeCause};
use fineprint_core::layers::{
    Affected, ContinuousEffect, EffectOrigin, LayerModification, LayeredProvider, MatchContext,
    ObjectBase,
};
use fineprint_core::memory::{
    CharacteristicsProvider, LkiRecord, LkiStore, MemoryError, MemoryTable, capture_lki_on_move,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, BusInterrupt, BusReport, EventMatcher, Modification,
    ReplacementEffect, ReplacementRegistry, process_event,
};
use fineprint_core::state::{GameEnvironment, GameState, ObjectWorld};
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
pub use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::ObjectSelectorAST;

// ---------------------------------------------------------------------------------------------
// ids and small values
// ---------------------------------------------------------------------------------------------

/// `GameState::new` issues players `1..=player_count`.
pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
pub const P4: PlayerId = PlayerId::new(4);

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A literal printed value `Literal(n)` (#232 FP-164: formerly the bare JSON integer placeholder).
fn printed(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

/// A synthetic creature with no abilities.
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

/// A synthetic enchantment with the "Saga" subtype (`test.saga_like`), no abilities.
pub fn saga_like() -> Characteristics {
    Characteristics {
        names: set(&["test.saga_like"]),
        card_types: set(&["Enchantment"]),
        subtypes: set(&["Saga"]),
        power: None,
        toughness: None,
        ..creature("test.saga_like", 0, 0)
    }
}

// ---------------------------------------------------------------------------------------------
// the object world and the providers
// ---------------------------------------------------------------------------------------------

/// A synthetic [`ObjectWorld`] over ordered maps (R3).
#[derive(Clone, Debug, Default)]
pub struct Objects {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl Objects {
    pub fn new() -> Objects {
        Objects::default()
    }

    /// A battlefield permanent whose base (stored) controller is `controller`.
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

    /// An object outside the battlefield (no controller in the layer system's view).
    pub fn put_off(&mut self, id: ObjectId, c: Characteristics) {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: None,
                on_battlefield: false,
            },
        );
    }

    pub fn counter(&mut self, id: ObjectId, kind: &str, amount: i64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_string(), amount);
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

    fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
        false
    }
}

/// A provider that implements ONLY `characteristics` (the pre-#216 shape): it does not
/// override `controller`, so the trait default applies.
pub struct BaseOnly<'a>(pub &'a Objects);

impl CharacteristicsProvider for BaseOnly<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.0.bases.get(&id).map(|b| b.characteristics.clone())
    }
}

/// A provider implementing `characteristics`, `counters` and `attached_here` but not
/// `controller` (scenario 12, compatibility).
pub struct BaseWithCounters<'a>(pub &'a Objects);

impl CharacteristicsProvider for BaseWithCounters<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.0.bases.get(&id).map(|b| b.characteristics.clone())
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.0.counters.get(&id).cloned().unwrap_or_default()
    }

    fn attached_here(&self, _id: ObjectId) -> Vec<ObjectId> {
        Vec::new()
    }
}

/// A fake provider that reports base characteristics and a FIXED controller per object
/// (`None` for objects not in `controllers`).
pub struct FixedController<'a> {
    pub objects: &'a Objects,
    pub controllers: BTreeMap<ObjectId, PlayerId>,
}

impl CharacteristicsProvider for FixedController<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.objects
            .bases
            .get(&id)
            .map(|b| b.characteristics.clone())
    }

    fn controller(&self, id: ObjectId) -> Option<PlayerId> {
        self.controllers.get(&id).copied()
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// A resolved continuous effect affecting exactly `objects`.
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

/// Registers a resolved effect with a fresh timestamp.
pub fn register_resolved(
    game: &mut GameState,
    layer: Layer,
    objects: &[ObjectId],
    modification: LayerModification,
) -> EffectId {
    let t = game.stamp_resolved_effect().expect("timestamp");
    register_at(game, t, layer, objects, modification)
}

/// Layer 2 (CR 613.1b): a resolved control-changing effect giving `object` to `to`.
pub fn give_control(game: &mut GameState, object: ObjectId, to: PlayerId) -> EffectId {
    register_resolved(
        game,
        Layer::L2,
        &[object],
        LayerModification::SetController(to),
    )
}

/// Layer 4 (CR 613.1d): adds a card type (no control change).
pub fn add_type(ty: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(&[ty]),
        subtypes: BTreeSet::new(),
    }
}

/// Layer 7b (CR 613.4b): sets power and toughness (no control change).
pub fn set_pt(p: i64, t: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(p),
        toughness: Some(t),
    }
}

// ---------------------------------------------------------------------------------------------
// the table
// ---------------------------------------------------------------------------------------------

/// The components an `EventContext` borrows, for `players` players (active player P1).
#[derive(Clone)]
pub struct Table {
    pub game: GameState,
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Permanents,
    pub memories: MemoryTable,
    pub lki: LkiStore,
    pub events: EventState,
    pub players: Vec<PlayerId>,
    next_card: u64,
}

impl Table {
    pub fn new(players: usize) -> Table {
        let mut game = GameState::new(0x5EED_0216, players, 20).expect("valid players");
        game.set_turn_number(1);
        game.set_active_player(Some(P1)).expect("known player");
        let ps: Vec<PlayerId> = (1..=players as u64).map(PlayerId::new).collect();
        Table {
            game,
            zones: Zones::new(&ps, ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            players: ps,
            next_card: 1,
        }
    }

    fn card(&mut self) -> CardId {
        let c = CardId::new(self.next_card);
        self.next_card += 1;
        c
    }

    /// A battlefield permanent owned by `owner` whose stored controller is `controller`
    /// (zones and permanents agree, CR 110.2).
    pub fn permanent(&mut self, owner: PlayerId, controller: PlayerId) -> ObjectId {
        let card = self.card();
        let id = self
            .zones
            .create_object(
                &mut self.ids,
                card,
                owner,
                MoveRequest {
                    controller: Some(controller),
                    ..MoveRequest::new(ZoneKind::Battlefield)
                },
            )
            .expect("create succeeds");
        // Make sure the stored controller is the requested one whatever the request default.
        self.zones
            .set_controller(id, controller)
            .expect("battlefield controller");
        let turn = self.game.turn().turn_number();
        self.permanents
            .enter(id, controller, turn)
            .expect("enter succeeds");
        id
    }

    /// A card owned by `owner` in a zone of kind `kind` (not the battlefield).
    pub fn card_in(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        assert_ne!(kind, ZoneKind::Battlefield);
        let card = self.card();
        self.zones
            .create_object(&mut self.ids, card, owner, MoveRequest::new(kind))
            .expect("create succeeds")
    }

    /// A spell on the stack owned by `owner` and controlled by `controller`.
    pub fn spell(&mut self, owner: PlayerId, controller: PlayerId) -> ObjectId {
        let card = self.card();
        self.zones
            .create_object(
                &mut self.ids,
                card,
                owner,
                MoveRequest {
                    controller: Some(controller),
                    ..MoveRequest::new(ZoneKind::Stack)
                },
            )
            .expect("create succeeds")
    }

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

    /// `capture_lki_on_move(.., id, req)` with `provider`.
    pub fn capture(
        &mut self,
        provider: &dyn CharacteristicsProvider,
        id: ObjectId,
        req: &MoveRequest,
    ) -> Result<bool, MemoryError> {
        capture_lki_on_move(
            &self.zones,
            &self.permanents,
            &self.memories,
            provider,
            &mut self.lki,
            id,
            req,
        )
    }

    pub fn lki_of(&self, id: ObjectId) -> &LkiRecord {
        self.lki.get(id).expect("LKI captured")
    }

    pub fn zone_of(&self, object: ObjectId) -> Option<ZoneKind> {
        self.zones.object(object).map(|r| r.zone.kind())
    }

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

pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// Runs `f` with a `LayeredProvider` over a snapshot of `table.game` and `objects` (the
/// wiring of the brief: snapshot, `GameEnvironment`, `LayeredProvider::new`).
pub fn with_layered<R>(
    table: &Table,
    objects: &Objects,
    f: impl FnOnce(&dyn CharacteristicsProvider) -> R,
) -> R {
    let snap = table.game.clone();
    let env = GameEnvironment::new(&snap, objects);
    let provider = LayeredProvider::new(snap.effects(), &env);
    f(&provider)
}

// ---------------------------------------------------------------------------------------------
// events and replacement effects
// ---------------------------------------------------------------------------------------------

pub fn zone_change(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

/// Replacement effect `n` (description `test.redirect_<n>`): a zone change of `object` goes
/// to `to` instead. Two of these on one object are two competing "instead" effects of the
/// same tier (CR 616.1).
pub fn redirect(n: u64, object: ObjectId, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect::new(
        eid(n),
        EventMatcher {
            affected_object: Some(object),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        Action::Modify(Modification::SetDestination(to)),
    )
    .with_description(format!("test.redirect_{n}"))
}

pub fn registry(effects: Vec<ReplacementEffect>) -> ReplacementRegistry {
    let mut r = ReplacementRegistry::new();
    for e in effects {
        r.add(e).expect("effect accepted");
    }
    r
}

// ---------------------------------------------------------------------------------------------
// replacement-bus session that records who is asked
// ---------------------------------------------------------------------------------------------

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

pub struct BusRun {
    pub after: Bus,
    pub result: Result<BusReport, BusError>,
    /// Every request the session asked, in order.
    pub requests: Vec<PendingChoiceRequest>,
}

impl BusRun {
    pub fn report(&self) -> &BusReport {
        match &self.result {
            Ok(r) => r,
            Err(e) => panic!("expected a report, got {e:?}"),
        }
    }

    /// The players the requests were addressed to, in order.
    pub fn choosers(&self) -> Vec<PlayerId> {
        self.requests.iter().map(|r| r.chooser).collect()
    }

    pub fn executed(&self) -> Vec<ExecutedEvent> {
        self.report()
            .executed
            .iter()
            .flat_map(|b| b.events.iter().cloned())
            .collect()
    }
}

/// The effect descriptions offered by a `CHOOSE_REPLACEMENT_EFFECT` request.
pub fn offered(req: &PendingChoiceRequest) -> Vec<String> {
    assert_eq!(req.choice_type, ChoiceType::ChooseReplacementEffect);
    match &req.payload {
        ChoicePayload::ChooseReplacementEffect { effects, .. } => effects.clone(),
        other => panic!("expected a replacement-effect payload, got {other:?}"),
    }
}

/// Runs `replacement::process_event(event)` on `base` with `provider` in the event context.
/// The i-th replacement choice is answered by selecting the offered effect whose description
/// is `pick[i]` (or index 0 once `pick` is exhausted).
pub fn run_bus(
    base: Bus,
    provider: &dyn CharacteristicsProvider,
    event: GameEvent,
    pick: &[&str],
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
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        let names = offered(&req);
        let index = match pick.get(requests.len()) {
            Some(want) => names
                .iter()
                .position(|n| n == want)
                .unwrap_or_else(|| panic!("{want} not offered in {names:?}")),
            None => 0,
        };
        let index = u32::try_from(index).expect("small");
        if let Err(e) = session.submit(req.request_id, ChoiceResponse::SelectReplacement { index })
        {
            panic!("answer {index} rejected: {e:?}");
        }
        requests.push(req);
        assert!(requests.len() < 16, "runaway choice loop");
    }
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

    pub fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    /// A Fisher-Yates shuffle of `v`.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).expect("fits");
            v.swap(i, j);
        }
    }
}
