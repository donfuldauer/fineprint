// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The priority-and-SBA heartbeat (REQ-TURN-001, SRS 5.1; CR 117.3-117.5, CR 704.3,
//! CR 603.3b).
//!
//! # Diagram mapping
//!
//! The SRS 5.1 state machine runs whenever a player would receive priority:
//!
//! - **Step 1** (CR 704.3, CR 117.5): [`HeartbeatHost::sba_pass`] performs ONE state-based-action
//!   pass; while it reports [`SbaOutcome::performed`] the pass is repeated (transition T1). A pass
//!   that only *found* something but changed nothing does not repeat (docs/DECISIONS.md "#41
//!   FP-040 — performed means state changed").
//! - **Step 2** (CR 603.3b, T2): when no state-based action was performed,
//!   [`HeartbeatHost::place_triggers`] puts the waiting triggers on the stack in APNAP order. If it
//!   stacked or removed any trigger (CR 603.3c, CR 603.3d: a removed trigger was first put on the
//!   stack), the heartbeat returns to step 1 (T3); an empty placement ends the settle (T4).
//!   [`settle_before_priority`] is steps 1 and 2.
//! - **Step 3** (CR 117.3): the priority player is asked one `PRIORITY_ACTION` request.
//!   - An action (cast, activate, play a land, special action, mana ability) goes to
//!     [`HeartbeatHost::take_action`]; the actor keeps priority (CR 117.3c, CR 116.3), the pass
//!     sequence restarts, and the heartbeat returns to step 1 (T5).
//!   - A pass (CR 117.3d) is recorded as a [`PassRecord`]. If not every player still in the game
//!     has passed in succession, priority moves to the next player in turn order who is still in
//!     the game and step 3 repeats WITHOUT a settle (T6). Otherwise (CR 117.4) the top object of a
//!     non-empty stack resolves through [`HeartbeatHost::resolve_top`], the active player receives
//!     priority (CR 117.3b) and the heartbeat returns to step 1 (T7); with an empty stack the step
//!     or phase ends: [`run_heartbeat`] returns [`HeartbeatOutcome::StepEnds`] with no priority
//!     player (T8).
//!
//! # Pass counting
//!
//! "All players pass in succession" (CR 117.4) is tested against the players still in the game:
//! the heartbeat keeps the set of players who passed since the last action, resolution or step
//! start; a player who left the game no longer counts (docs/DECISIONS.md "#44 FP-043 — Pass
//! counting uses the players still in the game"). [`HeartbeatState::pass_count`] is the size of
//! that set. Every pass is an explicit `PASS_PRIORITY` answer and is logged in
//! [`HeartbeatState::passes`]; there is no yield or auto-pass input (yields are a client
//! convenience, REQ-API-002).
//!
//! # Players who left the game
//!
//! A player who left the game ([`HeartbeatHost::has_left_game`]) is never granted priority,
//! never asked and never counted (CR 800.4j, CR 800.4a). When the active player would receive
//! priority but has left, the next player in turn order who is still in the game receives it
//! (CR 800.4j); APNAP order then starts at that player too.
//!
//! # Seams
//!
//! The heartbeat is written against [`HeartbeatHost`]: the stack, resolution, casting,
//! activation, legal-action generation, the waiting-trigger queue and the state-based-action
//! inputs belong to the host. A host's [`HeartbeatHost::sba_pass`] delegates to
//! [`sba_pass_post_batch`], which calls [`sba::perform_all_post_batch`](crate::sba::perform_all_post_batch)
//! with the host's [`PostBatchState`] provider. The heartbeat itself never pushes to or pops
//! from the stack and changes only the priority player
//! ([`GameState::set_priority_player`]).
//!
//! # Out of scope
//!
//! The stack and resolution, casting and activation, legal-action generation, the real-state
//! host (the game-loop integration unit), step and phase sequencing, expiring "until end of
//! step" effects and emptying mana pools when a step ends (CR 500.5: done by the turn sequencer,
//! not here), the procedure of leaving the game (CR 800.4a: the host removes a departed player's
//! objects and waiting triggers), and turn control (CR 724).
//!
//! # Determinism
//!
//! The heartbeat is a pure function of (host, state, answers) (R3, REQ-CORE-001): no clock, no
//! I/O, no global state, ordered containers only, iteration by turn order. Request ids come
//! from [`ChoiceContext::next_request_id`], so a `ChoiceSession` re-run from the same base with
//! the same answers asks the same requests and ends in the same state with the same pass log.

