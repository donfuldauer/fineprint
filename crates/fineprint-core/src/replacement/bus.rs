// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The bus loop of SRS 8.1 (REQ-EVENT-001): collect, choose by tier, apply, repeat, execute,
//! feed child events back (CR 614.6, CR 615.6, CR 616.1, CR 704.7).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId};

use super::types::{
    Action, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification, ReplacementEffect,
    ReplacementRegistry, Shield, affected_object, affected_player,
};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceTarget, ChoiceType, PendingChoiceRequest,
};
use crate::events::{
    AppliedEffects, EventBatch, EventContext, EventError, EventInstance, EventState, ExecutedBatch,
    GameEvent, Outcome,
};
use crate::memory::{LkiStore, MemoryTable};
use crate::permanent::Permanents;
use crate::state::GameState;
use crate::zones::{ZoneId, ZoneKind, Zones};

/// The deepest nesting of child event batches (REQ-EVENT-001): a child batch at a depth above
/// this is [`BusError::DepthExceeded`]. The submitted batch is depth 0.
pub const MAX_CHILD_DEPTH: usize = 64;

/// The most applications of replacement effects to one event (REQ-EVENT-001); one more is
/// [`BusError::RepeatLimit`]. Keeps CR 614.5-overriding effects (CR 903.9b-style) total.
pub const MAX_REPEATS: usize = 256;

/// The most event instances one [`process_batch`] call takes up (REQ-EVENT-001): the members
/// of the submitted batch and every child event at every depth, charged before they are
/// queued. One more is [`BusError::WorkLimit`]; it keeps expanding `Instead` effects total.
pub const MAX_BUS_EVENTS: usize = 4096;

/// What [`process_batch`] did (REQ-EVENT-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusReport {
    /// Every batch the bus executed in which something occurred, in execution order (the batch
    /// of the final modified events, then child batches depth-first); empty if nothing occurred.
    pub executed: Vec<ExecutedBatch>,
    /// Every application of an effect, in order (a CR 704.7 merged application counts once).
    pub applications: Vec<EffectId>,
    /// The total damage prevented (CR 615.6).
    pub prevented: i64,
}

/// Processes one batch of simultaneous events through the replacement and prevention bus
/// (REQ-EVENT-001, SRS 8.1, CR 614.6, CR 615.6, CR 616.1).
///
/// Each event is processed in APNAP order of its chooser (CR 616.1): the applicable effects
/// are collected against the current state, excluding the event's applied effects (CR 614.5)
/// unless an effect overrides that (CR 903.9b-style); contained effects wait while a
/// non-contained one applies (CR 616.1g); only the first non-empty tier is choosable
/// (CR 616.1a-e); one effect is applied without asking, several are offered through
/// `CHOOSE_REPLACEMENT_EFFECT`; the loop repeats on the modified event (CR 616.1f, CR 616.2).
/// The surviving events are then executed as one batch in submission order, and the queued
/// child events form one child batch processed the same way. An `Instead` child keeps the
/// applied set of the event it replaces (CR 614.5); other children start with none. With
/// `sba` true, zone changes of the same object to the same destination are one event
/// (CR 704.7). Before a batch is processed, a player whose prevention pool cannot cover
/// simultaneous damage from several sources divides it among them (CR 615.7). At most
/// [`MAX_BUS_EVENTS`] event instances are taken up in one call.
///
/// # Errors
/// [`BusInterrupt::Pending`] when a player must choose (propagate it to the
/// [`ChoiceSession`](crate::choice::ChoiceSession)); [`BusInterrupt::Error`] on a failure. In
/// both cases the registry and every component of `ctx` are restored to their values before
/// the call.
pub fn process_batch(
    ctx: &mut EventContext<'_>,
    registry: &mut ReplacementRegistry,
    batch: &EventBatch,
    sba: bool,
    choices: &mut ChoiceContext<'_>,
) -> Result<BusReport, BusInterrupt> {
    let registry_before = registry.clone();
    let world = WorldSnapshot::take(ctx);
    let mut bus = Bus {
        ctx,
        registry,
        choices,
        report: BusReport {
            executed: Vec::new(),
            applications: Vec::new(),
            prevented: 0,
        },
        work: 0,
        allotments: BTreeMap::new(),
    };
    let result = match bus.charge(batch.events().len()) {
        Ok(()) => bus.run(batch.events().to_vec(), sba, 0),
        Err(e) => Err(e.into()),
    };
    match result {
        Ok(()) => Ok(bus.report),
        Err(e) => {
            world.restore(bus.ctx);
            *bus.registry = registry_before;
            Err(e)
        }
    }
}

