// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `.pzl` scenario format and its runner (REQ-TEST-001 item 1).
//!
//! A scenario is a declarative JSON file (UTF-8, `.pzl` extension; docs/DECISIONS.md "#51
//! FP-049 — .pzl file syntax") holding an exact starting state, a scripted sequence of player
//! choices and assertions. The schema is documented in docs/DEVELOPMENT.md (".pzl scenarios").
//!
//! - [`parse_scenario`] reads a file as untrusted input (R12): any malformed file gives a
//!   located [`ScenarioError`], never a panic. Limits: [`MAX_SCENARIO_BYTES`], [`MAX_DEPTH`],
//!   [`MAX_ARRAY_ELEMENTS`]; integers only (R3).
//! - [`run_scenario`] runs a parsed [`Scenario`] on a [`ScenarioHost`] and returns a
//!   [`ScenarioReport`]; [`evaluate_assertions`] is its pure assertion half.
//! - [`StateHost`] is the reference host over the real engine components; later units extend
//!   what it supports.
//!
//! The runner is deterministic (R3): ordered containers only, no floats, no clock, no I/O, no
//! environment. The same scenario on the same host gives byte-identical report text
//! ([`ScenarioReport::to_text`]) and the same state hash ([`StateHost::state_hash`]).
//!
//! A scenario can never set a support status: only passing client-supplied behavioural tests
//! earn `VERIFIED` (R11), and nothing here records one.

#![deny(missing_docs)]

mod decode;
mod error;
mod host;
mod json;
mod model;
mod observe;
mod run;

#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod tests;

pub use error::{ScenarioError, ScenarioErrorKind};
pub use host::{StateHost, UNSUPPORTED};
pub use json::pointer_position;
pub use model::{
    ANSWER_TYPES, AbsentWhat, Assertion, CardDatabase, CardRef, CharacteristicsExpect,
    DesignationSpec, EventPattern, EventsMode, Expect, ObjectSetup, OptionsMode, OrderMode,
    PhaseName, PlayerSetup, PlayerZones, ResultStatus, Ruling, Scenario, ScriptStep, Setup,
    SetupObject, StackSetup, StepName, TurnSetup, ZoneName,
};
pub use observe::{
    Observed, ObservedCharacteristics, ObservedChoice, ObservedEvent, ObservedResult,
};
pub use run::{
    AssertionOutcome, BUILD_FAILURE_PREFIX, ScenarioHost, ScenarioReport, ScriptFailure,
    evaluate_assertions, run_scenario,
};

/// The largest `.pzl` file, in bytes (1 MiB).
pub const MAX_SCENARIO_BYTES: usize = 1_048_576;

/// The deepest nesting of arrays and objects (the top-level object is depth 1).
pub const MAX_DEPTH: usize = 64;

/// The largest total number of array elements in one file.
pub const MAX_ARRAY_ELEMENTS: usize = 10_000;

/// Parses one `.pzl` file (REQ-TEST-001, R12).
///
/// The bytes are untrusted: this never panics, whatever they hold. They must be UTF-8 JSON (a
/// leading byte-order mark is tolerated) of at most [`MAX_SCENARIO_BYTES`] bytes, nested at most
/// [`MAX_DEPTH`] deep, with at most [`MAX_ARRAY_ELEMENTS`] array elements, no duplicate keys,
/// integers only within `i64`/`u64`, every object free of unknown members, every enumerated
/// name known, every player, object and card reference declared, no id declared twice, and
/// `pzlVersion` 1.
///
/// # Errors
/// The first problem found, as a [`ScenarioError`] with its path and, where known, its line
/// and column.
pub fn parse_scenario(bytes: &[u8]) -> Result<Scenario, ScenarioError> {
    let root = json::parse(bytes)?;
    decode::decode(&root)
}
