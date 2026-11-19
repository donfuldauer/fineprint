// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Trigger detection (REQ-EVENT-002, SRS 8.2): which triggered abilities trigger on one batch
//! of simultaneous events, and the queue they wait in until they are put on the stack.
//!
//! **Events.** [`events_from_batch`] normalises an executed batch (read from the executed-event
//! log) into [`TriggerEvent`]s: one per executed event that a trigger can match, carrying every
//! [`EventKind`] it is an instance of, the subject object's pre-event and post-event ids, the
//! subject player and named fields for `filters` and `bind`.
//!
//! **Views.** Normally triggers are matched against the state immediately after the event; the
//! look-back triggers of CR 603.10a-g are matched against the state immediately before it, with
//! the abilities that existed then (CR 603.10). The category is derived from the matcher's kind
//! and filters by [`look_back_kind`]; a definition never sets it (SRS 8.2).
//!
//! **Detection.** [`detect`] reads the game through the [`TriggerEnv`] abstraction (the
//! integration unit implements it on the real state), matches every event-triggered ability's
//! [`EventMatcher`] (kind, subject, filters, `EACH`/`ONE_OR_MORE` batching, bindings), checks
//! the intervening "if" clause at trigger time (CR 603.4) and enforces `limit.perTurn` /
//! `limit.perGame` with counters in the source's Memory. It returns [`WaitingTrigger`]s, which
//! a [`WaitingTriggers`] queue holds in creation order.
//!
//! **Out of scope** (later units): putting triggers on the stack and their APNAP ordering
//! (CR 603.3b), the resolution-time intervening-if check, state triggers, phase/step
//! triggers, delayed and reflexive triggers, and cost-payment LKI for crew, saddle and station.
//!
//! **Determinism.** Only `Vec` and `BTreeMap`; registrations are sorted by (source, ability id)
//! and output is ordered by (first event index, source, ability id) (REQ-CORE-001).

mod detect;
mod event;
mod lookback;
mod queue;

#[cfg(test)]
mod tests;

pub use detect::{TriggerEnv, TriggerRegistration, WaitingTrigger, detect};
pub use event::{EventField, TriggerEvent, events_from_batch};
pub use lookback::{LookBackKind, StateView, look_back_kind};
pub use queue::WaitingTriggers;

#[cfg(doc)]
use fineprint_dsl::ability::{EventKind, EventMatcher};
