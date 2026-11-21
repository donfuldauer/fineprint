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
    EventBatch, EventContext, EventError, EventState, ExecutedBatch, GameEvent, ZoneChangeCause,
};
use crate::layers::{ContinuousEffects, LayerEnvironment};
use crate::memory::{LkiStore, MemoryTable};
use crate::permanent::Permanents;
use crate::replacement::{BusError, BusInterrupt, ReplacementRegistry, process_batch};
use crate::state::{DesignationValue, GameState};
use crate::zones::{MoveRequest, ZoneKind, Zones};

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
struct Saved {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
}

impl Saved {
    fn take(ctx: &EventContext<'_>) -> Saved {
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
/// - **Ceasing to exist** (CR 704.5d, CR 704.5e): after the batch, each such object still
///   present ceases to exist through [`Zones::cease_to_exist`](crate::zones::Zones::cease_to_exist)
///   (not a zone change, no LKI); a permanent among them also leaves the permanent container.
///   An object that is both a move target and ceasing only ceases.
/// - **Losing the game** (CR 704.5a, CR 704.5b, CR 704.5c): each losing player gets the
///   [`LOST_THE_GAME`] designation once, whatever the number of reasons (CR 704.7); losses of
///   different players are never merged. Deciding a draw or a winner (CR 104.4a) is the
///   caller's.
///
/// The empty-library-draw flag (CR 704.5b, "since the last time state-based actions were
/// checked") is consumed by every successful call, whether or not a player loses; the
/// deathtouch flag of `env` is cleared by [`SbaEnvironment::clear_deathtouch_damaged`] after
/// every successful call.
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
    match perform_findings(registry, ctx, replacements, findings, choices) {
        Ok(outcome) => {
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
    choices: &mut ChoiceContext<'_>,
) -> Result<SbaOutcome, BusInterrupt> {
    // CR 704.5b, CR 121.4: the check consumes the flag. Taken before the batch so that an
    // empty-library draw made by a replacement during the batch counts for the next check.
    ctx.events.take_empty_library_draws();

    let mut loss_reasons: BTreeMap<PlayerId, Vec<&'static str>> = BTreeMap::new();
    let mut ceasing: BTreeSet<ObjectId> = BTreeSet::new();
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
                ceasing.insert(o);
            }
            (SbaAction::PutIntoGraveyard | SbaAction::Destroy, SbaSubject::Object(o)) => {
                moving.insert(o);
            }
            _ => {}
        }
    }
    // Ceasing wins over a move of the same object.
    let moves: Vec<GameEvent> = moving
        .difference(&ceasing)
        .map(|&object| GameEvent::ZoneChange {
            object,
            // CR 704.5f, CR 704.5g, CR 704.5h, CR 704.5i: the owner's graveyard (CR 400.3).
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause: ZoneChangeCause::StateBasedAction,
        })
        .collect();

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

    // CR 704.5d, CR 704.5e: ceasing to exist is not a zone change.
    let mut ceased = Vec::new();
    for id in ceasing {
        if ctx.zones.object(id).is_none() {
            continue;
        }
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

    // CR 704.5a, CR 704.5b, CR 704.5c: the player loses the game.
    for &player in loss_reasons.keys() {
        if let Some(mut p) = ctx.game.player_mut(player) {
            p.set_designation(LOST_THE_GAME, DesignationValue::Flag);
        }
    }

    Ok(SbaOutcome {
        performed: !findings.is_empty(),
        lost_players: loss_reasons.keys().copied().collect(),
        loss_reasons,
        findings,
        executed,
        ceased,
    })
}
