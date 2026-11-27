// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the priority-heartbeat spec tests (REQ-TURN-001, REQ-SBA-001; CR 117.3a-d,
//! 117.4, 117.5, 704.3, 603.3b, 800.4a, 800.4j). Black-box: written from the work-item brief
//! only.
//!
//! Each `spec_turn_heartbeat*.rs` file includes this file with `#[path]`. Nothing here is card
//! data (R5): actions name synthetic object ids and `test.*` ability ids.
//!
//! A [`FakeHost`] owns a real `GameState` and scripted queues: one [`SbaStep`] per `sba_pass`
//! call, one [`PlaceStep`] per `place_triggers` call, one [`HookStep`] per `take_action` and per
//! `resolve_top` call, a counter for the stack depth and a static list of legal actions per
//! player. Every mutating host call is logged with the number of asks made before it in the
//! run ([`ChoiceContext::next_request_id`]), so [`trace`] can interleave host calls and asks in
//! the exact order they happened. A [`World`] (host + `HeartbeatState`) is the `Transactional`
//! state of a `ChoiceSession` running `run_heartbeat`; [`drive`] answers its requests with a
//! policy and keeps the state each run ended in (the state at each ask).

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Debug;
use std::rc::Rc;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, Status,
};
use fineprint_core::events::{Phase, Step};
use fineprint_core::replacement::{BusError, BusInterrupt};
use fineprint_core::sba::{LOST_THE_GAME, SbaFinding, SbaOutcome, SbaSubject};
use fineprint_core::state::{DesignationValue, GameState};
use fineprint_core::triggers::{
    PendingTrigger, RemovedReason, RemovedTrigger, StackedTrigger, TriggerOrigin, TriggerPlacement,
};
use fineprint_core::turn::{
    HeartbeatHost, HeartbeatOutcome, HeartbeatState, run_heartbeat, settle_before_priority,
};
use fineprint_core::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// ids and actions
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
pub const P4: PlayerId = PlayerId::new(4);

/// The turn number every fake game is at.
pub const TURN: u64 = 3;

/// The ability id of a synthetic mana ability (CR 605: it does not use the stack).
pub const MANA_ABILITY: &str = "test.mana_ability";
/// The ability id of a synthetic non-mana activated ability (it uses the stack).
pub const STACK_ABILITY: &str = "test.stack_ability";

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn pass() -> ChoiceResponse {
    ChoiceResponse::PassPriority
}

/// `PLAY_LAND` (a special action, CR 116.2a; no stack).
pub fn land(o: u64) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: oid(o),
        face_index: 0,
    }
}

/// `INITIATE_CAST`: the fake puts one object on the stack.
pub fn cast(o: u64) -> ChoiceResponse {
    ChoiceResponse::InitiateCast {
        object: oid(o),
        face_index: 0,
        cast_mode: "test.normal".to_string(),
    }
}

/// `ACTIVATE` of a non-mana ability: the fake puts one object on the stack.
pub fn activate(o: u64) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: oid(o),
        ability_id: STACK_ABILITY.to_string(),
    }
}

/// `ACTIVATE` of a mana ability (CR 605.3b: resolves immediately, no stack).
pub fn mana(o: u64) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: oid(o),
        ability_id: MANA_ABILITY.to_string(),
    }
}

/// `SPECIAL_ACTION` (CR 116.1: no stack).
pub fn special(o: u64) -> ChoiceResponse {
    ChoiceResponse::SpecialAction {
        kind: "test.special".to_string(),
        object: oid(o),
    }
}

/// Whether the fake's `take_action` puts an object on the stack for `action`.
pub fn pushes_stack(action: &ChoiceResponse) -> bool {
    match action {
        ChoiceResponse::InitiateCast { .. } => true,
        ChoiceResponse::Activate { ability_id, .. } => ability_id != MANA_ABILITY,
        _ => false,
    }
}

/// The answer to a host request (`CHOOSE_PLAYER` with one candidate).
pub fn host_answer(p: PlayerId) -> ChoiceResponse {
    ChoiceResponse::SubmitPlayer { player: p }
}

// ---------------------------------------------------------------------------------------------
// scripts
// ---------------------------------------------------------------------------------------------

