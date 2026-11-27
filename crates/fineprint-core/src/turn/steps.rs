// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The turn sequencer: phases, steps and the turn-based actions outside combat (REQ-TURN-002,
//! SRS 5.2; CR 500-505, CR 514, CR 703).
//!
//! # Structure
//!
//! [`run_turn`] takes one turn through the five phases in order (CR 500.1): beginning,
//! precombat main, combat, postcombat main and ending. [`run_phase`] runs one phase: the phase
//! boundary work and every [`Segment`] of [`segments_of`] the phase. [`run_segment`] runs one
//! segment, a step or a main phase (a main phase has no steps, CR 505.2), with its step boundary
//! work and its turn-based actions (CR 703.1: they do not use the stack; CR 703.2: no player
//! controls them).
//!
//! # Boundaries
//!
//! As a step or phase begins, effects that last until it expire (CR 500.4); as it ends, effects
//! that last until its end expire and then every mana pool empties (CR 500.5, CR 703.4q,
//! CR 106.4). "Until end of combat" effects expire at the end of the combat phase
//! ([`ExpiryPoint::EndPhase`] of [`Phase::Combat`], CR 500.5a). The end-of-turn expiry of
//! CR 514.2 is [`TurnHost::end_of_turn_cleanup`], not an [`ExpiryPoint`].
//!
//! # Steps
//!
//! - Untap (CR 502): phasing (CR 502.1), the day/night check (CR 502.2), then the untap
//!   (CR 502.3), with one `CHOOSE_UNTAP_SET` request when an effect lets the active player
//!   choose. No player receives priority (CR 502.4): triggers wait.
//! - Draw (CR 504): the active player draws (CR 504.1) unless the first-turn rule skips it
//!   ([`first_turn_draw_skipped`], CR 103.8a-c), then the heartbeat (CR 504.2).
//! - Main phases: the [`TurnBasedRegistry`] entries of the segment (lore counters at the
//!   precombat main only, CR 505.4, CR 715.3c, CR 505.1a), then the heartbeat.
//! - Combat steps: [`TurnHost::combat_step_actions`] (other units), then the heartbeat.
//! - Cleanup (CR 514): discard to the maximum hand size with one `SELECT_OBJECTS_FROM_ZONE`
//!   request (CR 514.1), then damage removal and "until end of turn" expiry (CR 514.2), then the
//!   CR 514.3a check: with nothing to do the step ends without priority; otherwise the heartbeat
//!   runs and another cleanup step begins.
//!
//! # Seams and determinism
//!
//! Every game-state change goes through [`TurnHost`], a supertrait of
//! [`HeartbeatHost`](crate::turn::heartbeat::HeartbeatHost): the sequencer holds no game state
//! beyond [`TurnState`] (docs/DECISIONS.md "#46 FP-045 — TurnHost seam"). It is a pure function
//! of (host, state, answers) (R3, REQ-CORE-001): no clock, no I/O, ordered containers only;
//! request ids come from [`ChoiceContext::next_request_id`], so a `ChoiceSession` re-run from the
//! same base with the same answers gives the same requests and the same final state.

use std::hash::{Hash, Hasher};

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse,
    ChoiceType, Interrupt, PendingChoiceRequest, ValidatorContract, ValidatorRule,
};
use crate::events::{Phase, Step};
use crate::format::FirstTurnDrawRule;
use crate::turn::heartbeat::{
    HeartbeatHost, HeartbeatOutcome, HeartbeatState, run_heartbeat, settle_before_priority,
};

/// The CR reference of the `CHOOSE_UNTAP_SET` request.
const UNTAP_CR: &str = "CR 502.3";

/// The CR reference of the cleanup discard request.
const DISCARD_CR: &str = "CR 514.1";

/// The five phases of a turn, in order (CR 500.1).
const PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];

/// One unit the sequencer runs: a step, or a main phase (a main phase has no steps, CR 505.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Segment {
    /// A step of the beginning, combat or ending phase.
    Step(Step),
    /// The precombat main phase: only the first main phase of the turn (CR 505.1a).
    PrecombatMain,
    /// A postcombat main phase (CR 505.1a).
    PostcombatMain,
}

