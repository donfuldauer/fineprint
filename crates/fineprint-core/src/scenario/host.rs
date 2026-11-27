// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The reference scenario host over the real engine components (REQ-TEST-001).
//!
//! [`StateHost`] builds the exact starting state of a [`Setup`] from the existing
//! [`GameState`], [`Zones`], [`Permanents`] and [`EventState`] APIs, evaluates characteristics
//! through the layer system ([`crate::layers::evaluate`]) and drives scripted answers through
//! the choice protocol ([`ChoiceSession`]) and the priority heartbeat ([`run_heartbeat`], with
//! the real state-based-action pass, [`sba_pass_post_batch`]).
//!
//! # Supported subset
//!
//! - Setup: players (life, counters, designations), every zone with exact order, battlefield
//!   permanents (controller, tapped, flipped, phased out, counters, designations, attachments,
//!   relative timestamps), the stack, the turn number, active player, phase and step, and the
//!   PRNG seed. Every card must be loaded ([`StateHost::add_card`]) and its `definitionHash`
//!   must equal the scenario's (R11).
//! - Choices: `PRIORITY_ACTION`, answered with `PASS_PRIORITY`, through the heartbeat of the
//!   current step, with real state-based actions before each priority (the heartbeat's settle).
//!   The step ends when every player in the game passes with an empty stack; the host does not
//!   move to the next step.
//!
//! Everything else fails loudly with a message that starts `unsupported by the reference
//! host:` and names the item: face-down objects, battlefield objects with static, triggered or
//! replacement abilities (their continuous effects and triggers are not wired yet), layouts the
//! layout module does not support, priority held by a player other than the active player at
//! setup, resolving a stack object, taking any action (casting, activating, playing a land,
//! special actions: they land with the action and casting units), and every answer type but
//! `PASS_PRIORITY` and the field-less answers (`YES`, `NO`, ...).
//!
//! The format id, rules baseline and card-database version are carried as metadata; the host
//! does not select a `FormatConfig` from them.
//!
//! # Observed events
//!
//! Executed game events since the end of the build, named in upper snake case after the
//! `GameEvent` variant (`ZONE_CHANGE`, `LIFE_CHANGE`, `ADD_COUNTERS`, ...) with camelCase
//! fields (players and objects by scenario id, zones and causes in upper snake case), and three
//! host observations: `PLAYER_LOST {player}` for every loss in a state-based-action pass,
//! `PRIORITY_PASSED {player, result, to?}` for every pass (`result` is `PRIORITY_MOVED`,
//! `TOP_RESOLVED` or `STEP_ENDS`), and `STEP_ENDS {phase, step?}` when the heartbeat ends the
//! step.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_dsl::card::{AbilityAST, CardDefinitionDSL};
use fineprint_dsl::expr::ObjectSelectorAST;
use fineprint_dsl::vocab::RulesVocabulary;
use serde_json::{Value, json};

use super::model::{CardRef, DesignationSpec, ObjectSetup, PhaseName, Setup, StepName, ZoneName};
use super::observe::{
    Observed, ObservedCharacteristics, ObservedChoice, ObservedEvent, ObservedResult,
};
use super::run::ScenarioHost;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceResponse, ChoiceSession,
    Interrupt, PendingChoiceRequest, Status,
};
use crate::events::{
    CounterTarget, DamageTarget, EventContext, EventState, ExecutedBatch, GameEvent, Outcome,
    Phase, Step, ZoneChangeCause,
};
use crate::layers::{
    ContinuousEffects, LayerEnvironment, LayeredProvider, MatchContext, ObjectBase, StampReason,
    evaluate,
};
use crate::layouts::{LayoutQuery, LayoutStatus, base_characteristics};
use crate::memory::{LkiStore, MemoryTable};
use crate::objects::ManaValueContext;
use crate::permanent::{AttachTarget, Permanents};
use crate::replacement::{BusInterrupt, ReplacementRegistry};
use crate::sba::{LOST_THE_GAME, SbaEnvironment, SbaOutcome, SbaRegistry, StoredSbaEnvironment};
use crate::state::{DesignationValue, GameEnvironment, GameState, ObjectWorld};
use crate::triggers::TriggerPlacement;
use crate::turn::{
    HeartbeatHost, HeartbeatOutcome, HeartbeatState, PassResult, SbaStage, run_heartbeat,
    sba_pass_post_batch,
};
use crate::txn::Transactional;
use crate::zones::{MoveRequest, Position, ZoneId, ZoneKind, Zones, ZonesConfig};

/// The prefix of every message about something the reference host cannot do yet.
pub const UNSUPPORTED: &str = "unsupported by the reference host: ";

/// The starting life every player gets before the setup's life totals are applied.
const DEFAULT_LIFE: i64 = 20;

fn unsupported(what: impl fmt::Display) -> String {
    format!("{UNSUPPORTED}{what}")
}

// -------------------------------------------------------------------------------------------
// The world: every engine component the host drives
// -------------------------------------------------------------------------------------------

/// The engine components of one scenario run.
#[derive(Clone, Debug)]
struct World {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
    replacements: ReplacementRegistry,
    sba_env: StoredSbaEnvironment,
    hb: HeartbeatState,
    /// The card definitions by card id.
    defs: Arc<BTreeMap<CardId, CardDefinitionDSL>>,
    /// The scenario id of every player, by player id.
    player_names: Arc<BTreeMap<PlayerId, String>>,
    /// The scenario id of every card (an object's name follows its card across zones).
    card_names: Arc<BTreeMap<CardId, String>>,
    /// The cards declared in the shared exile.
    shared_exile: Arc<BTreeSet<CardId>>,
    /// Host observations and mapped executed events, in order.
    journal: Vec<ObservedEvent>,
}

/// The objects as the layer system and LKI read them.
struct Board<'a> {
    zones: &'a Zones,
    permanents: &'a Permanents,
    events: &'a EventState,
    defs: &'a BTreeMap<CardId, CardDefinitionDSL>,
    vocab: Option<&'static RulesVocabulary>,
}

impl<'a> Board<'a> {
    fn of(w: &'a World) -> Board<'a> {
        Board {
            zones: &w.zones,
            permanents: &w.permanents,
            events: &w.events,
            defs: &w.defs,
            vocab: RulesVocabulary::bundled().ok(),
        }
    }
}

impl ObjectWorld for Board<'_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.zones.all_object_ids()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        let record = self.zones.object(id)?;
        let def = self.defs.get(&record.card)?;
        let permanent = self.permanents.get(id);
        let status = LayoutStatus {
            flipped: permanent.is_some_and(|p| p.status().flipped),
            ..LayoutStatus::default()
        };
        let kind = record.zone.kind();
        let query = LayoutQuery::new(kind).with_status(status);
        let characteristics = base_characteristics(def, &query, self.vocab?).ok()?;
        let controller = match permanent {
            Some(p) => Some(p.controller()),
            None => record.rules_controller(),
        };
        Some(ObjectBase {
            characteristics,
            controller,
            on_battlefield: kind == ZoneKind::Battlefield,
        })
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.events.markers().counters(id)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.permanents.attachments_of(AttachTarget::Permanent(id))
    }

    /// No continuous effect is registered by the reference host, so no selector is asked.
    fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
        false
    }
}

fn interrupt(message: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(message))
}

impl World {
    fn player_name(&self, id: PlayerId) -> String {
        self.player_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#player{}", id.get()))
    }

    fn object_name_in(&self, zones: &Zones, id: ObjectId) -> Option<String> {
        let card = zones.object(id)?.card;
        self.card_names.get(&card).cloned()
    }

