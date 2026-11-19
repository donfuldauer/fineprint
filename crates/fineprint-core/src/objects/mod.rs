// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Object characteristics from a definition, before continuous effects (REQ-OBJ-001).
//!
//! [`Characteristics`] holds the CR 109.3 characteristics of one card face, copied one to one
//! from a [`CardFaceDSL`](fineprint_dsl::card::CardFaceDSL) by [`Characteristics::from_face`],
//! which also checks the type line against the
//! [`RulesVocabulary`](fineprint_dsl::vocab::RulesVocabulary) ([`validate_types`], CR 205).
//!
//! **Mana value** ([`mana_value`], [`symbol_mana_value`]) follows CR 202.3 through 202.3g for
//! every subject of [`ManaValueSubject`]: an object's own cost, the back face of a nonmodal
//! double-faced object and copies of it (CR 202.3b), melded permanents and copies of them
//! (CR 202.3c), split cards off and on the stack (CR 202.3d), and `{X}` (CR 202.3e), hybrid
//! (CR 202.3f) and Phyrexian (CR 202.3g) symbols. "No mana cost" (`None`) and the cost `{0}` both
//! have mana value 0 (CR 202.3a), but they differ in [`Characteristics::mana_cost`] and wherever a
//! back face or melded permanent takes its cost from a front face.
//!
//! **Colors** ([`base_colors`], [`symbol_colors`]) come from the mana cost, the color indicator
//! and the colors a characteristic-defining ability defines (CR 105.2, 202.2).
//!
//! **Color identity** ([`color_identity`]) is the Commander property of a card, not of the
//! in-game object (CR 903.4, 903.4a), and includes every face the caller passes (CR 903.4d,
//! 903.4e) and a color chosen before the game (CR 903.4b).
//!
//! **Standalone scope.** This module is not wired into `GameState`, its Merkle layout or its wire
//! form; the integration unit does that. Every collection is a `BTreeSet`, and every computation
//! is integer-only (REQ-CORE-006).
//!
//! **Out of scope here** (later work): choosing which face or faces of a card an object uses
//! (layout selection), characteristics after the layer system applies continuous effects,
//! characteristic-defining abilities (their colors are an input here), parsing mana symbols out
//! of rules text (an input to [`color_identity`]), and types granted by effects in play.

mod characteristics;
mod colors;
mod error;
mod identity;
mod mana_value;
mod validation;

#[cfg(test)]
mod tests;

pub use characteristics::Characteristics;
pub use colors::{
    CdaColors, base_colors, color_mask, colors_of_mana_cost, symbol_colors, union_colors,
};
pub use error::ObjectError;
pub use identity::{IdentityFace, color_identity};
pub use mana_value::{
    ManaValueContext, ManaValueSubject, SplitHalf, SplitState, VariableValues, mana_value,
    symbol_mana_value,
};
pub use validation::{can_gain_subtype, validate_types};
