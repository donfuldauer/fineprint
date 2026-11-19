// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Permanent state on the battlefield (REQ-ZONE-001 item 3, REQ-CORE-006).
//!
//! [`Permanents`] holds, per battlefield object, everything the engine tracks beyond its
//! characteristics: the controller (CR 110.2, the result of layer 2, CR 613.1b), the four status
//! categories (CR 110.5), an open designation registry, continuous-control tracking (CR 302.6),
//! an attachment link and the combat state. A card or token becomes a permanent as it enters the
//! battlefield and stops being one as it leaves (CR 110.1); [`Permanents::enter`] and
//! [`Permanents::leave`] mark those two moments, and an object that re-enters is a new object
//! with fresh defaults (CR 400.7).
//!
//! **Status** ([`Status`]): tapped, flipped, face down and phased out, each an independent
//! boolean; a permanent enters untapped, unflipped, face up and phased in (CR 110.5b) and keeps
//! its status until something changes it (CR 110.5c). Status is not a characteristic (CR 110.5a).
//!
//! **Designations** are an open, data-defined registry: any string key mapped to a
//! [`DesignationValue`](crate::state::DesignationValue), ordered by key. There is no closed enum
//! of designation names, so new sets can add designations without engine changes.
//!
//! **Continuous control** is stored as the turn number since which the permanent has been under
//! its controller's continuous control, never as a summoning-sickness boolean. The CR 302.6
//! input is derived by [`Permanent::controlled_continuously_since_start_of`]; haste (CR 508.1a)
//! is the caller's concern. A control change resets the tracking (CR 613.1b); phasing never does
//! (CR 702.26d).
//!
//! **Attachments** never form a cycle: [`Permanents::attach`] rejects a link that would close
//! one, and [`Permanents::leave`] unattaches everything attached to the leaving permanent
//! (CR 701.3d).
//!
//! **Combat state** ([`CombatState`]): what the permanent attacks ([`AttackState`], with
//! `Attacking(None)` for an attacker whose planeswalker or battle was removed from combat,
//! CR 506.4c), the attackers it blocks, the blocked flag (CR 509.1h) and whether it was removed
//! from combat (CR 506.4).
//!
//! **Standalone scope.** This container is not wired into `GameState`, its Merkle layout or its
//! wire form; the integration unit does that. The zones' `ObjectRecord::face_down` is a separate
//! field and is reconciled with [`Status::face_down`] later.
//!
//! **Omniscient state.** `Permanents` is the omniscient engine state: its accessors and canonical
//! encoding expose face-down status and every designation. It must stay behind the redacted
//! per-player view (REQ-CORE-004, REQ-API-004) and is never handed to a client directly.
//!
//! **Errors** leave the container unchanged. Every collection is a `BTreeMap` or `BTreeSet`, so
//! every iteration is in ascending [`ObjectId`](fineprint_base::ids::ObjectId) or key order
//! (REQ-CORE-006).
//!
//! **Out of scope here** (later work): merged permanents, indirect phasing (CR 702.26g),
//! illegal-attachment state-based actions (CR 704.5m, CR 704.5n, CR 301.5c), new timestamps on
//! attach (CR 701.3c), the combat procedure, layer-2 control-changing effects, and validation of
//! attack and block legality (CR 506.3).

mod container;
mod error;
mod types;

#[cfg(test)]
mod tests;

pub use container::Permanents;
pub use error::PermanentError;
pub use types::{AttachTarget, AttackState, AttackTarget, CombatState, Permanent, Status};
