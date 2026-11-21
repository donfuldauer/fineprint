// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! One state-based action pass (REQ-SBA-001, CR 704.3): check every entry against one
//! snapshot, then perform every result simultaneously as one event.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};

use super::registry::{
    LegendGroup, SbaAction, SbaRegistry, SbaView, counter_removal, legend_groups,
};
use super::types::{LOST_THE_GAME, SbaEnvironment, SbaFinding, SbaOutcome, SbaSubject};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceType, PendingChoiceRequest,
};
use crate::events::{
    CounterTarget, EventBatch, EventContext, EventError, EventState, ExecutedBatch, GameEvent,
    ObjectMarkers, Outcome, ZoneChangeCause,
};
use crate::layers::{ContinuousEffects, LayerEnvironment};
use crate::memory::{
    CharacteristicsProvider, LkiStore, MemoryError, MemoryTable, capture_lki_on_move,
};
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, Permanents};
use crate::replacement::{BusError, BusInterrupt, ReplacementRegistry, process_batch};
use crate::state::{DesignationValue, GameState};
use crate::zones::{MoveRequest, ZoneId, ZoneKind, Zones};

/// Checks every active entry of `registry` against one snapshot of the state (REQ-SBA-001,
/// CR 704.3) and returns every applicable state-based action, without performing any.
///
/// Pure: reads `ctx` (players, zones, object markers, the empty-library-draw flag), the
/// evaluated characteristics (`effects` through `layer_env`, CR 613.1, CR 613.4) and `env`,
/// and changes nothing. Entries inactive in `env.variant()` are skipped. The findings are in
/// registry order, then subject order; a creature to which both CR 704.5g and CR 704.5h apply
/// has two findings. Battlefield objects that the layer environment cannot evaluate (for
/// example [`LayerError::UnknownObject`](crate::layers::LayerError::UnknownObject)) are not
/// checked by the object entries CR 704.5f-704.5s, and phased-out permanents are skipped by
/// them as subjects, as attachment targets and as legend-rule group members (CR 702.26b).
///
/// The legend rule (CR 704.5j) needs a choice, which a pure check cannot ask: every member of
/// each group of two or more legendary permanents of one (evaluated) controller connected by
/// shared names (CR 201.2a) is a finding; [`perform_all`] asks the controller which stay.
#[must_use]
pub fn check(
    registry: &SbaRegistry,
    ctx: &EventContext<'_>,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &dyn SbaEnvironment,
) -> Vec<SbaFinding> {
    let view = SbaView::new(
        ctx.game,
        ctx.zones,
        ctx.events,
        ctx.permanents,
        effects,
        layer_env,
        env,
    );
    check_view(registry, &view, env)
}

/// Every finding of the active entries of `registry` in `view`, in registry order, then
/// subject order (CR 704.3).
fn check_view(
    registry: &SbaRegistry,
    view: &SbaView<'_>,
    env: &dyn SbaEnvironment,
) -> Vec<SbaFinding> {
    let variant = env.variant();
    let mut findings = Vec::new();
    for entry in registry.entries() {
        if !entry.active_in(variant) {
            continue;
        }
        for subject in entry.subjects(view) {
            findings.push(SbaFinding {
                entry_id: entry.id,
                subject,
            });
        }
    }
    findings
}

/// A clone of every component of an [`EventContext`] (cheap, structurally shared), for
/// all-or-nothing passes.
pub(super) struct Saved {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
}

impl Saved {
    pub(super) fn take(ctx: &EventContext<'_>) -> Saved {
        Saved {
            game: ctx.game.clone(),
            zones: ctx.zones.clone(),
            ids: ctx.ids.clone(),
            permanents: ctx.permanents.clone(),
            memories: ctx.memories.clone(),
            lki: ctx.lki.clone(),
            events: ctx.events.clone(),
        }
    }

