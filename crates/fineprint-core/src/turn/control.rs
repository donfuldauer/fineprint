// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Turn control: extra and skipped turns, phases and steps, ending the turn or the combat
//! phase, controlling another player, and restarting the game (REQ-TURN-003, SRS 5.3).
//!
//! # Parts
//!
//! - [`TurnScheduler`] decides which turn is taken next and yields the phases and steps of the
//!   current turn one [`PlanItem`] at a time. Extra turns, phases and steps are inserted LIFO
//!   (CR 500.7, CR 500.8, CR 500.9, CR 500.10). Skips are replacement-style "skip your next
//!   occurrence" effects (CR 614.1b, CR 614.10, CR 614.10a, CR 614.10b), consumed at the moment
//!   the turn, phase or step would begin.
//! - [`end_the_turn`] (CR 725.1) and [`end_the_combat_phase`] (CR 725.2) are written against the
//!   [`EndTurnHost`] seam; the cleanup step itself still runs through
//!   [`run_segment`](crate::turn::steps::run_segment).
//! - [`PlayerControl`] models player-controlling effects (CR 724): which player decides a
//!   request, who may answer it and what each player may see. It is pure: it never touches the
//!   knowledge store or a redacted view (docs/DECISIONS.md "#50 FP-048 — Engine gaps").
//! - [`restart_game`] restarts the game (CR 728) against the [`RestartHost`] seam.
//! - [`run_planned_turn`] runs a scheduler-planned turn through the existing sequencer.
//!
//! # Determinism
//!
//! Everything here is a pure function of its inputs and the host (R3, REQ-CORE-001): ordered
//! containers only, no clock, no I/O, no global state. The scheduler and the control model are
//! plain values that live next to the host in a `ChoiceSession`'s state, so a re-run from the
//! session base restores them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::choice::{ChoiceContext, ChoiceError, Interrupt, PendingChoiceRequest};
use crate::events::{Phase, Step};
use crate::turn::heartbeat::{HeartbeatHost, MAX_SETTLE_ITERATIONS};
use crate::turn::start::{GameStartHost, StartOptions, StartReport, run_game_start};
use crate::turn::steps::{
    ExpiryPoint, Segment, TurnBasedRegistry, TurnHost, TurnReport, TurnState,
    is_blockers_or_damage, run_segment, segments_of,
};

/// The normal phases of a turn, in order.
const NORMAL_PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];

/// The error of a protocol misuse.
fn procedure_error(message: &str) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(format!("turn control: {message}")))
}

// ---------------------------------------------------------------------------------------------
// (A) Turn scheduler
// ---------------------------------------------------------------------------------------------

/// An opaque handle to the "then take another action" part of a skip effect (CR 614.10b). The
/// scheduler only carries it to the next turn that actually occurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FollowUpId(pub u64);

/// A turn that was skipped before the planned turn began (CR 614.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedTurn {
    /// The player whose turn was skipped.
    pub player: PlayerId,
    /// Whether the skipped turn was an extra turn.
    pub extra: bool,
}

/// The turn [`TurnScheduler::next_turn`] chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedTurn {
    /// The turn's active player.
    pub player: PlayerId,
    /// Whether it is an extra turn (CR 500.7).
    pub extra: bool,
    /// The turns skipped on the way to this one, in the order they were skipped.
    pub skipped: Vec<SkippedTurn>,
    /// The follow-up actions of the skip effects consumed on the way, in the order those skip
    /// effects were created (CR 614.10b: they are the first thing that happens in this turn).
    /// Informational only: the scheduler keeps its own copy with the installed plan, and
    /// [`run_planned_turn`] performs that copy, never this field, so a handle of another
    /// scheduler cannot run its follow-ups here (docs/DECISIONS.md "Gate 06-turn R4 —
    /// Follow-ups belong to the scheduler").
    pub follow_ups: Vec<FollowUpId>,
    /// The serial number of the plan [`TurnScheduler::next_turn`] installed for this turn
    /// (1 for the first turn, then counting up). [`run_planned_turn`] rejects a turn whose
    /// serial is not the scheduler's current plan, so a saved earlier turn cannot run on a
    /// later plan, even one of the same player (F39).
    pub plan_serial: u64,
}

/// One item of the current turn's plan, in the order [`TurnScheduler::next_item`] yields them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanItem {
    /// A phase begins.
    BeginPhase(Phase),
    /// A step or a main phase runs (through `run_segment`).
    Segment(Segment),
    /// A phase ends.
    EndPhase(Phase),
}

/// One segment of the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlanSegment {
    /// The segment.
    segment: Segment,
    /// Whether step skips may apply (false for the cleanup step reached by ending the turn).
    skippable: bool,
}

/// One phase of the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlanPhase {
    /// The phase.
    phase: Phase,
    /// Its segments not yet started, in order.
    segments: Vec<PlanSegment>,
    /// Whether the phase has begun (its `BeginPhase` was yielded).
    begun: bool,
    /// Whether phase skips may apply (false for the ending phase reached by ending the turn).
    skippable: bool,
}

impl PlanPhase {
    /// A not-yet-started phase with `segments`, all skippable.
    fn new(phase: Phase, segments: Vec<Segment>) -> PlanPhase {
        PlanPhase {
            phase,
            segments: segments
                .into_iter()
                .map(|segment| PlanSegment {
                    segment,
                    skippable: true,
                })
                .collect(),
            begun: false,
            skippable: true,
        }
    }

    /// The ending phase reached by ending the turn: only an unskippable cleanup step
    /// (CR 725.1d).
    fn forced_cleanup(begun: bool) -> PlanPhase {
        PlanPhase {
            phase: Phase::Ending,
            segments: vec![PlanSegment {
                segment: Segment::Step(Step::Cleanup),
                skippable: false,
            }],
            begun,
            skippable: false,
        }
    }
}

/// A pending turn skip.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TurnSkip {
    /// The player whose next turn is skipped.
    player: PlayerId,
    /// The follow-up action, if any (CR 614.10b).
    follow_up: Option<FollowUpId>,
    /// The creation sequence number (follow-ups are reported in creation order).
    created: u64,
}

