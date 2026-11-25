// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Integer recomputation of colors, mana value and color identity from parsed mana costs
//! (REQ-ING-002).
//!
//! These are small, pure functions with integer arithmetic only (R3). `fineprint-core` has the
//! in-game equivalents, but `fineprint-ingest` depends on `fineprint-base` and `fineprint-dsl`
//! only (REQ-NFR-005), so the import keeps its own copy (DECISIONS.md "#87 FP-085 —
//! Recomputation duplicated rather than a core dependency"). A card in a bulk file is off the
//! stack, so `{X}`, `{Y}` and `{Z}` count 0 here.

use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};

/// The single-color mask of `color` (W = 1, U = 2, B = 4, R = 8, G = 16).
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

/// The union of two masks. Both are at most 31, so the result is too.
#[must_use]
pub fn union(a: ColorBitmask, b: ColorBitmask) -> ColorBitmask {
    ColorBitmask::new(a.bits() | b.bits()).unwrap_or(a)
}

/// The colors of one mana symbol (CR 202.2, 202.2d: a hybrid or Phyrexian symbol is all of its
/// component colors; generic, colorless, snow and variable symbols have no color).
#[must_use]
pub fn symbol_colors(symbol: ManaSymbol) -> ColorBitmask {
    match symbol {
        // CR 202.2: the color of a colored mana symbol.
        ManaSymbol::Colored(c) | ManaSymbol::GenericHybrid(_, c) | ManaSymbol::Phyrexian(c) => {
            color_mask(c)
        }
        // CR 202.2d: every component color of a hybrid symbol.
        ManaSymbol::Hybrid(a, b) | ManaSymbol::HybridPhyrexian(a, b) => {
            union(color_mask(a), color_mask(b))
        }
        ManaSymbol::Generic(_)
        | ManaSymbol::Colorless
        | ManaSymbol::Snow
        | ManaSymbol::Variable(_) => ColorBitmask::NONE,
    }
}

/// The colors of a mana cost; no mana cost is colorless (CR 202.2, 202.2b, 202.2c).
#[must_use]
pub fn cost_colors(cost: Option<&ManaCostDSL>) -> ColorBitmask {
    cost.map_or(ColorBitmask::NONE, |c| {
        c.symbols()
            .iter()
            .fold(ColorBitmask::NONE, |acc, s| union(acc, symbol_colors(*s)))
    })
}

/// A face's computed colors: the union of the colors of the mana cost (CR 105.2) and of the
/// color indicator (CR 202.2e, 204.2). A card with both is an inference (no CR example covers
/// both on one face), matching `fineprint-core`'s `base_colors`; see DECISIONS.md "#87 FP-085 —
/// Color indicator and cost colors". Neither gives colorless.
#[must_use]
pub fn face_colors(cost: Option<&ManaCostDSL>, indicator: Option<ColorBitmask>) -> ColorBitmask {
    union(cost_colors(cost), indicator.unwrap_or(ColorBitmask::NONE))
}

/// The card-level colors off the stack from the faces' computed colors, by Scryfall `layout`:
///
/// - `split` with several faces: the union of every face (CR 709.4, 709.4b);
/// - every other layout: the first (front, normal or main) face (CR 716.4 adventure, CR 710.2
///   flip, CR 712.6a transform, modal and other double-faced layouts);
/// - no faces: colorless.
#[must_use]
pub fn record_colors(layout: &str, face_colors: &[ColorBitmask]) -> ColorBitmask {
    if layout == "split" && face_colors.len() > 1 {
        face_colors
            .iter()
            .fold(ColorBitmask::NONE, |acc, c| union(acc, *c))
    } else {
        face_colors.first().copied().unwrap_or(ColorBitmask::NONE)
    }
}

/// The mana value one symbol contributes off the stack (CR 202.3).
#[must_use]
pub fn symbol_mana_value(symbol: ManaSymbol) -> u64 {
    match symbol {
        ManaSymbol::Generic(n) => u64::from(n),
        ManaSymbol::Colored(_) | ManaSymbol::Colorless | ManaSymbol::Snow => 1,
        // CR 202.3f: a hybrid symbol counts its largest component; `{n/c}` counts n.
        ManaSymbol::Hybrid(_, _) => 1,
        ManaSymbol::GenericHybrid(n, _) => u64::from(n),
        // CR 202.3g: each Phyrexian symbol counts 1.
        ManaSymbol::Phyrexian(_) | ManaSymbol::HybridPhyrexian(_, _) => 1,
        // CR 202.3e: X, Y and Z are 0 off the stack.
        ManaSymbol::Variable(_) => 0,
    }
}

