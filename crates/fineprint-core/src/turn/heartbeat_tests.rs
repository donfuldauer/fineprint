// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the priority-and-SBA heartbeat: REQ-TURN-001 (SRS 5.1; CR 117.3-117.5,
// CR 704.3, CR 603.3b-d, CR 800.4a, CR 800.4j), with REQ-SBA-001 for the SBA seam. Fake hosts
// and synthetic triggers only (`test.*` ids); no card data (R5).
//
// Diagram transitions (SRS 5.1): T1 SBA performed -> repeat step 1; T2 none performed -> step 2;
// T3 triggers put on stack -> step 1; T4 none put -> step 3; T5 action -> actor keeps priority,
// PassCount 0 -> step 1; T6 pass with PassCount < PlayersInGame -> next player -> step 3 (no
// settle); T7 all passed, stack non-empty -> resolve, active player, PassCount 0 -> step 1;
// T8 all passed, stack empty -> step ends.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceSession, ChoiceTarget, ChoiceType, Interrupt, PendingChoiceRequest,
    Status, ValidatorContract, ValidatorRule,
};
use crate::events::{EventContext, EventState, Phase, Step};
use crate::layers::{ContinuousEffects, LayerEnvironment, MatchContext, ObjectBase};
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable};
use crate::objects::Characteristics;
use crate::permanent::Permanents;
use crate::replacement::{BusInterrupt, ReplacementRegistry};
use crate::sba::{
    LOST_THE_GAME, SbaEnvironment, SbaFinding, SbaOutcome, SbaRegistry, SbaSubject,
    StoredSbaEnvironment,
};
use crate::state::{DesignationValue, GameState};
use crate::triggers::{
    PendingTrigger, PlacementEnv, RemovedReason, RemovedTrigger, StackedTrigger, TriggerOrigin,
    TriggerPlacement, place_triggers,
};
use crate::txn::Transactional;
use crate::zones::{Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Fake host
// ---------------------------------------------------------------------------------------------

/// One scripted state-based-action pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SbaStep {
    /// Nothing found, nothing performed.
    Nothing,
    /// Something found, nothing changed (a replacement skipped the move).
    FindingsOnly,
    /// Something performed.
    Performed,
    /// The player loses the game: performed.
    Kill(PlayerId),
    /// Asks a yes/no question first (a host-raised request inside the SBA pass).
    Ask,
}

/// A host call, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Sba,
    Place(Vec<PlayerId>),
    Resolve,
    Action(PlayerId, ChoiceResponse),
    RolledBack(PlayerId, ChoiceResponse),
}

/// One object on the fake stack.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StackObj {
    label: String,
    /// The player who loses the game in the next SBA pass after it resolves.
    kills: Option<PlayerId>,
    /// The player who leaves the game as it resolves.
    leaves_now: Option<PlayerId>,
    /// Triggers that trigger when it resolves.
    triggers: Vec<PendingTrigger>,
}

fn obj(label: &str) -> StackObj {
    StackObj {
        label: label.to_owned(),
        kills: None,
        leaves_now: None,
        triggers: Vec::new(),
    }
}

#[derive(Clone, Debug)]
struct Fake {
    game: GameState,
    /// If set, `game_mut` returns this state instead (a broken host).
    split_game: Option<GameState>,
    phase: Option<Phase>,
    step: Option<Step>,
    stack: Vec<StackObj>,
    sba_script: Vec<SbaStep>,
    sba_calls: usize,
    /// Losses waiting for the next SBA pass.
    pending_kills: Vec<PlayerId>,
    /// Every SBA pass performs something (never settles).
    endless_sba: bool,
    /// Every placement removes one trigger (never settles).
    endless_removal: bool,
    /// The waiting triggers.
    waiting: Vec<PendingTrigger>,
    /// Per placement call (0-based), how many extra triggers it removes (CR 603.3d).
    removed_script: BTreeMap<usize, usize>,
    place_calls: usize,
    actions: BTreeMap<PlayerId, Vec<ChoiceResponse>>,
    ask_on_resolve: bool,
    log: Vec<Call>,
    /// `priority_actions` calls so far in this run, and at which call (0-based) which player
    /// leaves the game (a host that models leaving at any time).
    asks: Cell<usize>,
    concede_at: BTreeMap<usize, PlayerId>,
    conceded: RefCell<BTreeSet<PlayerId>>,
    next_label: u64,
    /// The next `n` actions are reversed by the host (CR 734.1): `try_take_action` reports
    /// `RolledBack` and changes nothing but the call log.
    rollbacks: usize,
}

fn trig(controller: PlayerId, id: &str) -> PendingTrigger {
    PendingTrigger {
        origin: TriggerOrigin::Event,
        controller,
        source: None,
        ability_id: id.to_owned(),
        bindings: BTreeMap::new(),
        optional: false,
        responds_to_trigger: false,
        targets: Vec::new(),
        modes: None,
    }
}

fn cast(n: u64) -> ChoiceResponse {
    ChoiceResponse::InitiateCast {
        object: ObjectId::new(n),
        face_index: 0,
        cast_mode: "normal".to_owned(),
    }
}

fn mana(n: u64) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: ObjectId::new(n),
        ability_id: "test.mana".to_owned(),
    }
}

fn special(n: u64) -> ChoiceResponse {
    ChoiceResponse::SpecialAction {
        kind: "test.special".to_owned(),
        object: ObjectId::new(n),
    }
}

fn land(n: u64) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: ObjectId::new(n),
        face_index: 0,
    }
}

const PASS: ChoiceResponse = ChoiceResponse::PassPriority;

/// Synthetic placement environment: no slot has a legal target (no trigger here has a slot).
struct NoTargets;

impl PlacementEnv for NoTargets {
    fn legal_targets(
        &self,
        _: &PendingTrigger,
        _: Option<usize>,
        _: usize,
        _: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        Vec::new()
    }
}

fn lost(game: &GameState, p: PlayerId) -> bool {
    game.player(p)
        .is_none_or(|s| s.designation(LOST_THE_GAME).is_some())
}

fn lose(game: &mut GameState, p: PlayerId) {
    if let Some(mut s) = game.player_mut(p) {
        s.set_designation(LOST_THE_GAME, DesignationValue::Flag);
    }
}

fn outcome(performed: bool, findings: Vec<SbaFinding>, lost_players: Vec<PlayerId>) -> SbaOutcome {
    SbaOutcome {
        performed,
        findings,
        executed: Vec::new(),
        lost_players,
        loss_reasons: BTreeMap::new(),
        ceased: Vec::new(),
        unattached: Vec::new(),
        removed_from_game: Vec::new(),
        protectors_chosen: Vec::new(),
        speed_set: Vec::new(),
    }
}

fn yes_no(choices: &ChoiceContext<'_>, chooser: PlayerId, what: &str) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser,
        choice_type: ChoiceType::ChooseOptionalTrigger,
        reason: ChoiceReason {
            cr: "test.host_question".to_owned(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::No,
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Yes, ChoiceOption::No]),
        payload: ChoicePayload::ChooseOptionalTrigger {
            description: what.to_owned(),
        },
    }
}

impl Fake {
    /// `n` players at 20 life, turn 3, precombat main, active player the first in turn order.
    fn new(n: usize) -> Fake {
        let mut game = GameState::new(7, n, 20).unwrap();
        game.set_turn_number(3);
        let first = game.turn_order()[0];
        game.set_active_player(Some(first)).unwrap();
        Fake {
            game,
            split_game: None,
            phase: Some(Phase::PrecombatMain),
            step: None,
            stack: Vec::new(),
            sba_script: Vec::new(),
            sba_calls: 0,
            pending_kills: Vec::new(),
            endless_sba: false,
            endless_removal: false,
            waiting: Vec::new(),
            removed_script: BTreeMap::new(),
            place_calls: 0,
            actions: BTreeMap::new(),
            ask_on_resolve: false,
            log: Vec::new(),
            asks: Cell::new(0),
            concede_at: BTreeMap::new(),
            conceded: RefCell::new(BTreeSet::new()),
            next_label: 0,
            rollbacks: 0,
        }
    }

