// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! One state-based action pass (REQ-SBA-001, CR 704.3): check every entry against one
//! snapshot, then perform every result simultaneously as one event.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};

use super::registry::{
    LegendGroup, ProtectorAsk, SbaAction, SbaRegistry, SbaView, counter_removal, legend_groups,
    protector_asks,
};
use super::types::{LOST_THE_GAME, SPEED, SbaEnvironment, SbaFinding, SbaOutcome, SbaSubject};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceType, PendingChoiceRequest, RequestId,
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
    /// CR 704.5j: the legend-rule groups that are findings (CR 101.4: the controller's APNAP
    /// rank, then the lowest id; asked in the order of `requests`).
    legend_groups: Vec<LegendGroup>,
    /// CR 704.5q, CR 704.5r: the counters to remove per object and kind. When both entries
    /// remove counters of one kind from one object, the larger amount is removed (decision
    /// #42 FP-041).
    counter_removals: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    /// CR 704.5x, CR 704.5y: the protector choices (CR 101.4: the controller's APNAP rank, then
    /// ascending battle id; asked in the order of `requests`). A battle that is also a CR 704.5v or
    /// CR 704.5w finding normally goes to the graveyard and has none before the batch; it is
    /// remembered in `retained_battles` instead.
    protector_asks: Vec<ProtectorAsk>,
    /// CR 704.5x, CR 704.5y, CR 704.5v, CR 704.5w, CR 614.6: the battles at defense 0 whose
    /// protector choice was suppressed before the batch, each with the CR reference of its
    /// CR 704.5x or CR 704.5y finding. If a replacement effect replaced the graveyard move and
    /// the battle is still on the battlefield as the same object without a protector after the
    /// batch, its controller is asked then (decision "Gate 02-state-objects R3 — Protector request
    /// after the batch for a retained zero-defense battle").
    retained_battles: BTreeMap<ObjectId, &'static str>,
    /// CR 101.4: every request of the pass, in asking order: the chooser's APNAP rank, then
    /// the legend-rule groups before the protector choices of that player, then the group's
    /// lowest id or the ascending battle id. A protector choice without a chooser or without
    /// candidates is no request (its battle goes to the graveyard); one whose controller has
    /// lost the game or is not a player of the game (CR 800.4a) is no request either, and its
    /// battle stays ([`can_choose_protector`]).
    requests: Vec<PlannedRequest>,
}

/// One choice of a pass, an index into [`Plan::legend_groups`] or [`Plan::protector_asks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PlannedRequest {
    /// CR 704.5j: the legend-rule group at this index.
    Legend(usize),
    /// CR 704.5x, CR 704.5y: the protector choice at this index.
    Protector(usize),
}

impl Plan {
    fn new(
        registry: &SbaRegistry,
        view: &SbaView<'_>,
        game: &GameState,
        findings: &[SbaFinding],
    ) -> Plan {
        let mut legend_subjects: BTreeSet<ObjectId> = BTreeSet::new();
        let mut counter_removals: BTreeMap<ObjectId, BTreeMap<String, i64>> = BTreeMap::new();
        let mut protector_battles: BTreeMap<ObjectId, &'static str> = BTreeMap::new();
        let mut zero_defense: BTreeSet<ObjectId> = BTreeSet::new();
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
                // CR 704.5x, CR 704.5y (mutually exclusive for one battle in one check).
                SbaAction::ChooseProtector => {
                    protector_battles.insert(o, entry.cr_ref);
                }
                _ => {}
            }
            // CR 704.5v, CR 704.5w: the battle goes to its owner's graveyard anyway.
            if matches!(finding.entry_id, "704.5v" | "704.5w") {
                zero_defense.insert(o);
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
        // CR 704.5v, CR 704.5w, CR 704.8: no request before the batch for a battle that leaves
        // with it (asking would add a meaningless decision); remembered in case its move is
        // replaced (CR 614.6) and it stays.
        let (retained_battles, protector_battles): (BTreeMap<_, _>, BTreeMap<_, _>) =
            protector_battles
                .into_iter()
                .partition(|(b, _)| zero_defense.contains(b));
        let protector_asks = if protector_battles.is_empty() {
            Vec::new()
        } else {
            protector_asks(view, &protector_battles)
        };
        // CR 101.4: one APNAP-ordered sequence per pass for both kinds of choice (decision
        // "Gate 02-state-objects F2 — One APNAP request sequence per SBA pass"): the chooser's APNAP rank, then (one player's own
        // choices, in a fixed order) the legend-rule groups before the protector choices, then
        // the lowest group id or the battle id.
        let order = view.apnap_order();
        let rank = |p: PlayerId| order.iter().position(|&q| q == p).unwrap_or(order.len());
        let mut keyed: Vec<((usize, u8, ObjectId), PlannedRequest)> = Vec::new();
        for (i, g) in legend_groups.iter().enumerate() {
            keyed.push((
                (rank(g.controller), 0, g.members[0]),
                PlannedRequest::Legend(i),
            ));
        }
        // CR 704.5x, CR 704.5y, CR 800.4a: only a controller still in the game is asked
        // ([`can_choose_protector`]); a battle whose controller lost the game gets no request and
        // no request id, and does not move (decision "Gate 02-state-objects R3d — No protector
        // request to a controller who lost the game, on every path").
        for (i, a) in protector_asks.iter().enumerate() {
            if let Some(controller) = a.controller
                && !a.candidates.is_empty()
                && can_choose_protector(game, controller)
            {
                keyed.push((
                    (rank(controller), 1, a.battle),
                    PlannedRequest::Protector(i),
                ));
            }
        }
        keyed.sort_unstable();
        Plan {
            legend_groups,
            counter_removals,
            protector_asks,
            retained_battles,
            requests: keyed.into_iter().map(|(_, r)| r).collect(),
        }
    }
}

