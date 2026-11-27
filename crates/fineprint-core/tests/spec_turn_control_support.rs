// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the turn-control spec tests (REQ-TURN-003; CR 500.7-500.11, 614.10,
//! 725.1, 724.1, 728.1). Black-box: written from the work-item brief only, against the public
//! API of `fineprint_core::turn::control`.
//!
//! Each `spec_turn_control*.rs` file includes this file with `#[path]`. Nothing here is card
//! data (R5): every object is a plain synthetic id.
//!
//! - Plan helpers: build the expected item sequence of a turn and drain a [`TurnScheduler`].
//! - [`Ctl`]: a recording `TurnHost` + `EndTurnHost` that wraps the `TurnFake` of the
//!   turn-sequencer spec tests and logs the end-the-turn calls in the same ordered log.
//! - [`CW`]: host + `TurnState` + scheduler, the `Transactional` state of a `ChoiceSession`.
//! - [`RFake`]: a recording `RestartHost` that wraps the `Mock` `GameStartHost` of the
//!   game-start spec tests.
//! - [`SchedModel`] and [`ControlModel`]: independent reference models of the brief's
//!   specified behaviour, for the property tests.

#![allow(dead_code)]

#[path = "spec_turn_steps_support.rs"]
pub mod steps;

#[path = "spec_turn_start_support.rs"]
pub mod start;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceSession, ChoiceType, Interrupt, PendingChoiceRequest, RequestId, Status,
};
use fineprint_core::events::{Phase, Step};
use fineprint_core::format::{FirstTurnDrawRule, FormatConfig};
use fineprint_core::replacement::BusInterrupt;
use fineprint_core::sba::SbaOutcome;
use fineprint_core::state::GameState;
use fineprint_core::triggers::TriggerPlacement;
use fineprint_core::turn::heartbeat::HeartbeatHost;
use fineprint_core::turn::{
    EndTurnHost, ExpiryPoint, GameStartHost, PlanItem, PlannedTurn, RestartHost, Segment, TurnHost,
    TurnScheduler, TurnState, UntapOptions, segments_of,
};
use fineprint_core::txn::Transactional;

#[allow(unused_imports)]
pub use steps::{Lcg, P1, P2, P3, TC, TurnFake};

/// A fourth player for the 2-4 seat property tests.
pub const P4: PlayerId = PlayerId::new(4);

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

// ---------------------------------------------------------------------------------------------
// plan items
// ---------------------------------------------------------------------------------------------

pub fn bp(p: Phase) -> PlanItem {
    PlanItem::BeginPhase(p)
}

pub fn ep(p: Phase) -> PlanItem {
    PlanItem::EndPhase(p)
}

pub fn st(s: Step) -> PlanItem {
    PlanItem::Segment(Segment::Step(s))
}

pub fn seg(s: Segment) -> PlanItem {
    PlanItem::Segment(s)
}

/// The items of one whole phase: `BeginPhase`, its segments, `EndPhase`.
pub fn phase_items(p: Phase, first_strike: bool) -> Vec<PlanItem> {
    let mut v = vec![bp(p)];
    v.extend(segments_of(p, first_strike).into_iter().map(seg));
    v.push(ep(p));
    v
}

/// The items of a phase holding exactly `steps`.
pub fn phase_with(p: Phase, steps: &[Step]) -> Vec<PlanItem> {
    let mut v = vec![bp(p)];
    v.extend(steps.iter().copied().map(st));
    v.push(ep(p));
    v
}

pub fn cat(parts: Vec<Vec<PlanItem>>) -> Vec<PlanItem> {
    parts.into_iter().flatten().collect()
}

/// The five phases in turn order.
pub const PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];

/// The full normal plan of one turn, as the brief specifies it.
pub fn normal_plan(first_strike: bool) -> Vec<PlanItem> {
    PHASES
        .iter()
        .flat_map(|&p| phase_items(p, first_strike))
        .collect()
}

/// The upper bound on items pulled from one turn (guards a non-terminating scheduler).
pub const MAX_ITEMS: usize = 500;

