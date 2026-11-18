// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared base types for the Fine Print rules engine (rules crate).

#![forbid(unsafe_code)]

/// Ordered collections and the stable (ObjectId, Timestamp) sort (REQ-CORE-006).
pub mod collections;
/// Canonical injective binary encoding: helpers and fixed layouts (REQ-NFR-006).
pub mod encode;
/// SHA-256 digests and the incremental Merkle hashing framework (REQ-NFR-006, REQ-LOOP-001).
pub mod hash;
/// Identifier newtypes, monotonic generators and the canonical encoding hook (REQ-CORE-002).
pub mod ids;
/// Checked integer and scale-2 fixed-point numbers (REQ-CORE-006).
pub mod num;
/// PCG64-DXSM generator, SplitMix64 seed expansion and unbiased sampling (REQ-CORE-001, REQ-API-004).
pub mod prng;