    fn object_name(&self, id: ObjectId, before: &Zones) -> String {
        self.object_name_in(&self.zones, id)
            .or_else(|| self.object_name_in(before, id))
            .unwrap_or_else(|| format!("#object{}", id.get()))
    }

    /// Runs `f` with an event context over this world (the LKI provider is the layer-evaluated
    /// view of a snapshot taken first, as `EventContext` documents).
    fn with_context<T>(&mut self, f: impl FnOnce(&mut EventContext<'_>) -> T) -> T {
        let snap = self.clone();
        let board = Board::of(&snap);
        let env = GameEnvironment::new(&snap.game, &board);
        let provider = LayeredProvider::new(snap.game.effects(), &env);
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
        f(&mut ctx)
    }

    /// Records the executed batches and the losses of one state-based-action pass.
    fn journal_pass(&mut self, outcome: &SbaOutcome, before: &Zones) {
        for batch in &outcome.executed {
            self.journal_batch(batch, before);
        }
        for &p in &outcome.lost_players {
            let player = self.player_name(p);
            self.journal.push(ObservedEvent {
                event_type: "PLAYER_LOST".to_owned(),
                fields: BTreeMap::from([("player".to_owned(), Value::String(player))]),
            });
        }
    }

    fn journal_batch(&mut self, batch: &ExecutedBatch, before: &Zones) {
        for e in &batch.events {
            let observed = self.map_event(e.instance.event(), &e.outcome, before);
            self.journal.push(observed);
        }
    }

    /// One executed event as an observed event.
    fn map_event(&self, event: &GameEvent, outcome: &Outcome, before: &Zones) -> ObservedEvent {
        let obj = |id: ObjectId| Value::String(self.object_name(id, before));
        let player = |id: PlayerId| Value::String(self.player_name(id));
        let attach = |t: AttachTarget| match t {
            AttachTarget::Permanent(o) => obj(o),
            AttachTarget::Player(p) => player(p),
        };
        let (name, fields): (&str, Value) = match event {
            GameEvent::ZoneChange {
                object,
                request,
                cause,
            } => {
                let (object, from) = match outcome {
                    Outcome::ZoneChange(m) => (obj(m.new), json!(zone_kind_name(m.from.kind()))),
                    _ => (obj(*object), Value::Null),
                };
                (
                    "ZONE_CHANGE",
                    json!({"object": object, "from": from, "to": zone_kind_name(request.to), "cause": cause_name(*cause)}),
                )
            }
            GameEvent::Draw { player: p } => ("DRAW", json!({"player": player(*p)})),
            GameEvent::Damage {
                source,
                target,
                amount,
                combat,
                ..
            } => {
                let target = match target {
                    DamageTarget::Player(p) => player(*p),
                    DamageTarget::Object(o) => obj(*o),
                };
                (
                    "DAMAGE",
                    json!({"source": source.map(obj), "target": target, "amount": amount, "combat": combat}),
                )
            }
            GameEvent::LifeChange { player: p, delta } => {
                ("LIFE_CHANGE", json!({"player": player(*p), "delta": delta}))
            }
            GameEvent::AddCounters {
                target,
                kind,
                amount,
            }
            | GameEvent::RemoveCounters {
                target,
                kind,
                amount,
            } => {
                let target = match target {
                    CounterTarget::Player(p) => player(*p),
                    CounterTarget::Object(o) => obj(*o),
                };
                let name = if matches!(event, GameEvent::AddCounters { .. }) {
                    "ADD_COUNTERS"
                } else {
                    "REMOVE_COUNTERS"
                };
                (
                    name,
                    json!({"target": target, "kind": kind, "amount": amount}),
                )
            }
            GameEvent::Tap { object } => ("TAP", json!({"object": obj(*object)})),
            GameEvent::Untap { object } => ("UNTAP", json!({"object": obj(*object)})),
            GameEvent::CreateToken {
                owner, controller, ..
            } => (
                "CREATE_TOKEN",
                json!({"owner": player(*owner), "controller": controller.map(player)}),
            ),
            GameEvent::TurnBegins { player: p } => ("TURN_BEGINS", json!({"player": player(*p)})),
            GameEvent::PhaseBegins { phase } => {
                ("PHASE_BEGINS", json!({"phase": phase_name(*phase)}))
            }
            GameEvent::StepBegins { step } => ("STEP_BEGINS", json!({"step": step_name(*step)})),
            GameEvent::Attach { attachment, target } => (
                "ATTACH",
                json!({"attachment": obj(*attachment), "target": attach(*target)}),
            ),
            GameEvent::Unattach { attachment } => {
                ("UNATTACH", json!({"attachment": obj(*attachment)}))
            }
            GameEvent::BecomeCopy { object, copied } => (
                "BECOME_COPY",
                json!({"object": obj(*object), "copied": obj(*copied)}),
            ),
            GameEvent::TurnFaceUp { object } => ("TURN_FACE_UP", json!({"object": obj(*object)})),
            GameEvent::TurnFaceDown { object } => {
                ("TURN_FACE_DOWN", json!({"object": obj(*object)}))
            }
            GameEvent::UnattachFrom { attachment, host } => (
                "UNATTACH_FROM",
                json!({"attachment": obj(*attachment), "host": attach(*host)}),
            ),
        };
        let fields = match fields {
            Value::Object(map) => map.into_iter().filter(|(_, v)| !v.is_null()).collect(),
            _ => BTreeMap::new(),
        };
        ObservedEvent {
            event_type: name.to_owned(),
            fields,
        }
    }
}

fn zone_kind_name(kind: ZoneKind) -> &'static str {
    match kind {
        ZoneKind::Library => "LIBRARY",
        ZoneKind::Hand => "HAND",
        ZoneKind::Battlefield => "BATTLEFIELD",
        ZoneKind::Graveyard => "GRAVEYARD",
        ZoneKind::Stack => "STACK",
        ZoneKind::Exile => "EXILE",
        ZoneKind::Command => "COMMAND",
    }
}

fn cause_name(cause: ZoneChangeCause) -> &'static str {
    match cause {
        ZoneChangeCause::Other => "OTHER",
        ZoneChangeCause::Destroy => "DESTROY",
        ZoneChangeCause::Sacrifice => "SACRIFICE",
        ZoneChangeCause::Discard => "DISCARD",
        ZoneChangeCause::Mill => "MILL",
        ZoneChangeCause::Exile => "EXILE",
        ZoneChangeCause::CounterSpell => "COUNTER_SPELL",
        ZoneChangeCause::StateBasedAction => "STATE_BASED_ACTION",
        ZoneChangeCause::StateBasedDestroy => "STATE_BASED_DESTROY",
    }
}

fn phase_of(name: PhaseName) -> Phase {
    match name {
        PhaseName::Beginning => Phase::Beginning,
        PhaseName::PrecombatMain => Phase::PrecombatMain,
        PhaseName::Combat => Phase::Combat,
        PhaseName::PostcombatMain => Phase::PostcombatMain,
        PhaseName::Ending => Phase::Ending,
    }
}

fn step_of(name: StepName) -> Option<Step> {
    Some(match name {
        StepName::Untap => Step::Untap,
        StepName::Upkeep => Step::Upkeep,
        StepName::Draw => Step::Draw,
        StepName::Main => return None,
        StepName::BeginningOfCombat => Step::BeginningOfCombat,
        StepName::DeclareAttackers => Step::DeclareAttackers,
        StepName::DeclareBlockers => Step::DeclareBlockers,
        StepName::FirstStrikeDamage => Step::FirstStrikeDamage,
        StepName::CombatDamage => Step::CombatDamage,
        StepName::EndOfCombat => Step::EndOfCombat,
        StepName::End => Step::End,
        StepName::Cleanup => Step::Cleanup,
    })
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Beginning => PhaseName::Beginning.as_str(),
        Phase::PrecombatMain => PhaseName::PrecombatMain.as_str(),
        Phase::Combat => PhaseName::Combat.as_str(),
        Phase::PostcombatMain => PhaseName::PostcombatMain.as_str(),
        Phase::Ending => PhaseName::Ending.as_str(),
    }
}