use std::collections::BTreeSet;

use fineprint_base::ids::PlayerId;

use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse,
    ChoiceType, Interrupt, PendingChoiceRequest, ValidatorContract, ValidatorRule,
};
use crate::events::{EventContext, Phase, Step};
use crate::layers::{ContinuousEffects, LayerEnvironment};
use crate::replacement::{BusInterrupt, ReplacementRegistry};
use crate::sba::{
    LOST_THE_GAME, PostBatchState, SbaEnvironment, SbaOutcome, SbaRegistry, perform_all_post_batch,
};
use crate::state::GameState;
use crate::triggers::TriggerPlacement;

/// The most host calls (state-based-action passes plus trigger placements) one
/// [`settle_before_priority`] makes before it gives up with an error: a guard against a host
/// that never settles (REQ-TURN-001).
pub const MAX_SETTLE_ITERATIONS: usize = 10_000;

/// The CR reference of the heartbeat's `PRIORITY_ACTION` request.
const PRIORITY_CR: &str = "CR 117.3";

/// The game as the heartbeat sees it (REQ-TURN-001, SRS 5.1).
///
/// The host owns the stack, resolution, the legal actions, the waiting-trigger queue and the
/// state-based-action inputs; the heartbeat drives them in the order of the SRS 5.1 diagram.
/// Every method must be a deterministic function of the host and the answers (R3): a
/// `ChoiceSession` re-runs the heartbeat from its base state after every accepted answer.
pub trait HeartbeatHost {
    /// The game state: turn order, active player, player designations.
    fn game(&self) -> &GameState;

    /// Write access to the game state. The heartbeat only calls
    /// [`GameState::set_priority_player`] through it.
    fn game_mut(&mut self) -> &mut GameState;

    /// The current phase and step, copied into each [`PassRecord`]. Default: `(None, None)`.
    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        (None, None)
    }

    /// Whether `player` has left the game (CR 800.4j). Default: `player` is not a player of
    /// [`HeartbeatHost::game`], or has the [`LOST_THE_GAME`] designation.
    fn has_left_game(&self, player: PlayerId) -> bool {
        self.game()
            .player(player)
            .is_none_or(|p| p.designation(LOST_THE_GAME).is_some())
    }

    /// Step 1 (CR 704.3): ONE state-based-action pass, all applicable actions simultaneously as
    /// one event. The standard body is [`sba_pass_post_batch`]. A [`BusInterrupt::Pending`]
    /// suspends the heartbeat; on an error the host must be unchanged.
    ///
    /// # Errors
    /// Whatever the pass returns.
    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt>;

    /// Step 2 (CR 603.3b): take the waiting triggers and put them on the stack, the players in
    /// `apnap` order (each player in the game exactly once). The standard body is
    /// [`triggers::place_triggers`](crate::triggers::place_triggers) over the host's queue.
    ///
    /// # Errors
    /// Whatever the placement returns.
    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt>;

    /// Whether the stack is empty.
    fn stack_is_empty(&self) -> bool;

    /// Resolves the top object of the stack (CR 117.4). Called only when the stack is not
    /// empty.
    ///
    /// # Errors
    /// Whatever resolution returns (a pending choice or a failure).
    fn resolve_top(&mut self, choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt>;

    /// Every legal non-pass action of `player` (who has priority), each as the exact
    /// [`ChoiceResponse`] that takes it (`PLAY_LAND`, `INITIATE_CAST`, `ACTIVATE`,
    /// `SPECIAL_ACTION`), in a deterministic order.
    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse>;

    /// Takes `action`, one of [`HeartbeatHost::priority_actions`] for `player`, through the
    /// transaction pipeline (SRS 7). Spells and abilities go on the stack here; mana abilities
    /// and special actions do not use the stack (CR 605.3b, CR 116.1).
    ///
    /// # Errors
    /// Whatever the action returns (a pending choice or a failure).
    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;
}

/// How one pass ended (REQ-TURN-001, CR 117.3d, CR 117.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassResult {
    /// Not every player still in the game has passed: priority moved to `to`, the next player
    /// in turn order who is still in the game (CR 117.3d).
    PriorityMoved {
        /// The player who receives priority.
        to: PlayerId,
    },
    /// Every player still in the game passed in succession and the stack was not empty: the
    /// top object resolved (CR 117.4).
    TopResolved,
    /// Every player still in the game passed in succession with an empty stack: the step or
    /// phase ends (CR 117.4).
    StepEnds,
}

