// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the replacement and prevention bus: REQ-EVENT-001 (SRS 8.1 loop, CR 614.5,
// CR 615.6, CR 615.7, CR 616.1a-g, CR 704.7, CR 122.1c, CR 122.1d, CR 122.1h, CR 903.9b-style override),
// REQ-DSL-009 (tier mapping), REQ-CORE-001 / REQ-CORE-006 (determinism). Synthetic ids and
// synthetic characteristics only; no card data.

use std::collections::BTreeSet;

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId};
use fineprint_dsl::ability::TierHint;
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceTarget, ChoiceType, Interrupt, PendingChoiceRequest, Status,
};
use crate::events::{
    CounterTarget, DamageTarget, EventBatch, EventContext, EventError, EventInstance, EventState,
    GameEvent, Phase, ZoneChangeCause,
};
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::Permanents;
use crate::state::GameState;
use crate::txn::Transactional;
use crate::zones::{MoveRequest, Position, ZoneId, ZoneKind, Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

#[derive(Clone, Copy, Default)]
struct TestProvider;

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(Characteristics {
            names: BTreeSet::from(["test.vanilla_2_2".to_owned()]),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::from(["Creature".to_owned()]),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream(Vec::new()),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        })
    }
}

/// Every piece the bus reads and writes, plus the registry.
#[derive(Clone, Debug)]
struct World {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
    registry: ReplacementRegistry,
    next_card: u64,
}

impl World {
    /// Two players at 20 life, turn 1, active player 1.
    fn new() -> World {
        let mut game = GameState::new(7, 2, 20).unwrap();
        game.set_turn_number(1);
        game.set_active_player(Some(p(1))).unwrap();
        World {
            game,
            zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            registry: ReplacementRegistry::new(),
            next_card: 100,
        }
    }

    fn card(&mut self) -> CardId {
        self.next_card += 1;
        CardId::new(self.next_card)
    }

    /// A new object owned by `owner` in a zone of `kind`; on the battlefield it is a permanent
    /// controlled by `controller`.
    fn create_controlled(&mut self, owner: u64, controller: u64, kind: ZoneKind) -> ObjectId {
        let mut req = MoveRequest::new(kind);
        req.position = Position::Bottom;
        if kind == ZoneKind::Battlefield {
            req.controller = Some(p(controller));
        }
        let card = self.card();
        let id = self
            .zones
            .create_object(&mut self.ids, card, p(owner), req)
            .unwrap();
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, p(controller), 1).unwrap();
        }
        id
    }

    fn create(&mut self, owner: u64, kind: ZoneKind) -> ObjectId {
        self.create_controlled(owner, owner, kind)
    }

    /// Executes `event` directly (no bus), for setup.
    fn setup(&mut self, event: GameEvent) {
        let provider = TestProvider;
        let mut ctx = EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &provider,
        };
        ctx.execute(event).unwrap();
    }

    fn add(&mut self, effect: ReplacementEffect) {
        self.registry.add(effect).unwrap();
    }

    fn counters(&mut self, object: ObjectId, kind: &str, amount: i64) {
        self.setup(GameEvent::AddCounters {
            target: CounterTarget::Object(object),
            kind: kind.to_owned(),
            amount,
        });
    }

    fn life(&self, player: u64) -> i64 {
        self.game.player(p(player)).unwrap().life()
    }

    fn counter(&self, object: ObjectId, kind: &str) -> i64 {
        self.events.markers().counter(object, kind)
    }

    fn zone_of_card(&self, card: CardId) -> Option<ZoneId> {
        [
            ZoneId::Battlefield,
            ZoneId::Exile,
            ZoneId::Graveyard(p(1)),
            ZoneId::Graveyard(p(2)),
            ZoneId::Hand(p(1)),
            ZoneId::Hand(p(2)),
        ]
        .into_iter()
        .find(|&zone| {
            self.zones
                .contents(zone)
                .iter()
                .any(|&id| self.zones.object(id).is_some_and(|r| r.card == card))
        })
    }

    fn card_of(&self, object: ObjectId) -> CardId {
        self.zones.object(object).unwrap().card
    }
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let mut bytes = encode_to_vec(&self.game.state_hash());
        bytes.extend(encode_to_vec(&self.zones));
        bytes.extend(encode_to_vec(&self.permanents));
        bytes.extend(encode_to_vec(&self.memories));
        bytes.extend(encode_to_vec(&self.lki));
        bytes.extend(encode_to_vec(&self.events));
        bytes.extend(encode_to_vec(&self.registry));
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

type Out = Result<BusReport, BusError>;

/// The procedure body: runs the bus on `w`; a bus error becomes the (finished) output so the
/// rolled-back state can be inspected.
fn run_bus(
    w: &mut World,
    batch: &EventBatch,
    sba: bool,
    ch: &mut ChoiceContext<'_>,
) -> Result<Out, Interrupt> {
    let provider = TestProvider;
    let World {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        registry,
        ..
    } = w;
    let mut ctx = EventContext {
        game,
        zones,
        ids,
        permanents,
        memories,
        lki,
        events,
        provider: &provider,
    };
    match process_batch(&mut ctx, registry, batch, sba, ch) {
        Ok(r) => Ok(Ok(r)),
        Err(BusInterrupt::Pending(r)) => Err(Interrupt::Pending(r)),
        Err(BusInterrupt::Error(e)) => Ok(Err(e)),
    }
}

/// The result of one driven session.
struct Run {
    asked: Vec<PendingChoiceRequest>,
    state: World,
    out: Out,
    hash: Digest,
}

fn drive(w: &World, events: Vec<GameEvent>, sba: bool, answers: &[u32]) -> Run {
    let answers: Vec<ChoiceResponse> = answers
        .iter()
        .map(|&index| ChoiceResponse::SelectReplacement { index })
        .collect();
    drive_with(w, events, sba, &answers)
}

/// As [`drive`], with answers of any choice type.
fn drive_with(w: &World, events: Vec<GameEvent>, sba: bool, answers: &[ChoiceResponse]) -> Run {
    drive_batch(w, EventBatch::from_events(events), sba, answers)
}

/// As [`drive_with`], for a batch of event instances (with their applied sets).
fn drive_batch(w: &World, batch: EventBatch, sba: bool, answers: &[ChoiceResponse]) -> Run {
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, sba, ch);
    let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let mut asked = Vec::new();
    let mut answers = answers.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                assert!(answers.next().is_none(), "unused answers");
                return Run {
                    asked,
                    state: state.clone(),
                    out: output.clone(),
                    hash: session.state_hash(),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        let answer = answers.next().expect("more requests than answers").clone();
        session.submit(pending.request_id, answer).unwrap();
        asked.push(pending);
    }
}

fn ok(run: &Run) -> &BusReport {
    run.out.as_ref().expect("bus failed")
}

fn effects_of(r: &PendingChoiceRequest) -> Vec<String> {
    match &r.payload {
        ChoicePayload::ChooseReplacementEffect { effects, .. } => effects.clone(),
        other => panic!("not a replacement request: {other:?}"),
    }
}

fn desc(id: u64) -> String {
    format!("effect {id}")
}

fn damage_player(player: u64, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Player(p(player)),
        amount,
        combat: false,
    }
}

fn damage_object(object: ObjectId, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: None,
        source_controller: None,
        target: DamageTarget::Object(object),
        amount,
        combat: false,
    }
}

fn damage_from(source: ObjectId, object: ObjectId, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: Some(source),
        source_controller: None,
        target: DamageTarget::Object(object),
        amount,
        combat: false,
    }
}

fn damage_from_to_player(source: ObjectId, player: u64, amount: i64) -> GameEvent {
    GameEvent::Damage {
        source: Some(source),
        source_controller: None,
        target: DamageTarget::Player(p(player)),
        amount,
        combat: false,
    }
}

/// A `SUBMIT_DIVISION` answer giving each source its share.
fn division(shares: &[(ObjectId, u64)]) -> ChoiceResponse {
    ChoiceResponse::SubmitDivision {
        assignments: shares
            .iter()
            .map(|&(s, n)| (ChoiceTarget::Object(s), n))
            .collect(),
    }
}

fn life(player: u64, delta: i64) -> GameEvent {
    GameEvent::LifeChange {
        player: p(player),
        delta,
    }
}

fn zc(object: ObjectId, to: ZoneKind, cause: ZoneChangeCause) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause,
    }
}

fn add_one(id: u64, kind: BusEventKind) -> ReplacementEffect {
    ReplacementEffect::new(
        e(id),
        EventMatcher::new(kind),
        Action::Modify(Modification::Amount(AmountOp::Add(1))),
    )
}

fn prevent_all(id: u64) -> ReplacementEffect {
    ReplacementEffect::new(
        e(id),
        EventMatcher::new(BusEventKind::Damage),
        Action::Prevent { then: Vec::new() },
    )
}

const TIERS: [Tier; 5] = [
    Tier::SelfReplacement,
    Tier::ControlEtb,
    Tier::CopyEtb,
    Tier::BackFaceEtb,
    Tier::Other,
];

// ---------------------------------------------------------------------------------------------
// Model types
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: every event kind maps to its bus kind and SRS wire name.
#[test]
fn req_event_001_bus_event_kind_of_and_name() {
    let o = ObjectId::new(1);
    let cases = [
        (
            zc(o, ZoneKind::Hand, ZoneChangeCause::Other),
            BusEventKind::ZoneChange,
            "ZONE_CHANGE",
        ),
        (GameEvent::Draw { player: p(1) }, BusEventKind::Draw, "DRAW"),
        (damage_player(1, 1), BusEventKind::Damage, "DAMAGE"),
        (life(1, 1), BusEventKind::LifeChange, "LIFE_CHANGE"),
        (
            GameEvent::AddCounters {
                target: CounterTarget::Object(o),
                kind: "k".into(),
                amount: 1,
            },
            BusEventKind::AddCounters,
            "ADD_COUNTERS",
        ),
        (
            GameEvent::RemoveCounters {
                target: CounterTarget::Object(o),
                kind: "k".into(),
                amount: 1,
            },
            BusEventKind::RemoveCounters,
            "REMOVE_COUNTERS",
        ),
        (GameEvent::Tap { object: o }, BusEventKind::Tap, "TAP"),
        (GameEvent::Untap { object: o }, BusEventKind::Untap, "UNTAP"),
        (
            GameEvent::CreateToken {
                owner: p(1),
                card: CardId::new(1),
                controller: None,
            },
            BusEventKind::CreateToken,
            "CREATE_TOKEN",
        ),
        (
            GameEvent::TurnBegins { player: p(1) },
            BusEventKind::TurnBegins,
            "TURN_BEGINS",
        ),
        (
            GameEvent::PhaseBegins {
                phase: Phase::Combat,
            },
            BusEventKind::PhaseBegins,
            "PHASE_BEGINS",
        ),
        (
            GameEvent::StepBegins {
                step: crate::events::Step::Upkeep,
            },
            BusEventKind::StepBegins,
            "STEP_BEGINS",
        ),
        (
            GameEvent::Attach {
                attachment: o,
                target: crate::permanent::AttachTarget::Player(p(1)),
            },
            BusEventKind::Attach,
            "ATTACH",
        ),
        (
            GameEvent::Unattach { attachment: o },
            BusEventKind::Unattach,
            "UNATTACH",
        ),
        (
            GameEvent::BecomeCopy {
                object: o,
                copied: o,
            },
            BusEventKind::BecomeCopy,
            "BECOME_COPY",
        ),
        (
            GameEvent::TurnFaceUp { object: o },
            BusEventKind::TurnFaceUp,
            "TURN_FACE_UP",
        ),
        (
            GameEvent::TurnFaceDown { object: o },
            BusEventKind::TurnFaceDown,
            "TURN_FACE_DOWN",
        ),
    ];
    for (i, (event, kind, name)) in cases.iter().enumerate() {
        assert_eq!(BusEventKind::of(event), *kind);
        assert_eq!(kind.name(), *name);
        assert_eq!(encode_to_vec(kind), vec![u8::try_from(i).unwrap()]);
    }
}

/// REQ-DSL-009, REQ-EVENT-001: CR 614.15 -> CR 616.1a wins over a hint; hints map to
/// CR 616.1b-d; otherwise CR 616.1e.
#[test]
fn req_dsl_009_tier_from_ability() {
    assert_eq!(tier_from_ability(Some(true), None), Tier::SelfReplacement);
    assert_eq!(
        tier_from_ability(Some(true), Some(TierHint::CopyEtb)),
        Tier::SelfReplacement
    );
    assert_eq!(
        tier_from_ability(Some(false), Some(TierHint::ControlEtb)),
        Tier::ControlEtb
    );
    assert_eq!(
        tier_from_ability(None, Some(TierHint::CopyEtb)),
        Tier::CopyEtb
    );
    assert_eq!(
        tier_from_ability(None, Some(TierHint::BackFaceEtb)),
        Tier::BackFaceEtb
    );
    assert_eq!(tier_from_ability(None, None), Tier::Other);
    assert_eq!(tier_from_ability(Some(false), None), Tier::Other);
    // Derive order is precedence (CR 616.1a-e).
    let mut sorted = TIERS;
    sorted.sort();
    assert_eq!(sorted, TIERS);
}

/// REQ-EVENT-001: the builder methods and constructors set exactly their field.
#[test]
fn req_event_001_effect_builders() {
    let base = add_one(3, BusEventKind::Damage);
    assert_eq!(base.tier, Tier::Other);
    assert_eq!(base.source, None);
    assert_eq!(base.description, "effect 3");
    assert!(!base.contained && !base.overrides_614_5);
    assert_eq!(base.shield, None);
    let built = base
        .clone()
        .with_tier(Tier::CopyEtb)
        .with_source(ObjectId::new(9))
        .with_description("d")
        .with_contained(true)
        .with_overrides_614_5(true)
        .with_shield(Shield::Uses(2));
    assert_eq!(built.tier, Tier::CopyEtb);
    assert_eq!(built.source, Some(ObjectId::new(9)));
    assert_eq!(built.description, "d");
    assert!(built.contained && built.overrides_614_5);
    assert_eq!(built.shield, Some(Shield::Uses(2)));
    assert_eq!(built.action, base.action);
    assert_eq!(
        EventMatcher::new(BusEventKind::Tap).kind,
        Some(BusEventKind::Tap)
    );
    assert_eq!(EventMatcher::default().kind, None);
    assert_eq!(AmountOp::Add(-5).apply(3), 0);
    assert_eq!(AmountOp::Add(i64::MAX).apply(3), i64::MAX);
    assert_eq!(AmountOp::Mul(2).apply(i64::MAX), i64::MAX);
    assert_eq!(AmountOp::Set(-1).apply(3), 0);
    assert_eq!(AmountOp::Set(4).apply(3), 4);
}

/// REQ-EVENT-001: `add` rejects duplicates and every malformed effect, and leaves the registry
/// unchanged.
#[test]
fn req_event_001_registry_validation() {
    let mut r = ReplacementRegistry::new();
    assert!(r.is_empty());
    r.add(add_one(5, BusEventKind::Damage)).unwrap();
    r.add(add_one(2, BusEventKind::LifeChange)).unwrap();
    assert_eq!(r.ids(), vec![e(2), e(5)]);
    assert_eq!(r.len(), 2);
    assert_eq!(
        r.add(add_one(5, BusEventKind::AddCounters)),
        Err(BusError::DuplicateEffect(e(5)))
    );
    let m = |k| EventMatcher::new(k);
    let modify = |id, k, md| ReplacementEffect::new(e(id), m(k), Action::Modify(md));
    let invalid = [
        ReplacementEffect::new(e(10), EventMatcher::default(), Action::Skip),
        ReplacementEffect::new(
            e(11),
            m(BusEventKind::Tap),
            Action::Prevent { then: Vec::new() },
        ),
        prevent_all(12).with_shield(Shield::Uses(0)),
        prevent_all(13).with_shield(Shield::Pool(0)),
        prevent_all(14).with_shield(Shield::Pool(-3)),
        ReplacementEffect::new(e(15), m(BusEventKind::Damage), Action::Skip)
            .with_shield(Shield::Pool(2)),
        ReplacementEffect::new(e(16), m(BusEventKind::LifeChange), Action::Skip)
            .with_shield(Shield::Pool(2)),
        modify(
            17,
            BusEventKind::Damage,
            Modification::SetDestination(ZoneKind::Exile),
        ),
        modify(
            18,
            BusEventKind::CreateToken,
            Modification::SetFaceDown(true),
        ),
        modify(19, BusEventKind::Tap, Modification::SetController(p(1))),
        modify(
            20,
            BusEventKind::Draw,
            Modification::EntersAsCopyOf(ObjectId::new(1)),
        ),
        modify(
            21,
            BusEventKind::Tap,
            Modification::Amount(AmountOp::Add(1)),
        ),
        modify(
            22,
            BusEventKind::Damage,
            Modification::Amount(AmountOp::Mul(-1)),
        ),
    ];
    for effect in invalid {
        let id = effect.id;
        match r.add(effect) {
            Err(BusError::InvalidEffect(msg)) => assert!(!msg.is_empty()),
            other => panic!("effect {id:?} accepted: {other:?}"),
        }
    }
    assert_eq!(r.ids(), vec![e(2), e(5)]);
    // Every valid pairing is accepted.
    let valid = [
        modify(
            30,
            BusEventKind::ZoneChange,
            Modification::SetDestination(ZoneKind::Exile),
        ),
        modify(
            31,
            BusEventKind::ZoneChange,
            Modification::SetFaceDown(true),
        ),
        modify(
            32,
            BusEventKind::ZoneChange,
            Modification::SetController(p(2)),
        ),
        modify(
            33,
            BusEventKind::CreateToken,
            Modification::SetController(p(2)),
        ),
        modify(
            34,
            BusEventKind::ZoneChange,
            Modification::EntersAsCopyOf(ObjectId::new(1)),
        ),
        modify(
            35,
            BusEventKind::CreateToken,
            Modification::EntersAsCopyOf(ObjectId::new(1)),
        ),
        modify(
            36,
            BusEventKind::RemoveCounters,
            Modification::Amount(AmountOp::Mul(0)),
        ),
        prevent_all(37).with_shield(Shield::Pool(1)),
        ReplacementEffect::new(e(38), m(BusEventKind::Tap), Action::Skip)
            .with_shield(Shield::Uses(1)),
    ];
    for effect in valid {
        r.add(effect).unwrap();
    }
    assert!(r.get(e(37)).is_some());
    assert_eq!(r.remove(e(37)).map(|x| x.id), Some(e(37)));
    assert_eq!(r.remove(e(37)), None);
    assert!(r.get(e(37)).is_none());
    // Display and source of the errors.
    for err in [
        BusError::DuplicateEffect(e(1)),
        BusError::InvalidEffect("x".into()),
        BusError::Event(EventError::Overflow),
        BusError::Choice(ChoiceError::NotPending),
        BusError::DepthExceeded,
        BusError::RepeatLimit,
        BusError::WorkLimit,
    ] {
        assert!(!err.to_string().is_empty());
        let has_source = std::error::Error::source(&err).is_some();
        assert_eq!(
            has_source,
            matches!(err, BusError::Event(_) | BusError::Choice(_))
        );
    }
}

/// REQ-EVENT-001, REQ-CORE-006: the registry's canonical bytes are deterministic and change
/// when a shield use is consumed.
#[test]
fn req_event_001_registry_encoding_tracks_shield() {
    let mut w = World::new();
    w.add(prevent_all(1).with_shield(Shield::Uses(2)));
    w.add(
        ReplacementEffect::new(
            e(2),
            EventMatcher {
                from: Some(ZoneKind::Hand),
                to: Some(ZoneKind::Exile),
                cause: Some(ZoneChangeCause::Discard),
                combat: Some(true),
                counter_kind: Some("k".into()),
                amount_below: Some(3),
                requires_counter: Some((ObjectId::new(1), "k".into())),
                affected_object: Some(ObjectId::new(1)),
                affected_player: Some(p(1)),
                ..EventMatcher::new(BusEventKind::ZoneChange)
            },
            Action::Modify(Modification::SetDestination(ZoneKind::Graveyard)),
        )
        .with_source(ObjectId::new(4)),
    );
    let before = encode_to_vec(&w.registry);
    assert_eq!(before, encode_to_vec(&w.registry.clone()));
    let run = drive(&w, vec![damage_player(2, 3)], false, &[]);
    ok(&run);
    let after = encode_to_vec(&run.state.registry);
    assert_ne!(before, after);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Uses(1))
    );
    // Every action, modification, op and shield encodes distinctly.
    let actions = [
        Action::Skip,
        Action::Instead(vec![life(1, 1)]),
        Action::Prevent { then: vec![] },
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        Action::Modify(Modification::SetController(p(1))),
        Action::Modify(Modification::SetFaceDown(true)),
        Action::Modify(Modification::Amount(AmountOp::Add(1))),
        Action::Modify(Modification::Amount(AmountOp::Mul(1))),
        Action::Modify(Modification::Amount(AmountOp::Set(1))),
        Action::Modify(Modification::EntersAsCopyOf(ObjectId::new(1))),
    ];
    let bytes: BTreeSet<Vec<u8>> = actions.iter().map(encode_to_vec).collect();
    assert_eq!(bytes.len(), actions.len());
    assert_ne!(
        encode_to_vec(&Shield::Uses(1)),
        encode_to_vec(&Shield::Pool(1))
    );
    let tiers: BTreeSet<Vec<u8>> = TIERS.iter().map(encode_to_vec).collect();
    assert_eq!(tiers.len(), 5);
}