    fn p(&self, i: usize) -> PlayerId {
        self.game.turn_order()[i]
    }

    fn sba_count(&self) -> usize {
        self.log.iter().filter(|c| **c == Call::Sba).count()
    }

    fn resolves(&self) -> usize {
        self.log.iter().filter(|c| **c == Call::Resolve).count()
    }
}

impl HeartbeatHost for Fake {
    fn game(&self) -> &GameState {
        &self.game
    }

    fn game_mut(&mut self) -> &mut GameState {
        match &mut self.split_game {
            Some(g) => g,
            None => &mut self.game,
        }
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        (self.phase, self.step)
    }

    fn has_left_game(&self, player: PlayerId) -> bool {
        self.conceded.borrow().contains(&player) || lost(&self.game, player)
    }

    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        let index = self.sba_calls;
        let step = if self.endless_sba {
            SbaStep::Performed
        } else {
            self.sba_script
                .get(index)
                .copied()
                .unwrap_or(SbaStep::Nothing)
        };
        if step == SbaStep::Ask {
            let chooser = self.p(0);
            let request = yes_no(choices, chooser, "test.sba_question");
            choices.ask(request).map_err(BusInterrupt::from)?;
        }
        self.sba_calls += 1;
        self.log.push(Call::Sba);
        let kills: Vec<PlayerId> = std::mem::take(&mut self.pending_kills);
        if !kills.is_empty() {
            for &k in &kills {
                lose(&mut self.game, k);
            }
            return Ok(outcome(true, Vec::new(), kills));
        }
        Ok(match step {
            SbaStep::Nothing | SbaStep::Ask => outcome(false, Vec::new(), Vec::new()),
            SbaStep::FindingsOnly => outcome(
                false,
                vec![SbaFinding {
                    entry_id: "test.finding",
                    subject: SbaSubject::Object(ObjectId::new(900)),
                }],
                Vec::new(),
            ),
            SbaStep::Performed => outcome(true, Vec::new(), Vec::new()),
            SbaStep::Kill(p) => {
                lose(&mut self.game, p);
                outcome(true, Vec::new(), vec![p])
            }
        })
    }

    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        // CR 800.4a: the host drops the triggers of a player who left the game.
        let waiting: Vec<PendingTrigger> = std::mem::take(&mut self.waiting)
            .into_iter()
            .filter(|t| !self.has_left_game(t.controller))
            .collect();
        let mut placement = place_triggers(&NoTargets, choices, apnap, waiting)?;
        let extra = if self.endless_removal {
            1
        } else {
            self.removed_script
                .get(&self.place_calls)
                .copied()
                .unwrap_or(0)
        };
        for i in 0..extra {
            placement.removed.push(RemovedTrigger {
                trigger: trig(apnap[0], &format!("test.removed_{i}")),
                reason: RemovedReason::NoLegalTargets,
            });
        }
        for s in &placement.stacked {
            self.stack.push(obj(&s.trigger.ability_id));
        }
        self.place_calls += 1;
        self.log.push(Call::Place(apnap.to_vec()));
        Ok(placement)
    }

    fn stack_is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    fn resolve_top(&mut self, choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        if self.ask_on_resolve {
            let chooser = self.p(0);
            let request = yes_no(choices, chooser, "test.resolve_question");
            choices.ask(request)?;
        }
        let Some(top) = self.stack.pop() else {
            return Err(Interrupt::Error(ChoiceError::Procedure(
                "resolve_top on an empty stack".to_owned(),
            )));
        };
        if let Some(k) = top.kills {
            self.pending_kills.push(k);
        }
        if let Some(k) = top.leaves_now {
            lose(&mut self.game, k);
        }
        self.waiting.extend(top.triggers);
        self.log.push(Call::Resolve);
        Ok(())
    }

    fn priority_actions(&self, player: PlayerId) -> Vec<ChoiceResponse> {
        let k = self.asks.get();
        self.asks.set(k + 1);
        if let Some(&who) = self.concede_at.get(&k) {
            self.conceded.borrow_mut().insert(who);
        }
        self.actions.get(&player).cloned().unwrap_or_default()
    }

    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Call::Action(player, action.clone()));
        // Spells go on the stack; mana abilities, special actions and lands do not
        // (CR 605.3b, CR 116.1).
        if let ChoiceResponse::InitiateCast { object, .. } = action {
            self.next_label += 1;
            let label = format!("test.spell_{}_{}", object.get(), self.next_label);
            self.stack.push(obj(&label));
        }
        Ok(())
    }

    fn try_take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<ActionOutcome, Interrupt> {
        if self.rollbacks > 0 {
            self.rollbacks -= 1;
            self.log.push(Call::RolledBack(player, action.clone()));
            return Ok(ActionOutcome::RolledBack);
        }
        self.take_action(player, action, choices)?;
        Ok(ActionOutcome::Taken)
    }
}

/// The session state: the host world and the heartbeat state.
#[derive(Clone, Debug)]
struct World {
    host: Fake,
    hb: HeartbeatState,
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        let text = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            self.host.game.state_hash(),
            self.host.stack,
            self.host.log,
            self.host.conceded,
            self.hb
        );
        sha256(text.as_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.host.game.turn().priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

fn world(host: Fake) -> World {
    World {
        host,
        hb: HeartbeatState::new(),
    }
}

/// What a driven session ended with.
#[derive(Debug)]
struct Drive<O> {
    /// Every request asked, answered or pending, in order.
    asked: Vec<PendingChoiceRequest>,
    /// The final world and output, when done.
    done: Option<(World, O)>,
    /// The pending request, when suspended.
    pending: Option<PendingChoiceRequest>,
    /// The error, if the session failed.
    error: Option<ChoiceError>,
    /// The session's state hash at the end.
    hash: Option<Digest>,
}

fn drive_with<O, F>(w: World, answers: &[ChoiceResponse], f: F) -> Drive<O>
where
    O: Clone,
    F: Fn(&mut World, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let mut asked = Vec::new();
    let failed = |asked, e| Drive {
        asked,
        done: None,
        pending: None,
        error: Some(e),
        hash: None,
    };
    let mut session = match ChoiceSession::start(w, f) {
        Ok(s) => s,
        Err(e) => return failed(asked, e),
    };
    let mut answers = answers.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                return Drive {
                    asked,
                    done: Some((state.clone(), output.clone())),
                    pending: None,
                    error: None,
                    hash: Some(session.state_hash()),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        asked.push(pending.clone());
        let Some(answer) = answers.next() else {
            return Drive {
                asked,
                done: None,
                pending: Some(pending),
                error: None,
                hash: Some(session.state_hash()),
            };
        };
        if let Err(e) = session.submit(pending.request_id, answer.clone()) {
            return failed(asked, e);
        }
    }
}

fn heartbeat(w: &mut World, ch: &mut ChoiceContext<'_>) -> Result<HeartbeatOutcome, Interrupt> {
    run_heartbeat(&mut w.host, &mut w.hb, ch)
}

fn settle(w: &mut World, ch: &mut ChoiceContext<'_>) -> Result<SettleReport, Interrupt> {
    settle_before_priority(&mut w.host, ch)
}

fn run(host: Fake, answers: &[ChoiceResponse]) -> Drive<HeartbeatOutcome> {
    drive_with(world(host), answers, heartbeat)
}

fn choosers<O>(d: &Drive<O>) -> Vec<PlayerId> {
    d.asked.iter().map(|r| r.chooser).collect()
}

fn done(d: &Drive<HeartbeatOutcome>) -> (&World, HeartbeatOutcome) {
    let (w, o) = d.done.as_ref().expect("the heartbeat should be done");
    (w, *o)
}

fn counts(w: &World) -> Vec<usize> {
    w.hb.passes().iter().map(|r| r.pass_count).collect()
}

// ---------------------------------------------------------------------------------------------
// Settle: steps 1 and 2 (T1-T4)
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_001_t1_sba_performed_repeats_step_1() {
    // CR 704.3: "If any state-based actions are performed as a result of a check, the check is
    // repeated".
    let mut h = Fake::new(2);
    h.sba_script = vec![SbaStep::Performed, SbaStep::Performed, SbaStep::Nothing];
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(report.sba_passes, 3);
    assert_eq!(report.triggers_stacked, 0);
    let p = w.host.game.turn_order().to_vec();
    assert_eq!(
        w.host.log,
        vec![Call::Sba, Call::Sba, Call::Sba, Call::Place(p)]
    );
}

#[test]
fn req_turn_001_t1_findings_alone_do_not_repeat_step_1() {
    // CR 704.3 (docs/DECISIONS.md "#41 FP-040 — performed means state changed"): a check that
    // performed nothing is not repeated, even with findings.
    let mut h = Fake::new(2);
    h.sba_script = vec![SbaStep::FindingsOnly, SbaStep::Performed];
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(report.sba_passes, 1);
    assert_eq!(w.host.sba_calls, 1);
}

#[test]
fn req_turn_001_t2_no_sba_goes_to_trigger_placement_in_apnap_order() {
    // CR 704.3: "otherwise all triggered abilities that are waiting to be put on the stack are
    // put on the stack"; CR 603.3b: "each player, in APNAP order", the active player first.
    let mut h = Fake::new(3);
    let active = h.p(1);
    h.game.set_active_player(Some(active)).unwrap();
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(
        report,
        SettleReport {
            sba_passes: 1,
            triggers_stacked: 0
        }
    );
    let (a, b, c) = (w.host.p(0), w.host.p(1), w.host.p(2));
    assert_eq!(w.host.log, vec![Call::Sba, Call::Place(vec![b, c, a])]);
}

#[test]
fn req_turn_001_t3_stacked_triggers_return_to_step_1() {
    // CR 603.3b: "Then the game once again checks for and performs state-based actions until
    // none are performed, then abilities that triggered during this process go on the stack."
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.waiting = vec![trig(b, "test.trigger_b"), trig(a, "test.trigger_a")];
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(
        report,
        SettleReport {
            sba_passes: 2,
            triggers_stacked: 2
        }
    );
    assert_eq!(
        w.host.log,
        vec![
            Call::Sba,
            Call::Place(vec![a, b]),
            Call::Sba,
            Call::Place(vec![a, b])
        ]
    );
    // CR 603.3b APNAP: the active player's trigger goes on the stack first.
    let labels: Vec<&str> = w.host.stack.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, vec!["test.trigger_a", "test.trigger_b"]);
}

