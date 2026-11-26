// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The bus loop of SRS 8.1 (REQ-EVENT-001): collect, choose by tier, apply, repeat, execute,
//! feed child events back (CR 614.6, CR 615.6, CR 616.1, CR 704.7).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId};

use super::types::{
    Action, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification, ReplacementEffect,
    ReplacementRegistry, Shield, affected_object, affected_player, required_objects,
};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceTarget, ChoiceType, PendingChoiceRequest,
};
use crate::events::{
    AppliedEffects, EventBatch, EventContext, EventError, EventInstance, EventState, ExecutedBatch,
    GameEvent,
};
use crate::memory::{LkiStore, MemoryTable};
use crate::permanent::Permanents;
use crate::state::GameState;
use crate::zones::{ZoneKind, Zones};

/// The deepest nesting of events (REQ-EVENT-001): an event whose depth is above this is
/// [`BusError::DepthExceeded`]. The submitted batch is depth 0; an `Instead` event is one
/// deeper than the event it replaces (within the same batch), and the `Prevent` follow-up
/// batch is one deeper than the batch that queued it.
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
    /// Every batch the bus executed in which something occurred, in execution order: the one
    /// batch of the final events (every `Instead` descendant included), then the `Prevent`
    /// follow-up batches depth-first; empty if nothing occurred.
    pub executed: Vec<ExecutedBatch>,
    /// Every application of an effect, in order (a CR 704.7 merged application counts once).
    pub applications: Vec<EffectId>,
    /// The total damage prevented (CR 615.6).
    pub prevented: i64,
}

/// Processes one batch of simultaneous events through the replacement and prevention bus
/// (REQ-EVENT-001, SRS 8.1, CR 614.6, CR 615.6, CR 616.1).
///
/// Each event is processed in APNAP order of its chooser, then submission order (CR 101.4,
/// CR 616.1); an `Instead` descendant is processed as soon as the event it replaces has
/// resolved, in the same order at that event's position, recursively: the applicable effects
/// are collected against the current state, excluding the event's applied effects (CR 614.5)
/// unless an effect overrides that (CR 903.9b-style); contained effects wait while a
/// non-contained one applies (CR 616.1g); only the first non-empty tier is choosable
/// (CR 616.1a-e); one effect is applied without asking, several are offered through
/// `CHOOSE_REPLACEMENT_EFFECT`; the loop repeats on the modified event (CR 616.1f, CR 616.2).
/// The events of an applied `Instead` are part of the same simultaneous batch as the event they
/// replace, at every generation (CR 614.6, CR 704.3): they keep its applied set (CR 614.5) and
/// are resolved against the state before the batch. With `sba` true, zone changes of the same
/// object to the same destination are one group (CR 704.7): its candidates are the union of its
/// members' effects; each round selects ONE effect (the chooser's pick, or the sole candidate
/// applied without asking) and applies it once to every member it matches; the bus repeats
/// collection until no candidate remains (CR 616.1f). The final events are executed as ONE
/// batch (CR 704.3, CR 704.8: last known information is captured before it): the events that
/// are not zone changes first, then the zone changes, and an object moves at most once: if
/// several final zone changes name it, one is chosen by content (a move produced or redirected
/// by a replacement first). Exception: with `sba` false, if every competing move of an object
/// was submitted directly (no `Instead` descendant among them), nothing is reconciled; all are
/// executed and the second fails with `UnknownObject`, rolling back the call (a caller's
/// duplicate). That choice among one object's moves does not depend on submission order; but
/// the results of different objects or separate groups are resolved in APNAP order of their
/// choosers, then submission order (an `Instead` descendant taking the position of the event
/// it replaces, recursively), and may compete for a one-use effect, so reversing the submission
/// can change which object such an effect applies to. The `Prevent` follow-up events of the whole
/// batch form one child batch after it (no applied effects, CR 615.5), processed the same way.
/// A member stops just before a prevention pool is first applied to it;
/// once every member has stopped or finished, a player whose pool cannot cover the current
/// simultaneous damage from several sources divides it among them, each source's share at
/// most its damage (CR 615.7), and the stopped members continue, those with a share first. An
/// object that a [`Modification::EntersAsCopyOf`] effect makes enter as a copy enters as that
/// copy as part of its own entering event in the executed batch (CR 707.5, CR 614.1c,
/// CR 616.1c): there is no separate `BecomeCopy` event. A shield-less prevention effect queues its
/// follow-up events once per recipient per batch (CR 122.1c, CR 615.13). At most
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
        retired: BTreeSet::new(),
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
    /// The `Shield::Pool` effects whose CR 615.7 allocation step has run for it.
    allocated: BTreeSet<EffectId>,
    /// The `Shield::Pool` effects it no longer takes (its source's allotment was used up).
    declined: BTreeSet<EffectId>,
    /// The `Shield::Pool` effect it stopped just before applying, waiting for its allocation.
    stopped: Option<EffectId>,
    /// CR 704.7: the index of the first member of its same-result group (see [`coalesce`]),
    /// if it is in one (only submitted members are grouped).
    group: Option<usize>,
    /// The submitted member its `Instead` lineage descends from (itself if submitted; the
    /// group's first member if that member is in a CR 704.7 group).
    root: usize,
    /// Its lineage position (see [`schedule_key`]): `[root]` for a submitted member (the group's
    /// first index for a CR 704.7 group member); for an `Instead` descendant, its parent's path
    /// followed by its position in the parent's `Instead` output.
    path: Vec<usize>,
    /// Its nesting: the batch's child depth plus its `Instead` generation (see
    /// [`MAX_CHILD_DEPTH`]).
    depth: usize,
    /// The (object, destination) its root was submitted with, if the root is a zone change;
    /// inherited by the lineage (see [`Bus::reconcile`]).
    submitted_move: Option<(ObjectId, ZoneKind)>,
}

impl Member {
    fn new(
        chooser: PlayerId,
        instance: &EventInstance,
        root: usize,
        depth: usize,
        submitted_move: Option<(ObjectId, ZoneKind)>,
    ) -> Member {
        Member {
            chooser,
            event: Some(instance.event().clone()),
            applied: instance.applied_effects().clone(),
            repeats: 0,
            allocated: BTreeSet::new(),
            declined: BTreeSet::new(),
            stopped: None,
            group: None,
            root,
            path: vec![root],
            depth,
            submitted_move,
        }
    }
}

