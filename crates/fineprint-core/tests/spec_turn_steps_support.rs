// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the turn-sequencer spec tests (REQ-TURN-002; CR 500.1, 500.4, 500.5,
//! 502.1-502.4, 504.1, 504.2, 103.8a, 103.8c, 505.4, 514.1-514.3a, 703.1). Black-box: written
//! from the work-item brief only.
//!
//! Each `spec_turn_steps*.rs` file includes this file with `#[path]`. Nothing here is card data
//! (R5): every object is a plain synthetic id, every ability id is `test.*`.
//!
//! A [`TurnFake`] owns a real `GameState` and records every host call (and every priority ask,
//! through `priority_actions`, which the heartbeat calls once just before each `PRIORITY_ACTION`
//! request) in one ordered log of [`TC`] values. Scripts decide what the settle finds: a
//! queue of placement sizes, and a per-cleanup-step queue of [`Work`] armed by each
//! `end_of_turn_cleanup` call. A [`World`] (host + `TurnState`) is the `Transactional` state of
//! a `ChoiceSession` running one of the sequencer entry points ([`Proc`]).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, Status,
};
use fineprint_core::events::{Phase, Step};
use fineprint_core::format::FirstTurnDrawRule;
use fineprint_core::replacement::BusInterrupt;
use fineprint_core::sba::SbaOutcome;
use fineprint_core::state::GameState;
use fineprint_core::triggers::{PendingTrigger, StackedTrigger, TriggerOrigin, TriggerPlacement};
use fineprint_core::turn::heartbeat::HeartbeatHost;
use fineprint_core::turn::{
    ExpiryPoint, Segment, SegmentReport, TurnBasedRegistry, TurnHost, TurnReport, TurnState,
    UntapOptions, run_phase, run_segment, run_turn,
};
use fineprint_core::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn oids(ns: &[u64]) -> Vec<ObjectId> {
    ns.iter().copied().map(oid).collect()
}

/// The first id a drawn card gets (then +1 per draw).
pub const FIRST_DRAWN: u64 = 5_000;

// ---------------------------------------------------------------------------------------------
// the log
// ---------------------------------------------------------------------------------------------

/// One host call (or priority ask), as logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TC {
    BeginTurn(PlayerId),
    BeginPhase(Phase),
    BeginStep(Step),
    Expire(ExpiryPoint),
    EmptyMana,
    Phasing(PlayerId),
    DayNight(Option<PlayerId>),
    UntapOptions(PlayerId),
    Untap(PlayerId, Vec<ObjectId>),
    Draw(PlayerId),
    Discard(PlayerId, Vec<ObjectId>),
    EotCleanup,
    Lore(Vec<ObjectId>),
    Combat(Step),
    Sba,
    Place(Vec<PlayerId>),
    /// The heartbeat is about to ask this player a `PRIORITY_ACTION` request.
    Prio(PlayerId),
    Resolve,
    Act(PlayerId),
}

/// What the cleanup-step settle finds, armed by one `end_of_turn_cleanup` call (CR 514.3a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// The next `place_triggers` call stacks one trigger.
    Trigger,
    /// The next `sba_pass` call reports `performed == true`.
    Sba,
    /// An object is put on the stack (the stack is non-empty at the check).
    Stack,
}

// ---------------------------------------------------------------------------------------------
// the fake host
// ---------------------------------------------------------------------------------------------

/// A recording, scripted turn host over a real `GameState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnFake {
    pub game: GameState,
    pub phase: (Option<Phase>, Option<Step>),
    pub log: RefCell<Vec<TC>>,
    pub anomalies: Vec<String>,
    // configuration
    pub draw_rule: FirstTurnDrawRule,
    pub starting: Option<PlayerId>,
    pub first_strike: bool,
    pub untap_opts: BTreeMap<PlayerId, UntapOptions>,
    pub hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    pub max_hand: BTreeMap<PlayerId, Option<u64>>,
    pub sagas: BTreeMap<PlayerId, Vec<ObjectId>>,
    /// `draw_card` first asks a host request (`CHOOSE_PLAYER`).
    pub draw_asks: bool,
    /// Every player has left the game (`has_left_game` is always true).
    pub everyone_left: bool,
    // scripts
    /// One entry per `place_triggers` call (triggers stacked); empty = 0.
    pub place: VecDeque<usize>,
    /// One entry per `end_of_turn_cleanup` call; `None` or empty = nothing to do.
    pub cleanup_work: VecDeque<Option<Work>>,
    /// Every `end_of_turn_cleanup` call arms this work (guard tests).
    pub cleanup_work_forever: Option<Work>,
    // runtime
    pub sba_armed: bool,
    pub place_armed: bool,
    pub depth: u64,
    pub next_card: u64,
}

