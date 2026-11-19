// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::objects` colors and color identity. Black-box: written from
//! the work-item brief only.
//!
//! Colors from mana cost, color indicator and characteristic-defining abilities (CR 105.2,
//! 202.2, 107.4) and color identity (CR 903.4, 903.4b-e) (REQ-OBJ-001).

use fineprint_core::objects::{
    IdentityFace, base_colors, color_identity, color_mask, colors_of_mana_cost, symbol_colors,
    union_colors,
};
use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol, Variable};

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits in 0..=31")
}

fn colors_of(text: &str) -> ColorBitmask {
    let c = cost(text);
    colors_of_mana_cost(Some(&c))
}

fn face<'a>(mana_cost: Option<&'a ManaCostDSL>) -> IdentityFace<'a> {
    IdentityFace {
        mana_cost,
        color_indicator: None,
        cda_colors: ColorBitmask::NONE,
        rules_text_symbols: &[],
    }
}

const W: u8 = 1;
const U: u8 = 2;
const B: u8 = 4;
const R: u8 = 8;
const G: u8 = 16;

// ---------------------------------------------------------------------------------------------
// Case 11: colors (CR 202.2, 105.2, 107.4)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 202.2, CR 202.2d, CR 107.4e, CR 107.4f: monocolored hybrid and Phyrexian
/// symbols give their colors: `{2/B}{2/B}{G/P}` is black and green.
#[test]
fn color_202_2d_generic_hybrid_and_phyrexian_colors() {
    assert_eq!(colors_of("{2/B}{2/B}{G/P}"), mask(B | G));
}

/// REQ-OBJ-001, CR 202.2b, CR 107.4c, CR 107.4h: `{C}`, `{S}`, generic and variable symbols
/// carry no color, and an object with no mana cost has no color from its cost.
#[test]
fn color_202_2b_colorless_symbols_and_no_cost_are_colorless() {
    for text in ["{C}", "{S}", "{3}", "{X}", "{0}", "{X}{Y}{2}{C}{S}"] {
        assert_eq!(colors_of(text), ColorBitmask::NONE, "{text}");
    }
    assert_eq!(colors_of_mana_cost(None), ColorBitmask::NONE);
}

/// REQ-OBJ-001, CR 202.2d, CR 107.4f: a hybrid Phyrexian symbol is both of its colors.
#[test]
fn color_107_4f_hybrid_phyrexian_is_both_colors() {
    assert_eq!(colors_of("{W/U/P}"), mask(W | U));
}

/// REQ-OBJ-001, CR 107.4e: `{2/W}` is white.
#[test]
fn color_107_4e_generic_hybrid_is_its_color() {
    assert_eq!(colors_of("{2/W}"), mask(W));
}

/// REQ-OBJ-001, CR 202.2d, CR 107.4e: `{W/U}` is white and blue.
#[test]
fn color_107_4e_hybrid_is_both_colors() {
    assert_eq!(colors_of("{W/U}"), mask(W | U));
}

/// REQ-OBJ-001, CR 202.2: `{R}{R}` is red (repeats do not change the set).
#[test]
fn color_202_2_repeated_symbol_is_one_color() {
    assert_eq!(colors_of("{R}{R}"), mask(R));
}

/// REQ-OBJ-001, CR 202.2: all five colors in one cost give bits 31.
#[test]
fn color_202_2_all_five_colors() {
    let all = colors_of("{W}{U}{B}{R}{G}");
    assert_eq!(all, mask(31));
    assert_eq!(all.bits(), 31);
}

/// REQ-OBJ-001, CR 202.2, CR 202.2d: `symbol_colors` for every symbol variant.
#[test]
fn color_202_2_symbol_colors_every_variant() {
    for c in Color::ALL {
        assert_eq!(symbol_colors(ManaSymbol::Colored(c)), color_mask(c));
        assert_eq!(symbol_colors(ManaSymbol::Phyrexian(c)), color_mask(c));
        assert_eq!(
            symbol_colors(ManaSymbol::GenericHybrid(2, c)),
            color_mask(c)
        );
        for d in Color::ALL {
            if c == d {
                continue;
            }
            let both = union_colors(color_mask(c), color_mask(d));
            assert_eq!(symbol_colors(ManaSymbol::Hybrid(c, d)), both);
            assert_eq!(symbol_colors(ManaSymbol::HybridPhyrexian(c, d)), both);
        }
    }
    for n in [0u32, 1, 5] {
        assert_eq!(symbol_colors(ManaSymbol::Generic(n)), ColorBitmask::NONE);
    }
    assert_eq!(symbol_colors(ManaSymbol::Colorless), ColorBitmask::NONE);
    assert_eq!(symbol_colors(ManaSymbol::Snow), ColorBitmask::NONE);
    for v in Variable::ALL {
        assert_eq!(symbol_colors(ManaSymbol::Variable(v)), ColorBitmask::NONE);
    }
}