// `Phase` and `Step` do not implement `Hash`; hash the enum discriminants, which identify the
// fieldless variants exactly, so `Hash` agrees with the derived `Eq` (docs/DECISIONS.md "#46
// FP-045 — Hash of Segment and ExpiryPoint").
impl Hash for Segment {
    fn hash<S: Hasher>(&self, state: &mut S) {
        std::mem::discriminant(self).hash(state);
        if let Segment::Step(step) = self {
            std::mem::discriminant(step).hash(state);
        }
    }
}

impl Segment {
    /// The phase the segment belongs to (CR 500.1): a step's [`Step::phase`], or the main phase
    /// itself.
    #[must_use]
    pub fn phase(self) -> Phase {
        match self {
            Segment::Step(step) => step.phase(),
            Segment::PrecombatMain => Phase::PrecombatMain,
            Segment::PostcombatMain => Phase::PostcombatMain,
        }
    }
}

/// The segments of `phase` in order (CR 500.1). Beginning: untap, upkeep, draw. Precombat main:
/// itself. Combat: beginning of combat, declare attackers, declare blockers, the first-strike
/// combat damage step only if `first_strike_damage_step`, combat damage, end of combat.
/// Postcombat main: itself. Ending: end, cleanup.
#[must_use]
pub fn segments_of(phase: Phase, first_strike_damage_step: bool) -> Vec<Segment> {
    // CR 500.1: "The beginning, combat, and ending phases are further broken down into steps,
    // which proceed in order."
    match phase {
        Phase::Beginning => vec![
            Segment::Step(Step::Untap),
            Segment::Step(Step::Upkeep),
            Segment::Step(Step::Draw),
        ],
        Phase::PrecombatMain => vec![Segment::PrecombatMain],
        Phase::Combat => {
            let mut steps = vec![
                Segment::Step(Step::BeginningOfCombat),
                Segment::Step(Step::DeclareAttackers),
                Segment::Step(Step::DeclareBlockers),
            ];
            if first_strike_damage_step {
                steps.push(Segment::Step(Step::FirstStrikeDamage));
            }
            steps.push(Segment::Step(Step::CombatDamage));
            steps.push(Segment::Step(Step::EndOfCombat));
            steps
        }
        Phase::PostcombatMain => vec![Segment::PostcombatMain],
        Phase::Ending => vec![Segment::Step(Step::End), Segment::Step(Step::Cleanup)],
    }
}

/// A point at which "until ..." effects expire (CR 500.4, CR 500.5, CR 500.5a). The end-of-turn
/// expiry of CR 514.2 is NOT one of these (see [`TurnHost::end_of_turn_cleanup`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpiryPoint {
    /// The step begins (CR 500.4).
    BeginStep(Step),
    /// The step ends (CR 500.5).
    EndStep(Step),
    /// The phase begins (CR 500.4).
    BeginPhase(Phase),
    /// The phase ends (CR 500.5; "until end of combat" at the end of [`Phase::Combat`],
    /// CR 500.5a).
    EndPhase(Phase),
}

impl Hash for ExpiryPoint {
    fn hash<S: Hasher>(&self, state: &mut S) {
        std::mem::discriminant(self).hash(state);
        match self {
            ExpiryPoint::BeginStep(s) | ExpiryPoint::EndStep(s) => {
                std::mem::discriminant(s).hash(state);
            }
            ExpiryPoint::BeginPhase(p) | ExpiryPoint::EndPhase(p) => {
                std::mem::discriminant(p).hash(state);
            }
        }
    }
}

/// Which permanents the active player's untap step may untap (CR 502.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UntapOptions {
    /// Permanents that untap with no decision (ascending ids): already excludes "doesn't untap"
    /// ones.
    pub forced: Vec<ObjectId>,
    /// Permanents that an effect lets the player choose whether to untap (ascending ids;
    /// disjoint from `forced`).
    pub choosable: Vec<ObjectId>,
}