/// Performs one state-based action pass (REQ-SBA-001, CR 704.3, CR 704.7, CR 704.8): every
/// applicable state-based action is found by [`check`] on one snapshot, before anything
/// changes, then all are performed simultaneously as one event. The heartbeat repeats the
/// pass while it performs something (CR 704.3); this is one pass.
///
/// - **Request order** (CR 101.4): the legend-rule and protector choices below are ONE
///   sequence for the pass, asked before anything changes: by the chooser's APNAP rank (the
///   active player first, then the turn order), then, for one player, every legend-rule group
///   before every protector choice, then ascending lowest group id or ascending battle id.
///   Request ids are assigned in this order. The only requests after the batch are the
///   protector choices of retained zero-defense battles (see "Battle protectors" below).
/// - **The legend rule** (CR 704.5j, CR 201.2a): the legendary permanents of each (evaluated,
///   CR 613.1b) controller form a conflict graph (an edge between two that have at least one
///   name in common); each connected component with an edge is a group. Before anything
///   changes, the controller of each group is asked ONE `SELECT_OBJECTS_FROM_ZONE` choice
///   (in the request order of the pass below; candidates the group ascending; min 1; max 1 if every pair of members has the same name, otherwise
///   the group size minus 1). The answer is the set of survivors and must be a maximal set of
///   members no two of which have the same name; on timeout the greedy such set by ascending
///   id. Every other member is moved with the batch below.
/// - **The one batch** (CR 704.3, CR 704.7, CR 704.8): every move, counter removal and
///   unattach event of the pass is in ONE [`process_batch`] call with `sba` true, in this
///   order: one `Unattach` event per attachment that becomes unattached (CR 704.5n,
///   CR 704.5p, CR 701.3d; one event even if both entries apply), ascending id; then one
///   `ZoneChange` to the owner's graveyard per moving object (CR 704.5f-704.5k,
///   CR 704.5m, CR 704.5s), ascending id, cause [`ZoneChangeCause::StateBasedAction`]
///   ([`ZoneChangeCause::Sacrifice`] for a Saga its controller sacrifices, CR 704.5s, even if
///   another entry also moves it; an object with several findings moves once); then one
///   `RemoveCounters` event per object and counter kind (CR 704.5q, CR 704.5r), ascending id
///   then kind. The unattach events come first so that the bus, which executes the events
///   that are not zone changes in submission order (a replacement's `Instead` events at the
///   position of the event they replace, CR 614.6), executes each planned unattach before an
///   `Attach` that a replacement effect makes of a later event of the batch: the planned
///   unattach clears the link from before the pass, not such a new link (decision "#42 FP-041
///   — Planned unattach and pre-pass host (round 3)"; known limitation: an `Attach` produced
///   by replacing an EARLIER unattach event of the batch is still cleared). Counter
///   removals and unattach events are emitted for EVERY applicable permanent, including one
///   that also moves in the pass; only an object that ceases to exist and a dungeon removed
///   from the game get none (decision "#42 FP-041 — One SBA batch: moves, counter removals and
///   unattach events"). Replacement effects see the merged same-object group (CR 704.7) and
///   are judged against the state before the event; the bus executes every event that is not
///   a zone change before every zone change, and captures last known information of every
///   object before any event runs (CR 704.8), so an attachment whose host leaves in the same
///   pass gets exactly one `Unattach` event against its attachment from before the pass (the
///   host's zone change then has nothing to unattach), and the look-back of "becomes
///   unattached" triggers (CR 603.10c) sees it. Counters left on an object that moves get no
///   removal event (CR 122.2). [`SbaOutcome::unattached`] lists the attachment of every
///   `Unattach` event of the batch that occurred, planned or produced by a replacement effect
///   (for example an `Instead` of another event of the batch), if it is still on the
///   battlefield as the same object at the end of the pass.
/// - **Battle protectors** (CR 704.5x, CR 704.5y, CR 310.11, CR 310.9a, CR 310.12a): before
///   anything changes, the (evaluated) controller of each battle that has no player in the game
///   as its protector and is not being attacked (CR 704.5x), or whose protector can't be its
///   protector, whether or not it is being attacked (CR 704.5y), is asked ONE `CHOOSE_PLAYER`
///   choice (in the request order of the pass above; reason `CR 704.5x` or `CR 704.5y`;
///   `reason.source` the battle; candidates the appropriate players ascending: for a Siege the
///   opponents of its controller, for a battle with no battle type its controller, each allowed
///   by [`SbaEnvironment::can_be_protector`]; min 1, max 1, default the lowest candidate). An
///   answer that is not a candidate is [`ChoiceError::Invalid`]. A battle with no candidate is
///   moved to its owner's graveyard with the batch below, without a request. A battle whose
///   controller has lost the game (the [`LOST_THE_GAME`] designation from an earlier pass) or is
///   not a player of the game gets no request and consumes no request id, and stays where it is
///   (CR 800.4a: the battle leaves the game when its controller does, performed by the caller;
///   CR 800.4h: a choice that survives the departure goes to the next player in turn order, not
///   modelled here, #125;
///   decision "Gate 02-state-objects R3d — No protector request to a controller who lost the
///   game, on every path"). A battle that is
///   also a CR 704.5v or CR 704.5w finding gets no request before the batch: when it leaves with
///   the batch, asking would change nothing (CR 704.8). If a replacement effect replaced its
///   graveyard move (CR 614.6, CR 704.7) and it is still on the battlefield as the same object
///   and still a CR 704.5x or CR 704.5y subject after the batch, its controller is asked the same
///   choice AFTER the batch, in the same pass (the subject test, the controller and the
///   candidates from a view of the state after the batch, built from the post-batch layer
///   inputs: those [`perform_all_relayered`] or [`perform_all_post_batch`] receives from its
///   provider, or `effects` and `layer_env` again for `perform_all`; the protector facts
///   ([`SbaEnvironment::battle_protector`], [`SbaEnvironment::can_be_protector`]) from the
///   read-only view the [`perform_all_post_batch`] provider supplies, or from `env` otherwise;
///   the reason the CR reference of the entry that flags it after the batch; ordered by the
///   controller's APNAP rank, then ascending battle id; these requests come after every request
///   of the pass and every request of the batch); with no candidate, or with a controller who
///   has lost the game (the [`LOST_THE_GAME`] designation), it gets no request and the next
///   pass finds it again (decisions "Gate 02-state-objects R3 — Protector request after the
///   batch for a retained zero-defense battle", "Gate 02-state-objects R3b — Post-batch layer
///   inputs for the retained-battle request" and "Gate 02-state-objects R3c — Post-batch
///   eligibility, CR reference and lost controller for the retained-battle request"). After the
///   pass succeeded each chosen
///   protector is recorded through [`SbaEnvironment::set_battle_protector`] and reported in
///   [`SbaOutcome::protectors_chosen`]. Changing the protector of a battle that is being
///   attacked removes it from combat (CR 506.4, CR 506.4c: it stops being attacked; its
///   attackers stay attacking creatures that attack nothing); the combat code applies that
///   from [`SbaOutcome::protectors_chosen`], the pass does not touch combat state.
/// - **Speed** (CR 704.5aa, CR 702.179a): each player found gets the [`SPEED`] designation
///   `Int(1)` once, reported in [`SbaOutcome::speed_set`]; not an event.
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
/// counter removal or an unattach event occurred, an object ceased to exist
/// or was removed from the game, a player lost, a protector was chosen, a speed was set, or the
/// replacement registry changed
/// (for example a one-use effect was consumed). A finding whose only move a replacement
/// skipped is reported in [`SbaOutcome::findings`] but is not performed; the heartbeat
/// repeats the check while `performed` (CR 704.3).
///
/// # Errors
/// [`BusInterrupt::Pending`] when a player must choose a replacement effect, the legend-rule
/// survivors or a battle protector (propagate it to the [`ChoiceSession`](crate::choice::ChoiceSession), which
/// re-runs the pass from the base state); [`BusInterrupt::Error`] on a failure, including a
/// legend-rule answer that is not a selection or not a maximal set of candidates no two of
/// which have the same name, or a protector answer that is not a candidate ([`ChoiceError::Invalid`] with the request's id, which a
/// [`ChoiceSession`](crate::choice::ChoiceSession) rejects without recording, the request
/// staying pending). In both cases every component of `ctx`, the replacement registry and the flags of `env` are exactly as before the call.
///
/// # Layer inputs after the batch
/// `perform_all` reads `effects` and `layer_env` for every view of the pass, including the
/// post-batch view of the retained zero-defense battles (see "Battle protectors"): that step
/// then evaluates the state after the batch with the caller's PRE-batch layer inputs. A caller
/// whose layer inputs depend on state the batch can change (an attachment the batch unattaches,
/// counters it removes, an object it moves) calls [`perform_all_relayered`] instead and
/// supplies the layer inputs of the state after the batch; a caller whose protector facts
/// ([`SbaEnvironment::battle_protector`], [`SbaEnvironment::can_be_protector`]) also depend on
/// such state calls [`perform_all_post_batch`] and supplies a read-only view of them too.
pub fn perform_all(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &mut dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    perform_all_relayered(
        registry,
        ctx,
        replacements,
        effects,
        layer_env,
        env,
        choices,
        &mut |_, view| view(effects, layer_env),
    )
}