impl TurnFake {
    /// `players` players (ids 1..=players), turn 0, no active player, P1 starts, CR default
    /// draw rule, no first strike, empty hands, maximum hand size 7, nothing scripted.
    pub fn new(players: usize) -> TurnFake {
        let game = GameState::new(0x0046_5EED, players, 20).expect("valid player count");
        let mut max_hand = BTreeMap::new();
        for i in 1..=players {
            max_hand.insert(PlayerId::new(i as u64), Some(7));
        }
        TurnFake {
            game,
            phase: (None, None),
            log: RefCell::new(Vec::new()),
            anomalies: Vec::new(),
            draw_rule: FirstTurnDrawRule::CrDefault,
            starting: Some(P1),
            first_strike: false,
            untap_opts: BTreeMap::new(),
            hands: BTreeMap::new(),
            max_hand,
            sagas: BTreeMap::new(),
            draw_asks: false,
            everyone_left: false,
            place: VecDeque::new(),
            cleanup_work: VecDeque::new(),
            cleanup_work_forever: None,
            sba_armed: false,
            place_armed: false,
            depth: 0,
            next_card: FIRST_DRAWN,
        }
    }

    /// Sets the turn number and the active player directly (fixture for `run_phase` /
    /// `run_segment`).
    pub fn at(mut self, turn: u64, active: Option<PlayerId>) -> TurnFake {
        self.game.set_turn_number(turn);
        self.game.set_active_player(active).expect("known player");
        self
    }

    pub fn with_hand(mut self, p: PlayerId, cards: Vec<ObjectId>) -> TurnFake {
        self.hands.insert(p, cards);
        self
    }

    pub fn with_max(mut self, p: PlayerId, max: Option<u64>) -> TurnFake {
        self.max_hand.insert(p, max);
        self
    }

    pub fn with_untap(
        mut self,
        p: PlayerId,
        forced: Vec<ObjectId>,
        choosable: Vec<ObjectId>,
    ) -> TurnFake {
        self.untap_opts
            .insert(p, UntapOptions { forced, choosable });
        self
    }

    pub fn with_sagas(mut self, p: PlayerId, sagas: Vec<ObjectId>) -> TurnFake {
        self.sagas.insert(p, sagas);
        self
    }

    pub fn with_cleanup_work(mut self, work: Vec<Option<Work>>) -> TurnFake {
        self.cleanup_work = work.into();
        self
    }

    pub fn with_place(mut self, place: Vec<usize>) -> TurnFake {
        self.place = place.into();
        self
    }

    pub fn calls(&self) -> Vec<TC> {
        self.log.borrow().clone()
    }

    fn push(&self, c: TC) {
        self.log.borrow_mut().push(c);
    }

    fn active_or_first(&self) -> PlayerId {
        self.game
            .turn()
            .active_player()
            .unwrap_or(self.game.turn_order()[0])
    }
}

/// A one-candidate `CHOOSE_PLAYER` host request with the next request id.
pub fn host_request(choices: &ChoiceContext<'_>, chooser: PlayerId) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser,
        choice_type: ChoiceType::ChoosePlayer,
        reason: ChoiceReason {
            cr: "CR 504.1".to_string(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitPlayer { player: chooser },
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Player(chooser)]),
        payload: ChoicePayload::ChoosePlayer {
            players: vec![chooser],
        },
    }
}

fn sba_outcome(performed: bool) -> SbaOutcome {
    SbaOutcome {
        performed,
        findings: Vec::new(),
        executed: Vec::new(),
        lost_players: Vec::new(),
        loss_reasons: BTreeMap::new(),
        ceased: Vec::new(),
        unattached: Vec::new(),
        removed_from_game: Vec::new(),
        protectors_chosen: Vec::new(),
        speed_set: Vec::new(),
    }
}