fn step_name(step: Step) -> &'static str {
    match step {
        Step::Untap => StepName::Untap.as_str(),
        Step::Upkeep => StepName::Upkeep.as_str(),
        Step::Draw => StepName::Draw.as_str(),
        Step::BeginningOfCombat => StepName::BeginningOfCombat.as_str(),
        Step::DeclareAttackers => StepName::DeclareAttackers.as_str(),
        Step::DeclareBlockers => StepName::DeclareBlockers.as_str(),
        Step::FirstStrikeDamage => StepName::FirstStrikeDamage.as_str(),
        Step::CombatDamage => StepName::CombatDamage.as_str(),
        Step::EndOfCombat => StepName::EndOfCombat.as_str(),
        Step::End => StepName::End.as_str(),
        Step::Cleanup => StepName::Cleanup.as_str(),
    }
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let mut out = Vec::new();
        self.game.state_hash().encode_canonical(&mut out);
        self.zones.encode_canonical(&mut out);
        self.permanents.encode_canonical(&mut out);
        self.events.encode_canonical(&mut out);
        for p in self.hb.passes() {
            p.sequence.encode_canonical(&mut out);
            p.player.encode_canonical(&mut out);
            let tag: u8 = match p.result {
                PassResult::PriorityMoved { .. } => 0,
                PassResult::TopResolved => 1,
                PassResult::StepEnds => 2,
            };
            tag.encode_canonical(&mut out);
        }
        sha256(&out)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.game.turn().priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

impl HeartbeatHost for World {
    fn game(&self) -> &GameState {
        &self.game
    }

    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        (self.events.phase(), self.events.step())
    }

    /// One real state-based-action pass through [`sba_pass_post_batch`].
    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        let snap = self.clone();
        let board = Board::of(&snap);
        let env = GameEnvironment::new(&snap.game, &board);
        let provider = LayeredProvider::new(snap.game.effects(), &env);
        let registry = SbaRegistry::standard();
        let effects = snap.game.effects().clone();
        let defs = Arc::clone(&self.defs);
        let view_env = self.sba_env.clone();
        let mut post = |ctx: &EventContext<'_>,
                        view: &mut dyn FnMut(
            &ContinuousEffects,
            &dyn LayerEnvironment,
            &dyn SbaEnvironment,
        )| {
            let after = Board {
                zones: ctx.zones,
                permanents: ctx.permanents,
                events: ctx.events,
                defs: &defs,
                vocab: RulesVocabulary::bundled().ok(),
            };
            let env = GameEnvironment::new(ctx.game, &after);
            view(ctx.game.effects(), &env, &view_env);
        };
        let out = {
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
            sba_pass_post_batch(
                SbaStage {
                    registry: &registry,
                    ctx: &mut ctx,
                    replacements: &mut self.replacements,
                    effects: &effects,
                    layer_env: &env,
                    env: &mut self.sba_env,
                    post_batch: &mut post,
                },
                choices,
            )
        };
        if let Ok(outcome) = &out {
            self.journal_pass(outcome, &snap.zones);
        }
        out
    }

    /// The reference host has no waiting-trigger queue: battlefield objects with triggered
    /// abilities are refused at build, so nothing ever waits.
    fn place_triggers(
        &mut self,
        _apnap: &[PlayerId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        Ok(TriggerPlacement::default())
    }

    fn stack_is_empty(&self) -> bool {
        self.zones.is_empty(ZoneId::Stack)
    }

    fn resolve_top(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        Err(interrupt(unsupported(
            "resolving the top object of the stack (lands with the resolution unit)",
        )))
    }

    fn priority_actions(&self, _player: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }

    fn take_action(
        &mut self,
        _player: PlayerId,
        action: &ChoiceResponse,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Err(interrupt(unsupported(format!(
            "the priority action {}",
            action.wire_name()
        ))))
    }
}

// -------------------------------------------------------------------------------------------
// The host
// -------------------------------------------------------------------------------------------

/// The heartbeat procedure of a session.
type Procedure =
    Box<dyn Fn(&mut World, &mut ChoiceContext<'_>) -> Result<HeartbeatOutcome, Interrupt>>;

/// A choice session over the heartbeat.
type Session = ChoiceSession<World, HeartbeatOutcome, Procedure>;

/// One built scenario.
struct Run {
    /// The state right after the build.
    built: World,
    /// The heartbeat session, when a player holds priority at setup.
    session: Option<Session>,
    /// The world as it was when the session last suspended (the live state while a choice is
    /// pending; the session itself shows its base state then).
    tap: Rc<RefCell<Option<World>>>,
}

/// The reference [`ScenarioHost`] over the real engine components (REQ-TEST-001). See the
/// module documentation for the supported subset.
#[derive(Default)]
pub struct StateHost {
    cards: BTreeMap<String, CardDefinitionDSL>,
    run: Option<Run>,
}

impl fmt::Debug for StateHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateHost")
            .field("cards", &self.cards.keys().collect::<Vec<_>>())
            .field("built", &self.run.is_some())
            .finish()
    }
}

impl StateHost {
    /// A host with no card definitions loaded.
    #[must_use]
    pub fn new() -> StateHost {
        StateHost::default()
    }

    /// Loads one card definition (R11: only definitions whose hash verifies).
    ///
    /// # Errors
    /// A message if the definition's hash does not verify or a definition with the same
    /// cardDefId is already loaded.
    pub fn add_card(&mut self, def: CardDefinitionDSL) -> Result<(), String> {
        def.verify_hash()
            .map_err(|e| format!("card {}: {e}", super::error::quote(&def.card_def_id)))?;
        if self.cards.contains_key(&def.card_def_id) {
            return Err(format!(
                "card {} is loaded twice",
                super::error::quote(&def.card_def_id)
            ));
        }
        self.cards.insert(def.card_def_id.clone(), def);
        Ok(())
    }

    /// Parses (strictly, [`CardDefinitionDSL::from_json`]) and loads one card definition;
    /// returns its cardDefId.
    ///
    /// # Errors
    /// As [`CardDefinitionDSL::from_json`] and [`StateHost::add_card`].
    pub fn add_card_json(&mut self, bytes: &[u8]) -> Result<String, String> {
        let def = CardDefinitionDSL::from_json(bytes).map_err(|e| e.to_string())?;
        let id = def.card_def_id.clone();
        self.add_card(def)?;
        Ok(id)
    }

    /// The cardDefIds loaded, ascending.
    #[must_use]
    pub fn card_ids(&self) -> Vec<String> {
        self.cards.keys().cloned().collect()
    }

    /// The hash of the current state (the live state while a choice is pending), or `None`
    /// before a successful build (REQ-TEST-001, R3: the same scenario gives the same hash).
    #[must_use]
    pub fn state_hash(&self) -> Option<Digest> {
        self.current().map(|w| w.state_hash())
    }

    /// The current world: the live state of the session, or the built state.
    fn current(&self) -> Option<World> {
        let run = self.run.as_ref()?;
        match &run.session {
            None => Some(run.built.clone()),
            Some(s) => match s.status() {
                Status::Done { state, .. } => Some(state.clone()),
                Status::Pending(_) => {
                    Some(run.tap.borrow().clone().unwrap_or_else(|| s.base().clone()))
                }
            },
        }
    }