// ---------------------------------------------------------------------------------------------
// Loop basics
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001 (SRS 8.1 step [1]): R(E) empty executes E as it is; an empty batch does
/// nothing.
#[test]
fn req_event_001_no_effect_executes_event() {
    let w = World::new();
    let run = drive(&w, vec![damage_player(2, 3)], false, &[]);
    let report = ok(&run);
    assert!(run.asked.is_empty());
    assert!(report.applications.is_empty());
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(run.state.life(2), 17);
    let empty = drive(&w, vec![], false, &[]);
    assert_eq!(
        ok(&empty),
        &BusReport {
            executed: vec![],
            applications: vec![],
            prevented: 0
        }
    );
}

/// REQ-EVENT-001: an event that is skipped is not executed and nothing is logged (SRS 8.1).
#[test]
fn req_event_001_skip_logs_nothing() {
    let mut w = World::new();
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Skip,
    ));
    let run = drive(&w, vec![life(1, 5)], false, &[]);
    let report = ok(&run);
    assert!(report.executed.is_empty());
    assert_eq!(report.applications, vec![e(1)]);
    assert!(run.state.events.log().is_empty());
    assert_eq!(run.state.life(1), 20);
}

/// REQ-EVENT-001: the executed instance carries the applied set (CR 614.5) in the log.
#[test]
fn req_event_001_applied_set_logged() {
    let mut w = World::new();
    w.add(add_one(4, BusEventKind::LifeChange));
    let run = drive(&w, vec![life(1, 2)], false, &[]);
    let report = ok(&run);
    let ev = &report.executed[0].events[0];
    assert_eq!(ev.instance.event(), &life(1, 3));
    assert_eq!(
        ev.instance.applied_effects().iter().collect::<Vec<_>>(),
        vec![e(4)]
    );
    assert_eq!(run.state.events.log().batches(), report.executed.as_slice());
    assert_eq!(run.state.life(1), 23);
}

/// REQ-EVENT-001: life-change amount modifications keep the sign; a 0 result drops the event.
#[test]
fn req_event_001_life_change_amounts() {
    let mut w = World::new();
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Modify(Modification::Amount(AmountOp::Mul(2))),
    ));
    let run = drive(&w, vec![life(1, -3), life(2, 4)], false, &[]);
    ok(&run);
    assert_eq!(run.state.life(1), 14);
    assert_eq!(run.state.life(2), 28);
    let mut w = World::new();
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Modify(Modification::Amount(AmountOp::Set(0))),
    ));
    let run = drive(&w, vec![life(1, -3)], false, &[]);
    assert!(ok(&run).executed.is_empty());
    assert_eq!(run.state.life(1), 20);
}

// ---------------------------------------------------------------------------------------------
// Tiers (CR 616.1a-e)
// ---------------------------------------------------------------------------------------------

/// Two effects in tier `a` and two in tier `b` on one damage event: `a` is offered first,
/// then (once no `a` effect remains) `b`.
fn tier_pair_case(a: Tier, b: Tier, a_low_ids: bool) {
    let mut w = World::new();
    let (a1, a2, b1, b2) = if a_low_ids {
        (1, 2, 3, 4)
    } else {
        (3, 4, 1, 2)
    };
    for (id, t) in [(a1, a), (a2, a), (b1, b), (b2, b)] {
        w.add(add_one(id, BusEventKind::Damage).with_tier(t));
    }
    let run = drive(&w, vec![damage_player(2, 1)], false, &[0, 0]);
    let report = ok(&run);
    assert_eq!(run.asked.len(), 2, "{a:?} {b:?}");
    assert_eq!(effects_of(&run.asked[0]), vec![desc(a1), desc(a2)]);
    assert_eq!(effects_of(&run.asked[1]), vec![desc(b1), desc(b2)]);
    assert_eq!(report.applications, vec![e(a1), e(a2), e(b1), e(b2)]);
    assert_eq!(run.state.life(2), 15);
}

/// REQ-EVENT-001: CR 616.1a-e, every pair of tiers (adjacent and not), in both EffectId
/// orders: the earlier tier is offered alone first; the later tier only once it is empty.
#[test]
fn req_event_001_cr_616_1_every_tier_boundary() {
    for (i, &a) in TIERS.iter().enumerate() {
        for &b in &TIERS[i + 1..] {
            tier_pair_case(a, b, true);
            tier_pair_case(a, b, false);
        }
    }
}

/// REQ-EVENT-001: CR 616.1e, two `Other` effects ask; one effect in a tier is applied without
/// asking.
#[test]
fn req_event_001_cr_616_1_single_effect_no_ask() {
    let mut w = World::new();
    w.add(add_one(1, BusEventKind::Damage));
    let run = drive(&w, vec![damage_player(2, 1)], false, &[]);
    assert!(run.asked.is_empty());
    assert_eq!(ok(&run).applications, vec![e(1)]);
    w.add(add_one(2, BusEventKind::Damage));
    let run = drive(&w, vec![damage_player(2, 1)], false, &[1]);
    assert_eq!(run.asked.len(), 1);
    assert_eq!(ok(&run).applications, vec![e(2), e(1)]);
}

/// REQ-EVENT-001: SRS regression "a 616.1 tier test (copy ETB versus control ETB)": CR 616.1b
/// control-ETB applies before CR 616.1c copy-ETB, whichever has the lower EffectId.
#[test]
fn req_event_001_cr_616_1b_control_beats_copy_etb() {
    for (control, copy) in [(1, 2), (2, 1)] {
        let mut w = World::new();
        let original = w.create(1, ZoneKind::Battlefield);
        let token_card = w.card();
        w.add(
            ReplacementEffect::new(
                e(control),
                EventMatcher::new(BusEventKind::CreateToken),
                Action::Modify(Modification::SetController(p(2))),
            )
            .with_tier(Tier::ControlEtb),
        );
        w.add(
            ReplacementEffect::new(
                e(copy),
                EventMatcher::new(BusEventKind::CreateToken),
                Action::Modify(Modification::EntersAsCopyOf(original)),
            )
            .with_tier(Tier::CopyEtb),
        );
        let event = GameEvent::CreateToken {
            owner: p(1),
            card: token_card,
            controller: None,
        };
        let run = drive(&w, vec![event], false, &[]);
        let report = ok(&run);
        assert!(run.asked.is_empty());
        assert_eq!(report.applications, vec![e(control), e(copy)]);
        // The token entered under player 2 as a copy (CR 707.5: one executed batch, the copy
        // link is part of the entry; no BecomeCopy event).
        assert_eq!(report.executed.len(), 1);
        assert_eq!(report.executed[0].events.len(), 1);
        let token = match report.executed[0].events[0].outcome {
            crate::events::Outcome::TokenCreated { object } => object,
            other => panic!("{other:?}"),
        };
        assert_eq!(run.state.permanents.get(token).unwrap().controller(), p(2));
        assert_eq!(run.state.events.markers().copy_of(token), Some(original));
    }
}

// ---------------------------------------------------------------------------------------------
// Contained events (CR 616.1g)
// ---------------------------------------------------------------------------------------------

fn token_event(w: &mut World) -> GameEvent {
    let card = w.card();
    GameEvent::CreateToken {
        owner: p(1),
        card,
        controller: None,
    }
}

/// REQ-EVENT-001: CR 616.1g (SRS regression: the nested "enters" inside "create token",
/// modelled synthetically): the contained effect is not offered while the non-contained one
/// applies; it applies afterwards; both EffectId orders.
#[test]
fn req_event_001_cr_616_1g_contained_waits() {
    for (outer, inner) in [(1, 2), (2, 1)] {
        let mut w = World::new();
        let original = w.create(2, ZoneKind::Battlefield);
        w.add(ReplacementEffect::new(
            e(outer),
            EventMatcher::new(BusEventKind::CreateToken),
            Action::Modify(Modification::SetController(p(2))),
        ));
        w.add(
            ReplacementEffect::new(
                e(inner),
                EventMatcher::new(BusEventKind::CreateToken),
                Action::Modify(Modification::EntersAsCopyOf(original)),
            )
            .with_contained(true),
        );
        let event = token_event(&mut w);
        // Same tier: without CR 616.1g the two would be offered together.
        let run = drive(&w, vec![event], false, &[]);
        assert!(run.asked.is_empty());
        assert_eq!(ok(&run).applications, vec![e(outer), e(inner)]);
    }
}

/// REQ-EVENT-001: CR 616.1g, a contained effect alone is offered at once; a contained effect of
/// a higher tier still waits for a non-contained effect of a lower tier (the contained filter
/// precedes the tier restriction).
#[test]
fn req_event_001_cr_616_1g_alone_and_before_tiers() {
    let mut w = World::new();
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::CreateToken),
            Action::Modify(Modification::SetController(p(2))),
        )
        .with_contained(true)
        .with_tier(Tier::ControlEtb),
    );
    let event = token_event(&mut w);
    let run = drive(&w, vec![event.clone()], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    // Two contained effects alone: offered together.
    w.add(
        ReplacementEffect::new(
            e(2),
            EventMatcher::new(BusEventKind::CreateToken),
            Action::Modify(Modification::SetController(p(1))),
        )
        .with_contained(true)
        .with_tier(Tier::ControlEtb),
    );
    let run = drive(&w, vec![event.clone()], false, &[1]);
    assert_eq!(effects_of(&run.asked[0]), vec![desc(1), desc(2)]);
    assert_eq!(ok(&run).applications, vec![e(2), e(1)]);
    // A non-contained Other effect goes first, although the contained ones have a higher tier.
    w.add(ReplacementEffect::new(
        e(3),
        EventMatcher::new(BusEventKind::CreateToken),
        Action::Modify(Modification::SetController(p(1))),
    ));
    let run = drive(&w, vec![event], false, &[0]);
    assert_eq!(effects_of(&run.asked[0]), vec![desc(1), desc(2)]);
    assert_eq!(ok(&run).applications, vec![e(3), e(1), e(2)]);
}

// ---------------------------------------------------------------------------------------------
// CR 614.5 and the override flag
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: CR 614.5, an applied effect is not offered again, although it still matches
/// the modified event; an event submitted with an applied set excludes it from the start.
#[test]
fn req_event_001_cr_614_5_applies_once() {
    let mut w = World::new();
    w.add(add_one(1, BusEventKind::Damage));
    let run = drive(&w, vec![damage_player(2, 1)], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.life(2), 18);
    // Pre-applied.
    let mut applied = crate::events::AppliedEffects::new();
    applied.insert(e(1));
    let batch = EventBatch::new(vec![
        crate::events::EventInstance::new(damage_player(2, 1)).with_applied(applied),
    ]);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let session = ChoiceSession::start(w.clone(), procedure).unwrap();
    match session.status() {
        Status::Done { state, output } => {
            assert!(output.as_ref().unwrap().applications.is_empty());
            assert_eq!(state.life(2), 19);
        }
        Status::Pending(_) => panic!("unexpected request"),
    }
}

/// REQ-EVENT-001: CR 614.5 exception (CR 903.9b-style): an overriding effect applies again
/// while it matches (bounded by `amount_below`); an unbounded one hits `RepeatLimit` and
/// changes nothing.
#[test]
fn req_event_001_cr_614_5_override() {
    let mut w = World::new();
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                amount_below: Some(5),
                ..EventMatcher::new(BusEventKind::Damage)
            },
            Action::Modify(Modification::Amount(AmountOp::Add(1))),
        )
        .with_overrides_614_5(true),
    );
    let run = drive(&w, vec![damage_player(2, 1)], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1); 4]);
    assert_eq!(run.state.life(2), 15);
    let ev = &ok(&run).executed[0].events[0];
    assert!(ev.instance.applied_effects().contains(e(1)));

    let mut w = World::new();
    w.add(add_one(1, BusEventKind::Damage).with_overrides_614_5(true));
    let before = w.state_hash();
    let run = drive(&w, vec![damage_player(2, 1)], false, &[]);
    assert_eq!(run.out, Err(BusError::RepeatLimit));
    assert_eq!(run.state.state_hash(), before);
    assert_eq!(MAX_REPEATS, 256);
}

// ---------------------------------------------------------------------------------------------
// Prevention (CR 615.6, CR 615.7) and counters (CR 122.1c, CR 122.1d, CR 122.1h)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: CR 615.6, prevent-all prevents the whole amount; nothing is executed.
#[test]
fn req_event_001_cr_615_prevent_all() {
    let mut w = World::new();
    w.add(prevent_all(1));
    let run = drive(&w, vec![damage_player(2, 7)], false, &[]);
    let report = ok(&run);
    assert_eq!(report.prevented, 7);
    assert!(report.executed.is_empty());
    assert_eq!(run.state.life(2), 20);
    assert!(run.state.registry.get(e(1)).is_some());
}

/// REQ-EVENT-001: CR 615.7, CR 615.8, `Shield::Uses` decrements per application and the effect is
/// removed at 0.
#[test]
fn req_event_001_cr_615_shield_uses() {
    let mut w = World::new();
    w.add(prevent_all(1).with_shield(Shield::Uses(2)));
    let run = drive(&w, vec![damage_player(2, 3)], false, &[]);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Uses(1))
    );
    let run = drive(&run.state, vec![damage_player(2, 3)], false, &[]);
    assert_eq!(ok(&run).prevented, 3);
    assert!(run.state.registry.get(e(1)).is_none());
    let run = drive(&run.state, vec![damage_player(2, 3)], false, &[]);
    assert_eq!(ok(&run).prevented, 0);
    assert_eq!(run.state.life(2), 17);
}

/// REQ-EVENT-001: CR 615.7, `Shield::Pool` prevents partially (the rest is dealt) and totally;
/// the drained effect is removed.
#[test]
fn req_event_001_cr_615_shield_pool() {
    let mut w = World::new();
    w.add(prevent_all(1).with_shield(Shield::Pool(5)));
    let run = drive(&w, vec![damage_player(2, 3)], false, &[]);
    assert_eq!(ok(&run).prevented, 3);
    assert_eq!(run.state.life(2), 20);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Pool(2))
    );
    let run = drive(&run.state, vec![damage_player(2, 4)], false, &[]);
    assert_eq!(ok(&run).prevented, 2);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.life(2), 18);
    assert!(run.state.registry.get(e(1)).is_none());
}

/// REQ-EVENT-001: CR 122.1c, CR 615.1: a shield counter prevents damage and exactly one shield
/// counter is removed by a child event that itself goes through the bus; without the counter
/// the effect no longer applies.
#[test]
fn req_event_001_cr_122_1c_shield_counter_damage() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "shield", 3);
    w.add(ReplacementEffect::shield_counter_damage(e(1), x));
    let run = drive(&w, vec![damage_object(x, 4)], false, &[]);
    let report = ok(&run);
    assert_eq!(report.prevented, 4);
    assert_eq!(run.state.counter(x, "shield"), 2);
    assert_eq!(run.state.events.markers().damage(x), 0);
    assert_eq!(report.executed.len(), 1); // only the child RemoveCounters
    // A replacement on the child RemoveCounters applies too.
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher {
            counter_kind: Some("shield".into()),
            ..EventMatcher::new(BusEventKind::RemoveCounters)
        },
        Action::Modify(Modification::Amount(AmountOp::Add(1))),
    ));
    let run = drive(&w, vec![damage_object(x, 4)], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1), e(2)]);
    assert_eq!(run.state.counter(x, "shield"), 1);
    // Counter gone: damage is dealt.
    let mut w = World::new();
    let y = w.create(1, ZoneKind::Battlefield);
    w.add(ReplacementEffect::shield_counter_damage(e(1), y));
    let run = drive(&w, vec![damage_object(y, 2)], false, &[]);
    assert!(ok(&run).applications.is_empty());
    assert_eq!(run.state.events.markers().damage(y), 2);
}

/// REQ-EVENT-001: CR 122.1c, destruction is replaced by removing a shield counter; another
/// cause is not.
#[test]
fn req_event_001_cr_122_1c_shield_counter_destroy() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "shield", 1);
    w.add(ReplacementEffect::shield_counter_destroy(e(1), x));
    let run = drive(
        &w,
        vec![zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy)],
        false,
        &[],
    );
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert!(run.state.permanents.get(x).is_some());
    assert_eq!(run.state.counter(x, "shield"), 0);
    // Now the counter is gone: destruction happens.
    let card = w.card_of(x);
    let run2 = drive(
        &run.state,
        vec![zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy)],
        false,
        &[],
    );
    assert!(ok(&run2).applications.is_empty());
    assert_eq!(run2.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    // Sacrifice is not destruction.
    let run3 = drive(
        &w,
        vec![zc(x, ZoneKind::Graveyard, ZoneChangeCause::Sacrifice)],
        false,
        &[],
    );
    assert!(ok(&run3).applications.is_empty());
}

/// REQ-EVENT-001: CR 122.1d, untapping is replaced by removing a stun counter.
#[test]
fn req_event_001_cr_122_1d_stun_counter() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.setup(GameEvent::Tap { object: x });
    w.counters(x, "stun", 1);
    w.add(ReplacementEffect::stun_counter(e(1), x));
    let run = drive(&w, vec![GameEvent::Untap { object: x }], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert!(run.state.permanents.get(x).unwrap().status().tapped);
    assert_eq!(run.state.counter(x, "stun"), 0);
    let run = drive(&run.state, vec![GameEvent::Untap { object: x }], false, &[]);
    assert!(ok(&run).applications.is_empty());
    assert!(!run.state.permanents.get(x).unwrap().status().tapped);
}

/// REQ-EVENT-001: CR 122.1h, from the battlefield to a graveyard goes to exile instead; from
/// another zone it does not apply.
#[test]
fn req_event_001_cr_122_1h_finality_counter() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let h = w.create(1, ZoneKind::Hand);
    w.counters(x, "finality", 1);
    w.counters(h, "finality", 1);
    w.add(ReplacementEffect::finality_counter(e(1), x));
    w.add(ReplacementEffect::finality_counter(e(2), h));
    let (cx, ch) = (w.card_of(x), w.card_of(h));
    let run = drive(
        &w,
        vec![
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            zc(h, ZoneKind::Graveyard, ZoneChangeCause::Discard),
        ],
        false,
        &[],
    );
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.zone_of_card(cx), Some(ZoneId::Exile));
    assert_eq!(run.state.zone_of_card(ch), Some(ZoneId::Graveyard(p(1))));
}

// ---------------------------------------------------------------------------------------------
// CR 704.7
// ---------------------------------------------------------------------------------------------

fn sba_world(controllers: &[u64]) -> (World, Vec<GameEvent>) {
    let mut w = World::new();
    let mut events = Vec::new();
    for &c in controllers {
        let x = w.create_controlled(1, c, ZoneKind::Battlefield);
        events.push(zc(
            x,
            ZoneKind::Graveyard,
            ZoneChangeCause::StateBasedAction,
        ));
    }
    let to_gy = || EventMatcher {
        to: Some(ZoneKind::Graveyard),
        ..EventMatcher::new(BusEventKind::ZoneChange)
    };
    w.add(
        ReplacementEffect::new(
            e(1),
            to_gy(),
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_shield(Shield::Uses(5)),
    );
    w.add(ReplacementEffect::new(
        e(2),
        to_gy(),
        Action::Modify(Modification::SetDestination(ZoneKind::Hand)),
    ));
    (w, events)
}

/// REQ-EVENT-001: CR 704.7, an sba batch of three DIFFERENT permanents' moves is three
/// results: three requests, three applications, three shield uses (round 1, clause j). Two
/// moves of the SAME permanent to the same zone are one result: one request, one application,
/// one shield use, one executed move. With sba false each event asks.
#[test]
fn req_event_001_cr_704_7_merge() {
    let (w, events) = sba_world(&[1, 1, 1]);
    let run = drive(&w, events.clone(), true, &[0, 0, 0]);
    let report = ok(&run);
    assert_eq!(run.asked.len(), 3);
    assert_eq!(report.applications, vec![e(1); 3]);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Uses(2))
    );
    assert_eq!(run.state.zones.contents(ZoneId::Exile).len(), 3);
    // The same permanent twice: merged (CR 704.7).
    let (w1, one) = sba_world(&[1]);
    let twice = vec![one[0].clone(), one[0].clone()];
    let run = drive(&w1, twice, true, &[0]);
    let report = ok(&run);
    assert_eq!(run.asked.len(), 1);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Uses(4))
    );
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(run.state.zones.contents(ZoneId::Exile).len(), 1);
    let run = drive(&w, events, false, &[0, 0, 0]);
    assert_eq!(run.asked.len(), 3);
    assert_eq!(ok(&run).applications, vec![e(1); 3]);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Uses(2))
    );
}

/// REQ-EVENT-001: CR 704.7, CR 616.1: moves of different objects never merge, whoever
/// controls them; one request per object, the active player's first (APNAP), whatever the
/// submission order (round 1, clause j).
#[test]
fn req_event_001_cr_704_7_different_choosers_split() {
    let (w, events) = sba_world(&[2, 1]);
    let run = drive(&w, events, true, &[0, 1]);
    assert_eq!(run.asked.len(), 2);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(run.asked[1].chooser, p(2));
    assert_eq!(ok(&run).applications, vec![e(1), e(2)]);
    // Two objects of player 1 and one of player 2: three requests, player 1's first.
    let (w, events) = sba_world(&[1, 2, 1]);
    let run = drive(&w, events, true, &[0, 1, 0]);
    assert_eq!(run.asked.len(), 3);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(run.asked[1].chooser, p(1));
    assert_eq!(run.asked[2].chooser, p(2));
    assert_eq!(ok(&run).applications, vec![e(1), e(2), e(1)]);
}