/// The provider of the layer inputs of the state after the batch of a pass, for
/// [`perform_all_relayered`] (REQ-SBA-001, REQ-LAYER-001; CR 613.1, CR 613.4: characteristics
/// after every layer).
///
/// The engine calls it with the [`EventContext`] as it is after the batch (no mutable borrow is
/// held then); it must call the second argument exactly once with the continuous effects and
/// the layer environment that describe that state. The inputs never leave the call, so they may
/// borrow from the provider's own snapshot of the context.
pub type PostBatchLayers<'a> =
    &'a mut dyn FnMut(&EventContext<'_>, &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment));

/// [`perform_all`] with the layer inputs of the state after the batch supplied by `post_batch`
/// (REQ-SBA-001, CR 704.3, CR 704.5x, CR 614.6).
///
/// Everything [`perform_all`] documents holds unchanged: the check, the plan, the pre-batch
/// requests (request ids included) and the batch read `effects` and `layer_env`. Only the
/// post-batch protector step of the retained zero-defense battles ("Battle protectors" in
/// [`perform_all`]) reads the inputs `post_batch` supplies, for both the test that the battle
/// is still a CR 704.5x or CR 704.5y subject and its request (the chooser, its controller, and
/// the candidates).
///
/// `post_batch` is called at most once per pass, after the batch, and only when the pass has a
/// retained battle that is still on the battlefield; an ordinary pass never calls it. It must
/// be a pure function of the context it receives (R3, REQ-CORE-001: no clock, I/O, global or
/// environment read), so that re-running the pass from the same state (a
/// [`ChoiceSession`](crate::choice::ChoiceSession) replay) gives the same requests.
///
/// # Errors
/// As [`perform_all`]; a pending or failed post-batch request leaves every component of `ctx`,
/// the replacement registry and the flags of `env` exactly as before the call.
#[allow(clippy::too_many_arguments)]
pub fn perform_all_relayered(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &mut dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
    post_batch: PostBatchLayers<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // The pass's own `env` stays the protector view: the provider supplies layer inputs only.
    perform_pass(
        registry,
        ctx,
        replacements,
        effects,
        layer_env,
        env,
        choices,
        &mut |after, view| post_batch(after, &mut |e, l| view(e, l, None)),
    )
}