#[test]
fn req_turn_001_t3_removed_only_placement_returns_to_step_1() {
    // CR 603.3d: a trigger with no legal choice "is simply removed from the stack": it was put
    // on the stack first, so the settle returns to the SBA check (docs/DECISIONS.md "#44 FP-043
    // — Settle loop repeats on performed, not findings, and caps iterations").
    let mut h = Fake::new(2);
    h.removed_script.insert(0, 1);
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(
        report,
        SettleReport {
            sba_passes: 2,
            triggers_stacked: 0
        }
    );
    assert_eq!(w.host.place_calls, 2);
    assert_eq!(w.host.log.len(), 4);
}

#[test]
fn req_turn_001_t4_empty_placement_grants_priority_to_active_player() {
    // CR 117.5: "Then the player who would have received priority does so."; CR 117.3a: the
    // active player. The request is PRIORITY_ACTION with every legal action listed.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.actions.insert(a, vec![cast(10), land(11)]);
    let d = run(h, &[]);
    let r = d.pending.clone().expect("a priority request");
    assert_eq!(d.asked.len(), 1);
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.chooser, a);
    assert_eq!(r.choice_type, ChoiceType::PriorityAction);
    assert_eq!(r.reason.cr, "CR 117.3");
    assert_eq!(r.reason.source, None);
    assert!(!r.hidden);
    assert!(r.reveal_to.is_empty());
    assert_eq!(r.default_on_timeout, PASS);
    assert_eq!((r.min, r.max), (1, 1));
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        })
    );
    assert_eq!(
        r.payload,
        ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: vec![cast(10), land(11)],
        }
    );
}

#[test]
fn req_turn_001_settle_report_counts_passes_and_stacked_triggers() {
    // CR 117.5: SBA passes and trigger placements repeat in order until neither happens.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.sba_script = vec![SbaStep::Performed, SbaStep::Nothing, SbaStep::Performed];
    h.waiting = vec![trig(a, "test.one")];
    let d = drive_with(world(h), &[], settle);
    let (_, report) = d.done.unwrap();
    // Passes: P, N, [place 1], P, N, [place 0].
    assert_eq!(
        report,
        SettleReport {
            sba_passes: 4,
            triggers_stacked: 1
        }
    );
}

#[test]
fn req_turn_001_settle_caps_iterations_on_endless_sba() {
    // CR 704.3 loop guard: a host that never settles is an error, never an endless loop.
    let mut h = Fake::new(2);
    h.endless_sba = true;
    let d = drive_with(world(h), &[], settle);
    match d.error {
        Some(ChoiceError::Procedure(m)) => assert!(m.contains("MAX_SETTLE_ITERATIONS"), "{m}"),
        other => panic!("expected a procedure error, got {other:?}"),
    }
}

#[test]
fn req_turn_001_settle_caps_iterations_on_endless_removal() {
    // CR 603.3d with the loop guard: endless removals hit the cap too; the cap counts host
    // calls, so exactly MAX_SETTLE_ITERATIONS calls are made before the error.
    let mut h = Fake::new(2);
    h.endless_removal = true;
    let d = drive_with(world(h.clone()), &[], settle);
    assert!(matches!(d.error, Some(ChoiceError::Procedure(_))));
    let counted = drive_with(
        world(h),
        &[],
        |w: &mut World, ch: &mut ChoiceContext<'_>| {
            let r = settle_before_priority(&mut w.host, ch);
            assert!(matches!(
                r,
                Err(Interrupt::Error(ChoiceError::Procedure(_)))
            ));
            Ok(w.host.log.len())
        },
    );
    assert_eq!(counted.done.unwrap().1, MAX_SETTLE_ITERATIONS);
}

#[test]
fn req_turn_001_settle_propagates_host_requests() {
    // CR 603.3b: "each player ... puts ... on the stack in any order they choose": the host's
    // ORDER_SIMULTANEOUS_TRIGGERS request suspends the settle and resumes with the answer.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.waiting = vec![trig(a, "test.x"), trig(a, "test.y")];
    let d = drive_with(world(h.clone()), &[], settle);
    let r = d.pending.unwrap();
    assert_eq!(r.choice_type, ChoiceType::OrderSimultaneousTriggers);
    assert_eq!(r.chooser, a);
    let d = drive_with(
        world(h),
        &[ChoiceResponse::OrderTriggers { order: vec![1, 0] }],
        settle,
    );
    let (w, report) = d.done.unwrap();
    assert_eq!(report.triggers_stacked, 2);
    let labels: Vec<&str> = w.host.stack.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, vec!["test.y", "test.x"]);
}

#[test]
fn req_turn_001_settle_propagates_sba_pending_requests() {
    // CR 704.3: a choice inside the SBA pass (BusInterrupt::Pending) suspends the heartbeat.
    let mut h = Fake::new(2);
    h.sba_script = vec![SbaStep::Ask];
    let d = run(h.clone(), &[]);
    assert_eq!(
        d.pending.unwrap().choice_type,
        ChoiceType::ChooseOptionalTrigger
    );
    let d = run(h, &[ChoiceResponse::Yes]);
    assert_eq!(d.pending.unwrap().choice_type, ChoiceType::PriorityAction);
}