/// The extra-turn stack, the skip effects and the current turn's plan (REQ-TURN-003;
/// CR 500.7-500.12, CR 614.10). Ordered containers only (R3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnScheduler {
    /// The player who takes the first normal turn.
    starting_player: PlayerId,
    /// The player of the last NORMAL turn decided (taken, skipped or dropped).
    last_normal: Option<PlayerId>,
    /// The extra-turn stack; the LAST entry is taken first (CR 500.7).
    extra_turns: Vec<PlayerId>,
    /// Pending turn skips, oldest first.
    turn_skips: Vec<TurnSkip>,
    /// The sequence number of the next turn skip created.
    next_skip: u64,
    /// Pending phase skips, oldest first.
    phase_skips: Vec<(PlayerId, Phase)>,
    /// Pending step skips, oldest first.
    step_skips: Vec<(PlayerId, Step)>,
    /// The current turn's active player.
    active: Option<PlayerId>,
    /// Whether the combat phases of the current turn have a first-strike damage step.
    first_strike: bool,
    /// Whether a combat damage step (first-strike or regular) has begun in the current combat
    /// phase: set when [`TurnScheduler::next_item`] yields one, reset when a combat phase
    /// begins and when a plan is installed (CR 510.4: the first-strike decision is made as the
    /// first combat damage step begins and is then retained).
    damage_begun: bool,
    /// The follow-up actions (CR 614.10b) of the installed plan, in creation order, until
    /// [`run_planned_turn`] takes them (R4).
    follow_ups: Vec<FollowUpId>,
    /// The current turn's phases not yet ended; the first one may be in progress.
    plan: Vec<PlanPhase>,
    /// The item last yielded by [`TurnScheduler::next_item`] in the current turn.
    last_item: Option<PlanItem>,
    /// Whether [`TurnScheduler::next_item`] returned `None` for the current turn.
    turn_over: bool,
    /// Whether a main phase of the current turn has begun or been skipped (CR 505.1a).
    main_seen: bool,
    /// The serial number of the installed plan: 0 before the first, incremented by every plan
    /// installed (see [`PlannedTurn::plan_serial`]).
    plan_serial: u64,
}

/// Whether `phase` is a main phase.
fn is_main(phase: Phase) -> bool {
    matches!(phase, Phase::PrecombatMain | Phase::PostcombatMain)
}

/// Whether `segment` is a combat damage step, first-strike or regular (CR 510.4).
fn is_damage_step(segment: Segment) -> bool {
    matches!(
        segment,
        Segment::Step(Step::FirstStrikeDamage | Step::CombatDamage)
    )
}

impl TurnScheduler {
    /// A scheduler whose first normal turn is `starting_player`'s; no turn is in progress.
    #[must_use]
    pub fn new(starting_player: PlayerId) -> TurnScheduler {
        TurnScheduler {
            starting_player,
            last_normal: None,
            extra_turns: Vec::new(),
            turn_skips: Vec::new(),
            next_skip: 0,
            phase_skips: Vec::new(),
            step_skips: Vec::new(),
            active: None,
            first_strike: false,
            damage_begun: false,
            follow_ups: Vec::new(),
            plan: Vec::new(),
            last_item: None,
            turn_over: false,
            main_seen: false,
            plan_serial: 0,
        }
    }

    /// The active player of the turn last returned by [`TurnScheduler::next_turn`].
    #[must_use]
    pub fn active_player(&self) -> Option<PlayerId> {
        self.active
    }

    /// CR 500.7: "Some effects can give a player extra turns. They do this by adding the turns
    /// directly after the specified turn." One extra turn for `player` directly after the turn
    /// being taken or about to be taken; "The most recently created turn will be taken first."
    pub fn add_extra_turn(&mut self, player: PlayerId) {
        self.extra_turns.push(player);
    }

    /// CR 500.7: "If multiple players are given extra turns, the extra turns are added one at a
    /// time, in APNAP order". Adds one extra turn per entry, in slice order, so the LAST player
    /// in the slice takes the first of them.
    pub fn add_extra_turns_apnap(&mut self, players_in_apnap_order: &[PlayerId]) {
        for &p in players_in_apnap_order {
            self.add_extra_turn(p);
        }
    }

    /// The pending extra turns, in the order they will be taken (next first).
    #[must_use]
    pub fn pending_extra_turns(&self) -> Vec<PlayerId> {
        self.extra_turns.iter().rev().copied().collect()
    }

    /// CR 614.10: "skip `player`'s next turn" (normal or extra), with an optional follow-up
    /// action (CR 614.10b). Each call is one independent skip (CR 614.10a).
    pub fn add_turn_skip(&mut self, player: PlayerId, follow_up: Option<FollowUpId>) {
        self.turn_skips.push(TurnSkip {
            player,
            follow_up,
            created: self.next_skip,
        });
        self.next_skip = self.next_skip.saturating_add(1);
    }

    /// The number of pending turn skips of `player`.
    #[must_use]
    pub fn pending_turn_skips(&self, player: PlayerId) -> usize {
        self.turn_skips
            .iter()
            .filter(|s| s.player == player)
            .count()
    }

    /// CR 614.10: "skip `player`'s next `phase`". Consumed by the next occurrence of `phase` in
    /// a turn of `player` at the moment it would begin; "Once a step, phase, or turn has
    /// started, it can no longer be skipped".
    pub fn add_phase_skip(&mut self, player: PlayerId, phase: Phase) {
        self.phase_skips.push((player, phase));
    }

    /// CR 614.10: "skip `player`'s next `step`", consumed like [`TurnScheduler::add_phase_skip`].
    pub fn add_step_skip(&mut self, player: PlayerId, step: Step) {
        self.step_skips.push((player, step));
    }

    /// The number of pending skips of `player`'s `phase`.
    #[must_use]
    pub fn pending_phase_skips(&self, player: PlayerId, phase: Phase) -> usize {
        self.phase_skips
            .iter()
            .filter(|&&(p, ph)| p == player && ph == phase)
            .count()
    }

    /// The number of pending skips of `player`'s `step`.
    #[must_use]
    pub fn pending_step_skips(&self, player: PlayerId, step: Step) -> usize {
        self.step_skips
            .iter()
            .filter(|&&(p, s)| p == player && s == step)
            .count()
    }

    /// The next normal-turn player after the last normal turn: the starting player first, then
    /// the next seat in `seat_order` (wrapping) still in the game; if the last normal player is
    /// not seated, the first seat still in the game (docs/DECISIONS.md "#50 FP-048 — Turn
    /// scheduler judgement calls").
    fn next_normal_player(
        &self,
        seat_order: &[PlayerId],
        in_game: &dyn Fn(PlayerId) -> bool,
    ) -> Option<PlayerId> {
        let Some(last) = self.last_normal else {
            return Some(self.starting_player);
        };
        let Some(seat) = seat_order.iter().position(|&p| p == last) else {
            return seat_order.iter().copied().find(|&p| in_game(p));
        };
        let n = seat_order.len();
        (1..=n)
            .filter_map(|i| seat_order.get((seat + i) % n).copied())
            .find(|&p| in_game(p))
    }