/// REQ-EVENT-001: CR 704.7 does not merge damage; CR 615.7: a pool facing simultaneous damage
/// from two sources that it cannot cover is divided by the chooser (round 1, F8 protocol), and
/// each damage event is its own application.
#[test]
fn req_event_001_cr_704_7_merge_single_effect_and_pool() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let y = w.create(1, ZoneKind::Battlefield);
    let a = w.create(2, ZoneKind::Battlefield);
    let b = w.create(2, ZoneKind::Battlefield);
    let (a, b) = (a.min(b), a.max(b));
    w.add(prevent_all(1).with_shield(Shield::Pool(5)));
    let run = drive_with(
        &w,
        vec![damage_from(a, x, 3), damage_from(b, y, 3)],
        true,
        &[division(&[(a, 3), (b, 2)])],
    );
    let report = ok(&run);
    assert_eq!(run.asked.len(), 1);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(report.prevented, 5);
    assert_eq!(run.state.events.markers().damage(x), 0);
    assert_eq!(run.state.events.markers().damage(y), 1);
    assert!(run.state.registry.get(e(1)).is_none());
}

// ---------------------------------------------------------------------------------------------
// Child events
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: SRS 8.1, `Instead` children re-enter the bus and can be replaced. CR 614.6,
/// CR 704.3 (round 5): the `Instead` child is the modified event that occurs instead of the tap,
/// so it executes in the same batch as the sibling life change.
#[test]
fn req_event_001_children_reenter_bus() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::Tap),
        Action::Instead(vec![life(1, 1)]),
    ));
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Modify(Modification::Amount(AmountOp::Mul(3))),
    ));
    let run = drive(
        &w,
        vec![GameEvent::Tap { object: x }, life(2, 1)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2), e(2)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(run.state.life(1), 23);
    assert_eq!(run.state.life(2), 23);
    assert!(!run.state.permanents.get(x).unwrap().status().tapped);
}

/// REQ-EVENT-001: CR 707.5: `EntersAsCopyOf` on a zone change to the battlefield makes the new
/// object a copy as part of its entry (one executed batch, no `BecomeCopy` event); if the
/// final event does not enter the battlefield nothing happens.
#[test]
fn req_event_001_enters_as_copy_child() {
    let mut w = World::new();
    let original = w.create(2, ZoneKind::Battlefield);
    let h = w.create(1, ZoneKind::Hand);
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::ZoneChange),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    ));
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    let new = match report.executed[0].events[0].outcome {
        crate::events::Outcome::ZoneChange(m) => m.new,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        report.executed[0].events[0].instance.event(),
        &zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)
    );
    assert_eq!(run.state.events.markers().copy_of(new), Some(original));
    assert_eq!(run.state.events.log().len(), 1);
    // Redirected to exile by a self-replacement first: no copy.
    w.add(
        ReplacementEffect::new(
            e(2),
            EventMatcher::new(BusEventKind::ZoneChange),
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_tier(Tier::SelfReplacement),
    );
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    assert_eq!(ok(&run).applications, vec![e(2), e(1)]);
    assert_eq!(ok(&run).executed.len(), 1);
}

/// REQ-EVENT-001: child nesting deeper than `MAX_CHILD_DEPTH` is `DepthExceeded`; nothing
/// changes. The looping effect overrides CR 614.5 (an `Instead` child keeps the applied set,
/// so a non-overriding effect would stop after one application, round 1 clause f/i).
#[test]
fn req_event_001_child_depth_limit() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::Tap),
            Action::Instead(vec![GameEvent::Tap { object: x }, life(1, 1)]),
        )
        .with_overrides_614_5(true),
    );
    let before = w.state_hash();
    let run = drive(&w, vec![GameEvent::Tap { object: x }], false, &[]);
    assert_eq!(run.out, Err(BusError::DepthExceeded));
    assert_eq!(run.hash, before);
    assert_eq!(run.state.life(1), 20);
    assert_eq!(MAX_CHILD_DEPTH, 64);
}

// ---------------------------------------------------------------------------------------------
// Chooser and APNAP (CR 616.1)
// ---------------------------------------------------------------------------------------------

fn two_effects(w: &mut World, kind: BusEventKind) {
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(kind),
        Action::Skip,
    ));
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(kind),
        Action::Skip,
    ));
}

/// REQ-EVENT-001: CR 616.1, the chooser is the controller of a permanent, the owner off the
/// battlefield, the affected player, or the active player when nothing is affected.
#[test]
fn req_event_001_cr_616_1_chooser() {
    let mut w = World::new();
    two_effects(&mut w, BusEventKind::Tap);
    let x = w.create_controlled(1, 2, ZoneKind::Battlefield);
    let run = drive(&w, vec![GameEvent::Tap { object: x }], false, &[0]);
    assert_eq!(run.asked[0].chooser, p(2));
    assert_eq!(run.asked[0].reason.source, Some(x));
    assert_eq!(run.asked[0].reason.cr, "CR 616.1");

    let mut w = World::new();
    two_effects(&mut w, BusEventKind::ZoneChange);
    let h = w.create(2, ZoneKind::Hand);
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Graveyard, ZoneChangeCause::Discard)],
        false,
        &[0],
    );
    assert_eq!(run.asked[0].chooser, p(2));

    let mut w = World::new();
    two_effects(&mut w, BusEventKind::LifeChange);
    let run = drive(&w, vec![life(2, 1)], false, &[0]);
    assert_eq!(run.asked[0].chooser, p(2));
    assert_eq!(run.asked[0].reason.source, None);

    let mut w = World::new();
    two_effects(&mut w, BusEventKind::CreateToken);
    let card = w.card();
    let token = GameEvent::CreateToken {
        owner: p(1),
        card,
        controller: Some(p(2)),
    };
    let run = drive(&w, vec![token], false, &[0]);
    assert_eq!(run.asked[0].chooser, p(2));

    let mut w = World::new();
    two_effects(&mut w, BusEventKind::PhaseBegins);
    let phase = GameEvent::PhaseBegins {
        phase: Phase::Combat,
    };
    let run = drive(&w, vec![phase.clone()], false, &[0]);
    assert_eq!(run.asked[0].chooser, p(1));
    w.game.set_active_player(Some(p(2))).unwrap();
    let run = drive(&w, vec![phase], false, &[0]);
    assert_eq!(run.asked[0].chooser, p(2));

    // An unknown affected object is an error, and nothing changes.
    let w = World::new();
    let run = drive(
        &w,
        vec![GameEvent::Tap {
            object: ObjectId::new(999),
        }],
        false,
        &[],
    );
    assert_eq!(
        run.out,
        Err(BusError::Event(EventError::UnknownObject(ObjectId::new(
            999
        ))))
    );
}

/// REQ-EVENT-001: CR 616.1 APNAP: the active player's events are processed first, then the
/// others in turn order; the batch still executes in submission order.
#[test]
fn req_event_001_cr_616_1_apnap_order() {
    let mut w = World::new();
    w.add(add_one(1, BusEventKind::LifeChange));
    w.add(add_one(2, BusEventKind::LifeChange));
    let events = vec![life(2, 1), life(1, 1)];
    let run = drive(&w, events.clone(), false, &[0, 0]);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(run.asked[1].chooser, p(2));
    let logged: Vec<_> = ok(&run).executed[0]
        .events
        .iter()
        .map(|x| x.instance.event().clone())
        .collect();
    assert_eq!(logged, vec![life(2, 3), life(1, 3)]);
    w.game.set_active_player(Some(p(2))).unwrap();
    let run = drive(&w, events, false, &[0, 0]);
    assert_eq!(run.asked[0].chooser, p(2));
    assert_eq!(run.asked[1].chooser, p(1));
}

// ---------------------------------------------------------------------------------------------
// Suspension (REQ-NET-003 protocol use)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: a `CHOOSE_REPLACEMENT_EFFECT` request has the SRS 15.3 shape; every valid
/// answer completes; the same answers give the same hash; an invalid index is rejected; the
/// visible state is the base while pending.
#[test]
fn req_event_001_suspend_resume() {
    let mut w = World::new();
    for id in [3, 1, 2] {
        w.add(add_one(id, BusEventKind::Damage).with_description(format!("shield {id}")));
    }
    let base_hash = w.state_hash();
    let batch = EventBatch::from_events(vec![damage_player(2, 1)]);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let r = session.pending().unwrap().clone();
    assert_eq!(r.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(r.chooser, p(2));
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseReplacementEffect {
            event: "DAMAGE".into(),
            effects: vec!["shield 1".into(), "shield 2".into(), "shield 3".into()],
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Number(0),
            ChoiceOption::Number(1),
            ChoiceOption::Number(2)
        ])
    );
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SelectReplacement { index: 0 }
    );
    assert!(!r.hidden);
    assert_eq!(session.state_hash(), base_hash);
    assert!(
        session
            .submit(r.request_id, ChoiceResponse::SelectReplacement { index: 3 })
            .is_err()
    );
    assert_eq!(session.state_hash(), base_hash);
    session
        .submit(r.request_id, ChoiceResponse::SelectReplacement { index: 2 })
        .unwrap();
    let r2 = session.pending().unwrap().clone();
    assert_eq!(r2.request_id.get(), 1);
    assert_eq!(
        effects_of(&r2),
        vec!["shield 1".to_owned(), "shield 2".into()]
    );
    session.submit_default(r2.request_id).unwrap();
    match session.status() {
        Status::Done { output, state } => {
            assert_eq!(
                output.as_ref().unwrap().applications,
                vec![e(3), e(1), e(2)]
            );
            assert_eq!(state.life(2), 16);
        }
        Status::Pending(_) => panic!("still pending"),
    }
    // Every answer sequence completes; equal answers give equal hashes and reports.
    for first in 0..3 {
        for second in 0..2 {
            let a = drive(&w, vec![damage_player(2, 1)], false, &[first, second]);
            let b = drive(&w, vec![damage_player(2, 1)], false, &[first, second]);
            assert_eq!(a.hash, b.hash);
            assert_eq!(a.out, b.out);
            assert_eq!(ok(&a).applications.len(), 3);
        }
    }
    // Replay through the session equals step-by-step submission.
    let batch = EventBatch::from_events(vec![damage_player(2, 1)]);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let replayed = ChoiceSession::replay(
        w.clone(),
        procedure,
        &[
            ChoiceResponse::SelectReplacement { index: 2 },
            ChoiceResponse::SelectReplacement { index: 0 },
        ],
    )
    .unwrap();
    assert_eq!(replayed.state_hash(), session.state_hash());
}

/// REQ-EVENT-001: interrupt conversions between the bus and the choice protocol.
#[test]
fn req_event_001_interrupt_conversions() {
    let w = World::new();
    let mut w2 = w.clone();
    two_effects(&mut w2, BusEventKind::LifeChange);
    let batch = EventBatch::from_events(vec![life(1, 1)]);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let session = ChoiceSession::start(w2, procedure).unwrap();
    let request = session.pending().unwrap().clone();
    let pending = BusInterrupt::from(Interrupt::Pending(Box::new(request.clone())));
    assert_eq!(pending, BusInterrupt::Pending(Box::new(request.clone())));
    assert!(!pending.to_string().is_empty());
    assert_eq!(
        Interrupt::from(pending),
        Interrupt::Pending(Box::new(request))
    );
    let err = BusInterrupt::from(Interrupt::Error(ChoiceError::NotPending));
    assert_eq!(
        err,
        BusInterrupt::Error(BusError::Choice(ChoiceError::NotPending))
    );
    assert!(!err.to_string().is_empty());
    for err in [BusError::RepeatLimit, BusError::WorkLimit] {
        match Interrupt::from(BusInterrupt::Error(err.clone())) {
            Interrupt::Error(ChoiceError::Procedure(m)) => assert_eq!(m, err.to_string()),
            other => panic!("{other:?}"),
        }
    }
    assert!(BusError::WorkLimit.to_string().contains("4096"));
    assert_eq!(
        BusInterrupt::from(EventError::Overflow),
        BusInterrupt::Error(BusError::Event(EventError::Overflow))
    );
    assert_eq!(
        BusError::from(EventError::Overflow),
        BusError::Event(EventError::Overflow)
    );
}

/// REQ-EVENT-001: a procedure that uses the bus through `Interrupt` (the session's own error
/// type) works with `?`.
#[test]
fn req_event_001_process_event_via_question_mark() {
    let mut w = World::new();
    two_effects(&mut w, BusEventKind::LifeChange);
    let procedure = |s: &mut World, ch: &mut ChoiceContext<'_>| -> Result<usize, Interrupt> {
        let provider = TestProvider;
        let mut ctx = EventContext {
            game: &mut s.game,
            zones: &mut s.zones,
            ids: &mut s.ids,
            permanents: &mut s.permanents,
            memories: &mut s.memories,
            lki: &mut s.lki,
            events: &mut s.events,
            provider: &provider,
        };
        let report = process_event(&mut ctx, &mut s.registry, life(1, 1), ch)?;
        Ok(report.applications.len())
    };
    let mut session = ChoiceSession::start(w, procedure).unwrap();
    let id = session.pending().unwrap().request_id;
    session.submit_default(id).unwrap();
    assert!(matches!(session.status(), Status::Done { output: 1, .. }));
}

// ---------------------------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: an executor error in the final batch restores the registry (shield use
/// included) and the world.
#[test]
fn req_event_001_rollback_final_batch() {
    let mut w = World::new();
    w.add(add_one(1, BusEventKind::LifeChange).with_shield(Shield::Uses(1)));
    let before = w.state_hash();
    let run = drive(&w, vec![life(1, 2), life(9, 1)], false, &[]);
    assert_eq!(
        run.out,
        Err(BusError::Event(EventError::UnknownPlayer(p(9))))
    );
    assert_eq!(run.hash, before);
    assert_eq!(run.state.registry, w.registry);
}

/// REQ-EVENT-001: an error in a child batch also undoes the already executed parent batch.
#[test]
fn req_event_001_rollback_child_batch() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::Tap),
            Action::Instead(vec![life(9, 1)]),
        )
        .with_shield(Shield::Uses(1)),
    );
    let before = w.state_hash();
    let run = drive(
        &w,
        vec![life(1, 3), GameEvent::Tap { object: x }],
        false,
        &[],
    );
    assert_eq!(
        run.out,
        Err(BusError::Event(EventError::UnknownPlayer(p(9))))
    );
    assert_eq!(run.hash, before);
    assert_eq!(run.state.life(1), 20);
    assert!(run.state.events.log().is_empty());
    assert_eq!(run.state.registry, w.registry);
}

// ---------------------------------------------------------------------------------------------
// Matchers
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001: each matcher field is an AND condition; a field that does not apply to the
/// event's kind does not match.
#[test]
fn req_event_001_matcher_fields() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "k", 1);
    let base = |kind| EventMatcher::new(kind);
    let cases: Vec<(EventMatcher, GameEvent, bool)> = vec![
        (EventMatcher::default(), life(1, 1), false),
        (base(BusEventKind::Tap), life(1, 1), false),
        (
            EventMatcher {
                affected_player: Some(p(1)),
                ..base(BusEventKind::LifeChange)
            },
            life(1, 1),
            true,
        ),
        (
            EventMatcher {
                affected_player: Some(p(2)),
                ..base(BusEventKind::LifeChange)
            },
            life(1, 1),
            false,
        ),
        (
            EventMatcher {
                affected_object: Some(x),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            true,
        ),
        (
            EventMatcher {
                affected_object: Some(ObjectId::new(77)),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                from: Some(ZoneKind::Battlefield),
                ..base(BusEventKind::ZoneChange)
            },
            zc(x, ZoneKind::Exile, ZoneChangeCause::Exile),
            true,
        ),
        (
            EventMatcher {
                from: Some(ZoneKind::Hand),
                ..base(BusEventKind::ZoneChange)
            },
            zc(x, ZoneKind::Exile, ZoneChangeCause::Exile),
            false,
        ),
        (
            EventMatcher {
                from: Some(ZoneKind::Hand),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                to: Some(ZoneKind::Exile),
                ..base(BusEventKind::ZoneChange)
            },
            zc(x, ZoneKind::Exile, ZoneChangeCause::Exile),
            true,
        ),
        (
            EventMatcher {
                to: Some(ZoneKind::Battlefield),
                ..base(BusEventKind::CreateToken)
            },
            GameEvent::CreateToken {
                owner: p(1),
                card: CardId::new(5),
                controller: None,
            },
            true,
        ),
        (
            EventMatcher {
                to: Some(ZoneKind::Exile),
                ..base(BusEventKind::CreateToken)
            },
            GameEvent::CreateToken {
                owner: p(1),
                card: CardId::new(5),
                controller: None,
            },
            false,
        ),
        (
            EventMatcher {
                to: Some(ZoneKind::Exile),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                cause: Some(ZoneChangeCause::Exile),
                ..base(BusEventKind::ZoneChange)
            },
            zc(x, ZoneKind::Exile, ZoneChangeCause::Exile),
            true,
        ),
        (
            EventMatcher {
                cause: Some(ZoneChangeCause::Exile),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                combat: Some(false),
                ..base(BusEventKind::Damage)
            },
            damage_player(1, 1),
            true,
        ),
        (
            EventMatcher {
                combat: Some(true),
                ..base(BusEventKind::Damage)
            },
            damage_player(1, 1),
            false,
        ),
        (
            EventMatcher {
                combat: Some(true),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                counter_kind: Some("k".into()),
                ..base(BusEventKind::AddCounters)
            },
            GameEvent::AddCounters {
                target: CounterTarget::Player(p(1)),
                kind: "k".into(),
                amount: 1,
            },
            true,
        ),
        (
            EventMatcher {
                counter_kind: Some("j".into()),
                ..base(BusEventKind::AddCounters)
            },
            GameEvent::AddCounters {
                target: CounterTarget::Player(p(1)),
                kind: "k".into(),
                amount: 1,
            },
            false,
        ),
        (
            EventMatcher {
                counter_kind: Some("k".into()),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                amount_below: Some(3),
                ..base(BusEventKind::LifeChange)
            },
            life(1, -2),
            true,
        ),
        (
            EventMatcher {
                amount_below: Some(2),
                ..base(BusEventKind::LifeChange)
            },
            life(1, -2),
            false,
        ),
        (
            EventMatcher {
                amount_below: Some(2),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
        (
            EventMatcher {
                requires_counter: Some((x, "k".into())),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            true,
        ),
        (
            EventMatcher {
                requires_counter: Some((x, "j".into())),
                ..base(BusEventKind::Tap)
            },
            GameEvent::Tap { object: x },
            false,
        ),
    ];
    for (i, (matcher, event, expected)) in cases.into_iter().enumerate() {
        let mut v = w.clone();
        let kind = BusEventKind::of(&event);
        // Skip is valid for every kind; an effect with no kind cannot be added, so test the
        // default matcher on an otherwise valid effect by expecting `add` to reject it.
        let effect = ReplacementEffect::new(e(1), matcher, Action::Skip);
        if effect.matcher.kind.is_none() {
            assert!(v.registry.add(effect).is_err());
            continue;
        }
        v.add(effect);
        let run = drive(&v, vec![event], false, &[]);
        let applied = !ok(&run).applications.is_empty();
        assert_eq!(applied, expected, "case {i} ({kind:?})");
    }
}

// ---------------------------------------------------------------------------------------------
// Property-style tests (deterministic generator, no dependencies)
// ---------------------------------------------------------------------------------------------

/// A small deterministic generator (SplitMix64).
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn gen_effect(g: &mut Gen, id: u64) -> ReplacementEffect {
    let damage = g.below(2) == 0;
    let kind = if damage {
        BusEventKind::Damage
    } else {
        BusEventKind::LifeChange
    };
    let mut matcher = EventMatcher::new(kind);
    if g.below(3) == 0 {
        matcher.affected_player = Some(p(1 + g.below(2)));
    }
    if g.below(3) == 0 {
        matcher.amount_below = Some(1 + i64::try_from(g.below(8)).unwrap());
    }
    let action = match g.below(if damage { 6 } else { 5 }) {
        0 => Action::Skip,
        1 => Action::Modify(Modification::Amount(AmountOp::Add(
            i64::try_from(g.below(5)).unwrap() - 2,
        ))),
        2 => Action::Modify(Modification::Amount(AmountOp::Mul(
            i64::try_from(g.below(3)).unwrap(),
        ))),
        3 => Action::Modify(Modification::Amount(AmountOp::Set(
            i64::try_from(g.below(4)).unwrap(),
        ))),
        4 => Action::Instead(vec![life(1 + g.below(2), 1)]),
        _ => Action::Prevent { then: Vec::new() },
    };
    let is_prevent = matches!(action, Action::Prevent { .. });
    let mut effect = ReplacementEffect::new(e(id), matcher, action)
        .with_tier(TIERS[usize::try_from(g.below(5)).unwrap()])
        .with_contained(g.below(4) == 0)
        .with_overrides_614_5(g.below(6) == 0);
    match g.below(4) {
        0 => effect = effect.with_shield(Shield::Uses(1 + u32::try_from(g.below(3)).unwrap())),
        1 if is_prevent => {
            effect = effect.with_shield(Shield::Pool(1 + i64::try_from(g.below(4)).unwrap()));
        }
        _ => {}
    }
    effect
}

fn gen_case(seed: u64) -> (Vec<ReplacementEffect>, Vec<GameEvent>, Vec<u32>) {
    let mut g = Gen(seed);
    let n = 1 + g.below(6);
    let effects: Vec<ReplacementEffect> = (0..n).map(|i| gen_effect(&mut g, 10 + i * 3)).collect();
    let events = (0..1 + g.below(3))
        .map(|_| {
            let player = 1 + g.below(2);
            let amount = 1 + i64::try_from(g.below(5)).unwrap();
            if g.below(2) == 0 {
                damage_player(player, amount)
            } else {
                life(player, amount)
            }
        })
        .collect();
    let answers = (0..64)
        .map(|_| u32::try_from(g.below(6)).unwrap())
        .collect();
    (effects, events, answers)
}

/// Runs a generated case with answers taken modulo the offered count.
fn drive_mod(w: &World, events: Vec<GameEvent>, answers: &[u32]) -> Run {
    let batch = EventBatch::from_events(events);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let mut asked = Vec::new();
    let mut k = 0;
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                return Run {
                    asked,
                    state: state.clone(),
                    out: output.clone(),
                    hash: session.state_hash(),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        let n = u32::try_from(effects_of(&pending).len()).unwrap();
        let index = answers[k % answers.len()] % n;
        k += 1;
        session
            .submit(
                pending.request_id,
                ChoiceResponse::SelectReplacement { index },
            )
            .unwrap();
        asked.push(pending);
    }
}

/// REQ-EVENT-001, REQ-CORE-001, REQ-CORE-006 (property): over generated effect sets, (1) no
/// non-overriding effect applies twice to one event (CR 614.5); (2) shields never go below
/// one use / one point and exhausted effects are removed (CR 615.7); (3) the result does not
/// depend on registry insertion order; (4) re-running gives identical reports and bytes.
#[test]
fn req_event_001_property_invariants() {
    for seed in 0..300_u64 {
        let (effects, events, answers) = gen_case(seed);
        let mut w = World::new();
        for effect in &effects {
            w.add(effect.clone());
        }
        let mut reversed = World::new();
        for effect in effects.iter().rev() {
            reversed.add(effect.clone());
        }
        let a = drive_mod(&w, events.clone(), &answers);
        let b = drive_mod(&reversed, events.clone(), &answers);
        let c = drive_mod(&w, events.clone(), &answers);
        // (3), (4)
        assert_eq!(a.out, b.out, "seed {seed}");
        assert_eq!(a.hash, b.hash, "seed {seed}");
        assert_eq!(a.out, c.out, "seed {seed}");
        assert_eq!(a.hash, c.hash, "seed {seed}");
        assert_eq!(
            encode_to_vec(&a.state.registry),
            encode_to_vec(&c.state.registry)
        );
        assert_eq!(a.asked, c.asked, "seed {seed}");
        // (2)
        for id in a.state.registry.ids() {
            match a.state.registry.get(id).unwrap().shield {
                Some(Shield::Uses(n)) => assert!(n >= 1, "seed {seed}"),
                Some(Shield::Pool(n)) => assert!(n >= 1, "seed {seed}"),
                None => {}
            }
        }
        for effect in &effects {
            if a.out.is_ok() && a.state.registry.get(effect.id).is_none() {
                assert!(
                    effect.shield.is_some(),
                    "seed {seed}: unshielded effect removed"
                );
            }
        }
        // (1): per single event, a non-overriding effect applies at most once.
        for event in &events {
            let single = drive_mod(&w, vec![event.clone()], &answers);
            if let Ok(report) = &single.out {
                let mut seen = BTreeSet::new();
                // Child events are separate events; count only the parent's applications by
                // excluding runs with `Instead` children.
                if effects
                    .iter()
                    .any(|x| matches!(x.action, Action::Instead(_)))
                {
                    continue;
                }
                for id in &report.applications {
                    let overrides = w.registry.get(*id).is_some_and(|x| x.overrides_614_5);
                    assert!(seen.insert(*id) || overrides, "seed {seed}: {id:?} twice");
                }
                for batch in &report.executed {
                    for ev in &batch.events {
                        for id in ev.instance.applied_effects().iter() {
                            assert!(report.applications.contains(&id));
                        }
                    }
                }
            } else {
                assert!(
                    matches!(
                        single.out,
                        Err(BusError::RepeatLimit | BusError::DepthExceeded)
                    ),
                    "seed {seed}: {:?}",
                    single.out
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Review round 1 regressions (PR #178)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, F1: CR 614.5, CR 614.6: an `Instead` child is the modified event and keeps
/// the replaced event's applied set. A replaces a draw with a life gain, B replaces that gain
/// with a draw; neither overrides CR 614.5, so the second draw happens.
#[test]
fn req_event_001_r1_f1_instead_children_keep_applied() {
    let mut w = World::new();
    w.create(1, ZoneKind::Library);
    let draw = GameEvent::Draw { player: p(1) };
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::Draw),
        Action::Instead(vec![life(1, 1)]),
    ));
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Instead(vec![draw.clone()]),
    ));
    let run = drive(&w, vec![draw.clone()], false, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    let ev = &report.executed[0].events[0];
    assert_eq!(ev.instance.event(), &draw);
    assert_eq!(
        ev.instance.applied_effects().iter().collect::<Vec<_>>(),
        vec![e(1), e(2)]
    );
    assert_eq!(run.state.zones.contents(ZoneId::Hand(p(1))).len(), 1);
    assert_eq!(run.state.life(1), 20);
}

/// REQ-EVENT-001, F1: CR 615.5: a prevention follow-up is a new event with no applied effects,
/// so the same prevention effect may apply to it.
#[test]
fn req_event_001_r1_f1_prevent_follow_up_starts_fresh() {
    let mut w = World::new();
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::Damage),
            Action::Prevent {
                then: vec![damage_player(2, 1)],
            },
        )
        .with_shield(Shield::Uses(2)),
    );
    let run = drive(&w, vec![damage_player(2, 3)], false, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(report.prevented, 4);
    assert!(run.state.registry.get(e(1)).is_none());
    assert_eq!(run.state.life(2), 19);
}

/// REQ-EVENT-001, F2: CR 704.3, CR 704.5g, CR 704.5h, CR 704.7, CR 701.8a: two state-based
/// moves of the same permanent to its graveyard are one destruction: no `UnknownObject`, one
/// executed move, with or without a replacement; with a one-use exile redirect, one
/// application and one use. With sba false the duplicate is not coalesced and fails.
#[test]
fn req_event_001_r1_f2_same_object_sba_moves_execute_once() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    let mv = zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction);
    let run = drive(&w, vec![mv.clone(), mv.clone()], true, &[]);
    let report = ok(&run);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    // With an exile redirect of one use.
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                to: Some(ZoneKind::Graveyard),
                ..EventMatcher::new(BusEventKind::ZoneChange)
            },
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_shield(Shield::Uses(1)),
    );
    let run = drive(&w, vec![mv.clone(), mv.clone()], true, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert!(run.state.registry.get(e(1)).is_none());
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert_eq!(run.state.zones.contents(ZoneId::Exile).len(), 1);
    // sba false: no coalescing; the second move fails and everything rolls back.
    let before = w.state_hash();
    let run = drive(&w, vec![mv.clone(), mv], false, &[]);
    assert!(run.out.is_err());
    assert_eq!(run.hash, before);
}