/// What one `sba_pass` call does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SbaStep {
    /// Nothing applicable: `performed == false`, no findings.
    Nothing,
    /// A finding whose action changed nothing: `performed == false`, findings non-empty.
    FindingsOnly,
    /// Something was performed: `performed == true`.
    Performed,
    /// These players lose the game (LOST_THE_GAME designation set): `performed == true`.
    Lose(Vec<PlayerId>),
    /// Asks the active player (or the first player) a host request, then reports
    /// `performed == true` once answered.
    Ask,
    /// Fails with `BusInterrupt::Error`.
    Error,
}

/// What one `place_triggers` call does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceStep {
    /// Puts `stacked` triggers on the stack and removes `removed` (CR 603.3c-d).
    Place { stacked: usize, removed: usize },
    /// Asks a host request, then places nothing.
    Ask,
    /// Fails with `Interrupt::Error(ChoiceError::Procedure(..))`.
    Error,
}

/// What one `take_action` or `resolve_top` call does besides its normal effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookStep {
    /// Only the normal effect.
    Plain,
    /// These players lose the game.
    Lose(Vec<PlayerId>),
    /// Asks a host request first.
    Ask,
    /// Fails with `Interrupt::Error(ChoiceError::Procedure(..))` before any effect.
    Error,
}

/// The error the fake's `Error` steps return through `Interrupt::Error`.
pub fn fake_error() -> ChoiceError {
    ChoiceError::Procedure("test.fake_host_failure".to_string())
}

/// One mutating host call, as logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Sba,
    Place(Vec<PlayerId>),
    Act(PlayerId, ChoiceResponse),
    Resolve,
}

// ---------------------------------------------------------------------------------------------
// the fake host
// ---------------------------------------------------------------------------------------------

/// A scripted heartbeat host over a real `GameState` (see the module documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeHost {
    pub game: GameState,
    pub phase: (Option<Phase>, Option<Step>),
    pub sba: VecDeque<SbaStep>,
    /// Every `sba_pass` reports `performed == true` (settle guard tests).
    pub sba_forever: bool,
    pub place: VecDeque<PlaceStep>,
    /// Every `place_triggers` stacks one trigger (settle guard tests).
    pub place_forever: bool,
    pub action_steps: VecDeque<HookStep>,
    pub resolve_steps: VecDeque<HookStep>,
    /// The stack depth (objects on the stack).
    pub depth: u64,
    /// The legal non-pass actions per player (static).
    pub actions: BTreeMap<PlayerId, Vec<ChoiceResponse>>,
    /// `(asks made before the call, call)`, in call order.
    pub log: Vec<(u64, Call)>,
    /// Contract violations seen by the fake (expected empty).
    pub anomalies: Vec<String>,
    /// Totals.
    pub sba_calls: usize,
    pub place_calls: usize,
    pub triggers_stacked: u64,
    pub action_pushes: u64,
    pub resolutions: u64,
    /// One entry per `priority_actions` call (the heartbeat collecting the actions of the
    /// player about to be asked): `Some(p)` makes `p` leave the game (concede) at that moment,
    /// possibly the player being asked (CR 800.4a). Consumed in call order within a run.
    pub concede_on_ask: RefCell<VecDeque<Option<PlayerId>>>,
    /// The players who conceded through `concede_on_ask`.
    pub conceded: RefCell<BTreeSet<PlayerId>>,
    /// The players still in the game as each `resolve_top` call began, in call order.
    pub live_at_resolve: Vec<BTreeSet<PlayerId>>,
}

impl FakeHost {
    /// `players` players (ids 1..=players, turn order = seat order) at turn [`TURN`], precombat
    /// main phase, active player `active`, empty stack, empty scripts.
    pub fn new(players: usize, active: Option<PlayerId>) -> FakeHost {
        let mut game = GameState::new(0x0044_FEED, players, 20).expect("valid player count");
        game.set_turn_number(TURN);
        game.set_active_player(active).expect("known player");
        FakeHost {
            game,
            phase: (Some(Phase::PrecombatMain), None),
            sba: VecDeque::new(),
            sba_forever: false,
            place: VecDeque::new(),
            place_forever: false,
            action_steps: VecDeque::new(),
            resolve_steps: VecDeque::new(),
            depth: 0,
            actions: BTreeMap::new(),
            log: Vec::new(),
            anomalies: Vec::new(),
            sba_calls: 0,
            place_calls: 0,
            triggers_stacked: 0,
            action_pushes: 0,
            resolutions: 0,
            concede_on_ask: RefCell::new(VecDeque::new()),
            conceded: RefCell::new(BTreeSet::new()),
            live_at_resolve: Vec::new(),
        }
    }