fn placement(controller: PlayerId, stacked: usize) -> TriggerPlacement {
    let stacked = (0..stacked)
        .map(|n| StackedTrigger {
            trigger: PendingTrigger {
                origin: TriggerOrigin::Event,
                controller,
                source: Some(oid(900 + n as u64)),
                ability_id: format!("test.trigger_{n}"),
                bindings: BTreeMap::new(),
                optional: false,
                responds_to_trigger: false,
                targets: Vec::new(),
                modes: None,
            },
            modes: Vec::new(),
            targets: Vec::new(),
        })
        .collect();
    TriggerPlacement {
        stacked,
        removed: Vec::new(),
    }
}

impl HeartbeatHost for TurnFake {
    fn game(&self) -> &GameState {
        &self.game
    }

    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        self.phase
    }

    fn has_left_game(&self, player: PlayerId) -> bool {
        self.everyone_left || self.game.player(player).is_none()
    }

    fn sba_pass(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.push(TC::Sba);
        let performed = std::mem::replace(&mut self.sba_armed, false);
        Ok(sba_outcome(performed))
    }

    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.push(TC::Place(apnap.to_vec()));
        let n = if std::mem::replace(&mut self.place_armed, false) {
            1
        } else {
            self.place.pop_front().unwrap_or(0)
        };
        self.depth += n as u64;
        Ok(placement(apnap.first().copied().unwrap_or(P1), n))
    }

    fn stack_is_empty(&self) -> bool {
        self.depth == 0
    }

    fn resolve_top(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.push(TC::Resolve);
        if self.depth == 0 {
            self.anomalies
                .push("resolve_top with an empty stack".to_string());
        } else {
            self.depth -= 1;
        }
        Ok(())
    }

    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        self.push(TC::Prio(player));
        Vec::new()
    }

    fn take_action(
        &mut self,
        player: PlayerId,
        _action: &ChoiceResponse,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.push(TC::Act(player));
        self.anomalies
            .push("take_action with no legal action".to_string());
        Ok(())
    }
}

impl TurnHost for TurnFake {
    fn begin_turn(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.push(TC::BeginTurn(active));
        let n = self.game.turn().turn_number();
        self.game.set_turn_number(n + 1);
        self.game
            .set_active_player(Some(active))
            .map_err(|e| Interrupt::Error(ChoiceError::Procedure(format!("{e}"))))?;
        self.phase = (None, None);
        Ok(())
    }

    fn begin_phase(&mut self, phase: Phase) -> Result<(), Interrupt> {
        self.push(TC::BeginPhase(phase));
        self.phase = (Some(phase), None);
        Ok(())
    }

    fn begin_step(&mut self, step: Step) -> Result<(), Interrupt> {
        self.push(TC::BeginStep(step));
        self.phase.1 = Some(step);
        Ok(())
    }

    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt> {
        self.push(TC::Expire(at));
        Ok(())
    }

    fn empty_mana_pools(&mut self) -> Result<(), Interrupt> {
        self.push(TC::EmptyMana);
        Ok(())
    }

    fn apply_phasing(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.push(TC::Phasing(active));
        Ok(())
    }

    fn check_day_night(&mut self, previous_active: Option<PlayerId>) -> Result<(), Interrupt> {
        self.push(TC::DayNight(previous_active));
        Ok(())
    }

    fn untap_options(&self, active: PlayerId) -> UntapOptions {
        self.push(TC::UntapOptions(active));
        self.untap_opts.get(&active).cloned().unwrap_or_default()
    }

    fn untap(
        &mut self,
        active: PlayerId,
        permanents: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.push(TC::Untap(active, permanents.to_vec()));
        Ok(())
    }

    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule {
        self.draw_rule
    }

    fn starting_player(&self) -> Option<PlayerId> {
        self.starting
    }