/// One explicit pass (REQ-TURN-001; SRS 5.1: every pass is recorded so a replay never depends
/// on client yield settings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassRecord {
    /// The 0-based position of this pass over the life of the [`HeartbeatState`].
    pub sequence: u64,
    /// The turn number when the pass was made.
    pub turn_number: u64,
    /// The phase, from [`HeartbeatHost::phase_and_step`].
    pub phase: Option<Phase>,
    /// The step, from [`HeartbeatHost::phase_and_step`].
    pub step: Option<Step>,
    /// The player who passed.
    pub player: PlayerId,
    /// [`HeartbeatState::pass_count`] right after this pass.
    pub pass_count: usize,
    /// The number of players still in the game at the pass.
    pub players_in_game: usize,
    /// Whether the stack was empty at the pass.
    pub stack_empty: bool,
    /// What the pass led to.
    pub result: PassResult,
}

/// The heartbeat's own state (REQ-TURN-001): the players who passed in succession and the pass
/// log. It lives next to the host in a `ChoiceSession`'s state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeartbeatState {
    /// The players still in the game who passed since the last action, resolution or step
    /// start (ascending ids).
    passed: BTreeSet<PlayerId>,
    /// Every pass, oldest first.
    passes: Vec<PassRecord>,
}

impl HeartbeatState {
    /// An empty state: no pass counted, an empty log.
    #[must_use]
    pub fn new() -> HeartbeatState {
        HeartbeatState::default()
    }

    /// PassCount (SRS 5.1): the number of players still in the game who passed in succession
    /// since the last action, resolution or step start, as of the heartbeat's last look at the
    /// game (CR 117.4; docs/DECISIONS.md "#44 FP-043 — Pass counting uses the players still in
    /// the game").
    #[must_use]
    pub fn pass_count(&self) -> usize {
        self.passed.len()
    }

    /// Every pass ever recorded through this state, oldest first.
    #[must_use]
    pub fn passes(&self) -> &[PassRecord] {
        &self.passes
    }

    /// Forgets the passes of players who left the game (CR 800.4j: they are skipped).
    fn forget_departed<H: HeartbeatHost + ?Sized>(&mut self, host: &H) {
        self.passed.retain(|&p| !host.has_left_game(p));
    }
}

/// How [`run_heartbeat`] ended (REQ-TURN-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeartbeatOutcome {
    /// Every player still in the game passed in succession with an empty stack: the current
    /// step or phase ends (CR 117.4). The priority player is `None`; the turn sequencer expires
    /// "until end of step" effects and empties mana pools (CR 500.5).
    StepEnds,
    /// No player is in the game; nobody was asked. The priority player is `None`.
    NoPlayerInGame,
}

/// What one [`settle_before_priority`] did (REQ-TURN-001).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettleReport {
    /// The number of state-based-action passes made (step 1 calls).
    pub sba_passes: usize,
    /// The number of triggers put on the stack (step 2), removed triggers not included.
    pub triggers_stacked: usize,
}

/// The state-based-action inputs of one heartbeat step 1 (REQ-TURN-001, REQ-SBA-001): the
/// arguments of [`sba::perform_all_post_batch`](crate::sba::perform_all_post_batch) except the
/// choice context.
pub struct SbaStage<'a, 'c> {
    /// The state-based-action registry.
    pub registry: &'a SbaRegistry,
    /// The event context of the game.
    pub ctx: &'a mut EventContext<'c>,
    /// The replacement registry.
    pub replacements: &'a mut ReplacementRegistry,
    /// The continuous effects (before the pass).
    pub effects: &'a ContinuousEffects,
    /// The layer environment (before the pass).
    pub layer_env: &'a dyn LayerEnvironment,
    /// The host's state-based-action environment.
    pub env: &'a mut dyn SbaEnvironment,
    /// The provider of the layer inputs and the read-only protector view of the state after the
    /// batch (docs/DECISIONS.md "Gate 02-state-objects R3c").
    pub post_batch: PostBatchState<'a>,
}

