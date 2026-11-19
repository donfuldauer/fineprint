// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Color identity for Commander (REQ-OBJ-001, CR 903.4).

use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{ManaCostDSL, ManaSymbol};

use super::colors::{base_colors, symbol_colors, union_colors};

/// The color-identity inputs of one face or set of alternative characteristics (CR 903.4).
///
/// `rules_text_symbols` are the mana symbols of the face's rules text; the caller must leave out
/// symbols that appear only in reminder text (CR 903.4c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityFace<'a> {
    /// The face's mana cost.
    pub mana_cost: Option<&'a ManaCostDSL>,
    /// The face's color indicator.
    pub color_indicator: Option<ColorBitmask>,
    /// The colors the face's characteristic-defining abilities define.
    pub cda_colors: ColorBitmask,
    /// Mana symbols in the face's rules text, reminder text excluded (CR 903.4c).
    pub rules_text_symbols: &'a [ManaSymbol],
}

/// The color identity of a card (CR 903.4): the colors of every mana symbol in each face's cost
/// and rules text, plus its color indicators and CDA colors, plus `chosen_color` when a color
/// was chosen before the game began (CR 903.4b).
///
/// The caller passes every face, including the back face of a double-faced card (CR 903.4d) and
/// any alternative characteristics (CR 903.4e). The result is a property of the card, fixed as
/// the game begins, not of the in-game object (CR 903.4a). An empty list with no choice is
/// colorless.
#[must_use]
pub fn color_identity(
    faces: &[IdentityFace<'_>],
    chosen_color: Option<ColorBitmask>,
) -> ColorBitmask {
    // CR 903.4b: a pre-game color choice is part of the identity.
    let start = chosen_color.unwrap_or(ColorBitmask::NONE);
    faces.iter().fold(start, |acc, face| {
        // CR 903.4: cost symbols, color indicator and CDA colors.
        let face_colors = base_colors(face.mana_cost, face.color_indicator, face.cda_colors);
        // CR 903.4: mana symbols in rules text (reminder text excluded by the caller, 903.4c).
        let text_colors = face
            .rules_text_symbols
            .iter()
            .fold(ColorBitmask::NONE, |m, s| {
                union_colors(m, symbol_colors(*s))
            });
        union_colors(acc, union_colors(face_colors, text_colors))
    })
}