    fn draw_card(
        &mut self,
        player: PlayerId,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        if self.draw_asks {
            choices.ask(host_request(choices, player))?;
        }
        self.push(TC::Draw(player));
        let card = oid(self.next_card);
        self.next_card += 1;
        let hand = self.hands.entry(player).or_default();
        hand.push(card);
        hand.sort();
        Ok(())
    }

    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        let mut h = self.hands.get(&player).cloned().unwrap_or_default();
        h.sort();
        h
    }

    fn max_hand_size(&self, player: PlayerId) -> Option<u64> {
        self.max_hand.get(&player).copied().flatten()
    }

    fn discard_cards(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.push(TC::Discard(player, cards.to_vec()));
        let hand = self.hands.entry(player).or_default();
        for c in cards {
            match hand.iter().position(|x| x == c) {
                Some(i) => {
                    hand.remove(i);
                }
                None => self.anomalies.push(format!("discard of {c:?} not in hand")),
            }
        }
        Ok(())
    }

    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt> {
        self.push(TC::EotCleanup);
        let work = match self.cleanup_work_forever {
            Some(w) => Some(w),
            None => self.cleanup_work.pop_front().flatten(),
        };
        match work {
            Some(Work::Trigger) => self.place_armed = true,
            Some(Work::Sba) => self.sba_armed = true,
            Some(Work::Stack) => self.depth += 1,
            None => {}
        }
        Ok(())
    }

    fn sagas_with_chapter_abilities(&self, player: PlayerId) -> Vec<ObjectId> {
        self.sagas.get(&player).cloned().unwrap_or_default()
    }

    fn add_lore_counters(
        &mut self,
        sagas: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.push(TC::Lore(sagas.to_vec()));
        Ok(())
    }

    fn first_strike_damage_step(&self) -> bool {
        self.first_strike
    }

    fn combat_step_actions(
        &mut self,
        step: Step,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.push(TC::Combat(step));
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// the session state and procedures
// ---------------------------------------------------------------------------------------------

/// Host + sequencer state: the `Transactional` state of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    pub host: TurnFake,
    pub ts: TurnState,
}

impl World {
    pub fn new(host: TurnFake) -> World {
        World {
            host,
            ts: TurnState::new(),
        }
    }

    pub fn calls(&self) -> Vec<TC> {
        self.host.calls()
    }
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.host.game.state_hash().as_bytes());
        bytes.extend_from_slice(format!("{:?}|{:?}", self.host, self.ts).as_bytes());
        sha256(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.host.game.turn().priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// Which sequencer entry point a session runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proc {
    /// `run_turn` once per player, in order.
    Turns(Vec<PlayerId>),
    Phase(Phase),
    Segment(Segment),
}

/// A procedure's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rep {
    Turns(Vec<TurnReport>),
    Phase(Vec<SegmentReport>),
    Segment(SegmentReport),
}

/// A procedure's output: its report, or its error kept as data.
pub type Out = Result<Rep, ChoiceError>;

fn keep<T>(r: Result<T, Interrupt>) -> Result<Result<T, ChoiceError>, Interrupt> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(Interrupt::Pending(p)) => Err(Interrupt::Pending(p)),
        Err(Interrupt::Error(e)) => Ok(Err(e)),
    }
}

/// Runs `proc` with `registry` over a world.
pub fn run_proc(
    proc: &Proc,
    registry: &TurnBasedRegistry,
    w: &mut World,
    c: &mut ChoiceContext<'_>,
) -> Result<Out, Interrupt> {
    match proc {
        Proc::Turns(players) => {
            let mut reports = Vec::new();
            for &p in players {
                match keep(run_turn(&mut w.host, &mut w.ts, registry, p, c))? {
                    Ok(r) => reports.push(r),
                    Err(e) => return Ok(Err(e)),
                }
            }
            Ok(Ok(Rep::Turns(reports)))
        }
        Proc::Phase(phase) => {
            Ok(keep(run_phase(&mut w.host, &mut w.ts, registry, *phase, c))?.map(Rep::Phase))
        }
        Proc::Segment(seg) => {
            Ok(keep(run_segment(&mut w.host, &mut w.ts, registry, *seg, c))?.map(Rep::Segment))
        }
    }
}

