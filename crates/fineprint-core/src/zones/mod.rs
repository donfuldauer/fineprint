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
//! Per CR 400.7 an object that moves from one zone to another becomes a new object: it receives
//! a fresh `ObjectId` from the caller's generator, and no reference to the old id remains in the
//! container. The returned [`Moved`] carries both ids so that the caller can hand them to the
//! exception registry ([`EXCEPTIONS`], CR 400.7a-m). A move into the zone the object is already
//! in is not a zone change: in a library, hand, graveyard, the battlefield or the stack the
//! object keeps its id and is only repositioned (CR 400.7: not a move from one zone to another; CR 400.5; `Moved` then has
//! `old == new`), while in exile and the command zone it becomes a new object without changing
//! zones (CR 400.8, CR 406.7, CR 400.10) and gets a fresh id. Objects that would go to another
//! player's library, hand or graveyard go to their owner's (CR 400.3). An issued id that is not above
//! the container's high-water mark (the highest id it has issued or accepted) is rejected
//! ([`ZoneError::IdCollision`]). This is a high-water rejection, not proof of id reuse: it also
//! rejects ids the container never held. Example: the first object created gets id 10 from a
//! generator restored with `IdGenerator::from_last_issued(9)`; a later generator that proposes 5
//! gets `IdCollision(5)` although 5 was never held.
//!
//! **Omniscient state.** [`Zones`] is the omniscient engine state: `contents`, `object`,
//! `object_of_card`, `all_object_ids` and the canonical encoding expose hidden information
//! (library order, hand membership, the `CardId` of face-down objects). It must stay behind the
//! redacted per-player view and the determinized-fork boundary (REQ-CORE-004, REQ-API-004) and is
//! never handed to a client directly.
//!
//! **Stack scope.** In this unit the stack holds only card-backed objects (spells, and copies
//! that have a `CardId`), and [`Zones::create_object`] requires a `CardId`. Cardless stack objects
//! (abilities, which go on the stack without a card, CR 405.1, CR 109.1; copies of spells that
//! are not cards) are out of scope and are the job of the later units that own them (#31 FP-030,
//! #58 FP-056, #66 FP-064, and a later stack/ability unit for activated abilities).
//!
//! **Controllers.** [`ObjectRecord::controller`] holds the owner outside the battlefield and the
//! stack (the CR 108.4a fallback); [`ObjectRecord::rules_controller`] gives the rules-level value
//! (CR 109.4).
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
//! state, the ante zone, subgames, merged permanents, the bodies of the CR 400.7 exceptions
//! other than CR 400.7d and CR 400.7j (those two return `Applies`; their logic lives in
//! `fineprint_core::memory`), and wiring this container into `GameState`, its Merkle layout and its wire form.

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
