// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Deterministic rules engine core of Fine Print (rules crate).

#![forbid(unsafe_code)]

/// The root game-state value: players, turn position, PRNG, ID generators (REQ-CORE-001).
pub mod state;