/// REQ-EVENT-001, F3: CR 616.1, CR 112.2, CR 109.4: the chooser for a spell on the stack is
/// its controller (P2), not its owner (P1); a graveyard card's chooser is its owner.
#[test]
fn req_event_001_r1_f3_stack_object_controller_chooses() {
    let mut w = World::new();
    two_effects(&mut w, BusEventKind::ZoneChange);
    let mut req = MoveRequest::new(ZoneKind::Stack);
    req.controller = Some(p(2));
    let card = w.card();
    let spell = w.zones.create_object(&mut w.ids, card, p(1), req).unwrap();
    let run = drive(
        &w,
        vec![zc(
            spell,
            ZoneKind::Graveyard,
            ZoneChangeCause::CounterSpell,
        )],
        false,
        &[0],
    );
    assert_eq!(run.asked[0].chooser, p(2));
    let g = w.create(1, ZoneKind::Graveyard);
    let run = drive(
        &w,
        vec![zc(g, ZoneKind::Exile, ZoneChangeCause::Exile)],
        false,
        &[0],
    );
    assert_eq!(run.asked[0].chooser, p(1));
}

/// REQ-EVENT-001, F4: CR 614.7, CR 614.7a, CR 120.8: 0 damage is no event; it does not consume
/// a one-use prevention shield, which then prevents the 3 damage.
#[test]
fn req_event_001_r1_f4_zero_damage_offers_no_effect() {
    let mut w = World::new();
    w.add(prevent_all(1).with_shield(Shield::Uses(1)));
    let run = drive(
        &w,
        vec![damage_player(2, 0), damage_player(2, 3)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.prevented, 3);
    assert_eq!(run.state.life(2), 20);
    assert!(run.state.registry.get(e(1)).is_none());
    // Alone, 0 damage makes no request even with two effects.
    let mut w = World::new();
    w.add(prevent_all(1));
    w.add(prevent_all(2));
    let run = drive(&w, vec![damage_player(2, 0)], false, &[]);
    assert!(run.asked.is_empty());
    assert!(ok(&run).applications.is_empty());
}

/// REQ-EVENT-001, F5: CR 704.7, CR 616.1: three different permanents dying are three results;
/// a one-use exile redirect exiles only one of them.
#[test]
fn req_event_001_r1_f5_different_objects_never_merge() {
    let mut w = World::new();
    let xs: Vec<ObjectId> = (0..3).map(|_| w.create(1, ZoneKind::Battlefield)).collect();
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                to: Some(ZoneKind::Graveyard),
                ..EventMatcher::new(BusEventKind::ZoneChange)
            },
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_shield(Shield::Uses(1)),
    );
    let events = xs
        .iter()
        .map(|&x| zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction))
        .collect();
    let run = drive(&w, events, true, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(run.state.zones.contents(ZoneId::Exile).len(), 1);
    assert_eq!(run.state.zones.contents(ZoneId::Graveyard(p(1))).len(), 2);
    assert_eq!(
        run.state.zone_of_card(w.card_of(xs[0])),
        Some(ZoneId::Exile)
    );
}

/// REQ-EVENT-001, F6: CR 614.1c, CR 603.6d, CR 616.1c: an enters-as-copy is part of the entry;
/// a `Skip` effect on `BecomeCopy` does not undo it (zone change and token). A `BecomeCopy`
/// submitted on its own is still replaced. A failing copy rolls everything back.
#[test]
fn req_event_001_r1_f6_enters_as_copy_is_not_replaced() {
    let mut w = World::new();
    let original = w.create(2, ZoneKind::Battlefield);
    let other = w.create(1, ZoneKind::Battlefield);
    let h = w.create(1, ZoneKind::Hand);
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::ZoneChange),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    ));
    w.add(ReplacementEffect::new(
        e(3),
        EventMatcher::new(BusEventKind::CreateToken),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    ));
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(BusEventKind::BecomeCopy),
        Action::Skip,
    ));
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    // CR 707.5 (round 2): one executed batch holding only the entering event.
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    let new = match report.executed[0].events[0].outcome {
        crate::events::Outcome::ZoneChange(m) => m.new,
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        report.executed[0].events[0].instance.event(),
        GameEvent::ZoneChange { .. }
    ));
    assert_eq!(run.state.events.markers().copy_of(new), Some(original));
    // A token.
    let token = token_event(&mut w);
    let run = drive(&w, vec![token], false, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(3)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    let object = match report.executed[0].events[0].outcome {
        crate::events::Outcome::TokenCreated { object } => object,
        other => panic!("{other:?}"),
    };
    assert_eq!(run.state.events.markers().copy_of(object), Some(original));
    // A post-entry BecomeCopy on its own is replaced.
    let run = drive(
        &w,
        vec![GameEvent::BecomeCopy {
            object: other,
            copied: original,
        }],
        false,
        &[],
    );
    assert_eq!(ok(&run).applications, vec![e(2)]);
    assert_eq!(run.state.events.markers().copy_of(other), None);
    // The copy step fails: everything rolls back.
    let mut w = World::new();
    let h = w.create(1, ZoneKind::Hand);
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::ZoneChange),
            Action::Modify(Modification::EntersAsCopyOf(ObjectId::new(999))),
        )
        .with_shield(Shield::Uses(1)),
    );
    let before = w.state_hash();
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    assert_eq!(
        run.out,
        Err(BusError::Event(EventError::UnknownObject(ObjectId::new(
            999
        ))))
    );
    assert_eq!(run.hash, before);
}

/// REQ-EVENT-001, F7: an overriding (CR 614.5 exception) `Instead` that expands a draw into
/// 100 draws hits the shared work budget (`WorkLimit`) and changes nothing; exactly
/// `MAX_BUS_EVENTS` events are allowed, one more is `WorkLimit`, children included. Round 6
/// (lineage paths, CR 101.4, CR 616.1): descendants are scheduled depth-first, so an overriding
/// chain that only doubles reaches `MAX_CHILD_DEPTH` (about 130 events) before the budget and
/// is `DepthExceeded`, equally rolled back.
#[test]
fn req_event_001_r1_f7_work_budget() {
    assert_eq!(MAX_BUS_EVENTS, 4096);
    let draw = GameEvent::Draw { player: p(1) };
    for (fan_out, error) in [(100, BusError::WorkLimit), (2, BusError::DepthExceeded)] {
        let mut w = World::new();
        w.add(
            ReplacementEffect::new(
                e(1),
                EventMatcher::new(BusEventKind::Draw),
                Action::Instead(vec![draw.clone(); fan_out]),
            )
            .with_overrides_614_5(true),
        );
        let before = w.state_hash();
        let run = drive(&w, vec![draw.clone()], false, &[]);
        assert_eq!(run.out, Err(error));
        assert_eq!(run.hash, before);
        assert_eq!(run.state.registry, w.registry);
    }
    // The submitted batch alone.
    let w = World::new();
    let run = drive(&w, vec![life(1, 1); MAX_BUS_EVENTS], false, &[]);
    ok(&run);
    assert_eq!(run.state.life(1), 20 + 4096);
    let before = w.state_hash();
    let run = drive(&w, vec![life(1, 1); MAX_BUS_EVENTS + 1], false, &[]);
    assert_eq!(run.out, Err(BusError::WorkLimit));
    assert_eq!(run.hash, before);
    // A child counts.
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::Tap),
        Action::Instead(vec![life(2, 1)]),
    ));
    let mut events = vec![life(1, 1); MAX_BUS_EVENTS - 2];
    events.push(GameEvent::Tap { object: x });
    let run = drive(&w, events.clone(), false, &[]);
    ok(&run);
    assert_eq!(run.state.life(2), 21);
    events.insert(0, life(1, 1));
    let before = w.state_hash();
    let run = drive(&w, events, false, &[]);
    assert_eq!(run.out, Err(BusError::WorkLimit));
    assert_eq!(run.hash, before);
}

/// The F8 world: player 2 has a prevention pool of `pool`; sources `a` < `b`.
fn pool_world(pool: i64) -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let b = w.create(1, ZoneKind::Battlefield);
    let (a, b) = (a.min(b), a.max(b));
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                affected_player: Some(p(2)),
                ..EventMatcher::new(BusEventKind::Damage)
            },
            Action::Prevent { then: Vec::new() },
        )
        .with_shield(Shield::Pool(pool)),
    );
    (w, a, b)
}

/// The sources of the damage events executed in `report`.
fn dealt_sources(report: &BusReport) -> Vec<Option<ObjectId>> {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::Damage { source, .. } => Some(*source),
            _ => None,
        })
        .collect()
}

/// REQ-EVENT-001, F8: CR 615.7: two sources each dealing 3 to P2 at once against a pool of 3:
/// P2 divides the pool (one `DIVIDE_VALUE` request); every division is honoured; equal
/// answers give equal results; one source or a pool that covers everything asks nothing.
#[test]
fn req_event_001_r1_f8_pool_allocated_by_affected_player() {
    let (w, a, b) = pool_world(3);
    let events = vec![
        damage_from_to_player(a, 2, 3),
        damage_from_to_player(b, 2, 3),
    ];
    // A takes it all: B's damage is dealt.
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 3), (b, 0)])]);
    assert_eq!(run.asked.len(), 1);
    let r = &run.asked[0];
    assert_eq!(r.choice_type, ChoiceType::DivideValue);
    assert_eq!(r.chooser, p(2));
    assert_eq!(r.reason.cr, "CR 615.7");
    assert_eq!(r.reason.source, None);
    assert_eq!((r.min, r.max), (2, 2));
    assert_eq!(
        r.payload,
        ChoicePayload::DivideValue {
            total: 3,
            recipients: vec![(ChoiceTarget::Object(a), 0), (ChoiceTarget::Object(b), 0)],
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Object(a), ChoiceOption::Object(b)])
    );
    assert_eq!(r.default_on_timeout, division(&[(a, 3), (b, 0)]));
    let report = ok(&run);
    assert_eq!(report.prevented, 3);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(dealt_sources(report), vec![Some(b)]);
    assert_eq!(run.state.life(2), 17);
    assert!(run.state.registry.get(e(1)).is_none());
    // Split 1 / 2: 3 prevented, 3 dealt.
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 1), (b, 2)])]);
    let report = ok(&run);
    assert_eq!(report.prevented, 3);
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(run.state.life(2), 17);
    // B takes it all: mirrors the first.
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 0), (b, 3)])]);
    let report = ok(&run);
    assert_eq!(report.prevented, 3);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(dealt_sources(report), vec![Some(a)]);
    assert_eq!(run.state.life(2), 17);
    // Determinism.
    for shares in [[(a, 3), (b, 0)], [(a, 1), (b, 2)], [(a, 0), (b, 3)]] {
        let x = drive_with(&w, events.clone(), false, &[division(&shares)]);
        let y = drive_with(&w, events.clone(), false, &[division(&shares)]);
        assert_eq!(x.out, y.out);
        assert_eq!(x.hash, y.hash);
    }
    // One source: automatic, sequential.
    let run = drive(
        &w,
        vec![
            damage_from_to_player(a, 2, 3),
            damage_from_to_player(a, 2, 3),
        ],
        false,
        &[],
    );
    assert!(run.asked.is_empty());
    assert_eq!(ok(&run).prevented, 3);
    assert_eq!(run.state.life(2), 17);
    // A pool that covers everything: automatic.
    let (w6, a6, b6) = pool_world(6);
    let run = drive(
        &w6,
        vec![
            damage_from_to_player(a6, 2, 3),
            damage_from_to_player(b6, 2, 3),
        ],
        false,
        &[],
    );
    assert!(run.asked.is_empty());
    assert_eq!(ok(&run).prevented, 6);
    assert_eq!(run.state.life(2), 20);
    // A division that does not sum to the pool is rejected by the session.
    let batch = EventBatch::from_events(events);
    let procedure = move |s: &mut World, ch: &mut ChoiceContext<'_>| run_bus(s, &batch, false, ch);
    let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
    let id = session.pending().unwrap().request_id;
    assert!(session.submit(id, division(&[(a, 2), (b, 2)])).is_err());
    assert!(session.submit(id, division(&[(a, 3)])).is_err());
    session.submit(id, division(&[(a, 2), (b, 1)])).unwrap();
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(output.as_ref().unwrap().prevented, 3);
            assert_eq!(state.life(2), 17);
        }
        Status::Pending(_) => panic!("still pending"),
    }
}

/// REQ-EVENT-001, F8 as amended by round 2 G4: CR 615.7: an allotment larger than its
/// source's damage is rejected (the call fails, nothing changes); damage with no source is
/// outside the division and draws on what is left of the pool in submission order.
#[test]
fn req_event_001_r1_f8_unused_allotment_stays() {
    let (w, a, b) = pool_world(4);
    let events = vec![
        damage_from_to_player(a, 2, 1),
        damage_from_to_player(b, 2, 4),
        damage_player(2, 2),
    ];
    // Eligible: 1 + 4 = 5 > 4, two sources: asked. A's 3 exceeds its damage 1: rejected.
    let before = w.state_hash();
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 3), (b, 1)])]);
    assert_eq!(run.asked.len(), 1);
    assert!(matches!(
        run.out,
        Err(BusError::Choice(ChoiceError::Procedure(_)))
    ));
    assert_eq!(run.hash, before);
    // A: 1 prevented; B: 3 prevented (1 dealt); the pool is drained, so the sourceless 2 is
    // dealt.
    let run = drive_with(&w, events, false, &[division(&[(a, 1), (b, 3)])]);
    assert_eq!(run.asked.len(), 1);
    let report = ok(&run);
    assert_eq!(report.prevented, 4);
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(run.state.life(2), 17);
    assert!(run.state.registry.get(e(1)).is_none());
}

/// REQ-EVENT-001, F9: CR 109.4: an entry with a controller redirected to exile loses the
/// controller (no `ControllerNotApplicable`); a redirect to the battlefield keeps it.
#[test]
fn req_event_001_r1_f9_redirect_clears_controller() {
    let mut w = World::new();
    let h = w.create(1, ZoneKind::Hand);
    let card = w.card_of(h);
    let mut req = MoveRequest::new(ZoneKind::Battlefield);
    req.controller = Some(p(2));
    let event = GameEvent::ZoneChange {
        object: h,
        request: req,
        cause: ZoneChangeCause::Other,
    };
    let mut wx = w.clone();
    wx.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::ZoneChange),
        Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
    ));
    let run = drive(&wx, vec![event], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    for zone in [ZoneKind::Graveyard, ZoneKind::Hand, ZoneKind::Library] {
        let mut wz = w.clone();
        wz.add(ReplacementEffect::new(
            e(1),
            EventMatcher::new(BusEventKind::ZoneChange),
            Action::Modify(Modification::SetDestination(zone)),
        ));
        let mut req = MoveRequest::new(ZoneKind::Battlefield);
        req.controller = Some(p(2));
        let event = GameEvent::ZoneChange {
            object: h,
            request: req,
            cause: ZoneChangeCause::Other,
        };
        let run = drive(&wz, vec![event], false, &[]);
        assert!(run.out.is_ok(), "{zone:?}: {:?}", run.out);
    }
    // Redirected to the battlefield from a stack entry: the controller stays.
    let mut wb = w.clone();
    wb.add(ReplacementEffect::new(
        e(1),
        EventMatcher::new(BusEventKind::ZoneChange),
        Action::Modify(Modification::SetDestination(ZoneKind::Battlefield)),
    ));
    let mut req = MoveRequest::new(ZoneKind::Stack);
    req.controller = Some(p(2));
    let event = GameEvent::ZoneChange {
        object: h,
        request: req,
        cause: ZoneChangeCause::Other,
    };
    let run = drive(&wb, vec![event], false, &[]);
    let report = ok(&run);
    let new = match report.executed[0].events[0].outcome {
        crate::events::Outcome::ZoneChange(m) => m.new,
        other => panic!("{other:?}"),
    };
    assert_eq!(run.state.permanents.get(new).unwrap().controller(), p(2));
}

