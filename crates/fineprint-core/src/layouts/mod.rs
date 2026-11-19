// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Base characteristics per layout and zone (REQ-OBJ-002).
//!
//! Given a card definition, the zone an object is in, how it was cast and its status
//! ([`LayoutQuery`]), a [`Layout`] returns the object's **base characteristics**, before any
//! layer effect, and its **mana value** (CR 202.3). Alternative characteristics (Adventure,
//! Omen, Prototype, the flipped half of a flip card) are plain definition data selected here; no
//! function in this module creates or returns a layer effect.
//!
//! Twelve layouts are supported, one unit struct each: [`NormalLayout`], [`SplitLayout`]
//! (CR 709), [`FlipLayout`] (CR 710), [`DfcNonmodalLayout`] and [`DfcModalLayout`] (CR 712),
//! [`AdventurerLayout`] (CR 716), [`OmenLayout`] (CR 721), [`PrototypeLayout`] (CR 719), and
//! [`LevelerLayout`] (CR 711), [`SagaLayout`] (CR 715), [`ClassLayout`] (CR 717) and
//! [`CaseLayout`] (CR 720), which select their printed characteristics in every zone and expose
//! their gating abilities as [`Gating`] data only. [`layout_for`] maps a
//! [`LayoutBehavior`](fineprint_dsl::card::LayoutBehavior) to its layout; every other behavior
//! is [`LayoutError::UnsupportedLayout`].
//!
//! Each layout requires an exact set of face kinds (looked up by kind, never by index), and the
//! cast choice is consulted only on the stack and, for a prototype card, the battlefield. Errors
//! are checked in the order [`LayoutError::LayoutMismatch`], [`LayoutError::BadFaces`],
//! [`LayoutError::InvalidCast`], [`LayoutError::Object`]. No function panics on any definition
//! shape.
//!
//! Mana value and colors are computed by [`crate::objects`] ([`crate::objects::mana_value`],
//! [`crate::objects::base_colors`]); nothing here reimplements that arithmetic.
//!
//! **Standalone scope.** This module is not wired into `GameState`, its Merkle layout or its wire
//! form; the integration unit does that. It is integer-only and uses no hash-ordered container
//! (REQ-CORE-006).
//!
//! **Out of scope here** (later work): the ROOM, MELD, ATTRACTION, STATION, PREPARATION and
//! MUTATE layouts, tokens (TOKEN, DFC_TOKEN), variant layouts (PLANAR, SCHEME, VANGUARD,
//! CONSPIRACY, DUNGEON), face-down status, copy effects (only the `copying_back_face` mana-value
//! flag is modelled), level/lore counters and designations, and the layer system.

mod alternative;
mod dfc;
mod faces;
mod flip;
mod printed;
mod split;
mod types;

#[cfg(test)]
mod tests;

use fineprint_dsl::card::{CardDefinitionDSL, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

pub use alternative::{AdventurerLayout, OmenLayout, PrototypeLayout};
pub use dfc::{DfcModalLayout, DfcNonmodalLayout};
pub use flip::FlipLayout;
pub use printed::{CaseLayout, ClassLayout, LevelerLayout, NormalLayout, SagaLayout};
pub use split::SplitLayout;
pub use types::{CastChoice, FaceUp, Gating, Layout, LayoutError, LayoutQuery, LayoutStatus};

use crate::objects::{Characteristics, ManaValueContext};

/// The layout object for `behavior`.
///
/// # Errors
/// [`LayoutError::UnsupportedLayout`] for every behavior outside the twelve supported layouts.
pub fn layout_for(behavior: LayoutBehavior) -> Result<&'static dyn Layout, LayoutError> {
    Ok(match behavior {
        LayoutBehavior::Normal => &NormalLayout,
        LayoutBehavior::Split => &SplitLayout,
        LayoutBehavior::Flip => &FlipLayout,
        LayoutBehavior::DfcNonmodal => &DfcNonmodalLayout,
        LayoutBehavior::DfcModal => &DfcModalLayout,
        LayoutBehavior::Adventurer => &AdventurerLayout,
        LayoutBehavior::Omen => &OmenLayout,
        LayoutBehavior::Prototype => &PrototypeLayout,
        LayoutBehavior::Leveler => &LevelerLayout,
        LayoutBehavior::Saga => &SagaLayout,
        LayoutBehavior::Class => &ClassLayout,
        LayoutBehavior::Case => &CaseLayout,
        LayoutBehavior::Room
        | LayoutBehavior::Meld
        | LayoutBehavior::Attraction
        | LayoutBehavior::Station
        | LayoutBehavior::Preparation
        | LayoutBehavior::Mutate
        | LayoutBehavior::Token
        | LayoutBehavior::DfcToken
        | LayoutBehavior::Planar
        | LayoutBehavior::Scheme
        | LayoutBehavior::Vanguard
        | LayoutBehavior::Conspiracy
        | LayoutBehavior::Dungeon => return Err(LayoutError::UnsupportedLayout(behavior)),
    })
}

/// The base characteristics of an object with definition `def`, using the layout `def.layout`
/// ([`Layout::base_characteristics`]).
///
/// # Errors
/// [`LayoutError::UnsupportedLayout`], then as [`Layout::base_characteristics`].
pub fn base_characteristics(
    def: &CardDefinitionDSL,
    query: &LayoutQuery,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    layout_for(def.layout)?.base_characteristics(def, query, vocab)
}

/// The mana value of an object with definition `def`, using the layout `def.layout`
/// ([`Layout::mana_value`]).
///
/// # Errors
/// [`LayoutError::UnsupportedLayout`], then as [`Layout::mana_value`].
pub fn mana_value(
    def: &CardDefinitionDSL,
    query: &LayoutQuery,
    ctx: ManaValueContext,
    vocab: &RulesVocabulary,
) -> Result<i64, LayoutError> {
    layout_for(def.layout)?.mana_value(def, query, ctx, vocab)
}

/// The gating data of `def`, using the layout `def.layout` ([`Layout::gating`]).
///
/// # Errors
/// [`LayoutError::UnsupportedLayout`], then as [`Layout::gating`].
pub fn gating(def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
    layout_for(def.layout)?.gating(def)
}