    pub(super) fn restore(self, ctx: &mut EventContext<'_>) {
        *ctx.game = self.game;
        *ctx.zones = self.zones;
        *ctx.ids = self.ids;
        *ctx.permanents = self.permanents;
        *ctx.memories = self.memories;
        *ctx.lki = self.lki;
        *ctx.events = self.events;
    }
}

/// What the performance of a pass needs from the check snapshot besides the findings
/// (CR 704.3: everything is read from the one snapshot before anything changes).
struct Plan {
    /// CR 704.5j: the legend-rule groups that are findings, in asking order (CR 101.4: the
    /// controller's APNAP rank, then the lowest id).
    legend_groups: Vec<LegendGroup>,
    /// CR 704.5q, CR 704.5r: the counters to remove per object and kind. When both entries
    /// remove counters of one kind from one object, the larger amount is removed (decision
    /// #42 FP-041).
    counter_removals: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl Plan {
    fn new(registry: &SbaRegistry, view: &SbaView<'_>, findings: &[SbaFinding]) -> Plan {
        let mut legend_subjects: BTreeSet<ObjectId> = BTreeSet::new();
        let mut counter_removals: BTreeMap<ObjectId, BTreeMap<String, i64>> = BTreeMap::new();
        for finding in findings {
            let (Some(entry), SbaSubject::Object(o)) =
                (registry.get(finding.entry_id), finding.subject)
            else {
                continue;
            };
            match entry.action {
                SbaAction::LegendChoice => {
                    legend_subjects.insert(o);
                }
                SbaAction::RemoveCounters => {
                    let removals = counter_removals.entry(o).or_default();
                    for (kind, amount) in counter_removal(view, finding.entry_id, o) {
                        let slot = removals.entry(kind).or_insert(0);
                        *slot = (*slot).max(amount);
                    }
                }
                _ => {}
            }
        }
        let legend_groups = if legend_subjects.is_empty() {
            Vec::new()
        } else {
            legend_groups(view)
                .into_iter()
                .filter(|g| g.members.iter().all(|id| legend_subjects.contains(id)))
                .collect()
        };
        Plan {
            legend_groups,
            counter_removals,
        }
    }
}

/// Performs one state-based action pass (REQ-SBA-001, CR 704.3, CR 704.7, CR 704.8): every
/// applicable state-based action is found by [`check`] on one snapshot, before anything
/// changes, then all are performed simultaneously as one event. The heartbeat repeats the
/// pass while it performs something (CR 704.3); this is one pass.
///
/// - **The legend rule** (CR 704.5j, CR 201.2a): the legendary permanents of each (evaluated,
///   CR 613.1b) controller form a conflict graph (an edge between two that have at least one
///   name in common); each connected component with an edge is a group. Before anything
///   changes, the controller of each group is asked ONE `SELECT_OBJECTS_FROM_ZONE` choice
///   (CR 101.4: groups in APNAP order of their controllers, then ascending lowest id; candidates
///   the group ascending; min 1; max 1 if every pair of members has the same name, otherwise
///   the group size minus 1). The answer is the set of survivors and must be a maximal set of
///   members no two of which have the same name; on timeout the greedy such set by ascending
///   id. Every other member is moved with the batch below.
/// - **Moves** (CR 704.5f-704.5k, CR 704.5m, CR 704.5s): ONE batch with one
///   `ZoneChange` to the owner's graveyard per object, cause
///   [`ZoneChangeCause::StateBasedAction`] ([`ZoneChangeCause::Sacrifice`] for a Saga its
///   controller sacrifices, CR 704.5s, even if another entry also moves it), and one
///   `RemoveCounters` event per object and counter kind (CR 704.5q, CR 704.5r), ascending
///   object id then kind (an object with several findings moves once), submitted through
///   [`process_batch`] with `sba` true: replacement effects see the merged same-object group
///   (CR 704.7) and last known information of every moved object is captured before any move
///   (CR 704.8), then one `Unattach` event per permanent that becomes unattached (CR 704.5n,
///   CR 704.5p, CR 701.3d), ascending id, submitted after every zone change and counter
///   removal. The bus executes the events that are not zone changes before the zone changes
///   (its reconcile order), and the batch captures last known information before any of its
///   events, so records and the look-back of "becomes unattached" triggers (CR 603.10c) see
///   the attachments from before the pass. Each remains on the battlefield and is reported
///   in [`SbaOutcome::unattached`] if it was still attached. An object with a planned move or
///   ceasing has no counters removed and no unattach event in this batch.
/// - **Prevented moves** (CR 704.3, CR 704.5q, CR 704.5r, CR 704.5n, CR 704.5p): an object
///   whose planned move did not take place (a replacement effect kept it on the battlefield
///   under the same id) gets its counter removals and its unattach event in ONE follow-up
///   [`process_batch`] call (`sba` true) right after the main batch, in the same forms and
///   order, with the amounts from the check snapshot. An object that left the battlefield
///   (to any zone) gets nothing.
/// - **Removing a dungeon from the game** (CR 704.5t, CR 309.7): like ceasing to exist below
///   (no zone change, last known information captured), reported in
///   [`SbaOutcome::removed_from_game`].
/// - **Ceasing to exist** (CR 704.5d, CR 704.5e): after the batch, each such object
///   ceases to exist through [`Zones::cease_to_exist`](crate::zones::Zones::cease_to_exist)
///   (not a zone change: no `ZoneChange` event, nothing added to `executed`). Its last known
///   information is captured for every ceasing object, before any removal and from the state
///   before the batch (CR 704.8; REQ-CORE-003: the object leaves its
///   zone); a permanent among them also leaves the permanent container. An object that is both
///   a move target and ceasing only ceases. If a replacement effect of the batch moved a
///   ceasing object (CR 400.7: a new object), the reason it ceases (a token, a copy of a spell,
///   a copy of a card) is tested again in its new zone, the CR 723.3c exemption read from its
///   old id; if the reason still holds, the new object ceases in this same pass and its NEW id
///   is reported in [`SbaOutcome::ceased`]; otherwise (for example a card copy moved onto the
///   battlefield) it stays.
/// - **Losing the game** (CR 704.5a, CR 704.5b, CR 704.5c): each losing player gets the
///   [`LOST_THE_GAME`] designation once, whatever the number of reasons (CR 704.7); losses of
///   different players are never merged. Deciding a draw or a winner (CR 104.4a) is the
///   caller's.
///
/// The empty-library-draw flag (CR 704.5b, "since the last time state-based actions were
/// checked") is consumed by every successful call, whether or not a player loses; the
/// deathtouch flag of `env` is cleared by [`SbaEnvironment::clear_deathtouch_damaged`] after
/// every successful call, for every creature (indestructible and phased-out ones included).
///
/// [`SbaOutcome::performed`] is `true` exactly when the pass changed the game state: a move, a
/// counter removal or an unattach event occurred (in either batch), an object ceased to exist
/// or was removed from the game, a player lost, or the replacement registry changed
/// (for example a one-use effect was consumed). A finding whose only move a replacement
/// skipped is reported in [`SbaOutcome::findings`] but is not performed; the heartbeat
/// repeats the check while `performed` (CR 704.3).
///
/// # Errors
/// [`BusInterrupt::Pending`] when a player must choose a replacement effect or the legend-rule
/// survivors (propagate it to the [`ChoiceSession`](crate::choice::ChoiceSession), which
/// re-runs the pass from the base state); [`BusInterrupt::Error`] on a failure, including a
/// legend-rule answer that is not a maximal set of candidates no two of which have the same
/// name ([`ChoiceError::Procedure`]). In both cases every component of `ctx` (both batches
/// included), the replacement registry and the flags of `env` are exactly as before the call.
pub fn perform_all(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &mut dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // CR 704.3: every predicate is evaluated against one snapshot before anything changes.
    let (findings, plan) = {
        let view = SbaView::new(
            ctx.game,
            ctx.zones,
            ctx.events,
            ctx.permanents,
            effects,
            layer_env,
            &*env,
        );
        let findings = check_view(registry, &view, &*env);
        let plan = Plan::new(registry, &view, &findings);
        (findings, plan)
    };
    let saved = Saved::take(ctx);
    let replacements_before = replacements.clone();
    match perform_findings(registry, ctx, replacements, findings, &plan, &*env, choices) {
        Ok(mut outcome) => {
            // CR 704.3: "If any state-based actions are performed as a result of a check, the
            // check is repeated": performed means the pass changed the state.
            outcome.performed |= *replacements != replacements_before;
            // CR 704.5h: "since the last time state-based actions were checked".
            env.clear_deathtouch_damaged();
            Ok(outcome)
        }
        Err(e) => {
            saved.restore(ctx);
            *replacements = replacements_before;
            Err(e)
        }
    }
}

/// Performs `findings` simultaneously; on error the caller restores everything.
fn perform_findings(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    findings: Vec<SbaFinding>,
    plan: &Plan,
    env: &dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // CR 704.5j: every legend-rule choice is asked before any state changes, so a pending
    // choice leaves the state as it was.
    // CR 101.4: the groups are in APNAP order of their controllers; all moves happen together
    // after every answer.
    let mut legend_losers: BTreeSet<ObjectId> = BTreeSet::new();
    for group in &plan.legend_groups {
        let kept = ask_legend_choice(choices, group)?;
        legend_losers.extend(
            group
                .members
                .iter()
                .copied()
                .filter(|id| !kept.contains(id)),
        );
    }

    // CR 704.5b, CR 121.4: the check consumes the flag. Taken before the batch so that an
    // empty-library draw made by a replacement during the batch counts for the next check.
    ctx.events.take_empty_library_draws();

    let mut loss_reasons: BTreeMap<PlayerId, Vec<&'static str>> = BTreeMap::new();
    let mut ceasing: BTreeMap<ObjectId, BTreeSet<CeaseReason>> = BTreeMap::new();
    let mut moving: BTreeMap<ObjectId, ZoneChangeCause> = BTreeMap::new();
    let mut unattaching: BTreeSet<ObjectId> = BTreeSet::new();
    let mut dungeons: BTreeSet<ObjectId> = BTreeSet::new();
    // CR 704.5j: the legend-rule losers are put into their owners' graveyards.
    for &o in &legend_losers {
        moving.insert(o, ZoneChangeCause::StateBasedAction);
    }
    for finding in &findings {
        let Some(entry) = registry.get(finding.entry_id) else {
            continue;
        };
        match (entry.action, finding.subject) {
            // CR 704.7: one loss per player, every reason kept.
            (SbaAction::LoseGame, SbaSubject::Player(p)) => {
                loss_reasons.entry(p).or_default().push(finding.entry_id);
            }
            (SbaAction::CeaseToExist, SbaSubject::Object(o)) => {
                ceasing
                    .entry(o)
                    .or_default()
                    .extend(cease_reasons(finding.entry_id, o, env));
            }
            (SbaAction::PutIntoGraveyard | SbaAction::Destroy, SbaSubject::Object(o)) => {
                moving.entry(o).or_insert(ZoneChangeCause::StateBasedAction);
            }
            // CR 704.5s, CR 715.4: the controller sacrifices it; the sacrifice cause wins over
            // a plain SBA move of the same object (decision #42 FP-041).
            (SbaAction::Sacrifice, SbaSubject::Object(o)) => {
                moving.insert(o, ZoneChangeCause::Sacrifice);
            }
            // CR 704.5n, CR 704.5p
            (SbaAction::Unattach, SbaSubject::Object(o)) => {
                unattaching.insert(o);
            }
            // CR 704.5t
            (SbaAction::RemoveFromGame, SbaSubject::Object(o)) => {
                dungeons.insert(o);
            }
            // CR 704.5j (asked above) and CR 704.5q, CR 704.5r (from the plan).
            _ => {}
        }
    }
    // Ceasing wins over a move of the same object.
    moving.retain(|o, _| !ceasing.contains_key(o));
    // Every CR 704.5n / 704.5p finding, for the follow-up batch and the outcome.
    let unattach_findings = unattaching.clone();
    // A move or ceasing wins over unattaching and counter removal in the main batch (decision
    // #42 FP-041); a prevented move gets them in the follow-up batch.
    let leaves = |o: &ObjectId| moving.contains_key(o) || ceasing.contains_key(o);
    unattaching.retain(|o| !leaves(o));
    // CR 704.5t: an object that ceases to exist (CR 704.5d, CR 704.5e) only ceases.
    dungeons.retain(|o| !ceasing.contains_key(o));

    // CR 704.3, CR 704.7: one batch, ascending object id, then counter kind.
    let mut batch_events: BTreeMap<ObjectId, Vec<GameEvent>> = BTreeMap::new();
    for (&object, &cause) in &moving {
        batch_events
            .entry(object)
            .or_default()
            .push(GameEvent::ZoneChange {
                object,
                // CR 704.5f-704.5k, CR 704.5m, CR 704.5s: the owner's graveyard (CR 400.3).
                request: MoveRequest::new(ZoneKind::Graveyard),
                cause,
            });
    }
    for (&object, removals) in &plan.counter_removals {
        if leaves(&object) || dungeons.contains(&object) {
            continue;
        }
        for (kind, &amount) in removals {
            if amount > 0 {
                // CR 704.5q, CR 704.5r, CR 122.3
                batch_events
                    .entry(object)
                    .or_default()
                    .push(GameEvent::RemoveCounters {
                        target: CounterTarget::Object(object),
                        kind: kind.clone(),
                        amount,
                    });
            }
        }
    }
    let mut moves: Vec<GameEvent> = batch_events.into_values().flatten().collect();
    // CR 704.5n, CR 704.5p, CR 701.3d: unattached, it remains on the battlefield. After every
    // zone change and counter removal of the batch, ascending id: the batch captures last
    // known information before any event runs, so every record and the look-back of "becomes
    // unattached" triggers (CR 603.10c) see the attachments from before the pass (CR 704.8).
    moves.extend(
        unattaching
            .iter()
            .map(|&attachment| GameEvent::Unattach { attachment }),
    );

    // CR 704.8: the batch may change the state (a removed permanent unattaches what is attached
    // to it). The permanent state and markers of an object that only ceases (or a dungeon
    // removed from the game) are read from here. Only a snapshot: nothing is inserted into the
    // live LKI store before the bus runs (an `Instead` moving a ceasing object would make the
    // bus fail with `AlreadyCaptured`).
    let pre_batch = (!moves.is_empty() && (!ceasing.is_empty() || !dungeons.is_empty()))
        .then(|| (ctx.permanents.clone(), ctx.events.markers().clone()));

    let mut executed: Vec<ExecutedBatch> = Vec::new();
    if !moves.is_empty() {
        // CR 704.3, CR 704.7, CR 704.8: one simultaneous event through the bus.
        let report = process_batch(
            ctx,
            replacements,
            &EventBatch::from_events(moves),
            true,
            choices,
        )?;
        executed = report.executed;
        // CR 704.3, CR 704.5q, CR 704.5r, CR 704.5n, CR 704.5p: an object whose planned move a
        // replacement effect prevented (still on the battlefield under the same id; CR 400.7:
        // an object that moved has a new id) still gets its counter removals and its unattach.
        // They cannot be in the main batch (the executor rejects events for an object moved
        // earlier in the batch), so they follow in one batch, from the check snapshot's plan.
        let follow_up = prevented_move_events(ctx, &moving, plan, &unattach_findings);
        if !follow_up.is_empty() {
            let report = process_batch(
                ctx,
                replacements,
                &EventBatch::from_events(follow_up),
                true,
                choices,
            )?;
            executed.extend(report.executed);
        }
    }

    // CR 400.7: a replacement effect of the batch may have moved a ceasing object; follow
    // every executed move (old -> new -> newer) to the object it is now.
    let mut moved_to: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    for batch in &executed {
        for event in &batch.events {
            if let Outcome::ZoneChange(m) = event.outcome
                && m.old != m.new
            {
                moved_to.insert(m.old, m.new);
            }
        }
    }

    // CR 704.5d, CR 704.5e: ceasing to exist is not a zone change (no ZoneChange event).
    // (id, moved by the batch, removed from the game by CR 704.5t)
    let mut targets: Vec<(ObjectId, bool, bool)> = Vec::new();
    for (old, reasons) in ceasing {
        let mut id = old;
        while let Some(&next) = moved_to.get(&id) {
            id = next;
        }
        let Some(record) = ctx.zones.object(id) else {
            continue;
        };
        if id != old {
            // Moved by an effect: the object kind persists (decision #41 FP-040); test the
            // reason again where the new object is. CR 723.3c: the exemption of the old id.
            let zone = record.zone;
            let exempt = env.exempt_from_copy_sba(old);
            let holds = reasons.iter().any(|r| match r {
                // CR 704.5d, CR 111.7
                CeaseReason::Token => zone != ZoneId::Battlefield,
                // CR 704.5e, CR 707.10a
                CeaseReason::SpellCopy => !exempt && zone != ZoneId::Stack,
                CeaseReason::CardCopy => {
                    !exempt && !matches!(zone, ZoneId::Stack | ZoneId::Battlefield)
                }
            });
            if !holds {
                continue;
            }
        }
        targets.push((id, id != old, false));
    }
    // CR 704.5t, CR 309.7: the dungeon card's owner removes it from the game; not a zone
    // change. A dungeon is in the command zone (CR 309.2c), which the batch never moves.
    for &id in &dungeons {
        if ctx.zones.object(id).is_some() {
            targets.push((id, false, true));
        }
    }
    // REQ-CORE-003, CR 704.3, CR 704.8: the ceasing objects leave their zones as part of the
    // one event, so their last known information is derived from the state before any of the
    // state-based actions were performed. An object the batch did not move is captured from the
    // pre-batch snapshot (a permanent the batch removed would otherwise have unattached it); an
    // object the batch moved and that ceases under its new id is captured as it is now, in the
    // zone it ceases from (the snapshot knows only the old id). Every record is captured before
    // any removal: a removal unattaches the objects attached to it, which would change a later
    // object's record. Captured after the batch, not before it: capturing before the batch would
    // make the bus fail with `AlreadyCaptured` if an `Instead` moved the object (decision #41
    // FP-040).
    let live = (!targets.is_empty()
        && (pre_batch.is_none() || targets.iter().any(|&(_, moved, _)| moved)))
    .then(|| (ctx.permanents.clone(), ctx.events.markers().clone()));
    for &(id, moved, _) in &targets {
        let (permanents, markers) = match (&pre_batch, &live, moved) {
            (Some(pre), _, false) => pre,
            (_, Some(now), _) => now,
            // `live` is built whenever `targets` is not empty and `pre_batch` is `None` or an
            // object moved.
            _ => continue,
        };
        capture_ceasing_lki(ctx, id, permanents, markers)?;
    }
    let mut ceased = Vec::new();
    let mut removed_from_game = Vec::new();
    for (id, _, removed) in targets {
        if ctx.permanents.get(id).is_some() {
            ctx.permanents
                .leave(id)
                .map_err(|e| BusInterrupt::Error(BusError::Event(EventError::from(e))))?;
        }
        ctx.zones
            .cease_to_exist(id)
            .map_err(|e| BusInterrupt::Error(BusError::Event(EventError::from(e))))?;
        if removed {
            removed_from_game.push(id);
        } else {
            ceased.push(id);
        }
    }
    ceased.sort_unstable();
    removed_from_game.sort_unstable();

    // CR 704.5n, CR 704.5p: the permanents an unattach event of either batch unattached.
    let unattached: Vec<ObjectId> = executed
        .iter()
        .flat_map(|b| &b.events)
        .filter_map(|ev| match ev.instance.event() {
            GameEvent::Unattach { attachment } if unattach_findings.contains(attachment) => {
                Some(*attachment)
            }
            _ => None,
        })
        .collect::<BTreeSet<ObjectId>>()
        .into_iter()
        .collect();

    // CR 704.5a, CR 704.5b, CR 704.5c: the player loses the game.
    for &player in loss_reasons.keys() {
        if let Some(mut p) = ctx.game.player_mut(player) {
            p.set_designation(LOST_THE_GAME, DesignationValue::Flag);
        }
    }

    // CR 704.3: performed means the state changed (perform_all adds a registry change).
    let performed = !executed.is_empty()
        || !ceased.is_empty()
        || !loss_reasons.is_empty()
        || !unattached.is_empty()
        || !removed_from_game.is_empty();
    Ok(SbaOutcome {
        performed,
        lost_players: loss_reasons.keys().copied().collect(),
        loss_reasons,
        findings,
        executed,
        ceased,
        unattached,
        removed_from_game,
    })
}

/// CR 704.5q, CR 704.5r, CR 704.5n, CR 704.5p: the follow-up events of the objects whose
/// planned move did not take place (still on the battlefield under the same id after the main
/// batch): their counter removals (ascending id, then kind), then their unattach events
/// (ascending id), as in the main batch.
fn prevented_move_events(
    ctx: &EventContext<'_>,
    moving: &BTreeMap<ObjectId, ZoneChangeCause>,
    plan: &Plan,
    unattach_findings: &BTreeSet<ObjectId>,
) -> Vec<GameEvent> {
    let stayed: Vec<ObjectId> = moving
        .keys()
        .copied()
        .filter(|&id| {
            ctx.zones
                .object(id)
                .is_some_and(|r| r.zone == ZoneId::Battlefield)
        })
        .collect();
    let mut events = Vec::new();
    for &object in &stayed {
        let Some(removals) = plan.counter_removals.get(&object) else {
            continue;
        };
        for (kind, &amount) in removals {
            if amount > 0 {
                events.push(GameEvent::RemoveCounters {
                    target: CounterTarget::Object(object),
                    kind: kind.clone(),
                    amount,
                });
            }
        }
    }
    events.extend(
        stayed
            .iter()
            .filter(|id| unattach_findings.contains(id))
            .map(|&attachment| GameEvent::Unattach { attachment }),
    );
    events
}

/// CR 704.5j, CR 201.2a: asks the controller of `group` which permanents stay, as ONE
/// `SELECT_OBJECTS_FROM_ZONE` choice: candidates the group ascending, min 1, max 1 for a group
/// whose members all have the same name and otherwise the group size minus 1; on timeout the
/// greedy maximal set by ascending id ([`LegendGroup::default_survivors`]).
///
/// # Errors
/// [`BusInterrupt::Pending`] until answered; [`BusInterrupt::Error`] with
/// [`ChoiceError::Procedure`] if the answer is not a valid survivor set.
fn ask_legend_choice(
    choices: &mut ChoiceContext<'_>,
    group: &LegendGroup,
) -> Result<Vec<ObjectId>, BusInterrupt> {
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: group.controller,
        choice_type: ChoiceType::SelectObjectsFromZone,
        reason: ChoiceReason {
            cr: "CR 704.5j".to_owned(),
            source: None,
        },
        min: 1,
        max: u64::try_from(group.max_survivors()).unwrap_or(u64::MAX),
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitObjectSelection {
            objects: group.default_survivors(),
        },
        options: ChoiceOptions::Enumerated(
            group
                .members
                .iter()
                .map(|&id| ChoiceOption::Object(id))
                .collect(),
        ),
        payload: ChoicePayload::SelectObjectsFromZone {
            candidates: group.members.clone(),
        },
    };
    legend_survivors(group, choices.ask(request)?)
}