/// REQ-EVENT-001, F11: CR 122.1d, CR 701.26b: untapping an untapped permanent is no event, so
/// its stun counter stays and no effect applies; a tapped one stays tapped and loses one.
#[test]
fn req_event_001_r1_f11_stun_only_on_real_untap() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "stun", 1);
    w.add(ReplacementEffect::stun_counter(e(1), x));
    let run = drive(&w, vec![GameEvent::Untap { object: x }], false, &[]);
    assert!(ok(&run).applications.is_empty());
    assert_eq!(run.state.counter(x, "stun"), 1);
    assert!(!run.state.permanents.get(x).unwrap().status().tapped);
    w.setup(GameEvent::Tap { object: x });
    let run = drive(&w, vec![GameEvent::Untap { object: x }], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.counter(x, "stun"), 0);
    assert!(run.state.permanents.get(x).unwrap().status().tapped);
}

// ---------------------------------------------------------------------------------------------
// Review round 2 (G1-G4, F2, F6)
// ---------------------------------------------------------------------------------------------

/// A world with `original` (P2) on the battlefield, `h` in P1's hand and an effect e1 making
/// anything that enters the battlefield by a zone change enter as a copy of `original`.
fn copy_world() -> (World, ObjectId, ObjectId) {
    let mut w = World::new();
    let original = w.create(2, ZoneKind::Battlefield);
    let h = w.create(1, ZoneKind::Hand);
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher {
            to: Some(ZoneKind::Battlefield),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        Action::Modify(Modification::EntersAsCopyOf(original)),
    ));
    (w, original, h)
}

/// The object the executed zone change of `object`'s card put somewhere, from `report`.
fn new_id_of(report: &BusReport, old: ObjectId) -> ObjectId {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .find_map(|ev| match ev.outcome {
            crate::events::Outcome::ZoneChange(m) if m.old == old => Some(m.new),
            _ => None,
        })
        .expect("no zone change of that object")
}

/// REQ-EVENT-001, REQ-CORE-003, G1: CR 707.5, CR 608.2h: a copy whose source leaves its zone in
/// the same bus call succeeds, in both submission orders: the copy link names the source (its
/// LKI is captured before the batch), the source is in the graveyard as a new object, and one
/// executed batch holds both events (no `BecomeCopy`).
#[test]
fn req_event_001_r2_g1_copy_source_leaves_in_same_call() {
    let (w, original, h) = copy_world();
    let card = w.card_of(original);
    let enter = zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other);
    let leave = zc(original, ZoneKind::Graveyard, ZoneChangeCause::Destroy);
    for events in [
        vec![enter.clone(), leave.clone()],
        vec![leave.clone(), enter.clone()],
    ] {
        let run = drive(&w, events, false, &[]);
        let report = ok(&run);
        assert_eq!(report.applications, vec![e(1)]);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(report.executed[0].events.len(), 2);
        let new = new_id_of(report, h);
        assert_eq!(run.state.events.markers().copy_of(new), Some(original));
        assert!(run.state.zones.object(original).is_none());
        assert!(run.state.lki.get(original).is_some());
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(2))));
        assert!(
            report.executed[0]
                .events
                .iter()
                .all(|ev| !matches!(ev.instance.event(), GameEvent::BecomeCopy { .. }))
        );
    }
}

/// REQ-EVENT-001, G1: CR 707.5: with the source staying in its zone the copy works (zone change
/// and token); a never-issued source fails and rolls back (see the F6 test); a standalone
/// `BecomeCopy` of a source that left in an EARLIER call stays an error (`UnknownObject`) and
/// changes nothing, although its LKI exists.
#[test]
fn req_event_001_r2_g1_source_stays_and_standalone_stays_strict() {
    let (mut w, original, h) = copy_world();
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(BusEventKind::CreateToken),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    ));
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    let new = new_id_of(ok(&run), h);
    assert_eq!(run.state.events.markers().copy_of(new), Some(original));
    let token = token_event(&mut w);
    let run = drive(&w, vec![token], false, &[]);
    let report = ok(&run);
    let object = match report.executed[0].events[0].outcome {
        crate::events::Outcome::TokenCreated { object } => object,
        other => panic!("{other:?}"),
    };
    assert_eq!(run.state.events.markers().copy_of(object), Some(original));
    // Standalone BecomeCopy of a source that left in an earlier call.
    let mut w = World::new();
    let original = w.create(2, ZoneKind::Battlefield);
    let other = w.create(1, ZoneKind::Battlefield);
    w.setup(zc(original, ZoneKind::Graveyard, ZoneChangeCause::Destroy));
    assert!(w.lki.get(original).is_some());
    let before = w.state_hash();
    let run = drive(
        &w,
        vec![GameEvent::BecomeCopy {
            object: other,
            copied: original,
        }],
        false,
        &[],
    );
    assert_eq!(
        run.out,
        Err(BusError::Event(EventError::UnknownObject(original)))
    );
    assert_eq!(run.hash, before);
}

/// REQ-EVENT-001, F6: CR 707.5, CR 603.6d: the call that enters a permanent as a copy logs ONE
/// executed batch holding the entering event and no `BecomeCopy`; a `Skip` effect on
/// `BecomeCopy` cannot affect it.
#[test]
fn req_event_001_r2_f6_one_executed_batch_for_the_entry() {
    let (mut w, original, h) = copy_world();
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher::new(BusEventKind::BecomeCopy),
        Action::Skip,
    ));
    let log_before = w.events.log().len();
    let run = drive(
        &w,
        vec![zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(
        report.executed[0].events[0].instance.event(),
        &zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)
    );
    assert_eq!(run.state.events.log().len(), log_before + 1);
    let new = new_id_of(report, h);
    assert_eq!(run.state.events.markers().copy_of(new), Some(original));
}

/// A pool world plus a damage modifier `op` to player 2 at `tier` (effect e2).
fn pool_world_with(pool: i64, op: AmountOp, tier: Tier) -> (World, ObjectId, ObjectId) {
    let (mut w, a, b) = pool_world(pool);
    w.add(
        ReplacementEffect::new(
            e(2),
            EventMatcher {
                affected_player: Some(p(2)),
                ..EventMatcher::new(BusEventKind::Damage)
            },
            Action::Modify(Modification::Amount(op)),
        )
        .with_tier(tier),
    );
    (w, a, b)
}

/// The amount of the damage events from `source` executed in `report`.
fn dealt_by(report: &BusReport, source: ObjectId) -> i64 {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::Damage {
                source: Some(s),
                amount,
                ..
            } if *s == source => Some(*amount),
            _ => None,
        })
        .sum()
}

/// REQ-EVENT-001, G2: CR 615.7, CR 616.1f, CR 616.2: a self-replacement doubling (1 + 1 becomes
/// 2 + 2) applies before the pool of 3, so the allocation is asked on the CURRENT damage (one
/// `DIVIDE_VALUE`, total 3) and both mirrored answers are honoured; replays are identical.
#[test]
fn req_event_001_r2_g2_allocation_on_modified_damage() {
    let (w, a, b) = pool_world_with(3, AmountOp::Mul(2), Tier::SelfReplacement);
    let events = vec![
        damage_from_to_player(a, 2, 1),
        damage_from_to_player(b, 2, 1),
    ];
    for (share_a, share_b) in [(1, 2), (2, 1)] {
        let answers = [division(&[(a, share_a), (b, share_b)])];
        let run = drive_with(&w, events.clone(), false, &answers);
        assert_eq!(run.asked.len(), 1);
        let r = &run.asked[0];
        assert_eq!(r.choice_type, ChoiceType::DivideValue);
        assert_eq!(r.chooser, p(2));
        assert_eq!(
            r.payload,
            ChoicePayload::DivideValue {
                total: 3,
                recipients: vec![(ChoiceTarget::Object(a), 0), (ChoiceTarget::Object(b), 0)],
            }
        );
        assert_eq!(r.default_on_timeout, division(&[(a, 2), (b, 1)]));
        let report = ok(&run);
        assert_eq!(report.prevented, 3);
        let dealt_a = 2 - i64::try_from(share_a).unwrap();
        let dealt_b = 2 - i64::try_from(share_b).unwrap();
        assert_eq!(dealt_by(report, a), dealt_a);
        assert_eq!(dealt_by(report, b), dealt_b);
        assert_eq!(run.state.life(2), 19);
        assert!(run.state.registry.get(e(1)).is_none());
        // Replay determinism.
        let again = drive_with(&w, events.clone(), false, &answers);
        assert_eq!(again.out, run.out);
        assert_eq!(again.hash, run.hash);
        assert_eq!(again.state.registry, run.state.registry);
        assert_eq!(again.asked, run.asked);
    }
}

/// REQ-EVENT-001, G2: CR 615.7, CR 616.2: a self-replacement reduction (2 + 2 becomes 1 + 1)
/// leaves the pool of 3 covering everything: nothing is asked, both are fully prevented, the
/// pool keeps 1.
#[test]
fn req_event_001_r2_g2_reduction_before_pool_asks_nothing() {
    let (w, a, b) = pool_world_with(3, AmountOp::Add(-1), Tier::SelfReplacement);
    let run = drive(
        &w,
        vec![
            damage_from_to_player(a, 2, 2),
            damage_from_to_player(b, 2, 2),
        ],
        false,
        &[],
    );
    assert!(run.asked.is_empty());
    let report = ok(&run);
    assert_eq!(report.prevented, 2);
    assert!(report.executed.is_empty());
    assert_eq!(run.state.life(2), 20);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Pool(1))
    );
}

/// REQ-EVENT-001, G2: CR 615.7, CR 616.1e, CR 616.1f: the doubling at the SAME tier as the pool
/// is the chooser's choice per member. Shield first: 1 + 1 <= 3, no division, both prevented,
/// requests [CHOOSE, CHOOSE]. Doubling first: 2 + 2 > 3, requests [CHOOSE, CHOOSE,
/// DIVIDE_VALUE (total 3)].
#[test]
fn req_event_001_r2_g2_same_tier_shield_versus_modifier() {
    let (w, a, b) = pool_world_with(3, AmountOp::Mul(2), Tier::Other);
    let events = vec![
        damage_from_to_player(a, 2, 1),
        damage_from_to_player(b, 2, 1),
    ];
    let pick = |index| ChoiceResponse::SelectReplacement { index };
    // Shield (e1, index 0) first.
    let run = drive_with(&w, events.clone(), false, &[pick(0), pick(0)]);
    let kinds: Vec<ChoiceType> = run.asked.iter().map(|r| r.choice_type).collect();
    assert_eq!(
        kinds,
        vec![
            ChoiceType::ChooseReplacementEffect,
            ChoiceType::ChooseReplacementEffect
        ]
    );
    let report = ok(&run);
    assert_eq!(report.prevented, 2);
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(run.state.life(2), 20);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Pool(1))
    );
    // Doubling (e2, index 1) first.
    let run = drive_with(
        &w,
        events,
        false,
        &[pick(1), pick(1), division(&[(a, 2), (b, 1)])],
    );
    let kinds: Vec<ChoiceType> = run.asked.iter().map(|r| r.choice_type).collect();
    assert_eq!(
        kinds,
        vec![
            ChoiceType::ChooseReplacementEffect,
            ChoiceType::ChooseReplacementEffect,
            ChoiceType::DivideValue
        ]
    );
    assert!(matches!(
        run.asked[2].payload,
        ChoicePayload::DivideValue { total: 3, .. }
    ));
    let report = ok(&run);
    assert_eq!(report.prevented, 3);
    assert_eq!(report.applications, vec![e(2), e(2), e(1), e(1)]);
    assert_eq!(dealt_by(report, a), 0);
    assert_eq!(dealt_by(report, b), 1);
    assert_eq!(run.state.life(2), 19);
}

/// REQ-EVENT-001, G2: CR 615.7, CR 616.1: the allocation request comes AFTER the active
/// player's unrelated `CHOOSE_REPLACEMENT_EFFECT` made before the pool is reached.
#[test]
fn req_event_001_r2_g2_request_order_choice_then_division() {
    let (mut w, a, b) = pool_world(3);
    for id in [5, 6] {
        w.add(ReplacementEffect::new(
            e(id),
            EventMatcher {
                affected_player: Some(p(1)),
                ..EventMatcher::new(BusEventKind::LifeChange)
            },
            Action::Modify(Modification::Amount(AmountOp::Add(1))),
        ));
    }
    let run = drive_with(
        &w,
        vec![
            damage_from_to_player(a, 2, 3),
            damage_from_to_player(b, 2, 3),
            life(1, 1),
        ],
        false,
        &[
            ChoiceResponse::SelectReplacement { index: 0 },
            division(&[(a, 3), (b, 0)]),
        ],
    );
    let asked: Vec<(ChoiceType, PlayerId)> = run
        .asked
        .iter()
        .map(|r| (r.choice_type, r.chooser))
        .collect();
    assert_eq!(
        asked,
        vec![
            (ChoiceType::ChooseReplacementEffect, p(1)),
            (ChoiceType::DivideValue, p(2))
        ]
    );
    let report = ok(&run);
    assert_eq!(report.prevented, 3);
    assert_eq!(run.state.life(1), 23);
    assert_eq!(run.state.life(2), 17);
}

/// The G3 world: `x` (P1) with `counters` shield counters and its shield-counter damage
/// prevention (e1); sources `s1`, `s2`.
fn shield_world(counters: i64) -> (World, ObjectId, ObjectId, ObjectId) {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let s1 = w.create(2, ZoneKind::Battlefield);
    let s2 = w.create(2, ZoneKind::Battlefield);
    w.counters(x, "shield", counters);
    w.add(ReplacementEffect::shield_counter_damage(e(1), x));
    (w, x, s1, s2)
}

/// The `RemoveCounters` events executed in `report`.
fn removals(report: &BusReport) -> usize {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter(|ev| matches!(ev.instance.event(), GameEvent::RemoveCounters { .. }))
        .count()
}

/// REQ-EVENT-001, G3: CR 122.1c, CR 615.13: simultaneous damage from two sources to a permanent
/// with 2 shield counters is prevented by ONE application per recipient for the follow-up:
/// one counter removed, both events prevented, `applications` lists e1 per prevented event.
/// With one counter both are still prevented and the one counter is removed.
#[test]
fn req_event_001_r2_g3_one_removal_for_simultaneous_damage() {
    for (counters, left) in [(2, 1), (1, 0)] {
        let (w, x, s1, s2) = shield_world(counters);
        let run = drive(
            &w,
            vec![damage_from(s1, x, 1), damage_from(s2, x, 1)],
            false,
            &[],
        );
        let report = ok(&run);
        assert_eq!(report.prevented, 2);
        assert_eq!(report.applications, vec![e(1), e(1)]);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(report.executed[0].events.len(), 1);
        assert_eq!(removals(report), 1);
        assert_eq!(run.state.counter(x, "shield"), left);
        assert_eq!(run.state.events.markers().damage(x), 0);
    }
}

/// REQ-EVENT-001, G3: CR 122.1c: separate damage batches remove separately (2 -> 1 -> 0); one
/// damage event removes one; two different permanents each lose one.
#[test]
fn req_event_001_r2_g3_separate_batches_and_recipients() {
    let (w, x, s1, s2) = shield_world(2);
    let run = drive(&w, vec![damage_from(s1, x, 1)], false, &[]);
    assert_eq!(removals(ok(&run)), 1);
    assert_eq!(run.state.counter(x, "shield"), 1);
    let run = drive(&run.state, vec![damage_from(s2, x, 1)], false, &[]);
    assert_eq!(removals(ok(&run)), 1);
    assert_eq!(ok(&run).prevented, 1);
    assert_eq!(run.state.counter(x, "shield"), 0);
    // Two recipients.
    let (mut w, x, s1, s2) = shield_world(2);
    let y = w.create(1, ZoneKind::Battlefield);
    w.counters(y, "shield", 2);
    w.add(ReplacementEffect::shield_counter_damage(e(2), y));
    let run = drive(
        &w,
        vec![damage_from(s1, x, 1), damage_from(s2, y, 1)],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.prevented, 2);
    assert_eq!(removals(report), 2);
    assert_eq!(run.state.counter(x, "shield"), 1);
    assert_eq!(run.state.counter(y, "shield"), 1);
}

/// REQ-EVENT-001, G3: CR 615.13: a generic shield-less `Prevent { then }` on damage to one player
/// from two simultaneous sources queues its follow-up once; a `Uses` shield is unchanged (one
/// follow-up per application).
#[test]
fn req_event_001_r2_g3_generic_prevent_follow_up_once() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let b = w.create(1, ZoneKind::Battlefield);
    let matcher = EventMatcher {
        affected_player: Some(p(2)),
        ..EventMatcher::new(BusEventKind::Damage)
    };
    w.add(ReplacementEffect::new(
        e(1),
        matcher.clone(),
        Action::Prevent {
            then: vec![life(1, 1)],
        },
    ));
    let events = vec![
        damage_from_to_player(a, 2, 2),
        damage_from_to_player(b, 2, 3),
    ];
    let run = drive(&w, events.clone(), false, &[]);
    let report = ok(&run);
    assert_eq!(report.prevented, 5);
    assert_eq!(report.applications, vec![e(1), e(1)]);
    assert_eq!(run.state.life(1), 21);
    assert_eq!(run.state.life(2), 20);
    // With a Uses shield each application queues its follow-up.
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let b = w.create(1, ZoneKind::Battlefield);
    w.add(
        ReplacementEffect::new(
            e(1),
            matcher,
            Action::Prevent {
                then: vec![life(1, 1)],
            },
        )
        .with_shield(Shield::Uses(2)),
    );
    let run = drive(
        &w,
        vec![
            damage_from_to_player(a, 2, 2),
            damage_from_to_player(b, 2, 3),
        ],
        false,
        &[],
    );
    assert_eq!(ok(&run).prevented, 5);
    assert_eq!(run.state.life(1), 22);
}

/// REQ-EVENT-001, G4: CR 615.7: pool 4, A deals 3, B deals 2: each share is capped at its
/// source's damage, so every point of the pool is used. (A:2, B:2) prevents 4, A deals 1, B
/// none, the pool is gone; (A:3, B:1) is valid; (A:1, B:3) exceeds B's 2 and is rejected
/// (nothing changes); the timeout default is (A:3, B:1).
#[test]
fn req_event_001_r2_g4_allotment_capped_at_source_damage() {
    let (w, a, b) = pool_world(4);
    let events = vec![
        damage_from_to_player(a, 2, 3),
        damage_from_to_player(b, 2, 2),
    ];
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 2), (b, 2)])]);
    assert_eq!(run.asked.len(), 1);
    assert_eq!(run.asked[0].default_on_timeout, division(&[(a, 3), (b, 1)]));
    let report = ok(&run);
    assert_eq!(report.prevented, 4);
    assert_eq!(dealt_by(report, a), 1);
    assert_eq!(dealt_by(report, b), 0);
    assert_eq!(run.state.life(2), 19);
    assert!(run.state.registry.get(e(1)).is_none());
    let run = drive_with(&w, events.clone(), false, &[division(&[(a, 3), (b, 1)])]);
    let report = ok(&run);
    assert_eq!(report.prevented, 4);
    assert_eq!(dealt_by(report, a), 0);
    assert_eq!(dealt_by(report, b), 1);
    assert!(run.state.registry.get(e(1)).is_none());
    // Over the cap: rejected, everything restored.
    let before = w.state_hash();
    let run = drive_with(&w, events, false, &[division(&[(a, 1), (b, 3)])]);
    assert!(matches!(
        run.out,
        Err(BusError::Choice(ChoiceError::Procedure(_)))
    ));
    assert_eq!(run.hash, before);
    assert_eq!(run.state.registry, w.registry);
}

/// REQ-EVENT-001, G4: CR 615.7: pool 3, A deals 3, B deals 2: (3,0), (2,1), (1,2) each prevent
/// 3 and leave 2 dealt; the default is (A:3, B:0). Pool 6 asks nothing, prevents 5, keeps 1,
/// which prevents 1 of a later damage event.
#[test]
fn req_event_001_r2_g4_pool_three_and_pool_six() {
    let (w, a, b) = pool_world(3);
    let events = vec![
        damage_from_to_player(a, 2, 3),
        damage_from_to_player(b, 2, 2),
    ];
    for shares in [[(a, 3), (b, 0)], [(a, 2), (b, 1)], [(a, 1), (b, 2)]] {
        let run = drive_with(&w, events.clone(), false, &[division(&shares)]);
        assert_eq!(run.asked[0].default_on_timeout, division(&[(a, 3), (b, 0)]));
        let report = ok(&run);
        assert_eq!(report.prevented, 3);
        assert_eq!(dealt_by(report, a) + dealt_by(report, b), 2);
        assert_eq!(run.state.life(2), 18);
        assert!(run.state.registry.get(e(1)).is_none());
    }
    let (w, a, b) = pool_world(6);
    let run = drive(
        &w,
        vec![
            damage_from_to_player(a, 2, 3),
            damage_from_to_player(b, 2, 2),
        ],
        false,
        &[],
    );
    assert!(run.asked.is_empty());
    assert_eq!(ok(&run).prevented, 5);
    assert_eq!(
        run.state.registry.get(e(1)).unwrap().shield,
        Some(Shield::Pool(1))
    );
    let run = drive(&run.state, vec![damage_from_to_player(a, 2, 3)], false, &[]);
    assert_eq!(ok(&run).prevented, 1);
    assert_eq!(run.state.life(2), 18);
    assert!(run.state.registry.get(e(1)).is_none());
}