#[test]
fn req_turn_001_settle_with_nobody_in_game_skips_placement() {
    // CR 800.4j: players who left are skipped; with nobody left, no APNAP order exists.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    lose(&mut h.game, a);
    lose(&mut h.game, b);
    let d = drive_with(world(h), &[], settle);
    let (w, report) = d.done.unwrap();
    assert_eq!(report.sba_passes, 1);
    assert_eq!(w.host.log, vec![Call::Sba]);
}

// ---------------------------------------------------------------------------------------------
// Priority: T5-T8
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_001_t5_action_keeps_priority_and_resets_pass_count() {
    // CR 117.3c: the actor "receives priority afterward"; CR 117.4: passes count only "without
    // taking any actions in between".
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.actions.insert(b, vec![cast(20)]);
    // a passes, b casts (keeps priority), b passes, a passes -> resolve; a (active) and b pass.
    let d = run(h, &[PASS, cast(20), PASS, PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b, b, a, a, b]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    // Without the reset b's first pass would complete the round with the spell unanswered.
    assert_eq!(counts(w), vec![1, 1, 2, 1, 2]);
    assert_eq!(w.hb.passes()[2].result, PassResult::TopResolved);
    // A settle (SBA pass) ran after the action, before b got priority again (go to step 1).
    let at = w
        .host
        .log
        .iter()
        .position(|c| matches!(c, Call::Action(..)))
        .unwrap();
    assert_eq!(w.host.log[at], Call::Action(b, cast(20)));
    assert_eq!(w.host.log[at + 1], Call::Sba);
}

#[test]
fn req_turn_001_t5_mana_abilities_and_special_actions_reset_pass_count() {
    // CR 605.3b (a mana ability "doesn't go on the stack"), CR 116.1, CR 116.3 (special actions:
    // the player "receives priority afterward"); any action ends the pass sequence
    // (docs/DECISIONS.md "#44 FP-043 — Mana abilities and special actions reset PassCount").
    for action in [mana(30), special(31), land(32)] {
        let mut h = Fake::new(2);
        let (a, b) = (h.p(0), h.p(1));
        h.actions.insert(b, vec![action.clone()]);
        let d = run(h, &[PASS, action.clone(), PASS, PASS]);
        assert_eq!(choosers(&d), vec![a, b, b, a]);
        let (w, out) = done(&d);
        assert_eq!(out, HeartbeatOutcome::StepEnds);
        assert!(w.host.stack.is_empty(), "no stack object for {action:?}");
        assert_eq!(counts(w), vec![1, 1, 2]);
        assert_eq!(w.host.resolves(), 0);
        assert_eq!(w.host.sba_count(), 2, "a settle after the action");
    }
}

#[test]
fn req_turn_001_t6_pass_moves_priority_without_settle() {
    // CR 117.3d: "Then the next player in turn order receives priority." No SBA check between
    // passes (SRS 5.1: "go to step 3").
    let h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    let d = run(h.clone(), &[PASS]);
    assert_eq!(choosers(&d), vec![a, b]);
    let d = run(h, &[PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b, c]);
    let (w, _) = done(&d);
    assert_eq!(
        w.host.sba_count(),
        1,
        "one settle before the first priority only"
    );
    let results: Vec<PassResult> = w.hb.passes().iter().map(|r| r.result).collect();
    assert_eq!(
        results,
        vec![
            PassResult::PriorityMoved { to: b },
            PassResult::PriorityMoved { to: c },
            PassResult::StepEnds
        ]
    );
}

#[test]
fn req_turn_001_t7_all_pass_resolves_top_and_active_player_gets_priority() {
    // CR 117.4: "If all players pass in succession ..., the spell or ability on top of the stack
    // resolves"; CR 117.3b: "The active player receives priority after a spell or ability
    // (other than a mana ability) resolves." The caster (c) and the last passer (b) are not the
    // active player (a).
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.actions.insert(c, vec![cast(40)]);
    let answers = [PASS, PASS, cast(40), PASS, PASS, PASS];
    let d = run(h.clone(), &answers);
    assert_eq!(choosers(&d), vec![a, b, c, c, a, b, a]);
    let mut all = answers.to_vec();
    all.extend([PASS, PASS, PASS]);
    let d = run(h, &all);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    let records = w.hb.passes();
    assert_eq!(records[4].player, b);
    assert_eq!(records[4].result, PassResult::TopResolved);
    assert!(!records[4].stack_empty);
    assert_eq!(records[4].pass_count, 3);
    // After the resolution PassCount is 0 again: a's next pass counts 1.
    assert_eq!(records[5].pass_count, 1);
    assert_eq!(records[5].player, a);
    // Resolution, then a settle (go to step 1) before priority.
    let res = w.host.log.iter().position(|c| *c == Call::Resolve).unwrap();
    assert_eq!(w.host.log[res + 1], Call::Sba);
}

#[test]
fn req_turn_001_t7_resolution_with_departed_active_player_goes_to_next_player() {
    // CR 800.4j: "If the active player would receive priority, instead the next player in turn
    // order receives priority". The spell makes the active player lose (via the SBA check).
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.stack.push(StackObj {
        kills: Some(a),
        ..obj("test.kill_spell")
    });
    let d = run(h, &[PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b, c, b]);
}

#[test]
fn req_turn_001_t7_resolution_triggers_are_placed_before_priority() {
    // CR 117.5 after resolution: triggers go on the stack before anyone gets priority.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.stack.push(StackObj {
        triggers: vec![trig(b, "test.on_resolve")],
        ..obj("test.spell")
    });
    let d = run(h, &[PASS, PASS, PASS, PASS, PASS, PASS]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(choosers(&d), vec![a, b, a, b, a, b]);
    assert_eq!(w.host.resolves(), 2);
}

#[test]
fn req_turn_001_t7_host_request_during_resolution_suspends() {
    // CR 117.4 resolution may ask a player; the heartbeat propagates the host's request.
    let mut h = Fake::new(2);
    h.ask_on_resolve = true;
    h.stack.push(obj("test.spell"));
    let d = run(h.clone(), &[PASS, PASS]);
    assert_eq!(
        d.pending.unwrap().choice_type,
        ChoiceType::ChooseOptionalTrigger
    );
    let d = run(h, &[PASS, PASS, ChoiceResponse::No, PASS, PASS]);
    assert_eq!(done(&d).1, HeartbeatOutcome::StepEnds);
}

#[test]
fn req_turn_001_t8_all_pass_with_empty_stack_ends_step() {
    // CR 117.4: "if the stack is empty, the phase or step ends." The priority player is None
    // (docs/DECISIONS.md "#44 FP-043 — Priority player is None when the step ends").
    let h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    let d = run(h, &[PASS, PASS]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(w.host.game.turn().priority_player(), None);
    let last = w.hb.passes().last().unwrap();
    assert_eq!(last.player, b);
    assert_eq!(last.result, PassResult::StepEnds);
    assert!(last.stack_empty);
    assert_eq!((last.pass_count, last.players_in_game), (2, 2));
    assert_eq!(choosers(&d), vec![a, b]);
    // Nothing else happens: no resolution, one settle.
    assert_eq!(w.host.resolves(), 0);
    assert_eq!(w.host.sba_count(), 1);
}

#[test]
fn req_turn_001_t8_multiplayer_full_round_ends_step() {
    // CR 117.4 with four players: three passes are not "all players".
    let h = Fake::new(4);
    let d = run(h.clone(), &[PASS, PASS, PASS]);
    assert!(d.pending.is_some());
    let d = run(h, &[PASS, PASS, PASS, PASS]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(counts(w), vec![1, 2, 3, 4]);
}

// ---------------------------------------------------------------------------------------------
// Entry, departed players, pass log
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_001_entry_resets_pass_count_and_log_keeps_growing() {
    // CR 117.4: a new step starts a new pass sequence; the pass log keeps every pass.
    let h = Fake::new(2);
    let d = run(h, &[PASS, PASS]);
    let (w, _) = done(&d);
    assert_eq!(w.hb.pass_count(), 2);
    let mut next = w.clone();
    next.host.step = Some(Step::End);
    next.host.phase = Some(Phase::Ending);
    let d = drive_with(next, &[PASS, PASS], heartbeat);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    let seqs: Vec<u64> = w.hb.passes().iter().map(|r| r.sequence).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3]);
    assert_eq!(counts(w), vec![1, 2, 1, 2]);
    assert_eq!(w.hb.passes()[2].step, Some(Step::End));
    assert_eq!(w.hb.passes()[0].step, None);
}