    /// Chooses the next turn actually taken and installs its plan (REQ-TURN-003). The
    /// candidate is the top of the extra-turn stack, else the next normal turn.
    ///
    /// CR 800.4k: "If a player who has left the game would begin a turn, that turn doesn't
    /// begin." CR 614.10: a candidate with a pending turn skip is skipped (the oldest skip is
    /// consumed) and its follow-up waits for the next turn that occurs (CR 614.10a,
    /// CR 614.10b). CR 500.12: "No game events can occur between steps, phases, or turns": this
    /// only decides. Returns `None` if no player of `seat_order` is in the game.
    ///
    /// `first_strike_damage_step` only shapes the provisional plan of the turn's combat phases
    /// (what [`TurnScheduler::next_item`] yields if nobody corrects it); it is no longer
    /// decisive: [`run_planned_turn`] decides at each combat phase's damage boundary through
    /// [`TurnScheduler::set_first_strike_damage_step`] (CR 510.4; docs/DECISIONS.md "Gate
    /// 06-turn F49 — First-strike decision at the damage boundary", refined by "Gate 06-turn
    /// R5 — First-strike decision at the first damage step").
    ///
    /// The follow-ups consumed on the way are recorded with the installed plan (replacing those
    /// of the previous plan) and performed by [`run_planned_turn`] (R4).
    pub fn next_turn(
        &mut self,
        seat_order: &[PlayerId],
        in_game: &dyn Fn(PlayerId) -> bool,
        first_strike_damage_step: bool,
    ) -> Option<PlannedTurn> {
        if !seat_order.iter().any(|&p| in_game(p)) {
            return None;
        }
        let mut skipped = Vec::new();
        let mut follow_ups = Vec::new();
        loop {
            let (player, extra) = match self.extra_turns.pop() {
                Some(p) => (p, true),
                None => {
                    let p = self.next_normal_player(seat_order, in_game)?;
                    // A normal turn advances the normal order even when it is skipped or
                    // dropped; an extra turn never does (CR 500.7).
                    self.last_normal = Some(p);
                    (p, false)
                }
            };
            // CR 800.4k.
            if !in_game(player) {
                continue;
            }
            // CR 614.10, CR 614.10a: the oldest pending skip of this player is consumed.
            if let Some(i) = self.turn_skips.iter().position(|s| s.player == player) {
                let skip = self.turn_skips.remove(i);
                skipped.push(SkippedTurn { player, extra });
                // CR 614.10b.
                if let Some(f) = skip.follow_up {
                    follow_ups.push((skip.created, f));
                }
                continue;
            }
            self.install_plan(player, first_strike_damage_step);
            // The follow-ups in the order their skip effects were created; the scheduler keeps
            // them with the installed plan for its driver run (R4).
            follow_ups.sort_by_key(|&(created, _)| created);
            self.follow_ups = follow_ups.into_iter().map(|(_, f)| f).collect();
            return Some(PlannedTurn {
                player,
                extra,
                skipped,
                follow_ups: self.follow_ups.clone(),
                plan_serial: self.plan_serial,
            });
        }
    }

    /// Installs the normal plan of a new turn of `active`.
    fn install_plan(&mut self, active: PlayerId, first_strike: bool) {
        self.active = Some(active);
        self.first_strike = first_strike;
        self.plan = NORMAL_PHASES
            .iter()
            .map(|&phase| PlanPhase::new(phase, segments_of(phase, first_strike)))
            .collect();
        self.last_item = None;
        self.turn_over = false;
        self.main_seen = false;
        self.damage_begun = false;
        self.plan_serial = self.plan_serial.wrapping_add(1);
    }

    /// The serial number of the installed plan (0 before the first turn is planned), the value
    /// [`PlannedTurn::plan_serial`] carries.
    #[must_use]
    pub fn plan_serial(&self) -> u64 {
        self.plan_serial
    }

    /// CR 505.1a: "Only the first main phase of the turn is a precombat main phase. All other
    /// main phases are postcombat main phases." Relabels the not-yet-started main phases of the
    /// plan: the first one is precombat only if no main phase of the turn has begun or been
    /// skipped yet.
    fn relabel_main_phases(&mut self) {
        let mut seen = self.main_seen;
        for phase in &mut self.plan {
            if !is_main(phase.phase) {
                continue;
            }
            if !phase.begun {
                let (label, segment) = if seen {
                    (Phase::PostcombatMain, Segment::PostcombatMain)
                } else {
                    (Phase::PrecombatMain, Segment::PrecombatMain)
                };
                phase.phase = label;
                phase.segments = vec![PlanSegment {
                    segment,
                    skippable: true,
                }];
            }
            seen = true;
        }
    }