    /// Builds the world of `setup` (no session yet).
    fn build_world(&self, setup: &Setup) -> Result<World, String> {
        RulesVocabulary::bundled().map_err(|e| format!("rules vocabulary: {e}"))?;
        // R11: every required definition is loaded and has the required hash.
        for (id, hash) in &setup.card_database.definition_hashes {
            let Some(def) = self.cards.get(id) else {
                return Err(format!(
                    "card {} required by cardDatabase.definitionHashes is not loaded",
                    super::error::quote(id)
                ));
            };
            if def.definition_hash != *hash {
                return Err(format!(
                    "card {}: the loaded definitionHash {} differs from the required {}",
                    super::error::quote(id),
                    def.definition_hash,
                    hash
                ));
            }
        }
        let objects = setup.objects();
        // Every id (players and objects together) is declared once: the typed `Setup` is a
        // public input, so the host checks it as the parser does (R12).
        let mut declared: BTreeSet<&str> = BTreeSet::new();
        for id in setup
            .players
            .iter()
            .map(|p| p.id.as_str())
            .chain(objects.iter().map(|o| o.id.as_str()))
        {
            if !declared.insert(id) {
                return Err(format!(
                    "id {} is declared more than once",
                    super::error::quote(id)
                ));
            }
        }
        for o in setup.battlefield.iter().chain(&setup.shared_exile) {
            if o.face_down {
                return Err(unsupported(format!(
                    "the face-down object {}",
                    super::error::quote(&o.id)
                )));
            }
        }
        // A shared-exile object is no permanent: its permanent status fields would be dropped,
        // so any non-default value is refused rather than silently ignored.
        for o in &setup.shared_exile {
            if let Some(field) = shared_exile_field(o) {
                return Err(unsupported(format!(
                    "{field} on the shared-exile object {}",
                    super::error::quote(&o.id)
                )));
            }
        }
        for o in &objects {
            // R11: every card the setup uses has a required definitionHash (checked above).
            if !setup.card_database.definition_hashes.contains_key(&o.card) {
                return Err(format!(
                    "card {} used by object {} has no entry in cardDatabase.definitionHashes",
                    super::error::quote(&o.card),
                    super::error::quote(&o.id)
                ));
            }
            let Some(def) = self.cards.get(&o.card) else {
                return Err(format!(
                    "card {} is not loaded",
                    super::error::quote(&o.card)
                ));
            };
            if o.zone == ZoneName::Battlefield {
                for face in &def.faces {
                    for a in &face.abilities {
                        if matches!(
                            a,
                            AbilityAST::Static(_)
                                | AbilityAST::Triggered(_)
                                | AbilityAST::Replacement(_)
                        ) {
                            return Err(unsupported(format!(
                                "the {} ability of battlefield object {} (its effects are not wired yet)",
                                a.kind(),
                                super::error::quote(&o.id)
                            )));
                        }
                    }
                }
            }
        }
        let count = setup.players.len();
        let mut game = GameState::new(setup.prng_seed, count, DEFAULT_LIFE)
            .map_err(|e| format!("game state: {e}"))?;
        let order = game.turn_order().to_vec();
        let mut player_ids: BTreeMap<String, PlayerId> = BTreeMap::new();
        let mut player_names = BTreeMap::new();
        for (p, &id) in setup.players.iter().zip(&order) {
            player_ids.insert(p.id.clone(), id);
            player_names.insert(id, p.id.clone());
            let mut pm = game
                .player_mut(id)
                .ok_or_else(|| format!("player {} missing", p.id))?;
            pm.set_life(p.life);
            for (kind, &amount) in &p.counters {
                pm.set_counter(kind, amount);
            }
            for (key, value) in &p.designations {
                if let Some(v) = designation(value) {
                    pm.set_designation(key, v);
                }
            }
        }
        let pid = |name: &str| -> Result<PlayerId, String> {
            player_ids
                .get(name)
                .copied()
                .ok_or_else(|| format!("player {} is not declared", super::error::quote(name)))
        };
        game.set_turn_number(setup.turn.number);
        game.set_active_player(Some(pid(&setup.turn.active_player)?))
            .map_err(|e| format!("active player: {e}"))?;
        let mut zones = Zones::new(&order, ZonesConfig::default());
        let mut ids = IdGenerator::new();
        let mut permanents = Permanents::new();
        let mut defs = BTreeMap::new();
        let mut card_names = BTreeMap::new();
        let mut shared = BTreeSet::new();
        let mut by_name: BTreeMap<String, ObjectId> = BTreeMap::new();
        let mut create = |game: &mut GameState,
                          zones: &mut Zones,
                          ids: &mut IdGenerator<ObjectId>,
                          name: &str,
                          card: &str,
                          owner: PlayerId,
                          req: MoveRequest|
         -> Result<(ObjectId, CardId), String> {
            let card_id = game.issue_card_id().map_err(|e| format!("card id: {e}"))?;
            let def = self
                .cards
                .get(card)
                .ok_or_else(|| format!("card {} is not loaded", super::error::quote(card)))?;
            defs.insert(card_id, def.clone());
            card_names.insert(card_id, name.to_owned());
            let id = zones
                .create_object(ids, card_id, owner, req)
                .map_err(|e| format!("object {}: {e}", super::error::quote(name)))?;
            Ok((id, card_id))
        };
        // Player zones, in declaration order. Library: top first, so each goes to the bottom.
        // Graveyard: bottom first, so each goes on top. The other player zones are unordered.
        for p in &setup.players {
            let owner = pid(&p.id)?;
            for (zone, list) in p.zones.lists() {
                for (i, r) in list.iter().enumerate() {
                    let name = r
                        .id()
                        .map_or_else(|| format!("{}.{}.{i}", p.id, zone.as_str()), str::to_owned);
                    let (kind, position) = match zone {
                        ZoneName::Library => (ZoneKind::Library, Position::Bottom),
                        ZoneName::Graveyard => (ZoneKind::Graveyard, Position::Top),
                        ZoneName::Hand => (ZoneKind::Hand, Position::Top),
                        ZoneName::Exile => (ZoneKind::Exile, Position::Top),
                        _ => (ZoneKind::Command, Position::Top),
                    };
                    let req = MoveRequest {
                        position,
                        ..MoveRequest::new(kind)
                    };
                    let card = match r {
                        CardRef::Card(c) | CardRef::Named { card: c, .. } => c,
                    };
                    let (id, _) = create(&mut game, &mut zones, &mut ids, &name, card, owner, req)?;
                    by_name.insert(name, id);
                }
            }
        }
        let battlefield_turn = |o: &ObjectSetup| o.controlled_since_turn.unwrap_or(0);
        for o in &setup.battlefield {
            let owner = pid(&o.owner)?;
            let controller = match &o.controller {
                Some(c) => pid(c)?,
                None => owner,
            };
            let req = MoveRequest {
                controller: Some(controller),
                ..MoveRequest::new(ZoneKind::Battlefield)
            };
            let (id, _) = create(&mut game, &mut zones, &mut ids, &o.id, &o.card, owner, req)?;
            by_name.insert(o.id.clone(), id);
            // A new permanent (default status), then the setup's status.
            permanents
                .enter(id, controller, battlefield_turn(o))
                .map_err(|e| format!("permanent {}: {e}", o.id))?;
            let status = |r: Result<(), crate::permanent::PermanentError>| {
                r.map_err(|e| format!("permanent {}: {e}", o.id))
            };
            status(permanents.set_tapped(id, o.tapped))?;
            status(permanents.set_flipped(id, o.flipped))?;
            status(permanents.set_phased_out(id, o.phased_out))?;
            for (key, value) in &o.designations {
                if let Some(v) = designation(value) {
                    status(permanents.set_designation(id, key, v))?;
                }
            }
        }
        // The stack: bottom first, so each goes on top.
        for o in &setup.stack {
            let owner = pid(&o.owner)?;
            let controller = match &o.controller {
                Some(c) => pid(c)?,
                None => owner,
            };
            let req = MoveRequest {
                controller: Some(controller),
                ..MoveRequest::new(ZoneKind::Stack)
            };
            let (id, _) = create(&mut game, &mut zones, &mut ids, &o.id, &o.card, owner, req)?;
            by_name.insert(o.id.clone(), id);
        }
        for o in &setup.shared_exile {
            let owner = pid(&o.owner)?;
            let (id, card) = create(
                &mut game,
                &mut zones,
                &mut ids,
                &o.id,
                &o.card,
                owner,
                MoveRequest::new(ZoneKind::Exile),
            )?;
            shared.insert(card);
            by_name.insert(o.id.clone(), id);
        }
        let mut world = World {
            game,
            zones,
            ids,
            permanents,
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            replacements: ReplacementRegistry::new(),
            sba_env: StoredSbaEnvironment::default(),
            hb: HeartbeatState::new(),
            defs: Arc::new(defs),
            player_names: Arc::new(player_names),
            card_names: Arc::new(card_names),
            shared_exile: Arc::new(shared),
            journal: Vec::new(),
        };
        // Every object has a base (its layout is supported).
        {
            let board = Board::of(&world);
            for (name, &id) in &by_name {
                if board.base(id).is_none() {
                    return Err(unsupported(format!(
                        "the layout of object {} in its zone",
                        super::error::quote(name)
                    )));
                }
            }
        }
        // Timestamps (entering a zone): every object outside the battlefield in declaration order,
        // then the battlefield in ascending setup timestamp, ties in declaration order.
        for o in &objects {
            if o.zone != ZoneName::Battlefield
                && let Some(&id) = by_name.get(&o.id)
            {
                world
                    .game
                    .stamp_object(id, StampReason::EntersZone)
                    .map_err(|e| format!("timestamp: {e}"))?;
            }
        }
        let mut stamped: Vec<(u64, usize, ObjectId)> = Vec::new();
        for (i, o) in setup.battlefield.iter().enumerate() {
            if let Some(&id) = by_name.get(&o.id) {
                stamped.push((o.timestamp.unwrap_or(0), i, id));
            }
        }
        stamped.sort_unstable();
        for &(_, _, id) in &stamped {
            world
                .game
                .stamp_object(id, StampReason::EntersZone)
                .map_err(|e| format!("timestamp: {e}"))?;
        }
        // Phase and step, counters on objects and attachments, through the events that own
        // them.
        let mut events: Vec<GameEvent> = Vec::new();
        events.push(match step_of(setup.turn.step) {
            Some(step) => GameEvent::StepBegins { step },
            None => GameEvent::PhaseBegins {
                phase: phase_of(setup.turn.phase),
            },
        });
        let mut counted: Vec<(String, &BTreeMap<String, i64>)> = Vec::new();
        for o in setup.battlefield.iter().chain(&setup.shared_exile) {
            counted.push((o.id.clone(), &o.counters));
        }
        for (name, counters) in counted {
            if let Some(&id) = by_name.get(&name) {
                for (kind, &amount) in counters {
                    events.push(GameEvent::AddCounters {
                        target: CounterTarget::Object(id),
                        kind: kind.clone(),
                        amount,
                    });
                }
            }
        }
        for o in &setup.battlefield {
            let (Some(a), Some(&id)) = (&o.attached_to, by_name.get(&o.id)) else {
                continue;
            };
            let target = match (player_ids.get(a), by_name.get(a)) {
                (Some(&p), _) => AttachTarget::Player(p),
                (None, Some(&t)) => AttachTarget::Permanent(t),
                (None, None) => {
                    return Err(format!(
                        "attachedTo {} is not declared",
                        super::error::quote(a)
                    ));
                }
            };
            events.push(GameEvent::Attach {
                attachment: id,
                target,
            });
        }
        for e in events {
            world
                .with_context(|ctx| ctx.execute(crate::events::EventInstance::new(e)))
                .map_err(|e| format!("setup event: {e}"))?;
        }
        // The attachments above issued fresh timestamps (CR 701.3c). The setup's `timestamp` is
        // the authored order of the battlefield, so every battlefield object is stamped again,
        // in the same order as the first pass: each ends later than every setup event and in
        // exactly the authored relative order (docs/DECISIONS.md "Gate 06-turn F35").
        for &(_, _, id) in &stamped {
            world
                .game
                .stamp_object(id, StampReason::EntersZone)
                .map_err(|e| format!("timestamp: {e}"))?;
        }
        if let Some(p) = &setup.priority
            && *p != setup.turn.active_player
        {
            return Err(unsupported(format!(
                "priority held by {} rather than the active player at setup",
                super::error::quote(p)
            )));
        }
        Ok(world)
    }