/// Pulls every remaining item of the current turn.
pub fn drain(s: &mut TurnScheduler) -> Vec<PlanItem> {
    let mut v = Vec::new();
    while let Some(i) = s.next_item() {
        v.push(i);
        assert!(v.len() <= MAX_ITEMS, "the turn did not end");
    }
    // Once over, it stays over.
    assert_eq!(s.next_item(), None);
    v
}

/// Pulls items up to and including the first one equal to `item`; returns them.
pub fn advance_to(s: &mut TurnScheduler, item: PlanItem) -> Vec<PlanItem> {
    let mut v = Vec::new();
    loop {
        let i = s
            .next_item()
            .unwrap_or_else(|| panic!("{item:?} never came (pulled {v:?})"));
        v.push(i);
        assert!(v.len() <= MAX_ITEMS, "the turn did not end");
        if i == item {
            return v;
        }
    }
}

/// All players in the game.
pub fn everyone(_: PlayerId) -> bool {
    true
}

/// The next turn with everyone in the game, no first-strike step.
pub fn next(s: &mut TurnScheduler, seats: &[PlayerId]) -> PlannedTurn {
    s.next_turn(seats, &everyone, false)
        .expect("some player is in the game")
}

/// The next turn, then all its items.
pub fn take(s: &mut TurnScheduler, seats: &[PlayerId]) -> (PlannedTurn, Vec<PlanItem>) {
    let t = next(s, seats);
    let items = drain(s);
    (t, items)
}

/// The players of the next `n` turns (each turn drained), everyone in the game.
pub fn order(s: &mut TurnScheduler, seats: &[PlayerId], n: usize) -> Vec<PlayerId> {
    (0..n).map(|_| take(s, seats).0.player).collect()
}

/// The segments among `items`.
pub fn segments(items: &[PlanItem]) -> Vec<Segment> {
    items
        .iter()
        .filter_map(|i| match i {
            PlanItem::Segment(s) => Some(*s),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// requests
// ---------------------------------------------------------------------------------------------

/// A one-candidate `CHOOSE_PLAYER` request for `chooser`, revealed to `reveal_to`.
pub fn request(chooser: PlayerId, reveal_to: &[PlayerId], hidden: bool) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser,
        choice_type: ChoiceType::ChoosePlayer,
        reason: ChoiceReason {
            cr: "CR 724.5".to_string(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden,
        reveal_to: reveal_to.to_vec(),
        default_on_timeout: ChoiceResponse::SubmitPlayer { player: chooser },
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Player(chooser)]),
        payload: ChoicePayload::ChoosePlayer {
            players: vec![chooser],
        },
    }
}

// ---------------------------------------------------------------------------------------------
// the end-the-turn host
// ---------------------------------------------------------------------------------------------

/// One logged call of a [`Ctl`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum C {
    /// A call of the wrapped `TurnFake` (its own log entry).
    Base(TC),
    /// `EndTurnHost::cease_pending_triggers` (CR 725.1a).
    Cease,
    /// `EndTurnHost::exile_stack` (CR 725.1b).
    ExileStack,
    /// `EndTurnHost::remove_all_from_combat` (CR 725.1d, CR 725.2d).
    RemoveCombat,
}

/// A recording `TurnHost` + `EndTurnHost` over a `TurnFake`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ctl {
    pub t: TurnFake,
    pub log: RefCell<Vec<C>>,
    /// One entry per `sba_pass` call: overrides `performed`; once empty, the fake decides.
    pub sba_script: VecDeque<bool>,
    /// The next `sba_pass` makes a trigger wait (the next `place_triggers` stacks one).
    pub arm_trigger_on_sba: bool,
}

impl Ctl {
    pub fn new(t: TurnFake) -> Ctl {
        Ctl {
            t,
            log: RefCell::new(Vec::new()),
            sba_script: VecDeque::new(),
            arm_trigger_on_sba: false,
        }
    }

    fn sync(&self) {
        let mut inner = self.t.log.borrow_mut();
        let mut mine = self.log.borrow_mut();
        for c in inner.drain(..) {
            mine.push(C::Base(c));
        }
    }

    fn push(&self, c: C) {
        self.sync();
        self.log.borrow_mut().push(c);
    }

    pub fn calls(&self) -> Vec<C> {
        self.sync();
        self.log.borrow().clone()
    }