/// What the members of one batch queued while they were resolved.
#[derive(Default)]
struct Queue {
    /// The `Prevent` follow-up events (CR 615.5), in the order they were queued: one child
    /// batch after the executed batch.
    children: Vec<EventInstance>,
    /// `(parent member, position in the parent's output, event)`: the `Instead` events not yet
    /// taken up as members of the same batch (CR 614.6), each with its parent's applied set
    /// (CR 614.5); see [`Bus::absorb`].
    instead: Vec<(usize, usize, EventInstance)>,
    /// `(member, copied)`: the object member `member` puts onto the battlefield enters as a
    /// copy of `copied` (CR 614.1c, CR 707.5).
    copies: Vec<(usize, ObjectId)>,
    /// `(effect, affected object, affected player)` of the shield-less prevention effects
    /// whose follow-up events this batch already queued (CR 122.1c, CR 615.13).
    prevented_once: BTreeSet<(EffectId, Option<ObjectId>, Option<PlayerId>)>,
}

/// A final event of a resolved batch with its entry-copy plan: the objects it enters as a
/// copy of, in application order (CR 614.1c, CR 707.5; empty for most events).
type Planned = (EventInstance, Vec<ObjectId>);

/// A resolved batch (see [`Bus::resolve_batch`]): nothing of it has been executed yet.
#[derive(Default)]
struct Plan {
    /// The final events of every member, `Instead` lineages included, in execution order: the
    /// events that are not zone changes first, then at most one zone change per object (see
    /// [`reconcile`]; except, with `sba` false, a caller's duplicate of directly submitted
    /// moves, which are all kept).
    events: Vec<Planned>,
    /// The `Prevent` follow-up events of the whole batch (CR 615.5), in the order they were
    /// queued: one child batch that follows the executed batch.
    after: Vec<EventInstance>,
}

