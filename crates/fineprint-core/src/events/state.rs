// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Standalone events-owned state: object markers, the executed-event log and the event state
//! (REQ-EVENT-001, REQ-DSL-003).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{CardId, ObjectId, PlayerId};

use super::history::History;
use super::types::{ExecutedBatch, Phase, Step};

/// Per-object state that events change and no other container holds yet (REQ-EVENT-001):
/// counters on objects (CR 122.1), damage marked on permanents (CR 120.3e), which card ids are
/// tokens (CR 111.6), and copy-of links (CR 707.2).
///
/// Counters, marked damage and the copy link of an object cease to exist when it changes zones
/// (CR 122.2, CR 400.7). Every collection is ordered; cloning is a cheap snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectMarkers {
    counters: Arc<BTreeMap<ObjectId, BTreeMap<String, i64>>>,
    damage: Arc<BTreeMap<ObjectId, i64>>,
    tokens: Arc<BTreeSet<CardId>>,
    copies: Arc<BTreeMap<ObjectId, ObjectId>>,
}

impl ObjectMarkers {
    /// The counters on `id`: kind to amount, ascending kind, amounts at least 1 (CR 122.1).
    #[must_use]
    pub fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    /// The number of counters of `kind` on `id` (0 if none).
    #[must_use]
    pub fn counter(&self, id: ObjectId, kind: &str) -> i64 {
        self.counters
            .get(&id)
            .and_then(|m| m.get(kind))
            .copied()
            .unwrap_or(0)
    }

    /// The damage marked on `id` (CR 120.3e); 0 if none.
    #[must_use]
    pub fn damage(&self, id: ObjectId) -> i64 {
        self.damage.get(&id).copied().unwrap_or(0)
    }

    /// Removes all damage marked on every permanent, including phased-out permanents (the
    /// cleanup step, CR 514.2).
    pub fn clear_all_damage(&mut self) {
        // CR 514.2
        if !self.damage.is_empty() {
            self.damage = Arc::new(BTreeMap::new());
        }
    }

    /// Whether `card` was created as a token (CR 111.6: a token isn't a card; the id names
    /// its physical stand-in).
    #[must_use]
    pub fn is_token(&self, card: CardId) -> bool {
        self.tokens.contains(&card)
    }

    /// The object `id` is a copy of, if it became a copy (CR 707.2, CR 613.1a).
    #[must_use]
    pub fn copy_of(&self, id: ObjectId) -> Option<ObjectId> {
        self.copies.get(&id).copied()
    }

    /// Sets the count of `kind` on `id`; 0 or less removes the entry.
    pub(super) fn set_counter(&mut self, id: ObjectId, kind: &str, amount: i64) {
        let counters = Arc::make_mut(&mut self.counters);
        let entry = counters.entry(id).or_default();
        if amount <= 0 {
            entry.remove(kind);
        } else {
            entry.insert(kind.to_owned(), amount);
        }
        if entry.is_empty() {
            counters.remove(&id);
        }
    }

    /// Sets the damage marked on `id` (at least 1).
    pub(super) fn set_damage(&mut self, id: ObjectId, amount: i64) {
        Arc::make_mut(&mut self.damage).insert(id, amount);
    }

    /// Marks `card` as a token.
    pub(super) fn mark_token(&mut self, card: CardId) {
        Arc::make_mut(&mut self.tokens).insert(card);
    }

    /// Records that `id` is a copy of `copied`.
    pub(super) fn set_copy(&mut self, id: ObjectId, copied: ObjectId) {
        Arc::make_mut(&mut self.copies).insert(id, copied);
    }

    /// Forgets every marker of `id`: it changed zones (CR 122.2, CR 400.7). Token marks are
    /// per card and stay.
    pub(super) fn forget(&mut self, id: ObjectId) {
        if self.counters.contains_key(&id) {
            Arc::make_mut(&mut self.counters).remove(&id);
        }
        if self.damage.contains_key(&id) {
            Arc::make_mut(&mut self.damage).remove(&id);
        }
        if self.copies.contains_key(&id) {
            Arc::make_mut(&mut self.copies).remove(&id);
        }
    }
}

impl CanonicalEncode for ObjectMarkers {
    /// The counters map (count, then per object id ascending its kind-to-amount map), the
    /// damage map (count, then id and amount), the token card set (count, then ids ascending)
    /// and the copy map (count, then object id and copied id), in that order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.counters.encode_canonical(out);
        self.damage.encode_canonical(out);
        self.tokens.encode_canonical(out);
        self.copies.encode_canonical(out);
    }
}