    /// The observed state of `w`, with the session's outcome when done.
    fn observe_world(&self, w: &World, done: Option<HeartbeatOutcome>) -> Observed {
        let mut o = Observed::default();
        let order = w.game.turn_order().to_vec();
        for &p in &order {
            let name = w.player_name(p);
            if let Some(ps) = w.game.player(p) {
                o.life.insert(name.clone(), ps.life());
                o.counters.insert(name.clone(), ps.counters().clone());
                o.designations.insert(
                    name.clone(),
                    ps.designations()
                        .iter()
                        .map(|(k, v)| (k.clone(), designation_json(v)))
                        .collect(),
                );
            }
        }
        // Zones: per-player lists (library top first, graveyard bottom first).
        for &p in &order {
            let pname = w.player_name(p);
            for (zone, id, reverse) in [
                (ZoneName::Library, ZoneId::Library(p), false),
                (ZoneName::Hand, ZoneId::Hand(p), false),
                (ZoneName::Graveyard, ZoneId::Graveyard(p), true),
            ] {
                let mut list = w.zones.contents(id);
                if reverse {
                    list.reverse();
                }
                let names = list.into_iter().map(|id| w.object_name(id, &w.zones));
                o.zones
                    .entry(zone.key(Some(&pname)))
                    .or_default()
                    .extend(names);
            }
        }
        let shared_zones: [(ZoneId, bool); 4] = [
            (ZoneId::Battlefield, false),
            (ZoneId::Stack, true),
            (ZoneId::Exile, false),
            (ZoneId::Command, false),
        ];
        for (zid, reverse) in shared_zones {
            let mut list = w.zones.contents(zid);
            if reverse {
                list.reverse();
            }
            for id in list {
                let Some(rec) = w.zones.object(id) else {
                    continue;
                };
                let name = w.object_name(id, &w.zones);
                let owner = w.player_name(rec.owner);
                let zone = match zid {
                    ZoneId::Battlefield => ZoneName::Battlefield,
                    ZoneId::Stack => ZoneName::Stack,
                    ZoneId::Command => ZoneName::Command,
                    _ if w.shared_exile.contains(&rec.card) => ZoneName::SharedExile,
                    _ => ZoneName::Exile,
                };
                // Exile and the command zone are shared in the engine but listed per player in
                // a scenario; the command zone is also given as a whole.
                if zone.is_per_player() {
                    o.zones
                        .entry(zone.key(Some(&owner)))
                        .or_default()
                        .push(name.clone());
                }
                if !matches!(zone, ZoneName::Exile) {
                    o.zones.entry(zone.key(None)).or_default().push(name);
                }
            }
        }
        for key in ["battlefield", "stack"] {
            o.zones.entry(key.to_owned()).or_default();
        }
        for &p in &order {
            let pname = w.player_name(p);
            for zone in [ZoneName::Exile, ZoneName::Command] {
                o.zones.entry(zone.key(Some(&pname))).or_default();
            }
        }
        // Objects: characteristics, counters, designations.
        let board = Board::of(w);
        let env = GameEnvironment::new(&w.game, &board);
        for id in w.zones.all_object_ids() {
            let name = w.object_name(id, &w.zones);
            let counters = w.events.markers().counters(id);
            if !counters.is_empty() {
                o.counters.insert(name.clone(), counters);
            }
            if let Some(p) = w.permanents.get(id)
                && !p.designations().is_empty()
            {
                o.designations.insert(
                    name.clone(),
                    p.designations()
                        .iter()
                        .map(|(k, v)| (k.clone(), designation_json(v)))
                        .collect(),
                );
            }
            if let Ok(e) = evaluate(w.game.effects(), &env, id) {
                o.characteristics.insert(
                    name,
                    characteristics(w, &e, w.zones.object(id).map(|r| r.zone.kind())),
                );
            }
        }
        // Events: the journal, the passes, the step end.
        o.events = w.journal.clone();
        for p in w.hb.passes() {
            let mut fields = BTreeMap::from([
                ("player".to_owned(), Value::String(w.player_name(p.player))),
                (
                    "result".to_owned(),
                    Value::String(
                        match p.result {
                            PassResult::PriorityMoved { .. } => "PRIORITY_MOVED",
                            PassResult::TopResolved => "TOP_RESOLVED",
                            PassResult::StepEnds => "STEP_ENDS",
                        }
                        .to_owned(),
                    ),
                ),
            ]);
            if let PassResult::PriorityMoved { to } = p.result {
                fields.insert("to".to_owned(), Value::String(w.player_name(to)));
            }
            o.events.push(ObservedEvent {
                event_type: "PRIORITY_PASSED".to_owned(),
                fields,
            });
        }
        if done == Some(HeartbeatOutcome::StepEnds) {
            let mut fields = BTreeMap::new();
            if let Some(ph) = w.events.phase() {
                fields.insert("phase".to_owned(), Value::String(phase_name(ph).to_owned()));
            }
            if let Some(st) = w.events.step() {
                fields.insert("step".to_owned(), Value::String(step_name(st).to_owned()));
            }
            o.events.push(ObservedEvent {
                event_type: "STEP_ENDS".to_owned(),
                fields,
            });
        }
        o.pending = self.pending();
        o.result = result(w, &order);
        o
    }
}