/// The provider of the state after the batch of a pass, for [`perform_all_post_batch`]: its
/// layer inputs and a read-only view of its protector facts (REQ-SBA-001, REQ-LAYER-001;
/// CR 613.1, CR 613.4: characteristics after every layer; CR 704.5x, CR 704.5y).
///
/// The engine calls it with the [`EventContext`] as it is after the batch (no mutable borrow is
/// held then); it must call the second argument exactly once with the continuous effects, the
/// layer environment and an [`SbaEnvironment`] that describe that state. The engine reads only
/// [`SbaEnvironment::battle_protector`] and [`SbaEnvironment::can_be_protector`] from that
/// environment, never writes through it, and never keeps it past the call, so it may borrow
/// from the provider's own snapshot of the caller state. Which entries are active (the game
/// variant, [`SbaEnvironment::variant`]) is read from the pass's original `env`, never from this
/// view.
pub type PostBatchState<'a> = &'a mut dyn FnMut(
    &EventContext<'_>,
    &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment, &dyn SbaEnvironment),
);

/// [`perform_all_relayered`] whose provider also supplies a read-only view of the protector
/// facts of the state after the batch (REQ-SBA-001, REQ-LAYER-001; CR 704.3, CR 704.5x,
/// CR 704.5y, CR 614.6).
///
/// Everything [`perform_all_relayered`] documents holds unchanged; in addition, the post-batch
/// protector step of the retained zero-defense battles ("Battle protectors" in [`perform_all`])
/// reads each battle's protector ([`SbaEnvironment::battle_protector`]) and who can be its
/// protector ([`SbaEnvironment::can_be_protector`]) from the [`SbaEnvironment`] the provider
/// supplies, both for the test that the battle is still a CR 704.5x or CR 704.5y subject and for
/// its candidates. A caller whose protector facts depend on state the batch can change (an
/// attachment the batch unattaches) uses this entry point.
///
/// `post_batch` is called at most once per pass, after the batch, and only when the pass has a
/// retained battle that is still on the battlefield; an ordinary pass never calls it. It must
/// be a pure function of the context it receives (R3, REQ-CORE-001: no clock, I/O, global or
/// environment read). The view it supplies is read-only: every write (the chosen protectors
/// through [`SbaEnvironment::set_battle_protector`], the deathtouch flag) goes to `env`, and
/// only after the pass succeeded.
///
/// # Errors
/// As [`perform_all`]; a pending or failed post-batch request leaves every component of `ctx`,
/// the replacement registry and the flags of `env` exactly as before the call.
#[allow(clippy::too_many_arguments)]
pub fn perform_all_post_batch(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &mut dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
    post_batch: PostBatchState<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    perform_pass(
        registry,
        ctx,
        replacements,
        effects,
        layer_env,
        env,
        choices,
        &mut |after, view| post_batch(after, &mut |e, l, s| view(e, l, Some(s))),
    )
}