/// One state-based-action pass for heartbeat step 1 (REQ-TURN-001, REQ-SBA-001; CR 704.3: "the
/// game checks for any of the listed conditions for state-based actions, then performs all
/// applicable state-based actions simultaneously as a single event").
///
/// This is the standard body of a host's [`HeartbeatHost::sba_pass`]: a host delegates to it
/// with its [`SbaStage`]. It calls
/// [`sba::perform_all_post_batch`](crate::sba::perform_all_post_batch) with the caller's
/// [`PostBatchState`] provider, never `perform_all` or `perform_all_relayered`, so the protector
/// request of a retained battle sees the state after the batch.
///
/// # Errors
/// As [`sba::perform_all_post_batch`](crate::sba::perform_all_post_batch): a pending choice or a
/// bus failure, with every component restored.
pub fn sba_pass_post_batch(
    stage: SbaStage<'_, '_>,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    let SbaStage {
        registry,
        ctx,
        replacements,
        effects,
        layer_env,
        env,
        post_batch,
    } = stage;
    perform_all_post_batch(
        registry,
        ctx,
        replacements,
        effects,
        layer_env,
        env,
        choices,
        post_batch,
    )
}

/// The number of players still in the game (REQ-TURN-001, CR 800.4j: players who left are
/// skipped).
pub fn players_in_game<H: HeartbeatHost + ?Sized>(host: &H) -> usize {
    host.game()
        .turn_order()
        .iter()
        .filter(|&&p| !host.has_left_game(p))
        .count()
}

/// The next player after `after` in turn order (wrapping) who is still in the game
/// (REQ-TURN-001; CR 117.3d, CR 800.4a: "priority passes to the next player in turn order who's
/// still in the game").
///
/// `after` may itself have left the game; it is returned only when it is the only player still
/// in the game. If `after` is not in the turn order, the first player in turn order who is still
/// in the game. `None` if no player is in the game.
pub fn next_player_in_turn_order<H: HeartbeatHost + ?Sized>(
    host: &H,
    after: PlayerId,
) -> Option<PlayerId> {
    let order = host.game().turn_order();
    let Some(seat) = order.iter().position(|&p| p == after) else {
        return first_in_game(host);
    };
    let n = order.len();
    (1..=n)
        .filter_map(|i| order.get((seat + i) % n).copied())
        .find(|&p| !host.has_left_game(p))
}

/// The first player in turn order who is still in the game.
fn first_in_game<H: HeartbeatHost + ?Sized>(host: &H) -> Option<PlayerId> {
    host.game()
        .turn_order()
        .iter()
        .copied()
        .find(|&p| !host.has_left_game(p))
}

/// The player who receives priority when "the active player" would (CR 117.3a, CR 117.3b): the
/// active player if still in the game; if the active player has left, the next player in turn
/// order after their seat who is still in the game (CR 800.4j); with no active player, the first
/// player in turn order who is still in the game (docs/DECISIONS.md "#44 FP-043 — Priority start
/// when the active player has left or is absent"). `None` if no player is in the game.
fn active_or_next<H: HeartbeatHost + ?Sized>(host: &H) -> Option<PlayerId> {
    match host.game().turn().active_player() {
        Some(a) if !host.has_left_game(a) => Some(a),
        Some(a) => next_player_in_turn_order(host, a),
        None => first_in_game(host),
    }
}

/// The players still in the game in APNAP order (CR 603.3b): starting at [`active_or_next`],
/// then turn order, each once.
fn apnap_order<H: HeartbeatHost + ?Sized>(host: &H) -> Vec<PlayerId> {
    let Some(start) = active_or_next(host) else {
        return Vec::new();
    };
    let order = host.game().turn_order();
    let n = order.len();
    let seat = order.iter().position(|&p| p == start).unwrap_or(0);
    (0..n)
        .filter_map(|i| order.get((seat + i) % n).copied())
        .filter(|&p| !host.has_left_game(p))
        .collect()
}

/// The error of a protocol misuse or a failed state write.
fn procedure_error(message: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(message))
}

/// Counts one host call of the settle loop against [`MAX_SETTLE_ITERATIONS`].
fn tick(iterations: &mut usize) -> Result<(), Interrupt> {
    *iterations += 1;
    if *iterations > MAX_SETTLE_ITERATIONS {
        return Err(procedure_error(format!(
            "heartbeat: state-based actions and triggers did not settle within \
             MAX_SETTLE_ITERATIONS ({MAX_SETTLE_ITERATIONS}) passes and placements"
        )));
    }
    Ok(())
}