    /// Marks `p` as having lost the game (LOST_THE_GAME designation, CR 800.4a).
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
            .is_some()
    }

    pub fn with_sba(mut self, steps: Vec<SbaStep>) -> FakeHost {
        self.sba = steps.into();
        self
    }

    pub fn with_place(mut self, steps: Vec<PlaceStep>) -> FakeHost {
        self.place = steps.into();
        self
    }

    pub fn with_actions(mut self, p: PlayerId, actions: Vec<ChoiceResponse>) -> FakeHost {
        self.actions.insert(p, actions);
        self
    }

    pub fn with_depth(mut self, depth: u64) -> FakeHost {
        self.depth = depth;
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.log.iter().map(|(_, c)| c.clone()).collect()
    }

    fn log_call(&mut self, choices: &ChoiceContext<'_>, call: Call) {
        self.log.push((choices.next_request_id().get(), call));
    }

    /// The players still in the game, in turn order.
    pub fn live(&self) -> Vec<PlayerId> {
        self.game
            .turn_order()
            .iter()
            .copied()
            .filter(|&p| !HeartbeatHost::has_left_game(self, p))
            .collect()
    }

    /// The player a host request goes to (F21): the active player if still in the game, else
    /// the next player in turn order after the active player's seat who is (CR 800.4j), else
    /// the first player still in the game; with nobody left, the active or first player.
    fn host_chooser(&self) -> PlayerId {
        let order = self.game.turn_order();
        let fallback = self.game.turn().active_player().unwrap_or(order[0]);
        let live = |p: PlayerId| !HeartbeatHost::has_left_game(self, p);
        match self.game.turn().active_player() {
            Some(a) if live(a) => a,
            Some(a) => {
                let seat = order.iter().position(|&p| p == a).unwrap_or(0);
                (1..=order.len())
                    .map(|i| order[(seat + i) % order.len()])
                    .find(|&p| live(p))
                    .unwrap_or(fallback)
            }
            None => order.iter().copied().find(|&p| live(p)).unwrap_or(fallback),
        }
    }

    /// The static legal actions of `player` (no side effect).
    fn actions_of(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        self.actions.get(&player).cloned().unwrap_or_default()
    }

    fn lose_all(&mut self, players: &[PlayerId]) {
        for &p in players {
            self.mark_lost(p);
        }
    }
}

/// A one-candidate `CHOOSE_PLAYER` request with the next request id (a valid host request).
pub fn host_request(choices: &ChoiceContext<'_>, chooser: PlayerId) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser,
        choice_type: ChoiceType::ChoosePlayer,
        reason: ChoiceReason {
            cr: "CR 704.3".to_string(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: host_answer(chooser),
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Player(chooser)]),
        payload: ChoicePayload::ChoosePlayer {
            players: vec![chooser],
        },
    }
}

/// An SBA outcome with only `performed`, `findings` and `lost_players` set.
pub fn sba_outcome(performed: bool, findings: Vec<SbaFinding>, lost: Vec<PlayerId>) -> SbaOutcome {
    let mut loss_reasons = BTreeMap::new();
    for &p in &lost {
        loss_reasons.insert(p, vec!["704.5a"]);
    }
    SbaOutcome {
        performed,
        findings,
        executed: Vec::new(),
        lost_players: lost,
        loss_reasons,
        ceased: Vec::new(),
        unattached: Vec::new(),
        removed_from_game: Vec::new(),
        protectors_chosen: Vec::new(),
        speed_set: Vec::new(),
    }
}

/// A synthetic pending trigger controlled by `controller` (no targets, no modes).
pub fn pending_trigger(controller: PlayerId, n: u64) -> PendingTrigger {
    PendingTrigger {
        origin: TriggerOrigin::Event,
        controller,
        source: Some(oid(900 + n)),
        ability_id: format!("test.trigger_{n}"),
        bindings: BTreeMap::new(),
        optional: false,
        responds_to_trigger: false,
        targets: Vec::new(),
        modes: None,
    }
}

