// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Triggers (REQ-EVENT-002, SRS 8.2): which triggered abilities trigger on one batch of
//! simultaneous events, the queue they wait in, state, delayed, reflexive and sourceless
//! triggers, and putting them on the stack.
//!
//! **Events.** [`events_from_batch`] normalises an executed batch (read from the executed-event
//! log) into [`TriggerEvent`]s: one or more per executed event that a trigger can match (damage
//! to a player is also its life loss), carrying every [`EventKind`] it is an instance of, the
//! subject object's pre-event and post-event ids, the subject player and named fields for
//! `filters` and `bind`. [`events_from_batch_with_lki`] also reads the batch's last known
//! information (a sacrifice's controller).
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
//! `limit.perGame` with counters in the source's Memory ([`detect_with_pre_event_memory`]
//! reads them from the Memory as it was before the batch, so a departing source's used limit
//! still counts). It returns [`WaitingTrigger`]s, which
//! a [`WaitingTriggers`] queue holds in creation order.
//!
//! **To the stack.** [`StateTriggers`] checks state triggers (CR 603.8) and keeps an instance
//! from triggering again until it has left the stack. [`DelayedTriggers`] holds delayed
//! triggered abilities with the bindings captured when they were created (CR 603.7), and
//! [`reflexive_triggers`] creates reflexive ones (CR 603.12). [`SourcelessTriggers`] holds the
//! player-scoped inherent abilities with no source (the monarch, the initiative, rad counters
//! and speed; an exception to CR 113.8). Every origin becomes a [`PendingTrigger`];
//! [`place_triggers`] puts them on the stack in the two-part APNAP process of CR 603.3b with
//! the choice protocol (`ORDER_SIMULTANEOUS_TRIGGERS`, `CHOOSE_MODES`, `CHOOSE_TARGETS`) and
//! removes a trigger with no legal choice (CR 603.3c-d); [`ask_optional_trigger`] asks
//! `CHOOSE_OPTIONAL_TRIGGER` when an optional trigger resolves (CR 603.5).
//!
//! **Out of scope** (later units): the implementations of [`TriggerEnv`] and [`PlacementEnv`]
//! on the real game state (the state has no stack or ability registry yet; the game-loop
//! integration unit owns them, docs/DECISIONS.md "#31 FP-030 — Real-state environments belong
//! to the integration unit"), emitting `TriggerPutOnStackEvent` and `TriggerRemovedEvent`, the
//! state-based-action loop of CR 603.3b, the resolution-time intervening-if check, phase/step
//! triggers, and cost-payment LKI for crew, saddle and station.
//!
//! **Determinism.** Only `Vec`, `BTreeMap` and `BTreeSet`; registrations are sorted by (source, ability id)
//! and output is ordered by (first event index, source, ability id) (REQ-CORE-001).

mod delayed;
mod detect;
mod event;
mod lookback;
mod place;
mod queue;
mod sourceless;
mod state;

#[cfg(test)]
mod stack_tests;
#[cfg(test)]
mod tests;

pub use delayed::{DelayedTriggerId, DelayedTriggers, reflexive_triggers};
pub use detect::{
    TriggerEnv, TriggerRegistration, WaitingTrigger, detect, detect_with_pre_event_memory,
};
pub use event::{EventField, TriggerEvent, events_from_batch, events_from_batch_with_lki};
pub use lookback::{LookBackKind, StateView, look_back_kind};
pub use place::{
    PendingTrigger, PlacementEnv, RemovedReason, RemovedTrigger, StackedTrigger, TriggerOrigin,
    TriggerPlacement, ask_optional_trigger, place_triggers,
};
pub use queue::WaitingTriggers;
pub use sourceless::{SourcelessAbility, SourcelessKind, SourcelessTriggers};
pub use state::StateTriggers;

#[cfg(doc)]
use fineprint_dsl::ability::{EventKind, EventMatcher};