/// REQ-EVENT-001, F2 (corrected in review round 3, H2): CR 704.3, CR 704.7: [destroy, sba
/// move] and [sba move, destroy] of one permanent give identical reports, registries and state
/// hashes. Member-wise semantics: the shield counter's destroy replacement (cause `Destroy`
/// only, CR 122.1c, CR 701.8b) replaces the destroy member once (counter removed) and the sba
/// member still moves the object to the graveyard; a one-use exile redirect of destructions
/// replaces the destroy member, which wins the execution: the card is exiled.
#[test]
fn req_event_001_r2_f2_coalescing_is_order_independent() {
    let destroy_first = |x| {
        vec![
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        ]
    };
    // Shield counter (destruction half).
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    w.counters(x, "shield", 1);
    w.add(ReplacementEffect::shield_counter_destroy(e(1), x));
    let forward = destroy_first(x);
    let mut backward = forward.clone();
    backward.reverse();
    let one = drive(&w, forward.clone(), true, &[]);
    let two = drive(&w, backward.clone(), true, &[]);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    let report = ok(&one);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(one.state.counter(x, "shield"), 0);
    assert_eq!(one.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(zone_changes(report), 1);
    // A one-use exile redirect of destructions only.
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                cause: Some(ZoneChangeCause::Destroy),
                ..EventMatcher::new(BusEventKind::ZoneChange)
            },
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_shield(Shield::Uses(1)),
    );
    let forward = destroy_first(x);
    let mut backward = forward.clone();
    backward.reverse();
    let one = drive(&w, forward, true, &[]);
    let two = drive(&w, backward, true, &[]);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    let report = ok(&one);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(one.state.zone_of_card(card), Some(ZoneId::Exile));
}

// ---------------------------------------------------------------------------------------------
// Review round 3
// ---------------------------------------------------------------------------------------------

/// The number of zone changes executed in `report`.
fn zone_changes(report: &BusReport) -> usize {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter(|ev| matches!(ev.instance.event(), GameEvent::ZoneChange { .. }))
        .count()
}

/// Runs `events` (sba true) and the same events reversed with the same answers; asserts equal
/// outputs, registries and state hashes (CR 704.7 order independence); returns the first run.
fn both_orders(w: &World, events: &[GameEvent], answers: &[u32]) -> Run {
    let mut backward = events.to_vec();
    backward.reverse();
    let one = drive(w, events.to_vec(), true, answers);
    let two = drive(w, backward, true, answers);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    assert_eq!(one.asked, two.asked);
    one
}

/// A one-use effect `id` on zone changes with `cause`, doing `action`.
fn by_cause(id: u64, cause: ZoneChangeCause, action: Action) -> ReplacementEffect {
    ReplacementEffect::new(
        e(id),
        EventMatcher {
            cause: Some(cause),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        },
        action,
    )
    .with_shield(Shield::Uses(1))
}

fn to_exile() -> Action {
    Action::Modify(Modification::SetDestination(ZoneKind::Exile))
}

/// A permanent of P1 and its [destroy, sba] moves to the graveyard.
fn mixed_group() -> (World, ObjectId, CardId, Vec<GameEvent>) {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    let events = vec![
        zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
    ];
    (w, x, card, events)
}

/// REQ-EVENT-001, REQ-CORE-001, round 3 H2 (a): CR 704.7: a one-use exile redirect of
/// state-based moves only is applied to the sba member of a [destroy, sba] group even though
/// the destroy member sorts first: one application, the card is exiled, in both orders.
#[test]
fn req_event_001_r3_h2_a_replacement_matching_a_non_first_member_applies() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::StateBasedAction, to_exile()));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert!(run.asked.is_empty());
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert!(run.state.registry.get(e(1)).is_none());
}

/// REQ-EVENT-001, round 3 H2 (b): control: the sba move alone is redirected to exile.
#[test]
fn req_event_001_r3_h2_b_sba_move_alone_is_redirected() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::StateBasedAction, to_exile()));
    let run = drive(&w, vec![events[1].clone()], true, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
}

/// REQ-EVENT-001, round 3 H2 (c): CR 704.7: a one-use `Skip` of state-based moves only drops
/// the sba member; the destroy member, which it does not match, still moves the object to the
/// graveyard; both orders.
#[test]
fn req_event_001_r3_h2_c_skip_of_one_member_leaves_the_other() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::StateBasedAction, Action::Skip));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

/// REQ-EVENT-001, round 3 H2 (d): CR 122.1c, CR 701.8b, CR 704.7: the shield counter's
/// destroy replacement applies to the destroy member only (once: the counter is removed, the
/// effect is applied once); the sba member still moves the object to the graveyard; one
/// executed move; both orders. Round 4 (J2, CR 704.3): the counter removal is part of the same
/// executed batch, placed before the move.
#[test]
fn req_event_001_r3_h2_d_shield_counter_on_mixed_group() {
    let (mut w, x, card, events) = mixed_group();
    w.counters(x, "shield", 1);
    w.add(ReplacementEffect::shield_counter_destroy(e(1), x));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.counter(x, "shield"), 0);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    // One executed batch: the removal, then the move.
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 2);
    assert!(matches!(
        report.executed[0].events[0].instance.event(),
        GameEvent::RemoveCounters { .. }
    ));
    assert!(matches!(
        report.executed[0].events[1].instance.event(),
        GameEvent::ZoneChange { .. }
    ));
    // The shield counter does not apply to a state-based move alone (CR 122.1c, CR 704.1).
    let run = drive(&w, vec![events[1].clone()], true, &[]);
    assert!(ok(&run).applications.is_empty());
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

/// REQ-EVENT-001, round 3 H2 (e): CR 704.7: two identical sba members and a one-use redirect:
/// one application covers both; one move, to exile.
#[test]
fn req_event_001_r3_h2_e_same_cause_duplicates_one_application() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::StateBasedAction, to_exile()));
    let both = vec![events[1].clone(), events[1].clone()];
    let run = drive(&w, both, true, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert!(run.state.registry.get(e(1)).is_none());
}

/// REQ-EVENT-001, round 3 H2 (f): CR 704.7: a one-use exile redirect of destructions only:
/// the replaced destroy member wins over the unmatched sba member (different destinations:
/// the modified member is executed); the card is exiled; both orders.
#[test]
fn req_event_001_r3_h2_f_modified_member_wins() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::Destroy, to_exile()));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
}

/// REQ-EVENT-001, round 3 H2 (g): CR 616.1, CR 616.1f, CR 704.7: e1 redirects destructions to
/// exile, e2 skips state-based moves; the union of candidates is offered once to the object's
/// chooser (P1: one `CHOOSE_REPLACEMENT_EFFECT` with both); the chosen one applies to its
/// member; the other is then the only candidate and applies without a request. Either answer
/// ends with the destroy member exiling the card; applications in choice order; both orders.
#[test]
fn req_event_001_r3_h2_g_union_of_candidates_one_request() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::Destroy, to_exile()));
    w.add(by_cause(2, ZoneChangeCause::StateBasedAction, Action::Skip));
    for (answer, applications) in [(0, vec![e(1), e(2)]), (1, vec![e(2), e(1)])] {
        let run = both_orders(&w, &events, &[answer]);
        assert_eq!(run.asked.len(), 1);
        let r = &run.asked[0];
        assert_eq!(r.choice_type, ChoiceType::ChooseReplacementEffect);
        assert_eq!(r.chooser, p(1));
        assert_eq!(effects_of(r), vec![desc(1), desc(2)]);
        let report = ok(&run);
        assert_eq!(report.applications, applications);
        assert_eq!(zone_changes(report), 1);
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
        assert!(run.state.registry.is_empty());
    }
}

/// REQ-EVENT-001, REQ-CORE-001, round 3 H2, round 4 J6: CR 704.7: both members replaced to
/// different destinations: exactly one move, the least by canonical content (the hand), the
/// same for both orders.
#[test]
fn req_event_001_r3_h2_divergent_destinations_one_move() {
    let (mut w, _, card, events) = mixed_group();
    w.add(by_cause(1, ZoneChangeCause::Destroy, to_exile()));
    w.add(by_cause(
        2,
        ZoneChangeCause::StateBasedAction,
        Action::Modify(Modification::SetDestination(ZoneKind::Hand)),
    ));
    let run = both_orders(&w, &events, &[0]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2)]);
    assert_eq!(zone_changes(report), 1);
    // Round 4 J6: both members redirect (exile, hand), both differ from the original
    // graveyard, so all compete by content: the hand's encoding tag (1) is below exile's (5)
    // and the destination is encoded before the cause, so the hand move is the least.
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Hand(p(1))));
}

/// REQ-EVENT-001, round 3 H2 (h): CR 704.7: moves of two different objects never merge: a
/// one-use redirect of state-based moves exiles only the first object.
#[test]
fn req_event_001_r3_h2_h_different_objects_do_not_merge() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let y = w.create(1, ZoneKind::Battlefield);
    w.add(by_cause(1, ZoneChangeCause::StateBasedAction, to_exile()));
    let events = vec![
        zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        zc(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
    ];
    let run = drive(&w, events, true, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(zone_changes(report), 2);
    assert_eq!(run.state.zone_of_card(w.card_of(x)), Some(ZoneId::Exile));
    assert_eq!(
        run.state.zone_of_card(w.card_of(y)),
        Some(ZoneId::Graveyard(p(1)))
    );
}

/// The F8 pool world with the pool effect overriding CR 614.5 (CR 903.9b-style).
fn overriding_pool_world(pool: i64) -> (World, ObjectId, ObjectId) {
    let (mut w, a, b) = pool_world(pool);
    let effect = w.registry.remove(e(1)).unwrap();
    w.add(effect.with_overrides_614_5(true));
    (w, a, b)
}

/// REQ-EVENT-001, round 3 H4: CR 615.7, CR 614.5: an overriding pool of 3 against A:3 and
/// B:3: every division is honoured without `RepeatLimit`: (1,2) prevents 1 of A and 2 of B,
/// (2,1) the reverse, (3,0) and (0,3) as before; the pool is used up and removed. The
/// non-overriding pool gives the same results.
#[test]
fn req_event_001_r3_h4_overriding_pool_honours_split() {
    for overriding in [true, false] {
        let (w, a, b) = if overriding {
            overriding_pool_world(3)
        } else {
            pool_world(3)
        };
        let events = vec![
            damage_from_to_player(a, 2, 3),
            damage_from_to_player(b, 2, 3),
        ];
        for (sa, sb) in [(1, 2), (2, 1), (3, 0), (0, 3)] {
            let run = drive_with(&w, events.clone(), false, &[division(&[(a, sa), (b, sb)])]);
            let report = ok(&run);
            assert_eq!(report.prevented, 3);
            assert_eq!(dealt_by(report, a), 3 - i64::try_from(sa).unwrap());
            assert_eq!(dealt_by(report, b), 3 - i64::try_from(sb).unwrap());
            assert_eq!(run.state.life(2), 17);
            assert!(run.state.registry.get(e(1)).is_none());
        }
    }
}

/// REQ-EVENT-001, round 3 H5: CR 615.7, CR 120.1: pool 4; sourceless 4, A 4 and B 4 to P2;
/// P2 gives A all 4: A takes the pool whatever the submission order; the sourceless damage
/// and B's are dealt in full.
#[test]
fn req_event_001_r3_h5_allotted_members_take_the_pool_first() {
    let (w, a, b) = pool_world(4);
    let s = damage_player(2, 4);
    let da = damage_from_to_player(a, 2, 4);
    let db = damage_from_to_player(b, 2, 4);
    for events in [
        vec![s.clone(), da.clone(), db.clone()],
        vec![da.clone(), db.clone(), s.clone()],
        vec![db.clone(), s.clone(), da.clone()],
    ] {
        let run = drive_with(&w, events, false, &[division(&[(a, 4), (b, 0)])]);
        assert_eq!(run.asked.len(), 1);
        assert_eq!(
            run.asked[0].payload,
            ChoicePayload::DivideValue {
                total: 4,
                recipients: vec![(ChoiceTarget::Object(a), 0), (ChoiceTarget::Object(b), 0)],
            }
        );
        let report = ok(&run);
        assert_eq!(report.prevented, 4);
        assert_eq!(dealt_by(report, a), 0);
        assert_eq!(dealt_by(report, b), 4);
        assert_eq!(
            dealt_sources(report).iter().filter(|s| s.is_none()).count(),
            1
        );
        assert_eq!(run.state.life(2), 12);
        assert!(run.state.registry.get(e(1)).is_none());
    }
}

// ---------------------------------------------------------------------------------------------
// Review round 4
// ---------------------------------------------------------------------------------------------

/// The executed zone changes of `object` in `report`.
fn moves_of(report: &BusReport, object: ObjectId) -> usize {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter(|ev| {
            matches!(ev.instance.event(), GameEvent::ZoneChange { object: o, .. } if *o == object)
        })
        .count()
}

/// REQ-EVENT-001, REQ-CORE-001, round 4 J1: CR 704.3, CR 704.7, CR 614.6, CR 614.7: a one-use
/// `Instead` effect on destructions only, whose child moves the object to exile, applies to
/// the destroy member of a [destroy, sba] group; the child move competes with the unmatched
/// sba move and, being a redirect, wins: no `UnknownObject`, no rollback, one executed batch
/// with exactly one move, the card in exile, both orders.
#[test]
fn req_event_001_r4_j1_instead_child_move_competes_with_group_move() {
    let (mut w, x, card, events) = mixed_group();
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(x, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 4 J1 (i): CR 704.7, CR 614.7: the `Instead` child moves the object to
/// the graveyard (the group's own destination): one move, to the graveyard, both orders.
/// Round 5 N2: neither candidate is a redirect, so the least canonical content wins: the
/// cause is encoded last and `Other` (tag 0) sorts before `StateBasedAction` (tag 7), so the
/// executed move is the child's.
#[test]
fn req_event_001_r4_j1_i_instead_child_move_same_destination() {
    let (mut w, x, card, events) = mixed_group();
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(x, ZoneKind::Graveyard, ZoneChangeCause::Other)]),
    ));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 1);
    let executed: Vec<&GameEvent> = report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .map(|ev| ev.instance.event())
        .filter(|ev| matches!(ev, GameEvent::ZoneChange { .. }))
        .collect();
    assert_eq!(
        executed,
        vec![&zc(x, ZoneKind::Graveyard, ZoneChangeCause::Other)]
    );
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 4 J1 (ii): CR 616.2, CR 704.7: a second effect redirects the `Instead`
/// child's move (exile to hand): the child's FINAL destination is used: one move, to the hand,
/// both orders.
#[test]
fn req_event_001_r4_j1_ii_redirected_child_move_uses_final_destination() {
    let (mut w, x, card, events) = mixed_group();
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(x, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    w.add(by_cause(
        2,
        ZoneChangeCause::Other,
        Action::Modify(Modification::SetDestination(ZoneKind::Hand)),
    ));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Hand(p(1))));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 4 J1 (iii): CR 122.1c, CR 704.3, CR 704.7: an `Instead` child that is a
/// counter removal (the shield counter's destroy half) keeps the object id live: ONE executed
/// batch with the removal and then the plain sba move; the counter is removed; both orders.
#[test]
fn req_event_001_r4_j1_iii_counter_removal_child_keeps_plain_move() {
    let (mut w, x, card, events) = mixed_group();
    w.counters(x, "shield", 1);
    w.add(ReplacementEffect::shield_counter_destroy(e(1), x));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    let kinds: Vec<BusEventKind> = report.executed[0]
        .events
        .iter()
        .map(|ev| BusEventKind::of(ev.instance.event()))
        .collect();
    assert_eq!(
        kinds,
        vec![BusEventKind::RemoveCounters, BusEventKind::ZoneChange]
    );
    assert_eq!(
        report.executed[0].events[1].instance.event(),
        &zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction)
    );
    assert_eq!(run.state.counter(x, "shield"), 0);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

/// Checks the J2 outcome of `run` for the shielded `x` (and `y`, if moved too).
fn check_j2(run: &Run, x: ObjectId, y: Option<ObjectId>) {
    let report = ok(run);
    assert_eq!(report.applications, vec![e(1)]);
    // One executed batch: the removal first, then the moves.
    assert_eq!(report.executed.len(), 1);
    let batch = &report.executed[0].events;
    assert_eq!(batch.len(), 2 + usize::from(y.is_some()));
    assert!(matches!(
        batch[0].instance.event(),
        GameEvent::RemoveCounters { target: CounterTarget::Object(o), .. } if *o == x
    ));
    assert_eq!(moves_of(report, x), 1);
    // CR 704.8: the departing object's LKI is derived from the state before the batch,
    // shield counter included.
    let record = run
        .state
        .lki
        .get(x)
        .expect("no LKI for the departed object");
    assert_eq!(record.counters().get("shield"), Some(&1));
    // The counter counts are as before: none left on the old id, none on the new object.
    let new = new_id_of(report, x);
    assert_eq!(run.state.counter(x, "shield"), 0);
    assert_eq!(run.state.counter(new, "shield"), 0);
    if let Some(y) = y {
        assert_eq!(moves_of(report, y), 1);
        assert!(run.state.lki.get(y).is_some());
    }
}

/// REQ-EVENT-001, REQ-CORE-001, round 4 J2: CR 704.3, CR 704.8, CR 122.1c: a shielded permanent
/// departs in a [destroy, sba] group (the shield replaces the destroy member; the sba member
/// moves it): the removal and the move are one executed batch and the LKI record of the
/// departed object holds the shield counter; with and without an unrelated sba move of a second
/// object in the same batch; both orders of the group's members.
#[test]
fn req_event_001_r4_j2_lki_keeps_pre_sba_counters() {
    let (mut w, x, _, events) = mixed_group();
    w.counters(x, "shield", 1);
    w.add(ReplacementEffect::shield_counter_destroy(e(1), x));
    let run = both_orders(&w, &events, &[]);
    check_j2(&run, x, None);
    // With an unrelated sba move of Y in the same batch: the group's members swapped, Y last.
    let y = w.create(1, ZoneKind::Battlefield);
    let y_move = zc(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction);
    let one = drive(
        &w,
        vec![events[0].clone(), events[1].clone(), y_move.clone()],
        true,
        &[],
    );
    let two = drive(
        &w,
        vec![events[1].clone(), events[0].clone(), y_move],
        true,
        &[],
    );
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    check_j2(&one, x, Some(y));
}

/// REQ-EVENT-001, REQ-CORE-001, round 4 J3: CR 704.7, CR 616.1, CR 616.1f: e1 redirects the
/// destroy member to exile; e2 sets the sba member's (already false) face-down flag to false,
/// leaving its destination unchanged. Whatever is answered, the redirect wins: the card ends in
/// exile, both orders. Control: with e2 alone the card goes to the graveyard.
#[test]
fn req_event_001_r4_j3_unchanged_destination_modification_does_not_beat_redirect() {
    let (mut w, _, card, events) = mixed_group();
    let face_up = by_cause(
        2,
        ZoneChangeCause::StateBasedAction,
        Action::Modify(Modification::SetFaceDown(false)),
    );
    let mut only_e2 = w.clone();
    w.add(by_cause(1, ZoneChangeCause::Destroy, to_exile()));
    w.add(face_up.clone());
    for (answer, applications) in [(0, vec![e(1), e(2)]), (1, vec![e(2), e(1)])] {
        let run = both_orders(&w, &events, &[answer]);
        assert_eq!(run.asked.len(), 1);
        let report = ok(&run);
        assert_eq!(report.applications, applications);
        assert_eq!(zone_changes(report), 1);
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
        assert!(run.state.registry.is_empty());
    }
    only_e2.add(face_up);
    let run = both_orders(&only_e2, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(2)]);
    assert_eq!(zone_changes(report), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
}

/// REQ-EVENT-001, REQ-CORE-001, round 4 J4: CR 704.7, CR 707.5, CR 614.1c, CR 616.1c: two
/// identical hand-to-battlefield moves of one object (`sba` true); one instance already has the
/// enters-as-copy effect C in its applied set, so C applies only to the other. Both members end
/// with equal events and applied sets; the one with the entry-copy plan wins: the object enters
/// once, as a copy, in both submission orders, with equal reports and state hashes.
#[test]
fn req_event_001_r4_j4_entry_copy_plan_is_part_of_the_selection() {
    let (w, original, h) = copy_world();
    let enter = zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other);
    let mut applied = crate::events::AppliedEffects::new();
    applied.insert(e(1));
    let seeded = EventInstance::new(enter.clone()).with_applied(applied);
    let plain = EventInstance::new(enter);
    let one = drive_batch(
        &w,
        EventBatch::new(vec![seeded.clone(), plain.clone()]),
        true,
        &[],
    );
    let two = drive_batch(&w, EventBatch::new(vec![plain, seeded]), true, &[]);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    for run in [&one, &two] {
        let report = ok(run);
        assert_eq!(report.applications, vec![e(1)]);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(moves_of(report, h), 1);
        let new = new_id_of(report, h);
        assert_eq!(run.state.events.markers().copy_of(new), Some(original));
    }
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 N1: CR 704.7, CR 707.5, CR 614.1c: as J4, but the
/// seeded move (C already applied) has cause `Other` and the plain one `StateBasedAction`. C
/// applies to the plain move only. Neither is a redirect; the cause is encoded last and `Other`
/// (tag 0) sorts before `StateBasedAction` (tag 7), so the seeded move wins on canonical event
/// content; it carries no entry-copy plan and the losing plain move's plan is dropped: the new
/// object is NOT a copy, in both submission orders, with equal reports and state hashes.
#[test]
fn req_event_001_r5_n1_losing_move_cannot_supply_copy_plan() {
    let (w, _, h) = copy_world();
    let mut applied = crate::events::AppliedEffects::new();
    applied.insert(e(1));
    let seeded = EventInstance::new(zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other))
        .with_applied(applied);
    let plain = EventInstance::new(zc(
        h,
        ZoneKind::Battlefield,
        ZoneChangeCause::StateBasedAction,
    ));
    let one = drive_batch(
        &w,
        EventBatch::new(vec![seeded.clone(), plain.clone()]),
        true,
        &[],
    );
    let two = drive_batch(&w, EventBatch::new(vec![plain, seeded]), true, &[]);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    for run in [&one, &two] {
        let report = ok(run);
        assert_eq!(report.applications, vec![e(1)]);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(moves_of(report, h), 1);
        assert_eq!(
            report.executed[0].events[0].instance.event(),
            &zc(h, ZoneKind::Battlefield, ZoneChangeCause::Other)
        );
        let new = new_id_of(report, h);
        assert_eq!(run.state.events.markers().copy_of(new), None);
    }
}