/// Processes one event (`sba` false) through the bus (REQ-EVENT-001); see [`process_batch`].
///
/// # Errors
/// As [`process_batch`].
pub fn process_event(
    ctx: &mut EventContext<'_>,
    registry: &mut ReplacementRegistry,
    event: GameEvent,
    choices: &mut ChoiceContext<'_>,
) -> Result<BusReport, BusInterrupt> {
    process_batch(
        ctx,
        registry,
        &EventBatch::from_events(vec![event]),
        false,
        choices,
    )
}

/// A clone of every component of an [`EventContext`], for all-or-nothing processing.
struct WorldSnapshot {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
}

impl WorldSnapshot {
    fn take(ctx: &EventContext<'_>) -> WorldSnapshot {
        WorldSnapshot {
            game: ctx.game.clone(),
            zones: ctx.zones.clone(),
            ids: ctx.ids.clone(),
            permanents: ctx.permanents.clone(),
            memories: ctx.memories.clone(),
            lki: ctx.lki.clone(),
            events: ctx.events.clone(),
        }
    }

    fn restore(self, ctx: &mut EventContext<'_>) {
        *ctx.game = self.game;
        *ctx.zones = self.zones;
        *ctx.ids = self.ids;
        *ctx.permanents = self.permanents;
        *ctx.memories = self.memories;
        *ctx.lki = self.lki;
        *ctx.events = self.events;
    }
}

/// One event of a batch while the bus works on it.
struct Member {
    /// The event as modified so far; `None` once it will not happen.
    event: Option<GameEvent>,
    /// The effects that modified it (CR 614.5).
    applied: AppliedEffects,
    /// Applications so far.
    repeats: usize,
    /// Its chooser as submitted (CR 616.1).
    chooser: PlayerId,
    /// The `Shield::Pool` effects whose pool its chooser allocated among sources (CR 615.7).
    pools: BTreeSet<EffectId>,
}

/// What the members of one batch queued while they were resolved.
#[derive(Default)]
struct Queue {
    /// Child events, in the order they were queued, with their applied sets.
    children: Vec<EventInstance>,
    /// `(member, copied)`: the object member `member` puts onto the battlefield becomes a copy
    /// of `copied` (CR 614.1c).
    copies: Vec<(usize, ObjectId)>,
}

struct Bus<'r, 'c, 'h> {
    ctx: &'r mut EventContext<'c>,
    registry: &'r mut ReplacementRegistry,
    choices: &'r mut ChoiceContext<'h>,
    report: BusReport,
    /// Event instances taken up so far in this call (see [`MAX_BUS_EVENTS`]).
    work: usize,
    /// The CR 615.7 allotments of the current batch: `(pool effect, chooser, source)` to the
    /// damage of that source the shield may still prevent.
    allotments: BTreeMap<(EffectId, PlayerId, ObjectId), i64>,
}