    /// Consumes the oldest pending skip of the active player's `phase`, if any.
    fn consume_phase_skip(&mut self, phase: Phase) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        match self
            .phase_skips
            .iter()
            .position(|&(p, ph)| p == active && ph == phase)
        {
            Some(i) => {
                self.phase_skips.remove(i);
                true
            }
            None => false,
        }
    }

    /// Consumes the oldest pending skip of the active player's `step`, if any.
    fn consume_step_skip(&mut self, step: Step) -> bool {
        let Some(active) = self.active else {
            return false;
        };
        match self
            .step_skips
            .iter()
            .position(|&(p, s)| p == active && s == step)
        {
            Some(i) => {
                self.step_skips.remove(i);
                true
            }
            None => false,
        }
    }

    /// The next item of the current turn, or `None` when the turn is over (REQ-TURN-003).
    ///
    /// CR 614.10: the skips of the phase or step about to begin are applied here, never once
    /// it has begun; CR 500.11: a skipped phase or step is passed "as though it didn't exist",
    /// so it yields no item (a skipped phase takes its steps with it). CR 505.1a: "Only the
    /// first main phase of the turn is a precombat main phase. All other main phases are
    /// postcombat main phases."
    pub fn next_item(&mut self) -> Option<PlanItem> {
        loop {
            let Some(front) = self.plan.first() else {
                if self.active.is_some() {
                    self.turn_over = true;
                }
                return None;
            };
            if !front.begun {
                let (phase, skippable) = (front.phase, front.skippable);
                if is_main(phase) {
                    // CR 505.1a: a main phase reached, begun or skipped, is the turn's first.
                    self.main_seen = true;
                }
                // CR 614.10, CR 500.11.
                if skippable && self.consume_phase_skip(phase) {
                    self.plan.remove(0);
                    continue;
                }
                if let Some(front) = self.plan.first_mut() {
                    front.begun = true;
                }
                return Some(self.yielded(PlanItem::BeginPhase(phase)));
            }
            if front.segments.is_empty() {
                let phase = self.plan.remove(0).phase;
                return Some(self.yielded(PlanItem::EndPhase(phase)));
            }
            let seg = self.plan[0].segments.remove(0);
            // CR 614.10, CR 500.11.
            if let Segment::Step(step) = seg.segment
                && seg.skippable
                && self.consume_step_skip(step)
            {
                continue;
            }
            return Some(self.yielded(PlanItem::Segment(seg.segment)));
        }
    }

    /// Records `item` as the last item yielded and returns it. CR 510.4: a combat phase that
    /// begins has no combat damage step begun yet; a yielded combat damage step has begun.
    fn yielded(&mut self, item: PlanItem) -> PlanItem {
        match item {
            PlanItem::BeginPhase(Phase::Combat) => self.damage_begun = false,
            PlanItem::Segment(s) if is_damage_step(s) => self.damage_begun = true,
            _ => {}
        }
        self.last_item = Some(item);
        item
    }

    /// Whether the next item [`TurnScheduler::next_item`] would yield is the FIRST combat
    /// damage step (first-strike or regular) of the in-progress combat phase, pending step
    /// skips taken into account (CR 614.10: a skipped step yields nothing). This is the moment
    /// the first-strike decision is made (CR 510.4: "as the combat damage step begins"),
    /// whatever plan segment preceded it (R5). False once a combat damage step of this combat
    /// phase has begun (the decision is then retained), outside a combat phase, and when the
    /// next item is not a combat damage step.
    #[must_use]
    pub fn first_damage_step_is_next(&self) -> bool {
        if self.turn_over || self.damage_begun || self.phase_in_progress() != Some(Phase::Combat) {
            return false;
        }
        let Some(active) = self.active else {
            return false;
        };
        // The skips `next_item` would consume on the way, oldest first.
        let mut consumed: Vec<Step> = Vec::new();
        for seg in &self.plan[0].segments {
            if let Segment::Step(step) = seg.segment
                && seg.skippable
            {
                let used = consumed.iter().filter(|&&s| s == step).count();
                if self.pending_step_skips(active, step) > used {
                    consumed.push(step);
                    continue;
                }
            }
            return is_damage_step(seg.segment);
        }
        false
    }

    /// The phase of the last item yielded; `None` before the first item or after the turn is
    /// over.
    #[must_use]
    pub fn current_phase(&self) -> Option<Phase> {
        if self.turn_over {
            return None;
        }
        self.last_item.map(|item| match item {
            PlanItem::BeginPhase(p) | PlanItem::EndPhase(p) => p,
            PlanItem::Segment(s) => s.phase(),
        })
    }

    /// Whether the first plan phase is in progress (begun and not ended).
    fn phase_in_progress(&self) -> Option<Phase> {
        self.plan.first().filter(|p| p.begun).map(|p| p.phase)
    }

    /// Whether a "you get" effect of `you_get` may add to the current turn (CR 500.10a: "If an
    /// effect that says "you get" an additional step or phase would add a step or phase to a
    /// turn other than its controller's, no steps or phases are added.").
    fn you_get_allows(&self, you_get: Option<PlayerId>) -> bool {
        match you_get {
            None => true,
            Some(controller) => self.active == Some(controller),
        }
    }

    /// The plan index at which a phase inserted directly after `after` goes: after the current
    /// phase if it is `after`, otherwise after the first not-yet-started occurrence of `after`.
    fn index_after_phase(&self, after: Phase) -> Option<usize> {
        if self.turn_over {
            return None;
        }
        if self.phase_in_progress() == Some(after) {
            return Some(1);
        }
        if self.phase_in_progress().is_none() && self.last_item == Some(PlanItem::EndPhase(after)) {
            // The current phase just ended: "directly after" it is the front of the plan.
            return Some(0);
        }
        self.plan
            .iter()
            .position(|p| !p.begun && p.phase == after)
            .map(|i| i + 1)
    }

    /// Inserts `inserted` directly after `after` if allowed.
    fn insert_phase_after(
        &mut self,
        after: Phase,
        inserted: PlanPhase,
        you_get: Option<PlayerId>,
    ) -> bool {
        if !self.you_get_allows(you_get) {
            return false;
        }
        match self.index_after_phase(after) {
            Some(i) => {
                // CR 500.8: "If multiple extra phases are created after the same phase, the
                // most recently created phase will occur first": inserting directly after the
                // anchor puts the newest in front of the older ones.
                self.plan.insert(i, inserted);
                self.relabel_main_phases();
                true
            }
            None => false,
        }
    }

    /// CR 500.8: "Some effects can add phases to a turn. They do this by adding the phases
    /// directly after the specified phase." Inserts `phase` after the current phase if it is
    /// `after`, otherwise after the first not-yet-started occurrence of `after`; false (nothing
    /// added) if there is none or `you_get` names a player who is not the active player
    /// (CR 500.10a). An extra main phase is a postcombat main phase, unless no main phase of
    /// the turn precedes it (CR 505.1a: only the first main phase of the turn is a precombat
    /// main phase; the main phases after it are then postcombat main phases).
    pub fn add_extra_phase(
        &mut self,
        after: Phase,
        phase: Phase,
        you_get: Option<PlayerId>,
    ) -> bool {
        let inserted = match phase {
            // CR 505.1a: labelled by `relabel_main_phases` once inserted.
            Phase::PrecombatMain | Phase::PostcombatMain => {
                PlanPhase::new(Phase::PostcombatMain, vec![Segment::PostcombatMain])
            }
            _ => PlanPhase::new(phase, segments_of(phase, self.first_strike)),
        };
        self.insert_phase_after(after, inserted, you_get)
    }

    /// The position of `anchor`: `(plan index, segment index, is the current step)`. The
    /// current step if it is `anchor`, otherwise its first not-yet-started occurrence.
    fn find_step(&self, anchor: Step) -> Option<(usize, usize, bool)> {
        if self.turn_over {
            return None;
        }
        if self.phase_in_progress().is_some()
            && self.last_item == Some(PlanItem::Segment(Segment::Step(anchor)))
        {
            return Some((0, 0, true));
        }
        self.plan.iter().enumerate().find_map(|(pi, p)| {
            p.segments
                .iter()
                .position(|s| s.segment == Segment::Step(anchor))
                .map(|si| (pi, si, false))
        })
    }

    /// Checks the CR 500.9 preconditions of an extra `step` anchored at `anchor`.
    fn step_insertion(
        &self,
        anchor: Step,
        step: Step,
        you_get: Option<PlayerId>,
    ) -> Option<(usize, usize, bool)> {
        if step.phase() != anchor.phase() || !self.you_get_allows(you_get) {
            return None;
        }
        self.find_step(anchor)
    }

    /// Inserts a skippable `step` at `(pi, si)`.
    fn insert_step(&mut self, pi: usize, si: usize, step: Step) {
        if let Some(phase) = self.plan.get_mut(pi) {
            phase.segments.insert(
                si,
                PlanSegment {
                    segment: Segment::Step(step),
                    skippable: true,
                },
            );
        }
    }

    /// CR 500.9: "Some effects can add steps to a phase. They do this by adding the steps
    /// directly after a specified step". `anchor` is the current step or the first
    /// not-yet-started occurrence; "If multiple extra steps are created after the same step,
    /// the most recently created step will occur first." False (nothing added) if `step` is
    /// not of `anchor`'s phase, there is no such anchor, or `you_get` names a player who is not
    /// the active player (CR 500.10a).
    pub fn add_extra_step_after(
        &mut self,
        anchor: Step,
        step: Step,
        you_get: Option<PlayerId>,
    ) -> bool {
        match self.step_insertion(anchor, step, you_get) {
            Some((pi, si, current)) => {
                // The current step is no longer in the plan: insert at the front of its phase.
                let at = if current { 0 } else { si + 1 };
                self.insert_step(pi, at, step);
                true
            }
            None => false,
        }
    }

    /// CR 500.9: "... or directly before a specified step." `anchor` must not have started
    /// (before the current step nothing is added). The newest step goes directly before the
    /// anchor (docs/DECISIONS.md "#50 FP-048 — Turn scheduler judgement calls"). Otherwise as
    /// [`TurnScheduler::add_extra_step_after`].
    pub fn add_extra_step_before(
        &mut self,
        anchor: Step,
        step: Step,
        you_get: Option<PlayerId>,
    ) -> bool {
        match self.step_insertion(anchor, step, you_get) {
            Some((pi, si, false)) => {
                self.insert_step(pi, si, step);
                true
            }
            _ => false,
        }
    }

    /// CR 500.10: "Some effects add a step after a particular phase. In that case, that effect
    /// first creates the phase which normally contains that step directly after the specified
    /// phase. Any other steps that phase would normally have are skipped". The created phase
    /// contains only `step`; anchored as [`TurnScheduler::add_extra_phase`]. False for a step
    /// whose phase has no steps.
    pub fn add_extra_step_after_phase(
        &mut self,
        after: Phase,
        step: Step,
        you_get: Option<PlayerId>,
    ) -> bool {
        let phase = step.phase();
        if !segments_of(phase, true).contains(&Segment::Step(step)) {
            return false;
        }
        self.insert_phase_after(
            after,
            PlanPhase::new(phase, vec![Segment::Step(step)]),
            you_get,
        )
    }

    /// CR 725.1d: "The current phase and/or step ends. ... The game skips straight to the
    /// cleanup step; skip any phases or steps between this phase or step and the cleanup step.
    /// If an effect ends the turn during the cleanup step, a new cleanup step begins."
    ///
    /// Discards every remaining item (extra phases and steps included) up to the cleanup step.
    /// What remains: the current phase's `EndPhase` (unless it is the ending phase), then the
    /// ending phase with only the cleanup step; in the ending phase, only the cleanup step and
    /// `EndPhase(Ending)`. Skips never apply to that cleanup step, and the end step never
    /// begins (CR 725.1e). Pending extra turns and skips are kept. Does nothing before the
    /// first item or after the turn is over.
    pub fn end_turn_now(&mut self) {
        if self.last_item.is_none() || self.turn_over || self.plan.is_empty() {
            return;
        }
        match self.phase_in_progress() {
            Some(Phase::Ending) => {
                self.plan.truncate(1);
                self.plan[0] = PlanPhase::forced_cleanup(true);
            }
            Some(_) => {
                self.plan.truncate(1);
                self.plan[0].segments.clear();
                self.plan.push(PlanPhase::forced_cleanup(false));
            }
            None => {
                self.plan.clear();
                self.plan.push(PlanPhase::forced_cleanup(false));
            }
        }
    }

    /// The in-progress combat phase's not-yet-started segments, if a combat phase of the
    /// current turn is in progress.
    fn combat_segments_mut(&mut self) -> Option<&mut Vec<PlanSegment>> {
        if self.turn_over || self.phase_in_progress() != Some(Phase::Combat) {
            return None;
        }
        self.plan.first_mut().map(|p| &mut p.segments)
    }

    /// CR 508.8: "If no creatures are declared as attackers or put onto the battlefield
    /// attacking, skip the declare blockers and combat damage steps." Discards the in-progress
    /// combat phase's not-yet-started declare blockers, first-strike combat damage and combat
    /// damage steps (extra ones included); the end of combat step and `EndPhase(Combat)` stay
    /// (CR 506.1). Returns whether a combat phase was in progress (false: nothing changed).
    pub fn skip_blockers_and_damage(&mut self) -> bool {
        match self.combat_segments_mut() {
            Some(segments) => {
                segments.retain(|s| !is_blockers_or_damage(s.segment));
                true
            }
            None => false,
        }
    }

    /// CR 510.4: sets, for the in-progress combat phase's not-yet-started steps, whether the
    /// combat damage step about to begin is preceded by a first-strike combat damage step.
    /// `true` inserts a first-strike step directly before the first not-yet-started combat
    /// damage step if none is planned before it; `false` removes the not-yet-started
    /// first-strike steps. Returns whether the plan changed. Outside a combat phase, or with no
    /// combat damage step left, nothing changes. Once a combat damage step (first-strike or
    /// regular) has begun in the current combat phase nothing changes either: CR 510.4 decides
    /// "as the combat damage step begins" and the second step then follows unconditionally, so
    /// a later call (after an extra declare blockers step, say) never adds a second
    /// first-strike step nor removes the decided one (R6; CR 506.1 allows two combat damage
    /// steps).
    pub fn set_first_strike_damage_step(&mut self, first_strike: bool) -> bool {
        // CR 510.4, CR 506.1 (R6): the decision is retained once a damage step has begun.
        if self.damage_begun {
            return false;
        }
        let Some(segments) = self.combat_segments_mut() else {
            return false;
        };
        let first_strike_seg = Segment::Step(Step::FirstStrikeDamage);
        let damage = segments
            .iter()
            .position(|s| s.segment == Segment::Step(Step::CombatDamage));
        if first_strike {
            let Some(at) = damage else {
                return false;
            };
            if segments[..at].iter().any(|s| s.segment == first_strike_seg) {
                return false;
            }
            segments.insert(
                at,
                PlanSegment {
                    segment: first_strike_seg,
                    skippable: true,
                },
            );
            true
        } else {
            let before = segments.len();
            segments.retain(|s| s.segment != first_strike_seg);
            segments.len() != before
        }
    }

    /// CR 725.2d: "The current combat phase ends. ... The game skips straight to the next
    /// phase". Only while a combat phase is in progress: its remaining steps are discarded and
    /// its `EndPhase(Combat)` is kept; returns true. CR 725.2g: "If an effect attempts to end
    /// the combat phase at any time that's not a combat phase, nothing happens" (false).
    pub fn end_combat_phase_now(&mut self) -> bool {
        if self.turn_over || self.phase_in_progress() != Some(Phase::Combat) {
            return false;
        }
        self.plan[0].segments.clear();
        true
    }
}