/// Upcasts a [`TurnHost`] to `&mut dyn TurnHost`, so that a generic (possibly unsized) host can
/// be handed to a [`TurnBasedAction::run`]. Implemented for every sized [`TurnHost`] and, through
/// the supertrait, for `dyn TurnHost`; implementors of [`TurnHost`] never implement it by hand
/// (docs/DECISIONS.md "#46 FP-045 — Registry actions take &mut dyn TurnHost").
pub trait AsDynTurnHost {
    /// This host as a trait object.
    fn as_dyn_turn_host(&mut self) -> &mut dyn TurnHost;
}

impl<T: TurnHost> AsDynTurnHost for T {
    fn as_dyn_turn_host(&mut self) -> &mut dyn TurnHost {
        self
    }
}

/// The game as the turn sequencer sees it (REQ-TURN-002, SRS 5.2). Every method must be a
/// deterministic function of the host and the answers (R3): a `ChoiceSession` re-runs the whole
/// procedure from its base state after every accepted answer.
///
/// The host owns the turn number, the current phase and step, effect durations, mana pools,
/// the day/night designation, hands and hand sizes (docs/DECISIONS.md "#46 FP-045 — Host-owned
/// turn state").
pub trait TurnHost: HeartbeatHost + AsDynTurnHost {
    /// A new turn begins for `active`: the host increments the turn number, sets the active
    /// player and records TurnBegins (CR 500.1). Called once by [`run_turn`], first.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn begin_turn(&mut self, active: PlayerId) -> Result<(), Interrupt>;

    /// A phase begins (CR 500.6): record it (current phase, PhaseBegins).
    ///
    /// # Errors
    /// Whatever the host returns.
    fn begin_phase(&mut self, phase: Phase) -> Result<(), Interrupt>;

    /// A step begins (CR 500.6): record it (current step, StepBegins).
    ///
    /// # Errors
    /// Whatever the host returns.
    fn begin_step(&mut self, step: Step) -> Result<(), Interrupt>;

    /// Expire the effects that last until `at` (CR 500.4 for the `Begin*` points, CR 500.5 and
    /// CR 500.5a for the `End*` points).
    ///
    /// # Errors
    /// Whatever the host returns.
    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt>;

    /// Empty every player's mana pool (CR 500.5, CR 106.4), honouring rule modifications of how
    /// mana empties (SRS 613.11). Always called AFTER the `EndStep`/`EndPhase` expiry.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn empty_mana_pools(&mut self) -> Result<(), Interrupt>;

    /// CR 502.1: phase `active`'s permanents in and out, simultaneously.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn apply_phasing(&mut self, active: PlayerId) -> Result<(), Interrupt>;

    /// CR 502.2: the day/night check. `previous_active` is the previous turn's active player
    /// (`None` on the first turn). The host decides from its own cast-spell history; if it is
    /// neither day nor night nothing happens.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn check_day_night(&mut self, previous_active: Option<PlayerId>) -> Result<(), Interrupt>;

    /// CR 502.3: which permanents `active` may untap.
    fn untap_options(&self, active: PlayerId) -> UntapOptions;