/// CR 704.5j, CR 201.2a: the permanents of `group` the answer `response` keeps.
///
/// # Errors
/// [`BusInterrupt::Error`] with [`ChoiceError::Procedure`] if the answer is not a selection, or
/// not a maximal set of candidates no two of which have the same name
/// ([`LegendGroup::check_survivors`]).
pub(super) fn legend_survivors(
    group: &LegendGroup,
    response: ChoiceResponse,
) -> Result<Vec<ObjectId>, BusInterrupt> {
    let protocol =
        |message: String| BusInterrupt::Error(BusError::Choice(ChoiceError::Procedure(message)));
    match response {
        ChoiceResponse::SubmitObjectSelection { objects } => {
            group.check_survivors(&objects).map_err(|m| {
                protocol(format!(
                    "invalid legend-rule survivors among {} candidates: {m}",
                    group.members.len()
                ))
            })?;
            Ok(objects)
        }
        other => Err(protocol(format!("unexpected answer {}", other.wire_name()))),
    }
}

/// Why an object ceases to exist; the kind persists across a move by an effect (decision
/// #41 FP-040).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CeaseReason {
    /// CR 704.5d, CR 111.7: a token.
    Token,
    /// CR 704.5e, CR 707.10a: a copy of a spell.
    SpellCopy,
    /// CR 704.5e, CR 707.10a: a copy of a card.
    CardCopy,
}