    /// Only the wrapped fake's entries.
    pub fn base_calls(&self) -> Vec<TC> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                C::Base(tc) => Some(tc),
                _ => None,
            })
            .collect()
    }

    pub fn clear(&self) {
        self.sync();
        self.log.borrow_mut().clear();
    }
}

impl HeartbeatHost for Ctl {
    fn game(&self) -> &GameState {
        self.t.game()
    }
    fn game_mut(&mut self) -> &mut GameState {
        self.t.game_mut()
    }
    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        self.t.phase_and_step()
    }
    fn has_left_game(&self, player: PlayerId) -> bool {
        self.t.has_left_game(player)
    }
    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        let mut out = self.t.sba_pass(choices)?;
        if let Some(p) = self.sba_script.pop_front() {
            out.performed = p;
        }
        if std::mem::replace(&mut self.arm_trigger_on_sba, false) {
            self.t.place_armed = true;
        }
        self.sync();
        Ok(out)
    }
    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        let r = self.t.place_triggers(apnap, choices);
        self.sync();
        r
    }
    fn stack_is_empty(&self) -> bool {
        self.t.stack_is_empty()
    }
    fn resolve_top(&mut self, choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        let r = self.t.resolve_top(choices);
        self.sync();
        r
    }
    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        let r = self.t.priority_actions(player);
        self.sync();
        r
    }
    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        let r = self.t.take_action(player, action, choices);
        self.sync();
        r
    }
}

impl TurnHost for Ctl {
    fn begin_turn(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.t.begin_turn(active)
    }
    fn begin_phase(&mut self, phase: Phase) -> Result<(), Interrupt> {
        self.t.begin_phase(phase)
    }
    fn begin_step(&mut self, step: Step) -> Result<(), Interrupt> {
        self.t.begin_step(step)
    }
    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt> {
        self.t.expire_effects(at)
    }
    fn empty_mana_pools(&mut self) -> Result<(), Interrupt> {
        self.t.empty_mana_pools()
    }
    fn apply_phasing(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.t.apply_phasing(active)
    }
    fn check_day_night(&mut self, previous_active: Option<PlayerId>) -> Result<(), Interrupt> {
        self.t.check_day_night(previous_active)
    }
    fn untap_options(&self, active: PlayerId) -> UntapOptions {
        self.t.untap_options(active)
    }
    fn untap(
        &mut self,
        active: PlayerId,
        permanents: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.t.untap(active, permanents, choices)
    }
    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule {
        self.t.first_turn_draw_rule()
    }
    fn starting_player(&self) -> Option<PlayerId> {
        self.t.starting_player()
    }
    fn draw_card(
        &mut self,
        player: PlayerId,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.t.draw_card(player, choices)
    }
    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        self.t.hand(player)
    }
    fn max_hand_size(&self, player: PlayerId) -> Option<u64> {
        self.t.max_hand_size(player)
    }
    fn discard_cards(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.t.discard_cards(player, cards, choices)
    }
    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt> {
        self.t.end_of_turn_cleanup()
    }
    fn sagas_with_chapter_abilities(&self, player: PlayerId) -> Vec<ObjectId> {
        self.t.sagas_with_chapter_abilities(player)
    }
    fn add_lore_counters(
        &mut self,
        sagas: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.t.add_lore_counters(sagas, choices)
    }
    fn first_strike_damage_step(&self) -> bool {
        self.t.first_strike_damage_step()
    }
    fn combat_step_actions(
        &mut self,
        step: Step,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.t.combat_step_actions(step, choices)
    }
}

impl EndTurnHost for Ctl {
    fn cease_pending_triggers(&mut self) -> Result<(), Interrupt> {
        self.push(C::Cease);
        Ok(())
    }
    fn exile_stack(&mut self) -> Result<(), Interrupt> {
        self.push(C::ExileStack);
        Ok(())
    }
    fn remove_all_from_combat(&mut self) -> Result<(), Interrupt> {
        self.push(C::RemoveCombat);
        Ok(())
    }
}

