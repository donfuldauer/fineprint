// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The engine's only source of randomness (REQ-CORE-001, REQ-API-004).
//!
//! **Algorithm.** PCG64-DXSM ([`ALGORITHM_ID`] = `"PCG64-DXSM"`): a 128-bit LCG stepped with the
//! 64-bit "cheap multiplier" `0xda94_2042_e4dd_58b5`, whose output is the DXSM permutation of the
//! state *before* the step. The whole generator is two `u128` values (state and odd increment),
//! so it lives in the game state, can be saved with [`Pcg64Dxsm::raw`], restored with
//! [`Pcg64Dxsm::from_raw`] and hashed through [`CanonicalEncode`](crate::ids::CanonicalEncode).
//!
//! **Frozen output.** The generator, the seeding procedure and every sampling algorithm in this
//! module (bounded integers, shuffle, choice, subset) are fixed per engine major version: the
//! same seed gives bit-identical results on every platform, and any change to an output is a
//! breaking change that needs a new engine major version.
//!
//! **Seeding.** [`Pcg64Dxsm::from_u64_seed`] is the one helper used both for match seeding and
//! for fork reseeding: run [`SplitMix64`] from `state = seed64`, take four outputs `o0..o3`,
//! set `initstate = (o0 << 64) | o1` and `initseq = (o2 << 64) | o3`, then seed with
//! [`Pcg64Dxsm::new`] (NumPy `srandom` seeding: standard 128-bit PCG multiplier during seeding
//! only, top bit of `initseq` discarded). A fork reseeds with
//! `from_u64_seed(hi64(state) ^ lo64(state) ^ hi64(increment) ^ lo64(increment) ^ fork_salt)`
//! ([`Pcg64Dxsm::fork`]).
//!
//! The module uses integer arithmetic only, never panics, and its results do not depend on
//! endianness or the width of `usize`.

mod pcg64;
mod sample;
mod splitmix64;

#[cfg(test)]
mod tests;

pub use pcg64::Pcg64Dxsm;
pub use splitmix64::SplitMix64;

/// The algorithm ID recorded in replay headers and in the state (REQ-CORE-001).
pub const ALGORITHM_ID: &str = "PCG64-DXSM";