// ---------------------------------------------------------------------------------------------
// (B) Ending the turn and the combat phase
// ---------------------------------------------------------------------------------------------

/// The game-side operations of ending the turn or the combat phase (CR 725.1, CR 725.2).
pub trait EndTurnHost: HeartbeatHost {
    /// CR 725.1a: "If there are any triggered abilities that triggered before this process
    /// began but haven't been put onto the stack yet, those abilities cease to exist."
    ///
    /// # Errors
    /// Whatever the host returns.
    fn cease_pending_triggers(&mut self) -> Result<(), Interrupt>;

    /// CR 725.1b: "Exile every object on the stack, including the object that's resolving."
    ///
    /// # Errors
    /// Whatever the host returns.
    fn exile_stack(&mut self) -> Result<(), Interrupt>;

    /// CR 725.1d, CR 725.2d: "remove all creatures and planeswalkers from combat."
    ///
    /// # Errors
    /// Whatever the host returns.
    fn remove_all_from_combat(&mut self) -> Result<(), Interrupt>;
}

/// What [`end_the_turn`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndTurnReport {
    /// The phase that was current when the turn was ended.
    pub phase_ended: Option<Phase>,
}

/// Ends the turn (REQ-TURN-003; CR 725.1: "follow these steps in order").
///
/// CR 725.1a [`EndTurnHost::cease_pending_triggers`]; CR 725.1b [`EndTurnHost::exile_stack`];
/// CR 725.1c "Check state-based actions. No player gets priority, and no triggered abilities
/// are put onto the stack": [`HeartbeatHost::sba_pass`] repeats while it performs something and
/// [`HeartbeatHost::place_triggers`] is never called, so triggers that trigger now wait for the
/// cleanup step (CR 725.1f); CR 725.1d removes everything from combat if the current phase is
/// combat, then [`TurnScheduler::end_turn_now`]. The cleanup step then runs through
/// `run_segment`, which stacks those triggers and repeats cleanup (CR 725.1f, CR 514.3a).
///
/// # Errors
/// Any host interrupt; [`ChoiceError::Procedure`] if the state-based-action check does not
/// settle within [`MAX_SETTLE_ITERATIONS`] passes.
pub fn end_the_turn<H: EndTurnHost + ?Sized>(
    host: &mut H,
    scheduler: &mut TurnScheduler,
    choices: &mut ChoiceContext<'_>,
) -> Result<EndTurnReport, Interrupt> {
    let phase_ended = scheduler.current_phase();
    // CR 725.1a.
    host.cease_pending_triggers()?;
    // CR 725.1b.
    host.exile_stack()?;
    // CR 725.1c: no priority, no trigger placement.
    check_sba_without_priority(host, choices, "ending the turn")?;
    // CR 725.1d.
    if phase_ended == Some(Phase::Combat) {
        host.remove_all_from_combat()?;
    }
    scheduler.end_turn_now();
    Ok(EndTurnReport { phase_ended })
}

