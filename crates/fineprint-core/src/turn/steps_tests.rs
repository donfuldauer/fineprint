// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the turn sequencer: REQ-TURN-002 (SRS 5.2; CR 500.1-500.6, CR 502.1-502.4,
// CR 504.1-504.2, CR 103.8a-c, CR 505.1a, CR 505.4, CR 715.3c, CR 514.1-514.3a, CR 703.4), with
// REQ-CORE-001 for deterministic replay. A recording fake host and synthetic triggers only
// (`test.*` ids); no card data (R5).

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceResponse, ChoiceSession,
    ChoiceTarget, ChoiceType, Interrupt, PendingChoiceRequest, Status, ValidatorContract,
    ValidatorRule,
};
use crate::events::{Phase, Step};
use crate::format::FirstTurnDrawRule;
use crate::replacement::BusInterrupt;
use crate::sba::SbaOutcome;
use crate::state::GameState;
use crate::triggers::{
    PendingTrigger, PlacementEnv, StackedTrigger, TriggerOrigin, TriggerPlacement, place_triggers,
};
use crate::turn::heartbeat::HeartbeatHost;
use crate::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// Recording fake host
// ---------------------------------------------------------------------------------------------

/// A host call, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Turn(PlayerId),
    Phase(Phase),
    Step(Step),
    Expire(ExpiryPoint),
    Mana,
    Phasing(PlayerId),
    DayNight(Option<PlayerId>),
    Untap(PlayerId, Vec<ObjectId>),
    Draw(PlayerId),
    Discard(PlayerId, Vec<ObjectId>),
    Eot,
    Lore(Vec<ObjectId>),
    Combat(Step),
    Sba,
    Place,
    Resolve,
    FollowUp(FollowUpId),
}

#[derive(Clone, Debug)]
struct Fake {
    game: GameState,
    phase: Option<Phase>,
    step: Option<Step>,
    log: Vec<Ev>,
    stack: Vec<String>,
    waiting: Vec<PendingTrigger>,
    /// An SBA is waiting to be performed by the next pass.
    sba_pending: bool,
    untap_opts: UntapOptions,
    rule: FirstTurnDrawRule,
    starting: Option<PlayerId>,
    hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    max_hand: Option<u64>,
    sagas: Vec<ObjectId>,
    first_strike: bool,
    /// The first `n` `end_of_turn_cleanup` calls make a trigger wait.
    cleanup_triggers: u32,
    /// The first `n` `end_of_turn_cleanup` calls make an SBA wait.
    cleanup_sbas: u32,
    /// Every `end_of_turn_cleanup` makes an SBA wait (the cleanup never ends).
    always_work: bool,
    eot_calls: u32,
    /// Resolving a trigger puts a new card into the active player's hand.
    resolve_draws: bool,
    next_card: u64,
    /// Every player has left the game (the heartbeat asks nobody).
    everyone_left: bool,
    /// What `attackers_declared` reports (CR 508.8).
    attackers: bool,
    /// The combat steps whose `combat_step_actions` put one object on the stack, so that the
    /// step's priority resolves something.
    stack_in: Vec<Step>,
    /// Resolving an object during the declare attackers step puts a creature onto the
    /// battlefield attacking (`attackers` becomes true).
    resolve_adds_attacker: bool,
    /// Resolving an object during the declare blockers step flips `first_strike` (a creature
    /// gains or loses first strike during declare-blockers priority).
    resolve_flips_first_strike: bool,
    /// Every `first_strike_damage_step` call, with the step current at the call.
    first_strike_reads: std::cell::RefCell<Vec<Option<Step>>>,
    /// The `end_of_turn_cleanup` call makes every player leave the game and puts an object on
    /// the stack that nobody resolves.
    leave_in_cleanup: bool,
}

impl Fake {
    /// `n` players, turn 0, no active player, empty hands, maximum hand size 7.
    fn new(n: usize) -> Fake {
        let game = GameState::new(11, n, 20).unwrap();
        Fake {
            game,
            phase: None,
            step: None,
            log: Vec::new(),
            stack: Vec::new(),
            waiting: Vec::new(),
            sba_pending: false,
            untap_opts: UntapOptions::default(),
            rule: FirstTurnDrawRule::CrDefault,
            starting: None,
            hands: BTreeMap::new(),
            max_hand: Some(7),
            sagas: Vec::new(),
            first_strike: false,
            cleanup_triggers: 0,
            cleanup_sbas: 0,
            always_work: false,
            eot_calls: 0,
            resolve_draws: false,
            next_card: 1000,
            everyone_left: false,
            attackers: true,
            stack_in: Vec::new(),
            resolve_adds_attacker: false,
            resolve_flips_first_strike: false,
            first_strike_reads: std::cell::RefCell::new(Vec::new()),
            leave_in_cleanup: false,
        }
    }

    fn p(&self, i: usize) -> PlayerId {
        self.game.turn_order()[i]
    }

    /// Sets the turn number and the active player (as `begin_turn` would have).
    fn at_turn(mut self, number: u64, active: usize) -> Fake {
        self.game.set_turn_number(number);
        let a = self.p(active);
        self.game.set_active_player(Some(a)).unwrap();
        self
    }

    fn hand_of(&self, p: PlayerId) -> Vec<ObjectId> {
        self.hands.get(&p).cloned().unwrap_or_default()
    }
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn objs(range: std::ops::Range<u64>) -> Vec<ObjectId> {
    range.map(ObjectId::new).collect()
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

/// Synthetic placement environment: no trigger here has a target slot.
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

impl HeartbeatHost for Fake {
    fn game(&self) -> &GameState {
        &self.game
    }

    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }

    fn phase_and_step(&self) -> (Option<Phase>, Option<Step>) {
        (self.phase, self.step)
    }

    fn has_left_game(&self, player: PlayerId) -> bool {
        self.everyone_left || self.game.player(player).is_none()
    }

    fn sba_pass(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.log.push(Ev::Sba);
        let performed = std::mem::take(&mut self.sba_pending);
        Ok(sba_outcome(performed))
    }

    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.log.push(Ev::Place);
        let waiting = std::mem::take(&mut self.waiting);
        let placement = place_triggers(&NoTargets, choices, apnap, waiting)?;
        for s in &placement.stacked {
            self.stack.push(s.trigger.ability_id.clone());
        }
        Ok(placement)
    }

    fn stack_is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    fn resolve_top(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.log.push(Ev::Resolve);
        self.stack.pop();
        if self.resolve_adds_attacker && self.step == Some(Step::DeclareAttackers) {
            self.attackers = true;
        }
        if self.resolve_flips_first_strike && self.step == Some(Step::DeclareBlockers) {
            self.first_strike = !self.first_strike;
        }
        if self.resolve_draws
            && let Some(a) = self.game.turn().active_player()
        {
            self.next_card += 1;
            let card = o(self.next_card);
            self.hands.entry(a).or_default().push(card);
        }
        Ok(())
    }

    fn priority_actions(&self, _player: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }

    fn take_action(
        &mut self,
        _player: PlayerId,
        _action: &ChoiceResponse,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

impl TurnHost for Fake {
    fn begin_turn(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.log.push(Ev::Turn(active));
        let n = self.game.turn().turn_number() + 1;
        self.game.set_turn_number(n);
        self.game
            .set_active_player(Some(active))
            .map_err(|e| Interrupt::Error(ChoiceError::Procedure(e.to_string())))
    }

    fn begin_phase(&mut self, phase: Phase) -> Result<(), Interrupt> {
        self.log.push(Ev::Phase(phase));
        self.phase = Some(phase);
        self.step = None;
        Ok(())
    }

    fn begin_step(&mut self, step: Step) -> Result<(), Interrupt> {
        self.log.push(Ev::Step(step));
        self.step = Some(step);
        Ok(())
    }

    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt> {
        self.log.push(Ev::Expire(at));
        Ok(())
    }

    fn empty_mana_pools(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Mana);
        Ok(())
    }

    fn apply_phasing(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.log.push(Ev::Phasing(active));
        Ok(())
    }

    fn check_day_night(&mut self, previous_active: Option<PlayerId>) -> Result<(), Interrupt> {
        self.log.push(Ev::DayNight(previous_active));
        Ok(())
    }

    fn untap_options(&self, _active: PlayerId) -> UntapOptions {
        self.untap_opts.clone()
    }

    fn untap(
        &mut self,
        active: PlayerId,
        permanents: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Untap(active, permanents.to_vec()));
        Ok(())
    }

    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule {
        self.rule
    }

    fn starting_player(&self) -> Option<PlayerId> {
        self.starting
    }

    fn draw_card(
        &mut self,
        player: PlayerId,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Draw(player));
        Ok(())
    }

    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        self.hand_of(player)
    }

    fn max_hand_size(&self, _player: PlayerId) -> Option<u64> {
        self.max_hand
    }

    fn discard_cards(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Discard(player, cards.to_vec()));
        self.hands
            .entry(player)
            .or_default()
            .retain(|c| !cards.contains(c));
        Ok(())
    }

    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Eot);
        if self.leave_in_cleanup {
            self.everyone_left = true;
            self.stack.push("test.unresolved".to_owned());
        }
        if self.eot_calls < self.cleanup_triggers
            && let Some(a) = self.game.turn().active_player()
        {
            self.waiting.push(trig(a, "test.cleanup_trigger"));
        }
        if self.always_work || self.eot_calls < self.cleanup_sbas {
            self.sba_pending = true;
        }
        self.eot_calls += 1;
        Ok(())
    }

    fn sagas_with_chapter_abilities(&self, _player: PlayerId) -> Vec<ObjectId> {
        self.sagas.clone()
    }

    fn add_lore_counters(
        &mut self,
        sagas: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Lore(sagas.to_vec()));
        Ok(())
    }

    fn first_strike_damage_step(&self) -> bool {
        self.first_strike_reads.borrow_mut().push(self.step);
        self.first_strike
    }

    fn attackers_declared(&self) -> bool {
        self.attackers
    }

    fn perform_follow_up(
        &mut self,
        follow_up: FollowUpId,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::FollowUp(follow_up));
        Ok(())
    }

    fn combat_step_actions(
        &mut self,
        step: Step,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Combat(step));
        if self.stack_in.contains(&step) {
            self.stack.push(format!("test.combat_object_{step:?}"));
        }
        Ok(())
    }
}