/// Host + sequencer state + scheduler: the `Transactional` state of a session.
#[derive(Clone, Debug)]
pub struct CW {
    pub host: Ctl,
    pub ts: TurnState,
    pub sched: TurnScheduler,
}

impl CW {
    /// A two-player world (P1 starts, P1 active on turn 1).
    pub fn two() -> CW {
        CW {
            host: Ctl::new(TurnFake::new(2).at(1, Some(P1))),
            ts: TurnState::new(),
            sched: TurnScheduler::new(P1),
        }
    }
}

impl Transactional for CW {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        self.host.t.game.turn().priority_player()
    }
    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// Answers priority with a pass and anything else with its timeout default.
pub fn pass_or_default(req: &PendingChoiceRequest) -> ChoiceResponse {
    match req.choice_type {
        ChoiceType::PriorityAction => ChoiceResponse::PassPriority,
        _ => req.default_on_timeout.clone(),
    }
}

/// Runs `f` over `base` in a `ChoiceSession`, answering with `answer`, to the end.
pub fn run_session<S, O, F>(
    base: S,
    f: F,
    answer: impl Fn(&PendingChoiceRequest) -> ChoiceResponse,
) -> Result<(S, O), ChoiceError>
where
    S: Transactional + Clone,
    O: Clone,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let mut s = ChoiceSession::start(base, f)?;
    let mut n = 0usize;
    while let Some(req) = s.pending().cloned() {
        n += 1;
        assert!(n < 1_000, "the session did not terminate");
        s.submit(req.request_id, answer(&req))?;
    }
    match s.status() {
        Status::Done { state, output } => Ok((state.clone(), output.clone())),
        Status::Pending(_) => unreachable!("loop ended with a pending request"),
    }
}

/// [`run_session`] answering with [`pass_or_default`].
pub fn run_cw<O, F>(base: CW, f: F) -> Result<(CW, O), ChoiceError>
where
    O: Clone,
    F: Fn(&mut CW, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    run_session(base, f, pass_or_default)
}

// ---------------------------------------------------------------------------------------------
// the restart host
// ---------------------------------------------------------------------------------------------

/// The owner of a `start::card(p, i)` id (`p * 1000 + i`).
pub fn owner_of(card: ObjectId) -> PlayerId {
    pid(card.get() / 1000)
}

/// A recording `RestartHost` over the game-start `Mock`.
#[derive(Clone, Debug)]
pub struct RFake {
    pub m: start::Mock,
    pub log: RefCell<Vec<String>>,
    /// What `restart_exempt_cards` reports.
    pub exempt: BTreeSet<ObjectId>,
    /// Cards outside libraries and hands (for example on the battlefield), with the player who
    /// controls them; the owner is [`owner_of`] the id.
    pub elsewhere: BTreeMap<ObjectId, PlayerId>,
    /// Cards not in their owner's deck for the new game (exempt), filled by the reset.
    pub left_out: BTreeSet<ObjectId>,
    /// The exempt set `reset_for_new_game` received.
    pub reset_got: Option<BTreeSet<ObjectId>>,
}

impl RFake {
    /// `n` players with `lib` cards each, 20 life, hand size 7.
    pub fn new(n: usize, lib: u64) -> RFake {
        RFake {
            m: start::Mock::new(0x0046_0728, n, lib, 20, 7, false),
            log: RefCell::new(Vec::new()),
            exempt: BTreeSet::new(),
            elsewhere: BTreeMap::new(),
            left_out: BTreeSet::new(),
            reset_got: None,
        }
    }

    fn note(&self, s: String) {
        self.log.borrow_mut().push(s);
    }

    /// Moves the mock's own log entries into this log.
    fn sync(&mut self) {
        let entries: Vec<String> = self.m.log.drain(..).collect();
        self.log.borrow_mut().extend(entries);
    }

    pub fn calls(&self) -> Vec<String> {
        let mut v = self.log.borrow().clone();
        v.extend(self.m.log.iter().cloned());
        v
    }

    /// Every card that is in some library or hand, with the player whose library or hand it
    /// is in.
    pub fn deck_cards(&self) -> BTreeMap<ObjectId, PlayerId> {
        let mut out = BTreeMap::new();
        for (p, lib) in &self.m.libs {
            for c in lib {
                out.insert(*c, *p);
            }
        }
        for (p, hand) in &self.m.hands {
            for c in hand {
                out.insert(*c, *p);
            }
        }
        out
    }
}