/// CR 725.1c, CR 725.2c: "Check state-based actions. No player gets priority, and no triggered
/// abilities are put onto the stack." [`HeartbeatHost::sba_pass`] repeats while it performs
/// something; [`HeartbeatHost::place_triggers`] is never called.
fn check_sba_without_priority<H: EndTurnHost + ?Sized>(
    host: &mut H,
    choices: &mut ChoiceContext<'_>,
    what: &str,
) -> Result<(), Interrupt> {
    let mut passes = 0usize;
    loop {
        if passes >= MAX_SETTLE_ITERATIONS {
            return Err(procedure_error(&format!(
                "the state-based-action check of {what} does not settle"
            )));
        }
        passes += 1;
        let outcome = host.sba_pass(choices).map_err(Interrupt::from)?;
        if !outcome.performed {
            return Ok(());
        }
    }
}

/// Ends the combat phase (REQ-TURN-003; CR 725.2: "follow these steps in order").
///
/// CR 725.2g: "If an effect attempts to end the combat phase at any time that's not a combat
/// phase, nothing happens": outside a combat phase nothing is called and the result is false.
/// Otherwise: CR 725.2a [`EndTurnHost::cease_pending_triggers`]; CR 725.2b
/// [`EndTurnHost::exile_stack`]; CR 725.2c the state-based-action check with no priority and
/// no trigger placement (as [`end_the_turn`]; triggers that trigger now wait for the next
/// phase, CR 725.2f); CR 725.2d [`EndTurnHost::remove_all_from_combat`], then
/// [`TurnScheduler::end_combat_phase_now`] discards the remaining combat steps, so the end of
/// combat step never begins (CR 725.2e) and "until end of combat" effects expire at the kept
/// `EndPhase(Combat)` through the sequencer. Returns true.
///
/// # Errors
/// Any host interrupt; [`ChoiceError::Procedure`] if the state-based-action check does not
/// settle within [`MAX_SETTLE_ITERATIONS`] passes.
pub fn end_the_combat_phase<H: EndTurnHost + ?Sized>(
    host: &mut H,
    scheduler: &mut TurnScheduler,
    choices: &mut ChoiceContext<'_>,
) -> Result<bool, Interrupt> {
    // CR 725.2g.
    if scheduler.turn_over || scheduler.phase_in_progress() != Some(Phase::Combat) {
        return Ok(false);
    }
    // CR 725.2a.
    host.cease_pending_triggers()?;
    // CR 725.2b.
    host.exile_stack()?;
    // CR 725.2c.
    check_sba_without_priority(host, choices, "ending the combat phase")?;
    // CR 725.2d.
    host.remove_all_from_combat()?;
    scheduler.end_combat_phase_now();
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// (C) Controlling another player
// ---------------------------------------------------------------------------------------------

/// Player-controlling effects (REQ-TURN-003; CR 724). A plain value with ordered containers
/// only (R3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerControl {
    /// Pending effects: controlled player -> controller (the last created, CR 724.1a).
    pending: BTreeMap<PlayerId, PlayerId>,
    /// The effect of the current turn: (controlled, controller).
    active: Option<(PlayerId, PlayerId)>,
}