    /// CR 502.3 and CR 122.1d: untap exactly `permanents` (ascending ids) simultaneously (the
    /// host applies the stun-counter replacement).
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn untap(
        &mut self,
        active: PlayerId,
        permanents: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;

    /// The format's first-turn draw rule (`FormatConfig.first_turn_draw_rule`).
    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule;

    /// The player who plays first, if known.
    fn starting_player(&self) -> Option<PlayerId>;

    /// CR 504.1: `player` draws one card (a Draw event).
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn draw_card(
        &mut self,
        player: PlayerId,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;

    /// The cards in `player`'s hand, ascending ids.
    fn hand(&self, player: PlayerId) -> Vec<ObjectId>;

    /// CR 402.2: `player`'s maximum hand size; `None` means no maximum.
    fn max_hand_size(&self, player: PlayerId) -> Option<u64>;

    /// CR 514.1: `player` discards exactly `cards` (ascending ids) from hand as a discard.
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn discard_cards(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;

    /// CR 514.2: simultaneously remove all marked damage (phased-out permanents included) and
    /// end every "until end of turn" and "this turn" effect.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt>;

    /// CR 505.4, CR 715.3c: the Sagas `player` controls with one or more chapter abilities,
    /// ascending ids.
    fn sagas_with_chapter_abilities(&self, player: PlayerId) -> Vec<ObjectId>;

    /// CR 505.4: put one lore counter on each of `sagas` simultaneously.
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn add_lore_counters(
        &mut self,
        sagas: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;

    /// Whether the combat phase has a first-strike combat damage step (CR 510.4). Default:
    /// `false`.
    fn first_strike_damage_step(&self) -> bool {
        false
    }

    /// The turn-based action of a combat step (declare attackers or blockers, combat damage).
    /// Default: nothing. Called for the six combat steps, after the begin-step expiry and
    /// before the heartbeat.
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn combat_step_actions(
        &mut self,
        _step: Step,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

/// One registered turn-based action of a segment (REQ-TURN-002, CR 703.1). Entries of the same
/// segment run in registration order.
#[derive(Clone, Copy)]
pub struct TurnBasedAction {
    /// Stable id, for example `"lore_counters"`.
    pub id: &'static str,
    /// The CR rule it implements, for example `"CR 505.4"`.
    pub cr: &'static str,
    /// The segment whose start runs it.
    pub segment: Segment,
    /// The action, for the active player.
    pub run: fn(&mut dyn TurnHost, PlayerId, &mut ChoiceContext<'_>) -> Result<(), Interrupt>,
}

/// The registry of segment turn-based actions (REQ-TURN-002): the turn-based actions that are
/// not built into the sequencer, for example the lore counters of CR 505.4.
///
/// In a main phase the entries run before the heartbeat. In a step they run after the step's
/// built-in turn-based actions and before the priority (or CR 514.3a) check (docs/DECISIONS.md
/// "#46 FP-045 — Registry entries of steps").
#[derive(Clone, Default)]
pub struct TurnBasedRegistry {
    /// The entries, in registration order.
    entries: Vec<TurnBasedAction>,
}

/// CR 505.4, CR 715.3c: as the active player's precombat main phase begins, they put a lore
/// counter on each Saga they control with one or more chapter abilities (a turn-based action,
/// CR 703.4f).
fn lore_counters(
    host: &mut dyn TurnHost,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    let sagas = host.sagas_with_chapter_abilities(active);
    if sagas.is_empty() {
        return Ok(());
    }
    host.add_lore_counters(&sagas, choices)
}

impl TurnBasedRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> TurnBasedRegistry {
        TurnBasedRegistry::default()
    }

    /// The standard registry: exactly one entry, `"lore_counters"` (CR 505.4) at
    /// [`Segment::PrecombatMain`]. It calls [`TurnHost::sagas_with_chapter_abilities`] for the
    /// active player and, only if that is non-empty, [`TurnHost::add_lore_counters`] once.
    #[must_use]
    pub fn standard() -> TurnBasedRegistry {
        TurnBasedRegistry {
            // CR 505.1a: only the first main phase of the turn is a precombat main phase.
            entries: vec![TurnBasedAction {
                id: "lore_counters",
                cr: "CR 505.4",
                segment: Segment::PrecombatMain,
                run: lore_counters,
            }],
        }
    }

    /// Appends `action`.
    ///
    /// # Errors
    /// [`ChoiceError::Procedure`] if an entry with the same id is already registered; the
    /// registry is then unchanged.
    pub fn register(&mut self, action: TurnBasedAction) -> Result<(), ChoiceError> {
        if self.entries.iter().any(|e| e.id == action.id) {
            return Err(ChoiceError::Procedure(format!(
                "turn-based action {:?} is already registered",
                action.id
            )));
        }
        self.entries.push(action);
        Ok(())
    }

    /// The entries for `segment`, in registration order.
    #[must_use]
    pub fn entries_for(&self, segment: Segment) -> Vec<TurnBasedAction> {
        self.entries
            .iter()
            .filter(|e| e.segment == segment)
            .copied()
            .collect()
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Sequencer state that lives next to the host in a `ChoiceSession`'s state (REQ-TURN-002). It
/// is not part of the canonical game-state hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnState {
    /// The heartbeat state shared by every heartbeat the sequencer runs.
    heartbeat: HeartbeatState,
    /// The previous turn's active player.
    previous_active: Option<PlayerId>,
}

impl TurnState {
    /// A fresh state: an empty heartbeat state, no previous turn.
    #[must_use]
    pub fn new() -> TurnState {
        TurnState::default()
    }

    /// The heartbeat state (pass log) used by every heartbeat the sequencer runs.
    #[must_use]
    pub fn heartbeat(&self) -> &HeartbeatState {
        &self.heartbeat
    }

    /// The previous turn's active player (`None` before the first turn finishes).
    #[must_use]
    pub fn previous_active_player(&self) -> Option<PlayerId> {
        self.previous_active
    }

    /// Records the active player of a turn that finished; the next turn's CR 502.2 day/night
    /// check reads it (REQ-TURN-002, REQ-TURN-003).
    pub(crate) fn record_turn_taken(&mut self, active: PlayerId) {
        self.previous_active = Some(active);
    }
}

/// What one segment did (REQ-TURN-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentReport {
    /// The segment.
    pub segment: Segment,
    /// True if `run_heartbeat` ran in this segment; false for the untap step; for the cleanup
    /// step true if any cleanup step gave priority.
    pub heartbeat_ran: bool,
    /// The number of cleanup steps run (at least 1 for `Segment::Step(Step::Cleanup)`; 0 for
    /// every other segment).
    pub cleanup_steps: u32,
}

/// What one turn did (REQ-TURN-002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnReport {
    /// The turn's active player.
    pub active_player: PlayerId,
    /// One entry per segment, in order (the cleanup step appears once, with its
    /// `cleanup_steps`).
    pub segments: Vec<SegmentReport>,
}

/// The most cleanup steps one cleanup segment runs before it fails with
/// [`ChoiceError::Procedure`] (a guard against a host that always has work, CR 514.3a).
pub const MAX_CLEANUP_STEPS: u32 = 10_000;

/// The error of a protocol misuse or an untrusted answer.
fn procedure_error(message: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(message))
}

/// The current turn's active player, or an error naming `what`.
fn active_player<H: TurnHost + ?Sized>(host: &H, what: &str) -> Result<PlayerId, Interrupt> {
    host.game()
        .turn()
        .active_player()
        .ok_or_else(|| procedure_error(format!("turn sequencer: {what} without an active player")))
}

/// CR 103.8: whether `active`'s draw step is skipped. True only on the first turn (turn number
/// 1), when `active` is [`TurnHost::starting_player`] and the rule says skip: `AlwaysSkip`
/// skips; `NeverSkip` does not; `CrDefault` skips only in a two-player game (CR 103.8a; the
/// Two-Headed Giant rule CR 103.8b is out of scope; CR 103.8c: other multiplayer games never
/// skip).
#[must_use]
pub fn first_turn_draw_skipped<H: TurnHost + ?Sized>(host: &H, active: PlayerId) -> bool {
    if host.game().turn().turn_number() != 1 || host.starting_player() != Some(active) {
        return false;
    }
    match host.first_turn_draw_rule() {
        FirstTurnDrawRule::AlwaysSkip => true,
        FirstTurnDrawRule::NeverSkip => false,
        // CR 103.8a: "In a two-player game, the player who plays first skips the draw step";
        // CR 103.8c: "In all other multiplayer games, no player skips".
        FirstTurnDrawRule::CrDefault => host.game().player_count() == 2,
    }
}

/// Runs one whole turn for `active` (REQ-TURN-002; CR 500.1): [`TurnHost::begin_turn`], then
/// [`run_phase`] for each of the five phases in order; finally `active` becomes the previous
/// turn's active player.
///
/// # Errors
/// [`Interrupt::Pending`] (a player must decide), any host error, or
/// [`ChoiceError::Procedure`] (no active player after `begin_turn`, an untrusted answer, or the
/// [`MAX_CLEANUP_STEPS`] guard).
pub fn run_turn<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    registry: &TurnBasedRegistry,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<TurnReport, Interrupt> {
    host.begin_turn(active)?;
    let mut segments = Vec::new();
    // CR 500.1: "A turn consists of five phases, in this order ... Each of these phases takes
    // place every turn, even if nothing happens during the phase."
    for phase in PHASES {
        segments.extend(run_phase(host, state, registry, phase, choices)?);
    }
    state.previous_active = Some(active);
    Ok(TurnReport {
        active_player: active,
        segments,
    })
}

/// Runs one phase of the current turn (REQ-TURN-002): [`TurnHost::begin_phase`], the
/// [`ExpiryPoint::BeginPhase`] expiry (CR 500.4), each segment of
/// [`segments_of`]`(phase, host.first_strike_damage_step())` through [`run_segment`], then the
/// [`ExpiryPoint::EndPhase`] expiry and [`TurnHost::empty_mana_pools`] (CR 500.5, CR 500.5a).
/// The active player is the game's; this does not call [`TurnHost::begin_turn`].
///
/// # Errors
/// As [`run_turn`]; [`ChoiceError::Procedure`] if the game has no active player.
pub fn run_phase<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    registry: &TurnBasedRegistry,
    phase: Phase,
    choices: &mut ChoiceContext<'_>,
) -> Result<Vec<SegmentReport>, Interrupt> {
    active_player(host, "a phase")?;
    // CR 500.6, CR 500.4.
    host.begin_phase(phase)?;
    host.expire_effects(ExpiryPoint::BeginPhase(phase))?;
    let segments = segments_of(phase, host.first_strike_damage_step());
    let mut reports = Vec::with_capacity(segments.len());
    for segment in segments {
        reports.push(run_segment(host, state, registry, segment, choices)?);
    }
    // CR 500.5: expiry first, then the mana pools empty (CR 703.4q); "until end of combat"
    // expires here at the end of the combat phase (CR 500.5a).
    host.expire_effects(ExpiryPoint::EndPhase(phase))?;
    host.empty_mana_pools()?;
    Ok(reports)
}

/// Runs the registry entries of `segment` for `active`, in registration order.
fn run_registry<H: TurnHost + ?Sized>(
    host: &mut H,
    registry: &TurnBasedRegistry,
    segment: Segment,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    for entry in registry.entries_for(segment) {
        (entry.run)(host.as_dyn_turn_host(), active, choices)?;
    }
    Ok(())
}

/// Runs the heartbeat of the current step (CR 117.3a); [`HeartbeatOutcome::NoPlayerInGame`]
/// also ends the step.
fn heartbeat<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    match run_heartbeat(host, &mut state.heartbeat, choices)? {
        HeartbeatOutcome::StepEnds | HeartbeatOutcome::NoPlayerInGame => Ok(()),
    }
}

/// Runs one segment (a step, or a main phase) of the current turn (REQ-TURN-002) with NO phase
/// boundary work (no [`TurnHost::begin_phase`], no phase-level expiry or mana emptying).
///
/// A step: [`TurnHost::begin_step`], the [`ExpiryPoint::BeginStep`] expiry (CR 500.4), the
/// step's turn-based actions and priority, the [`ExpiryPoint::EndStep`] expiry and
/// [`TurnHost::empty_mana_pools`] (CR 500.5); the cleanup step repeats this whole sequence per
/// cleanup step (CR 514.3a). A main phase: its registry entries, then the heartbeat.
///
/// # Errors
/// As [`run_turn`]; [`ChoiceError::Procedure`] if the game has no active player.
pub fn run_segment<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    registry: &TurnBasedRegistry,
    segment: Segment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SegmentReport, Interrupt> {
    let active = active_player(host, "a segment")?;
    let step = match segment {
        Segment::PrecombatMain | Segment::PostcombatMain => {
            // CR 505.4, CR 703.4f: the main-phase turn-based actions, then priority (CR 505.2,
            // CR 117.3a).
            run_registry(host, registry, segment, active, choices)?;
            heartbeat(host, state, choices)?;
            return Ok(SegmentReport {
                segment,
                heartbeat_ran: true,
                cleanup_steps: 0,
            });
        }
        Segment::Step(step) => step,
    };
    if step == Step::Cleanup {
        return run_cleanup(host, state, registry, active, choices);
    }
    // CR 500.6, CR 500.4.
    host.begin_step(step)?;
    host.expire_effects(ExpiryPoint::BeginStep(step))?;
    let heartbeat_ran = match step {
        Step::Untap => {
            run_untap(host, state, active, choices)?;
            run_registry(host, registry, segment, active, choices)?;
            // CR 502.4: no player receives priority during the untap step; triggers wait.
            false
        }
        Step::Draw => {
            // CR 504.1, CR 703.4d, CR 103.8.
            if !first_turn_draw_skipped(host, active) {
                host.draw_card(active, choices)?;
            }
            run_registry(host, registry, segment, active, choices)?;
            // CR 504.2.
            heartbeat(host, state, choices)?;
            true
        }
        Step::Upkeep | Step::End => {
            run_registry(host, registry, segment, active, choices)?;
            heartbeat(host, state, choices)?;
            true
        }
        Step::BeginningOfCombat
        | Step::DeclareAttackers
        | Step::DeclareBlockers
        | Step::FirstStrikeDamage
        | Step::CombatDamage
        | Step::EndOfCombat => {
            host.combat_step_actions(step, choices)?;
            run_registry(host, registry, segment, active, choices)?;
            heartbeat(host, state, choices)?;
            true
        }
        // Handled above.
        Step::Cleanup => false,
    };
    // CR 500.5, CR 703.4q: expiry first, then the mana pools empty.
    host.expire_effects(ExpiryPoint::EndStep(step))?;
    host.empty_mana_pools()?;
    Ok(SegmentReport {
        segment,
        heartbeat_ran,
        cleanup_steps: 0,
    })
}

/// The untap step's turn-based actions (CR 502.1-502.3, CR 703.4a-c), no priority (CR 502.4).
fn run_untap<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &TurnState,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    // CR 502.1: first, phasing.
    host.apply_phasing(active)?;
    // CR 502.2, CR 732.2: second, the day/night check against the previous turn.
    host.check_day_night(state.previous_active)?;
    // CR 502.3: third, the active player determines which permanents untap, then untaps them
    // all simultaneously.
    let options = host.untap_options(active);
    if options.choosable.is_empty() {
        // No decision: the host's forced list, as given; nothing to untap, no call
        // (docs/DECISIONS.md "#46 FP-045 — Untap call without a choice").
        if options.forced.is_empty() {
            return Ok(());
        }
        return host.untap(active, &options.forced, choices);
    }
    let chosen = ask_untap_set(active, &options.choosable, choices)?;
    let mut untapping = options.forced;
    untapping.extend(chosen);
    untapping.sort_unstable();
    untapping.dedup();
    if untapping.is_empty() {
        return Ok(());
    }
    host.untap(active, &untapping, choices)
}

/// Checks that `objects` are distinct members of `candidates`, `count` of them if given (R12).
fn check_selection(
    objects: &[ObjectId],
    candidates: &[ObjectId],
    count: Option<usize>,
    what: &str,
) -> Result<(), Interrupt> {
    let mut sorted = objects.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let valid = sorted.len() == objects.len()
        && objects.iter().all(|o| candidates.contains(o))
        && count.is_none_or(|k| objects.len() == k);
    if valid {
        Ok(())
    } else {
        Err(procedure_error(format!(
            "turn sequencer: the {what} answer is not a valid selection"
        )))
    }
}

/// CR 502.3: the one `CHOOSE_UNTAP_SET` request of the untap step.
fn ask_untap_set(
    active: PlayerId,
    choosable: &[ObjectId],
    choices: &mut ChoiceContext<'_>,
) -> Result<Vec<ObjectId>, Interrupt> {
    let max = u64::try_from(choosable.len()).unwrap_or(u64::MAX);
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: active,
        choice_type: ChoiceType::ChooseUntapSet,
        reason: ChoiceReason {
            cr: UNTAP_CR.to_owned(),
            source: None,
        },
        min: 0,
        max,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitSet {
            objects: choosable.to_vec(),
        },
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 0,
            max,
            rule: ValidatorRule::Subset {
                candidates: choosable.to_vec(),
            },
        }),
        payload: ChoicePayload::ChooseUntapSet {
            permanents: choosable.to_vec(),
        },
    };
    let ChoiceResponse::SubmitSet { objects } = choices.ask(request)? else {
        return Err(procedure_error(
            "turn sequencer: the untap answer is not SUBMIT_SET".to_owned(),
        ));
    };
    // R12: the session validated the answer; never trust it blindly anyway.
    check_selection(&objects, choosable, None, "untap")?;
    Ok(objects)
}

