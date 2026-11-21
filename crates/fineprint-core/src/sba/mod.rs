// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! State-based actions (REQ-SBA-001, SRS 9, CR 704).
//!
//! **Registry.** [`SbaRegistry::standard`] holds one [`SbaEntry`] per rule CR 704.5a-704.5i,
//! each with its id, CR reference, predicate, [`SbaAction`] and [`VariantGate`]. A new
//! state-based action is a new entry, not a new control path. Cards contribute nothing here
//! (REQ-DSL-013): every predicate is generic.
//!
//! **One pass** (CR 704.3). [`check`] evaluates every active entry against one snapshot and
//! changes nothing. [`perform_all`] checks, then performs every result simultaneously as one
//! event: the graveyard moves as ONE batch through the replacement bus with CR 704.7 merging
//! (CR 704.8: last known information from before the pass), ceasing to exist
//! (CR 704.5d, CR 704.5e) and the [`LOST_THE_GAME`] designation (CR 704.5a-704.5c). The loop
//! of CR 704.3 (repeat while something was performed, then put waiting triggers on the stack)
//! belongs to the heartbeat.
//!
//! **Inputs.** Toughness, abilities and card types are the evaluated characteristics
//! (CR 613.1, CR 613.4) from the caller's [`ContinuousEffects`](crate::layers::ContinuousEffects)
//! and [`LayerEnvironment`](crate::layers::LayerEnvironment), never from `GameState`; loyalty
//! is the number of loyalty counters the layer environment reports (CR 306.5c); damage marked
//! and token marks come from the object markers; copies of spells and cards, the CR 723.3c
//! exemption, deathtouch damage and the game variant come from the caller's
//! [`SbaEnvironment`].
//!
//! **Not modelled here.** Regeneration (CR 701.8c), the Two-Headed Giant team checks
//! (CR 704.6a, CR 704.6b), the entries after CR 704.5i, and choosing a winner or a draw
//! (CR 104.4a).
//!
//! **Determinism (REQ-CORE-001, REQ-CORE-006).** Ordered containers only, ascending-id
//! iteration, integer arithmetic, no clock.

#![deny(missing_docs)]

mod perform;
mod registry;
mod types;

#[cfg(test)]
mod tests;

pub use perform::{check, perform_all};
pub use registry::{SbaAction, SbaEntry, SbaRegistry, VariantGate};
pub use types::{
    GameVariant, LOST_THE_GAME, SbaEnvironment, SbaFinding, SbaOutcome, SbaSubject,
    StoredSbaEnvironment,
};