/// A request routed to the player who decides it (CR 724.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedRequest {
    /// The player who answers the request.
    pub decider: PlayerId,
    /// The request, unchanged: its `chooser` still names the controlled player (CR 724.3).
    pub request: PendingChoiceRequest,
    /// Every player who may see the request's contents: the chooser, the decider and
    /// `reveal_to`.
    pub viewers: BTreeSet<PlayerId>,
}

/// A control-rule violation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The answer came from a player who does not decide the request (CR 724.5).
    NotTheDecider {
        /// The request's chooser.
        chooser: PlayerId,
        /// The player who decides it.
        expected: PlayerId,
        /// The player who answered.
        got: PlayerId,
    },
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::NotTheDecider {
                chooser,
                expected,
                got,
            } => write!(
                f,
                "the decision of {chooser:?} is made by {expected:?}, not {got:?}"
            ),
        }
    }
}

impl std::error::Error for ControlError {}

/// The scope of a piece of information (CR 724.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoScope {
    /// Information about an object in the game.
    InGame,
    /// Information about cards outside the game.
    OutsideGame,
}

impl PlayerControl {
    /// No effect pending or active.
    #[must_use]
    pub fn new() -> PlayerControl {
        PlayerControl::default()
    }

    /// CR 724.1: "This effect applies to the next turn that the affected player actually
    /// takes." CR 724.1a: "Multiple player-controlling effects that affect the same player
    /// overwrite each other. The last one to be created is the one that works." A self-control
    /// effect (CR 724.9) overwrites too.
    pub fn add_control_effect(&mut self, controlled: PlayerId, controller: PlayerId) {
        self.pending.insert(controlled, controller);
    }

    /// A turn of `active` actually begins. CR 724.1: "the effect doesn't end until the
    /// beginning of the next turn": the previous turn's effect ends, and a pending effect of
    /// `active` becomes this turn's (a self-control effect means no control, CR 724.9). Other
    /// pending effects wait; a skipped turn never calls this (CR 724.1b).
    pub fn begin_turn(&mut self, active: PlayerId) {
        self.active = None;
        if let Some(controller) = self.pending.remove(&active)
            && controller != active
        {
            self.active = Some((active, controller));
        }
    }

    /// `player` left the game. CR 800.4a: effects giving that player control of any objects
    /// or players end; effects controlling `player` are removed too.
    pub fn player_left(&mut self, player: PlayerId) {
        self.pending
            .retain(|&controlled, &mut controller| controlled != player && controller != player);
        if self
            .active
            .is_some_and(|(controlled, controller)| controlled == player || controller == player)
        {
            self.active = None;
        }
    }

    /// The player who makes `player`'s decisions this turn: the controller if `player` is
    /// controlled, otherwise `player` (CR 724.5, CR 724.8, CR 724.9).
    #[must_use]
    pub fn controller_of(&self, player: PlayerId) -> PlayerId {
        match self.active {
            Some((controlled, controller)) if controlled == player => controller,
            _ => player,
        }
    }

    /// Whether another player controls `player` this turn.
    #[must_use]
    pub fn is_controlled(&self, player: PlayerId) -> bool {
        self.controller_of(player) != player
    }

    /// The controller of the pending effect on `player`, if any.
    #[must_use]
    pub fn pending_controller(&self, player: PlayerId) -> Option<PlayerId> {
        self.pending.get(&player).copied()
    }

    /// Routes `request` to its decider (CR 724.5: "a player makes all choices and decisions the
    /// controlled player is allowed to make or is told to make"). CR 724.3: "Only control of
    /// the player changes", so the request is unchanged. A controller's own requests route to
    /// the controller (CR 724.8).
    #[must_use]
    pub fn route(&self, request: &PendingChoiceRequest) -> RoutedRequest {
        let decider = self.controller_of(request.chooser);
        let mut viewers: BTreeSet<PlayerId> = request.reveal_to.iter().copied().collect();
        viewers.insert(request.chooser);
        viewers.insert(decider);
        RoutedRequest {
            decider,
            request: request.clone(),
            viewers,
        }
    }

    /// Checks that `answerer` decides `request` (CR 724.5).
    ///
    /// # Errors
    /// [`ControlError::NotTheDecider`] for anybody else, the controlled player included.
    pub fn check_answerer(
        &self,
        request: &PendingChoiceRequest,
        answerer: PlayerId,
    ) -> Result<(), ControlError> {
        let expected = self.controller_of(request.chooser);
        if answerer == expected {
            Ok(())
        } else {
            Err(ControlError::NotTheDecider {
                chooser: request.chooser,
                expected,
                got: answerer,
            })
        }
    }

    /// CR 724.6: "A player may concede the game at any time, even if they are controlled by
    /// another player." Always true.
    #[must_use]
    pub fn can_concede(&self, _player: PlayerId) -> bool {
        true
    }

    /// CR 724.6: "The controller of another player can't make that player concede." True only
    /// if `decider` is `player`.
    #[must_use]
    pub fn may_concede_for(&self, decider: PlayerId, player: PlayerId) -> bool {
        decider == player
    }

    /// CR 724.5a: "The controller of another player can use only that player's resources
    /// (cards, mana, and so on) to pay costs for that player." Always `controlled`.
    #[must_use]
    pub fn cost_resource_owner(&self, controlled: PlayerId) -> PlayerId {
        controlled
    }

    /// The players whose in-game information `viewer` may see: `viewer`, plus the player
    /// `viewer` controls this turn (CR 724.4).
    #[must_use]
    pub fn viewing_players(&self, viewer: PlayerId) -> BTreeSet<PlayerId> {
        let mut players = BTreeSet::from([viewer]);
        if let Some((controlled, controller)) = self.active
            && controller == viewer
        {
            players.insert(controlled);
        }
        players
    }