/// The reasons a cease finding of entry `entry_id` gives for `object`, from the check snapshot.
fn cease_reasons(entry_id: &str, object: ObjectId, env: &dyn SbaEnvironment) -> Vec<CeaseReason> {
    match entry_id {
        // CR 704.5d
        "704.5d" => vec![CeaseReason::Token],
        // CR 704.5e
        _ => {
            let mut reasons = Vec::new();
            if env.is_spell_copy(object) {
                reasons.push(CeaseReason::SpellCopy);
            }
            if env.is_card_copy(object) {
                reasons.push(CeaseReason::CardCopy);
            }
            reasons
        }
    }
}

/// The LKI provider of a ceasing object: characteristics from the context's provider, counters
/// from the object markers (CR 122.1) and attachments from the permanent container
/// (CR 701.3a), the same three answers the event executor gives on a zone change.
struct CeasingLki<'a> {
    inner: &'a dyn CharacteristicsProvider,
    markers: &'a ObjectMarkers,
    permanents: &'a Permanents,
}

impl CharacteristicsProvider for CeasingLki<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.inner.characteristics(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.markers.counters(id)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.permanents.attachments_of(AttachTarget::Permanent(id))
    }
}

/// Captures the last known information of `id` before it ceases to exist (REQ-CORE-003,
/// CR 608.2h, CR 704.8), reading the permanent state and counters from the given snapshot. The request names exile as a pseudo-destination: for exile
/// `capture_lki_on_move` always treats the object as leaving its zone, so a record is stored
/// from any zone (a token already in a graveyard included). An object the provider does not
/// know gets no record (it still ceases).
fn capture_ceasing_lki(
    ctx: &mut EventContext<'_>,
    id: ObjectId,
    permanents: &Permanents,
    markers: &ObjectMarkers,
) -> Result<(), BusInterrupt> {
    let provider = CeasingLki {
        inner: ctx.provider,
        markers,
        permanents,
    };
    match capture_lki_on_move(
        ctx.zones,
        permanents,
        ctx.memories,
        &provider,
        ctx.lki,
        id,
        &MoveRequest::new(ZoneKind::Exile),
    ) {
        Ok(_) | Err(MemoryError::NoCharacteristics(_) | MemoryError::AlreadyCaptured(_)) => Ok(()),
        Err(e) => Err(BusInterrupt::Error(BusError::Event(EventError::from(e)))),
    }
}
