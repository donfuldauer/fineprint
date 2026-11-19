// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The replacement and prevention bus of SRS 8.1 (REQ-EVENT-001, REQ-DSL-009).
//!
//! **Loop.** [`process_batch`] takes a batch of simultaneous events. For each event E, in APNAP
//! order of its chooser: [1] collect the applicable effects R(E) from the
//! [`ReplacementRegistry`] in ascending [`EffectId`](fineprint_base::ids::EffectId), against the
//! current state, excluding E's applied effects (CR 614.5) unless an effect carries the
//! per-effect override flag (CR 903.9b-style); if R(E) is empty, E is executed as it is.
//! Contained effects are not choosable while a non-contained one applies (CR 616.1g). [2] The
//! chooser is the controller of the affected object (its owner if it has none), or the
//! affected player, else the active player (CR 616.1). [3] Only the first non-empty
//! [`Tier`] is choosable (CR 616.1a-e). [4] One effect is applied without asking; several are
//! offered through `CHOOSE_REPLACEMENT_EFFECT` ([`ChoiceContext::ask`](crate::choice::ChoiceContext::ask));
//! the chosen effect modifies the event and joins its applied set. [5] Repeat on the modified
//! event (CR 616.1f, CR 616.2). The surviving events execute together through
//! [`EventContext::execute_batch`](crate::events::EventContext::execute_batch); queued child
//! events (from [`Action::Instead`], keeping the replaced event's applied set, CR 614.5, and
//! [`Action::Prevent`]'s `then`, starting fresh, CR 615.5) then form one child batch that goes
//! through the same bus. A [`Modification::EntersAsCopyOf`] copy is executed right after the
//! entry, without replacement collection (CR 614.1c, CR 616.1c). Events that would not happen
//! (0 damage, untapping an untapped permanent) offer no effect (CR 614.7, CR 614.7a). One call
//! takes up at most [`MAX_BUS_EVENTS`] events.
//!
//! **Prevention (CR 615.1, CR 615.6, CR 615.7).** Prevention effects are [`Action::Prevent`] effects; [`Shield`]s
//! limit uses or the damage pool and exhausted effects leave the registry. Counter-backed
//! effects (shield, stun, finality counters, CR 122.1c, CR 122.1d, CR 122.1h) are built by the
//! [`ReplacementEffect`] constructors and apply while the counter is there. When simultaneous
//! damage from several sources exceeds a pool, the chooser divides the pool among the sources
//! with `DIVIDE_VALUE` before the batch is processed (CR 615.7).
//!
//! **State-based actions (CR 704.7).** With `sba` true, zone changes of the same object to the
//! same destination are one event: one choice, one application, one execution. Different
//! objects never merge.
//!
//! **Atomicity and suspension.** Any error restores the registry and every component of the
//! event context. A pending choice propagates as [`BusInterrupt::Pending`]; the
//! [`ChoiceSession`](crate::choice::ChoiceSession) re-runs the call with the recorded answers.
//!
//! **Determinism (REQ-CORE-001, REQ-CORE-006).** Ordered containers only, ascending-id
//! iteration, integer arithmetic, no clock.
//!
//! **Standalone scope.** The registry is a separate component, not joined to `GameState`; the
//! caller keeps it equal to the effects in force. ETB look-ahead (CR 614.12), optional effects
//! and trigger detection are later units.

#![deny(missing_docs)]

mod bus;
mod types;

#[cfg(test)]
mod tests;

pub use bus::{
    BusReport, MAX_BUS_EVENTS, MAX_CHILD_DEPTH, MAX_REPEATS, process_batch, process_event,
};
pub use types::{
    Action, AmountOp, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification,
    ReplacementEffect, ReplacementRegistry, Shield, Tier, tier_from_ability,
};