/// Steps 1 and 2 of the heartbeat (REQ-TURN-001; CR 117.5, CR 704.3, CR 603.3b).
///
/// CR 117.5: "Each time a player would get priority, the game first performs all applicable
/// state-based actions as a single event ..., then repeats this process until no state-based
/// actions are performed. Then triggered abilities are put on the stack ... These steps repeat
/// in order until no further state-based actions are performed and no abilities trigger."
///
/// Step 1 repeats [`HeartbeatHost::sba_pass`] while it reports `performed` (findings alone do
/// not repeat). Step 2 calls [`HeartbeatHost::place_triggers`] with the players still in the
/// game in APNAP order; a placement that stacked or removed any trigger returns to step 1
/// (CR 603.3c, CR 603.3d: a removed trigger was put on the stack first), an empty placement ends
/// the settle. With no player in the game, the settle ends after step 1 without a placement.
///
/// # Errors
/// Any interrupt of the host (propagated unchanged; a [`BusInterrupt`] becomes the matching
/// [`Interrupt`]), or [`ChoiceError::Procedure`] when more than [`MAX_SETTLE_ITERATIONS`] host
/// calls would be needed.
pub fn settle_before_priority<H: HeartbeatHost + ?Sized>(
    host: &mut H,
    choices: &mut ChoiceContext<'_>,
) -> Result<SettleReport, Interrupt> {
    let mut report = SettleReport::default();
    let mut iterations = 0usize;
    loop {
        // Step 1, CR 704.3: "If any state-based actions are performed as a result of a check,
        // the check is repeated" (T1); otherwise go to step 2 (T2).
        loop {
            tick(&mut iterations)?;
            let outcome = host.sba_pass(choices).map_err(Interrupt::from)?;
            report.sba_passes += 1;
            if !outcome.performed {
                break;
            }
        }
        // Step 2, CR 603.3b: "each player, in APNAP order, puts ... triggered abilities they
        // control on the stack ... Then the game once again checks for and performs
        // state-based actions".
        let apnap = apnap_order(host);
        if apnap.is_empty() {
            return Ok(report);
        }
        tick(&mut iterations)?;
        let placement = host.place_triggers(&apnap, choices)?;
        report.triggers_stacked += placement.stacked.len();
        // CR 603.3c, CR 603.3d: a removed trigger went on the stack first, so it counts as
        // processed (T3); only an empty placement goes on to step 3 (T4).
        if placement.stacked.is_empty() && placement.removed.is_empty() {
            return Ok(report);
        }
    }
}

/// Sets the priority player through the host's game state.
fn set_priority<H: HeartbeatHost + ?Sized>(
    host: &mut H,
    player: Option<PlayerId>,
) -> Result<(), Interrupt> {
    host.game_mut()
        .set_priority_player(player)
        .map_err(|e| procedure_error(format!("heartbeat: cannot set the priority player: {e}")))
}

/// Step 3 (CR 117.3): one `PRIORITY_ACTION` request of `player`.
fn ask_priority<H: HeartbeatHost + ?Sized>(
    host: &H,
    player: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<ChoiceResponse, Interrupt> {
    let actions = host.priority_actions(player);
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: player,
        choice_type: ChoiceType::PriorityAction,
        reason: ChoiceReason {
            cr: PRIORITY_CR.to_owned(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::PassPriority,
        // docs/DECISIONS.md "#28 FP-027 — Payloads and responses of the procedure-raised types":
        // the payload carries every legal answer.
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        }),
        payload: ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: actions.clone(),
        },
    };
    let response = choices.ask(request)?;
    // R12: the session validated the answer; never trust it blindly anyway.
    if response != ChoiceResponse::PassPriority && !actions.contains(&response) {
        return Err(procedure_error(format!(
            "heartbeat: {} is not a legal priority answer",
            response.wire_name()
        )));
    }
    Ok(response)
}

