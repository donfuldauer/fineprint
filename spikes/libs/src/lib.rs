// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! FP-004 dependency spike (throwaway, non-rules).
//!
//! Produces the evidence for the third-party crate pins recorded in `docs/DECISIONS.md`
//! (entries titled "FP-004 — ..."): the PCG64-DXSM generator (REQ-CORE-001), RFC 8785
//! canonical JSON and the SHA-256 Merkle state hash (REQ-NFR-006), the MessagePack transport
//! encoding (REQ-NFR-006) and a pest grammar (REQ-NFR-005). No production crate depends on
//! this crate. The PRNG and Merkle modules are prototypes of rules-path code and therefore
//! use integers and ordered containers only (REQ-CORE-006).

#![forbid(unsafe_code)]

// `pest_derive` without its `std` feature emits `alloc::` paths (keeps the target-side pest
// free of the `std`-only stack guard and its assembly build step).
extern crate alloc;

pub mod grammar;
pub mod jcs;
pub mod merkle;
pub mod prng;
pub mod transport;
