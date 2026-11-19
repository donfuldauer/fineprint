// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Game events, execution primitives, the executed-event log and the history ledger
//! (REQ-EVENT-001, REQ-DSL-003).
//!
//! **Events.** Every atomic change to the game is a [`GameEvent`] (SRS 8.1): zone changes,
//! drawing, damage, life change, counters, tap/untap, token creation, the beginning of a turn,
//! phase or step, attaching and unattaching, becoming a copy, and turning face up or down. An
//! [`EventInstance`] carries the event with its [`AppliedEffects`] (CR 614.5). Sacrifice,
//! destroy, discard, mill, exile and countering a spell are zone changes labelled with a
//! [`ZoneChangeCause`].
//!
//! **Execution.** [`EventContext::execute_batch`] executes one [`EventBatch`] of simultaneous
//! events all-or-nothing: it snapshots every component, captures last known information for
//! every object the batch moves out of its zone before any move (CR 704.8, CR 603.10a), runs
//! the events in submission order and, on any error, restores every component. Events that
//! would change nothing (0 damage, gaining 0 life, tapping a tapped permanent, ...) are no-ops:
//! no event occurs, nothing is logged or counted. Each batch in which something occurred is
//! appended to the [`EventLog`] as an [`ExecutedBatch`], which a later trigger-detection unit
//! reads (CR 603.2, CR 603.10): the event as submitted, its [`Outcome`] (old and new object
//! ids, token object, timestamp, turn) and whether LKI was captured (the record itself is in
//! the `LkiStore` under the old id).
//!
//! **History.** [`History`] counts, per game turn and player, spells cast, life gained and
//! lost, cards drawn, damage dealt and tokens created, fed only from executed events (after
//! replacement and prevention) and from [`History::record_spell_cast`]; it answers
//! [`HistoryQuery`]s for `HISTORY(query)` value expressions (REQ-DSL-003).
//!
//! **Determinism.** No floating point, no hash-ordered containers: every collection is a
//! `BTreeMap`, `BTreeSet` or ordered `Vec`, and every logged type has a canonical encoding with
//! a documented tag per enum variant (REQ-CORE-001, REQ-CORE-006). Components sit behind `Arc`
//! and are written with `Arc::make_mut`, so cloning an [`EventState`] is a cheap snapshot.
//!
//! **Omniscient state.** The log, the LKI it points to and the object markers expose hidden
//! information (cards drawn into a hand, face-down objects); they stay behind the redacted
//! per-player view (REQ-CORE-004) and are never handed to a client directly.
//!
//! **Standalone scope.** Nothing here is wired into `GameState`, its Merkle layout or its wire
//! form; [`EventState`] is a separate component and `GameState`'s `HistoryLedger` placeholder
//! is untouched. The integration unit joins them.
//!
//! **Out of scope here** (later units): applying replacement and prevention effects, trigger
//! matching, the destroy event and its own replacement (CR 701.8), shuffle, control change,
//! phasing, winning and losing, and the damage results of CR 120.3b-d and CR 120.3f-h
//! (poison, loyalty, -1/-1 counters, lifelink, toxic, defense), which later units derive from
//! the log.

mod error;
mod exec;
mod history;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub use error::EventError;
pub use exec::EventContext;
pub use history::{History, HistoryQuery, HistoryScope, HistoryStat};
pub use state::{EventLog, EventState, ObjectMarkers};
pub use types::{
    AppliedEffects, CounterTarget, DamageTarget, EventBatch, EventInstance, ExecutedBatch,
    ExecutedEvent, GameEvent, Outcome, Phase, Step, ZoneChangeCause,
};