/// A placement with `stacked` triggers put on the stack and `removed` removed.
pub fn placement(controller: PlayerId, stacked: usize, removed: usize) -> TriggerPlacement {
    let mut n = 0u64;
    let mut next = || {
        n += 1;
        pending_trigger(controller, n)
    };
    let stacked = (0..stacked)
        .map(|_| StackedTrigger {
            trigger: next(),
            modes: Vec::new(),
            targets: Vec::new(),
        })
        .collect();
    let removed = (0..removed)
        .map(|_| RemovedTrigger {
            trigger: next(),
            reason: RemovedReason::NoLegalTargets,
        })
        .collect();
    TriggerPlacement { stacked, removed }
}

fn bus_from(i: Interrupt) -> BusInterrupt {
    match i {
        Interrupt::Pending(r) => BusInterrupt::Pending(r),
        Interrupt::Error(e) => BusInterrupt::Error(BusError::Choice(e)),
    }
}

impl HeartbeatHost for FakeHost {
    fn game(&self) -> &GameState {
        &self.game
    }

    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        self.phase
    }

    /// The LOST_THE_GAME designation (the trait default) or a concession through
    /// `concede_on_ask`.
    fn has_left_game(&self, player: PlayerId) -> bool {
        self.conceded.borrow().contains(&player)
            || self
                .game
                .player(player)
                .is_none_or(|p| p.designation(LOST_THE_GAME).is_some())
    }

    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.log_call(choices, Call::Sba);
        self.sba_calls += 1;
        if self.sba_forever {
            return Ok(sba_outcome(true, Vec::new(), Vec::new()));
        }
        match self.sba.pop_front().unwrap_or(SbaStep::Nothing) {
            SbaStep::Nothing => Ok(sba_outcome(false, Vec::new(), Vec::new())),
            SbaStep::FindingsOnly => Ok(sba_outcome(
                false,
                vec![SbaFinding {
                    entry_id: "704.5g",
                    subject: SbaSubject::Object(oid(77)),
                }],
                Vec::new(),
            )),
            SbaStep::Performed => Ok(sba_outcome(true, Vec::new(), Vec::new())),
            SbaStep::Lose(players) => {
                self.lose_all(&players);
                Ok(sba_outcome(true, Vec::new(), players))
            }
            SbaStep::Ask => {
                let chooser = self.host_chooser();
                choices
                    .ask(host_request(choices, chooser))
                    .map_err(bus_from)?;
                Ok(sba_outcome(true, Vec::new(), Vec::new()))
            }
            SbaStep::Error => Err(BusInterrupt::Error(BusError::DepthExceeded)),
        }
    }

    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.log_call(choices, Call::Place(apnap.to_vec()));
        self.place_calls += 1;
        let controller = apnap.first().copied().unwrap_or(P1);
        let step = if self.place_forever {
            PlaceStep::Place {
                stacked: 1,
                removed: 0,
            }
        } else {
            self.place.pop_front().unwrap_or(PlaceStep::Place {
                stacked: 0,
                removed: 0,
            })
        };
        match step {
            PlaceStep::Place { stacked, removed } => {
                self.depth += stacked as u64;
                self.triggers_stacked += stacked as u64;
                Ok(placement(controller, stacked, removed))
            }
            PlaceStep::Ask => {
                let chooser = self.host_chooser();
                choices.ask(host_request(choices, chooser))?;
                Ok(TriggerPlacement::default())
            }
            PlaceStep::Error => Err(Interrupt::Error(fake_error())),
        }
    }

    fn stack_is_empty(&self) -> bool {
        self.depth == 0
    }

    fn resolve_top(&mut self, choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.log_call(choices, Call::Resolve);
        let live = self.live().into_iter().collect();
        self.live_at_resolve.push(live);
        match self.resolve_steps.pop_front().unwrap_or(HookStep::Plain) {
            HookStep::Plain => {}
            HookStep::Lose(players) => self.lose_all(&players),
            HookStep::Ask => {
                let chooser = self.host_chooser();
                choices.ask(host_request(choices, chooser))?;
            }
            HookStep::Error => return Err(Interrupt::Error(fake_error())),
        }
        if self.depth == 0 {
            self.anomalies
                .push("resolve_top called with an empty stack".to_string());
        } else {
            self.depth -= 1;
        }
        self.resolutions += 1;
        Ok(())
    }

    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        if let Some(Some(leaver)) = self.concede_on_ask.borrow_mut().pop_front() {
            self.conceded.borrow_mut().insert(leaver);
        }
        self.actions_of(player)
    }

    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log_call(choices, Call::Act(player, action.clone()));
        if !self.actions_of(player).contains(action) {
            self.anomalies
                .push(format!("take_action with an unlisted action {action:?}"));
        }
        match self.action_steps.pop_front().unwrap_or(HookStep::Plain) {
            HookStep::Plain => {}
            HookStep::Lose(players) => self.lose_all(&players),
            HookStep::Ask => {
                choices.ask(host_request(choices, player))?;
            }
            HookStep::Error => return Err(Interrupt::Error(fake_error())),
        }
        if pushes_stack(action) {
            self.depth += 1;
            self.action_pushes += 1;
        }
        Ok(())
    }
}