/// REQ-OBJ-001, CR 202.2, CR 202.2e, CR 105.2: base colors are the union of the cost colors
/// and the color indicator.
#[test]
fn color_202_2e_indicator_unions_with_cost() {
    let c = cost("{1}{R}");
    assert_eq!(
        base_colors(Some(&c), Some(mask(U)), ColorBitmask::NONE),
        mask(R | U)
    );
    // An indicator naming a color already in the cost changes nothing.
    assert_eq!(
        base_colors(Some(&c), Some(mask(R)), ColorBitmask::NONE),
        mask(R)
    );
    assert_eq!(base_colors(Some(&c), None, ColorBitmask::NONE), mask(R));
}

/// REQ-OBJ-001, CR 105.2: colors defined by a characteristic-defining ability are included.
#[test]
fn color_105_2_cda_colors_are_included() {
    let c = cost("{2}{W}");
    assert_eq!(base_colors(Some(&c), None, mask(B | G)), mask(W | B | G));
    assert_eq!(base_colors(None, None, mask(31)), mask(31));
    assert_eq!(
        base_colors(Some(&c), Some(mask(U)), mask(R)),
        mask(W | U | R)
    );
}

/// REQ-OBJ-001, CR 202.2e, CR 202.2b: a no-cost face with an indicator has the indicator's
/// colors; with neither, it is colorless.
#[test]
fn color_202_2e_indicator_on_no_cost_face() {
    assert_eq!(
        base_colors(None, Some(mask(G | U)), ColorBitmask::NONE),
        mask(G | U)
    );
    assert_eq!(
        base_colors(None, None, ColorBitmask::NONE),
        ColorBitmask::NONE
    );
    assert_eq!(
        base_colors(None, Some(ColorBitmask::NONE), ColorBitmask::NONE),
        ColorBitmask::NONE
    );
}

/// REQ-OBJ-001, CR 202.2: `color_mask` of each color is the matching constant.
#[test]
fn color_202_2_color_mask_matches_constants() {
    assert_eq!(color_mask(Color::White), ColorBitmask::WHITE);
    assert_eq!(color_mask(Color::Blue), ColorBitmask::BLUE);
    assert_eq!(color_mask(Color::Black), ColorBitmask::BLACK);
    assert_eq!(color_mask(Color::Red), ColorBitmask::RED);
    assert_eq!(color_mask(Color::Green), ColorBitmask::GREEN);
    let all = Color::ALL.iter().fold(ColorBitmask::NONE, |acc, c| {
        union_colors(acc, color_mask(*c))
    });
    assert_eq!(all, mask(31));
}