/// Runs the priority heartbeat for one step (REQ-TURN-001, SRS 5.1; CR 117.3-117.5, CR 704.3,
/// CR 603.3b, CR 800.4j).
///
/// Entry: PassCount is reset (a new step starts a new count; the pass log keeps growing). The
/// active player is the first to receive priority (CR 117.3a); if they have left the game, the
/// next player in turn order who is still in the game (CR 800.4j). Then, in a loop: settle
/// (steps 1 and 2, [`settle_before_priority`]); grant priority (step 3) by setting the priority
/// player and asking one `PRIORITY_ACTION` request; on an action, the actor keeps priority,
/// PassCount is reset and the loop returns to step 1 (CR 117.3c, CR 116.3); on a pass, the pass
/// is recorded and priority moves on (CR 117.3d) or, once every player still in the game has
/// passed in succession (CR 117.4), the top object resolves and the active player receives
/// priority (CR 117.3b), or with an empty stack the step ends.
///
/// # Errors
/// [`Interrupt::Pending`] (a player must decide: a `PRIORITY_ACTION` or a request raised by the
/// host), any host error, or [`ChoiceError::Procedure`] when the priority player cannot be set,
/// the settle does not converge, or an answer is not a legal priority answer.
pub fn run_heartbeat<H: HeartbeatHost + ?Sized>(
    host: &mut H,
    state: &mut HeartbeatState,
    choices: &mut ChoiceContext<'_>,
) -> Result<HeartbeatOutcome, Interrupt> {
    // A new step starts a new pass sequence.
    state.passed.clear();
    // CR 117.3a, CR 800.4j: the active player, or the next player still in the game.
    let Some(mut intended) = active_or_next(host) else {
        set_priority(host, None)?;
        return Ok(HeartbeatOutcome::NoPlayerInGame);
    };
    loop {
        // Steps 1 and 2 (CR 117.5).
        settle_before_priority(host, choices)?;
        state.forget_departed(host);
        let mut player = intended;
        // Step 3, repeated without a settle while passes move priority on (T6).
        loop {
            // CR 800.4a: "If the player who left the game had priority at the time they left,
            // priority passes to the next player in turn order who's still in the game."
            if host.has_left_game(player) {
                match next_player_in_turn_order(host, player) {
                    Some(next) => player = next,
                    None => {
                        set_priority(host, None)?;
                        return Ok(HeartbeatOutcome::NoPlayerInGame);
                    }
                }
            }
            set_priority(host, Some(player))?;
            let response = ask_priority(host, player, choices)?;
            if host.has_left_game(player) {
                // CR 800.4a: the player left while deciding (a host that models leaving at any
                // time); their answer is moot and priority passes on at the top of the loop.
                continue;
            }
            if response != ChoiceResponse::PassPriority {
                // T5, CR 117.3c: "If a player has priority when they cast a spell, activate an
                // ability, or take a special action, that player receives priority afterward."
                // CR 116.3, CR 605.3b: special actions and mana abilities too; any action ends
                // the pass sequence (CR 117.4: "without taking any actions in between").
                host.take_action(player, &response, choices)?;
                state.passed.clear();
                intended = player;
                break;
            }
            // CR 117.3d: "If a player has priority and chooses not to take any actions, that
            // player passes."
            state.passed.insert(player);
            state.forget_departed(host);
            let in_game = players_in_game(host);
            let count = state.pass_count();
            let stack_empty = host.stack_is_empty();
            let result = if count < in_game {
                // CR 117.3d: "Then the next player in turn order receives priority."
                match next_player_in_turn_order(host, player) {
                    Some(to) => PassResult::PriorityMoved { to },
                    None => {
                        return Err(procedure_error(
                            "heartbeat: no next player in turn order after a pass".to_owned(),
                        ));
                    }
                }
            } else if stack_empty {
                PassResult::StepEnds
            } else {
                PassResult::TopResolved
            };
            let (phase, step) = host.phase_and_step();
            state.passes.push(PassRecord {
                sequence: u64::try_from(state.passes.len()).unwrap_or(u64::MAX),
                turn_number: host.game().turn().turn_number(),
                phase,
                step,
                player,
                pass_count: count,
                players_in_game: in_game,
                stack_empty,
                result,
            });
            match result {
                PassResult::PriorityMoved { to } => player = to,
                PassResult::TopResolved => {
                    // T7, CR 117.4: "the spell or ability on top of the stack resolves".
                    host.resolve_top(choices)?;
                    state.passed.clear();
                    // CR 117.3b: "The active player receives priority after a spell or ability
                    // (other than a mana ability) resolves." CR 800.4j: or the next player.
                    match active_or_next(host) {
                        Some(next) => intended = next,
                        None => {
                            set_priority(host, None)?;
                            return Ok(HeartbeatOutcome::NoPlayerInGame);
                        }
                    }
                    break;
                }
                PassResult::StepEnds => {
                    // T8, CR 117.4: "if the stack is empty, the phase or step ends." CR 500.5
                    // belongs to the turn sequencer.
                    set_priority(host, None)?;
                    return Ok(HeartbeatOutcome::StepEnds);
                }
            }
        }
    }
}