impl GameStartHost for RFake {
    fn game(&self) -> &GameState {
        GameStartHost::game(&self.m)
    }
    fn game_mut(&mut self) -> &mut GameState {
        GameStartHost::game_mut(&mut self.m)
    }
    fn format(&self) -> &FormatConfig {
        self.m.format()
    }
    fn starting_hand_size(&self, player: PlayerId) -> u32 {
        self.m.starting_hand_size(player)
    }
    fn library(&self, player: PlayerId) -> Vec<ObjectId> {
        self.m.library(player)
    }
    fn set_library(&mut self, player: PlayerId, order: Vec<ObjectId>) -> Result<(), Interrupt> {
        let r = self.m.set_library(player, order);
        self.sync();
        r
    }
    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        GameStartHost::hand(&self.m, player)
    }
    fn draw_cards(
        &mut self,
        player: PlayerId,
        count: u32,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        let r = self.m.draw_cards(player, count, choices);
        self.sync();
        r
    }
    fn return_hand_to_library(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        let r = self.m.return_hand_to_library(player);
        self.sync();
        r
    }
    fn put_on_library_bottom(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
    ) -> Result<(), Interrupt> {
        let r = self.m.put_on_library_bottom(player, cards);
        self.sync();
        r
    }
    fn set_starting_player(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        let r = self.m.set_starting_player(player);
        self.sync();
        r
    }
    fn set_player_life(&mut self, player: PlayerId, life: i64) -> Result<(), Interrupt> {
        let r = self.m.set_player_life(player, life);
        self.sync();
        r
    }
    fn pregame_eligible(&self, player: PlayerId) -> Vec<ObjectId> {
        self.m.pregame_eligible(player)
    }
    fn perform_pregame_actions(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        let r = self.m.perform_pregame_actions(player, cards, choices);
        self.sync();
        r
    }
}

impl RestartHost for RFake {
    fn end_game_without_result(&mut self) -> Result<(), Interrupt> {
        self.note("end_game".to_string());
        Ok(())
    }
    fn restart_exempt_cards(&self) -> BTreeSet<ObjectId> {
        self.note("exempt_cards".to_string());
        self.exempt.clone()
    }
    fn reset_for_new_game(&mut self, exempt: &BTreeSet<ObjectId>) -> Result<(), Interrupt> {
        self.note(format!(
            "reset:{:?}",
            exempt.iter().map(|c| c.get()).collect::<Vec<_>>()
        ));
        self.reset_got = Some(exempt.clone());
        // CR 728.2: every card returns to its OWNER's library, wherever it is and whoever
        // controls it; CR 728.5: exempt cards are not in their owner's deck.
        let mut all: Vec<ObjectId> = self.deck_cards().into_keys().collect();
        all.extend(self.elsewhere.keys().copied());
        self.elsewhere.clear();
        for lib in self.m.libs.values_mut() {
            lib.clear();
        }
        for hand in self.m.hands.values_mut() {
            hand.clear();
        }
        all.sort();
        all.dedup();
        for c in all {
            if exempt.contains(&c) {
                self.left_out.insert(c);
            } else {
                self.m.libs.entry(owner_of(c)).or_default().push(c);
            }
        }
        Ok(())
    }
    fn finish_restarting_effect(
        &mut self,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.note("finish".to_string());
        Ok(())
    }
}

impl Transactional for RFake {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        None
    }
    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

// ---------------------------------------------------------------------------------------------
// reference models
// ---------------------------------------------------------------------------------------------

/// An independent model of the brief's `next_turn` / extra-turn / turn-skip behaviour.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedModel {
    pub start: Option<PlayerId>,
    pub last_normal: Option<PlayerId>,
    /// The extra-turn stack; the last entry is taken next.
    pub extras: Vec<PlayerId>,
    /// Per player, the pending turn skips, oldest first, with their follow-ups and a creation
    /// sequence number.
    pub skips: BTreeMap<PlayerId, VecDeque<(u64, Option<u64>)>>,
    pub seq: u64,
}