/// A boxed procedure for a session.
pub type ProcFn = Box<dyn Fn(&mut World, &mut ChoiceContext<'_>) -> Result<Out, Interrupt>>;

pub fn proc_fn(proc: Proc, registry: TurnBasedRegistry) -> ProcFn {
    Box::new(move |w, c| run_proc(&proc, &registry, w, c))
}

// ---------------------------------------------------------------------------------------------
// answering
// ---------------------------------------------------------------------------------------------

/// Scripted answers for the sequencer's own requests; priority requests are always passed.
#[derive(Clone, Debug, Default)]
pub struct Answers {
    /// Answers to `CHOOSE_UNTAP_SET`, in order; once empty, the request's default.
    pub untap: VecDeque<Vec<ObjectId>>,
    /// Answers to `SELECT_OBJECTS_FROM_ZONE`, in order; once empty, the first `min` candidates.
    pub discard: VecDeque<Vec<ObjectId>>,
}

impl Answers {
    pub fn none() -> Answers {
        Answers::default()
    }

    pub fn untap(mut self, objects: Vec<ObjectId>) -> Answers {
        self.untap.push_back(objects);
        self
    }

    pub fn discard(mut self, objects: Vec<ObjectId>) -> Answers {
        self.discard.push_back(objects);
        self
    }

    pub fn answer(&mut self, req: &PendingChoiceRequest) -> ChoiceResponse {
        match (&req.choice_type, &req.payload) {
            (ChoiceType::PriorityAction, _) => ChoiceResponse::PassPriority,
            (ChoiceType::ChooseUntapSet, _) => match self.untap.pop_front() {
                Some(objects) => ChoiceResponse::SubmitSet { objects },
                None => req.default_on_timeout.clone(),
            },
            (
                ChoiceType::SelectObjectsFromZone,
                ChoicePayload::SelectObjectsFromZone { candidates },
            ) => match self.discard.pop_front() {
                Some(objects) => ChoiceResponse::SubmitObjectSelection { objects },
                None => ChoiceResponse::SubmitObjectSelection {
                    objects: candidates.iter().copied().take(req.min as usize).collect(),
                },
            },
            _ => req.default_on_timeout.clone(),
        }
    }
}

/// The most requests one driven session may publish.
pub const MAX_ASKS: usize = 1_000;

/// The result of driving a session.
#[derive(Clone, Debug)]
pub struct Run {
    pub requests: Vec<PendingChoiceRequest>,
    pub answers: Vec<ChoiceResponse>,
    /// The final state (if done) or the base state.
    pub last: World,
    pub out: Option<Out>,
    pub hash: Digest,
}

impl Run {
    pub fn calls(&self) -> Vec<TC> {
        self.last.calls()
    }

    /// The report, panicking on a procedure error or an unfinished run.
    pub fn rep(&self) -> Rep {
        match &self.out {
            Some(Ok(r)) => r.clone(),
            other => panic!("the procedure did not finish cleanly: {other:?}"),
        }
    }

    pub fn segment(&self) -> SegmentReport {
        match self.rep() {
            Rep::Segment(s) => s,
            other => panic!("not a segment report: {other:?}"),
        }
    }

    pub fn types(&self) -> Vec<ChoiceType> {
        self.requests.iter().map(|r| r.choice_type).collect()
    }

    pub fn non_priority(&self) -> Vec<PendingChoiceRequest> {
        self.requests
            .iter()
            .filter(|r| r.choice_type != ChoiceType::PriorityAction)
            .cloned()
            .collect()
    }
}

/// Drives `proc` over `base`, answering with `answers`, to the end.
pub fn drive(base: World, proc: Proc, registry: &TurnBasedRegistry, mut answers: Answers) -> Run {
    let mut session = match ChoiceSession::start(base, proc_fn(proc, registry.clone())) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    };
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < MAX_ASKS, "the session did not terminate");
        let a = answers.answer(&req);
        requests.push(req.clone());
        if let Err(e) = session.submit(req.request_id, a.clone()) {
            panic!("answer {a:?} to {req:?} rejected: {e:?}");
        }
    }
    finish(session, requests)
}

fn finish(session: ChoiceSession<World, Out, ProcFn>, requests: Vec<PendingChoiceRequest>) -> Run {
    let (last, out) = match session.status() {
        Status::Done { state, output } => (state.clone(), Some(output.clone())),
        Status::Pending(_) => (session.base().clone(), None),
    };
    Run {
        requests,
        answers: session.answers().to_vec(),
        last,
        out,
        hash: session.state_hash(),
    }
}

