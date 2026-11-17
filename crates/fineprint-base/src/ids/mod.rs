// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Identity vocabulary of the engine (REQ-CORE-002) and the canonical binary encoding hook
//! (REQ-CORE-006).
//!
//! Every identifier is a `u64` newtype whose ordering is the numeric order of the `u64`.
//! Identifiers are issued by monotonic [`IdGenerator`]s held in the game state: the first issued
//! value is 1, values never repeat, and exhaustion is an error ([`IdExhausted`]), never a wrap.

mod encode;
mod generator;
mod types;

pub use encode::CanonicalEncode;
pub use generator::{IdExhausted, IdGenerator, IdGenerators};
pub use types::{AbilityInstanceId, CardId, EffectId, Id, LinkId, ObjectId, PlayerId, Timestamp};