/// CR 514.1: the cleanup discard. With more cards in hand than the maximum, one
/// `SELECT_OBJECTS_FROM_ZONE` request for exactly the excess, then one discard.
fn cleanup_discard<H: TurnHost + ?Sized>(
    host: &mut H,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    let Some(max_size) = host.max_hand_size(active) else {
        return Ok(());
    };
    let mut hand = host.hand(active);
    hand.sort_unstable();
    hand.dedup();
    let size = u64::try_from(hand.len()).unwrap_or(u64::MAX);
    if size <= max_size {
        return Ok(());
    }
    let excess = size - max_size;
    let k = usize::try_from(excess).unwrap_or(usize::MAX);
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: active,
        choice_type: ChoiceType::SelectObjectsFromZone,
        reason: ChoiceReason {
            cr: DISCARD_CR.to_owned(),
            source: None,
        },
        min: excess,
        max: excess,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitObjectSelection {
            objects: hand.iter().take(k).copied().collect(),
        },
        options: ChoiceOptions::Validator(ValidatorContract {
            min: excess,
            max: excess,
            rule: ValidatorRule::Subset {
                candidates: hand.clone(),
            },
        }),
        payload: ChoicePayload::SelectObjectsFromZone {
            candidates: hand.clone(),
        },
    };
    let ChoiceResponse::SubmitObjectSelection { mut objects } = choices.ask(request)? else {
        return Err(procedure_error(
            "turn sequencer: the discard answer is not SUBMIT_OBJECT_SELECTION".to_owned(),
        ));
    };
    // R12: re-check the count and the membership.
    check_selection(&objects, &hand, Some(k), "discard")?;
    objects.sort_unstable();
    host.discard_cards(active, &objects, choices)
}