/// The executed-event log (REQ-EVENT-001): every batch in which at least one event occurred,
/// in execution order. Trigger detection reads it (CR 603.2, CR 603.10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventLog {
    batches: Arc<Vec<ExecutedBatch>>,
}

impl EventLog {
    /// Every logged batch; the batch at position `i` has `index == i`.
    #[must_use]
    pub fn batches(&self) -> &[ExecutedBatch] {
        &self.batches
    }

    /// The number of logged batches.
    #[must_use]
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// Whether no batch was logged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// The batches whose index is at least `index`; empty if none.
    #[must_use]
    pub fn batches_since(&self, index: u64) -> &[ExecutedBatch] {
        let start =
            usize::try_from(index).map_or(self.batches.len(), |i| i.min(self.batches.len()));
        &self.batches[start..]
    }

    /// The index the next logged batch gets.
    pub(super) fn next_index(&self) -> u64 {
        // `usize` is at most 64 bits on every supported target, so this is lossless.
        self.batches.len() as u64
    }

    /// Appends a batch.
    pub(super) fn push(&mut self, batch: ExecutedBatch) {
        Arc::make_mut(&mut self.batches).push(batch);
    }
}

impl CanonicalEncode for EventLog {
    /// The batch count, then each batch (index, turn, events) in order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.batches.encode_canonical(out);
    }
}

/// Every piece of state the events module owns (REQ-EVENT-001, REQ-DSL-003): the object
/// markers, the executed-event log, the history ledger, the current phase and step, and the
/// players who attempted to draw from an empty library.
///
/// Standalone: not part of `GameState`. Cloning is a cheap snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventState {
    markers: ObjectMarkers,
    log: EventLog,
    history: History,
    phase: Option<Phase>,
    step: Option<Step>,
    empty_library_draws: Arc<BTreeSet<PlayerId>>,
}

impl EventState {
    /// An empty event state: no markers, no log, no history, no phase or step.
    #[must_use]
    pub fn new() -> EventState {
        EventState::default()
    }

    /// The object markers.
    #[must_use]
    pub fn markers(&self) -> &ObjectMarkers {
        &self.markers
    }

    /// Write access to the object markers (only [`ObjectMarkers::clear_all_damage`] is public).
    pub fn markers_mut(&mut self) -> &mut ObjectMarkers {
        &mut self.markers
    }

    /// The executed-event log.
    #[must_use]
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    /// The history ledger.
    #[must_use]
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Write access to the history ledger (for [`History::record_spell_cast`]).
    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// The current phase, if one has begun this turn.
    #[must_use]
    pub fn phase(&self) -> Option<Phase> {
        self.phase
    }

    /// The current step, if one has begun in the current phase.
    #[must_use]
    pub fn step(&self) -> Option<Step> {
        self.step
    }

    /// The players who attempted to draw from an empty library since the last
    /// [`EventState::take_empty_library_draws`], ascending, without duplicates (the CR 704.5b
    /// input; CR 121.4).
    #[must_use]
    pub fn empty_library_draws(&self) -> Vec<PlayerId> {
        self.empty_library_draws.iter().copied().collect()
    }

    /// Returns [`EventState::empty_library_draws`] and clears it (for the state-based action
    /// check, CR 704.5b).
    pub fn take_empty_library_draws(&mut self) -> Vec<PlayerId> {
        let players = self.empty_library_draws();
        if !players.is_empty() {
            self.empty_library_draws = Arc::new(BTreeSet::new());
        }
        players
    }

    pub(super) fn log_mut(&mut self) -> &mut EventLog {
        &mut self.log
    }

    pub(super) fn set_phase(&mut self, phase: Option<Phase>) {
        self.phase = phase;
    }

    pub(super) fn set_step(&mut self, step: Option<Step>) {
        self.step = step;
    }

    pub(super) fn flag_empty_library_draw(&mut self, player: PlayerId) {
        Arc::make_mut(&mut self.empty_library_draws).insert(player);
    }
}

impl CanonicalEncode for EventState {
    /// The markers, the log, the history, the `Option` phase, the `Option` step and the
    /// empty-library-draw player set, in that order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.markers.encode_canonical(out);
        self.log.encode_canonical(out);
        self.history.encode_canonical(out);
        self.phase.encode_canonical(out);
        self.step.encode_canonical(out);
        self.empty_library_draws.encode_canonical(out);
    }
}