impl Bus<'_, '_, '_> {
    /// Charges `n` more event instances against the call's work budget; more than
    /// [`MAX_BUS_EVENTS`] in all is [`BusError::WorkLimit`].
    fn charge(&mut self, n: usize) -> Result<(), BusError> {
        self.work = self.work.saturating_add(n);
        if self.work > MAX_BUS_EVENTS {
            return Err(BusError::WorkLimit);
        }
        Ok(())
    }

    /// Processes one batch at child depth `depth` (SRS 8.1). The events were charged against
    /// the work budget when they were submitted or queued.
    fn run(
        &mut self,
        events: Vec<EventInstance>,
        sba: bool,
        depth: usize,
    ) -> Result<(), BusInterrupt> {
        if depth > MAX_CHILD_DEPTH {
            return Err(BusError::DepthExceeded.into());
        }
        if events.is_empty() {
            return Ok(());
        }
        let mut members = Vec::with_capacity(events.len());
        for instance in events {
            members.push(Member {
                chooser: self.chooser(instance.event())?,
                event: Some(instance.event().clone()),
                applied: instance.applied_effects().clone(),
                repeats: 0,
                pools: BTreeSet::new(),
            });
        }
        if sba {
            coalesce(&mut members);
        }
        let choosers: Vec<PlayerId> = members.iter().map(|m| m.chooser).collect();
        let order = self.apnap_order(&choosers);
        self.allocate(&mut members, &order)?;
        let mut queue = Queue::default();
        for &i in &order {
            self.resolve(&mut members, i, &mut queue)?;
        }
        let entered = self.execute(&members)?;
        self.copy(&queue.copies, &entered)?;
        if queue.children.is_empty() {
            return Ok(());
        }
        self.run(queue.children, false, depth + 1)
    }

    /// Executes the surviving events as one batch in submission order (SRS 8.1: execute E);
    /// returns, per member index, the object it put onto the battlefield.
    fn execute(&mut self, members: &[Member]) -> Result<BTreeMap<usize, ObjectId>, BusError> {
        let survivors: Vec<(usize, EventInstance)> = members
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                m.event
                    .clone()
                    .map(|e| (i, EventInstance::new(e).with_applied(m.applied.clone())))
            })
            .collect();
        let mut entered = BTreeMap::new();
        if survivors.is_empty() {
            return Ok(entered);
        }
        let batch = EventBatch::new(survivors.iter().map(|(_, s)| s.clone()).collect());
        let executed = self.ctx.execute_batch(&batch)?;
        // Executed events are the survivors that occurred, in submission order.
        let mut next = executed.events.iter().peekable();
        for (i, instance) in &survivors {
            if let Some(ex) = next.peek()
                && ex.instance == *instance
            {
                if let Some(object) = entered_object(ex.outcome) {
                    entered.insert(*i, object);
                }
                next.next();
            }
        }
        if !executed.events.is_empty() {
            self.report.executed.push(executed);
        }
        Ok(entered)
    }

    /// CR 614.1c, CR 616.1c: an object that enters as a copy is a copy as part of its entry.
    /// The copies are executed directly, as one batch right after the entry batch, without
    /// replacement collection (the same-event entry with the ETB look-ahead is a later unit).
    fn copy(
        &mut self,
        copies: &[(usize, ObjectId)],
        entered: &BTreeMap<usize, ObjectId>,
    ) -> Result<(), BusError> {
        let events: Vec<EventInstance> = copies
            .iter()
            .filter_map(|(member, copied)| {
                entered.get(member).map(|&object| {
                    EventInstance::new(GameEvent::BecomeCopy {
                        object,
                        copied: *copied,
                    })
                })
            })
            .collect();
        if events.is_empty() {
            return Ok(());
        }
        let executed = self.ctx.execute_batch(&EventBatch::new(events))?;
        if !executed.events.is_empty() {
            self.report.executed.push(executed);
        }
        Ok(())
    }

    /// The chooser of `event` (CR 616.1): the controller of the affected object (CR 112.2,
    /// CR 109.4: a permanent's or a stack object's controller), its owner if it has no
    /// controller, else the affected player; for a token creation the controller it would
    /// enter under; with neither, the active player.
    fn chooser(&self, event: &GameEvent) -> Result<PlayerId, BusError> {
        // CR 616.1
        if let GameEvent::CreateToken {
            owner, controller, ..
        } = event
        {
            return Ok(controller.unwrap_or(*owner));
        }
        if let Some(object) = affected_object(event) {
            if let Some(permanent) = self.ctx.permanents.get(object) {
                return Ok(permanent.controller());
            }
            // CR 109.4, CR 112.2: a spell on the stack has a controller; elsewhere the owner.
            return self
                .ctx
                .zones
                .object(object)
                .map(|r| r.rules_controller().unwrap_or(r.owner))
                .ok_or(BusError::Event(EventError::UnknownObject(object)));
        }
        if let Some(player) = affected_player(event) {
            return Ok(player);
        }
        self.player_order()
            .first()
            .copied()
            .ok_or(BusError::Event(EventError::UnknownPlayer(PlayerId::new(0))))
    }

    /// The players in APNAP order: the turn order starting from the active player (ascending
    /// ids if the turn order is empty).
    fn player_order(&self) -> Vec<PlayerId> {
        let game = &*self.ctx.game;
        let mut order: Vec<PlayerId> = if game.turn_order().is_empty() {
            game.players().map(|p| p.id()).collect()
        } else {
            game.turn_order().to_vec()
        };
        if let Some(active) = game.turn().active_player()
            && let Some(pos) = order.iter().position(|&p| p == active)
        {
            order.rotate_left(pos);
        }
        order
    }

    /// Member indices in APNAP order of their choosers, submission order within one chooser
    /// (CR 616.1, SRS 8.1 "simultaneous choosers -> APNAP").
    fn apnap_order(&self, choosers: &[PlayerId]) -> Vec<usize> {
        let players = self.player_order();
        let rank = |p: PlayerId| {
            players
                .iter()
                .position(|&q| q == p)
                .unwrap_or(players.len())
        };
        let mut order: Vec<usize> = (0..choosers.len()).collect();
        order.sort_by_key(|&i| (rank(choosers[i]), choosers[i], i));
        order
    }

    /// Whether `m` matches `event` now (SRS 8.1 step [1]: against the current state).
    fn matches(&self, m: &EventMatcher, event: &GameEvent) -> bool {
        if m.kind != Some(BusEventKind::of(event)) {
            return false;
        }
        if m.affected_object.is_some() && m.affected_object != affected_object(event) {
            return false;
        }
        if m.affected_player.is_some() && m.affected_player != affected_player(event) {
            return false;
        }
        if let Some(z) = m.from {
            let GameEvent::ZoneChange { object, .. } = event else {
                return false;
            };
            if self.ctx.zones.object(*object).map(|r| r.zone.kind()) != Some(z) {
                return false;
            }
        }
        if let Some(z) = m.to {
            let to = match event {
                GameEvent::ZoneChange { request, .. } => request.to,
                GameEvent::CreateToken { .. } => ZoneKind::Battlefield,
                _ => return false,
            };
            if to != z {
                return false;
            }
        }
        if let Some(c) = m.cause
            && !matches!(event, GameEvent::ZoneChange { cause, .. } if *cause == c)
        {
            return false;
        }
        if let Some(c) = m.combat
            && !matches!(event, GameEvent::Damage { combat, .. } if *combat == c)
        {
            return false;
        }
        if let Some(k) = &m.counter_kind {
            let kind = match event {
                GameEvent::AddCounters { kind, .. } | GameEvent::RemoveCounters { kind, .. } => {
                    kind
                }
                _ => return false,
            };
            if kind != k {
                return false;
            }
        }
        if let Some(n) = m.amount_below {
            let amount = match event {
                GameEvent::Damage { amount, .. }
                | GameEvent::AddCounters { amount, .. }
                | GameEvent::RemoveCounters { amount, .. } => *amount,
                GameEvent::LifeChange { delta, .. } => delta.saturating_abs(),
                _ => return false,
            };
            if amount >= n {
                return false;
            }
        }
        if let Some((object, kind)) = &m.requires_counter
            && self.ctx.events.markers().counter(*object, kind) < 1
        {
            return false;
        }
        true
    }

    /// Whether `event` would happen at all. CR 614.7: an effect that would replace an event
    /// that never happens does nothing. CR 614.7a, CR 120.8: a source that would deal 0 damage
    /// deals no damage. CR 701.26b: only tapped permanents can be untapped (so CR 122.1d's
    /// stun counter replaces only a real untap). A `Tap` of a tapped permanent is not treated
    /// here.
    fn would_happen(&self, event: &GameEvent) -> bool {
        match event {
            GameEvent::Damage { amount, .. } => *amount > 0,
            GameEvent::Untap { object } => self
                .ctx
                .permanents
                .get(*object)
                .is_some_and(|p| p.status().tapped),
            _ => true,
        }
    }

    /// The choosable effects for `member`'s current `event` (SRS 8.1 steps [1] and [3]),
    /// ascending id.
    fn choosable(&self, event: &GameEvent, member: &Member) -> Vec<EffectId> {
        // CR 614.7, CR 614.7a: an event that would not happen offers no effect.
        if !self.would_happen(event) {
            return Vec::new();
        }
        // [1] CR 614.5: not an effect already applied, unless it overrides 614.5 (CR 903.9b).
        let mut candidates: Vec<&ReplacementEffect> = self
            .registry
            .iter()
            .filter(|e| e.overrides_614_5 || !member.applied.contains(e.id))
            .filter(|e| match e.shield {
                Some(Shield::Uses(n)) => n > 0,
                Some(Shield::Pool(n)) => n > 0 && self.allotment(e.id, member, event) != Some(0),
                None => true,
            })
            .filter(|e| self.matches(&e.matcher, event))
            .collect();
        // CR 616.1g: a contained effect can't be chosen while another effect applies to the
        // containing event; filtered before the tier restriction.
        if candidates.iter().any(|e| !e.contained) {
            candidates.retain(|e| !e.contained);
        }
        // [3] CR 616.1a-e: the first non-empty tier.
        let Some(top) = candidates.iter().map(|e| e.tier).min() else {
            return Vec::new();
        };
        candidates
            .into_iter()
            .filter(|e| e.tier == top)
            .map(|e| e.id)
            .collect()
    }

    /// CR 615.7: what is left of the allotment the chooser gave `event`'s source from pool
    /// `pool`, if the member's damage was allocated.
    fn allotment(&self, pool: EffectId, member: &Member, event: &GameEvent) -> Option<i64> {
        if !member.pools.contains(&pool) {
            return None;
        }
        let GameEvent::Damage {
            source: Some(source),
            ..
        } = event
        else {
            return None;
        };
        self.allotments
            .get(&(pool, member.chooser, *source))
            .copied()
    }

    /// CR 615.7: before the batch is processed, each chooser whose simultaneous damage from
    /// two or more sources exceeds a prevention pool chooses which damage the pool prevents
    /// (`DIVIDE_VALUE`), in ascending pool id, then APNAP chooser order.
    fn allocate(&mut self, members: &mut [Member], order: &[usize]) -> Result<(), BusInterrupt> {
        self.allotments.clear();
        let pools: Vec<(EffectId, i64, EventMatcher)> = self
            .registry
            .iter()
            .filter_map(|e| match e.shield {
                Some(Shield::Pool(n)) if n > 0 => Some((e.id, n, e.matcher.clone())),
                _ => None,
            })
            .collect();
        if pools.is_empty() {
            return Ok(());
        }
        let mut players: Vec<PlayerId> = Vec::new();
        for &i in order {
            if !players.contains(&members[i].chooser) {
                players.push(members[i].chooser);
            }
        }
        for (pool_id, pool, matcher) in pools {
            for &chooser in &players {
                let mut eligible = Vec::new();
                let mut sources: BTreeMap<ObjectId, i64> = BTreeMap::new();
                let mut total: i64 = 0;
                for (i, m) in members.iter().enumerate() {
                    if m.chooser != chooser || m.applied.contains(pool_id) {
                        continue;
                    }
                    let Some(
                        event @ GameEvent::Damage {
                            source: Some(source),
                            amount,
                            ..
                        },
                    ) = &m.event
                    else {
                        continue;
                    };
                    if *amount <= 0 || !self.matches(&matcher, event) {
                        continue;
                    }
                    eligible.push(i);
                    let entry = sources.entry(*source).or_insert(0);
                    *entry = entry.saturating_add(*amount);
                    total = total.saturating_add(*amount);
                }
                if sources.len() < 2 || total <= pool {
                    continue;
                }
                let shares = self.ask_division(chooser, pool, &sources)?;
                for (source, share) in shares {
                    self.allotments.insert((pool_id, chooser, source), share);
                }
                for i in eligible {
                    members[i].pools.insert(pool_id);
                }
            }
        }
        Ok(())
    }

    /// CR 615.7: asks `chooser` to divide `pool` among `sources` (ascending id).
    fn ask_division(
        &mut self,
        chooser: PlayerId,
        pool: i64,
        sources: &BTreeMap<ObjectId, i64>,
    ) -> Result<Vec<(ObjectId, i64)>, BusInterrupt> {
        let total = u64::try_from(pool).unwrap_or(0);
        let recipients: Vec<(ChoiceTarget, u64)> = sources
            .keys()
            .map(|&s| (ChoiceTarget::Object(s), 0))
            .collect();
        let count = u64::try_from(recipients.len()).unwrap_or(u64::MAX);
        let default: Vec<(ChoiceTarget, u64)> = recipients
            .iter()
            .enumerate()
            .map(|(k, &(t, _))| (t, if k == 0 { total } else { 0 }))
            .collect();
        // CR 615.7
        let request = PendingChoiceRequest {
            request_id: self.choices.next_request_id(),
            chooser,
            choice_type: ChoiceType::DivideValue,
            reason: ChoiceReason {
                cr: "CR 615.7".to_owned(),
                source: None,
            },
            min: count,
            max: count,
            hidden: false,
            reveal_to: Vec::new(),
            default_on_timeout: ChoiceResponse::SubmitDivision {
                assignments: default,
            },
            options: ChoiceOptions::Enumerated(
                sources.keys().map(|&s| ChoiceOption::Object(s)).collect(),
            ),
            payload: ChoicePayload::DivideValue { total, recipients },
        };
        match self.choices.ask(request)? {
            ChoiceResponse::SubmitDivision { assignments } => assignments
                .into_iter()
                .map(|(target, amount)| match target {
                    ChoiceTarget::Object(s) if sources.contains_key(&s) => {
                        Ok((s, i64::try_from(amount).unwrap_or(i64::MAX)))
                    }
                    other => Err(BusInterrupt::Error(BusError::Choice(
                        ChoiceError::Procedure(format!("{other} is not a damage source here")),
                    ))),
                })
                .collect(),
            other => Err(BusInterrupt::Error(BusError::Choice(
                ChoiceError::Procedure(format!("unexpected answer {}", other.wire_name())),
            ))),
        }
    }

    /// SRS 8.1 step [4]: the chooser picks one of `set` (asked only if it has several).
    fn choose(&mut self, event: &GameEvent, set: &[EffectId]) -> Result<EffectId, BusInterrupt> {
        if let [only] = set {
            return Ok(*only);
        }
        let chooser = self.chooser(event)?;
        let effects: Vec<String> = set
            .iter()
            .filter_map(|id| self.registry.get(*id).map(|e| e.description.clone()))
            .collect();
        let options = (0..set.len())
            .map(|i| ChoiceOption::Number(i64::try_from(i).unwrap_or(i64::MAX)))
            .collect();
        // CR 616.1
        let request = PendingChoiceRequest {
            request_id: self.choices.next_request_id(),
            chooser,
            choice_type: ChoiceType::ChooseReplacementEffect,
            reason: ChoiceReason {
                cr: "CR 616.1".to_owned(),
                source: affected_object(event),
            },
            min: 1,
            max: 1,
            hidden: false,
            reveal_to: Vec::new(),
            default_on_timeout: ChoiceResponse::SelectReplacement { index: 0 },
            options: ChoiceOptions::Enumerated(options),
            payload: ChoicePayload::ChooseReplacementEffect {
                event: BusEventKind::of(event).name().to_owned(),
                effects,
            },
        };
        match self.choices.ask(request)? {
            ChoiceResponse::SelectReplacement { index } => usize::try_from(index)
                .ok()
                .and_then(|i| set.get(i).copied())
                .ok_or_else(|| {
                    BusInterrupt::Error(BusError::Choice(ChoiceError::Procedure(format!(
                        "replacement index {index} is out of range"
                    ))))
                }),
            other => Err(BusInterrupt::Error(BusError::Choice(
                ChoiceError::Procedure(format!("unexpected answer {}", other.wire_name())),
            ))),
        }
    }

    /// SRS 8.1 steps [1]-[5] for one member until no effect applies (CR 616.1f).
    fn resolve(
        &mut self,
        members: &mut [Member],
        i: usize,
        queue: &mut Queue,
    ) -> Result<(), BusInterrupt> {
        loop {
            let Some(event) = members[i].event.clone() else {
                return Ok(());
            };
            let set = self.choosable(&event, &members[i]);
            if set.is_empty() {
                // SRS 8.1: R(E) empty -> execute E.
                return Ok(());
            }
            let chosen = self.choose(&event, &set)?;
            let effect = self.effect(chosen)?;
            let mut pool = pool_of(&effect);
            self.apply(&mut members[i], i, &effect, &mut pool, queue)?;
            self.consume(&effect, pool);
            self.report.applications.push(chosen);
        }
    }

    fn effect(&self, id: EffectId) -> Result<ReplacementEffect, BusError> {
        self.registry.get(id).cloned().ok_or_else(|| {
            BusError::Choice(ChoiceError::Procedure(format!(
                "effect {} vanished from the registry",
                id.get()
            )))
        })
    }

    /// SRS 8.1 step [4]: E' = R*(E), E'.AppliedEffects += R* (CR 614.5); CR 615.6 prevention.
    fn apply(
        &mut self,
        member: &mut Member,
        index: usize,
        effect: &ReplacementEffect,
        pool: &mut Option<i64>,
        queue: &mut Queue,
    ) -> Result<(), BusError> {
        if member.repeats >= MAX_REPEATS {
            return Err(BusError::RepeatLimit);
        }
        member.repeats += 1;
        // CR 614.5
        member.applied.insert(effect.id);
        let Some(event) = member.event.as_mut() else {
            return Ok(());
        };
        match &effect.action {
            // CR 614.1c, CR 616.1c: the copy is part of the entry (see `copy`).
            Action::Modify(Modification::EntersAsCopyOf(copied)) => {
                queue.copies.push((index, *copied));
            }
            Action::Modify(m) => {
                if !modify(event, *m) {
                    member.event = None;
                }
            }
            Action::Skip => member.event = None,
            Action::Instead(events) => {
                // CR 614.6: the modified event replaces the event; CR 614.5: it keeps the
                // replaced event's applied effects, this one included.
                member.event = None;
                self.charge(events.len())?;
                queue.children.extend(
                    events
                        .iter()
                        .cloned()
                        .map(|e| EventInstance::new(e).with_applied(member.applied.clone())),
                );
            }
            Action::Prevent { then } => {
                // CR 615.6: prevented damage never happens. CR 615.7: a pool prevents up to
                // what is left of it (and of the chooser's allotment for the source).
                if let GameEvent::Damage { source, amount, .. } = event {
                    let available = (*amount).max(0);
                    let allotment = match source {
                        Some(s) if member.pools.contains(&effect.id) => {
                            self.allotments.get_mut(&(effect.id, member.chooser, *s))
                        }
                        _ => None,
                    };
                    let prevented = match pool {
                        Some(n) => {
                            let mut p = available.min((*n).max(0));
                            if let Some(a) = allotment {
                                p = p.min((*a).max(0));
                                *a -= p;
                            }
                            *n -= p;
                            p
                        }
                        None => available,
                    };
                    *amount -= prevented;
                    self.report.prevented = self.report.prevented.saturating_add(prevented);
                    if *amount <= 0 {
                        member.event = None;
                    }
                }
                // CR 615.5: the follow-up is a new event, with no applied effects.
                self.charge(then.len())?;
                queue
                    .children
                    .extend(then.iter().cloned().map(EventInstance::new));
            }
        }
        Ok(())
    }

    /// Consumes one use of `effect`'s shield, or sets its pool to `pool`; removes it when
    /// exhausted (CR 615.7, CR 615.8).
    fn consume(&mut self, effect: &ReplacementEffect, pool: Option<i64>) {
        let exhausted = match self.registry.get_mut(effect.id) {
            Some(e) => match e.shield {
                Some(Shield::Uses(n)) => {
                    let left = n.saturating_sub(1);
                    e.shield = Some(Shield::Uses(left));
                    left == 0
                }
                Some(Shield::Pool(_)) => {
                    let left = pool.unwrap_or(0).max(0);
                    e.shield = Some(Shield::Pool(left));
                    left == 0
                }
                None => false,
            },
            None => false,
        };
        if exhausted {
            self.registry.remove(effect.id);
        }
    }
}

