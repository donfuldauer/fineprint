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
        // CR 107.4e, 202.2d: {C/c} is a colored symbol of exactly the one color c (colorless is
        // not a color).
        ManaSymbol::ColorlessHybrid(c) => color_mask(c),
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

/// What an object's characteristic-defining abilities do to its colors (CR 604.3, 105.2, 105.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CdaColors {
    /// No color-defining CDA: cost and indicator colors stand.
    #[default]
    None,
    /// The CDA adds these colors to the cost and indicator colors (CR 604.3 "add to").
    Add(ColorBitmask),
    /// The CDA sets the object's colors to exactly this set, overriding cost and indicator
    /// (CR 604.3 "override", CR 702.114a devoid = `Set(ColorBitmask::NONE)`).
    Set(ColorBitmask),
}

/// An object's colors before other continuous effects: its cost colors (CR 202.2, 105.2) and
/// color indicator (CR 202.2e), then the colors its characteristic-defining abilities define
/// (CR 604.3, 613.3: CDAs apply first).
///
/// - [`CdaColors::None`]: cost colors united with indicator colors.
/// - [`CdaColors::Add`]: that union plus the CDA's colors (CR 604.3 "add to").
/// - [`CdaColors::Set`]: exactly the CDA's colors, ignoring cost and indicator (CR 604.3
///   "override"); a devoid-style CDA is `Set(ColorBitmask::NONE)` and makes the object
///   colorless (CR 702.114a).
///
/// Other color-changing effects are applied in layer 5 (CR 613.1e) by later work.
#[must_use]
pub fn base_colors(
    mana_cost: Option<&ManaCostDSL>,
    color_indicator: Option<ColorBitmask>,
    cda: CdaColors,
) -> ColorBitmask {
    let from_cost = colors_of_mana_cost(mana_cost);
    let from_indicator = color_indicator.unwrap_or(ColorBitmask::NONE);
    let printed = union_colors(from_cost, from_indicator);
    match cda {
        // CR 105.2, 202.2, 202.2e: cost and indicator colors stand.
        CdaColors::None => printed,
        // CR 604.3: the CDA adds to the printed colors.
        CdaColors::Add(mask) => union_colors(printed, mask),
        // CR 604.3, 702.114a: the CDA overrides the printed colors.
        CdaColors::Set(mask) => mask,
    }
}

#[cfg(test)]
mod colorless_hybrid_tests {
    //! REQ-OBJ-001, REQ-DSL-002: colors of colorless-hybrid symbols (CR 107.4e, 202.2d).

    use fineprint_dsl::card::ColorBitmask;
    use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};

    use super::{color_mask, colors_of_mana_cost, symbol_colors, union_colors};

    fn colors(text: &str) -> ColorBitmask {
        colors_of_mana_cost(Some(&ManaCostDSL::parse(text).expect("cost")))
    }

    /// REQ-OBJ-001 (CR 107.4e, 202.2d): `{C/c}` is exactly the color c, never colorless.
    #[test]
    fn req_obj_001_cr_107_4e_colorless_hybrid_is_its_color() {
        for c in Color::ALL {
            let mask = symbol_colors(ManaSymbol::ColorlessHybrid(c));
            assert_eq!(mask, color_mask(c));
            assert_ne!(mask, ColorBitmask::NONE);
        }
        assert_eq!(colors("{C/W}"), ColorBitmask::WHITE);
    }

    /// REQ-OBJ-001, REQ-DSL-002 (CR 202.2, 202.2d): a cost is all the colors of its `{C/c}`
    /// symbols.
    #[test]
    fn req_obj_001_cr_202_2d_colorless_hybrid_cost_colors() {
        assert_eq!(
            colors("{C/W}{C/U}"),
            union_colors(ColorBitmask::WHITE, ColorBitmask::BLUE)
        );
        assert_eq!(colors("{C}{C/G}{1}"), ColorBitmask::GREEN);
    }
}