/// The first shared-exile setup field with a non-default value, if any (camelCase, as in the
/// scenario format). These fields describe a permanent or an attachment; an exiled object has
/// neither, so the host refuses them instead of dropping them.
fn shared_exile_field(o: &ObjectSetup) -> Option<&'static str> {
    if o.tapped {
        Some("tapped")
    } else if o.flipped {
        Some("flipped")
    } else if o.phased_out {
        Some("phasedOut")
    } else if o.designations.values().any(|v| designation(v).is_some()) {
        Some("designations")
    } else if o.controller.as_ref().is_some_and(|c| *c != o.owner) {
        Some("controller")
    } else if o.attached_to.is_some() {
        Some("attachedTo")
    } else if o.timestamp.is_some() {
        Some("timestamp")
    } else if o.controlled_since_turn.is_some() {
        Some("controlledSinceTurn")
    } else {
        None
    }
}

fn designation(value: &DesignationSpec) -> Option<DesignationValue> {
    match value {
        DesignationSpec::Bool(false) => None,
        DesignationSpec::Bool(true) => Some(DesignationValue::Flag),
        DesignationSpec::Int(i) => Some(DesignationValue::Int(*i)),
        DesignationSpec::Text(s) => Some(DesignationValue::Text(s.clone())),
    }
}

fn designation_json(value: &DesignationValue) -> Value {
    match value {
        DesignationValue::Flag => Value::Bool(true),
        DesignationValue::Int(i) => Value::from(*i),
        DesignationValue::Text(s) => Value::String(s.clone()),
    }
}

/// The observed characteristics of an evaluated object.
fn characteristics(
    w: &World,
    e: &crate::layers::Evaluated,
    zone: Option<ZoneKind>,
) -> ObservedCharacteristics {
    let c = &e.characteristics;
    let on_battlefield = zone == Some(ZoneKind::Battlefield);
    let pt = c.pt_applies(on_battlefield);
    let value = |v: &Option<fineprint_dsl::card::ValueExpressionAST>| -> Option<i64> {
        if !pt {
            return None;
        }
        Some(v.as_ref().and_then(crate::layers::literal_of).unwrap_or(0))
    };
    let bits = c.colors.bits();
    let colors = [(1u8, "W"), (2, "U"), (4, "B"), (8, "R"), (16, "G")]
        .iter()
        .filter(|(b, _)| bits & b != 0)
        .map(|(_, n)| (*n).to_owned())
        .collect();
    let mut abilities: Vec<String> = c
        .abilities
        .iter()
        .map(|a| match a {
            AbilityAST::Keyword(k) => crate::layers::keyword_id(&k.keyword),
            other => other.kind().to_owned(),
        })
        .collect();
    abilities.sort();
    abilities.dedup();
    let ctx = if zone == Some(ZoneKind::Stack) {
        ManaValueContext::OnStack(Default::default())
    } else {
        ManaValueContext::OffStack
    };
    ObservedCharacteristics {
        power: value(&c.power),
        toughness: value(&c.toughness),
        types: c.card_types.iter().cloned().collect(),
        subtypes: c.subtypes.iter().cloned().collect(),
        supertypes: c.supertypes.iter().cloned().collect(),
        colors,
        abilities,
        controller: e.controller.map(|p| w.player_name(p)),
        mana_value: Some(e.mana_value(ctx)),
    }
}

/// The game result: the players with the lost-the-game designation lost; one player left
/// wins; none left is a draw (see docs/DECISIONS.md "#51 FP-049 — Reference host result").
fn result(w: &World, order: &[PlayerId]) -> ObservedResult {
    let lost = |p: PlayerId| {
        w.game
            .player(p)
            .is_none_or(|s| s.designation(LOST_THE_GAME).is_some())
    };
    let losers: Vec<String> = order
        .iter()
        .filter(|&&p| lost(p))
        .map(|&p| w.player_name(p))
        .collect();
    let remaining: Vec<String> = order
        .iter()
        .filter(|&&p| !lost(p))
        .map(|&p| w.player_name(p))
        .collect();
    let status = if losers.is_empty() {
        "IN_PROGRESS"
    } else if remaining.len() == 1 {
        "WON"
    } else if remaining.is_empty() {
        "DRAWN"
    } else {
        "IN_PROGRESS"
    };
    let winners = if status == "WON" {
        remaining
    } else {
        Vec::new()
    };
    ObservedResult {
        status: status.to_owned(),
        winners,
        losers,
    }
}

/// The options of a pending request, as names.
fn options_of(r: &PendingChoiceRequest) -> Vec<String> {
    match (&r.payload, &r.options) {
        (
            ChoicePayload::PriorityAction {
                can_pass, actions, ..
            },
            _,
        ) => {
            let mut out = Vec::new();
            if *can_pass {
                out.push("PASS_PRIORITY".to_owned());
            }
            out.extend(actions.iter().map(|a| a.wire_name().to_owned()));
            out
        }
        (_, ChoiceOptions::Enumerated(list)) => list.iter().map(|o| format!("{o:?}")).collect(),
        _ => Vec::new(),
    }
}