/// CR 704.3, CR 704.7: state-based actions that would have the same result happen once. Of
/// the zone changes of one object to one destination, only the first (in submission order)
/// is kept; the others are dropped. Different objects never merge.
fn coalesce(members: &mut [Member]) {
    let mut seen: BTreeSet<(ObjectId, ZoneKind)> = BTreeSet::new();
    for m in members {
        if let Some(GameEvent::ZoneChange {
            object, request, ..
        }) = &m.event
            && !seen.insert((*object, request.to))
        {
            m.event = None;
        }
    }
}

fn pool_of(effect: &ReplacementEffect) -> Option<i64> {
    match effect.shield {
        Some(Shield::Pool(n)) => Some(n),
        _ => None,
    }
}

/// Rewrites `event` by `m` (CR 614.6); `false` if the result is a no-op (amount 0).
fn modify(event: &mut GameEvent, m: Modification) -> bool {
    match (event, m) {
        (GameEvent::ZoneChange { request, .. }, Modification::SetDestination(z)) => {
            request.to = z;
            // Only the battlefield and the stack take a controller (CR 109.4).
            if !matches!(z, ZoneKind::Battlefield | ZoneKind::Stack) {
                request.controller = None;
            }
        }
        (GameEvent::ZoneChange { request, .. }, Modification::SetController(p)) => {
            request.controller = Some(p);
        }
        (GameEvent::CreateToken { controller, .. }, Modification::SetController(p)) => {
            *controller = Some(p);
        }
        (GameEvent::ZoneChange { request, .. }, Modification::SetFaceDown(b)) => {
            request.face_down = b;
        }
        (
            GameEvent::Damage { amount, .. }
            | GameEvent::AddCounters { amount, .. }
            | GameEvent::RemoveCounters { amount, .. },
            Modification::Amount(op),
        ) => {
            *amount = op.apply(*amount);
            return *amount > 0;
        }
        (GameEvent::LifeChange { delta, .. }, Modification::Amount(op)) => {
            let magnitude = op.apply(delta.saturating_abs());
            *delta = if *delta < 0 { -magnitude } else { magnitude };
            return magnitude > 0;
        }
        // `add` rejects every other pairing.
        _ => {}
    }
    true
}

/// The object an executed event put onto the battlefield, if any.
fn entered_object(outcome: Outcome) -> Option<ObjectId> {
    match outcome {
        Outcome::ZoneChange(moved)
            if moved.to == ZoneId::Battlefield && moved.from != ZoneId::Battlefield =>
        {
            Some(moved.new)
        }
        Outcome::TokenCreated { object } => Some(object),
        _ => None,
    }
}