/// Starts a session (no answers).
pub fn start(
    base: World,
    proc: Proc,
    registry: &TurnBasedRegistry,
) -> ChoiceSession<World, Out, ProcFn> {
    match ChoiceSession::start(base, proc_fn(proc, registry.clone())) {
        Ok(s) => s,
        Err(e) => panic!("session start failed: {e:?}"),
    }
}

/// Starts a session and answers every request with `answers` until the first request
/// satisfying `stop` is pending (or the run ends); returns the session and the requests seen
/// (including the pending one).
pub fn drive_until(
    base: World,
    proc: Proc,
    registry: &TurnBasedRegistry,
    mut answers: Answers,
    stop: impl Fn(&PendingChoiceRequest) -> bool,
) -> (ChoiceSession<World, Out, ProcFn>, Vec<PendingChoiceRequest>) {
    let mut session = start(base, proc, registry);
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < MAX_ASKS, "the session did not terminate");
        requests.push(req.clone());
        if stop(&req) {
            break;
        }
        let a = answers.answer(&req);
        if let Err(e) = session.submit(req.request_id, a.clone()) {
            panic!("answer {a:?} to {req:?} rejected: {e:?}");
        }
    }
    (session, requests)
}

/// Continues a session to the end with `answers`.
pub fn drive_rest(
    mut session: ChoiceSession<World, Out, ProcFn>,
    mut requests: Vec<PendingChoiceRequest>,
    mut answers: Answers,
    already_recorded_pending: bool,
) -> Run {
    let mut first = already_recorded_pending;
    while let Some(req) = session.pending().cloned() {
        assert!(requests.len() < MAX_ASKS, "the session did not terminate");
        if !first {
            requests.push(req.clone());
        }
        first = false;
        let a = answers.answer(&req);
        if let Err(e) = session.submit(req.request_id, a.clone()) {
            panic!("answer {a:?} to {req:?} rejected: {e:?}");
        }
    }
    finish(session, requests)
}

/// Replays `answers` all at once over `base`.
pub fn replay(
    base: World,
    proc: Proc,
    registry: &TurnBasedRegistry,
    answers: &[ChoiceResponse],
) -> Run {
    let session = match ChoiceSession::replay(base, proc_fn(proc, registry.clone()), answers) {
        Ok(s) => s,
        Err(e) => panic!("replay failed: {e:?}"),
    };
    finish(session, Vec::new())
}

// ---------------------------------------------------------------------------------------------
// expected logs
// ---------------------------------------------------------------------------------------------

/// A quiet heartbeat with `order` the players in priority order (active first): one settle
/// (nothing happens) then one pass each (CR 117.3a, CR 500.2).
pub fn hb(order: &[PlayerId]) -> Vec<TC> {
    let mut v = vec![TC::Sba, TC::Place(order.to_vec())];
    v.extend(order.iter().map(|&p| TC::Prio(p)));
    v
}

/// A heartbeat that starts with one trigger stacked by the first placement: it resolves after a
/// full round of passes, then a second round of passes ends the step.
pub fn hb_with_trigger(order: &[PlayerId]) -> Vec<TC> {
    let mut v = vec![TC::Sba, TC::Place(order.to_vec())];
    v.extend(hb(order));
    v.push(TC::Resolve);
    v.extend(hb(order));
    v
}

/// A heartbeat whose stack holds one object at the start: passes, resolve, passes.
pub fn hb_with_stack(order: &[PlayerId]) -> Vec<TC> {
    let mut v = hb(order);
    v.push(TC::Resolve);
    v.extend(hb(order));
    v
}

/// One step (CR 500.4, CR 500.5): begin, begin expiry, body, end expiry, empty mana.
pub fn step(s: Step, body: Vec<TC>) -> Vec<TC> {
    let mut v = vec![TC::BeginStep(s), TC::Expire(ExpiryPoint::BeginStep(s))];
    v.extend(body);
    v.push(TC::Expire(ExpiryPoint::EndStep(s)));
    v.push(TC::EmptyMana);
    v
}