// ---------------------------------------------------------------------------------------------
// Review round 5
// ---------------------------------------------------------------------------------------------

/// Runs `events` (sba true) and the same events with the first two (a CR 704.7 group's
/// members) swapped, with the same answers; asserts equal outputs, registries, state hashes
/// and requests; returns the first run.
fn both_group_orders(w: &World, events: &[GameEvent], answers: &[u32]) -> Run {
    let mut swapped = events.to_vec();
    swapped.swap(0, 1);
    let one = drive(w, events.to_vec(), true, answers);
    let two = drive(w, swapped, true, answers);
    assert_eq!(one.out, two.out);
    assert_eq!(one.hash, two.hash);
    assert_eq!(one.state.registry, two.state.registry);
    assert_eq!(one.asked, two.asked);
    one
}

/// A `RemoveCounters` of one `kind` counter from `object`.
fn remove_one_counter(object: ObjectId, kind: &str) -> GameEvent {
    GameEvent::RemoveCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount: 1,
    }
}

/// A one-use effect `id` on events of `kind` affecting `object`, doing `action`.
fn on_object(id: u64, kind: BusEventKind, object: ObjectId, action: Action) -> ReplacementEffect {
    ReplacementEffect::new(
        e(id),
        EventMatcher {
            affected_object: Some(object),
            ..EventMatcher::new(kind)
        },
        action,
    )
    .with_shield(Shield::Uses(1))
}

/// The bus event kinds of the one executed batch of `report`.
fn batch_kinds(report: &BusReport) -> Vec<BusEventKind> {
    assert_eq!(report.executed.len(), 1);
    report.executed[0]
        .events
        .iter()
        .map(|ev| BusEventKind::of(ev.instance.event()))
        .collect()
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 K2: CR 614.6, CR 704.3, CR 704.7, CR 400.7: a one-use
/// `Instead` on destructions replaces the destroy member of X's [destroy, sba] group with a
/// move of Y to exile, while Y has its own sba move. The `Instead` move is part of the batch;
/// Y's two candidates are reconciled (the replacement-produced move is a redirect and wins):
/// no `UnknownObject`, no rollback; X goes to the graveyard, Y to exile, one move each, one
/// executed batch; both orders of the group's members.
#[test]
fn req_event_001_r5_k2_group_child_move_of_singleton_object() {
    let (mut w, x, xcard, mut events) = mixed_group();
    let y = w.create(1, ZoneKind::Battlefield);
    let ycard = w.card_of(y);
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(y, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    events.push(zc(
        y,
        ZoneKind::Graveyard,
        ZoneChangeCause::StateBasedAction,
    ));
    let run = both_group_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(moves_of(report, y), 1);
    assert_eq!(run.state.zone_of_card(xcard), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(run.state.zone_of_card(ycard), Some(ZoneId::Exile));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 5 K2: CR 614.6, CR 704.3, CR 400.7: as above with X's destroy move
/// alone (no group, `sba` true): X's only move is replaced, so X stays; Y moves once, to exile;
/// one executed batch.
#[test]
fn req_event_001_r5_k2_ungrouped_child_move_of_singleton_object() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let y = w.create(1, ZoneKind::Battlefield);
    let (xcard, ycard) = (w.card_of(x), w.card_of(y));
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(y, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    let run = drive(
        &w,
        vec![
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            zc(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        ],
        true,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 0);
    assert_eq!(moves_of(report, y), 1);
    assert_eq!(run.state.zone_of_card(xcard), Some(ZoneId::Battlefield));
    assert_eq!(run.state.zone_of_card(ycard), Some(ZoneId::Exile));
}

/// REQ-EVENT-001, round 5 K2 (E5): CR 614.6, CR 400.7: `sba` false, [destroy X, destroy Y]; a
/// one-use effect on X's zone changes replaces X's move with a move of Y to exile. Y's two
/// candidates are not all submitted, so they are reconciled and the `Instead` move (a redirect)
/// wins: X stays, Y is exiled once, one executed batch.
#[test]
fn req_event_001_r5_k2_non_sba_child_move_of_another_members_object() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let y = w.create(1, ZoneKind::Battlefield);
    let (xcard, ycard) = (w.card_of(x), w.card_of(y));
    w.add(on_object(
        1,
        BusEventKind::ZoneChange,
        x,
        Action::Instead(vec![zc(y, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    let run = drive(
        &w,
        vec![
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
            zc(y, ZoneKind::Graveyard, ZoneChangeCause::Destroy),
        ],
        false,
        &[],
    );
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 0);
    assert_eq!(moves_of(report, y), 1);
    assert_eq!(run.state.zone_of_card(xcard), Some(ZoneId::Battlefield));
    assert_eq!(run.state.zone_of_card(ycard), Some(ZoneId::Exile));
}

/// Checks the K3/E3 outcome: one executed batch [remove X's shield counter, move Y].
fn check_k3(run: &Run, x: ObjectId, y: ObjectId, ycard: CardId) {
    let report = ok(run);
    assert_eq!(report.applications, vec![e(1)]);
    assert_eq!(
        batch_kinds(report),
        vec![BusEventKind::RemoveCounters, BusEventKind::ZoneChange]
    );
    assert_eq!(
        report.executed[0].events[0].instance.event(),
        &remove_one_counter(x, "shield")
    );
    assert_eq!(
        report.executed[0].events[1].instance.event(),
        &zc(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction)
    );
    assert_eq!(moves_of(report, x), 0);
    assert_eq!(run.state.counter(x, "shield"), 0);
    assert!(run.state.permanents.get(x).is_some());
    assert_eq!(run.state.zone_of_card(ycard), Some(ZoneId::Graveyard(p(1))));
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 K3: CR 614.6, CR 704.3, CR 704.7: a one-use effect on
/// X's zone changes replaces BOTH members of X's [destroy, sba] group with the removal of X's
/// shield counter; Y has its own sba move. The removal is the modified event of the state-based
/// event, so it executes in the same batch as Y's move, before it; X stays with no shield
/// counter; both orders of the group's members.
#[test]
fn req_event_001_r5_k3_fully_replaced_group_stays_in_the_batch() {
    let (mut w, x, _, mut events) = mixed_group();
    w.counters(x, "shield", 1);
    let y = w.create(1, ZoneKind::Battlefield);
    w.add(on_object(
        1,
        BusEventKind::ZoneChange,
        x,
        Action::Instead(vec![remove_one_counter(x, "shield")]),
    ));
    events.push(zc(
        y,
        ZoneKind::Graveyard,
        ZoneChangeCause::StateBasedAction,
    ));
    let run = both_group_orders(&w, &events, &[]);
    check_k3(&run, x, y, w.card_of(y));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 5 E3: CR 614.6, CR 704.3: as K3 with a single (ungrouped) sba move of
/// X: the removal and Y's move are one executed batch, the removal first.
#[test]
fn req_event_001_r5_e3_fully_replaced_singleton_stays_in_the_batch() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "shield", 1);
    let y = w.create(1, ZoneKind::Battlefield);
    w.add(on_object(
        1,
        BusEventKind::ZoneChange,
        x,
        Action::Instead(vec![remove_one_counter(x, "shield")]),
    ));
    let run = drive(
        &w,
        vec![
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
            zc(y, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        ],
        true,
        &[],
    );
    check_k3(&run, x, y, w.card_of(y));
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 K5: CR 614.6, CR 616.1f, CR 616.2, CR 704.7: e1
/// replaces the destroy member of X's group with a counter removal; e2 replaces that removal
/// with a move of X to exile. The whole `Instead` lineage resolves in the batch to its final
/// event, which competes with the sba move as a redirect and wins: X is exiled once, no
/// rollback, one executed batch [move X to exile]; both orders.
#[test]
fn req_event_001_r5_k5_nested_instead_lineage_reaches_final_move() {
    let (mut w, x, card, events) = mixed_group();
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![remove_one_counter(x, "shield")]),
    ));
    w.add(on_object(
        2,
        BusEventKind::RemoveCounters,
        x,
        Action::Instead(vec![zc(x, ZoneKind::Exile, ZoneChangeCause::Other)]),
    ));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.executed[0].events.len(), 1);
    assert_eq!(
        report.executed[0].events[0].instance.event(),
        &zc(x, ZoneKind::Exile, ZoneChangeCause::Other)
    );
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert!(run.state.registry.is_empty());
}

/// A world with Q (P2) holding two shield counters and its damage-prevention half e9.
fn shielded_q(w: &mut World) -> ObjectId {
    let q = w.create(2, ZoneKind::Battlefield);
    w.counters(q, "shield", 2);
    w.add(ReplacementEffect::shield_counter_damage(e(9), q));
    q
}

/// REQ-EVENT-001, round 5 K4-inverse: CR 122.1c, CR 615.13, CR 614.6, CR 704.3: the `Instead`
/// of X's destroy member damages Q while another member of the batch damages Q too. The two
/// damage events are simultaneous (one batch, one record of follow-ups), so the shield counter's
/// prevention removes ONE counter, exactly as when the two damage events are submitted together;
/// both orders of the group's members.
#[test]
fn req_event_001_r5_k4_inverse_simultaneous_damage_shares_one_removal() {
    let (mut w, _, _, mut events) = mixed_group();
    let q = shielded_q(&mut w);
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![damage_object(q, 1)]),
    ));
    events.push(damage_object(q, 1));
    let run = both_group_orders(&w, &events, &[]);
    assert_eq!(run.state.counter(q, "shield"), 1);
    // Control: the same two damage events submitted together to a fresh world.
    let mut control = World::new();
    let q2 = shielded_q(&mut control);
    let plain = drive(
        &control,
        vec![damage_object(q2, 1), damage_object(q2, 1)],
        true,
        &[],
    );
    assert_eq!(plain.state.counter(q2, "shield"), 1);
}

/// REQ-EVENT-001, round 5 K4 (not a defect): CR 615.5, CR 615.13, CR 122.1c, CR 120.4: e2 and
/// e3 prevent damage to A and to B, each with a follow-up of 1 damage to Q (two shield
/// counters). Plain: [damage A, damage B]; group case: e1 replaces X's destroy member with the
/// damage to A, next to the damage to B. The follow-ups of one batch form one child batch, so
/// Q loses ONE counter in both cases (the R8 reading in DECISIONS).
#[test]
fn req_event_001_r5_k4_group_case_equals_plain_batch() {
    let (mut w, _, _, mut events) = mixed_group();
    let a = w.create(2, ZoneKind::Battlefield);
    let b = w.create(2, ZoneKind::Battlefield);
    let q = shielded_q(&mut w);
    for (id, target) in [(2, a), (3, b)] {
        w.add(ReplacementEffect::new(
            e(id),
            EventMatcher {
                affected_object: Some(target),
                ..EventMatcher::new(BusEventKind::Damage)
            },
            Action::Prevent {
                then: vec![damage_object(q, 1)],
            },
        ));
    }
    let plain = drive(
        &w,
        vec![damage_object(a, 1), damage_object(b, 1)],
        true,
        &[],
    );
    ok(&plain);
    assert_eq!(plain.state.counter(q, "shield"), 1);
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![damage_object(a, 1)]),
    ));
    events.push(damage_object(b, 1));
    let group = both_group_orders(&w, &events, &[]);
    ok(&group);
    assert_eq!(group.state.counter(q, "shield"), 1);
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 E1/E4: CR 704.3, CR 704.7, CR 400.7: moves of one object
/// to two destinations in one state-based batch (two singletons; two groups; a group and a
/// singleton) execute ONE move, never `UnknownObject`. No candidate is a redirect, so the least
/// canonical content wins: the destination is encoded before the cause and the graveyard's tag
/// (3) is below exile's (5), so X goes to the graveyard, in the submitted and the reversed order.
#[test]
fn req_event_001_r5_e1_e4_two_destinations_one_object_moves_once() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    let gy_sba = zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction);
    let gy_destroy = zc(x, ZoneKind::Graveyard, ZoneChangeCause::Destroy);
    let ex_sba = zc(x, ZoneKind::Exile, ZoneChangeCause::StateBasedAction);
    let ex_other = zc(x, ZoneKind::Exile, ZoneChangeCause::Other);
    for events in [
        vec![gy_sba.clone(), ex_sba.clone()],
        vec![
            gy_sba.clone(),
            gy_destroy.clone(),
            ex_sba.clone(),
            ex_other.clone(),
        ],
        vec![gy_sba.clone(), gy_destroy.clone(), ex_sba.clone()],
    ] {
        let run = both_orders(&w, &events, &[]);
        let report = ok(&run);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(moves_of(report, x), 1);
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    }
}

/// REQ-EVENT-001, round 5 E2: CR 704.3, CR 704.8, CR 614.6: the `Instead` of X's destroy member
/// removes Y's shield counter while Y has an sba move: one executed batch [removal, X's move,
/// Y's move]; Y's last known information holds the shield counter (captured before the batch).
#[test]
fn req_event_001_r5_e2_child_counter_removal_on_moving_object() {
    let (mut w, x, _, mut events) = mixed_group();
    let y = w.create(1, ZoneKind::Battlefield);
    w.counters(y, "shield", 1);
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![remove_one_counter(y, "shield")]),
    ));
    events.push(zc(
        y,
        ZoneKind::Graveyard,
        ZoneChangeCause::StateBasedAction,
    ));
    let run = both_group_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(
        batch_kinds(report),
        vec![
            BusEventKind::RemoveCounters,
            BusEventKind::ZoneChange,
            BusEventKind::ZoneChange
        ]
    );
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(moves_of(report, y), 1);
    let record = run.state.lki.get(y).expect("no LKI for Y");
    assert_eq!(record.counters().get("shield"), Some(&1));
}

/// REQ-EVENT-001, round 5 N3: CR 616.2, CR 704.7: the `Instead` child STARTS at the group's
/// own destination (graveyard) and e2 redirects it to exile: its FINAL destination makes it a
/// redirect, so it wins over the plain sba move: one move, to exile, both orders.
#[test]
fn req_event_001_r5_n3_final_destination_classifies_redirect() {
    let (mut w, x, card, events) = mixed_group();
    w.add(by_cause(
        1,
        ZoneChangeCause::Destroy,
        Action::Instead(vec![zc(x, ZoneKind::Graveyard, ZoneChangeCause::Other)]),
    ));
    w.add(by_cause(2, ZoneChangeCause::Other, to_exile()));
    let run = both_orders(&w, &events, &[]);
    let report = ok(&run);
    assert_eq!(report.applications, vec![e(1), e(2)]);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(moves_of(report, x), 1);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Exile));
    assert!(run.state.registry.is_empty());
}

/// REQ-EVENT-001, round 5 (Q3): CR 614.6, CR 616.1f, CR 616.2: an `Instead` lineage is judged
/// against the state before the batch. An effect overriding CR 614.5 (CR 903.9b-style) that
/// needs X's shield counter replaces a tap with [remove the counter, tap]; its own output tap
/// still sees the counter (nothing executes while the batch resolves), so it applies again at
/// every generation until `DepthExceeded`; everything is rolled back.
#[test]
fn req_event_001_r5_override_matching_own_instead_output_exceeds_depth() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    w.counters(x, "shield", 1);
    w.add(
        ReplacementEffect::new(
            e(1),
            EventMatcher {
                requires_counter: Some((x, "shield".to_owned())),
                ..EventMatcher::new(BusEventKind::Tap)
            },
            Action::Instead(vec![
                remove_one_counter(x, "shield"),
                GameEvent::Tap { object: x },
            ]),
        )
        .with_overrides_614_5(true),
    );
    let before = w.state_hash();
    let run = drive(&w, vec![GameEvent::Tap { object: x }], false, &[]);
    assert_eq!(run.out, Err(BusError::DepthExceeded));
    assert_eq!(run.hash, before);
    assert_eq!(run.state.counter(x, "shield"), 1);
    assert!(!run.state.permanents.get(x).unwrap().status().tapped);
}

/// REQ-EVENT-001, round 5 limitation (b) (DECISIONS "Known limitations of the one-batch
/// resolution (round 5)"): CR 615.7: pool e1 (5, damage to P2) is allocated without a division
/// on A 2 + B 2 (total within the pool) and keeps 1; pool e3 (4, damage to P1) is divided
/// between C 3 and D 3 (C 1, D 3). C's remaining 2 damage is then replaced (e2) by 1 damage
/// from A and 1 from B to P2: lineage damage that appears after e1 was allocated. It triggers a
/// SECOND allocation of e1, which is a division (2 > 1), asked of P2 (A 1, B 0): A's lineage
/// damage is prevented, B's is dealt. Pinned behaviour.
#[test]
fn req_event_001_r5_limit_b_lineage_damage_after_division() {
    let (mut w, a, b) = pool_world(5);
    let c = w.create(2, ZoneKind::Battlefield);
    let d = w.create(2, ZoneKind::Battlefield);
    let (c, d) = (c.min(d), c.max(d));
    w.add(
        ReplacementEffect::new(
            e(3),
            EventMatcher {
                affected_player: Some(p(1)),
                ..EventMatcher::new(BusEventKind::Damage)
            },
            Action::Prevent { then: Vec::new() },
        )
        .with_shield(Shield::Pool(4)),
    );
    w.add(ReplacementEffect::new(
        e(2),
        EventMatcher {
            affected_player: Some(p(1)),
            amount_below: Some(3),
            ..EventMatcher::new(BusEventKind::Damage)
        },
        Action::Instead(vec![
            damage_from_to_player(a, 2, 1),
            damage_from_to_player(b, 2, 1),
        ]),
    ));
    let events = vec![
        damage_from_to_player(a, 2, 2),
        damage_from_to_player(b, 2, 2),
        damage_from_to_player(c, 1, 3),
        damage_from_to_player(d, 1, 3),
    ];
    let run = drive_with(
        &w,
        events,
        false,
        &[division(&[(c, 1), (d, 3)]), division(&[(a, 1), (b, 0)])],
    );
    assert_eq!(run.asked.len(), 2);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(
        run.asked[1].payload,
        ChoicePayload::DivideValue {
            total: 1,
            recipients: vec![(ChoiceTarget::Object(a), 0), (ChoiceTarget::Object(b), 0)],
        }
    );
    assert_eq!(run.asked[1].chooser, p(2));
    let report = ok(&run);
    assert_eq!(report.executed.len(), 1);
    assert_eq!(report.prevented, 2 + 2 + 1 + 3 + 1);
    assert_eq!(run.state.life(1), 20);
    assert_eq!(run.state.life(2), 19);
    assert!(run.state.registry.get(e(1)).is_none());
    assert!(run.state.registry.get(e(3)).is_none());
}

/// REQ-EVENT-001, REQ-CORE-001, round 5 (shape 4): CR 614.6 (an impossible instruction is
/// ignored), CR 614.7, CR 400.7: a `Prevent` follow-up (directly, or through its `Instead`) that
/// names X, which the executed batch of this call already moved, is ignored in the later batch:
/// Ok, no `UnknownObject`, no rollback; the earlier batch is kept; deterministic.
#[test]
fn req_event_001_r5_follow_up_naming_moved_object_is_ignored() {
    for via_instead in [false, true] {
        let mut w = World::new();
        let x = w.create(1, ZoneKind::Battlefield);
        let card = w.card_of(x);
        let exile_x = zc(x, ZoneKind::Exile, ZoneChangeCause::Other);
        let then = if via_instead {
            w.add(
                ReplacementEffect::new(
                    e(2),
                    EventMatcher::new(BusEventKind::LifeChange),
                    Action::Instead(vec![exile_x.clone()]),
                )
                .with_shield(Shield::Uses(1)),
            );
            life(1, 1)
        } else {
            exile_x
        };
        w.add(
            ReplacementEffect::new(
                e(1),
                EventMatcher::new(BusEventKind::Damage),
                Action::Prevent { then: vec![then] },
            )
            .with_shield(Shield::Uses(1)),
        );
        let events = vec![
            damage_player(1, 1),
            zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
        ];
        let run = drive(&w, events.clone(), true, &[]);
        let again = drive(&w, events, true, &[]);
        assert_eq!(run.out, again.out);
        assert_eq!(run.hash, again.hash);
        let report = ok(&run);
        let applications = if via_instead {
            vec![e(1), e(2)]
        } else {
            vec![e(1)]
        };
        assert_eq!(report.applications, applications);
        assert_eq!(report.executed.len(), 1);
        assert_eq!(moves_of(report, x), 1);
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
        assert_eq!(run.state.life(1), 20);
    }
}

// ---------------------------------------------------------------------------------------------
// Review round 6 (D1, D2, D6)
// ---------------------------------------------------------------------------------------------

/// A one-use effect that doubles any life change (no player restriction).
fn doubler() -> ReplacementEffect {
    ReplacementEffect::new(
        e(9),
        EventMatcher::new(BusEventKind::LifeChange),
        Action::Modify(Modification::Amount(AmountOp::Mul(2))),
    )
    .with_shield(Shield::Uses(1))
}

/// An effect replacing a tap of `object` by `instead`.
fn tap_instead(id: u64, object: ObjectId, instead: Vec<GameEvent>) -> ReplacementEffect {
    ReplacementEffect::new(
        e(id),
        EventMatcher {
            affected_object: Some(object),
            ..EventMatcher::new(BusEventKind::Tap)
        },
        Action::Instead(instead),
    )
}

