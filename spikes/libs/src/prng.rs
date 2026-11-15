// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! In-house PCG64-DXSM (REQ-CORE-001): a pure-integer generator with no dependencies.
//!
//! This is the NumPy `PCG64DXSM` / pcg-cpp `pcg_engines::cm_setseq_dxsm_128_64` variant: a
//! 128-bit LCG stepped with the 64-bit "cheap multiplier", whose output is the DXSM
//! permutation of the state *before* the step. Seeding follows NumPy (`pcg64_set_seed`, the PCG
//! reference `pcg_setseq_128_srandom_r`), which steps with the standard 128-bit PCG multiplier
//! while seeding. (pcg-cpp and `rand_pcg` seed with the cheap multiplier instead; the output
//! sequence from a given raw state is identical, see docs/DECISIONS.md "FP-004 — PRNG".)

/// The 64-bit "cheap multiplier" used both by the LCG step and the DXSM output permutation.
const CHEAP_MULTIPLIER: u64 = 0xda94_2042_e4dd_58b5;

/// The standard 128-bit PCG multiplier, used only while seeding.
const DEFAULT_MULTIPLIER_128: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

/// A PCG64-DXSM generator. Its whole state is two `u128` values, so it can live in `S_t`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcg64Dxsm {
    state: u128,
    increment: u128,
}

impl Pcg64Dxsm {
    /// Seeds the generator from a 128-bit initial state and a 128-bit stream selector, exactly
    /// as NumPy's `PCG64DXSM` does (the top bit of `initseq` is discarded).
    #[must_use]
    pub fn new(initstate: u128, initseq: u128) -> Self {
        let increment = (initseq << 1) | 1;
        // srandom: state = 0; step; state += initstate; step (standard multiplier).
        let state = increment.wrapping_add(initstate);
        Self {
            state: state
                .wrapping_mul(DEFAULT_MULTIPLIER_128)
                .wrapping_add(increment),
            increment,
        }
    }

    /// Returns the raw `(state, increment)` pair, for hashing and serialising the generator.
    #[must_use]
    pub fn raw(&self) -> (u128, u128) {
        (self.state, self.increment)
    }

    /// Returns the next 64-bit output and advances the generator.
    pub fn next_u64(&mut self) -> u64 {
        // DXSM ("double xorshift multiply") output of the pre-step state.
        let mut hi = (self.state >> 64) as u64;
        let lo = (self.state as u64) | 1;
        hi ^= hi >> 32;
        hi = hi.wrapping_mul(CHEAP_MULTIPLIER);
        hi ^= hi >> 48;
        hi = hi.wrapping_mul(lo);
        self.step();
        hi
    }

    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(u128::from(CHEAP_MULTIPLIER))
            .wrapping_add(self.increment);
    }
}
