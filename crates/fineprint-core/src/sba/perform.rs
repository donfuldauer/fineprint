// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! One state-based action pass (REQ-SBA-001, CR 704.3): check every entry against one
//! snapshot, then perform every result simultaneously as one event.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};

use super::registry::{SbaAction, SbaRegistry, SbaView};
use super::types::{LOST_THE_GAME, SbaEnvironment, SbaFinding, SbaOutcome, SbaSubject};
use crate::choice::ChoiceContext;
use crate::events::{
    EventBatch, EventContext, EventError, EventState, ExecutedBatch, GameEvent, ObjectMarkers,
    Outcome, ZoneChangeCause,
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
/// checked by the object entries CR 704.5f-704.5i.
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
    let variant = env.variant();
    let mut findings = Vec::new();
    for entry in registry.entries() {
        if !entry.active_in(variant) {
            continue;
        }
        for subject in entry.subjects(&view) {
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

/// Performs one state-based action pass (REQ-SBA-001, CR 704.3, CR 704.7, CR 704.8): every
/// applicable state-based action is found by [`check`] on one snapshot, before anything
/// changes, then all are performed simultaneously as one event. The heartbeat repeats the
/// pass while it performs something (CR 704.3); this is one pass.
///
/// - **Moves** (CR 704.5f, CR 704.5g, CR 704.5h, CR 704.5i): ONE batch with one
///   `ZoneChange` to the owner's graveyard per object, cause
///   [`ZoneChangeCause::StateBasedAction`], ascending object id (an object with several
///   findings moves once), submitted through [`process_batch`] with `sba` true: replacement
///   effects see the merged same-object group (CR 704.7) and last known information of every
///   moved object is captured before any move (CR 704.8).
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
/// [`SbaOutcome::performed`] is `true` exactly when the pass changed the game state: a move
/// occurred, an object ceased to exist, a player lost, or the replacement registry changed
/// (for example a one-use effect was consumed). A finding whose only move a replacement
/// skipped is reported in [`SbaOutcome::findings`] but is not performed; the heartbeat
/// repeats the check while `performed` (CR 704.3).
///
/// # Errors
/// [`BusInterrupt::Pending`] when a player must choose a replacement effect (propagate it to
/// the [`ChoiceSession`](crate::choice::ChoiceSession), which re-runs the pass from the base
/// state); [`BusInterrupt::Error`] on a failure. In both cases every component of `ctx`, the
/// replacement registry and the flags of `env` are exactly as before the call.
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
    let findings = check(registry, ctx, effects, layer_env, &*env);
    let saved = Saved::take(ctx);
    let replacements_before = replacements.clone();
    match perform_findings(registry, ctx, replacements, findings, &*env, choices) {
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
    env: &dyn SbaEnvironment,
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // CR 704.5b, CR 121.4: the check consumes the flag. Taken before the batch so that an
    // empty-library draw made by a replacement during the batch counts for the next check.
    ctx.events.take_empty_library_draws();

    let mut loss_reasons: BTreeMap<PlayerId, Vec<&'static str>> = BTreeMap::new();
    let mut ceasing: BTreeMap<ObjectId, BTreeSet<CeaseReason>> = BTreeMap::new();
    let mut moving: BTreeSet<ObjectId> = BTreeSet::new();
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
                moving.insert(o);
            }
            _ => {}
        }
    }
    // Ceasing wins over a move of the same object.
    let moves: Vec<GameEvent> = moving
        .iter()
        .filter(|o| !ceasing.contains_key(o))
        .map(|&object| GameEvent::ZoneChange {
            object,
            // CR 704.5f, CR 704.5g, CR 704.5h, CR 704.5i: the owner's graveyard (CR 400.3).
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause: ZoneChangeCause::StateBasedAction,
        })
        .collect();

    // CR 704.8: the batch may change the state (a removed permanent unattaches what is attached
    // to it). The permanent state and markers of an object that only ceases are read from here.
    // Only a snapshot: nothing is inserted into the live LKI store before the bus runs (an
    // `Instead` moving a ceasing object would make the bus fail with `AlreadyCaptured`).
    let pre_batch = (!moves.is_empty() && !ceasing.is_empty())
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
    let mut targets: Vec<(ObjectId, bool)> = Vec::new();
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
        targets.push((id, id != old));
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
    let live = (pre_batch.is_none() || targets.iter().any(|&(_, moved)| moved))
        .then(|| (ctx.permanents.clone(), ctx.events.markers().clone()));
    for &(id, moved) in &targets {
        let (permanents, markers) = match (&pre_batch, &live, moved) {
            (Some(pre), _, false) => pre,
            (_, Some(now), _) => now,
            // `live` is built whenever `pre_batch` is `None` or an object moved.
            _ => continue,
        };
        capture_ceasing_lki(ctx, id, permanents, markers)?;
    }
    let mut ceased = Vec::new();
    for (id, _) in targets {
        if ctx.permanents.get(id).is_some() {
            ctx.permanents
                .leave(id)
                .map_err(|e| BusInterrupt::Error(BusError::Event(EventError::from(e))))?;
        }
        ctx.zones
            .cease_to_exist(id)
            .map_err(|e| BusInterrupt::Error(BusError::Event(EventError::from(e))))?;
        ceased.push(id);
    }
    ceased.sort_unstable();

    // CR 704.5a, CR 704.5b, CR 704.5c: the player loses the game.
    for &player in loss_reasons.keys() {
        if let Some(mut p) = ctx.game.player_mut(player) {
            p.set_designation(LOST_THE_GAME, DesignationValue::Flag);
        }
    }

    // CR 704.3: performed means the state changed (perform_all adds a registry change).
    let performed = !executed.is_empty() || !ceased.is_empty() || !loss_reasons.is_empty();
    Ok(SbaOutcome {
        performed,
        lost_players: loss_reasons.keys().copied().collect(),
        loss_reasons,
        findings,
        executed,
        ceased,
    })
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
