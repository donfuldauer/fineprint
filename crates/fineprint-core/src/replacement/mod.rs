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
//! event (CR 616.1f, CR 616.2). The events of an applied [`Action::Instead`] keep the replaced
//! event's applied set (CR 614.5) and are part of the same simultaneous batch, at every
//! generation (CR 614.6, CR 704.3); they are resolved against the state before the batch, as
//! soon as the event they replace has resolved, in the same APNAP order at that event's
//! position, recursively (CR 101.4). The final events execute together as ONE batch through
//! [`EventContext::execute_batch_with_entry_copies`](crate::events::EventContext::execute_batch_with_entry_copies)
//! (the events that are not zone changes first, then at most one zone change per object, except
//! the `sba` false caller's duplicate below); the [`Action::Prevent`] follow-up events (`then`,
//! starting fresh, CR 615.5) of the whole batch then form one child batch that goes through
//! the same bus. An object that a [`Modification::EntersAsCopyOf`] effect makes enter as a copy
//! enters as that copy as part of its own entering event in the executed batch (CR 707.5,
//! CR 614.1c, CR 616.1c): there is no separate `BecomeCopy` event and the copy is not offered
//! to replacement effects; this is distinct from the ETB look-ahead of CR 614.12, a later unit.
//! Events that would not happen (0 damage, untapping an untapped permanent, tapping a tapped
//! permanent) offer no effect (CR 614.7, CR 614.7a, CR 701.26a). An event of a later batch of
//! the call that names an object an earlier batch moved, as its affected object, an `Attach`
//! host or a `BecomeCopy` copied object, is ignored (CR 101.3, CR 400.7). One call takes up at
//! most [`MAX_BUS_EVENTS`] events.
//!
//! **Prevention (CR 615.1, CR 615.6, CR 615.7).** Prevention effects are [`Action::Prevent`] effects; [`Shield`]s
//! limit uses or the damage pool and exhausted effects leave the registry. Counter-backed
//! effects (shield, stun, finality counters, CR 122.1c, CR 122.1d, CR 122.1h) are built by the
//! [`ReplacementEffect`] constructors and apply while the counter is there. When simultaneous
//! damage from several sources exceeds a pool, the chooser divides the pool among the sources
//! with `DIVIDE_VALUE` before the batch is processed (CR 615.7).
//!
//! **State-based actions (CR 704.7).** With `sba` true, zone changes of the same object to the
//! same destination are one group: the candidates are the union of the members' effects; each
//! round selects ONE effect (the chooser's pick, or the sole candidate applied without asking)
//! and applies it once to every member it matches (members it does not match still move the
//! object); the bus repeats collection until no candidate remains (CR 616.1f). Different
//! objects never merge. Every `Instead` descendant, of a group or not, is part of the one
//! executed batch (CR 614.6, CR 704.3, CR 704.8: last known information is captured before
//! it). An object moves at most once: its final zone changes (group members, other members and
//! `Instead` moves) compete, a move produced or redirected by a replacement winning, then the
//! least by content; the choice does not depend on submission order. Exception: with `sba`
//! false, when every competing move of an object was submitted directly (no `Instead`
//! descendant among them), nothing is reconciled: all are executed, the second fails with
//! `UnknownObject` and the call rolls back (a caller's duplicate). Results of different
//! objects or groups are resolved in APNAP order of their choosers, then submission order (an
//! `Instead` descendant taking the position of the event it replaces, recursively), and may
//! compete for a one-use effect, so reversing the submission can change which object such an
//! effect applies to.
//!
//! **Atomicity and suspension.** Any error restores the registry and every component of the
//! event context. A pending choice propagates as [`BusInterrupt::Pending`]; the
//! [`ChoiceSession`](crate::choice::ChoiceSession) re-runs the call with the recorded answers.
//!
//! **Determinism (REQ-CORE-001, REQ-CORE-006).** Ordered containers only, ascending-id
//! iteration, integer arithmetic, no clock.
//!
//! **Standalone scope.** The registry is a separate component, not joined to `GameState`; the
//! caller keeps it equal to the effects in force. The ETB look-ahead (CR 614.12) is the
//! standalone `etb` unit ([`project`], [`applicable`], [`resolve_entering`]), not yet joined to
//! this bus; optional effects and trigger detection are later units.

#![deny(missing_docs)]

mod bus;
mod etb;
mod types;

#[cfg(test)]
mod tests;

pub use bus::{
    BusReport, MAX_BUS_EVENTS, MAX_CHILD_DEPTH, MAX_REPEATS, process_batch, process_event,
};
pub use etb::{
    CopyAbilities, EtbAction, EtbApplied, EtbChoiceKind, EtbCondition, EtbContext, EtbEffect,
    EtbEffects, EtbEntry, EtbError, EtbOption, EtbResolved, EtbScope, ProjectedPermanent,
    applicable, project, resolve_entering,
};
pub use types::{
    Action, AmountOp, BusError, BusEventKind, BusInterrupt, EventMatcher, Modification,
    ReplacementEffect, ReplacementRegistry, Shield, Tier, tier_from_ability,
};