/// Drives `events` twice on `w` and checks both runs give the same output and hash
/// (REQ-CORE-001, REQ-CORE-006).
fn drive_twice(w: &World, events: Vec<GameEvent>, sba: bool, answers: &[u32]) -> Run {
    let run = drive(w, events.clone(), sba, answers);
    let again = drive(w, events, sba, answers);
    assert_eq!(run.out, again.out);
    assert_eq!(run.hash, again.hash);
    run
}

/// The executed `LifeChange` deltas of `report`, in execution order.
fn life_deltas(report: &BusReport) -> Vec<(PlayerId, i64)> {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::LifeChange { player, delta } => Some((*player, *delta)),
            _ => None,
        })
        .collect()
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D1: CR 101.4, CR 616.1, SRS 8.1 step [2]: P1's tap
/// becomes a P1 life gain (`Instead`); with P2's gain submitted at the same time, P1's
/// descendant takes P1's APNAP turn and gets the one-use doubler, in both submission orders,
/// exactly as a P1 gain submitted directly does.
#[test]
fn req_event_001_r6_d1_descendant_takes_its_choosers_apnap_turn() {
    let mut w = World::new();
    let t = w.create(1, ZoneKind::Battlefield);
    w.add(tap_instead(1, t, vec![life(1, 1)]));
    w.add(doubler());
    for events in [
        vec![GameEvent::Tap { object: t }, life(2, 1)],
        vec![life(2, 1), GameEvent::Tap { object: t }],
    ] {
        let run = drive_twice(&w, events, false, &[]);
        let report = ok(&run);
        assert_eq!(report.applications, vec![e(1), e(9)]);
        assert_eq!(run.state.life(1), 22);
        assert_eq!(run.state.life(2), 21);
        assert!(run.state.registry.get(e(9)).is_none());
    }
    // Control: the same gains submitted directly.
    let mut w = World::new();
    w.add(doubler());
    let run = drive_twice(&w, vec![life(1, 1), life(2, 1)], false, &[]);
    assert_eq!(run.state.life(1), 22);
    assert_eq!(run.state.life(2), 21);
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D1: CR 101.4, CR 616.1: both submitted events are
/// P2's; once P2's tap has resolved, its P1 descendant ranks before P2's pending gain and takes
/// the one-use doubler.
#[test]
fn req_event_001_r6_d1_descendant_of_p2_event_with_p1_chooser() {
    let mut w = World::new();
    let t2 = w.create(2, ZoneKind::Battlefield);
    w.add(tap_instead(1, t2, vec![life(1, 1)]));
    w.add(doubler());
    let run = drive_twice(
        &w,
        vec![GameEvent::Tap { object: t2 }, life(2, 1)],
        false,
        &[],
    );
    assert_eq!(ok(&run).applications, vec![e(1), e(9)]);
    assert_eq!(run.state.life(1), 22);
    assert_eq!(run.state.life(2), 21);
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D1: CR 101.4, CR 616.1: within one chooser a
/// descendant takes the submission position of the event it replaces: [tap, gain 3] gives the
/// doubler to the tap's gain of 1 (2 + 3 = 25); [gain 3, tap] gives it to the gain of 3
/// (6 + 1 = 27).
#[test]
fn req_event_001_r6_d1_descendant_takes_parent_submission_position() {
    let mut w = World::new();
    let t = w.create(1, ZoneKind::Battlefield);
    w.add(tap_instead(1, t, vec![life(1, 1)]));
    w.add(doubler());
    let run = drive_twice(
        &w,
        vec![GameEvent::Tap { object: t }, life(1, 3)],
        false,
        &[],
    );
    assert_eq!(run.state.life(1), 25);
    assert_eq!(life_deltas(ok(&run)), vec![(p(1), 2), (p(1), 3)]);
    let run = drive_twice(
        &w,
        vec![life(1, 3), GameEvent::Tap { object: t }],
        false,
        &[],
    );
    assert_eq!(run.state.life(1), 27);
    assert_eq!(life_deltas(ok(&run)), vec![(p(1), 6), (p(1), 1)]);
}

/// REQ-EVENT-001, round 6 D1: CR 101.4, CR 616.1: without `Instead` the order is unchanged
/// (APNAP, then submission): P1's gain takes the one-use doubler although P2's was submitted
/// first. Passes before and after the round 6 change.
#[test]
fn req_event_001_r6_d1_submitted_only_order_unchanged() {
    let mut w = World::new();
    w.add(doubler());
    let run = drive_twice(&w, vec![life(2, 1), life(1, 1)], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(9)]);
    assert_eq!(run.state.life(1), 22);
    assert_eq!(run.state.life(2), 21);
}

/// REQ-EVENT-001, round 6 D1: CR 101.4, CR 616.1: the `CHOOSE_REPLACEMENT_EFFECT` request for
/// P1's descendant (of P1's tap) comes before the request for P2's submitted gain.
#[test]
fn req_event_001_r6_d1_descendant_choice_asked_in_key_order() {
    let mut w = World::new();
    let t = w.create(1, ZoneKind::Battlefield);
    w.add(tap_instead(1, t, vec![life(1, 1)]));
    w.add(add_one(3, BusEventKind::LifeChange));
    w.add(add_one(4, BusEventKind::LifeChange));
    let run = drive_twice(
        &w,
        vec![GameEvent::Tap { object: t }, life(2, 1)],
        false,
        &[0, 0],
    );
    assert_eq!(run.asked.len(), 2);
    assert_eq!(run.asked[0].chooser, p(1));
    assert_eq!(run.asked[1].chooser, p(2));
    assert_eq!(run.state.life(1), 23);
    assert_eq!(run.state.life(2), 23);
}

/// The round 6 D2 world: A (P2's) takes 1 damage, prevented by a shield-less effect whose
/// follow-up is `follow_up` (directly, or with `via_instead` through a `LifeChange` gain of 1
/// that a one-use effect replaces by `instead`), while X (P1's) is put into the graveyard by a
/// state-based action. Returns the world, X and Z (another permanent of P1).
fn d2_world(
    via_instead: bool,
    follow_up: impl Fn(ObjectId, ObjectId) -> Vec<GameEvent>,
    instead: impl Fn(ObjectId, ObjectId) -> Vec<GameEvent>,
) -> (World, ObjectId, ObjectId, Vec<GameEvent>) {
    let mut w = World::new();
    let a = w.create(2, ZoneKind::Battlefield);
    let x = w.create(1, ZoneKind::Battlefield);
    let z = w.create(1, ZoneKind::Battlefield);
    let then = if via_instead {
        w.add(
            ReplacementEffect::new(
                e(2),
                EventMatcher::new(BusEventKind::LifeChange),
                Action::Instead(instead(x, z)),
            )
            .with_shield(Shield::Uses(1)),
        );
        vec![life(1, 1)]
    } else {
        follow_up(x, z)
    };
    w.add(ReplacementEffect::new(
        e(1),
        EventMatcher {
            affected_object: Some(a),
            ..EventMatcher::new(BusEventKind::Damage)
        },
        Action::Prevent { then },
    ));
    let events = vec![
        damage_object(a, 1),
        zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
    ];
    (w, x, z, events)
}

/// Whether `report` executed an event of `kind`.
fn executed_kind(report: &BusReport, kind: BusEventKind) -> bool {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .any(|ev| BusEventKind::of(ev.instance.event()) == kind)
}

fn attach(attachment: ObjectId, host: ObjectId) -> GameEvent {
    GameEvent::Attach {
        attachment,
        target: crate::permanent::AttachTarget::Permanent(host),
    }
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D2: CR 101.3, CR 614.6, CR 400.7, CR 701.3b: a
/// follow-up `Attach` (directly, or through its `Instead`) onto X, which the executed batch of
/// this call already moved, is ignored; the rest of the follow-up batch (a life gain) is still
/// carried out, and the earlier batch is kept.
#[test]
fn req_event_001_r6_d2_follow_up_attach_onto_moved_host_is_ignored() {
    for via_instead in [false, true] {
        let (w, x, z, events) = d2_world(
            via_instead,
            |x, z| vec![attach(z, x), life(1, 1)],
            |x, z| vec![attach(z, x), life(1, 2)],
        );
        let card = w.card_of(x);
        let z_card = w.card_of(z);
        let run = drive_twice(&w, events, true, &[]);
        let report = ok(&run);
        assert_eq!(report.prevented, 1);
        assert_eq!(moves_of(report, x), 1);
        assert!(!executed_kind(report, BusEventKind::Attach));
        assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
        assert_eq!(run.state.zone_of_card(z_card), Some(ZoneId::Battlefield));
        assert_eq!(run.state.permanents.attached_to(z), None);
        assert_eq!(run.state.life(1), if via_instead { 22 } else { 21 });
    }
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D2: CR 101.3 does not cover an id that was never
/// issued: a follow-up `Attach` onto it (directly or through `Instead`) fails with
/// `UnknownObject` and the whole call is rolled back.
#[test]
fn req_event_001_r6_d2_attach_onto_never_issued_host_fails() {
    let ghost = ObjectId::new(999);
    for via_instead in [false, true] {
        let (w, _x, _z, events) = d2_world(
            via_instead,
            |_, z| vec![attach(z, ghost)],
            |_, z| vec![attach(z, ghost)],
        );
        let before = w.state_hash();
        let run = drive_twice(&w, events, true, &[]);
        assert_eq!(
            run.out,
            Err(BusError::Event(EventError::UnknownObject(ghost)))
        );
        assert_eq!(run.hash, before);
    }
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D2: CR 608.2h: a damage source is not a role the
/// impossible-instruction check covers: follow-up damage from X, which the executed batch
/// already moved, is still dealt in the follow-up batch (directly or through `Instead`).
/// Passes before and after the round 6 change.
#[test]
fn req_event_001_r6_d2_follow_up_damage_from_moved_source_is_dealt() {
    let from_x = |x: ObjectId| GameEvent::Damage {
        source: Some(x),
        source_controller: Some(p(1)),
        target: DamageTarget::Player(p(2)),
        amount: 1,
        combat: false,
    };
    for via_instead in [false, true] {
        let (w, x, _z, events) =
            d2_world(via_instead, |x, _| vec![from_x(x)], |x, _| vec![from_x(x)]);
        let run = drive_twice(&w, events, true, &[]);
        let report = ok(&run);
        assert_eq!(report.executed.len(), 2);
        assert!(
            report.executed[1]
                .events
                .iter()
                .any(|ev| *ev.instance.event() == from_x(x))
        );
        assert_eq!(run.state.life(2), 19);
        assert_eq!(run.state.life(1), 20);
    }
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D2: CR 101.3, CR 400.7 (CR 608.2h limitation, see
/// DECISIONS): a follow-up `BecomeCopy` of X, which the executed batch already moved, is
/// ignored (no rollback); the life gain of the same follow-up batch is carried out.
#[test]
fn req_event_001_r6_d2_follow_up_copy_of_moved_object_is_ignored() {
    let copy = |x: ObjectId, z: ObjectId| GameEvent::BecomeCopy {
        object: z,
        copied: x,
    };
    for via_instead in [false, true] {
        let (w, _x, z, events) = d2_world(
            via_instead,
            |x, z| vec![copy(x, z), life(1, 1)],
            |x, z| vec![copy(x, z), life(1, 2)],
        );
        let run = drive_twice(&w, events, true, &[]);
        let report = ok(&run);
        assert!(!executed_kind(report, BusEventKind::BecomeCopy));
        assert_eq!(run.state.events.markers().copy_of(z), None);
        assert_eq!(run.state.life(1), if via_instead { 22 } else { 21 });
    }
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 D2: CR 704.3: in ONE batch, an `Attach` of Z onto X
/// and X's state-based move both execute (the events that are not zone changes first), so the
/// attach happens while X is still there. Pinned: afterwards Z is attached to nothing (the
/// permanent container ends attachments to an object that leaves the battlefield); Z itself stays on the battlefield.
#[test]
fn req_event_001_r6_d2_same_batch_attach_before_host_moves() {
    let mut w = World::new();
    let x = w.create(1, ZoneKind::Battlefield);
    let z = w.create(1, ZoneKind::Battlefield);
    let card = w.card_of(x);
    let z_card = w.card_of(z);
    let events = vec![
        attach(z, x),
        zc(x, ZoneKind::Graveyard, ZoneChangeCause::StateBasedAction),
    ];
    let run = drive_twice(&w, events, true, &[]);
    let report = ok(&run);
    let kinds: Vec<BusEventKind> = report.executed[0]
        .events
        .iter()
        .map(|ev| BusEventKind::of(ev.instance.event()))
        .collect();
    assert_eq!(kinds, vec![BusEventKind::Attach, BusEventKind::ZoneChange]);
    assert_eq!(run.state.zone_of_card(card), Some(ZoneId::Graveyard(p(1))));
    assert_eq!(run.state.zone_of_card(z_card), Some(ZoneId::Battlefield));
    assert_eq!(run.state.permanents.attached_to(z), None);
}

/// REQ-EVENT-001, round 6 D6: CR 701.26a (only untapped permanents can be tapped), CR 614.7: a
/// tap of a tapped permanent would not happen, so it offers no effect: nothing is asked or
/// applied, the one-use effect keeps its use, nothing is executed and the state is unchanged.
/// The same tap of the untapped permanent is replaced.
#[test]
fn req_event_001_r6_d6_tap_of_tapped_permanent_offers_no_effect() {
    let mut w = World::new();
    let obj = w.create(1, ZoneKind::Battlefield);
    w.setup(GameEvent::Tap { object: obj });
    w.add(tap_instead(1, obj, vec![life(1, 1)]).with_shield(Shield::Uses(1)));
    let before = w.state_hash();
    let registry_before = encode_to_vec(&w.registry);
    let run = drive_twice(&w, vec![GameEvent::Tap { object: obj }], false, &[]);
    let report = ok(&run);
    assert!(run.asked.is_empty());
    assert!(report.applications.is_empty());
    assert!(report.executed.is_empty());
    assert_eq!(encode_to_vec(&run.state.registry), registry_before);
    assert_eq!(run.state.life(1), 20);
    assert_eq!(run.hash, before);

    // The untapped case: the effect applies.
    let mut w = World::new();
    let obj = w.create(1, ZoneKind::Battlefield);
    w.add(tap_instead(1, obj, vec![life(1, 1)]).with_shield(Shield::Uses(1)));
    let run = drive_twice(&w, vec![GameEvent::Tap { object: obj }], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1)]);
    assert_eq!(run.state.life(1), 21);
    assert!(!run.state.permanents.get(obj).unwrap().status().tapped);
}

/// REQ-EVENT-001, REQ-ARCH-002, round 6 (r4633092145): CR 101.4, CR 616.1, CR 614.6: a
/// grandchild takes the exact position of the event it replaces. P1's tap of T becomes
/// [tap U, gain 3]; the tap of U becomes [gain 1]. The gain of 1 replaces the tap of U at its
/// position before the gain of 3, so it takes the one-use doubler: 2 + 3 = 25. With P2's gain
/// of 1 submitted too, in either order, P1's lineage still goes first (P2 ends at 21). Control:
/// with the output [gain 3, tap U] the gain of 3 is first: 6 + 1 = 27.
#[test]
fn req_event_001_r6_lineage_path_grandchild_takes_parent_position() {
    let world = |tap_u_first: bool| {
        let mut w = World::new();
        let t = w.create(1, ZoneKind::Battlefield);
        let u = w.create(1, ZoneKind::Battlefield);
        let mut output = vec![GameEvent::Tap { object: u }, life(1, 3)];
        if !tap_u_first {
            output.reverse();
        }
        w.add(tap_instead(1, t, output));
        w.add(tap_instead(2, u, vec![life(1, 1)]));
        w.add(doubler());
        (w, t)
    };
    let (w, t) = world(true);
    let run = drive_twice(&w, vec![GameEvent::Tap { object: t }], false, &[]);
    assert_eq!(ok(&run).applications, vec![e(1), e(2), e(9)]);
    assert_eq!(run.state.life(1), 25);
    // Executed at the replaced events' positions, as the direct [gain 1, gain 3].
    assert_eq!(life_deltas(ok(&run)), vec![(p(1), 2), (p(1), 3)]);
    for events in [
        vec![GameEvent::Tap { object: t }, life(2, 1)],
        vec![life(2, 1), GameEvent::Tap { object: t }],
    ] {
        let run = drive_twice(&w, events, false, &[]);
        assert_eq!(run.state.life(1), 25);
        assert_eq!(run.state.life(2), 21);
    }
    let (w, t) = world(false);
    let run = drive_twice(&w, vec![GameEvent::Tap { object: t }], false, &[]);
    assert_eq!(run.state.life(1), 27);
}

/// The round 6 phase C world (CodeRabbit review 5980983595): P1 is dealt damage by A (P1's
/// permanent); pool e1 (1) prevents damage to P1; e9 doubles damage to P1 below 3, once.
/// T's tap becomes `output(u)`; U's tap becomes [1 damage from A to P1].
fn pool_lineage_world(output: impl Fn(ObjectId, ObjectId) -> Vec<GameEvent>) -> (World, ObjectId) {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let t = w.create(1, ZoneKind::Battlefield);
    let u = w.create(1, ZoneKind::Battlefield);
    pool_lineage_effects(&mut w);
    w.add(tap_instead(2, t, output(a, u)));
    w.add(tap_instead(3, u, vec![damage_from_to_player(a, 1, 1)]));
    (w, t)
}

/// Pool e1 (1) on damage to P1 and the one-use doubler e9 on damage to P1 below 3.
fn pool_lineage_effects(w: &mut World) {
    let to_p1 = EventMatcher {
        affected_player: Some(p(1)),
        ..EventMatcher::new(BusEventKind::Damage)
    };
    w.add(
        ReplacementEffect::new(e(1), to_p1.clone(), Action::Prevent { then: Vec::new() })
            .with_shield(Shield::Pool(1)),
    );
    w.add(
        ReplacementEffect::new(
            e(9),
            EventMatcher {
                amount_below: Some(3),
                ..to_p1
            },
            Action::Modify(Modification::Amount(AmountOp::Mul(2))),
        )
        .with_shield(Shield::Uses(1)),
    );
}

/// REQ-EVENT-001, round 6 (CodeRabbit review 5980983595): CR 615.7, CR 616.1, CR 101.4: stopped
/// pool members resume in lineage-path order, the order phase A stopped them in. T's tap
/// becomes [tap U, 3 damage]; U's tap becomes [1 damage]; P1 picks pool e1 (1) for the 1 damage:
/// it is prevented and the 3 damage is dealt (P1 17), as for the direct [1 damage, 3 damage].
/// With the output [3 damage, tap U] the result equals the direct [3 damage, 1 damage]: the
/// pool cuts the 3 to 2, e9 doubles it to 4, the 1 is dealt (P1 15).
#[test]
fn req_event_001_r6_pool_resumption_follows_lineage_path() {
    for tap_u_first in [true, false] {
        let (w, t) = pool_lineage_world(|a, u| {
            let mut out = vec![GameEvent::Tap { object: u }, damage_from_to_player(a, 1, 3)];
            if !tap_u_first {
                out.reverse();
            }
            out
        });
        let run = drive_twice(&w, vec![GameEvent::Tap { object: t }], false, &[0]);
        // Control: the same damage submitted directly in the corresponding order.
        let mut c = World::new();
        let a = c.create(1, ZoneKind::Battlefield);
        pool_lineage_effects(&mut c);
        let mut direct = vec![
            damage_from_to_player(a, 1, 1),
            damage_from_to_player(a, 1, 3),
        ];
        if !tap_u_first {
            direct.reverse();
        }
        let control = drive_twice(&c, direct, false, &[0]);
        let expected = if tap_u_first { 17 } else { 15 };
        assert_eq!(control.state.life(1), expected);
        assert_eq!(run.state.life(1), expected);
        assert_eq!(ok(&run).prevented, ok(&control).prevented);
    }
}

/// The objects of the executed zone changes of `report`, in execution order.
fn moved_objects(report: &BusReport) -> Vec<ObjectId> {
    report
        .executed
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::ZoneChange { object, .. } => Some(*object),
            _ => None,
        })
        .collect()
}

/// REQ-EVENT-001, round 6 (CodeRabbit review 5980983595, execution order): CR 614.6, CR 704.3:
/// in the executed batch a descendant takes the position of the event it replaces, by lineage
/// path. T's tap becomes [tap U, exile Y]; U's tap becomes [exile X]: X's move executes before
/// Y's, as in the direct batch [exile X, exile Y] (and after it with the output reversed).
#[test]
fn req_event_001_r6_execution_order_follows_lineage_path() {
    for tap_u_first in [true, false] {
        let mut w = World::new();
        let t = w.create(1, ZoneKind::Battlefield);
        let u = w.create(1, ZoneKind::Battlefield);
        let x = w.create(1, ZoneKind::Battlefield);
        let y = w.create(1, ZoneKind::Battlefield);
        let exile = |o| zc(o, ZoneKind::Exile, ZoneChangeCause::Other);
        let mut output = vec![GameEvent::Tap { object: u }, exile(y)];
        let mut direct = vec![exile(x), exile(y)];
        if !tap_u_first {
            output.reverse();
            direct.reverse();
        }
        let control = drive_twice(&w, direct, false, &[]);
        w.add(tap_instead(1, t, output));
        w.add(tap_instead(2, u, vec![exile(x)]));
        let run = drive_twice(&w, vec![GameEvent::Tap { object: t }], false, &[]);
        let expected = if tap_u_first { vec![x, y] } else { vec![y, x] };
        assert_eq!(moved_objects(ok(&control)), expected);
        assert_eq!(moved_objects(ok(&run)), expected);
    }
}