/// The cleanup step (CR 514), repeated while CR 514.3a gives priority.
fn run_cleanup<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    registry: &TurnBasedRegistry,
    active: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<SegmentReport, Interrupt> {
    let segment = Segment::Step(Step::Cleanup);
    let mut steps: u32 = 0;
    let mut heartbeat_ran = false;
    loop {
        if steps >= MAX_CLEANUP_STEPS {
            return Err(procedure_error(format!(
                "turn sequencer: the cleanup step did not end within MAX_CLEANUP_STEPS \
                 ({MAX_CLEANUP_STEPS}) cleanup steps"
            )));
        }
        steps += 1;
        // CR 500.6, CR 500.4.
        host.begin_step(Step::Cleanup)?;
        host.expire_effects(ExpiryPoint::BeginStep(Step::Cleanup))?;
        // CR 514.1, CR 703.4n.
        cleanup_discard(host, active, choices)?;
        // CR 514.2, CR 703.4p: always, even if nothing was discarded.
        host.end_of_turn_cleanup()?;
        run_registry(host, registry, segment, active, choices)?;
        // CR 514.3a: "the game checks to see if any state-based actions would be performed
        // and/or any triggered abilities are waiting to be put onto the stack". A settle that
        // repeated (sba_passes > 1) performed an action or stacked or removed a trigger
        // (docs/DECISIONS.md "#46 FP-045 — CR 514.3a needs-priority test").
        let report = settle_before_priority(host, choices)?;
        let needs_priority =
            report.sba_passes > 1 || report.triggers_stacked > 0 || !host.stack_is_empty();
        if needs_priority {
            // CR 514.3a: "then the active player gets priority".
            heartbeat(host, state, choices)?;
            heartbeat_ran = true;
        }
        // CR 500.5, CR 703.4q.
        host.expire_effects(ExpiryPoint::EndStep(Step::Cleanup))?;
        host.empty_mana_pools()?;
        if !needs_priority {
            return Ok(SegmentReport {
                segment,
                heartbeat_ran,
                cleanup_steps: steps,
            });
        }
        // CR 514.3a: "Once the stack is empty and all players pass in succession, another
        // cleanup step begins."
    }
}
