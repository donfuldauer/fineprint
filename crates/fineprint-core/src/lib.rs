// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Deterministic rules engine core of Fine Print (rules crate).

#![forbid(unsafe_code)]

/// The choice protocol: pending choice requests, responses, generation from choice specs, validation and deterministic suspend/resume (REQ-NET-003, REQ-DSL-011).
pub mod choice;
/// Game events, execution primitives, the executed-event log and the history ledger (REQ-EVENT-001, REQ-DSL-003).
pub mod events;
/// Who knows what, shuffle knowledge and the redacted per-player zones view (REQ-CORE-004).
pub mod knowledge;
/// Base characteristics per card layout and zone, and mana value per layout (REQ-OBJ-002).
pub mod layouts;
/// Last known information, per-object Memory and the Linked Store (REQ-CORE-003, REQ-CORE-007).
pub mod memory;
/// Object characteristics, mana value, colors, color identity and type validation (REQ-OBJ-001).
pub mod objects;
/// Permanent state on the battlefield: status, designations, control, attachments, combat (REQ-ZONE-001).
pub mod permanent;
/// The replacement and prevention bus: collection, precedence tiers, choice, shields, child events (REQ-EVENT-001).
pub mod replacement;
/// The root game-state value: players, turn position, PRNG, ID generators (REQ-CORE-001).
pub mod state;
/// Trigger detection: look-back classification, event matching, intervening-if, trigger limits and the waiting-trigger queue (REQ-EVENT-002).
pub mod triggers;
/// Checkpoints, rollback, nested durable sub-transactions and the sandbox snapshot store (REQ-CORE-005).
pub mod txn;
/// Zones, the zone-move primitive and the CR 400.7 exception registry (REQ-ZONE-001, REQ-CORE-002).
pub mod zones;