/// A host that is a [`FakeHost`] except that `leaver` leaves the game (concedes) at the moment
/// the heartbeat computes the actions of `when_asking` (that is, just before `when_asking` is
/// asked), between two passes, with no host call in between (CR 800.4a). It delegates every
/// method; `has_left_game` is the inner host's (the trait default) or the concession.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conceding {
    pub inner: FakeHost,
    pub when_asking: PlayerId,
    pub leaver: PlayerId,
    pub conceded: Cell<bool>,
}

impl HeartbeatHost for Conceding {
    fn game(&self) -> &GameState {
        self.inner.game()
    }

    fn game_mut(&mut self) -> &mut GameState {
        self.inner.game_mut()
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        self.inner.phase_and_step()
    }

    fn has_left_game(&self, player: PlayerId) -> bool {
        (self.conceded.get() && player == self.leaver) || self.inner.has_left_game(player)
    }

    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.inner.sba_pass(choices)
    }

    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.inner.place_triggers(apnap, choices)
    }

    fn stack_is_empty(&self) -> bool {
        self.inner.stack_is_empty()
    }

    fn resolve_top(&mut self, choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.inner.resolve_top(choices)
    }

    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        if player == self.when_asking {
            self.conceded.set(true);
        }
        self.inner.priority_actions(player)
    }

    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.inner.take_action(player, action, choices)
    }
}

/// A fake host usable in a [`World`].
pub trait Fake: HeartbeatHost + Clone + Debug + PartialEq {
    fn fake(&self) -> &FakeHost;
    fn fake_mut(&mut self) -> &mut FakeHost;
}

impl Fake for FakeHost {
    fn fake(&self) -> &FakeHost {
        self
    }
    fn fake_mut(&mut self) -> &mut FakeHost {
        self
    }
}

impl Fake for Conceding {
    fn fake(&self) -> &FakeHost {
        &self.inner
    }
    fn fake_mut(&mut self) -> &mut FakeHost {
        &mut self.inner
    }
}

// ---------------------------------------------------------------------------------------------
// the session state
// ---------------------------------------------------------------------------------------------

/// The heartbeat's output, made comparable by the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    StepEnds,
    NoPlayerInGame,
}

pub fn out_of(o: HeartbeatOutcome) -> Out {
    match o {
        HeartbeatOutcome::StepEnds => Out::StepEnds,
        HeartbeatOutcome::NoPlayerInGame => Out::NoPlayerInGame,
    }
}

/// A procedure's output: the heartbeat's outcome, or its error kept as data.
pub type RunOut = Result<Out, ChoiceError>;

/// Host + heartbeat state: the `Transactional` state of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World<H> {
    pub host: H,
    pub hb: HeartbeatState,
    /// When non-empty, the procedure runs one `run_heartbeat` per entry with the host's phase
    /// and step set to it first (successive steps sharing one `HeartbeatState`).
    pub rounds: Vec<(Option<Phase>, Option<Step>)>,
    /// The outcome of each finished `run_heartbeat` of the run.
    pub outs: Vec<Out>,
}