/// What the model says one `next_turn` returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTurn {
    pub player: PlayerId,
    pub extra: bool,
    pub skipped: Vec<(PlayerId, bool)>,
    pub follow_ups: Vec<u64>,
    /// Extra turns dropped because their player had left.
    pub dropped: Vec<PlayerId>,
}

impl SchedModel {
    pub fn new(start: PlayerId) -> SchedModel {
        SchedModel {
            start: Some(start),
            ..SchedModel::default()
        }
    }

    pub fn add_extra_turn(&mut self, p: PlayerId) {
        self.extras.push(p);
    }

    pub fn add_turn_skip(&mut self, p: PlayerId, f: Option<u64>) {
        self.seq += 1;
        self.skips.entry(p).or_default().push_back((self.seq, f));
    }

    pub fn pending_turn_skips(&self, p: PlayerId) -> usize {
        self.skips.get(&p).map_or(0, VecDeque::len)
    }

    pub fn pending_extra_turns(&self) -> Vec<PlayerId> {
        self.extras.iter().rev().copied().collect()
    }

    fn next_normal(&self, seats: &[PlayerId], in_game: &dyn Fn(PlayerId) -> bool) -> PlayerId {
        let Some(last) = self.last_normal else {
            return self.start.expect("start");
        };
        match seats.iter().position(|s| *s == last) {
            Some(i) => {
                let n = seats.len();
                (1..=n)
                    .map(|k| seats[(i + k) % n])
                    .find(|p| in_game(*p))
                    .expect("someone in game")
            }
            None => *seats
                .iter()
                .find(|p| in_game(**p))
                .expect("someone in game"),
        }
    }

    pub fn next_turn(
        &mut self,
        seats: &[PlayerId],
        in_game: &dyn Fn(PlayerId) -> bool,
    ) -> Option<ModelTurn> {
        if !seats.iter().any(|p| in_game(*p)) {
            return None;
        }
        let mut skipped = Vec::new();
        let mut follows: Vec<(u64, u64)> = Vec::new();
        let mut dropped = Vec::new();
        loop {
            let (p, extra) = match self.extras.pop() {
                Some(p) => (p, true),
                None => (self.next_normal(seats, in_game), false),
            };
            if !in_game(p) {
                // CR 800.4k.
                if extra {
                    dropped.push(p);
                } else {
                    self.last_normal = Some(p);
                }
                continue;
            }
            if !extra {
                self.last_normal = Some(p);
            }
            if let Some((seq, f)) = self.skips.get_mut(&p).and_then(VecDeque::pop_front) {
                // CR 614.10, CR 614.10b.
                skipped.push((p, extra));
                if let Some(f) = f {
                    follows.push((seq, f));
                }
                continue;
            }
            follows.sort();
            return Some(ModelTurn {
                player: p,
                extra,
                skipped,
                follow_ups: follows.into_iter().map(|(_, f)| f).collect(),
                dropped,
            });
        }
    }
}

/// An independent model of the brief's `PlayerControl`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlModel {
    pub pending: BTreeMap<PlayerId, PlayerId>,
    pub active: Option<(PlayerId, PlayerId)>,
}

impl ControlModel {
    pub fn add(&mut self, controlled: PlayerId, controller: PlayerId) {
        self.pending.insert(controlled, controller);
    }

    pub fn begin_turn(&mut self, active: PlayerId) {
        self.active = None;
        if let Some(c) = self.pending.remove(&active)
            && c != active
        {
            self.active = Some((active, c));
        }
    }

    pub fn left(&mut self, p: PlayerId) {
        self.pending.retain(|k, v| *k != p && *v != p);
        if let Some((a, c)) = self.active
            && (a == p || c == p)
        {
            self.active = None;
        }
    }

    pub fn controller_of(&self, p: PlayerId) -> PlayerId {
        match self.active {
            Some((a, c)) if a == p => c,
            _ => p,
        }
    }

    /// The pending controller, with a self-control pending effect shown as `None`.
    pub fn pending_other(&self, p: PlayerId) -> Option<PlayerId> {
        self.pending.get(&p).copied().filter(|c| *c != p)
    }
}