/// A host that keeps the trait's default combat hooks.
#[derive(Clone, Debug)]
struct Plain(Fake);

impl HeartbeatHost for Plain {
    fn game(&self) -> &GameState {
        self.0.game()
    }
    fn game_mut(&mut self) -> &mut GameState {
        self.0.game_mut()
    }
    fn sba_pass(&mut self, c: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.0.sba_pass(c)
    }
    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        c: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.0.place_triggers(apnap, c)
    }
    fn stack_is_empty(&self) -> bool {
        self.0.stack_is_empty()
    }
    fn resolve_top(&mut self, c: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.0.resolve_top(c)
    }
    fn priority_actions(&self, p: PlayerId) -> Vec<ChoiceResponse> {
        self.0.priority_actions(p)
    }
    fn take_action(
        &mut self,
        p: PlayerId,
        a: &ChoiceResponse,
        c: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.0.take_action(p, a, c)
    }
}

impl TurnHost for Plain {
    fn begin_turn(&mut self, a: PlayerId) -> Result<(), Interrupt> {
        self.0.begin_turn(a)
    }
    fn begin_phase(&mut self, p: Phase) -> Result<(), Interrupt> {
        self.0.begin_phase(p)
    }
    fn begin_step(&mut self, s: Step) -> Result<(), Interrupt> {
        self.0.begin_step(s)
    }
    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt> {
        self.0.expire_effects(at)
    }
    fn empty_mana_pools(&mut self) -> Result<(), Interrupt> {
        self.0.empty_mana_pools()
    }
    fn apply_phasing(&mut self, a: PlayerId) -> Result<(), Interrupt> {
        self.0.apply_phasing(a)
    }
    fn check_day_night(&mut self, p: Option<PlayerId>) -> Result<(), Interrupt> {
        self.0.check_day_night(p)
    }
    fn untap_options(&self, a: PlayerId) -> UntapOptions {
        self.0.untap_options(a)
    }
    fn untap(
        &mut self,
        a: PlayerId,
        ps: &[ObjectId],
        c: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.0.untap(a, ps, c)
    }
    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule {
        self.0.first_turn_draw_rule()
    }
    fn starting_player(&self) -> Option<PlayerId> {
        self.0.starting_player()
    }
    fn draw_card(&mut self, p: PlayerId, c: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.0.draw_card(p, c)
    }
    fn hand(&self, p: PlayerId) -> Vec<ObjectId> {
        self.0.hand(p)
    }
    fn max_hand_size(&self, p: PlayerId) -> Option<u64> {
        self.0.max_hand_size(p)
    }
    fn discard_cards(
        &mut self,
        p: PlayerId,
        cs: &[ObjectId],
        c: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.0.discard_cards(p, cs, c)
    }
    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt> {
        self.0.end_of_turn_cleanup()
    }
    fn sagas_with_chapter_abilities(&self, p: PlayerId) -> Vec<ObjectId> {
        self.0.sagas_with_chapter_abilities(p)
    }
    fn add_lore_counters(
        &mut self,
        s: &[ObjectId],
        c: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.0.add_lore_counters(s, c)
    }
    fn perform_follow_up(
        &mut self,
        f: FollowUpId,
        c: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.0.perform_follow_up(f, c)
    }
}

// ---------------------------------------------------------------------------------------------
// Session driver
// ---------------------------------------------------------------------------------------------

/// The session state: the host and the sequencer state.
#[derive(Clone, Debug)]
struct World<H> {
    host: H,
    ts: TurnState,
}