impl<H: Fake> World<H> {
    pub fn new(host: H) -> World<H> {
        World {
            host,
            hb: HeartbeatState::new(),
            rounds: Vec::new(),
            outs: Vec::new(),
        }
    }

    pub fn fake(&self) -> &FakeHost {
        self.host.fake()
    }
}

impl<H: Fake> Transactional for World<H> {
    fn state_hash(&self) -> Digest {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.host.fake().game.state_hash().as_bytes());
        bytes
            .extend_from_slice(format!("{:?}|{:?}|{:?}", self.host, self.hb, self.outs).as_bytes());
        sha256(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.host.fake().game.turn().priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// The procedure: `run_heartbeat` once (or once per round), errors kept as data.
pub fn heartbeat_proc<H: Fake>(
    w: &mut World<H>,
    c: &mut ChoiceContext<'_>,
) -> Result<RunOut, Interrupt> {
    let rounds = if w.rounds.is_empty() {
        vec![None]
    } else {
        w.rounds.iter().copied().map(Some).collect()
    };
    let mut last = Out::StepEnds;
    for phase in rounds {
        if let Some(p) = phase {
            w.host.fake_mut().phase = p;
        }
        match run_heartbeat(&mut w.host, &mut w.hb, c) {
            Ok(o) => {
                last = out_of(o);
                w.outs.push(last);
            }
            Err(Interrupt::Pending(r)) => return Err(Interrupt::Pending(r)),
            Err(Interrupt::Error(e)) => return Ok(Err(e)),
        }
    }
    Ok(Ok(last))
}

/// The procedure: `settle_before_priority` once, the report as `(sba_passes,
/// triggers_stacked)`, errors kept as data.
pub fn settle_proc<H: Fake>(
    w: &mut World<H>,
    c: &mut ChoiceContext<'_>,
) -> Result<Result<(usize, usize), ChoiceError>, Interrupt> {
    match settle_before_priority(&mut w.host, c) {
        Ok(r) => Ok(Ok((r.sba_passes, r.triggers_stacked))),
        Err(Interrupt::Pending(r)) => Err(Interrupt::Pending(r)),
        Err(Interrupt::Error(e)) => Ok(Err(e)),
    }
}

// ---------------------------------------------------------------------------------------------
// driving a session
// ---------------------------------------------------------------------------------------------

/// The most requests one driven session may publish.
pub const MAX_ASKS: usize = 2_000;

/// The result of driving a session.
pub struct Run<H, O> {
    /// Every request published, in order (request id = index).
    pub requests: Vec<PendingChoiceRequest>,
    /// The state each request was asked in (the end state of the run that suspended on it).
    pub at_ask: Vec<World<H>>,
    /// The accepted answers.
    pub answers: Vec<ChoiceResponse>,
    /// The state the last run ended in (final state if done).
    pub last: World<H>,
    /// The output, if the procedure finished.
    pub out: Option<O>,
    /// The session's state hash at the end.
    pub hash: Digest,
}

/// Drives `procedure` over `base`, answering each request with `policy(request, state at the
/// ask)`; stops (leaving the request pending) when the policy returns `None`.
pub fn drive_with<H, O, P>(
    base: World<H>,
    procedure: P,
    mut policy: impl FnMut(&PendingChoiceRequest, &World<H>) -> Option<ChoiceResponse>,
) -> Run<H, O>
where
    H: Fake,
    O: Clone,
    P: Fn(&mut World<H>, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let side: Rc<RefCell<Option<World<H>>>> = Rc::new(RefCell::new(None));
    let channel = Rc::clone(&side);
    let wrapped = move |w: &mut World<H>, c: &mut ChoiceContext<'_>| {
        let r = procedure(w, c);
        *channel.borrow_mut() = Some(w.clone());
        r
    };
    let mut session = match ChoiceSession::start(base, wrapped) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    let mut requests = Vec::new();
    let mut at_ask = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < MAX_ASKS, "the session did not terminate");
        let world = side.borrow().clone().expect("a run happened");
        requests.push(req.clone());
        at_ask.push(world.clone());
        let Some(answer) = policy(&req, &world) else {
            break;
        };
        if let Err(e) = session.submit(req.request_id, answer.clone()) {
            panic!("answer {answer:?} to {req:?} rejected: {e:?}");
        }
    }
    let last = side.borrow().clone().expect("a run happened");
    let out = match session.status() {
        Status::Done { output, .. } => Some(output.clone()),
        Status::Pending(_) => None,
    };
    Run {
        requests,
        at_ask,
        answers: session.answers().to_vec(),
        last,
        out,
        hash: session.state_hash(),
    }
}

