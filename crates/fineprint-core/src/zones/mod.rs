// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Zones, the zone-move primitive and the CR 400.7 exception registry (REQ-ZONE-001,
//! REQ-CORE-002).
//!
//! [`Zones`] holds the zone containers (CR 400.1): one library, hand and graveyard per player
//! and the shared battlefield, stack, exile and command zones. Libraries, graveyards and the
//! stack are ordered with index 0 at the top (CR 400.5, CR 401.2, CR 404.1, CR 405.2); the
//! hand, battlefield, exile and command zones are unordered and iterate in ascending
//! [`ObjectId`](fineprint_base::ids::ObjectId) order, so every iteration is deterministic.
//!
//! **The zone-move primitive.** [`Zones::move_object`] is the only way an object changes zones.
//! Per CR 400.7 the moved object becomes a new object: it receives a fresh `ObjectId` from the
//! caller's generator, and no reference to the old id remains in the container. The returned
//! [`Moved`] carries both ids so that the caller can hand them to the exception registry
//! ([`EXCEPTIONS`], CR 400.7a-m). A move to the same zone also creates a new object (CR 400.8,
//! CR 406.7, CR 400.10). Objects that would go to another player's library, hand or graveyard
//! go to their owner's (CR 400.3).
//!
//! **Outside the game** (CR 400.11) is not a zone: it is a set of [`OutsidePool`] card pools
//! per owner, holding [`CardId`](fineprint_base::ids::CardId)s and never object ids.
//!
//! **Snapshots.** Every component is held behind `Arc` and written with `Arc::make_mut`, so
//! `clone()` is a cheap structural-sharing snapshot and mutating a clone never changes the
//! original. Equality compares values.
//!
//! **Out of scope here** (later work): the CR 401.5 top-card reveal re-evaluation,
//! simultaneous-entry ordering choices (CR 401.4, CR 404.3, CR 405.3), battlefield permanent
//! state, the ante zone, subgames, merged permanents, the bodies of the CR 400.7 exceptions,
//! and wiring this container into `GameState`, its Merkle layout and its wire form.

mod container;
mod error;
pub mod exceptions;
mod types;

#[cfg(test)]
mod tests;

pub use container::Zones;
pub use error::ZoneError;
pub use exceptions::*;
pub use types::{
    MoveRequest, Moved, ObjectRecord, OutsidePool, Position, ZoneId, ZoneKind, ZonesConfig,
};