/// One phase (CR 500.4, CR 500.5): begin, begin expiry, segments, end expiry, empty mana.
pub fn phase(p: Phase, body: Vec<TC>) -> Vec<TC> {
    let mut v = vec![TC::BeginPhase(p), TC::Expire(ExpiryPoint::BeginPhase(p))];
    v.extend(body);
    v.push(TC::Expire(ExpiryPoint::EndPhase(p)));
    v.push(TC::EmptyMana);
    v
}

/// A quiet cleanup step body (CR 514.2, CR 514.3a: nothing to do, no priority).
pub fn quiet_cleanup_body(order: &[PlayerId]) -> Vec<TC> {
    vec![TC::EotCleanup, TC::Sba, TC::Place(order.to_vec())]
}

pub fn cat(parts: Vec<Vec<TC>>) -> Vec<TC> {
    parts.into_iter().flatten().collect()
}

/// The combat steps in order (CR 500.1).
pub fn combat_steps(first_strike: bool) -> Vec<Step> {
    let mut v = vec![
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
    ];
    if first_strike {
        v.push(Step::FirstStrikeDamage);
    }
    v.push(Step::CombatDamage);
    v.push(Step::EndOfCombat);
    v
}

/// The expected log of one quiet two-player turn of `active` (the other player `other`).
pub fn quiet_turn(
    active: PlayerId,
    other: PlayerId,
    previous: Option<PlayerId>,
    untap_body_tail: Vec<TC>,
    draws: bool,
    first_strike: bool,
) -> Vec<TC> {
    let order = [active, other];
    let mut untap_body = vec![
        TC::Phasing(active),
        TC::DayNight(previous),
        TC::UntapOptions(active),
    ];
    untap_body.extend(untap_body_tail);
    let mut draw_body = Vec::new();
    if draws {
        draw_body.push(TC::Draw(active));
    }
    draw_body.extend(hb(&order));
    let combat = combat_steps(first_strike)
        .into_iter()
        .map(|s| step(s, cat(vec![vec![TC::Combat(s)], hb(&order)])))
        .collect::<Vec<_>>();
    cat(vec![
        vec![TC::BeginTurn(active)],
        phase(
            Phase::Beginning,
            cat(vec![
                step(Step::Untap, untap_body),
                step(Step::Upkeep, hb(&order)),
                step(Step::Draw, draw_body),
            ]),
        ),
        phase(Phase::PrecombatMain, hb(&order)),
        phase(Phase::Combat, cat(combat)),
        phase(Phase::PostcombatMain, hb(&order)),
        phase(
            Phase::Ending,
            cat(vec![
                step(Step::End, hb(&order)),
                step(Step::Cleanup, quiet_cleanup_body(&order)),
            ]),
        ),
    ])
}

/// The expected segment order of a turn (CR 500.1).
pub fn turn_segments(first_strike: bool) -> Vec<Segment> {
    let mut v = vec![
        Segment::Step(Step::Untap),
        Segment::Step(Step::Upkeep),
        Segment::Step(Step::Draw),
        Segment::PrecombatMain,
    ];
    v.extend(combat_steps(first_strike).into_iter().map(Segment::Step));
    v.push(Segment::PostcombatMain);
    v.push(Segment::Step(Step::End));
    v.push(Segment::Step(Step::Cleanup));
    v
}

/// Counts `c` in a log.
pub fn count(log: &[TC], c: &TC) -> usize {
    log.iter().filter(|x| *x == c).count()
}

/// The slice of `log` from the `n`-th (0-based) occurrence of `from` up to (excluding) the next
/// occurrence of `to` after it (or the end).
pub fn between(log: &[TC], from: &TC, n: usize, to: &TC) -> Vec<TC> {
    let start = log
        .iter()
        .enumerate()
        .filter(|(_, x)| *x == from)
        .nth(n)
        .map(|(i, _)| i)
        .expect("start marker present");
    let rest = &log[start..];
    let end = rest
        .iter()
        .skip(1)
        .position(|x| x == to)
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    rest[..end].to_vec()
}

/// A 64-bit LCG with a fixed seed (R3: no system randomness).
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x0046))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}