/// REQ-OBJ-001, CR 202.2: `union_colors` is set union: commutative, associative, idempotent,
/// with `NONE` as identity, over all 32 x 32 masks.
#[test]
fn color_202_2_union_colors_is_set_union() {
    let masks: Vec<ColorBitmask> = (0u8..32).map(mask).collect();
    for a in &masks {
        assert_eq!(union_colors(*a, ColorBitmask::NONE), *a);
        assert_eq!(union_colors(ColorBitmask::NONE, *a), *a);
        assert_eq!(union_colors(*a, *a), *a);
        for b in &masks {
            let ab = union_colors(*a, *b);
            assert_eq!(ab, union_colors(*b, *a));
            assert_eq!(ab.bits(), a.bits() | b.bits());
            for c in &masks {
                assert_eq!(union_colors(ab, *c), union_colors(*a, union_colors(*b, *c)));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Case 12: color identity (CR 903.4, 903.4b-e)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 903.4: the colors of the mana-cost symbols are in the identity.
#[test]
fn identity_903_4_cost_symbols() {
    let c = cost("{2}{W/U}{B/P}");
    assert_eq!(color_identity(&[face(Some(&c))], None), mask(W | U | B));
}

/// REQ-OBJ-001, CR 903.4: a mana symbol only in the rules text is in the identity.
#[test]
fn identity_903_4_rules_text_symbol() {
    let c = cost("{1}{G}");
    let text = [ManaSymbol::Colored(Color::Red)];
    let f = IdentityFace {
        rules_text_symbols: &text,
        ..face(Some(&c))
    };
    assert_eq!(color_identity(&[f], None), mask(G | R));
}

/// REQ-OBJ-001, CR 903.4, CR 107.4f: `{W/U/P}` in the rules text adds white and blue.
#[test]
fn identity_903_4_hybrid_phyrexian_in_text() {
    let text = [ManaSymbol::HybridPhyrexian(Color::White, Color::Blue)];
    let f = IdentityFace {
        rules_text_symbols: &text,
        ..face(None)
    };
    assert_eq!(color_identity(&[f], None), mask(W | U));
}

/// REQ-OBJ-001, CR 903.4: colorless and generic symbols in the text add no color.
#[test]
fn identity_903_4_colorless_text_symbols_add_nothing() {
    let text = [
        ManaSymbol::Colorless,
        ManaSymbol::Snow,
        ManaSymbol::Generic(3),
        ManaSymbol::Variable(Variable::X),
    ];
    let f = IdentityFace {
        rules_text_symbols: &text,
        ..face(None)
    };
    assert_eq!(color_identity(&[f], None), ColorBitmask::NONE);
}

/// REQ-OBJ-001, CR 903.4: a color indicator alone gives its colors.
#[test]
fn identity_903_4_indicator_only_face() {
    let f = IdentityFace {
        color_indicator: Some(mask(B | R)),
        ..face(None)
    };
    assert_eq!(color_identity(&[f], None), mask(B | R));
}

/// REQ-OBJ-001, CR 903.4: colors defined by a characteristic-defining ability are included.
#[test]
fn identity_903_4_cda_only_face() {
    let f = IdentityFace {
        cda_colors: mask(31),
        ..face(None)
    };
    assert_eq!(color_identity(&[f], None), mask(31));
}

/// REQ-OBJ-001, CR 903.4d: the back face of a double-faced card is included.
#[test]
fn identity_903_4d_back_face_included() {
    let front_cost = cost("{1}{G}");
    let front = face(Some(&front_cost));
    let back = IdentityFace {
        color_indicator: Some(mask(B)),
        ..face(None)
    };
    assert_eq!(color_identity(&[front], None), mask(G));
    assert_eq!(color_identity(&[front, back], None), mask(G | B));
}

/// REQ-OBJ-001, CR 903.4e: alternative characteristics are included.
#[test]
fn identity_903_4e_alternative_characteristics_included() {
    let main_cost = cost("{3}{W}");
    let alt_cost = cost("{U}{U}");
    let main = face(Some(&main_cost));
    let alternative = face(Some(&alt_cost));
    assert_eq!(color_identity(&[main, alternative], None), mask(W | U));
}

/// REQ-OBJ-001, CR 903.4b: a color chosen before the game by a commander's static ability is
/// part of its identity.
#[test]
fn identity_903_4b_chosen_color_included() {
    let c = cost("{4}");
    assert_eq!(color_identity(&[face(Some(&c))], Some(mask(R))), mask(R));
    let g = cost("{G}");
    assert_eq!(
        color_identity(&[face(Some(&g))], Some(mask(U))),
        mask(G | U)
    );
    assert_eq!(color_identity(&[], Some(mask(W))), mask(W));
}

/// REQ-OBJ-001, CR 903.4: no faces give an empty identity.
#[test]
fn identity_903_4_empty_list_is_colorless() {
    assert_eq!(color_identity(&[], None), ColorBitmask::NONE);
}

/// REQ-OBJ-001, CR 903.4: a face with no cost, indicator, CDA or text symbols is colorless.
#[test]
fn identity_903_4_no_cost_face_is_colorless() {
    assert_eq!(color_identity(&[face(None)], None), ColorBitmask::NONE);
}

/// REQ-OBJ-001, CR 903.4: identity is the union over faces, independent of face order.
#[test]
fn identity_903_4_union_is_order_independent() {
    let a_cost = cost("{W}");
    let b_cost = cost("{2/B}");
    let text = [ManaSymbol::Hybrid(Color::Red, Color::Green)];
    let a = face(Some(&a_cost));
    let b = face(Some(&b_cost));
    let c = IdentityFace {
        rules_text_symbols: &text,
        color_indicator: Some(mask(U)),
        ..face(None)
    };
    let expected = mask(31);
    let orders: [[IdentityFace<'_>; 3]; 6] = [
        [a, b, c],
        [a, c, b],
        [b, a, c],
        [b, c, a],
        [c, a, b],
        [c, b, a],
    ];
    for order in &orders {
        assert_eq!(color_identity(order, None), expected);
    }
    // And it equals the union of the single-face identities.
    let union = union_colors(
        color_identity(&[a], None),
        union_colors(color_identity(&[b], None), color_identity(&[c], None)),
    );
    assert_eq!(union, expected);
}
