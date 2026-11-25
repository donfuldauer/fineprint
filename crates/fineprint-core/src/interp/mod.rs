// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The DSL interpreter's expression layer (REQ-DSL-003, REQ-ARCH-002, REQ-CORE-006).
//!
//! Evaluates the SRS §14.4.0 expression language used by every ability: references
//! ([`resolve_reference`]), player and object selectors ([`resolve_players`],
//! [`select_objects`]), predicates over layer-evaluated characteristics
//! ([`eval_predicate_object`], [`eval_predicate_player`]) and integer value expressions
//! ([`eval_value`]: `LITERAL`, `X`, `COUNT`, `ATTR`, `PLAYER_ATTR`, `MATH`, `DISTINCT`, `HISTORY`,
//! `MEMORY`), inside an [`EvalContext`] (source, controller, X, targets, chosen values,
//! bindings, linked objects).
//!
//! **Impossible instructions (CR 101.3).** A reference that names nothing, or names a player or
//! object that is gone, is reported in [`Resolution::ignored`] while the rest is still returned;
//! in a value it contributes 0. [`partition_possible`] splits any list of parts the same way.
//! Only a malformed expression is an [`InterpError`].
//!
//! **Information (CR 608.2h, CR 113.7a, CR 608.2b).** An object is read from its current
//! information while it is in a zone, otherwise from its last known information. A static
//! ability never gets LKI (CR 604.7); that is the caller's choice of what to pass.
//!
//! **Boundary.** The game is read only through the [`InterpEnv`] trait; the integration unit
//! implements it on the real state. Evaluation is integer-only, has no side effects and is a
//! pure function of the environment and the context (REQ-CORE-006): ordered containers only,
//! selector results sorted on a stable key, APNAP order (CR 101.4) from the environment's seat
//! order.
//!
//! **Hidden information (R12).** [`ObjectView`], Memory and LKI data are omniscient; nothing here
//! is shown to a player except through the redacted view (REQ-CORE-004).

mod expr;
mod resolve;
mod types;

#[cfg(test)]
mod tests;

pub use expr::{compare, eval_predicate_object, eval_predicate_player, eval_value};
pub use resolve::{partition_possible, resolve_players, resolve_reference, select_objects};
pub use types::{
    ChosenValue, EvalContext, Ignored, InterpEnv, InterpError, ObjectView, Partition, PlayerStat,
    Referent, Resolution,
};