    /// Whether `viewer` may see information that `subject` could see. CR 724.4: in-game
    /// information is "visible to both that player and the controller of the player";
    /// information about cards outside the game "is visible only to that player, not the
    /// controller of the player" (R12: never widened).
    #[must_use]
    pub fn can_view(&self, viewer: PlayerId, subject: PlayerId, scope: InfoScope) -> bool {
        match scope {
            InfoScope::InGame => viewer == subject || viewer == self.controller_of(subject),
            InfoScope::OutsideGame => viewer == subject,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// (D) Restarting the game
// ---------------------------------------------------------------------------------------------

/// The game-side operations of restarting the game (CR 728).
pub trait RestartHost: GameStartHost {
    /// CR 728.1: "A game that is restarted immediately ends. No players in that game win, lose,
    /// or draw that game."
    ///
    /// # Errors
    /// Whatever the host returns.
    fn end_game_without_result(&mut self) -> Result<(), Interrupt>;

    /// CR 728.5: "Effects may exempt certain cards from the procedure that restarts the game."
    fn restart_exempt_cards(&self) -> BTreeSet<ObjectId>;

    /// CR 728.2, CR 728.5: every card involved in the game, phased-out permanents included,
    /// goes back to its OWNER's library, except `exempt`; "Ownership of cards in the new game
    /// doesn't change"; everything else of the old game is discarded.
    ///
    /// # Errors
    /// Whatever the host returns.
    fn reset_for_new_game(&mut self, exempt: &BTreeSet<ObjectId>) -> Result<(), Interrupt>;

    /// CR 728.4: "The effect that restarts the game finishes resolving just before the first
    /// turn's untap step." The restarting effect's additional instructions; no player has
    /// priority.
    ///
    /// # Errors
    /// Whatever the host returns (a pending choice or a failure).
    fn finish_restarting_effect(
        &mut self,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;
}

/// What [`restart_game`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestartReport {
    /// The new game's start.
    pub start: StartReport,
    /// The cards exempt from the restart (CR 728.5).
    pub exempt: BTreeSet<ObjectId>,
}

/// Restarts the game (REQ-TURN-003; CR 728). `restarter` controls the restarting spell or
/// ability. In order: CR 728.1 the game ends with no result; CR 728.5 the exempt cards;
/// CR 728.2 the reset; CR 728.1 a new game starts and CR 728.1a "The starting player in the new
/// game is the player who controlled the spell or ability that restarted the game" (no random
/// draw); CR 728.4 the restarting effect finishes resolving. All players of the old game take
/// part (CR 728.1).
///
/// # Errors
/// [`ChoiceError::Procedure`] if `restarter` is not a player of the game (nothing is called);
/// any host or game-start interrupt.
pub fn restart_game<H: RestartHost + ?Sized>(
    host: &mut H,
    restarter: PlayerId,
    choices: &mut ChoiceContext<'_>,
) -> Result<RestartReport, Interrupt> {
    if !host.game().turn_order().contains(&restarter) {
        return Err(procedure_error("the restarting player is not a player"));
    }
    // CR 728.1.
    host.end_game_without_result()?;
    // CR 728.5.
    let exempt = host.restart_exempt_cards();
    // CR 728.2.
    host.reset_for_new_game(&exempt)?;
    // CR 728.1, CR 728.1a.
    let start = run_game_start(
        host,
        &StartOptions {
            starting_player: Some(restarter),
        },
        choices,
    )?;
    // CR 728.4.
    host.finish_restarting_effect(choices)?;
    Ok(RestartReport { start, exempt })
}

// ---------------------------------------------------------------------------------------------
// (E) Driver
// ---------------------------------------------------------------------------------------------

/// Runs `turn`, planned by [`TurnScheduler::next_turn`], through the sequencer (REQ-TURN-003):
/// [`TurnHost::begin_turn`], then [`TurnHost::perform_follow_up`] for each follow-up the
/// scheduler recorded with its installed plan, in creation order (CR 614.10b; the scheduler's
/// own copy, never `turn`'s [`PlannedTurn::follow_ups`], and only once per plan), then one
/// [`TurnScheduler::next_item`] at a time until the turn is over. `BeginPhase(p)`: [`TurnHost::begin_phase`] and the [`ExpiryPoint::BeginPhase`] expiry;
/// `Segment(s)`: [`run_segment`]; `EndPhase(p)`: the [`ExpiryPoint::EndPhase`] expiry and
/// [`TurnHost::empty_mana_pools`] (as `run_phase`). Skips are applied as each phase or step
/// would begin (CR 614.10), and changes made to `scheduler` between items take effect at the
/// next item. On success it records the turn's active player as the previous active player in
/// `state`, as `run_turn` does (CR 502.2 day/night check of the next turn).
///
/// In a combat phase, as `run_phase` does: once a declare attackers step has ended,
/// [`TurnHost::attackers_declared`] `false` calls [`TurnScheduler::skip_blockers_and_damage`]
/// (CR 508.8). CR 510.4: when the next item is the first combat damage step of the combat
/// phase ([`TurnScheduler::first_damage_step_is_next`]), whatever step preceded it (declare
/// blockers, or declare attackers when declare blockers is skipped),
/// [`TurnHost::first_strike_damage_step`] is asked once and passed to
/// [`TurnScheduler::set_first_strike_damage_step`]; the decision is then retained for the rest
/// of the combat phase.
///
/// # Errors
/// As `run_turn`; [`ChoiceError::Procedure`] before any host call if `turn` is not the turn of
/// the scheduler's installed plan (its [`PlannedTurn::plan_serial`] or player differs, or that
/// plan is already over).
pub fn run_planned_turn<H: TurnHost + ?Sized>(
    host: &mut H,
    state: &mut TurnState,
    registry: &TurnBasedRegistry,
    scheduler: &mut TurnScheduler,
    turn: &PlannedTurn,
    choices: &mut ChoiceContext<'_>,
) -> Result<TurnReport, Interrupt> {
    // F39: the handle must own the installed plan (plan identity, not only the player: two
    // successive turns can belong to the same player), and that plan must not be over.
    if turn.plan_serial != scheduler.plan_serial
        || scheduler.active != Some(turn.player)
        || scheduler.turn_over
    {
        return Err(procedure_error(
            "the planned turn is not the scheduler's installed plan",
        ));
    }
    host.begin_turn(turn.player)?;
    // CR 614.10b: "That action is considered to be the first thing that happens during the
    // next step, phase, or turn to actually occur." The scheduler's own follow-ups of its
    // installed plan, taken so they run once per plan (R4).
    for follow_up in std::mem::take(&mut scheduler.follow_ups) {
        host.perform_follow_up(follow_up, choices)?;
    }
    let mut segments = Vec::new();
    loop {
        // CR 510.4: "as the combat damage step begins" (R5: whatever step preceded it).
        if scheduler.first_damage_step_is_next() {
            scheduler.set_first_strike_damage_step(host.first_strike_damage_step());
        }
        let Some(item) = scheduler.next_item() else {
            break;
        };
        match item {
            PlanItem::BeginPhase(p) => {
                host.begin_phase(p)?;
                host.expire_effects(ExpiryPoint::BeginPhase(p))?;
            }
            PlanItem::Segment(s) => {
                segments.push(run_segment(host, state, registry, s, choices)?);
                // CR 508.8.
                if s == Segment::Step(Step::DeclareAttackers) && !host.attackers_declared() {
                    scheduler.skip_blockers_and_damage();
                }
            }
            PlanItem::EndPhase(p) => {
                host.expire_effects(ExpiryPoint::EndPhase(p))?;
                host.empty_mana_pools()?;
            }
        }
    }
    state.record_turn_taken(turn.player);
    Ok(TurnReport {
        active_player: turn.player,
        segments,
    })
}
