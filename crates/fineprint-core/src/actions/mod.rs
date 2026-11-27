// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Legal-action generation for the player holding priority (REQ-DSL-013, REQ-ARCH-002,
//! SRS 14.6).
//!
//! [`legal_actions`] lists every legal non-pass action of the priority player (CR 117.1): play a
//! land (CR 116.2a, 305.1), cast a spell (CR 117.1a, 601.3), activate an ability (CR 117.1b,
//! 117.1d) and take a special action (CR 116.2, 117.1c), each as the exact
//! [`ChoiceResponse`](crate::choice::ChoiceResponse) that takes it, with the basis that allowed
//! it. [`priority_payload`] packs them into the `PRIORITY_ACTION` payload and [`explain`] says why
//! any action is legal or illegal (the basis of `explainLegality`).
//!
//! **Seam.** The generator is written against [`ActionHost`]: the host supplies the
//! [`RuleSet`](crate::layers::RuleSet) of pass B, the [`TimingContext`], the number of lands
//! played this turn and the candidates (objects that could be played, cast, activated or turned
//! face up, with the facts only the game state knows, such as "instant or flash" or "summoning
//! sick"). The generator consults ONLY permissions, prohibitions, additional land plays,
//! activation timing and limits and the [`SpecialActionRegistry`] (SRS 14.6); it never branches
//! on a card id, a card name or anything else card-specific (R4).
//!
//! **Prohibitions last (CR 101.2).** Every check of a candidate is evaluated, and an applicable
//! `PROHIBITION` makes the action illegal whatever permissions, timing permissions or additional
//! land plays exist and whatever their timestamps: "can't" always wins.
//!
//! **Parameter conventions** of the [`RuleEntry`](crate::layers::RuleEntry) params read here
//! (the DSL keeps them opaque; untrusted JSON of any shape, R12):
//!
//! - `"action"` (string): one of [`ACTION_PLAY_LAND`], [`ACTION_CAST`], [`ACTION_ACTIVATE`],
//!   [`ACTION_SPECIAL`]; an entry without a string `"action"` applies to nothing here.
//! - `"zone"` (string, optional): a zone wire name such as `"GRAVEYARD"`; the entry applies only
//!   to candidates in that zone. Absent means any zone; any other value (a wrong type or an
//!   unknown name) matches nothing. Not read for special actions (their candidates carry no
//!   zone).
//! - `"object"` (unsigned JSON integer, optional): the entry applies only to the candidate
//!   object with that raw id (for `ACTIVATE`, the ability's source). Absent means any object; any
//!   other value matches nothing.
//! - `"kind"` (string, optional, `SPECIAL_ACTION` prohibitions only): the special-action kind id;
//!   absent means every kind; any other value matches nothing.
//! - Only entries that modify the rules for the asked player count
//!   ([`RuleSet::applying`](crate::layers::RuleSet::applying)); "earliest" is application order.
//! - `PLAY_LAND` prohibitions and the land count come unfiltered from
//!   [`RuleSet::query`](crate::layers::RuleSet::query) and
//!   [`RuleSet::land_plays_allowed`](crate::layers::RuleSet::land_plays_allowed) (one source of
//!   truth); the `"zone"` / `"object"` filters are honoured on `PLAY_PERMISSION` entries only.
//!
//! **Determinism (R3).** Only `Vec`, `BTreeMap` and `BTreeSet`; no floats, clock or I/O. The
//! candidates of each class are put in a total order first, so the output never depends on the
//! host's candidate order. The output is ordered by class (`PLAY_LAND`, `INITIATE_CAST`,
//! `ACTIVATE`, `SPECIAL_ACTION`) and within a class ascending by the response's fields, exact
//! duplicates removed.

#![deny(missing_docs)]

mod generate;
mod registry;
mod types;

#[cfg(test)]
mod tests;

pub use generate::{explain, legal_actions, priority_actions, priority_payload};
pub use registry::{SpecialActionKind, SpecialActionRegistry, SpecialTiming};
pub use types::{
    AbilityCandidate, ActionHost, Allowance, CastCandidate, FaceDownOrigin, FaceUpDenial,
    LandCandidate, LegalAction, Legality, Reason, SpecialCandidate, TimingContext,
    TimingRequirement, TurnFaceUpCandidate,
};

/// The `"action"` of rule entries about playing a land (CR 116.2a, 305.1); the same string
/// [`RuleSet::land_plays_allowed`](crate::layers::RuleSet::land_plays_allowed) reads.
pub const ACTION_PLAY_LAND: &str = "PLAY_LAND";

/// The `"action"` of rule entries about casting a spell (CR 601.3).
pub const ACTION_CAST: &str = "CAST";

/// The `"action"` of rule entries about activating an ability (CR 602.5).
pub const ACTION_ACTIVATE: &str = "ACTIVATE";

/// The `"action"` of rule entries about taking a special action (CR 116.2).
pub const ACTION_SPECIAL: &str = "SPECIAL_ACTION";