struct Bus<'r, 'c, 'h> {
    ctx: &'r mut EventContext<'c>,
    registry: &'r mut ReplacementRegistry,
    choices: &'r mut ChoiceContext<'h>,
    report: BusReport,
    /// Event instances taken up so far in this call (see [`MAX_BUS_EVENTS`]).
    work: usize,
    /// The CR 615.7 allotments of the current allocation of each pool and chooser:
    /// `(pool effect, chooser, source)` to the damage of that source the shield may still
    /// prevent.
    allotments: BTreeMap<(EffectId, PlayerId, ObjectId), i64>,
    /// The old ids of the objects an executed batch of this call moved to another zone
    /// (CR 400.7); see [`Bus::impossible`].
    retired: BTreeSet<ObjectId>,
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

    /// Processes one batch at child depth `depth` (SRS 8.1): resolves it ([`Bus::resolve_batch`]),
    /// executes its final events, `Instead` lineages included, as ONE batch (CR 704.3), then
    /// processes its `Prevent` follow-up events as one child batch. The events were charged
    /// against the work budget when they were submitted or queued.
    fn run(
        &mut self,
        events: Vec<EventInstance>,
        sba: bool,
        depth: usize,
    ) -> Result<(), BusInterrupt> {
        let plan = self.resolve_batch(events, sba, depth)?;
        self.execute(plan.events)?;
        if plan.after.is_empty() {
            return Ok(());
        }
        self.run(plan.after, false, depth + 1)
    }

    /// Resolves one batch at child depth `depth` without executing anything (SRS 8.1 steps
    /// [1]-[5] for every member, CR 615.7 allocations, CR 704.7 groups). Returns the final events
    /// in execution order and the `Prevent` follow-up events that follow the executed batch.
    ///
    /// CR 614.6, CR 704.3: the events of an applied `Instead` are the modified event that occurs
    /// instead of the replaced one, so they are members of THIS batch (an `Instead` lineage), at
    /// any generation, whether the replaced member was in a CR 704.7 group or not. Nothing is
    /// executed while the batch resolves, so every member, lineage included, is judged against
    /// the state before the batch (CR 616.1f, CR 616.2). The loop: phase A repeatedly takes the
    /// not-started member with the least [`schedule_key`] (APNAP rank of its chooser, chooser,
    /// lineage path, member index; CR 101.4, CR 616.1; a CR 704.7 group at its first member), resolves
    /// it and at once takes up the `Instead` events it queued as not-started members, so a
    /// descendant competes by the same key as soon as its parent has resolved (it can't start
    /// earlier; if its chooser ranks before members that already resolved, it simply runs
    /// next). When every member has finished or stopped before a prevention pool, phases B and
    /// C allocate the lowest stopped pool (CR 615.7) and resume its members; the loop ends when
    /// nothing is pending. Then [`reconcile`] keeps at most one zone change per object (except
    /// a caller's duplicate of directly submitted moves with `sba` false, see [`reconcile`]).
    fn resolve_batch(
        &mut self,
        events: Vec<EventInstance>,
        sba: bool,
        depth: usize,
    ) -> Result<Plan, BusInterrupt> {
        if depth > MAX_CHILD_DEPTH {
            return Err(BusError::DepthExceeded.into());
        }
        // CR 101.3, CR 614.6, CR 614.7: an event naming, as its affected object, an `Attach`
        // host or a `BecomeCopy` copied object, an object an earlier batch of this call moved
        // is an instruction that can't be carried out: it is ignored; the rest of the batch is
        // carried out.
        let events: Vec<EventInstance> = events
            .into_iter()
            .filter(|instance| !self.impossible(instance.event()))
            .collect();
        if events.is_empty() {
            return Ok(Plan::default());
        }
        let mut members = Vec::with_capacity(events.len());
        for (i, instance) in events.iter().enumerate() {
            let submitted_move = match instance.event() {
                GameEvent::ZoneChange {
                    object, request, ..
                } => Some((*object, request.to)),
                _ => None,
            };
            members.push(Member::new(
                self.chooser(instance.event())?,
                instance,
                i,
                depth,
                submitted_move,
            ));
        }
        let submitted = members.len();
        let groups = if sba {
            coalesce(&mut members)
        } else {
            BTreeMap::new()
        };
        let mut queue = Queue::default();
        // Whether each member (same index) has been started by phase A.
        let mut started: Vec<bool> = vec![false; members.len()];
        loop {
            // Phase A (CR 101.4, CR 616.1, SRS 8.1 step [2]): the not-started member with the
            // least `schedule_key` runs until it finishes or stops before a prevention pool (a
            // CR 704.7 group as one, at its first member); the `Instead` events it queued are
            // taken up at once as not-started members (CR 614.6), so a descendant competes by
            // the same key as soon as its parent has resolved.
            if let Some(i) = self.next_member(&members, &started) {
                match members[i].group {
                    Some(g) if g == i => {
                        let group = groups.get(&i).map_or(&[][..], Vec::as_slice);
                        for &j in group {
                            started[j] = true;
                        }
                        self.resolve_group(&mut members, group, &mut queue)?;
                    }
                    _ => {
                        started[i] = true;
                        self.resolve(&mut members, i, &mut queue)?;
                    }
                }
                self.absorb(&mut members, &mut queue)?;
                started.resize(members.len(), false);
                continue;
            }
            // Phases B and C (CR 615.7, CR 616.1f, CR 616.2): the lowest pool among stopped
            // members is allocated on the current damage, per chooser in APNAP order; its
            // stopped members then take it, in `schedule_key` order (allotted members first),
            // and run on until they finish or stop again.
            let Some(pool) = members.iter().filter_map(|m| m.stopped).min() else {
                break;
            };
            let choosers: Vec<PlayerId> = members.iter().map(|m| m.chooser).collect();
            let order = self.apnap_order(&choosers);
            let mut pool_choosers: Vec<PlayerId> = Vec::new();
            for &i in &order {
                let m = &members[i];
                if m.stopped == Some(pool) && !pool_choosers.contains(&m.chooser) {
                    pool_choosers.push(m.chooser);
                }
            }
            let players = self.player_order();
            for chooser in pool_choosers {
                // CR 101.4, CR 616.1: the chooser's stopped members in `schedule_key` order
                // (lineage path), the order phase A stopped them in, so a descendant resumes at
                // the position of the event it replaces.
                let mut group: Vec<usize> = (0..members.len())
                    .filter(|&i| members[i].stopped == Some(pool) && members[i].chooser == chooser)
                    .collect();
                group.sort_by(|&i, &j| {
                    schedule_key(&players, &members[i], i).cmp(&schedule_key(
                        &players,
                        &members[j],
                        j,
                    ))
                });
                self.allocate(pool, chooser, &mut members, &group)?;
                // CR 615.7: members drawing on an allotment take the pool before members
                // outside the division (sourceless damage), so the division is honoured; the
                // sort is stable (`schedule_key` order among equals).
                group.sort_by_key(|&i| {
                    members[i]
                        .event
                        .as_ref()
                        .is_none_or(|ev| self.allotment(pool, &members[i], ev).is_none())
                });
                for &i in &group {
                    self.take_pool(&mut members, i, pool, &mut queue)?;
                    self.resolve(&mut members, i, &mut queue)?;
                }
            }
            // The members taken up here are scheduled by phase A on the next round.
            self.absorb(&mut members, &mut queue)?;
            started.resize(members.len(), false);
        }
        let events = reconcile(&members, submitted, sba, &queue.copies);
        Ok(Plan {
            events,
            after: queue.children,
        })
    }

    /// CR 101.3, CR 614.6, CR 614.7, CR 400.7: whether `event` names, in a role that needs the
    /// object as it is now, the old id of an object an executed batch of THIS call moved to
    /// another zone. The roles ([`required_objects`]): the affected object of every event, the
    /// host of an `Attach` to a permanent (CR 701.3b), and the copied object of a `BecomeCopy`
    /// (an engine limitation: the events layer can't copy last known information there,
    /// CR 608.2h). A `Damage` source is not such a role (CR 608.2h: the object as it most
    /// recently existed deals the damage). Such an event (a `Prevent` follow-up or its
    /// `Instead` lineage, in a later batch) is an instruction that can't be carried out and is
    /// ignored: not offered to effects, not executed, no error; the other events of its batch
    /// are still carried out (CR 101.3). An id that was never issued, or that left in an
    /// earlier call, is not covered: it stays an error.
    fn impossible(&self, event: &GameEvent) -> bool {
        required_objects(event)
            .iter()
            .any(|o| self.retired.contains(o))
    }

    /// Phase A of [`Bus::resolve_batch`]: the not-started member with the least
    /// [`Bus::schedule_key`], or `None` if every member has been started. A member of a CR 704.7
    /// group other than its first is never taken on its own (it starts with the group).
    fn next_member(&self, members: &[Member], started: &[bool]) -> Option<usize> {
        let players = self.player_order();
        members
            .iter()
            .enumerate()
            .filter(|&(i, m)| !started[i] && m.group.is_none_or(|g| g == i))
            .min_by_key(|&(i, m)| schedule_key(&players, m, i))
            .map(|(i, _)| i)
    }

    /// CR 614.6, CR 704.3: takes up the queued `Instead` events as new members of the batch,
    /// each inheriting its parent's root and submitted move, one generation deeper, its path
    /// the parent's path followed by its position in the parent's output; called
    /// right after each resolve, so the new members are scheduled by phase A at once. A member
    /// deeper than [`MAX_CHILD_DEPTH`] is [`BusError::DepthExceeded`]. An event that names a
    /// retired object in a required role is ignored (see [`Bus::impossible`]). The events were
    /// charged against the work budget when they were queued.
    fn absorb(&mut self, members: &mut Vec<Member>, queue: &mut Queue) -> Result<(), BusError> {
        for (parent, position, instance) in std::mem::take(&mut queue.instead) {
            let (root, depth, submitted_move, mut path) = {
                let p = &members[parent];
                (p.root, p.depth + 1, p.submitted_move, p.path.clone())
            };
            path.push(position);
            if depth > MAX_CHILD_DEPTH {
                return Err(BusError::DepthExceeded);
            }
            if self.impossible(instance.event()) {
                continue;
            }
            let mut member = Member::new(
                self.chooser(instance.event())?,
                &instance,
                root,
                depth,
                submitted_move,
            );
            member.path = path;
            members.push(member);
        }
        Ok(())
    }

    /// Executes the final events of a resolved batch as one batch in order (SRS 8.1: execute
    /// E; CR 704.3, CR 704.8: the events layer captures last known information for every object
    /// the batch moves before any event of the batch). CR 707.5, CR 614.1c, CR 616.1c: an object
    /// that enters as a copy (its event's entry-copy plan) is a copy as part of its own entering
    /// event, in this one executed batch, through
    /// [`EventContext::execute_batch_with_entry_copies`]: there is no separate `BecomeCopy`
    /// event and the copy is not offered to replacement effects (distinct from the CR 614.12
    /// look-ahead, a later unit).
    fn execute(&mut self, planned: Vec<Planned>) -> Result<(), BusError> {
        if planned.is_empty() {
            return Ok(());
        }
        let mut entry_copies: Vec<(usize, ObjectId)> = Vec::new();
        let mut events: Vec<EventInstance> = Vec::with_capacity(planned.len());
        for (position, (instance, plan)) in planned.into_iter().enumerate() {
            entry_copies.extend(plan.into_iter().map(|copied| (position, copied)));
            events.push(instance);
        }
        let executed = self
            .ctx
            .execute_batch_with_entry_copies(&EventBatch::new(events), &entry_copies)?;
        // CR 400.7: a moved object is a new object; its old id is retired for later batches.
        for ev in &executed.events {
            if let crate::events::Outcome::ZoneChange(m) = &ev.outcome
                && m.old != m.new
            {
                self.retired.insert(m.old);
            }
        }
        if !executed.events.is_empty() {
            self.report.executed.push(executed);
        }
        Ok(())
    }

    /// The chooser of `event` (CR 616.1): the controller of the affected object (CR 112.2,
    /// CR 109.4: a permanent's or a stack object's controller), its owner if it has no
    /// controller (CR 108.4a), else the affected player; for a token creation the controller it
    /// would enter under; with neither, the active player. A permanent's controller is the
    /// layer-evaluated one (CR 613.1b) from the context's provider, falling back to the stored
    /// controller when the provider names none.
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
                // CR 613.1b, CR 616.1: the controller after layer 2, else the stored one.
                return Ok(self
                    .ctx
                    .provider
                    .controller(object)
                    .unwrap_or_else(|| permanent.controller()));
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
    /// stun counter replaces only a real untap). CR 701.26a: only untapped permanents can be
    /// tapped. Such an event offers no effect and reaches the executor unchanged (a no-op).
    fn would_happen(&self, event: &GameEvent) -> bool {
        match event {
            GameEvent::Damage { amount, .. } => *amount > 0,
            // CR 701.26a
            GameEvent::Tap { object } => self
                .ctx
                .permanents
                .get(*object)
                .is_some_and(|p| !p.status().tapped),
            // CR 701.26b
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
        restrict(self.eligible(event, member))
    }

    /// SRS 8.1 step [1]: the effects applicable to `member`'s current `event`, ascending id,
    /// before the CR 616.1g and tier restrictions.
    fn eligible(&self, event: &GameEvent, member: &Member) -> Vec<&ReplacementEffect> {
        // CR 614.7, CR 614.7a: an event that would not happen offers no effect.
        if !self.would_happen(event) {
            return Vec::new();
        }
        // [1] CR 614.5: not an effect already applied, unless it overrides 614.5 (CR 903.9b).
        self.registry
            .iter()
            .filter(|e| e.overrides_614_5 || !member.applied.contains(e.id))
            .filter(|e| match e.shield {
                Some(Shield::Uses(n)) => n > 0,
                Some(Shield::Pool(n)) => {
                    n > 0
                        && !member.declined.contains(&e.id)
                        // CR 615.7: an allocated pool whose allotment for this member's source
                        // is used up is not offered again (an overriding pool would loop).
                        && !(member.allocated.contains(&e.id)
                            && self.allotment(e.id, member, event) == Some(0))
                }
                None => true,
            })
            .filter(|e| self.matches(&e.matcher, event))
            .collect()
    }

    /// CR 616.1, CR 704.3, CR 704.7: SRS 8.1 steps [1]-[5] for a same-result group of
    /// state-based zone changes of one object (`group`, ascending index). The candidates are
    /// the union of the effects eligible for at least one member (CR 616.1g and the tier
    /// restriction run on the union); the object's chooser picks one; it is applied once
    /// (one application, one shield use, one set of child events) to every member it is
    /// eligible for, and the members it does not match keep resolving. Repeats until no
    /// candidate remains (CR 616.1f, CR 616.2).
    fn resolve_group(
        &mut self,
        members: &mut [Member],
        group: &[usize],
        queue: &mut Queue,
    ) -> Result<(), BusInterrupt> {
        loop {
            let mut ids: BTreeSet<EffectId> = BTreeSet::new();
            let mut event: Option<GameEvent> = None;
            for &i in group {
                let Some(ev) = &members[i].event else {
                    continue;
                };
                if event.is_none() {
                    event = Some(ev.clone());
                }
                ids.extend(self.eligible(ev, &members[i]).iter().map(|e| e.id));
            }
            let Some(event) = event else {
                return Ok(());
            };
            let set = restrict(ids.iter().filter_map(|&id| self.registry.get(id)).collect());
            if set.is_empty() {
                // SRS 8.1: R(E) empty -> execute E.
                return Ok(());
            }
            let chosen = self.choose(&event, &set)?;
            let effect = self.effect(chosen)?;
            // The members the effect applies to, in content order (order independence).
            let mut targets: Vec<usize> = group
                .iter()
                .copied()
                .filter(|&i| {
                    members[i].event.as_ref().is_some_and(|ev| {
                        self.eligible(ev, &members[i])
                            .iter()
                            .any(|e| e.id == chosen)
                    })
                })
                .collect();
            targets.sort_by_cached_key(|&i| (content_key(&members[i]), i));
            let mut pool = pool_of(&effect);
            for (n, &i) in targets.iter().enumerate() {
                // CR 704.7: one application; its child events are queued once.
                self.apply(&mut members[i], i, &effect, &mut pool, queue, n == 0)?;
            }
            self.consume(&effect, pool);
            self.report.applications.push(chosen);
        }
    }

    /// CR 615.7: what is left of the allotment `member`'s chooser gave `event`'s source from
    /// pool `pool`, if the pool's current allocation for that chooser divided it.
    fn allotment(&self, pool: EffectId, member: &Member, event: &GameEvent) -> Option<i64> {
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

    /// CR 615.7, CR 616.1f, CR 616.2: the allocation of pool `pool` for `chooser`'s members
    /// `group`, all stopped just before taking it, on their CURRENT damage (after every
    /// modification so far). The eligible members are the group's damage events with a
    /// source and an amount above 0 that the pool matches now; if they come from two or more
    /// sources and their total exceeds what is left of the pool, the chooser divides the pool
    /// among the sources (`DIVIDE_VALUE`), no source getting more than its damage. Otherwise
    /// nothing is asked and the pool applies in order.
    fn allocate(
        &mut self,
        pool: EffectId,
        chooser: PlayerId,
        members: &mut [Member],
        group: &[usize],
    ) -> Result<(), BusInterrupt> {
        self.allotments
            .retain(|&(p, c, _), _| (p, c) != (pool, chooser));
        for &i in group {
            members[i].allocated.insert(pool);
        }
        let Some(effect) = self.registry.get(pool) else {
            return Ok(());
        };
        let Some(Shield::Pool(remaining)) = effect.shield else {
            return Ok(());
        };
        let matcher = effect.matcher.clone();
        let mut caps: BTreeMap<ObjectId, i64> = BTreeMap::new();
        let mut total: i64 = 0;
        for &i in group {
            let Some(
                event @ GameEvent::Damage {
                    source: Some(source),
                    amount,
                    ..
                },
            ) = &members[i].event
            else {
                continue;
            };
            if *amount <= 0 || !self.matches(&matcher, event) {
                continue;
            }
            let entry = caps.entry(*source).or_insert(0);
            *entry = entry.saturating_add(*amount);
            total = total.saturating_add(*amount);
        }
        if caps.len() < 2 || total <= remaining {
            return Ok(());
        }
        let shares = self.ask_division(chooser, remaining, &caps)?;
        for (source, share) in shares {
            self.allotments.insert((pool, chooser, source), share);
        }
        Ok(())
    }

    /// Phase C for member `i`, stopped before pool `pool`: applies it (CR 615.7: up to the
    /// source's allotment), or, if the source's allotment is used up or the pool is gone,
    /// declines it so the member continues without it.
    fn take_pool(
        &mut self,
        members: &mut [Member],
        i: usize,
        pool: EffectId,
        queue: &mut Queue,
    ) -> Result<(), BusInterrupt> {
        members[i].stopped = None;
        let Some(event) = members[i].event.clone() else {
            return Ok(());
        };
        let effect = match self.registry.get(pool) {
            Some(e) if matches!(e.shield, Some(Shield::Pool(n)) if n > 0) => e.clone(),
            _ => {
                members[i].declined.insert(pool);
                return Ok(());
            }
        };
        if self.allotment(pool, &members[i], &event) == Some(0) {
            members[i].declined.insert(pool);
            return Ok(());
        }
        self.apply_and_record(&mut members[i], i, &effect, queue)?;
        Ok(())
    }

    /// CR 615.7: asks `chooser` to divide `pool` among `sources` (ascending id, each with its
    /// cap: the source's current eligible damage). The bus rejects a share above its source's
    /// cap (the request's payload carries only minimums); the timeout default fills the
    /// sources in ascending id, each up to its cap, until the pool is used.
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
        let mut left = total;
        let default: Vec<(ChoiceTarget, u64)> = sources
            .iter()
            .map(|(&s, &cap)| {
                let share = left.min(u64::try_from(cap).unwrap_or(0));
                left -= share;
                (ChoiceTarget::Object(s), share)
            })
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
                        let share = i64::try_from(amount).unwrap_or(i64::MAX);
                        let cap = sources.get(&s).copied().unwrap_or(0);
                        if share > cap {
                            // CR 615.7: a source's share can't exceed the damage it deals.
                            return Err(BusInterrupt::Error(BusError::Choice(
                                ChoiceError::Procedure(format!(
                                    "share {share} for {target} exceeds its damage {cap}"
                                )),
                            )));
                        }
                        Ok((s, share))
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

    /// SRS 8.1 steps [1]-[5] for one member until no effect applies (CR 616.1f), or until it
    /// stops just before taking a prevention pool not yet allocated for it (CR 615.7; the
    /// member keeps its state and resumes in [`Bus::take_pool`]).
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
            if matches!(effect.shield, Some(Shield::Pool(_)))
                && !members[i].allocated.contains(&chosen)
            {
                // CR 615.7: wait for the allocation on the batch's current damage.
                members[i].stopped = Some(chosen);
                return Ok(());
            }
            self.apply_and_record(&mut members[i], i, &effect, queue)?;
        }
    }

    /// Applies `effect` to `member`, consumes its shield and records the application.
    fn apply_and_record(
        &mut self,
        member: &mut Member,
        index: usize,
        effect: &ReplacementEffect,
        queue: &mut Queue,
    ) -> Result<(), BusError> {
        let mut pool = pool_of(effect);
        self.apply(member, index, effect, &mut pool, queue, true)?;
        self.consume(effect, pool);
        self.report.applications.push(effect.id);
        Ok(())
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
    /// `children` false: the application's child events were already queued for another
    /// member of the same CR 704.7 group.
    fn apply(
        &mut self,
        member: &mut Member,
        index: usize,
        effect: &ReplacementEffect,
        pool: &mut Option<i64>,
        queue: &mut Queue,
        children: bool,
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
        let recipient = (effect.id, affected_object(event), affected_player(event));
        match &effect.action {
            // CR 614.1c, CR 616.1c, CR 707.5: the copy is part of the entry (see `execute`).
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
                if !children {
                    return Ok(());
                }
                self.charge(events.len())?;
                // CR 614.6, CR 704.3: they join this batch (see `absorb`).
                queue
                    .instead
                    .extend(events.iter().cloned().enumerate().map(|(position, e)| {
                        (
                            index,
                            position,
                            EventInstance::new(e).with_applied(member.applied.clone()),
                        )
                    }));
            }
            Action::Prevent { then } => {
                // CR 615.6: prevented damage never happens. CR 615.7: a pool prevents up to
                // what is left of it (and of the chooser's allotment for the source).
                if let GameEvent::Damage { source, amount, .. } = event {
                    let available = (*amount).max(0);
                    let allotment = match source {
                        Some(s) => self.allotments.get_mut(&(effect.id, member.chooser, *s)),
                        None => None,
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
                // CR 122.1c, CR 615.13: a shield-less prevention effect is applied once to all
                // simultaneous damage to one recipient, so its follow-up is queued once per
                // batch (see DECISIONS); `Uses` and `Pool` shields count their own uses.
                if children && (effect.shield.is_some() || queue.prevented_once.insert(recipient)) {
                    // CR 615.5: the follow-up is a new event, with no applied effects.
                    self.charge(then.len())?;
                    queue
                        .children
                        .extend(then.iter().cloned().map(EventInstance::new));
                }
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

/// CR 101.4, CR 616.1, SRS 8.1 step [2]: the phase A order of member `index` (`m`) among the
/// not-started members of a batch, given the players in APNAP order: (APNAP rank of its chooser,
/// chooser, lineage path compared lexicographically, member index). The rank is the chooser's
/// position in `players`; a chooser outside it ranks last, by ascending id. The path of an
/// `Instead` descendant extends its parent's path by its position in the parent's output, so a
/// descendant takes the exact position of the event it replaces, recursively (a grandchild
/// sorts at its parent's place among its parent's siblings). The member index only breaks the
/// tie of a CR 704.7 group's members, which share a path. For submitted members (path = [own
/// index], or [the group's first index]) this is the order of [`Bus::apnap_order`].
fn schedule_key<'m>(
    players: &[PlayerId],
    m: &'m Member,
    index: usize,
) -> (usize, PlayerId, &'m [usize], usize) {
    let rank = players
        .iter()
        .position(|&q| q == m.chooser)
        .unwrap_or(players.len());
    (rank, m.chooser, m.path.as_slice(), index)
}

/// CR 704.3, CR 704.7: state-based actions that would have the same result happen at the same
/// time as one event, and a single replacement effect replaces all of them. The zone changes
/// of one object to one destination are one group: every member is tagged with the index of
/// the group's first member (the group's position in submission and APNAP order), which also
/// becomes its root, and stays in the list, so collection sees every member's cause (see
/// [`Bus::resolve_group`]). Returns the members of each group of two or more by its first
/// index, ascending. Different objects never merge.
fn coalesce(members: &mut [Member]) -> BTreeMap<usize, Vec<usize>> {
    let mut by_result: BTreeMap<(ObjectId, ZoneKind), Vec<usize>> = BTreeMap::new();
    for (i, m) in members.iter().enumerate() {
        if let Some(GameEvent::ZoneChange {
            object, request, ..
        }) = &m.event
        {
            by_result.entry((*object, request.to)).or_default().push(i);
        }
    }
    let mut groups = BTreeMap::new();
    for group in by_result.into_values() {
        if group.len() < 2 {
            continue;
        }
        for &i in &group {
            members[i].group = Some(group[0]);
            members[i].root = group[0];
            members[i].path = vec![group[0]];
        }
        groups.insert(group[0], group);
    }
    groups
}

/// CR 704.3, CR 704.7, CR 614.6, CR 614.7, CR 400.7: the final events of a resolved batch
/// (`members`, of which the first `submitted` were submitted), in execution order, each with
/// its entry-copy plan from `copies`. The object of a zone change moves at most once in the
/// batch, except for the `sba` false caller's duplicate in the first item below (see DECISIONS
/// "Two destinations for one object"):
///
/// - The final zone changes of every member, `Instead` lineages included, are the candidates
///   of their object. One candidate is executed as is. With `sba` false and only submitted
///   candidates, every candidate is kept (a caller's duplicate fails in the events layer).
/// - Otherwise a candidate is a redirect if its root was not submitted as a move of this
///   object, or its FINAL destination differs from its root's submitted destination. If a
///   redirect exists, only redirects compete; otherwise all do. The winner is the least by
///   [`planned_key`] (canonical event bytes, canonical applied-set bytes, entry-copy plan:
///   one with a plan before one without, then the lesser plan); the candidate's position
///   breaks a remaining tie, where the candidates are interchangeable. The losers and their
///   plans are dropped; this never fails. The choice among one object's candidates depends
///   on their content only, not on submission order.
///
/// Order: the events that are not zone changes first, by (lineage path, member index), so a
/// descendant sits at the position of the event it replaces (CR 614.6), and so that they
/// name live objects (a counter removal before its object's move); then the kept zone
/// changes, each at the least (lineage path, member index) of its object's candidates. The events
/// layer captures last known information for every moved object before the batch (CR 704.8).
fn reconcile(
    members: &[Member],
    submitted: usize,
    sba: bool,
    copies: &[(usize, ObjectId)],
) -> Vec<Planned> {
    let planned = |i: usize| -> Option<Planned> {
        let m = &members[i];
        let e = m.event.as_ref()?;
        let plan = copies
            .iter()
            .filter(|(member, _)| *member == i)
            .map(|&(_, copied)| copied)
            .collect();
        Some((
            EventInstance::new(e.clone()).with_applied(m.applied.clone()),
            plan,
        ))
    };
    // CR 614.6: a descendant executes at the position of the event it replaces (lineage path;
    // `[root]` for a submitted member), the member index separating a group's members.
    let slot = |i: usize| (members[i].path.clone(), i);
    let mut others: Vec<((Vec<usize>, usize), Planned)> = Vec::new();
    let mut moves: BTreeMap<ObjectId, Vec<usize>> = BTreeMap::new();
    for (i, m) in members.iter().enumerate() {
        match &m.event {
            Some(GameEvent::ZoneChange { object, .. }) => moves.entry(*object).or_default().push(i),
            Some(_) => others.extend(planned(i).map(|p| (slot(i), p))),
            None => {}
        }
    }
    let mut kept: Vec<((Vec<usize>, usize), Planned)> = Vec::new();
    for (object, candidates) in moves {
        if candidates.len() == 1 || (!sba && candidates.iter().all(|&i| i < submitted)) {
            kept.extend(
                candidates
                    .into_iter()
                    .filter_map(|i| planned(i).map(|p| (slot(i), p))),
            );
            continue;
        }
        let first = candidates.iter().map(|&i| slot(i)).min();
        let redirect = |i: usize| match (&members[i].event, members[i].submitted_move) {
            (Some(GameEvent::ZoneChange { request, .. }), Some((o, to))) if o == object => {
                request.to != to
            }
            _ => true,
        };
        let any_redirect = candidates.iter().any(|&i| redirect(i));
        let winner = candidates
            .into_iter()
            .filter(|&i| !any_redirect || redirect(i))
            .filter_map(|i| planned(i).map(|p| (i, p)))
            .min_by_key(|(i, p)| (planned_key(p), *i))
            .map(|(_, p)| p);
        if let (Some(first), Some(winner)) = (first, winner) {
            kept.push((first, winner));
        }
    }
    others.sort_by(|(a, _), (b, _)| a.cmp(b));
    kept.sort_by(|(a, _), (b, _)| a.cmp(b));
    others.into_iter().chain(kept).map(|(_, p)| p).collect()
}

/// The order of one object's competing zone changes (see [`reconcile`]).
fn planned_key(planned: &Planned) -> (Vec<u8>, Vec<u8>, bool, Vec<ObjectId>) {
    let (instance, plan) = planned;
    let mut event = Vec::new();
    let mut applied = Vec::new();
    instance.event().encode_canonical(&mut event);
    instance.applied_effects().encode_canonical(&mut applied);
    (event, applied, plan.is_empty(), plan.clone())
}

/// The (canonical event bytes, canonical applied-set bytes) of `member`, the content order of
/// CR 704.7 group members.
fn content_key(member: &Member) -> (Vec<u8>, Vec<u8>) {
    let mut event = Vec::new();
    let mut applied = Vec::new();
    if let Some(e) = &member.event {
        e.encode_canonical(&mut event);
    }
    member.applied.encode_canonical(&mut applied);
    (event, applied)
}

/// SRS 8.1 step [3] on the step [1] `candidates`: CR 616.1g, then the first non-empty tier
/// (CR 616.1a-e); ascending id.
fn restrict(mut candidates: Vec<&ReplacementEffect>) -> Vec<EffectId> {
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

#[cfg(test)]
mod evaluated_chooser_tests {
    // Unit tests of the replacement chooser with the layer-evaluated controller: REQ-REPL-*
    // (CR 616.1, CR 108.4a), REQ-LAYER-001 (CR 613.1b), REQ-EVENT-001 (the bus). Synthetic ids
    // and synthetic characteristics only.

    use std::collections::BTreeSet;

    use fineprint_base::encode::encode_to_vec;
    use fineprint_base::hash::{Digest, leaf_digest};
    use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
    use fineprint_dsl::ability::Layer;
    use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
    use fineprint_dsl::expr::ObjectSelectorAST;

    use super::*;
    use crate::choice::{ChoicePayload, ChoiceResponse, ChoiceSession, Interrupt, Status};
    use crate::events::{EventBatch, EventContext, EventState, GameEvent, ZoneChangeCause};
    use crate::layers::{
        Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment,
        LayerModification, LayeredProvider, MatchContext, ObjectBase,
    };
    use crate::memory::{LkiStore, MemoryTable};
    use crate::objects::Characteristics;
    use crate::permanent::Permanents;
    use crate::replacement::{Action, BusEventKind, EventMatcher, ReplacementEffect};
    use crate::state::GameState;
    use crate::txn::Transactional;
    use crate::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};

    fn p(n: u64) -> PlayerId {
        PlayerId::new(n)
    }

    fn chars() -> Characteristics {
        Characteristics {
            names: BTreeSet::from(["test.vanilla_2_2".to_owned()]),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::from(["Creature".to_owned()]),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream(Vec::new()),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        }
    }

    /// The layer environment of the world's zones: every object with its zone record's
    /// rules-level controller as its base controller.
    struct Env<'a> {
        zones: &'a Zones,
    }

    impl LayerEnvironment for Env<'_> {
        fn objects(&self) -> Vec<ObjectId> {
            let mut all: Vec<ObjectId> = self.zones.all_object_ids();
            all.sort_unstable();
            all
        }

        fn base(&self, id: ObjectId) -> Option<ObjectBase> {
            self.zones.object(id).map(|r| ObjectBase {
                characteristics: chars(),
                controller: r.rules_controller(),
                on_battlefield: r.zone.kind() == ZoneKind::Battlefield,
            })
        }

        fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
            false
        }
    }

    #[derive(Clone, Debug)]
    struct World {
        game: GameState,
        zones: Zones,
        ids: IdGenerator<ObjectId>,
        permanents: Permanents,
        memories: MemoryTable,
        lki: LkiStore,
        events: EventState,
        registry: ReplacementRegistry,
        layer_effects: ContinuousEffects,
    }

    impl World {
        /// Two players, turn 1, active player 1, two competing `Skip` effects on `kind`.
        fn new(kind: BusEventKind) -> World {
            let mut game = GameState::new(7, 2, 20).unwrap();
            game.set_turn_number(1);
            game.set_active_player(Some(p(1))).unwrap();
            let mut registry = ReplacementRegistry::new();
            for id in [1, 2] {
                registry
                    .add(ReplacementEffect::new(
                        EffectId::new(id),
                        EventMatcher::new(kind),
                        Action::Skip,
                    ))
                    .unwrap();
            }
            World {
                game,
                zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
                ids: IdGenerator::new(),
                permanents: Permanents::new(),
                memories: MemoryTable::new(),
                lki: LkiStore::new(),
                events: EventState::new(),
                registry,
                layer_effects: ContinuousEffects::new(),
            }
        }

        /// A new object owned by `owner` in a zone of `kind`; on the battlefield it is a
        /// permanent stored as controlled by `stored`.
        fn create(&mut self, card: u64, owner: u64, stored: u64, kind: ZoneKind) -> ObjectId {
            let mut req = MoveRequest::new(kind);
            if kind == ZoneKind::Battlefield {
                req.controller = Some(p(stored));
            }
            let id = self
                .zones
                .create_object(&mut self.ids, CardId::new(card), p(owner), req)
                .unwrap();
            if kind == ZoneKind::Battlefield {
                self.permanents.enter(id, p(stored), 1).unwrap();
            }
            id
        }

        /// A resolved layer-2 control effect giving `object` to `to` (CR 613.1b).
        fn give_control(&mut self, effect: u64, object: ObjectId, to: u64) {
            self.layer_effects
                .register(
                    EffectId::new(effect),
                    ContinuousEffect {
                        source: None,
                        controller: p(to),
                        timestamp: Timestamp::new(effect),
                        layer: Layer::L2,
                        is_cda: false,
                        origin: EffectOrigin::Resolved,
                        affected: Affected::Objects([object].into_iter().collect()),
                        modification: LayerModification::SetController(p(to)),
                    },
                )
                .unwrap();
        }
    }

    impl Transactional for World {
        fn state_hash(&self) -> Digest {
            let mut bytes = encode_to_vec(&self.game.state_hash());
            bytes.extend(encode_to_vec(&self.zones));
            bytes.extend(encode_to_vec(&self.permanents));
            bytes.extend(encode_to_vec(&self.memories));
            bytes.extend(encode_to_vec(&self.lki));
            bytes.extend(encode_to_vec(&self.events));
            bytes.extend(encode_to_vec(&self.registry));
            leaf_digest(&bytes)
        }

        fn priority_player(&self) -> Option<PlayerId> {
            None
        }

        fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
            restored
        }
    }

    /// Runs `event` through the bus with the layer-evaluated provider and returns the first
    /// replacement request's chooser, its effects, and the final state hash (answering index 0).
    fn chooser_of(w: &World, event: GameEvent) -> (PlayerId, Vec<String>, Digest) {
        let batch = EventBatch::from_events(vec![event]);
        let procedure = move |s: &mut World, ch: &mut crate::choice::ChoiceContext<'_>| {
            let World {
                game,
                zones,
                ids,
                permanents,
                memories,
                lki,
                events,
                registry,
                layer_effects,
            } = s;
            let snapshot = zones.clone();
            let env = Env { zones: &snapshot };
            let provider = LayeredProvider::new(layer_effects, &env);
            let mut ctx = EventContext {
                game,
                zones,
                ids,
                permanents,
                memories,
                lki,
                events,
                provider: &provider,
            };
            match process_batch(&mut ctx, registry, &batch, false, ch) {
                Ok(r) => Ok(Ok(r)),
                Err(BusInterrupt::Pending(r)) => Err(Interrupt::Pending(r)),
                Err(BusInterrupt::Error(e)) => Ok(Err(e)),
            }
        };
        let mut session = ChoiceSession::start(w.clone(), procedure).unwrap();
        let Status::Pending(request) = session.status() else {
            panic!("no replacement choice was asked");
        };
        let request = request.clone();
        let ChoicePayload::ChooseReplacementEffect { effects, .. } = &request.payload else {
            panic!("not a replacement request");
        };
        assert_eq!(request.reason.cr, "CR 616.1");
        session
            .submit(
                request.request_id,
                ChoiceResponse::SelectReplacement { index: 0 },
            )
            .unwrap();
        let Status::Done { output, .. } = session.status() else {
            panic!("more than one request");
        };
        assert!(output.is_ok(), "bus failed: {output:?}");
        (request.chooser, effects.clone(), session.state_hash())
    }

    fn tap(object: ObjectId) -> GameEvent {
        GameEvent::Tap { object }
    }

    /// REQ-REPL-*, REQ-LAYER-001, REQ-EVENT-001; CR 616.1, CR 613.1b: a creature stored as
    /// controlled by P1 under a layer-2 effect giving it to P2 sends the choice to P2.
    #[test]
    fn req_repl_cr_616_1_chooser_is_evaluated_controller() {
        let mut w = World::new(BusEventKind::Tap);
        let x = w.create(1, 1, 1, ZoneKind::Battlefield);
        w.give_control(1, x, 2);
        let (chooser, effects, _) = chooser_of(&w, tap(x));
        assert_eq!(chooser, p(2));
        assert_eq!(effects.len(), 2);
    }

    /// REQ-REPL-*, REQ-EVENT-001; CR 616.1: with no layer-2 effect the stored controller P1
    /// chooses.
    #[test]
    fn req_repl_cr_616_1_chooser_without_control_effect() {
        let mut w = World::new(BusEventKind::Tap);
        let x = w.create(1, 1, 1, ZoneKind::Battlefield);
        assert_eq!(chooser_of(&w, tap(x)).0, p(1));
    }

    /// REQ-REPL-*, REQ-LAYER-001; CR 616.1, CR 613.1b: stored controller P2, evaluated P1
    /// (owned by P2): P1 chooses.
    #[test]
    fn req_repl_cr_616_1_chooser_evaluated_back_to_p1() {
        let mut w = World::new(BusEventKind::Tap);
        let x = w.create(1, 2, 2, ZoneKind::Battlefield);
        w.give_control(1, x, 1);
        assert_eq!(chooser_of(&w, tap(x)).0, p(1));
    }

    /// REQ-REPL-*, REQ-EVENT-001; CR 616.1, CR 108.4a: a card in a graveyard has no
    /// controller, so its owner chooses, even if a layer-2 effect names another player.
    #[test]
    fn req_repl_cr_108_4a_non_permanent_owner_chooses() {
        let mut w = World::new(BusEventKind::ZoneChange);
        let card = w.create(1, 2, 2, ZoneKind::Graveyard);
        w.give_control(1, card, 1);
        let event = GameEvent::ZoneChange {
            object: card,
            request: MoveRequest::new(ZoneKind::Exile),
            cause: ZoneChangeCause::Other,
        };
        assert_eq!(chooser_of(&w, event).0, p(2));
    }

    /// REQ-REPL-*, REQ-EVENT-001; R3 (determinism): repeated runs from the same state ask the
    /// same player for the same choice and end in the same state.
    #[test]
    fn req_repl_cr_616_1_chooser_is_deterministic() {
        let mut w = World::new(BusEventKind::Tap);
        let x = w.create(1, 1, 1, ZoneKind::Battlefield);
        w.give_control(1, x, 2);
        let first = chooser_of(&w, tap(x));
        for _ in 0..4 {
            assert_eq!(chooser_of(&w, tap(x)), first);
        }
    }
}