/// The mana value of a cost; no mana cost is 0 (CR 202.3, 202.3a). Saturating sums.
#[must_use]
pub fn cost_mana_value(cost: Option<&ManaCostDSL>) -> u64 {
    cost.map_or(0, |c| {
        c.symbols()
            .iter()
            .fold(0u64, |acc, s| acc.saturating_add(symbol_mana_value(*s)))
    })
}

/// The record-level mana value from the faces' costs, by Scryfall `layout` (CR 202.3):
///
/// - one face: that face's cost;
/// - `split`: the sum of every face (CR 202.3d, 709.4b, off the stack);
/// - `adventure` (CR 716.4) and `flip` (CR 710.2): the first face;
/// - every other layout with several faces (`transform`, `modal_dfc`, `meld`,
///   `reversible_card`, ...): the front (first) face (CR 202.3b, 712.6a; melded permanents, CR 202.3c,
///   are not computed at import).
#[must_use]
pub fn record_mana_value(layout: &str, face_costs: &[Option<&ManaCostDSL>]) -> u64 {
    if layout == "split" && face_costs.len() > 1 {
        face_costs
            .iter()
            .fold(0u64, |acc, c| acc.saturating_add(cost_mana_value(*c)))
    } else {
        face_costs.first().map_or(0, |c| cost_mana_value(*c))
    }
}

/// The colors of the mana symbols in `text` outside reminder text (CR 903.4, 903.4c).
///
/// A `(` ... `)` pair is reminder text and is skipped; nesting is tracked with a depth counter
/// and an unclosed `(` runs to the end of the text (a `)` at depth 0 is ignored). Outside
/// parentheses every `{` ... `}` group is parsed with `ManaSymbol::parse`; a group that does not
/// parse (such as a tap symbol) is ignored. Never panics: the scan walks `char_indices` and
/// slices only at char boundaries.
#[must_use]
pub fn text_symbol_colors(text: &str) -> ColorBitmask {
    let mut acc = ColorBitmask::NONE;
    let mut depth = 0usize;
    let mut open: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        match ch {
            '(' => {
                depth = depth.saturating_add(1);
                open = None;
            }
            ')' => depth = depth.saturating_sub(1),
            '{' if depth == 0 => open = Some(i),
            '}' if depth == 0 => {
                if let Some(start) = open.take() {
                    // `i + 1` is a char boundary: `}` is one byte.
                    if let Some(sym) = text.get(start..=i)
                        && let Ok(s) = ManaSymbol::parse(sym)
                    {
                        acc = union(acc, symbol_colors(s));
                    }
                }
            }
            _ => {}
        }
    }
    acc
}

/// The color identity of a record (CR 903.4): the union over every face (the back face of a
/// double-faced card included, CR 903.4d; an Adventure's face too) of the face's computed
/// colors (the union of its cost colors and color indicator, see [`face_colors`]) and the colors of the mana symbols in its rules text outside
/// reminder text (CR 903.4c).
#[must_use]
pub fn color_identity(faces: &[(ColorBitmask, Option<&str>)]) -> ColorBitmask {
    faces
        .iter()
        .fold(ColorBitmask::NONE, |acc, (colors, text)| {
            union(
                acc,
                union(*colors, text.map_or(ColorBitmask::NONE, text_symbol_colors)),
            )
        })
}

/// The mask for a list of color letters `W U B R G`; `None` for any other letter.
#[must_use]
pub fn letters_mask<'a>(letters: impl IntoIterator<Item = &'a str>) -> Option<ColorBitmask> {
    let mut acc = ColorBitmask::NONE;
    for l in letters {
        let mut chars = l.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return None;
        };
        acc = union(acc, color_mask(Color::from_letter(c)?));
    }
    Some(acc)
}

/// The color letters of a mask in `W U B R G` order (for reports).
#[must_use]
pub fn mask_letters(mask: ColorBitmask) -> String {
    Color::ALL
        .iter()
        .filter(|c| mask.bits() & color_mask(**c).bits() != 0)
        .map(|c| c.letter())
        .collect()
}

/// Whether a JSON number `cmc` equals the integer mana value `mv`: the number must be finite,
/// `cmc * 2` must be exactly a whole number, and it must equal `mv * 2` (Scryfall emits whole or
/// `.5` values; there is no tolerance). This is the only floating-point use in the import, a
/// validation step that never produces a rules value.
#[must_use]
pub fn cmc_matches(cmc: f64, mv: u64) -> bool {
    if !cmc.is_finite() {
        return false;
    }
    let doubled = cmc * 2.0;
    // Exact: a value that is not a whole or half number never matches (no tolerance).
    if doubled.fract() != 0.0 {
        return false;
    }
    // 2^53: every integer up to here is exact in f64.
    if !(0.0..=9_007_199_254_740_992.0).contains(&doubled) {
        return false;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let doubled = doubled as u64;
    mv.checked_mul(2) == Some(doubled)
}