impl<H: Clone + std::fmt::Debug + HeartbeatHost> Transactional for World<H> {
    fn state_hash(&self) -> Digest {
        sha256(format!("{:?}|{:?}", self.host, self.ts).as_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.host.game().turn().priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

fn world<H>(host: H) -> World<H> {
    World {
        host,
        ts: TurnState::new(),
    }
}

/// What a driven session ended with.
#[derive(Debug)]
struct Drive<H, O> {
    /// Every request asked, in order (priority requests included).
    asked: Vec<PendingChoiceRequest>,
    /// Every accepted answer, in order.
    answers: Vec<ChoiceResponse>,
    done: Option<(World<H>, O)>,
    pending: Option<PendingChoiceRequest>,
    error: Option<ChoiceError>,
    hash: Option<Digest>,
}

/// Drives `f` on `w`: every `PRIORITY_ACTION` request is answered with a pass; every other
/// request takes the next of `scripted` (`None` submits the request's default). Stops pending
/// when `scripted` runs out.
fn drive<H, O, F>(w: World<H>, scripted: &[Option<ChoiceResponse>], f: F) -> Drive<H, O>
where
    H: Clone + std::fmt::Debug + HeartbeatHost,
    O: Clone,
    F: Fn(&mut World<H>, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let mut asked = Vec::new();
    let failed = |asked, answers, e| Drive {
        asked,
        answers,
        done: None,
        pending: None,
        error: Some(e),
        hash: None,
    };
    let mut session = match ChoiceSession::start(w, f) {
        Ok(s) => s,
        Err(e) => return failed(asked, Vec::new(), e),
    };
    let mut scripted = scripted.iter();
    loop {
        let pending = match session.status() {
            Status::Done { state, output } => {
                return Drive {
                    asked,
                    answers: session.answers().to_vec(),
                    done: Some((state.clone(), output.clone())),
                    pending: None,
                    error: None,
                    hash: Some(session.state_hash()),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        asked.push(pending.clone());
        let answer = if pending.choice_type == ChoiceType::PriorityAction {
            ChoiceResponse::PassPriority
        } else {
            match scripted.next() {
                Some(Some(a)) => a.clone(),
                Some(None) => pending.default_on_timeout.clone(),
                None => {
                    return Drive {
                        asked,
                        answers: session.answers().to_vec(),
                        done: None,
                        pending: Some(pending),
                        error: None,
                        hash: Some(session.state_hash()),
                    };
                }
            }
        };
        if let Err(e) = session.submit(pending.request_id, answer) {
            let answers = session.answers().to_vec();
            return failed(asked, answers, e);
        }
    }
}

type Proc<O> = Box<dyn Fn(&mut World<Fake>, &mut ChoiceContext<'_>) -> Result<O, Interrupt>>;

fn turn_of(active: PlayerId) -> Proc<TurnReport> {
    Box::new(move |w, c| {
        run_turn(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            active,
            c,
        )
    })
}

fn segment_of(segment: Segment, registry: TurnBasedRegistry) -> Proc<SegmentReport> {
    Box::new(move |w, c| run_segment(&mut w.host, &mut w.ts, &registry, segment, c))
}

fn phase_of(phase: Phase) -> Proc<Vec<SegmentReport>> {
    Box::new(move |w, c| {
        run_phase(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            phase,
            c,
        )
    })
}

fn procedure_message(e: Option<&ChoiceError>) -> String {
    match e {
        Some(ChoiceError::Procedure(m)) => m.clone(),
        other => panic!("expected ChoiceError::Procedure, got {other:?}"),
    }
}

/// The host calls of a heartbeat with nothing waiting: one settle (one SBA pass, one empty
/// placement), then the passes.
const HB: [Ev; 2] = [Ev::Sba, Ev::Place];

/// The expected calls of one step: begin, begin-expiry, `body`, a heartbeat, end-expiry, mana.
fn plain_step(step: Step, body: &[Ev]) -> Vec<Ev> {
    let mut v = vec![Ev::Step(step), Ev::Expire(ExpiryPoint::BeginStep(step))];
    v.extend_from_slice(body);
    v.extend(HB);
    v.extend([Ev::Expire(ExpiryPoint::EndStep(step)), Ev::Mana]);
    v
}

/// The expected calls of a whole turn with nothing waiting, nothing choosable and no sagas:
/// `forced` untaps, `draws` says whether the draw step happens (CR 103.8a, CR 500.11: a skipped
/// first draw step leaves no trace at all).
fn expected_turn(
    active: PlayerId,
    previous: Option<PlayerId>,
    forced: &[ObjectId],
    draws: bool,
    first_strike: bool,
) -> Vec<Ev> {
    fn phase_start(v: &mut Vec<Ev>, p: Phase) {
        v.extend([Ev::Phase(p), Ev::Expire(ExpiryPoint::BeginPhase(p))]);
    }
    fn phase_end(v: &mut Vec<Ev>, p: Phase) {
        v.extend([Ev::Expire(ExpiryPoint::EndPhase(p)), Ev::Mana]);
    }
    let mut v = vec![Ev::Turn(active)];
    // Beginning phase.
    phase_start(&mut v, Phase::Beginning);
    v.extend([
        Ev::Step(Step::Untap),
        Ev::Expire(ExpiryPoint::BeginStep(Step::Untap)),
        Ev::Phasing(active),
        Ev::DayNight(previous),
    ]);
    // CR 502.3: with nothing choosable the forced list is untapped as given; no call when empty.
    if !forced.is_empty() {
        v.push(Ev::Untap(active, forced.to_vec()));
    }
    v.extend([Ev::Expire(ExpiryPoint::EndStep(Step::Untap)), Ev::Mana]);
    v.extend(plain_step(Step::Upkeep, &[]));
    if draws {
        v.extend(plain_step(Step::Draw, &[Ev::Draw(active)]));
    }
    phase_end(&mut v, Phase::Beginning);
    // Precombat main.
    phase_start(&mut v, Phase::PrecombatMain);
    v.extend(HB);
    phase_end(&mut v, Phase::PrecombatMain);
    // Combat.
    phase_start(&mut v, Phase::Combat);
    let mut combat = vec![
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
    ];
    if first_strike {
        combat.push(Step::FirstStrikeDamage);
    }
    combat.extend([Step::CombatDamage, Step::EndOfCombat]);
    for s in combat {
        v.extend(plain_step(s, &[Ev::Combat(s)]));
    }
    phase_end(&mut v, Phase::Combat);
    // Postcombat main.
    phase_start(&mut v, Phase::PostcombatMain);
    v.extend(HB);
    phase_end(&mut v, Phase::PostcombatMain);
    // Ending.
    phase_start(&mut v, Phase::Ending);
    v.extend(plain_step(Step::End, &[]));
    v.extend([
        Ev::Step(Step::Cleanup),
        Ev::Expire(ExpiryPoint::BeginStep(Step::Cleanup)),
        Ev::Eot,
        Ev::Sba,
        Ev::Place,
        Ev::Expire(ExpiryPoint::EndStep(Step::Cleanup)),
        Ev::Mana,
    ]);
    phase_end(&mut v, Phase::Ending);
    v
}

/// The segments a log went through, in order (steps and main phases).
fn segments_seen(log: &[Ev]) -> Vec<Segment> {
    log.iter()
        .filter_map(|e| match e {
            Ev::Step(s) => Some(Segment::Step(*s)),
            Ev::Phase(Phase::PrecombatMain) => Some(Segment::PrecombatMain),
            Ev::Phase(Phase::PostcombatMain) => Some(Segment::PostcombatMain),
            _ => None,
        })
        .collect()
}

const ALL_PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];

/// The full segment order of a turn.
fn turn_segments(first_strike: bool) -> Vec<Segment> {
    ALL_PHASES
        .into_iter()
        .flat_map(|p| segments_of(p, first_strike))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Segments
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_002_segments_of_every_phase_in_order() {
    // REQ-TURN-002, CR 500.1: the phases' steps proceed in order; CR 505.2: a main phase has no
    // steps.
    use Segment::Step as S;
    assert_eq!(
        segments_of(Phase::Beginning, false),
        vec![S(Step::Untap), S(Step::Upkeep), S(Step::Draw)]
    );
    assert_eq!(
        segments_of(Phase::PrecombatMain, true),
        vec![Segment::PrecombatMain]
    );
    assert_eq!(
        segments_of(Phase::PostcombatMain, false),
        vec![Segment::PostcombatMain]
    );
    assert_eq!(
        segments_of(Phase::Combat, false),
        vec![
            S(Step::BeginningOfCombat),
            S(Step::DeclareAttackers),
            S(Step::DeclareBlockers),
            S(Step::CombatDamage),
            S(Step::EndOfCombat)
        ]
    );
    assert_eq!(
        segments_of(Phase::Combat, true),
        vec![
            S(Step::BeginningOfCombat),
            S(Step::DeclareAttackers),
            S(Step::DeclareBlockers),
            S(Step::FirstStrikeDamage),
            S(Step::CombatDamage),
            S(Step::EndOfCombat)
        ]
    );
    assert_eq!(
        segments_of(Phase::Ending, true),
        vec![S(Step::End), S(Step::Cleanup)]
    );
    for p in ALL_PHASES {
        for s in segments_of(p, true) {
            assert_eq!(s.phase(), p);
        }
    }
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

#[test]
fn req_turn_002_segment_and_expiry_point_hash_agrees_with_eq() {
    // REQ-TURN-002, CR 500.1: equal values hash equally; distinct variants and steps or phases
    // are told apart.
    let segs = turn_segments(true);
    for a in &segs {
        for b in &segs {
            assert_eq!(a == b, hash_of(a) == hash_of(b), "{a:?} vs {b:?}");
        }
    }
    let points = [
        ExpiryPoint::BeginStep(Step::Upkeep),
        ExpiryPoint::EndStep(Step::Upkeep),
        ExpiryPoint::BeginStep(Step::Draw),
        ExpiryPoint::BeginPhase(Phase::Combat),
        ExpiryPoint::EndPhase(Phase::Combat),
        ExpiryPoint::EndPhase(Phase::Ending),
    ];
    for a in &points {
        for b in &points {
            assert_eq!(a == b, hash_of(a) == hash_of(b), "{a:?} vs {b:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// A whole turn
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_002_scripted_two_player_turn_passes_every_step_in_order() {
    // REQ-TURN-002, CR 500.1, CR 500.4, CR 500.5, CR 502.1-502.3, CR 504.1-504.2: every phase
    // and step in order, the begin-expiry at each start, the end-expiry and then the mana
    // emptying at each end.
    let mut h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    h.starting = Some(b);
    h.untap_opts.forced = vec![o(1), o(3)];
    let d = drive(world(h), &[], turn_of(a));
    let (w, report) = d.done.expect("the turn should finish");
    assert_eq!(
        w.host.log,
        expected_turn(a, None, &[o(1), o(3)], true, false)
    );
    assert_eq!(segments_seen(&w.host.log), turn_segments(false));
    // Two passes per heartbeat: upkeep, draw, two mains, five combat steps, end.
    assert_eq!(d.asked.len(), 2 * 10);
    assert!(
        d.asked
            .iter()
            .all(|r| r.choice_type == ChoiceType::PriorityAction)
    );
    assert_eq!(d.asked[0].chooser, a);
    assert_eq!(report.active_player, a);
    let segs: Vec<Segment> = report.segments.iter().map(|s| s.segment).collect();
    assert_eq!(segs, turn_segments(false));
    for s in &report.segments {
        let untap = s.segment == Segment::Step(Step::Untap);
        let cleanup = s.segment == Segment::Step(Step::Cleanup);
        assert_eq!(s.heartbeat_ran, !untap && !cleanup, "{s:?}");
        assert_eq!(s.cleanup_steps, u32::from(cleanup), "{s:?}");
    }
    assert_eq!(w.host.game.turn().turn_number(), 1);
    assert_eq!(w.host.game.turn().priority_player(), None);
    assert_eq!(w.ts.previous_active_player(), Some(a));
    assert_eq!(w.ts.heartbeat().passes().len(), 20);
}

#[test]
fn req_turn_002_turn_with_first_strike_damage_step() {
    // REQ-TURN-002, CR 500.1: with a first-strike combat damage step it comes right before the
    // combat damage step.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.first_strike = true;
    let d = drive(world(h), &[], turn_of(a));
    let (w, report) = d.done.unwrap();
    assert_eq!(w.host.log, expected_turn(a, None, &[], true, true));
    assert_eq!(segments_seen(&w.host.log), turn_segments(true));
    assert_eq!(report.segments.len(), turn_segments(true).len());
    assert_eq!(
        report.segments[7].segment,
        Segment::Step(Step::FirstStrikeDamage)
    );
    assert_eq!(
        report.segments[8].segment,
        Segment::Step(Step::CombatDamage)
    );
}

#[test]
fn req_turn_002_default_combat_hooks_do_nothing() {
    // REQ-TURN-002: the default `first_strike_damage_step` is false and the default
    // `combat_step_actions` does nothing (combat actions belong to other units).
    let h = Plain(Fake::new(2).at_turn(4, 0));
    let d = drive(world(h), &[], |w: &mut World<Plain>, c| {
        run_phase(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            Phase::Combat,
            c,
        )
    });
    let (w, reports) = d.done.unwrap();
    assert_eq!(reports.len(), 5);
    assert!(!w.host.0.log.iter().any(|e| matches!(e, Ev::Combat(_))));
}

#[test]
fn req_turn_002_turns_alternate_and_pass_previous_active_to_day_night() {
    // REQ-TURN-002, CR 502.2, CR 732.2: the day/night check sees the previous turn's active
    // player (none on the first turn).
    let h = Fake::new(2);
    let (a, b) = (h.p(0), h.p(1));
    let mut w = world(h);
    let mut seen = Vec::new();
    for (i, p) in [a, b, a].into_iter().enumerate() {
        w.host.log.clear();
        let d = drive(w, &[], turn_of(p));
        let (next, report) = d.done.unwrap();
        assert_eq!(report.active_player, p);
        assert_eq!(next.host.game.turn().turn_number(), i as u64 + 1);
        assert_eq!(next.ts.previous_active_player(), Some(p));
        seen.extend(next.host.log.iter().filter_map(|e| match e {
            Ev::DayNight(x) => Some(*x),
            _ => None,
        }));
        w = next;
    }
    assert_eq!(seen, vec![None, Some(a), Some(b)]);
}

#[test]
fn req_turn_002_run_turn_is_deterministic() {
    // REQ-CORE-001, REQ-TURN-002: the same base and the same answers give the same requests and
    // the same final state.
    let mk = || {
        let mut h = Fake::new(3);
        h.untap_opts.choosable = vec![o(2), o(4)];
        let a = h.p(0);
        h.hands.insert(a, objs(1..10));
        h.cleanup_triggers = 1;
        h
    };
    let a = mk().p(0);
    let script = [
        Some(ChoiceResponse::SubmitSet {
            objects: vec![o(4)],
        }),
        None,
    ];
    let d1 = drive(world(mk()), &script, turn_of(a));
    let d2 = drive(world(mk()), &script, turn_of(a));
    assert!(d1.done.is_some(), "{:?} {:?}", d1.error, d1.pending);
    assert_eq!(d1.hash, d2.hash);
    assert_eq!(d1.asked, d2.asked);
}

// ---------------------------------------------------------------------------------------------
// Untap step
// ---------------------------------------------------------------------------------------------

fn untap_only(h: Fake, script: &[Option<ChoiceResponse>]) -> Drive<Fake, SegmentReport> {
    drive(
        world(h),
        script,
        segment_of(Segment::Step(Step::Untap), TurnBasedRegistry::standard()),
    )
}

#[test]
fn req_turn_002_untap_order_and_no_priority_without_choice() {
    // REQ-TURN-002, CR 502.1-502.4: phasing, then day/night, then untap; no settle, no trigger
    // placement, no priority; with nothing choosable there is no request.
    let mut h = Fake::new(2).at_turn(5, 0);
    let a = h.p(0);
    // Passed through as the host gave it (docs/DECISIONS.md "#46 FP-045 — Untap call without a
    // choice").
    h.untap_opts.forced = vec![o(7), o(2)];
    h.waiting.push(trig(a, "test.waiting"));
    let d = untap_only(h, &[]);
    let (w, report) = d.done.unwrap();
    assert!(d.asked.is_empty());
    assert_eq!(
        w.host.log,
        vec![
            Ev::Step(Step::Untap),
            Ev::Expire(ExpiryPoint::BeginStep(Step::Untap)),
            Ev::Phasing(a),
            Ev::DayNight(None),
            Ev::Untap(a, vec![o(7), o(2)]),
            Ev::Expire(ExpiryPoint::EndStep(Step::Untap)),
            Ev::Mana,
        ]
    );
    // CR 502.4: the trigger waits for the next time a player would receive priority.
    assert_eq!(w.host.waiting.len(), 1);
    assert_eq!(w.host.game.turn().priority_player(), None);
    assert_eq!(
        report,
        SegmentReport {
            segment: Segment::Step(Step::Untap),
            heartbeat_ran: false,
            cleanup_steps: 0
        }
    );
}

#[test]
fn req_turn_002_untap_choice_one_request_and_one_simultaneous_untap() {
    // REQ-TURN-002, CR 502.3: one CHOOSE_UNTAP_SET request over the choosable permanents; the
    // forced and the chosen permanents untap in ONE call, ascending.
    let mut h = Fake::new(2).at_turn(5, 1);
    let a = h.p(1);
    h.untap_opts.forced = vec![o(1), o(5)];
    h.untap_opts.choosable = vec![o(2), o(4)];
    let d = untap_only(
        h,
        &[Some(ChoiceResponse::SubmitSet {
            objects: vec![o(4)],
        })],
    );
    let (w, _) = d.done.unwrap();
    assert_eq!(d.asked.len(), 1);
    let r = &d.asked[0];
    assert_eq!(r.choice_type, ChoiceType::ChooseUntapSet);
    assert_eq!(r.chooser, a);
    assert_eq!(r.reason.cr, "CR 502.3");
    assert_eq!(r.reason.source, None);
    assert_eq!((r.min, r.max), (0, 2));
    assert!(!r.hidden);
    assert!(r.reveal_to.is_empty());
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SubmitSet {
            objects: vec![o(2), o(4)]
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 0,
            max: 2,
            rule: ValidatorRule::Subset {
                candidates: vec![o(2), o(4)]
            }
        })
    );
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseUntapSet {
            permanents: vec![o(2), o(4)]
        }
    );
    let untaps: Vec<&Ev> = w
        .host
        .log
        .iter()
        .filter(|e| matches!(e, Ev::Untap(..)))
        .collect();
    assert_eq!(untaps, vec![&Ev::Untap(a, vec![o(1), o(4), o(5)])]);
    assert!(!w.host.log.contains(&Ev::Sba));
    assert!(!w.host.log.contains(&Ev::Place));
}

#[test]
fn req_turn_002_untap_default_untaps_everything() {
    // REQ-TURN-002, CR 502.3: "Normally, all of a player's permanents untap": the default
    // answer untaps every choosable permanent.
    let mut h = Fake::new(2).at_turn(5, 0);
    let a = h.p(0);
    h.untap_opts.forced = vec![o(3)];
    h.untap_opts.choosable = vec![o(1), o(9)];
    let d = untap_only(h, &[None]);
    let (w, _) = d.done.unwrap();
    assert!(w.host.log.contains(&Ev::Untap(a, vec![o(1), o(3), o(9)])));
}

#[test]
fn req_turn_002_untap_nothing_chosen_nothing_forced_no_untap_call() {
    // REQ-TURN-002, CR 502.3: an empty set chosen with nothing forced untaps nothing: no call.
    let mut h = Fake::new(2).at_turn(5, 0);
    h.untap_opts.choosable = vec![o(6)];
    let d = untap_only(h, &[Some(ChoiceResponse::SubmitSet { objects: vec![] })]);
    let (w, _) = d.done.unwrap();
    assert_eq!(d.asked.len(), 1);
    assert!(!w.host.log.iter().any(|e| matches!(e, Ev::Untap(..))));
    assert!(w.host.log.contains(&Ev::Phasing(w.host.p(0))));
}

#[test]
fn req_turn_002_untap_wrong_answers_are_rejected() {
    // REQ-TURN-002, R12, CR 502.3: an object not offered, a duplicate or another answer type is
    // rejected without panicking.
    for bad in [
        ChoiceResponse::SubmitSet {
            objects: vec![o(8)],
        },
        ChoiceResponse::SubmitSet {
            objects: vec![o(2), o(2)],
        },
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(2)],
        },
        ChoiceResponse::PassPriority,
    ] {
        let mut h = Fake::new(2).at_turn(5, 0);
        h.untap_opts.choosable = vec![o(2), o(4)];
        let d = untap_only(h, &[Some(bad.clone())]);
        assert!(
            matches!(d.error, Some(ChoiceError::Invalid { .. })),
            "{bad:?}: {:?}",
            d.error
        );
        assert!(d.done.is_none());
    }
}

#[test]
fn req_turn_002_untap_pending_surfaces_and_replay_reproduces_state() {
    // REQ-TURN-002, REQ-CORE-001, CR 502.3: the CHOOSE_UNTAP_SET request suspends the turn
    // unchanged; a re-run from the base with the recorded answers gives the same final state.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.untap_opts.choosable = vec![o(2), o(4)];
    let base = world(h);
    let pending = drive(base.clone(), &[], turn_of(a));
    let req = pending.pending.expect("the untap request is pending");
    assert_eq!(req.choice_type, ChoiceType::ChooseUntapSet);
    assert_eq!(req.request_id.get(), 0);
    let d = drive(
        base.clone(),
        &[Some(ChoiceResponse::SubmitSet {
            objects: vec![o(2)],
        })],
        turn_of(a),
    );
    let (w, _) = d.done.as_ref().unwrap();
    let replayed = ChoiceSession::replay(base, turn_of(a), &d.answers).unwrap();
    assert_eq!(Some(replayed.state_hash()), d.hash);
    match replayed.status() {
        Status::Done { state, .. } => assert_eq!(state.host.log, w.host.log),
        Status::Pending(r) => panic!("replay should finish, pending {r:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Draw step
// ---------------------------------------------------------------------------------------------

fn skip(
    n: usize,
    turn: u64,
    rule: FirstTurnDrawRule,
    starting: Option<usize>,
    active: usize,
) -> bool {
    let mut h = Fake::new(n).at_turn(turn, active);
    h.rule = rule;
    h.starting = starting.map(|i| h.p(i));
    let a = h.p(active);
    first_turn_draw_skipped(&h, a)
}

#[test]
fn req_turn_002_first_turn_draw_skip_matrix() {
    // REQ-TURN-002, CR 103.8a: in a two-player game the starting player skips the first draw;
    // CR 103.8c: in other multiplayer games nobody does; FormatConfig house rules override.
    use FirstTurnDrawRule::{AlwaysSkip, CrDefault, NeverSkip};
    assert!(skip(2, 1, CrDefault, Some(0), 0));
    assert!(!skip(2, 2, CrDefault, Some(0), 0));
    assert!(!skip(2, 1, CrDefault, Some(0), 1));
    assert!(!skip(3, 1, CrDefault, Some(0), 0));
    assert!(skip(3, 1, AlwaysSkip, Some(0), 0));
    assert!(!skip(3, 2, AlwaysSkip, Some(0), 0));
    assert!(!skip(2, 1, NeverSkip, Some(0), 0));
    assert!(!skip(2, 1, CrDefault, None, 0));
    assert!(!skip(2, 1, AlwaysSkip, None, 0));
}

#[test]
fn req_turn_002_draw_step_skipped_on_first_turn_never_begins() {
    // REQ-TURN-002, CR 103.8a: "In a two-player game, the player who plays first skips the
    // draw step ... of their first turn"; CR 500.11: the step is passed "as though it didn't
    // exist": no begin-step event or expiry, no draw, no priority, no end-step expiry or mana
    // emptying (F48). The beginning phase's own boundary work stays.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.starting = Some(a);
    let d = drive(world(h), &[], turn_of(a));
    let (w, report) = d.done.unwrap();
    assert_eq!(w.host.log, expected_turn(a, None, &[], false, false));
    assert!(!w.host.log.contains(&Ev::Step(Step::Draw)));
    assert!(
        !w.host
            .log
            .contains(&Ev::Expire(ExpiryPoint::BeginStep(Step::Draw)))
    );
    assert!(
        !w.host
            .log
            .contains(&Ev::Expire(ExpiryPoint::EndStep(Step::Draw)))
    );
    assert_eq!(report.segments[2].segment, Segment::Step(Step::Draw));
    assert!(!report.segments[2].heartbeat_ran);
    assert_eq!(report.segments[2].cleanup_steps, 0);
    // Upkeep, two mains, five combat steps, end: no priority in the draw step.
    assert_eq!(d.asked.len(), 2 * 9);
    assert!(
        w.ts.heartbeat()
            .passes()
            .iter()
            .all(|r| r.step != Some(Step::Draw))
    );
}

#[test]
fn req_turn_002_first_draw_step_skip_only_where_cr_103_8_says() {
    // REQ-TURN-002, CR 103.8a: only the starting player's first turn in a two-player game
    // skips the draw step; the second turn (CR 504.1), a three-player game (CR 103.8c) and the
    // NEVER_SKIP house rule keep it, with its draw and priority (F48).
    let cases: [(usize, u64, FirstTurnDrawRule, bool); 4] = [
        (2, 0, FirstTurnDrawRule::CrDefault, false),
        (2, 1, FirstTurnDrawRule::CrDefault, true),
        (3, 0, FirstTurnDrawRule::CrDefault, true),
        (2, 0, FirstTurnDrawRule::NeverSkip, true),
    ];
    for (n, turn_before, rule, draws) in cases {
        let mut h = Fake::new(n);
        let a = h.p(0);
        h.starting = Some(a);
        h.rule = rule;
        h.game.set_turn_number(turn_before);
        let d = drive(world(h), &[], turn_of(a));
        let (w, report) = d.done.unwrap();
        let label = format!("{n} players, turn {}, {rule:?}", turn_before + 1);
        assert_eq!(w.host.log.contains(&Ev::Step(Step::Draw)), draws, "{label}");
        assert_eq!(w.host.log.contains(&Ev::Draw(a)), draws, "{label}");
        assert_eq!(report.segments[2].heartbeat_ran, draws, "{label}");
        let draw_passes =
            w.ts.heartbeat()
                .passes()
                .iter()
                .filter(|r| r.step == Some(Step::Draw))
                .count();
        assert_eq!(draw_passes, if draws { n } else { 0 }, "{label}");
    }
}

/// Runs `Step::Draw` through [`run_segment`] twice in the current turn.
fn two_draw_segments() -> Proc<(SegmentReport, SegmentReport)> {
    Box::new(|w, c| {
        let r = TurnBasedRegistry::new();
        let first = run_segment(&mut w.host, &mut w.ts, &r, Segment::Step(Step::Draw), c)?;
        let second = run_segment(&mut w.host, &mut w.ts, &r, Segment::Step(Step::Draw), c)?;
        Ok((first, second))
    })
}

#[test]
fn req_turn_002_req_turn_003_first_turn_skip_covers_the_first_draw_step_only() {
    // REQ-TURN-002, REQ-TURN-003; Gate 06-turn R9. CR 103.8a: "the player who plays first
    // skips the draw step ... of their first turn": the first draw step run in that turn is
    // skipped with no host call (CR 500.11, F48); a second (added) draw step of the same turn
    // runs normally: begin-step event and expiry, the draw (CR 504.1), priority (CR 504.2),
    // end-step expiry and mana emptying.
    let mut h = Fake::new(2).at_turn(1, 0);
    let a = h.p(0);
    h.starting = Some(a);
    assert!(first_turn_draw_skipped(&h, a));
    let d = drive(world(h), &[], two_draw_segments());
    let (w, (first, second)) = d.done.unwrap();
    let draw = Segment::Step(Step::Draw);
    assert_eq!(
        first,
        SegmentReport {
            segment: draw,
            heartbeat_ran: false,
            cleanup_steps: 0
        }
    );
    assert_eq!(
        second,
        SegmentReport {
            segment: draw,
            heartbeat_ran: true,
            cleanup_steps: 0
        }
    );
    // Only the second segment left calls: exactly one draw step's worth.
    assert_eq!(w.host.log, plain_step(Step::Draw, &[Ev::Draw(a)]));
    assert_eq!(d.asked.len(), 2);
    assert_eq!(d.asked[0].chooser, a);
    // The query is pure: it still says the turn's normal draw step is skipped.
    assert!(first_turn_draw_skipped(&w.host, a));
}

#[test]
fn req_turn_002_first_turn_skip_is_reset_for_a_new_turn() {
    // REQ-TURN-002, REQ-CORE-001; Gate 06-turn R9, CR 103.8a: the consumed skip belongs to
    // one turn. A state reused for a new turn 1 (record_turn_taken, or the start of run_turn)
    // skips that turn's first draw step again.
    let draw = Segment::Step(Step::Draw);
    let mut h = Fake::new(2).at_turn(1, 0);
    let a = h.p(0);
    h.starting = Some(a);
    let after_record: Proc<(SegmentReport, SegmentReport)> = Box::new(move |w, c| {
        let r = TurnBasedRegistry::new();
        let first = run_segment(&mut w.host, &mut w.ts, &r, draw, c)?;
        w.ts.record_turn_taken(a);
        let again = run_segment(&mut w.host, &mut w.ts, &r, draw, c)?;
        Ok((first, again))
    });
    let d = drive(world(h.clone()), &[], after_record);
    let (w, (first, again)) = d.done.unwrap();
    assert!(!first.heartbeat_ran);
    assert!(!again.heartbeat_ran);
    assert!(w.host.log.is_empty());
    assert!(d.asked.is_empty());

    let new_turn: Proc<TurnReport> = Box::new(move |w, c| {
        run_segment(&mut w.host, &mut w.ts, &TurnBasedRegistry::new(), draw, c)?;
        // The same state, a new game: run_turn makes this turn 1 again.
        w.host.game.set_turn_number(0);
        run_turn(&mut w.host, &mut w.ts, &TurnBasedRegistry::standard(), a, c)
    });
    let d = drive(world(h), &[], new_turn);
    let (w, report) = d.done.unwrap();
    assert_eq!(w.host.log, expected_turn(a, None, &[], false, false));
    assert_eq!(report.segments[2].segment, draw);
    assert!(!report.segments[2].heartbeat_ran);
}

#[test]
fn req_turn_002_draw_step_draws_then_heartbeat() {
    // REQ-TURN-002, CR 504.1, CR 504.2: the draw, then priority to the active player.
    let h = Fake::new(2).at_turn(6, 1);
    let a = h.p(1);
    let d = drive(
        world(h),
        &[],
        segment_of(Segment::Step(Step::Draw), TurnBasedRegistry::new()),
    );
    let (w, report) = d.done.unwrap();
    assert_eq!(w.host.log, plain_step(Step::Draw, &[Ev::Draw(a)]));
    assert!(report.heartbeat_ran);
    assert_eq!(d.asked.len(), 2);
    assert_eq!(d.asked[0].chooser, a);
}

// ---------------------------------------------------------------------------------------------
// Main phases and the registry
// ---------------------------------------------------------------------------------------------

type ActionFn = fn(&mut dyn TurnHost, PlayerId, &mut ChoiceContext<'_>) -> Result<(), Interrupt>;

/// A synthetic registry action: marks itself with lore counters on object 900.
fn marker(
    host: &mut dyn TurnHost,
    _active: PlayerId,
    c: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    host.add_lore_counters(&[o(900)], c)
}

/// A synthetic registry action: marks itself with lore counters on object 901.
fn marker2(
    host: &mut dyn TurnHost,
    _active: PlayerId,
    c: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    host.add_lore_counters(&[o(901)], c)
}

fn action(id: &'static str, segment: Segment, run: ActionFn) -> TurnBasedAction {
    TurnBasedAction {
        id,
        cr: "CR 703.1",
        segment,
        run,
    }
}

#[test]
fn req_turn_002_registry_new_standard_register_and_order() {
    // REQ-TURN-002, CR 505.4: the standard registry holds exactly the lore-counter action at
    // the precombat main; entries keep registration order; a duplicate id is an error.
    let empty = TurnBasedRegistry::new();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    let mut r = TurnBasedRegistry::standard();
    assert_eq!(r.len(), 1);
    assert!(!r.is_empty());
    let e = r.entries_for(Segment::PrecombatMain);
    assert_eq!(e.len(), 1);
    assert_eq!((e[0].id, e[0].cr), ("lore_counters", "CR 505.4"));
    assert_eq!(e[0].segment, Segment::PrecombatMain);
    assert!(r.entries_for(Segment::PostcombatMain).is_empty());
    r.register(action("test.a", Segment::PrecombatMain, marker))
        .unwrap();
    r.register(action("test.b", Segment::PostcombatMain, marker2))
        .unwrap();
    r.register(action("test.c", Segment::PrecombatMain, marker2))
        .unwrap();
    let ids: Vec<&str> = r
        .entries_for(Segment::PrecombatMain)
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, vec!["lore_counters", "test.a", "test.c"]);
    let err = r.register(action("test.a", Segment::Step(Step::End), marker2));
    assert!(matches!(err, Err(ChoiceError::Procedure(_))));
    assert_eq!(r.len(), 4);
}

#[test]
fn req_turn_002_lore_counters_at_precombat_main_only() {
    // REQ-TURN-002, CR 505.4, CR 715.3c, CR 505.1a: lore counters at the precombat main phase,
    // before priority; never at the postcombat main.
    let mut h = Fake::new(2).at_turn(3, 0);
    h.sagas = vec![o(5), o(6)];
    let d = drive(world(h.clone()), &[], phase_of(Phase::PrecombatMain));
    let (w, reports) = d.done.unwrap();
    assert_eq!(
        w.host.log,
        vec![
            Ev::Phase(Phase::PrecombatMain),
            Ev::Expire(ExpiryPoint::BeginPhase(Phase::PrecombatMain)),
            Ev::Lore(vec![o(5), o(6)]),
            Ev::Sba,
            Ev::Place,
            Ev::Expire(ExpiryPoint::EndPhase(Phase::PrecombatMain)),
            Ev::Mana,
        ]
    );
    assert_eq!(
        reports,
        vec![SegmentReport {
            segment: Segment::PrecombatMain,
            heartbeat_ran: true,
            cleanup_steps: 0
        }]
    );
    let d = drive(world(h), &[], phase_of(Phase::PostcombatMain));
    let (w, _) = d.done.unwrap();
    assert!(!w.host.log.iter().any(|e| matches!(e, Ev::Lore(_))));
}

#[test]
fn req_turn_002_no_lore_counters_without_sagas() {
    // REQ-TURN-002, CR 505.4: no saga, no lore-counter call.
    let h = Fake::new(2).at_turn(3, 0);
    let d = drive(
        world(h),
        &[],
        segment_of(Segment::PrecombatMain, TurnBasedRegistry::standard()),
    );
    let (w, _) = d.done.unwrap();
    assert_eq!(w.host.log, HB.to_vec());
}

#[test]
fn req_turn_002_extra_registry_entries_run_in_order() {
    // REQ-TURN-002, CR 703.1: an extra precombat-main entry runs after the lore counters; a
    // postcombat entry runs at the postcombat main; step entries run after the step's own
    // turn-based actions and before priority.
    let mut h = Fake::new(2).at_turn(3, 0);
    h.sagas = vec![o(5)];
    let mut r = TurnBasedRegistry::standard();
    r.register(action("test.main", Segment::PrecombatMain, marker))
        .unwrap();
    r.register(action("test.post", Segment::PostcombatMain, marker2))
        .unwrap();
    let d = drive(
        world(h.clone()),
        &[],
        segment_of(Segment::PrecombatMain, r.clone()),
    );
    let (w, _) = d.done.unwrap();
    assert_eq!(
        w.host.log,
        vec![
            Ev::Lore(vec![o(5)]),
            Ev::Lore(vec![o(900)]),
            Ev::Sba,
            Ev::Place
        ]
    );
    let d = drive(
        world(h.clone()),
        &[],
        segment_of(Segment::PostcombatMain, r),
    );
    let (w, _) = d.done.unwrap();
    assert_eq!(w.host.log, vec![Ev::Lore(vec![o(901)]), Ev::Sba, Ev::Place]);

    // Step entries.
    let mut r = TurnBasedRegistry::new();
    let steps = [
        Step::Untap,
        Step::Upkeep,
        Step::Draw,
        Step::DeclareAttackers,
        Step::End,
        Step::Cleanup,
    ];
    let ids = ["t0", "t1", "t2", "t3", "t4", "t5"];
    for (s, id) in steps.iter().zip(ids) {
        r.register(action(id, Segment::Step(*s), marker)).unwrap();
    }
    let a = h.p(0);
    for s in steps {
        let d = drive(
            world(h.clone()),
            &[],
            segment_of(Segment::Step(s), r.clone()),
        );
        let (w, _) = d.done.unwrap();
        let log = &w.host.log;
        let at = log
            .iter()
            .position(|e| *e == Ev::Lore(vec![o(900)]))
            .expect("the step entry runs");
        assert_eq!(log.iter().filter(|e| matches!(e, Ev::Lore(_))).count(), 1);
        let (before, after) = log.split_at(at);
        match s {
            Step::Untap => assert!(before.contains(&Ev::DayNight(None))),
            Step::Draw => assert!(before.contains(&Ev::Draw(a))),
            Step::DeclareAttackers => assert!(before.contains(&Ev::Combat(s))),
            Step::Cleanup => assert!(before.contains(&Ev::Eot)),
            _ => {}
        }
        if s != Step::Untap {
            assert!(after.contains(&Ev::Sba), "{s:?}");
            assert!(!before.contains(&Ev::Sba), "{s:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Cleanup step
// ---------------------------------------------------------------------------------------------

fn cleanup(h: Fake, script: &[Option<ChoiceResponse>]) -> Drive<Fake, SegmentReport> {
    drive(
        world(h),
        script,
        segment_of(Segment::Step(Step::Cleanup), TurnBasedRegistry::standard()),
    )
}

const CLEANUP_START: [Ev; 2] = [
    Ev::Step(Step::Cleanup),
    Ev::Expire(ExpiryPoint::BeginStep(Step::Cleanup)),
];
const CLEANUP_END: [Ev; 2] = [Ev::Expire(ExpiryPoint::EndStep(Step::Cleanup)), Ev::Mana];

#[test]
fn req_turn_002_cleanup_without_work_ends_without_priority() {
    // REQ-TURN-002, CR 514.1-514.3: hand within the maximum (or no maximum): no discard; the
    // CR 514.2 actions always happen; nothing waiting: no priority, one cleanup step.
    for (max, cards) in [(Some(7), 7), (Some(3), 0), (None, 40)] {
        let mut h = Fake::new(2).at_turn(3, 0);
        let a = h.p(0);
        h.max_hand = max;
        h.hands.insert(a, objs(1..cards + 1));
        let d = cleanup(h, &[]);
        let (w, report) = d.done.unwrap();
        assert!(d.asked.is_empty());
        let mut expected = CLEANUP_START.to_vec();
        expected.extend([Ev::Eot, Ev::Sba, Ev::Place]);
        expected.extend(CLEANUP_END);
        assert_eq!(w.host.log, expected);
        assert_eq!(
            report,
            SegmentReport {
                segment: Segment::Step(Step::Cleanup),
                heartbeat_ran: false,
                cleanup_steps: 1
            }
        );
        assert_eq!(w.host.game.turn().priority_player(), None);
    }
}

#[test]
fn req_turn_002_cleanup_discards_down_to_maximum_hand_size() {
    // REQ-TURN-002, CR 514.1, CR 402.2: with 10 cards and a maximum of 7, one request for
    // exactly 3 cards, then one discard of the chosen cards (ascending), then CR 514.2.
    let mut h = Fake::new(2).at_turn(3, 1);
    let a = h.p(1);
    h.hands.insert(a, objs(1..11));
    let d = cleanup(
        h,
        &[Some(ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(9), o(2), o(5)],
        })],
    );
    let (w, report) = d.done.unwrap();
    assert_eq!(d.asked.len(), 1);
    let r = &d.asked[0];
    assert_eq!(r.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!(r.chooser, a);
    assert_eq!(r.reason.cr, "CR 514.1");
    assert_eq!((r.min, r.max), (3, 3));
    assert!(!r.hidden);
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(1), o(2), o(3)]
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 3,
            max: 3,
            rule: ValidatorRule::Subset {
                candidates: objs(1..11)
            }
        })
    );
    assert_eq!(
        r.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: objs(1..11)
        }
    );
    let mut expected = CLEANUP_START.to_vec();
    expected.extend([
        Ev::Discard(a, vec![o(2), o(5), o(9)]),
        Ev::Eot,
        Ev::Sba,
        Ev::Place,
    ]);
    expected.extend(CLEANUP_END);
    assert_eq!(w.host.log, expected);
    assert_eq!(w.host.hand_of(a).len(), 7);
    assert_eq!(report.cleanup_steps, 1);
}

#[test]
fn req_turn_002_cleanup_discard_wrong_answers_are_rejected() {
    // REQ-TURN-002, R12, CR 514.1: a wrong count, a card not in hand or a duplicate is rejected
    // without panicking.
    for bad in [
        vec![o(1)],
        vec![o(1), o(2), o(3), o(4)],
        vec![o(1), o(2), o(99)],
        vec![o(1), o(1), o(2)],
    ] {
        let mut h = Fake::new(2).at_turn(3, 0);
        let a = h.p(0);
        h.hands.insert(a, objs(1..11));
        let d = cleanup(
            h,
            &[Some(ChoiceResponse::SubmitObjectSelection {
                objects: bad.clone(),
            })],
        );
        assert!(
            matches!(d.error, Some(ChoiceError::Invalid { .. })),
            "{bad:?}: {:?}",
            d.error
        );
    }
}

#[test]
fn req_turn_002_cleanup_discard_pending_surfaces_and_replay_reproduces_state() {
    // REQ-TURN-002, REQ-CORE-001, CR 514.1: the discard request suspends the turn unchanged; a
    // re-run from the base with the recorded answers reproduces the final state.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.hands.insert(a, objs(1..10));
    let base = world(h);
    let p = drive(base.clone(), &[], turn_of(a));
    let req = p.pending.expect("the discard request is pending");
    assert_eq!(req.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!((req.min, req.max), (2, 2));
    assert_eq!(p.asked.len(), 21);
    let d = drive(base.clone(), &[None], turn_of(a));
    let (w, _) = d.done.as_ref().unwrap();
    assert!(w.host.log.contains(&Ev::Discard(a, vec![o(1), o(2)])));
    let replayed = ChoiceSession::replay(base, turn_of(a), &d.answers).unwrap();
    assert_eq!(Some(replayed.state_hash()), d.hash);
}

#[test]
fn req_turn_002_cleanup_repeats_when_a_trigger_fires_during_it() {
    // REQ-TURN-002, CR 514.3a: a trigger waiting after the CR 514.2 actions is put on the
    // stack, the active player gets priority, and once the stack is empty and all pass another
    // cleanup step begins: begin-expiry, discard check and CR 514.2 again, end-expiry and mana
    // emptying after each cleanup step.
    let mut h = Fake::new(2).at_turn(3, 0);
    let a = h.p(0);
    h.cleanup_triggers = 1;
    h.resolve_draws = true;
    h.hands.insert(a, objs(1..8));
    let d = cleanup(h, &[None]);
    let (w, report) = d.done.unwrap();
    // First cleanup step: no discard (7 cards); the trigger goes on the stack.
    let mut expected = CLEANUP_START.to_vec();
    expected.extend([Ev::Eot, Ev::Sba, Ev::Place, Ev::Sba, Ev::Place]);
    // The heartbeat: settle, two passes, resolve (puts a card into the hand), settle, two
    // passes.
    expected.extend([Ev::Sba, Ev::Place, Ev::Resolve, Ev::Sba, Ev::Place]);
    expected.extend(CLEANUP_END);
    // Second cleanup step: 8 cards, discard one; nothing waiting.
    expected.extend(CLEANUP_START);
    expected.extend([Ev::Discard(a, vec![o(1)]), Ev::Eot, Ev::Sba, Ev::Place]);
    expected.extend(CLEANUP_END);
    assert_eq!(w.host.log, expected);
    assert_eq!(
        report,
        SegmentReport {
            segment: Segment::Step(Step::Cleanup),
            heartbeat_ran: true,
            cleanup_steps: 2
        }
    );
    let kinds: Vec<ChoiceType> = d.asked.iter().map(|r| r.choice_type).collect();
    assert_eq!(
        kinds,
        vec![
            ChoiceType::PriorityAction,
            ChoiceType::PriorityAction,
            ChoiceType::PriorityAction,
            ChoiceType::PriorityAction,
            ChoiceType::SelectObjectsFromZone,
        ]
    );
    // CR 514.3a: the active player gets priority.
    assert_eq!(d.asked[0].chooser, a);
    assert_eq!(w.host.game.turn().priority_player(), None);
}

#[test]
fn req_turn_002_cleanup_repeats_when_a_state_based_action_is_performed() {
    // REQ-TURN-002, CR 514.3a: a state-based action performed during the cleanup check gives
    // priority and another cleanup step.
    let mut h = Fake::new(2).at_turn(3, 0);
    h.cleanup_sbas = 2;
    let d = cleanup(h, &[]);
    let (w, report) = d.done.unwrap();
    assert_eq!(report.cleanup_steps, 3);
    assert!(report.heartbeat_ran);
    assert_eq!(w.host.eot_calls, 3);
    let begins = w
        .host
        .log
        .iter()
        .filter(|e| **e == Ev::Step(Step::Cleanup))
        .count();
    let ends = w
        .host
        .log
        .windows(2)
        .filter(|p| p[0] == Ev::Expire(ExpiryPoint::EndStep(Step::Cleanup)) && p[1] == Ev::Mana)
        .count();
    assert_eq!((begins, ends), (3, 3));
    // Priority was given in the first two cleanup steps only: two passes each.
    assert_eq!(d.asked.len(), 4);
}

#[test]
fn req_turn_002_cleanup_gives_priority_with_a_non_empty_stack() {
    // REQ-TURN-002, CR 514.3a: an object already on the stack needs priority to resolve.
    let mut h = Fake::new(2).at_turn(3, 0);
    h.stack.push("test.leftover".to_owned());
    let d = cleanup(h, &[]);
    let (w, report) = d.done.unwrap();
    assert_eq!(report.cleanup_steps, 2);
    assert!(w.host.log.contains(&Ev::Resolve));
    assert!(w.host.stack.is_empty());
}

#[test]
fn req_turn_002_cleanup_live_endless_cycle_repeats_one_step_per_pass_cycle() {
    // REQ-TURN-002, CR 514.3a: a host that always has work, with the players still in the
    // game, starts one more cleanup step after every completed pass cycle and never stops on
    // its own; with every player gone it stops after one step instead (F47). The
    // MAX_CLEANUP_STEPS guard itself is driven by
    // req_turn_002_cleanup_live_endless_cycle_hits_the_guard (docs/DECISIONS.md
    // "Gate 06-turn F95 — Cleanup guard test").
    let mut h = Fake::new(2).at_turn(3, 0);
    h.always_work = true;
    // The cleanup steps begun by the latest run, recorded as it suspends.
    let begun = std::cell::Cell::new(0usize);
    let procedure = |w: &mut World<Fake>, c: &mut ChoiceContext<'_>| {
        let r = run_segment(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            Segment::Step(Step::Cleanup),
            c,
        );
        let n = w
            .host
            .log
            .iter()
            .filter(|e| **e == Ev::Step(Step::Cleanup))
            .count();
        begun.set(n);
        r
    };
    let mut session = ChoiceSession::start(world(h), procedure).unwrap();
    for cycle in 0..30usize {
        // Pass cycle `cycle`: both players pass in cleanup step `cycle + 1`.
        assert_eq!(begun.get(), cycle + 1);
        for _ in 0..2 {
            let req = session
                .pending()
                .cloned()
                .expect("the cycle never ends by itself");
            assert_eq!(req.choice_type, ChoiceType::PriorityAction);
            session
                .submit(req.request_id, ChoiceResponse::PassPriority)
                .unwrap();
        }
    }
    assert_eq!(begun.get(), 31);
}

#[test]
fn req_turn_002_cleanup_live_endless_cycle_hits_the_guard() {
    // REQ-TURN-002, CR 514.3a: a live endless cleanup cycle (players still in the game, the
    // host always has work, every priority request answered with a pass) begins exactly
    // MAX_CLEANUP_STEPS cleanup steps and then fails with ChoiceError::Procedure instead of
    // looping forever. Driven in one run through the auto-default test hook (F95).
    let mut h = Fake::new(2).at_turn(3, 0);
    h.always_work = true;
    let mut ts = TurnState::new();
    let mut c = ChoiceContext::auto_default();
    let r = run_segment(
        &mut h,
        &mut ts,
        &TurnBasedRegistry::standard(),
        Segment::Step(Step::Cleanup),
        &mut c,
    );
    match r {
        Err(Interrupt::Error(ChoiceError::Procedure(m))) => {
            assert!(m.contains("MAX_CLEANUP_STEPS"), "{m}");
        }
        other => panic!("expected the MAX_CLEANUP_STEPS error, got {other:?}"),
    }
    let begun = h
        .log
        .iter()
        .filter(|e| **e == Ev::Step(Step::Cleanup))
        .count();
    assert_eq!(begun, usize::try_from(MAX_CLEANUP_STEPS).unwrap());
}

// ---------------------------------------------------------------------------------------------
// Boundaries and entry points
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_002_every_phase_expires_at_begin_and_end_then_empties_mana() {
    // REQ-TURN-002, CR 500.4, CR 500.5, CR 500.5a: each phase begins with its BeginPhase expiry
    // and ends with its EndPhase expiry followed by the mana emptying; "until end of combat"
    // expires at EndPhase(Combat), after the end of combat step ended.
    let mut h = Fake::new(2);
    let a = h.p(0);
    h.first_strike = true;
    let d = drive(world(h), &[], turn_of(a));
    let (w, _) = d.done.unwrap();
    let log = &w.host.log;
    for p in ALL_PHASES {
        let start = log.iter().position(|e| *e == Ev::Phase(p)).unwrap();
        assert_eq!(log[start + 1], Ev::Expire(ExpiryPoint::BeginPhase(p)));
        let end = log
            .iter()
            .position(|e| *e == Ev::Expire(ExpiryPoint::EndPhase(p)))
            .unwrap();
        assert_eq!(log[end + 1], Ev::Mana);
        assert!(end > start);
    }
    let eoc = log
        .iter()
        .position(|e| *e == Ev::Expire(ExpiryPoint::EndStep(Step::EndOfCombat)))
        .unwrap();
    assert_eq!(log[eoc + 1], Ev::Mana);
    assert_eq!(
        log[eoc + 2],
        Ev::Expire(ExpiryPoint::EndPhase(Phase::Combat))
    );
    // Every step: begin-expiry right after it begins; every end-expiry followed by mana.
    for (i, e) in log.iter().enumerate() {
        if let Ev::Step(s) = e {
            assert_eq!(log[i + 1], Ev::Expire(ExpiryPoint::BeginStep(*s)));
        }
        if let Ev::Expire(ExpiryPoint::EndStep(_) | ExpiryPoint::EndPhase(_)) = e {
            assert_eq!(log[i + 1], Ev::Mana);
        }
    }
    let manas = log.iter().filter(|e| **e == Ev::Mana).count();
    // 11 steps (with first strike) + 5 phases.
    assert_eq!(manas, 11 + 5);
}

#[test]
fn req_turn_002_run_segment_does_no_phase_boundary_work() {
    // REQ-TURN-002, CR 500.4, CR 500.5: run_segment does the step boundary work only.
    let h = Fake::new(2).at_turn(3, 0);
    let d = drive(
        world(h),
        &[],
        segment_of(Segment::Step(Step::Upkeep), TurnBasedRegistry::standard()),
    );
    let (w, report) = d.done.unwrap();
    assert_eq!(w.host.log, plain_step(Step::Upkeep, &[]));
    assert!(report.heartbeat_ran);
    assert_eq!(report.cleanup_steps, 0);
}

#[test]
fn req_turn_002_entry_points_need_an_active_player() {
    // REQ-TURN-002: run_phase and run_segment without an active player fail with
    // ChoiceError::Procedure.
    let h = Fake::new(2);
    let d = drive(world(h.clone()), &[], phase_of(Phase::Beginning));
    assert!(procedure_message(d.error.as_ref()).contains("active player"));
    let d = drive(
        world(h),
        &[],
        segment_of(Segment::PrecombatMain, TurnBasedRegistry::standard()),
    );
    assert!(procedure_message(d.error.as_ref()).contains("active player"));
}

#[test]
fn req_turn_002_heartbeat_with_nobody_in_game_ends_the_step() {
    // REQ-TURN-002, CR 500.5: a heartbeat that finds nobody in the game ends the step; the
    // end-of-step work still happens.
    let mut h = Fake::new(2).at_turn(3, 0);
    h.everyone_left = true;
    let d = drive(
        world(h),
        &[],
        segment_of(Segment::Step(Step::End), TurnBasedRegistry::standard()),
    );
    let (w, report) = d.done.unwrap();
    assert!(report.heartbeat_ran);
    assert_eq!(
        w.host.log,
        vec![
            Ev::Step(Step::End),
            Ev::Expire(ExpiryPoint::BeginStep(Step::End)),
            Ev::Expire(ExpiryPoint::EndStep(Step::End)),
            Ev::Mana
        ]
    );
}

#[test]
fn req_turn_002_turn_state_accessors() {
    // REQ-TURN-002: a fresh TurnState has no previous turn and an empty pass log.
    let ts = TurnState::new();
    assert_eq!(ts.previous_active_player(), None);
    assert!(ts.heartbeat().passes().is_empty());
    assert_eq!(ts, TurnState::default());
}

// ---------------------------------------------------------------------------------------------
// Gate 06-turn: combat decisions, the first draw step and cleanup with nobody left
// ---------------------------------------------------------------------------------------------

/// The combat steps a log went through, in order.
fn combat_steps_seen(log: &[Ev]) -> Vec<Step> {
    log.iter()
        .filter_map(|e| match e {
            Ev::Step(s) if s.phase() == Phase::Combat => Some(*s),
            _ => None,
        })
        .collect()
}

fn combat_phase(h: Fake) -> Drive<Fake, Vec<SegmentReport>> {
    drive(world(h), &[], phase_of(Phase::Combat))
}

#[test]
fn req_turn_002_no_attackers_skips_blockers_and_damage() {
    // REQ-TURN-002, CR 508.8: "If no creatures are declared as attackers or put onto the
    // battlefield attacking, skip the declare blockers and combat damage steps"; CR 506.1: the
    // end of combat step still happens (F46). The skipped steps leave no trace: no begin-step
    // event, expiry, combat action or priority.
    for first_strike in [false, true] {
        let mut h = Fake::new(2).at_turn(4, 0);
        h.attackers = false;
        h.first_strike = first_strike;
        let d = combat_phase(h);
        let (w, reports) = d.done.unwrap();
        assert_eq!(
            combat_steps_seen(&w.host.log),
            vec![
                Step::BeginningOfCombat,
                Step::DeclareAttackers,
                Step::EndOfCombat
            ]
        );
        for s in [
            Step::DeclareBlockers,
            Step::FirstStrikeDamage,
            Step::CombatDamage,
        ] {
            assert!(!w.host.log.contains(&Ev::Combat(s)), "{s:?}");
            assert!(
                !w.host.log.contains(&Ev::Expire(ExpiryPoint::BeginStep(s))),
                "{s:?}"
            );
            assert!(
                !w.host.log.contains(&Ev::Expire(ExpiryPoint::EndStep(s))),
                "{s:?}"
            );
        }
        let segs: Vec<Segment> = reports.iter().map(|r| r.segment).collect();
        assert_eq!(
            segs,
            vec![
                Segment::Step(Step::BeginningOfCombat),
                Segment::Step(Step::DeclareAttackers),
                Segment::Step(Step::EndOfCombat)
            ]
        );
        // Three steps with priority, two passes each.
        assert_eq!(d.asked.len(), 6);
        // The first-strike question is never asked without a damage step.
        assert!(w.host.first_strike_reads.borrow().is_empty());
        // CR 500.5a: the phase still ends with its expiry and mana emptying.
        assert!(
            w.host
                .log
                .ends_with(&[Ev::Expire(ExpiryPoint::EndPhase(Phase::Combat)), Ev::Mana])
        );
    }
}

#[test]
fn req_turn_002_attacker_put_onto_battlefield_during_declare_attackers_priority_keeps_steps() {
    // REQ-TURN-002, CR 508.8: the skip is decided after declare-attackers priority, so a
    // creature put onto the battlefield attacking during that priority keeps the declare
    // blockers and combat damage steps (F46); attackers present from the start keep them too.
    for added in [true, false] {
        let mut h = Fake::new(2).at_turn(4, 0);
        h.attackers = !added;
        h.resolve_adds_attacker = added;
        h.stack_in = vec![Step::DeclareAttackers];
        let d = combat_phase(h);
        let (w, _) = d.done.unwrap();
        assert_eq!(
            combat_steps_seen(&w.host.log),
            vec![
                Step::BeginningOfCombat,
                Step::DeclareAttackers,
                Step::DeclareBlockers,
                Step::CombatDamage,
                Step::EndOfCombat
            ],
            "added during priority: {added}"
        );
        assert!(w.host.log.contains(&Ev::Resolve));
    }
}

#[test]
fn req_turn_002_first_strike_decided_when_declare_blockers_ends() {
    // REQ-TURN-002, CR 510.4: "If at least one attacking or blocking creature has first strike
    // ... or double strike ... as the combat damage step begins": the flag is read once, after
    // declare-blockers priority and before the first damage step (F49, F18). A creature that
    // gains first strike during declare-blockers priority adds the first-strike step; one that
    // loses it removes the step; with no change the flag decides (control cases).
    let cases = [
        // (flag at the start, flips during declare blockers, first-strike step expected)
        (false, true, true),
        (true, true, false),
        (false, false, false),
        (true, false, true),
    ];
    for (start, flips, expected) in cases {
        let mut h = Fake::new(2).at_turn(4, 0);
        h.first_strike = start;
        h.resolve_flips_first_strike = flips;
        h.stack_in = vec![Step::DeclareBlockers];
        let d = combat_phase(h);
        let (w, reports) = d.done.unwrap();
        let mut steps = vec![
            Step::BeginningOfCombat,
            Step::DeclareAttackers,
            Step::DeclareBlockers,
        ];
        if expected {
            steps.push(Step::FirstStrikeDamage);
        }
        steps.extend([Step::CombatDamage, Step::EndOfCombat]);
        let label = format!("start {start}, flips {flips}");
        assert_eq!(combat_steps_seen(&w.host.log), steps, "{label}");
        let segs: Vec<Segment> = reports.iter().map(|r| r.segment).collect();
        assert_eq!(
            segs,
            steps.iter().map(|s| Segment::Step(*s)).collect::<Vec<_>>(),
            "{label}"
        );
        // The damage split: the first-strike step, when present, runs its combat action and
        // priority before the regular combat damage step.
        if expected {
            let fs = w
                .host
                .log
                .iter()
                .position(|e| *e == Ev::Combat(Step::FirstStrikeDamage));
            let cd = w
                .host
                .log
                .iter()
                .position(|e| *e == Ev::Combat(Step::CombatDamage));
            assert!(fs.unwrap() < cd.unwrap(), "{label}");
        }
        // Read exactly once, at the end of the declare blockers step (after its resolution).
        assert_eq!(
            *w.host.first_strike_reads.borrow(),
            vec![Some(Step::DeclareBlockers)],
            "{label}"
        );
        let resolve = w.host.log.iter().position(|e| *e == Ev::Resolve).unwrap();
        let blockers_end = w
            .host
            .log
            .iter()
            .position(|e| *e == Ev::Expire(ExpiryPoint::EndStep(Step::DeclareBlockers)))
            .unwrap();
        assert!(resolve < blockers_end, "{label}");
    }
}

#[test]
fn req_turn_002_cleanup_with_nobody_left_and_a_stack_ends_after_one_step() {
    // REQ-TURN-002, CR 514.3a, CR 800.4j: every player leaves during cleanup while an object
    // stays on the stack. The heartbeat finds nobody in the game, so no pass cycle can
    // complete: the cleanup step ends (end-step expiry, mana emptying) and no further cleanup
    // step begins; there is no MAX_CLEANUP_STEPS error (F47).
    let mut h = Fake::new(2).at_turn(3, 0);
    h.leave_in_cleanup = true;
    let d = cleanup(h, &[]);
    let (w, report) = d.done.expect("cleanup ends without an error");
    assert!(d.asked.is_empty());
    assert_eq!(
        report,
        SegmentReport {
            segment: Segment::Step(Step::Cleanup),
            heartbeat_ran: true,
            cleanup_steps: 1
        }
    );
    let begins = w
        .host
        .log
        .iter()
        .filter(|e| **e == Ev::Step(Step::Cleanup))
        .count();
    assert_eq!(begins, 1);
    assert!(w.host.log.ends_with(&CLEANUP_END));
    assert!(!w.host.stack.is_empty());
}
