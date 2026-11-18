// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The PCG64-DXSM generator, its seeding and fork reseeding (REQ-CORE-001, REQ-API-004).
//!
//! This is the NumPy `PCG64DXSM` / pcg-cpp `cm_setseq_dxsm_128_64` variant, seeded as NumPy
//! does (`pcg_setseq_128_srandom_r`, standard 128-bit multiplier while seeding).

use super::splitmix64::SplitMix64;
use crate::ids::CanonicalEncode;

/// The 64-bit "cheap multiplier" used both by the LCG step and the DXSM output permutation.
const CHEAP_MULTIPLIER: u64 = 0xda94_2042_e4dd_58b5;

/// The standard 128-bit PCG multiplier, used only while seeding.
const DEFAULT_MULTIPLIER_128: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

/// Top 64 bits of a `u128` (truncating shift is the intent).
fn hi64(value: u128) -> u64 {
    (value >> 64) as u64
}

/// Bottom 64 bits of a `u128` (truncation is the intent).
fn lo64(value: u128) -> u64 {
    value as u64
}

/// A PCG64-DXSM generator (REQ-CORE-001). Its whole state is two `u128` values (the LCG state
/// and an odd increment), so it can be stored in the game state, saved, restored and hashed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg64Dxsm {
    state: u128,
    increment: u128,
}

impl Pcg64Dxsm {
    /// Seeds the generator from a 128-bit initial state and a 128-bit stream selector, exactly
    /// as NumPy's `PCG64DXSM` does: `increment = (initseq << 1) | 1` (the top bit of `initseq`
    /// is discarded), then `state = (increment + initstate) * M128 + increment` with the
    /// standard 128-bit PCG multiplier.
    #[must_use]
    pub fn new(initstate: u128, initseq: u128) -> Self {
        let increment = (initseq << 1) | 1;
        // srandom: state = 0; step; state += initstate; step (standard multiplier).
        let state = increment
            .wrapping_add(initstate)
            .wrapping_mul(DEFAULT_MULTIPLIER_128)
            .wrapping_add(increment);
        Self { state, increment }
    }

    /// The one seeding helper for both match seeding and fork reseeding (REQ-CORE-001,
    /// REQ-API-004): four [`SplitMix64`] outputs `o0..o3` from `seed64` give
    /// `initstate = (o0 << 64) | o1` and `initseq = (o2 << 64) | o3`, passed to [`Self::new`].
    #[must_use]
    pub fn from_u64_seed(seed64: u64) -> Self {
        let mut expander = SplitMix64::new(seed64);
        let o0 = expander.next_u64();
        let o1 = expander.next_u64();
        let o2 = expander.next_u64();
        let o3 = expander.next_u64();
        let initstate = (u128::from(o0) << 64) | u128::from(o1);
        let initseq = (u128::from(o2) << 64) | u128::from(o3);
        Self::new(initstate, initseq)
    }

    /// Restores a generator saved with [`Self::raw`]. Returns `None` when `increment` is even
    /// (a valid increment is always odd); no other validation is done, and no input panics.
    #[must_use]
    pub fn from_raw(state: u128, increment: u128) -> Option<Self> {
        if increment & 1 == 0 {
            return None;
        }
        Some(Self { state, increment })
    }

    /// Returns the raw `(state, increment)` pair, for hashing and serialising the generator.
    #[must_use]
    pub fn raw(&self) -> (u128, u128) {
        (self.state, self.increment)
    }

    /// Returns the next 64-bit output (the DXSM permutation of the current state) and then
    /// advances the generator by one LCG step with the cheap multiplier.
    pub fn next_u64(&mut self) -> u64 {
        // DXSM ("double xorshift multiply") output of the pre-step state.
        let mut hi = hi64(self.state);
        let lo = lo64(self.state) | 1;
        hi ^= hi >> 32;
        hi = hi.wrapping_mul(CHEAP_MULTIPLIER);
        hi ^= hi >> 48;
        hi = hi.wrapping_mul(lo);
        self.step();
        hi
    }

    /// One LCG step with the 64-bit cheap multiplier.
    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(u128::from(CHEAP_MULTIPLIER))
            .wrapping_add(self.increment);
    }

    /// The 64-bit fork seed (REQ-API-004):
    /// `hi64(state) ^ lo64(state) ^ hi64(increment) ^ lo64(increment) ^ fork_salt`, from the raw
    /// state at the moment of the call.
    #[must_use]
    pub fn fork_seed64(&self, fork_salt: u64) -> u64 {
        hi64(self.state)
            ^ lo64(self.state)
            ^ hi64(self.increment)
            ^ lo64(self.increment)
            ^ fork_salt
    }

    /// A reproducible child generator for a fork (REQ-API-004):
    /// `Pcg64Dxsm::from_u64_seed(self.fork_seed64(fork_salt))`. Does not advance `self`; the same
    /// parent state and salt always give the same fork.
    #[must_use]
    pub fn fork(&self, fork_salt: u64) -> Pcg64Dxsm {
        Self::from_u64_seed(self.fork_seed64(fork_salt))
    }
}

impl CanonicalEncode for Pcg64Dxsm {
    /// Appends 32 bytes: `state` little-endian (16 bytes), then `increment` little-endian
    /// (16 bytes). No tag, no length prefix.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.state.to_le_bytes());
        out.extend_from_slice(&self.increment.to_le_bytes());
    }
}
