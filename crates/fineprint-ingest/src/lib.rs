// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Offline card toolchain for Fine Print.

// non-rules crate: REQ-CORE-006 applies to the rules crates only
#![allow(clippy::disallowed_types)]
#![forbid(unsafe_code)]

/// `fineprint-ingest vocab`: the `RulesVocabulary` generator (REQ-OBJ-001, REQ-SUB-001,
/// REQ-DSL-014).
pub mod vocab;