/// A field-less answer of the given type.
fn simple_response(t: &str) -> Option<ChoiceResponse> {
    Some(match t {
        "PASS_PRIORITY" => ChoiceResponse::PassPriority,
        "YES" => ChoiceResponse::Yes,
        "NO" => ChoiceResponse::No,
        "DECLINE_OPTIONAL_PAYMENT" => ChoiceResponse::DeclineOptionalPayment,
        "CANCEL_AND_ROLLBACK" => ChoiceResponse::CancelAndRollback,
        "ACCEPT" => ChoiceResponse::Accept,
        "TAKE_MULLIGAN" => ChoiceResponse::TakeMulligan,
        _ => return None,
    })
}

impl ScenarioHost for StateHost {
    fn build(&mut self, setup: &Setup) -> Result<(), String> {
        self.run = None;
        let built = self.build_world(setup)?;
        let tap: Rc<RefCell<Option<World>>> = Rc::new(RefCell::new(None));
        let session = if setup.priority.is_some() {
            let cell = Rc::clone(&tap);
            let procedure: Procedure =
                Box::new(move |w: &mut World, ch: &mut ChoiceContext<'_>| {
                    let mut hb = std::mem::take(&mut w.hb);
                    let r = run_heartbeat(w, &mut hb, ch);
                    w.hb = hb;
                    if let Err(Interrupt::Pending(_)) = &r {
                        *cell.borrow_mut() = Some(w.clone());
                    }
                    r
                });
            Some(ChoiceSession::start(built.clone(), procedure).map_err(|e| e.to_string())?)
        } else {
            None
        };
        self.run = Some(Run {
            built,
            session,
            tap,
        });
        Ok(())
    }

    fn pending(&self) -> Option<ObservedChoice> {
        let run = self.run.as_ref()?;
        let r = run.session.as_ref()?.pending()?;
        let w = self.current()?;
        Some(ObservedChoice {
            chooser: w.player_name(r.chooser),
            choice_type: r.choice_type.as_str().to_owned(),
            options: options_of(r),
        })
    }

    fn answer(&mut self, chooser: &str, answer: &Value) -> Result<(), String> {
        let Some(run) = self.run.as_mut() else {
            return Err("the setup was not built".to_owned());
        };
        let Some(session) = run.session.as_mut() else {
            return Err("no choice is pending".to_owned());
        };
        let Some(pending) = session.pending().cloned() else {
            return Err("no choice is pending".to_owned());
        };
        let pending_name = run
            .built
            .player_names
            .get(&pending.chooser)
            .cloned()
            .unwrap_or_default();
        if chooser != pending_name {
            return Err(format!(
                "the pending choice belongs to {pending_name}, not {}",
                super::error::quote(chooser)
            ));
        }
        let Value::Object(map) = answer else {
            return Err("an answer is an object with a \"type\"".to_owned());
        };
        let t = map.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(response) = simple_response(t) else {
            return Err(unsupported(format!(
                "the answer type {}",
                super::error::quote(t)
            )));
        };
        if let Some(extra) = map.keys().find(|k| *k != "type") {
            return Err(format!(
                "answer {t} has no field {}",
                super::error::quote(extra)
            ));
        }
        let saved = run.tap.borrow().clone();
        match session.submit(pending.request_id, response) {
            Ok(()) => Ok(()),
            Err(e) => {
                *run.tap.borrow_mut() = saved;
                Err(e.to_string())
            }
        }
    }

    fn observe(&self) -> Observed {
        let Some(run) = self.run.as_ref() else {
            return Observed::default();
        };
        let done = run.session.as_ref().and_then(|s| match s.status() {
            Status::Done { output, .. } => Some(*output),
            Status::Pending(_) => None,
        });
        match self.current() {
            Some(w) => self.observe_world(&w, done),
            None => Observed::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    // REQ-TEST-001: every executed game event maps to an observed event with scenario ids.
    use super::*;
    use crate::scenario::parse_scenario;
    use crate::zones::Moved;

    const FLIER: &[u8] = include_bytes!("../../../../tests/fixtures/cards/keywords/flier_2_2.json");

    fn world() -> (World, ObjectId) {
        let def = CardDefinitionDSL::from_json(FLIER).unwrap();
        let text = format!(
            r#"{{"pzlVersion":1,"title":"t","requirements":["REQ-TEST-001"],"cr":[],"format":"f",
"rulesBaseline":"b","cardDatabase":{{"version":"v","definitionHashes":{{"test.flier_2_2":"{}"}}}},
"prngSeed":3,"players":[{{"id":"p1","life":20}},{{"id":"p2","life":20}}],
"battlefield":[{{"id":"o1","card":"test.flier_2_2","owner":"p1"}}],
"turn":{{"number":2,"activePlayer":"p1","phase":"COMBAT","step":"COMBAT_DAMAGE"}},
"priority":null,"assertions":[]}}"#,
            def.definition_hash
        );
        let s = parse_scenario(text.as_bytes()).unwrap();
        let mut h = StateHost::new();
        h.add_card(def).unwrap();
        let w = h.build_world(&s.setup).unwrap();
        let id = w.zones.contents(ZoneId::Battlefield)[0];
        (w, id)
    }

    #[test]
    fn req_test_001_every_game_event_maps() {
        let (w, o) = world();
        let p = w.game.turn_order()[1];
        let card = CardId::new(99);
        let before = w.zones.clone();
        let cases: Vec<(GameEvent, &str, &str, Value)> = vec![
            (GameEvent::Draw { player: p }, "DRAW", "player", json!("p2")),
            (
                GameEvent::Damage {
                    source: Some(o),
                    source_controller: None,
                    target: DamageTarget::Player(p),
                    amount: 2,
                    combat: true,
                },
                "DAMAGE",
                "source",
                json!("o1"),
            ),
            (
                GameEvent::Damage {
                    source: None,
                    source_controller: None,
                    target: DamageTarget::Object(o),
                    amount: 1,
                    combat: false,
                },
                "DAMAGE",
                "target",
                json!("o1"),
            ),
            (
                GameEvent::LifeChange {
                    player: p,
                    delta: -3,
                },
                "LIFE_CHANGE",
                "delta",
                json!(-3),
            ),
            (
                GameEvent::AddCounters {
                    target: CounterTarget::Player(p),
                    kind: "test".to_owned(),
                    amount: 1,
                },
                "ADD_COUNTERS",
                "target",
                json!("p2"),
            ),
            (
                GameEvent::RemoveCounters {
                    target: CounterTarget::Object(o),
                    kind: "test".to_owned(),
                    amount: 1,
                },
                "REMOVE_COUNTERS",
                "kind",
                json!("test"),
            ),
            (GameEvent::Tap { object: o }, "TAP", "object", json!("o1")),
            (
                GameEvent::Untap { object: o },
                "UNTAP",
                "object",
                json!("o1"),
            ),
            (
                GameEvent::CreateToken {
                    owner: p,
                    card,
                    controller: None,
                },
                "CREATE_TOKEN",
                "owner",
                json!("p2"),
            ),
            (
                GameEvent::TurnBegins { player: p },
                "TURN_BEGINS",
                "player",
                json!("p2"),
            ),
            (
                GameEvent::PhaseBegins {
                    phase: Phase::Ending,
                },
                "PHASE_BEGINS",
                "phase",
                json!("ENDING"),
            ),
            (
                GameEvent::StepBegins { step: Step::End },
                "STEP_BEGINS",
                "step",
                json!("END"),
            ),
            (
                GameEvent::Attach {
                    attachment: o,
                    target: AttachTarget::Player(p),
                },
                "ATTACH",
                "target",
                json!("p2"),
            ),
            (
                GameEvent::Unattach { attachment: o },
                "UNATTACH",
                "attachment",
                json!("o1"),
            ),
            (
                GameEvent::BecomeCopy {
                    object: o,
                    copied: ObjectId::new(777),
                },
                "BECOME_COPY",
                "copied",
                json!("#object777"),
            ),
            (
                GameEvent::TurnFaceUp { object: o },
                "TURN_FACE_UP",
                "object",
                json!("o1"),
            ),
            (
                GameEvent::TurnFaceDown { object: o },
                "TURN_FACE_DOWN",
                "object",
                json!("o1"),
            ),
            (
                GameEvent::UnattachFrom {
                    attachment: o,
                    host: AttachTarget::Permanent(o),
                },
                "UNATTACH_FROM",
                "host",
                json!("o1"),
            ),
            (
                GameEvent::ZoneChange {
                    object: o,
                    request: MoveRequest::new(ZoneKind::Exile),
                    cause: ZoneChangeCause::Exile,
                },
                "ZONE_CHANGE",
                "cause",
                json!("EXILE"),
            ),
        ];
        for (event, name, key, value) in cases {
            let e = w.map_event(&event, &Outcome::Applied, &before);
            assert_eq!(e.event_type, name);
            assert_eq!(e.fields.get(key), Some(&value), "{name}");
            assert!(e.fields.values().all(|v| !v.is_null()));
        }
        let moved = Outcome::ZoneChange(Moved {
            old: o,
            new: o,
            from: ZoneId::Battlefield,
            to: ZoneId::Graveyard(p),
        });
        let e = w.map_event(
            &GameEvent::ZoneChange {
                object: o,
                request: MoveRequest::new(ZoneKind::Graveyard),
                cause: ZoneChangeCause::Destroy,
            },
            &moved,
            &before,
        );
        assert_eq!(e.fields["from"], json!("BATTLEFIELD"));
        assert_eq!(e.fields["to"], json!("GRAVEYARD"));
        for cause in [
            ZoneChangeCause::Other,
            ZoneChangeCause::Sacrifice,
            ZoneChangeCause::Discard,
            ZoneChangeCause::Mill,
            ZoneChangeCause::CounterSpell,
            ZoneChangeCause::StateBasedAction,
            ZoneChangeCause::StateBasedDestroy,
        ] {
            assert!(
                cause_name(cause)
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_')
            );
        }
        for kind in ZoneKind::ALL {
            assert!(!zone_kind_name(kind).is_empty());
        }
        for name in StepName::ALL {
            if let Some(step) = step_of(*name) {
                assert_eq!(step_name(step), name.as_str());
            }
        }
        for name in PhaseName::ALL {
            assert_eq!(phase_name(phase_of(*name)), name.as_str());
        }
        // The built phase and step come from the setup.
        assert_eq!(
            w.phase_and_step(),
            (Some(Phase::Combat), Some(Step::CombatDamage))
        );
        assert_eq!(w.player_name(PlayerId::new(42)), "#player42");
    }

    /// The id of the object called `name` in `w`.
    fn named(w: &World, name: &str) -> ObjectId {
        w.zones
            .contents(ZoneId::Battlefield)
            .iter()
            .copied()
            .find(|&id| w.object_name_in(&w.zones, id).as_deref() == Some(name))
            .unwrap()
    }

    #[test]
    fn req_test_001_setup_keeps_the_authored_timestamp_order_across_attach() {
        // REQ-TEST-001: `timestamp` is the setup order of the battlefield, attachments
        // included (docs/DECISIONS.md "Gate 06-turn F35"). Declared out of order on purpose:
        // the other permanent first, then the host, then the attachment.
        let def = CardDefinitionDSL::from_json(FLIER).unwrap();
        let text = format!(
            r#"{{"pzlVersion":1,"title":"t","requirements":["REQ-TEST-001"],"cr":[],"format":"f",
"rulesBaseline":"b","cardDatabase":{{"version":"v","definitionHashes":{{"test.flier_2_2":"{}"}}}},
"prngSeed":3,"players":[{{"id":"p1","life":20}},{{"id":"p2","life":20}}],
"battlefield":[{{"id":"z","card":"test.flier_2_2","owner":"p1","timestamp":3}},
{{"id":"h","card":"test.flier_2_2","owner":"p2","timestamp":2}},
{{"id":"a","card":"test.flier_2_2","owner":"p1","attachedTo":"h","timestamp":1}}],
"turn":{{"number":2,"activePlayer":"p1","phase":"BEGINNING","step":"UPKEEP"}},
"priority":null,"assertions":[]}}"#,
            def.definition_hash
        );
        let s = parse_scenario(text.as_bytes()).unwrap();
        let mut h = StateHost::new();
        h.add_card(def).unwrap();
        let w = h.build_world(&s.setup).unwrap();
        let ts = |name: &str| {
            w.game
                .timestamps()
                .object_timestamp(named(&w, name))
                .unwrap()
        };
        assert!(ts("a") < ts("h"), "attachment before its host");
        assert!(ts("h") < ts("z"), "host before the later permanent");
        // The attach really happened and issued its timestamp (CR 701.3c); the authored
        // stamps come after it, so it cannot reorder the battlefield.
        assert_eq!(
            w.permanents.attached_to(named(&w, "a")),
            Some(AttachTarget::Permanent(named(&w, "h")))
        );
        let attached: Vec<_> = w
            .events
            .log()
            .batches()
            .iter()
            .flat_map(|b| &b.events)
            .filter_map(|e| match e.outcome {
                Outcome::Attached { timestamp } => Some(timestamp),
                _ => None,
            })
            .collect();
        assert_eq!(attached.len(), 1);
        assert!(attached[0] < ts("a"));
        // R3: the same setup builds the same state.
        let again = h.build_world(&s.setup).unwrap();
        assert_eq!(again.state_hash(), w.state_hash());
    }

    #[test]
    fn req_test_001_host_seams_refuse_unsupported_work() {
        let (mut w, _) = world();
        assert!(w.stack_is_empty());
        assert!(w.priority_actions(w.game.turn_order()[0]).is_empty());
        assert_eq!(w.priority_player(), None);
        let again = World::carry_durable(w.clone(), &w, &w);
        assert_eq!(again.state_hash(), w.state_hash());
        let refused =
            ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
                w.resolve_top(ch)
            });
        assert!(refused.unwrap_err().to_string().contains(UNSUPPORTED));
        let refused =
            ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
                let p = w.game.turn_order()[0];
                w.take_action(p, &ChoiceResponse::Yes, ch)
            });
        assert!(refused.unwrap_err().to_string().contains(UNSUPPORTED));
        let placed =
            ChoiceSession::start(w.clone(), |w: &mut World, ch: &mut ChoiceContext<'_>| {
                w.place_triggers(&[], ch)
            })
            .unwrap();
        assert!(
            matches!(placed.status(), Status::Done { output, .. } if *output == TriggerPlacement::default())
        );
        // Field-less answers map; others do not.
        for t in [
            "PASS_PRIORITY",
            "YES",
            "NO",
            "DECLINE_OPTIONAL_PAYMENT",
            "CANCEL_AND_ROLLBACK",
            "ACCEPT",
            "TAKE_MULLIGAN",
        ] {
            assert_eq!(simple_response(t).map(|r| r.wire_name()), Some(t));
        }
        assert_eq!(simple_response("PLAY_LAND"), None);
        w.journal.clear();
    }
}
