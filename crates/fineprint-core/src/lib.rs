// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Deterministic rules engine core of Fine Print (rules crate).

#![forbid(unsafe_code)]

/// Base characteristics per card layout and zone, and mana value per layout (REQ-OBJ-002).
pub mod layouts;
/// Last known information, per-object Memory and the Linked Store (REQ-CORE-003, REQ-CORE-007).
pub mod memory;
/// Object characteristics, mana value, colors, color identity and type validation (REQ-OBJ-001).
pub mod objects;
/// Permanent state on the battlefield: status, designations, control, attachments, combat (REQ-ZONE-001).
pub mod permanent;
/// The root game-state value: players, turn position, PRNG, ID generators (REQ-CORE-001).
pub mod state;
/// Zones, the zone-move primitive and the CR 400.7 exception registry (REQ-ZONE-001, REQ-CORE-002).
pub mod zones;