/// [`drive_with`] over [`heartbeat_proc`].
pub fn drive<H: Fake>(
    base: World<H>,
    policy: impl FnMut(&PendingChoiceRequest, &World<H>) -> Option<ChoiceResponse>,
) -> Run<H, RunOut> {
    drive_with(base, heartbeat_proc::<H>, policy)
}

/// A policy answering the requests with `answers` in order, then stopping.
pub fn script<H>(
    answers: Vec<ChoiceResponse>,
) -> impl FnMut(&PendingChoiceRequest, &World<H>) -> Option<ChoiceResponse> {
    let mut queue: VecDeque<ChoiceResponse> = answers.into();
    move |_, _| queue.pop_front()
}

/// Drives the heartbeat over `host` with the scripted `answers`.
pub fn run_script<H: Fake>(host: H, answers: Vec<ChoiceResponse>) -> Run<H, RunOut> {
    drive(World::new(host), script(answers))
}

/// Replays `answers` all at once over `base`; returns the final hash and state.
pub fn replay_all<H: Fake>(base: World<H>, answers: &[ChoiceResponse]) -> (Digest, World<H>) {
    let session = match ChoiceSession::replay(base, heartbeat_proc::<H>, answers) {
        Ok(s) => s,
        Err(e) => panic!("replay failed: {e:?}"),
    };
    let hash = session.state_hash();
    match session.status() {
        Status::Done { state, .. } => (hash, state.clone()),
        Status::Pending(_) => (hash, session.base().clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// traces
// ---------------------------------------------------------------------------------------------

/// One event of a run, host calls and asks interleaved in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ev {
    Sba,
    Place(Vec<PlayerId>),
    /// A `PRIORITY_ACTION` request to this player.
    Ask(PlayerId),
    /// Any other request (a host request) to this player.
    HostAsk(PlayerId),
    Act(PlayerId, ChoiceResponse),
    Resolve,
}

fn ask_ev(r: &PendingChoiceRequest) -> Ev {
    if r.choice_type == ChoiceType::PriorityAction {
        Ev::Ask(r.chooser)
    } else {
        Ev::HostAsk(r.chooser)
    }
}

/// The run's events in order: the last run's host calls with every published request inserted
/// at the position it was asked.
pub fn trace<H: Fake, O>(run: &Run<H, O>) -> Vec<Ev> {
    let mut out = Vec::new();
    let mut next = 0usize;
    for (stamp, call) in &run.last.fake().log {
        while (next as u64) < *stamp && next < run.requests.len() {
            out.push(ask_ev(&run.requests[next]));
            next += 1;
        }
        out.push(match call {
            Call::Sba => Ev::Sba,
            Call::Place(a) => Ev::Place(a.clone()),
            Call::Act(p, r) => Ev::Act(*p, r.clone()),
            Call::Resolve => Ev::Resolve,
        });
    }
    while next < run.requests.len() {
        out.push(ask_ev(&run.requests[next]));
        next += 1;
    }
    out
}

/// `[Sba, Place(apnap)]`: one settle where nothing happens.
pub fn quiet_settle(apnap: &[PlayerId]) -> Vec<Ev> {
    vec![Ev::Sba, Ev::Place(apnap.to_vec())]
}

/// Concatenates event slices.
pub fn evs(parts: &[&[Ev]]) -> Vec<Ev> {
    parts.iter().flat_map(|p| p.iter().cloned()).collect()
}

/// The number of `PassPriority` answers.
pub fn pass_answers(answers: &[ChoiceResponse]) -> usize {
    answers
        .iter()
        .filter(|a| matches!(a, ChoiceResponse::PassPriority))
        .count()
}

// ---------------------------------------------------------------------------------------------
// seeded generator (R3: no system randomness)
// ---------------------------------------------------------------------------------------------

/// A 64-bit LCG (Knuth MMIX constants) with a fixed seed.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x0044))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Uniform-ish in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
}
