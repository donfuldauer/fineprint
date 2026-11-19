// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Colors from the mana cost, color indicator and CDAs (REQ-OBJ-001, CR 105.2, 202.2).

use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};

/// The single-color mask of `color` (CR 105.2).
#[must_use]
pub fn color_mask(color: Color) -> ColorBitmask {
    match color {
        Color::White => ColorBitmask::WHITE,
        Color::Blue => ColorBitmask::BLUE,
        Color::Black => ColorBitmask::BLACK,
        Color::Red => ColorBitmask::RED,
        Color::Green => ColorBitmask::GREEN,
    }
}

/// The union of two color masks (CR 202.2c). Never exceeds the five colors.
#[must_use]
pub fn union_colors(a: ColorBitmask, b: ColorBitmask) -> ColorBitmask {
    // Both inputs are at most 31, so the bitwise or is too; the fallback is unreachable.
    ColorBitmask::new(a.bits() | b.bits()).unwrap_or(ColorBitmask::NONE)
}

/// The colors of one mana symbol (CR 202.2, 107.4c, 107.4e, 107.4f, 107.4h).
#[must_use]
pub fn symbol_colors(symbol: ManaSymbol) -> ColorBitmask {
    match symbol {
        // CR 202.2: the color of a colored mana symbol.
        ManaSymbol::Colored(c) => color_mask(c),
        // CR 107.4e, 107.4f, 202.2d: a hybrid (Phyrexian) symbol is all of its component colors.
        ManaSymbol::Hybrid(a, b) | ManaSymbol::HybridPhyrexian(a, b) => {
            union_colors(color_mask(a), color_mask(b))
        }
        // CR 107.4e: {n/c} is c; CR 107.4f, 202.2d: {c/P} is c.
        ManaSymbol::GenericHybrid(_, c) | ManaSymbol::Phyrexian(c) => color_mask(c),
        // CR 107.4b, 107.4c, 107.4h, 202.2b: generic, variable, colorless and snow have no color.
        ManaSymbol::Generic(_)
        | ManaSymbol::Colorless
        | ManaSymbol::Snow
        | ManaSymbol::Variable(_) => ColorBitmask::NONE,
    }
}

/// The colors of a mana cost; no mana cost is colorless (CR 202.2, 202.2b, 202.2c).
#[must_use]
pub fn colors_of_mana_cost(cost: Option<&ManaCostDSL>) -> ColorBitmask {
    cost.map_or(ColorBitmask::NONE, |c| {
        c.symbols().iter().fold(ColorBitmask::NONE, |acc, s| {
            union_colors(acc, symbol_colors(*s))
        })
    })
}

/// An object's colors before continuous effects: the union of its cost colors (CR 202.2), its
/// color indicator (CR 202.2e) and the colors its characteristic-defining abilities define
/// (CR 105.2).
#[must_use]
pub fn base_colors(
    mana_cost: Option<&ManaCostDSL>,
    color_indicator: Option<ColorBitmask>,
    cda_colors: ColorBitmask,
) -> ColorBitmask {
    let from_cost = colors_of_mana_cost(mana_cost);
    let from_indicator = color_indicator.unwrap_or(ColorBitmask::NONE);
    union_colors(union_colors(from_cost, from_indicator), cda_colors)
}