/// The engine's post-batch provider: the layer inputs of the state after the batch and, when
/// the caller supplies one, a read-only protector view of it (`None`: the pass's own `env`).
type PostBatchInputs<'a> = &'a mut dyn FnMut(
    &EventContext<'_>,
    &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment, Option<&dyn SbaEnvironment>),
);

/// The one pass behind [`perform_all`], [`perform_all_relayered`] and
/// [`perform_all_post_batch`] (CR 704.3).
#[allow(clippy::too_many_arguments)]
fn perform_pass(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    effects: &ContinuousEffects,
    layer_env: &dyn LayerEnvironment,
    env: &mut dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
    post_batch: PostBatchInputs<'_>,
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
        let plan = Plan::new(registry, &view, ctx.game, &findings);
        (findings, plan)
    };
    let saved = Saved::take(ctx);
    let replacements_before = replacements.clone();
    match perform_findings(
        registry,
        ctx,
        replacements,
        findings,
        &plan,
        post_batch,
        &*env,
        choices,
    ) {
        Ok(mut outcome) => {
            // CR 704.3: "If any state-based actions are performed as a result of a check, the
            // check is repeated": performed means the pass changed the state.
            outcome.performed |= *replacements != replacements_before;
            // CR 704.5x, CR 704.5y, CR 310.9f: record each chosen protector, ascending by
            // battle, only now that the pass succeeded.
            for &(battle, protector) in &outcome.protectors_chosen {
                env.set_battle_protector(battle, protector);
            }
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
///
/// The requests of `plan` are asked before any state changes; then the one batch runs; then
/// (CR 704.5x, CR 704.5y, CR 614.6) each battle of [`Plan::retained_battles`] that is still on
/// the battlefield as the same object and still a CR 704.5x or CR 704.5y subject in a fresh view
/// of the state after the batch (built from the post-batch layer inputs and, when supplied, the
/// read-only protector view that `post_batch` supplies; `env` otherwise; see
/// [`ask_retained_protectors`]) is asked its protector choice, with the CR reference of the
/// entry that flags it after the batch, in the order of the pre-batch requests (the
/// controller's APNAP rank, then ascending battle id); a battle whose controller has lost the
/// game gets none. A pending or failed request leaves the rollback to the caller like every
/// other.
#[allow(clippy::too_many_arguments)]
fn perform_findings(
    registry: &SbaRegistry,
    ctx: &mut EventContext<'_>,
    replacements: &mut ReplacementRegistry,
    findings: Vec<SbaFinding>,
    plan: &Plan,
    post_batch: PostBatchInputs<'_>,
    env: &dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // CR 704.5j, CR 704.5x, CR 704.5y, CR 310.11: every legend-rule and protector choice is
    // asked before any state changes, so a pending choice leaves the state as it was.
    // CR 101.4: one sequence for the pass, in APNAP order of the choosing players (decision
    // "Gate 02-state-objects F2 — One APNAP request sequence per SBA pass"); all moves happen together after every answer.
    let mut legend_losers: BTreeSet<ObjectId> = BTreeSet::new();
    let mut protectors_chosen: Vec<(ObjectId, PlayerId)> = Vec::new();
    for &request in &plan.requests {
        match request {
            PlannedRequest::Legend(i) => {
                let group = &plan.legend_groups[i];
                let kept = ask_legend_choice(choices, group)?;
                legend_losers.extend(
                    group
                        .members
                        .iter()
                        .copied()
                        .filter(|id| !kept.contains(id)),
                );
            }
            PlannedRequest::Protector(i) => {
                let ask = &plan.protector_asks[i];
                if let Some(controller) = ask.controller {
                    let chosen = ask_protector_choice(choices, ask, controller)?;
                    protectors_chosen.push((ask.battle, chosen));
                }
            }
        }
    }
    // CR 704.5x, CR 704.5y: a battle no player can protect is put into its owner's graveyard
    // with the batch, without a request.
    let unprotectable: BTreeSet<ObjectId> = plan
        .protector_asks
        .iter()
        .filter(|a| a.controller.is_none() || a.candidates.is_empty())
        .map(|a| a.battle)
        .collect();

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
    // CR 704.5x, CR 704.5y: no player can be chosen: its owner's graveyard.
    for &o in &unprotectable {
        moving.entry(o).or_insert(ZoneChangeCause::StateBasedAction);
    }
    // CR 704.5aa: the players whose speed becomes 1 (once per player).
    let mut speed_set: BTreeSet<PlayerId> = BTreeSet::new();
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
            // CR 704.5aa, CR 702.179a
            (SbaAction::SetSpeed, SbaSubject::Player(p)) => {
                speed_set.insert(p);
            }
            // CR 704.5j (asked above) and CR 704.5q, CR 704.5r (from the plan).
            _ => {}
        }
    }
    // Ceasing wins over a move of the same object.
    moving.retain(|o, _| !ceasing.contains_key(o));
    // CR 704.5t: an object that ceases to exist (CR 704.5d, CR 704.5e) only ceases.
    dungeons.retain(|o| !ceasing.contains_key(o));
    // CR 704.3: every applicable state-based action is performed in the one event, so a
    // permanent that also moves still gets its counter removals and its unattach event
    // (decision "#42 FP-041 — One SBA batch: moves, counter removals and unattach events").
    // Only an object that ceases to exist or a dungeon removed from the game gets none.
    let excluded = |o: &ObjectId| ceasing.contains_key(o) || dungeons.contains(o);
    // CR 701.3d, CR 603.10c: one unattach event per attachment (the set merges a CR 704.5n and
    // a CR 704.5p finding of one attachment).
    unattaching.retain(|o| !excluded(o));

    // CR 704.3, CR 704.7: one batch. Unattach events ascending id, then zone changes ascending
    // object id, then counter removals ascending object id then kind.
    // CR 704.5n, CR 704.5p, CR 701.3d: unattached, it remains on the battlefield. The bus
    // executes every event that is not a zone change before every zone change of the batch,
    // and captures last known information before any event runs, so the unattach event of an
    // attachment whose host (or which itself) leaves in this pass finds the attachment from
    // before the pass, and the look-back of "becomes unattached" triggers (CR 603.10c) sees it
    // (CR 704.8). The host's zone change then has nothing left to unattach: one event per pair.
    // The unattach events are submitted FIRST: the bus executes the events that are not zone
    // changes in submission order, an `Instead` descendant at the position of the event it
    // replaces (CR 614.6), so each planned unattach clears the link from before the pass, and
    // an `Attach` a replacement makes of a later event of the batch (a zone change or a counter
    // removal) is not cleared by it (decision "#42 FP-041 — Planned unattach and pre-pass host
    // (round 3)").
    let mut moves: Vec<GameEvent> = unattaching
        .iter()
        .map(|&attachment| GameEvent::Unattach { attachment })
        .collect();
    moves.extend(
        moving
            .iter()
            .map(|(&object, &cause)| GameEvent::ZoneChange {
                object,
                // CR 704.5f-704.5k, CR 704.5m, CR 704.5s: the owner's graveyard (CR 400.3).
                request: MoveRequest::new(ZoneKind::Graveyard),
                cause,
            }),
    );
    for (&object, removals) in &plan.counter_removals {
        if excluded(&object) {
            continue;
        }
        for (kind, &amount) in removals {
            if amount > 0 {
                // CR 704.5q, CR 704.5r, CR 122.3 (CR 122.2: counters left on an object that
                // moves simply cease to exist; they get no removal event).
                moves.push(GameEvent::RemoveCounters {
                    target: CounterTarget::Object(object),
                    kind: kind.clone(),
                    amount,
                });
            }
        }
    }

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
        // A dungeon removed from the game (CR 704.5t) should go outside the game and stay
        // available to a later venture (CR 309.2a, CR 400.11, CR 701.49c); retiring it here is
        // a placeholder until the dungeon outside-the-game pool lands (issue #215, decision
        // "#42 FP-041 — Removing a dungeon from the game").
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

    // CR 704.5n, CR 704.5p, CR 701.3d: "It remains on the battlefield": the permanents that
    // every Unattach event of the batch, planned or produced by a replacement, unattached and
    // that are still on the battlefield as the same object at the end of the pass (decision
    // #42 FP-041, D2). An unattach that found nothing attached is no event (the bus records
    // only events that occurred), and an executed one reports `Outcome::Applied`. The event
    // itself stays in `executed` either way.
    let unattached: Vec<ObjectId> = executed
        .iter()
        .flat_map(|b| &b.events)
        .filter_map(|ev| match (ev.instance.event(), ev.outcome) {
            (GameEvent::Unattach { attachment }, Outcome::Applied)
                if ctx
                    .zones
                    .object(*attachment)
                    .is_some_and(|r| r.zone == ZoneId::Battlefield) =>
            {
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
    // CR 704.5aa, CR 702.179a: "that player's speed becomes 1" (decision #43 FP-042: the
    // `speed` designation, `Int(1)`). Not an event.
    for &player in &speed_set {
        if let Some(mut p) = ctx.game.player_mut(player) {
            p.set_designation(SPEED, DesignationValue::Int(1));
        }
    }

    // CR 704.5x, CR 704.5y, CR 614.6: a battle at defense 0 whose graveyard move was replaced
    // and that stays without a protector gets its protector choice now, after the batch.
    protectors_chosen.extend(ask_retained_protectors(
        registry, ctx, plan, post_batch, env, choices,
    )?);
    protectors_chosen.sort_unstable();

    // CR 704.3: performed means the state changed (perform_all adds a registry change).
    let performed = !executed.is_empty()
        || !ceased.is_empty()
        || !loss_reasons.is_empty()
        || !unattached.is_empty()
        || !removed_from_game.is_empty()
        || !protectors_chosen.is_empty()
        || !speed_set.is_empty();
    Ok(SbaOutcome {
        performed,
        lost_players: loss_reasons.keys().copied().collect(),
        loss_reasons,
        findings,
        executed,
        ceased,
        unattached,
        removed_from_game,
        protectors_chosen,
        speed_set: speed_set.into_iter().collect(),
    })
}

/// CR 704.5x, CR 704.5y, CR 704.5v, CR 704.5w, CR 614.6: the protector choices of the battles of
/// [`Plan::retained_battles`] after the batch. A battle that left the battlefield (another id or
/// zone) gets none (CR 704.8: what it was is last known information from before the pass); a
/// battle that is still on the battlefield as the same object and is still a subject of a
/// CR 704.5x or CR 704.5y entry in a view of the state after the batch is asked like a pre-batch
/// choice ([`ask_protector_choice`]; controller and candidates from that view; the reason the
/// CR reference of the CR 704.5x or CR 704.5y entry that flags it in that view, the pre-batch
/// plan only selecting which battles were deferred), ordered by the controller's APNAP rank,
/// then ascending battle id ([`protector_asks`]). A battle without a controller, whose
/// controller has lost the game (the [`LOST_THE_GAME`] designation after the batch: it can't
/// answer), or without a candidate gets no request and consumes no request id: the next pass
/// finds it again (CR 704.5x). The pre-batch requests make the same controller check on the
/// state before the batch ([`can_choose_protector`]).
///
/// The view is built from the post-batch layer inputs (CR 613.1, CR 613.4: characteristics after
/// every layer) that `post_batch` supplies for `ctx` as it is after the batch, so a controller
/// or counters that the batch changed through a layer input are read as they are now; the
/// protectors and who can be one are read from the read-only [`SbaEnvironment`] view it
/// supplies ([`perform_all_post_batch`]), or from `env` when it supplies none
/// ([`perform_all`], [`perform_all_relayered`]). Nothing is written through either. Called at
/// most once, and only when a retained battle is still on the battlefield.
///
/// # Errors
/// [`BusInterrupt::Pending`] until every request is answered; [`BusInterrupt::Error`] if an
/// answer is not a candidate. The caller restores the state.
fn ask_retained_protectors(
    registry: &SbaRegistry,
    ctx: &EventContext<'_>,
    plan: &Plan,
    post_batch: PostBatchInputs<'_>,
    env: &dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<Vec<(ObjectId, PlayerId)>, BusInterrupt> {
    // CR 704.8: a retained battle that left the battlefield gets nothing; with none left the
    // provider is not called.
    let on_battlefield = plan.retained_battles.keys().any(|&b| {
        ctx.zones
            .object(b)
            .is_some_and(|r| r.zone == ZoneId::Battlefield)
    });
    if !on_battlefield {
        return Ok(Vec::new());
    }
    let mut asks: Vec<ProtectorAsk> = Vec::new();
    post_batch(ctx, &mut |effects, layer_env, protector_view| {
        // CR 704.5x, CR 704.5y: a fresh view of the state after the batch, through its own
        // layer inputs and, when the caller supplies one, its read-only protector view (never
        // written through), used only for the retained battles.
        let facts: &dyn SbaEnvironment = protector_view.unwrap_or(env);
        let view = SbaView::new(
            ctx.game,
            ctx.zones,
            ctx.events,
            ctx.permanents,
            effects,
            layer_env,
            facts,
        );
        let variant = env.variant();
        // CR 704.5x, CR 704.5y: each battle with the CR reference of the post-batch entry that
        // flags it (the two never flag the same battle; if they did, the first entry in
        // registry order would win).
        let mut unprotected: BTreeMap<ObjectId, &'static str> = BTreeMap::new();
        for entry in registry
            .entries()
            .iter()
            .filter(|entry| entry.action == SbaAction::ChooseProtector && entry.active_in(variant))
        {
            for subject in entry.subjects(&view) {
                if let SbaSubject::Object(o) = subject {
                    unprotected.entry(o).or_insert(entry.cr_ref);
                }
            }
        }
        // The pre-batch plan only selects which battles were deferred.
        let still: BTreeMap<ObjectId, &'static str> = plan
            .retained_battles
            .keys()
            .filter_map(|b| unprotected.get(b).map(|&cr| (*b, cr)))
            .collect();
        asks = protector_asks(&view, &still);
    });
    let mut chosen = Vec::new();
    for ask in &asks {
        // A controller who has lost the game (the LOST_THE_GAME designation) can't answer: like
        // a battle without a controller, no request; the next pass finds the battle again.
        if let Some(controller) = ask.controller
            && !ask.candidates.is_empty()
            && can_choose_protector(ctx.game, controller)
        {
            chosen.push((ask.battle, ask_protector_choice(choices, ask, controller)?));
        }
    }
    Ok(chosen)
}

/// CR 704.5x, CR 704.5y, CR 800.4a: whether `controller`, the controller of a battle, can be
/// asked its protector choice: it is a player of `game` without the [`LOST_THE_GAME`]
/// designation. CR 704.5x and CR 704.5y name "that battle's controller" as the chooser; CR 800.4a
/// removes the battle when that player leaves (performed by the caller), and CR 800.4h sends a
/// choice that survives the departure to the next player in turn order (not modelled, #125). One test for the pre-batch requests ([`Plan`]) and the post-batch requests
/// ([`ask_retained_protectors`]) (decisions "Gate 02-state-objects R3c — Post-batch eligibility,
/// CR reference and lost controller for the retained-battle request" and "Gate 02-state-objects
/// R3d — No protector request to a controller who lost the game, on every path").
fn can_choose_protector(game: &GameState, controller: PlayerId) -> bool {
    game.player(controller)
        .is_some_and(|p| p.designation(LOST_THE_GAME).is_none())
}

/// CR 704.5x, CR 704.5y, CR 310.11: asks `controller`, the controller of the battle of `ask`,
/// to choose its protector as ONE `CHOOSE_PLAYER` choice: candidates the appropriate players
/// ascending, min 1, max 1, default the lowest candidate.
///
/// # Errors
/// [`BusInterrupt::Pending`] until answered; [`BusInterrupt::Error`] with
/// [`ChoiceError::Invalid`] (this request's id) if the answer is not a candidate.
fn ask_protector_choice(
    choices: &mut ChoiceContext<'_>,
    ask: &ProtectorAsk,
    controller: PlayerId,
) -> Result<PlayerId, BusInterrupt> {
    let request_id = choices.next_request_id();
    let lowest = ask.candidates[0];
    let request = PendingChoiceRequest {
        request_id,
        chooser: controller,
        choice_type: ChoiceType::ChoosePlayer,
        reason: ChoiceReason {
            cr: ask.cr.to_owned(),
            // The battle whose protector is chosen: two requests with the same candidates are
            // told apart by it.
            source: Some(ask.battle),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitPlayer { player: lowest },
        options: ChoiceOptions::Enumerated(
            ask.candidates
                .iter()
                .map(|&p| ChoiceOption::Player(p))
                .collect(),
        ),
        payload: ChoicePayload::ChoosePlayer {
            players: ask.candidates.clone(),
        },
    };
    protector_answer(ask, request_id, choices.ask(request)?)
}

/// CR 704.5x, CR 704.5y: the protector the answer `response` to request `request_id` chooses
/// for the battle of `ask`.
///
/// # Errors
/// [`BusInterrupt::Error`] with [`ChoiceError::Invalid`] naming `request_id` if the answer is
/// not a `SubmitPlayer` of a candidate (the convention of [`legend_survivors`]: the session
/// rejects it without recording it).
pub(super) fn protector_answer(
    ask: &ProtectorAsk,
    request_id: RequestId,
    response: ChoiceResponse,
) -> Result<PlayerId, BusInterrupt> {
    let protocol = |explanation: String| {
        BusInterrupt::Error(BusError::Choice(ChoiceError::Invalid {
            request_id,
            explanation,
        }))
    };
    match response {
        ChoiceResponse::SubmitPlayer { player } if ask.candidates.contains(&player) => Ok(player),
        ChoiceResponse::SubmitPlayer { player } => Err(protocol(format!(
            "{player:?} can't be the protector of {:?}",
            ask.battle
        ))),
        other => Err(protocol(format!("unexpected answer {}", other.wire_name()))),
    }
}

/// CR 704.5j, CR 201.2a: asks the controller of `group` which permanents stay, as ONE
/// `SELECT_OBJECTS_FROM_ZONE` choice: candidates the group ascending, min 1, max 1 for a group
/// whose members all have the same name and otherwise the group size minus 1; on timeout the
/// greedy maximal set by ascending id ([`LegendGroup::default_survivors`]).
///
/// # Errors
/// [`BusInterrupt::Pending`] until answered; [`BusInterrupt::Error`] with
/// [`ChoiceError::Invalid`] (this request's id) if the answer is not a valid survivor set.
fn ask_legend_choice(
    choices: &mut ChoiceContext<'_>,
    group: &LegendGroup,
) -> Result<Vec<ObjectId>, BusInterrupt> {
    let request_id = choices.next_request_id();
    let request = PendingChoiceRequest {
        request_id,
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
    legend_survivors(group, request_id, choices.ask(request)?)
}

/// CR 704.5j, CR 201.2a: the permanents of `group` the answer `response` to request
/// `request_id` keeps.
///
/// # Errors
/// [`BusInterrupt::Error`] with [`ChoiceError::Invalid`] naming `request_id` if the answer is
/// not a selection, or not a maximal set of candidates no two of which have the same name
/// ([`LegendGroup::check_survivors`]). The procedure-level validation convention of decision
/// "#28 FP-027 — Card-name validation is structural": converted to an
/// [`Interrupt`](crate::choice::Interrupt), the error keeps its class, so
/// [`ChoiceSession::submit`](crate::choice::ChoiceSession::submit) rejects the answer without
/// recording it and the request stays pending.
pub(super) fn legend_survivors(
    group: &LegendGroup,
    request_id: RequestId,
    response: ChoiceResponse,
) -> Result<Vec<ObjectId>, BusInterrupt> {
    let protocol = |explanation: String| {
        BusInterrupt::Error(BusError::Choice(ChoiceError::Invalid {
            request_id,
            explanation,
        }))
    };
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
