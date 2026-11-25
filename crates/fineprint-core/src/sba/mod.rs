// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! State-based actions (REQ-SBA-001, SRS 9, CR 704).
//!
//! **Registry.** [`SbaRegistry::standard`] holds one [`SbaEntry`] per rule CR 704.5a-704.5t and
//! CR 704.5v-704.5aa (the CR has no 704.5l and no 704.5o; CR 704.5u is not modelled here), each with its id, CR reference, predicate, [`SbaAction`] and [`VariantGate`]. A new
//! state-based action is a new entry, not a new control path. Cards contribute nothing here
//! (REQ-DSL-013): every predicate is generic.
//!
//! **One pass** (CR 704.3). [`check`] evaluates every active entry against one snapshot and
//! changes nothing. [`perform_all`] checks, then performs every result simultaneously as one
//! event: the graveyard moves as ONE batch through the replacement bus with CR 704.7 merging
//! (CR 704.8: last known information from before the pass), together with the counter
//! removals (CR 704.5q, CR 704.5r) and the unattach events (CR 704.5n, CR 704.5p) of every
//! applicable permanent, including one that also moves; the legend-rule choices
//! (CR 704.5j) and the battle protector choices (CR 704.5x, CR 704.5y), asked in APNAP order
//! (CR 101.4) before anything changes; ceasing to exist
//! (CR 704.5d, CR 704.5e); removing a completed dungeon from the game (CR 704.5t); and the
//! [`LOST_THE_GAME`] designation (CR 704.5a-704.5c); the [`SPEED`] designation (CR 704.5aa).
//! The loop
//! of CR 704.3 (repeat while something was performed, then put waiting triggers on the stack)
//! belongs to the heartbeat.
//!
//! **Inputs.** Toughness, abilities and card types are the evaluated characteristics
//! (CR 613.1, CR 613.4) from the caller's [`ContinuousEffects`](crate::layers::ContinuousEffects)
//! and [`LayerEnvironment`](crate::layers::LayerEnvironment), never from `GameState`; loyalty
//! is the number of loyalty counters the layer environment reports (CR 306.5c); damage marked
//! and token marks come from the object markers; copies of spells and cards, the CR 723.3c
//! exemption, deathtouch damage, the game variant, how long each world permanent has had the
//! world supertype (CR 704.5k), Aura legality by enchant ability (CR 704.5m), Equipment and
//! Fortification legality by effects such as protection (CR 704.5n, CR 702.16d), counter limits
//! (CR 704.5r), Saga chapter data (CR 704.5s), completed dungeons (CR 704.5t), battle
//! protectors, protector legality by effects and pending battle abilities (CR 704.5v-704.5y) and
//! attachment timestamps (CR 704.5z, CR 613.7e) come from the caller's [`SbaEnvironment`]. Names, supertypes, subtypes, counters and controllers
//! (CR 613.1b) are evaluated characteristics and the layer environment's counters; attachments
//! and combat (whether a battle is being attacked, CR 310.11) come from the permanent container;
//! a player's speed is the [`SPEED`] player designation (CR 702.179b).
//!
//! **Not modelled here.** Regeneration (CR 701.8c), the Two-Headed Giant team checks
//! (CR 704.6a, CR 704.6b), CR 704.5u, and choosing a winner or a draw (CR 104.4a).
//!
//! **Determinism (REQ-CORE-001, REQ-CORE-006).** Ordered containers only, ascending-id
//! iteration, integer arithmetic, no clock.

#![deny(missing_docs)]

mod perform;
#[cfg(test)]
use perform::{Saved, legend_survivors, protector_answer};
#[cfg(test)]
use registry::LegendGroup;
#[cfg(test)]
use registry::ProtectorAsk;
mod registry;
#[cfg(test)]
use registry::target_self_or_gone;
mod types;

#[cfg(test)]
mod tests;

pub use perform::{check, perform_all};
pub use registry::{SbaAction, SbaEntry, SbaRegistry, VariantGate};
pub use types::{
    GameVariant, LOST_THE_GAME, SPEED, SbaEnvironment, SbaFinding, SbaOutcome, SbaSubject,
    StoredSbaEnvironment,
};