#[test]
fn req_turn_001_entry_with_no_player_in_game_asks_nothing() {
    // CR 800.4j: players who left are skipped; with nobody in the game nobody is asked.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    lose(&mut h.game, a);
    lose(&mut h.game, b);
    h.game.set_priority_player(Some(a)).unwrap();
    let d = run(h, &[]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::NoPlayerInGame);
    assert!(d.asked.is_empty());
    assert!(w.host.log.is_empty());
    assert_eq!(w.host.game.turn().priority_player(), None);
}

#[test]
fn req_turn_001_entry_active_player_left_next_player_gets_priority() {
    // CR 800.4j: "If the active player would receive priority, instead the next player in turn
    // order receives priority"; APNAP order starts there too.
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    lose(&mut h.game, a);
    let d = run(h, &[PASS, PASS]);
    assert_eq!(choosers(&d), vec![b, c]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(w.host.log[1], Call::Place(vec![b, c]));
    assert_eq!(w.hb.passes()[1].players_in_game, 2);
}

#[test]
fn req_turn_001_entry_active_player_left_wraps_past_seat() {
    // CR 800.4j with CR 800.4a: the next player in turn order after the departed active
    // player's seat, wrapping; APNAP starts at that player.
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.game.set_active_player(Some(c)).unwrap();
    lose(&mut h.game, c);
    let d = run(h.clone(), &[]);
    assert_eq!(choosers(&d), vec![a]);
    let d = drive_with(world(h), &[], settle);
    assert_eq!(d.done.unwrap().0.host.log[1], Call::Place(vec![a, b]));
}

#[test]
fn req_turn_001_entry_no_active_player_starts_at_first_in_game() {
    // CR 800.4j: the turn continues without an active player (docs/DECISIONS.md "#44 FP-043 —
    // Priority start when the active player has left or is absent").
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.game.set_active_player(None).unwrap();
    lose(&mut h.game, a);
    let d = run(h, &[PASS, PASS]);
    assert_eq!(choosers(&d), vec![b, c]);
    assert_eq!(done(&d).0.host.log[1], Call::Place(vec![b, c]));
}

#[test]
fn req_turn_001_priority_player_lost_during_settle_moves_on() {
    // CR 800.4a: "If the player who left the game had priority at the time they left, priority
    // passes to the next player in turn order who's still in the game."
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.sba_script = vec![SbaStep::Kill(a)];
    let d = run(h, &[PASS]);
    assert_eq!(choosers(&d), vec![b]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    let r = &w.hb.passes()[0];
    assert_eq!((r.player, r.pass_count, r.players_in_game), (b, 1, 1));
}

#[test]
fn req_turn_001_last_player_lost_during_settle_ends_with_no_player() {
    // CR 800.4j: with nobody left in the game, nobody receives priority.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    lose(&mut h.game, b);
    h.sba_script = vec![SbaStep::Kill(a)];
    let d = run(h, &[]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::NoPlayerInGame);
    assert!(d.asked.is_empty());
    assert_eq!(w.host.game.turn().priority_player(), None);
}

#[test]
fn req_turn_001_last_player_left_on_resolution_ends_with_no_player() {
    // CR 800.4j after a resolution (CR 117.3b): no player left to receive priority.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    lose(&mut h.game, b);
    h.stack.push(StackObj {
        leaves_now: Some(a),
        ..obj("test.leave_spell")
    });
    let d = run(h, &[PASS]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::NoPlayerInGame);
    assert_eq!(w.host.resolves(), 1);
    assert_eq!(w.host.game.turn().priority_player(), None);
}

#[test]
fn req_turn_001_left_players_are_never_asked_or_counted() {
    // CR 800.4j: players who left are skipped: never asked, never counted (4 players, one left).
    let mut h = Fake::new(4);
    let (a, b, c, e) = (h.p(0), h.p(1), h.p(2), h.p(3));
    lose(&mut h.game, b);
    let d = run(h, &[PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, c, e]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    for r in w.hb.passes() {
        assert_eq!(r.players_in_game, 3);
        assert_ne!(r.player, b);
    }
    assert_eq!(players_in_game(&w.host), 3);
}

#[test]
fn req_turn_001_departure_between_passes_uses_players_still_in_game() {
    // CR 117.4 tested against the players still in the game (docs/DECISIONS.md "#44 FP-043 —
    // Pass counting uses the players still in the game"): a passes, then a leaves while b
    // decides; b's pass counts 1 of 2 (a's pass no longer counts), so c still gets priority.
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.concede_at.insert(1, a);
    let d = run(h, &[PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b, c]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    let r: Vec<(usize, usize)> =
        w.hb.passes()
            .iter()
            .map(|r| (r.pass_count, r.players_in_game))
            .collect();
    assert_eq!(r, vec![(1, 3), (1, 2), (2, 2)]);
    assert_eq!(w.hb.pass_count(), 2);
}

#[test]
fn req_turn_001_departure_between_passes_skips_departed_next_player() {
    // CR 800.4j: a player who left between passes is skipped by the next pass.
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.concede_at.insert(0, b);
    let d = run(h, &[PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, c]);
    let (w, _) = done(&d);
    assert_eq!(w.hb.passes()[0].result, PassResult::PriorityMoved { to: c });
}

#[test]
fn req_turn_001_player_who_left_while_deciding_is_not_recorded() {
    // REQ-TURN-001, CR 800.4a: the player with priority left while their actions were being
    // collected, before the request was published: they are not asked (F43), priority passes
    // to the next player still in the game and no pass is recorded for the departed player.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.concede_at.insert(0, a);
    let d = run(h, &[PASS, PASS]);
    assert_eq!(choosers(&d), vec![b]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(w.hb.passes().len(), 1);
    assert_eq!(w.hb.passes()[0].player, b);
    assert_eq!(w.hb.passes()[0].players_in_game, 1);
}

#[test]
fn req_turn_001_departure_while_deciding_after_all_others_passed_ends_the_step() {
    // REQ-TURN-001, CR 117.4, CR 800.4a, CR 800.4j: a and b pass, c leaves while deciding
    // (before the request is published). Every player still in the game has passed in
    // succession, so with an empty stack the step ends: nobody is asked again and no pass is
    // recorded for the departure (F45).
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.concede_at.insert(2, c);
    let d = run(h, &[PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(w.hb.passes().len(), 2);
    assert_eq!(w.hb.passes()[1].result, PassResult::PriorityMoved { to: c });
    assert_eq!(w.host.game.turn().priority_player(), None);
    assert_eq!(w.host.resolves(), 0);
}

#[test]
fn req_turn_001_departure_while_deciding_after_all_others_passed_resolves_the_top() {
    // REQ-TURN-001, CR 117.4, CR 117.3b, CR 800.4j: as above with an object on the stack: it
    // resolves without another request, then the active player receives priority (F45).
    let mut h = Fake::new(3);
    let (a, b, c) = (h.p(0), h.p(1), h.p(2));
    h.stack.push(obj("test.spell"));
    h.concede_at.insert(2, c);
    let d = run(h, &[PASS, PASS, PASS, PASS]);
    assert_eq!(choosers(&d), vec![a, b, a, b]);
    let (w, out) = done(&d);
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(w.host.resolves(), 1);
    // The resolution comes before the next settle and the active player's request.
    let resolve = w.host.log.iter().position(|e| *e == Call::Resolve).unwrap();
    assert_eq!(w.host.log[resolve + 1], Call::Sba);
    let results: Vec<PassResult> = w.hb.passes().iter().map(|r| r.result).collect();
    assert_eq!(
        results,
        vec![
            PassResult::PriorityMoved { to: b },
            PassResult::PriorityMoved { to: c },
            PassResult::PriorityMoved { to: b },
            PassResult::StepEnds,
        ]
    );
}

#[test]
fn req_turn_001_rolled_back_action_keeps_priority() {
    // REQ-TURN-001, REQ-CORE-005, CR 734.2: "the player who had priority retains it and may
    // take another action or pass": a cast the host reverses (CR 734.1, CR 601.2) gives the
    // same player a second PRIORITY_ACTION request; nothing is settled in between, the pass
    // sequence is unchanged and the heartbeat then completes (F44).
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.actions.insert(b, vec![cast(5)]);
    h.rollbacks = 1;
    let d = run(h, &[PASS, cast(5), PASS]);
    assert_eq!(choosers(&d), vec![a, b, b]);
    for r in &d.asked {
        assert_eq!(r.choice_type, ChoiceType::PriorityAction);
    }
    let (w, out) = done(&d);
    // a's pass stood through the rollback: b's pass completes the round.
    assert_eq!(out, HeartbeatOutcome::StepEnds);
    assert_eq!(
        w.host.log,
        vec![
            Call::Sba,
            Call::Place(vec![a, b]),
            Call::RolledBack(b, cast(5)),
        ]
    );
    assert!(w.host.stack.is_empty());
    assert_eq!(counts(w), vec![1, 2]);
    assert_eq!(w.hb.passes()[1].player, b);
}

#[test]
fn req_turn_001_default_try_take_action_takes_the_action() {
    // REQ-TURN-001, CR 117.3c: the default `try_take_action` takes the action through
    // `take_action` and reports it taken. Gate 06-turn R19: the call itself is asserted, not
    // only the outcome.
    let base = Bare::new(GameState::new(3, 2, 20).unwrap());
    let a = base.game.turn_order()[0];
    let session = ChoiceSession::start(base, move |b: &mut Bare, c: &mut ChoiceContext<'_>| {
        b.try_take_action(a, &cast(1), c)
    })
    .unwrap();
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, ActionOutcome::Taken);
            assert_eq!(state.taken, vec![(a, cast(1))]);
        }
        Status::Pending(r) => panic!("unexpected request {r:?}"),
    }
}

#[test]
fn req_turn_001_pass_log_records_every_pass_explicitly() {
    // CR 117.3d: each pass is a PASS_PRIORITY answer and is recorded (SRS 5.1: "record every
    // pass explicitly in the replay log").
    let mut h = Fake::new(2);
    h.game.set_turn_number(9);
    h.step = Some(Step::Upkeep);
    h.phase = Some(Phase::Beginning);
    let (a, b) = (h.p(0), h.p(1));
    h.stack.push(obj("test.spell"));
    let d = run(h, &[PASS, PASS, PASS, PASS]);
    let (w, _) = done(&d);
    let rec = |sequence, player, pass_count, stack_empty, result| PassRecord {
        sequence,
        turn_number: 9,
        phase: Some(Phase::Beginning),
        step: Some(Step::Upkeep),
        player,
        pass_count,
        players_in_game: 2,
        stack_empty,
        result,
    };
    let expected = vec![
        rec(0, a, 1, false, PassResult::PriorityMoved { to: b }),
        rec(1, b, 2, false, PassResult::TopResolved),
        rec(2, a, 1, true, PassResult::PriorityMoved { to: b }),
        rec(3, b, 2, true, PassResult::StepEnds),
    ];
    assert_eq!(w.hb.passes(), expected.as_slice());
    // Every PASS_PRIORITY answer produced exactly one record, in order.
    assert_eq!(d.asked.len(), w.hb.passes().len());
}

#[test]
fn req_turn_001_failed_priority_write_is_a_procedure_error() {
    // CR 117.3: a host whose game state cannot record the priority player fails cleanly.
    let mut h = Fake::new(3);
    h.split_game = Some(GameState::new(1, 1, 20).unwrap());
    let third = h.p(2);
    h.game.set_active_player(Some(third)).unwrap();
    let d = run(h, &[]);
    match d.error {
        Some(ChoiceError::Procedure(m)) => assert!(m.contains("priority player"), "{m}"),
        other => panic!("expected a procedure error, got {other:?}"),
    }
}

#[test]
fn req_turn_001_state_new_is_empty() {
    // CR 117.4: a fresh heartbeat state has no pass counted and an empty log.
    let s = HeartbeatState::new();
    assert_eq!(s.pass_count(), 0);
    assert!(s.passes().is_empty());
    assert_eq!(s, HeartbeatState::default());
}

// ---------------------------------------------------------------------------------------------
// Turn-order helpers and the default has_left_game
// ---------------------------------------------------------------------------------------------

/// A host that keeps every default method.
#[derive(Clone, Debug)]
struct Bare {
    game: GameState,
    hb: HeartbeatState,
    /// Every `take_action` call, in order (R19: the default `try_take_action` must make it).
    taken: Vec<(PlayerId, ChoiceResponse)>,
}

impl Bare {
    fn new(game: GameState) -> Bare {
        Bare {
            game,
            hb: HeartbeatState::new(),
            taken: Vec::new(),
        }
    }
}

impl Transactional for Bare {
    fn state_hash(&self) -> Digest {
        sha256(format!("{:?}{:?}", self.game.state_hash(), self.hb).as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        self.game.turn().priority_player()
    }
    fn carry_durable(r: Self, _: &Self, _: &Self) -> Self {
        r
    }
}

impl HeartbeatHost for Bare {
    fn game(&self) -> &GameState {
        &self.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }
    fn sba_pass(&mut self, _: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        Ok(outcome(false, Vec::new(), Vec::new()))
    }
    fn place_triggers(
        &mut self,
        _: &[PlayerId],
        _: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        Ok(TriggerPlacement::default())
    }
    fn stack_is_empty(&self) -> bool {
        true
    }
    fn resolve_top(&mut self, _: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        Ok(())
    }
    fn priority_actions(&self, _: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }
    fn take_action(
        &mut self,
        player: PlayerId,
        action: &ChoiceResponse,
        _: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.taken.push((player, action.clone()));
        Ok(())
    }
}

#[test]
fn req_turn_001_default_has_left_game_reads_lost_the_game() {
    // CR 800.4j: a player who lost the game (the LOST_THE_GAME designation) has left; an unknown
    // id is not a player of the game.
    let mut game = GameState::new(3, 3, 20).unwrap();
    let order = game.turn_order().to_vec();
    lose(&mut game, order[1]);
    let host = Bare::new(game);
    assert!(!host.has_left_game(order[0]));
    assert!(host.has_left_game(order[1]));
    assert!(host.has_left_game(PlayerId::new(999)));
    assert_eq!(host.phase_and_step(), (None, None));
    assert_eq!(players_in_game(&host), 2);
}

#[test]
fn req_turn_001_next_player_in_turn_order_wraps_and_skips() {
    // CR 117.3d, CR 800.4a: "the next player in turn order who's still in the game".
    let mut game = GameState::new(3, 4, 20).unwrap();
    let o = game.turn_order().to_vec();
    let host = Bare::new(game.clone());
    assert_eq!(next_player_in_turn_order(&host, o[0]), Some(o[1]));
    assert_eq!(next_player_in_turn_order(&host, o[3]), Some(o[0]));
    lose(&mut game, o[1]);
    lose(&mut game, o[2]);
    let host = Bare::new(game.clone());
    assert_eq!(next_player_in_turn_order(&host, o[0]), Some(o[3]));
    // `after` may itself have left.
    assert_eq!(next_player_in_turn_order(&host, o[1]), Some(o[3]));
    // Not a player: the first player still in the game.
    assert_eq!(
        next_player_in_turn_order(&host, PlayerId::new(999)),
        Some(o[0])
    );
    lose(&mut game, o[3]);
    let host = Bare::new(game.clone());
    // The only player left is returned for itself.
    assert_eq!(next_player_in_turn_order(&host, o[0]), Some(o[0]));
    lose(&mut game, o[0]);
    let host = Bare::new(game);
    assert_eq!(next_player_in_turn_order(&host, o[0]), None);
    assert_eq!(players_in_game(&host), 0);
}

#[test]
fn req_turn_001_default_host_step_ends_after_full_round() {
    // CR 117.4 with the default host methods (two players, two passes).
    let game = GameState::new(3, 2, 20).unwrap();
    let f = |s: &mut Bare, ch: &mut ChoiceContext<'_>| {
        let mut hb = std::mem::take(&mut s.hb);
        let out = run_heartbeat(s, &mut hb, ch);
        s.hb = hb;
        out
    };
    let session = ChoiceSession::replay(Bare::new(game), f, &[PASS, PASS]).unwrap();
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, HeartbeatOutcome::StepEnds);
            assert_eq!(state.hb.passes().len(), 2);
            assert_eq!(state.hb.passes()[0].phase, None);
        }
        Status::Pending(r) => panic!("unexpected pending {r:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_001_replay_is_deterministic() {
    // REQ-CORE-001 with CR 117.4: the same base and answers give the same state, pass log and
    // request sequence, whether submitted one by one or replayed.
    let mut h = Fake::new(3);
    let (b, c) = (h.p(1), h.p(2));
    h.actions.insert(b, vec![cast(5), mana(6)]);
    h.actions.insert(c, vec![special(7)]);
    h.waiting = vec![trig(c, "test.w1")];
    h.sba_script = vec![SbaStep::Performed, SbaStep::Nothing, SbaStep::FindingsOnly];
    let answers = [
        PASS,
        PASS,
        PASS,
        PASS,
        cast(5),
        mana(6),
        PASS,
        special(7),
        PASS,
        PASS,
        PASS,
        PASS,
        PASS,
        PASS,
    ];
    let one = run(h.clone(), &answers);
    let two = run(h.clone(), &answers);
    assert_eq!(one.asked, two.asked);
    assert_eq!(one.hash, two.hash);
    let ids: Vec<u64> = one.asked.iter().map(|r| r.request_id.get()).collect();
    let expected: Vec<u64> = (0..u64::try_from(ids.len()).unwrap()).collect();
    assert_eq!(ids, expected);
    let (w1, o1) = done(&one);
    let (w2, o2) = done(&two);
    assert_eq!(o1, o2);
    assert_eq!(w1.hb, w2.hb);
    assert_eq!(w1.host.log, w2.host.log);
    let replayed = ChoiceSession::replay(world(h), heartbeat, &answers).unwrap();
    assert_eq!(Some(replayed.state_hash()), one.hash);
}

// ---------------------------------------------------------------------------------------------
// The real SBA seam: sba_pass_post_batch
// ---------------------------------------------------------------------------------------------

/// No objects: the layer and LKI views of an empty board.
struct EmptyBoard;

impl LayerEnvironment for EmptyBoard {
    fn objects(&self) -> Vec<ObjectId> {
        Vec::new()
    }
    fn base(&self, _: ObjectId) -> Option<ObjectBase> {
        None
    }
    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }
}

impl CharacteristicsProvider for EmptyBoard {
    fn characteristics(&self, _: ObjectId) -> Option<Characteristics> {
        None
    }
}

/// A host over the real SBA components; its `sba_pass` delegates to `sba_pass_post_batch`.
#[derive(Clone, Debug)]
struct RealSba {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
    replacements: ReplacementRegistry,
    effects: ContinuousEffects,
    env: StoredSbaEnvironment,
    hb: HeartbeatState,
    post_batch_calls: usize,
}

impl RealSba {
    fn new() -> RealSba {
        let mut game = GameState::new(11, 2, 20).unwrap();
        game.set_turn_number(1);
        let order = game.turn_order().to_vec();
        game.set_active_player(Some(order[0])).unwrap();
        RealSba {
            zones: Zones::new(&order, ZonesConfig::default()),
            game,
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            events: EventState::new(),
            replacements: ReplacementRegistry::new(),
            effects: ContinuousEffects::new(),
            env: StoredSbaEnvironment::default(),
            hb: HeartbeatState::new(),
            post_batch_calls: 0,
        }
    }
}

impl Transactional for RealSba {
    fn state_hash(&self) -> Digest {
        sha256(format!("{:?}{:?}", self.game.state_hash(), self.hb).as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        self.game.turn().priority_player()
    }
    fn carry_durable(r: Self, _: &Self, _: &Self) -> Self {
        r
    }
}

impl HeartbeatHost for RealSba {
    fn game(&self) -> &GameState {
        &self.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }
    fn sba_pass(&mut self, choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        let registry = SbaRegistry::standard();
        let board = EmptyBoard;
        let effects = self.effects.clone();
        let view_env = StoredSbaEnvironment::default();
        let mut calls = 0usize;
        let mut provider = |_: &EventContext<'_>,
                            view: &mut dyn FnMut(
            &ContinuousEffects,
            &dyn LayerEnvironment,
            &dyn SbaEnvironment,
        )| {
            calls += 1;
            view(&effects, &EmptyBoard, &view_env);
        };
        let mut ctx = EventContext {
            game: &mut self.game,
            zones: &mut self.zones,
            ids: &mut self.ids,
            permanents: &mut self.permanents,
            memories: &mut self.memories,
            lki: &mut self.lki,
            events: &mut self.events,
            provider: &board,
        };
        let out = sba_pass_post_batch(
            SbaStage {
                registry: &registry,
                ctx: &mut ctx,
                replacements: &mut self.replacements,
                effects: &effects,
                layer_env: &board,
                env: &mut self.env,
                post_batch: &mut provider,
            },
            choices,
        );
        self.post_batch_calls += calls;
        out
    }
    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        place_triggers(&NoTargets, choices, apnap, Vec::new())
    }
    fn stack_is_empty(&self) -> bool {
        true
    }
    fn resolve_top(&mut self, _: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        Ok(())
    }
    fn priority_actions(&self, _: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }
    fn take_action(
        &mut self,
        _: PlayerId,
        _: &ChoiceResponse,
        _: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

fn real_heartbeat(
    w: &mut RealSba,
    ch: &mut ChoiceContext<'_>,
) -> Result<HeartbeatOutcome, Interrupt> {
    let mut hb = std::mem::take(&mut w.hb);
    let out = run_heartbeat(w, &mut hb, ch);
    w.hb = hb;
    out
}

#[test]
fn req_turn_001_sba_pass_post_batch_performs_one_real_pass() {
    // CR 704.3 through sba::perform_all_post_batch (REQ-SBA-001): a player at 0 life loses
    // in the first pass; the second pass performs nothing.
    let mut w = RealSba::new();
    let order = w.game.turn_order().to_vec();
    w.game.player_mut(order[1]).unwrap().set_life(0);
    let f = |w: &mut RealSba, ch: &mut ChoiceContext<'_>| {
        let first = w.sba_pass(ch).map_err(Interrupt::from)?;
        let second = w.sba_pass(ch).map_err(Interrupt::from)?;
        Ok((first, second))
    };
    let session = ChoiceSession::start(w, f).unwrap();
    match session.status() {
        Status::Done { state, output } => {
            let (first, second) = output;
            assert!(first.performed);
            assert_eq!(first.lost_players, vec![order[1]]);
            assert!(!second.performed);
            assert!(state.has_left_game(order[1]));
            // An ordinary pass never calls the post-batch provider.
            assert_eq!(state.post_batch_calls, 0);
        }
        Status::Pending(r) => panic!("unexpected pending {r:?}"),
    }
}

#[test]
fn req_turn_001_heartbeat_over_real_sba_skips_the_player_who_lost() {
    // CR 117.5, CR 704.3, CR 800.4a: the active player at 0 life loses in step 1, so the other
    // player receives priority; one pass then ends the step (one player in the game).
    let mut w = RealSba::new();
    let order = w.game.turn_order().to_vec();
    w.game.player_mut(order[0]).unwrap().set_life(0);
    let session = ChoiceSession::start(w.clone(), real_heartbeat).unwrap();
    assert_eq!(session.pending().unwrap().chooser, order[1]);
    let session = ChoiceSession::replay(w, real_heartbeat, &[PASS]).unwrap();
    match session.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, HeartbeatOutcome::StepEnds);
            assert_eq!(state.hb.passes()[0].players_in_game, 1);
            assert!(lost(&state.game, order[0]));
        }
        Status::Pending(r) => panic!("unexpected pending {r:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Property tests: the heartbeat terminates or asks for a choice
// ---------------------------------------------------------------------------------------------

/// A fixed-seed linear congruential generator (no system randomness, R3).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next()).unwrap() % n
        }
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

fn generate(rng: &mut Lcg) -> Fake {
    let n = 2 + rng.below(3);
    let mut h = Fake::new(n);
    let order = h.game.turn_order().to_vec();
    let pick = |rng: &mut Lcg| order[rng.below(order.len())];
    if rng.chance(15) {
        h.game.set_active_player(None).unwrap();
    } else {
        let a = pick(rng);
        h.game.set_active_player(Some(a)).unwrap();
    }
    for &p in &order {
        if rng.chance(20) {
            lose(&mut h.game, p);
        }
    }
    for _ in 0..rng.below(6) {
        let step = match rng.below(5) {
            0 => SbaStep::Nothing,
            1 => SbaStep::FindingsOnly,
            2 => SbaStep::Performed,
            3 => SbaStep::Kill(pick(rng)),
            _ => SbaStep::Ask,
        };
        h.sba_script.push(step);
    }
    for i in 0..rng.below(4) {
        let c = pick(rng);
        h.waiting.push(trig(c, &format!("test.gen_{i}")));
    }
    for k in 0..rng.below(3) {
        h.removed_script.insert(k, 1 + rng.below(2));
    }
    for i in 0..rng.below(4) {
        let kills = if rng.chance(25) {
            Some(pick(rng))
        } else {
            None
        };
        let leaves_now = if rng.chance(10) {
            Some(pick(rng))
        } else {
            None
        };
        let triggers = if rng.chance(30) {
            vec![trig(pick(rng), &format!("test.res_{i}"))]
        } else {
            Vec::new()
        };
        h.stack.push(StackObj {
            label: format!("test.stack_{i}"),
            kills,
            leaves_now,
            triggers,
        });
    }
    h.ask_on_resolve = rng.chance(20);
    for (j, &p) in order.iter().enumerate() {
        let mut acts = Vec::new();
        for k in 0..rng.below(3) {
            let id = u64::try_from(j * 10 + k).unwrap();
            acts.push(match rng.below(4) {
                0 => cast(id),
                1 => mana(id),
                2 => special(id),
                _ => land(id),
            });
        }
        h.actions.insert(p, acts);
    }
    if rng.chance(20) {
        let at = rng.below(6);
        let who = pick(rng);
        h.concede_at.insert(at, who);
    }
    h
}

/// An answer to `request`: for PRIORITY_ACTION a pass (mostly) or one of the actions; for a
/// host request its default.
fn answer(rng: &mut Lcg, request: &PendingChoiceRequest) -> ChoiceResponse {
    match &request.payload {
        ChoicePayload::PriorityAction { actions, .. } if !actions.is_empty() && rng.chance(25) => {
            actions[rng.below(actions.len())].clone()
        }
        _ => request.default_on_timeout.clone(),
    }
}

#[test]
fn req_turn_001_property_heartbeat_terminates_or_asks() {
    // CR 117.4, CR 117.5, CR 704.3, CR 800.4j over 300 generated hosts (2-4 players, random
    // left-game sets, SBA/trigger scripts, stack depths, action scripts): every run either ends
    // (Ok) or suspends on a PRIORITY_ACTION or a host-raised request; it never errors and never
    // hits the settle cap. The pass log invariants hold at the end.
    let mut rng = Lcg(0x5EED_0043);
    let (mut ended, mut asked) = (0, 0);
    for case in 0..300 {
        let host = generate(&mut rng);
        let mut session = ChoiceSession::start(world(host), heartbeat)
            .unwrap_or_else(|e| panic!("case {case}: start failed: {e:?}"));
        for _ in 0..80 {
            let request = match session.status() {
                Status::Done { state: w, output } => {
                    ended += 1;
                    for (i, r) in w.hb.passes().iter().enumerate() {
                        assert_eq!(r.sequence, u64::try_from(i).unwrap(), "case {case}");
                        assert!(r.pass_count >= 1, "case {case}");
                        assert!(r.pass_count <= r.players_in_game, "case {case}");
                    }
                    match output {
                        HeartbeatOutcome::StepEnds => {
                            let last = w.hb.passes().last().expect("a pass ends the step");
                            assert!(w.host.stack.is_empty(), "case {case}");
                            if last.result == PassResult::StepEnds {
                                assert!(last.stack_empty);
                                assert_eq!(last.pass_count, last.players_in_game);
                            } else {
                                // F45, CR 117.4, CR 800.4j: a departure after every player
                                // still in the game had passed ends the step with no new pass.
                                assert!(players_in_game(&w.host) > 0, "case {case}");
                                assert_eq!(
                                    w.hb.pass_count(),
                                    players_in_game(&w.host),
                                    "case {case}"
                                );
                            }
                        }
                        HeartbeatOutcome::NoPlayerInGame => {
                            assert_eq!(players_in_game(&w.host), 0, "case {case}");
                        }
                    }
                    assert_eq!(w.host.game.turn().priority_player(), None);
                    break;
                }
                Status::Pending(r) => r.clone(),
            };
            asked += 1;
            match request.choice_type {
                ChoiceType::PriorityAction
                | ChoiceType::OrderSimultaneousTriggers
                | ChoiceType::ChooseOptionalTrigger => {}
                other => panic!("case {case}: unexpected request type {other:?}"),
            }
            let a = answer(&mut rng, &request);
            session
                .submit(request.request_id, a)
                .unwrap_or_else(|e| panic!("case {case}: submit failed: {e:?}"));
        }
    }
    assert!(ended > 100, "most generated hosts end the step ({ended})");
    assert!(asked > 300);
}

#[test]
fn req_turn_001_property_never_asks_a_departed_player() {
    // CR 800.4j: across 200 generated hosts, every PRIORITY_ACTION chooser is still in the game
    // and holds priority in the run that asks (checked inside the procedure at the ask).
    let mut rng = Lcg(0xA5A5_0043);
    for case in 0..200 {
        let mut host = generate(&mut rng);
        // Leaving while deciding is covered separately; here departures come from the game.
        host.concede_at.clear();
        let mut answers = Vec::new();
        for _ in 0..30 {
            let probe = |w: &mut World, ch: &mut ChoiceContext<'_>| match run_heartbeat(
                &mut w.host,
                &mut w.hb,
                ch,
            ) {
                Err(Interrupt::Pending(r)) => {
                    if r.choice_type == ChoiceType::PriorityAction {
                        assert!(
                            !w.host.has_left_game(r.chooser),
                            "case {case}: asked a departed player"
                        );
                        assert_eq!(w.host.game.turn().priority_player(), Some(r.chooser));
                    }
                    Err(Interrupt::Pending(r))
                }
                other => other,
            };
            let session = ChoiceSession::replay(world(host.clone()), probe, &answers)
                .unwrap_or_else(|e| panic!("case {case}: {e:?}"));
            let Some(r) = session.pending().cloned() else {
                break;
            };
            answers.push(answer(&mut rng, &r));
        }
    }
}
