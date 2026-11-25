// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for colorless-hybrid symbols `{C/W}`..`{C/G}` in `fineprint_core::objects`:
//! mana value, colors and color identity (REQ-OBJ-001). Black-box: from the brief only.

use fineprint_core::objects::{
    IdentityFace, ManaValueContext, ManaValueSubject, VariableValues, color_identity, color_mask,
    colors_of_mana_cost, mana_value, symbol_colors, symbol_mana_value,
};
use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};

const COLORS: [Color; 5] = [
    Color::White,
    Color::Blue,
    Color::Black,
    Color::Red,
    Color::Green,
];

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

fn mv(text: &str) -> i64 {
    let c = cost(text);
    mana_value(
        &ManaValueSubject::Plain {
            mana_cost: Some(&c),
        },
        ManaValueContext::OffStack,
    )
}

fn colors_of(text: &str) -> ColorBitmask {
    let c = cost(text);
    colors_of_mana_cost(Some(&c))
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits in range")
}

fn identity_of(text: &str) -> ColorBitmask {
    let c = cost(text);
    let face = IdentityFace {
        mana_cost: Some(&c),
        color_indicator: None,
        cda_colors: ColorBitmask::NONE,
        rules_text_symbols: &[],
    };
    color_identity(&[face], None)
}

/// REQ-OBJ-001, CR 202.3f: `{C/x}` has mana value 1 (largest component), directly and in costs.
#[test]
fn req_obj_001_cr_202_3f_colorless_hybrid_mana_value() {
    for c in COLORS {
        for ctx in [
            ManaValueContext::OffStack,
            ManaValueContext::OnStack(VariableValues::default()),
        ] {
            assert_eq!(symbol_mana_value(ManaSymbol::ColorlessHybrid(c), ctx), 1);
        }
    }
    for l in ["W", "U", "B", "R", "G"] {
        assert_eq!(mv(&format!("{{C/{l}}}")), 1, "{l}");
    }
    assert_eq!(mv("{C/W}{1}"), 2);
    assert_eq!(mv("{C/W}{C/U}"), 2);
    assert_eq!(mv("{C/W}{C/W}{C/W}"), 3);
    assert_eq!(mv("{2/W}{C/W}"), 3);
    assert_eq!(mv("{2/B}{2/B}{2/B}"), 6);
    assert_eq!(mv("{X}{C/R}"), 1);
}

/// REQ-OBJ-001, CR 202.2, 202.2d, 107.4e: `{C/x}` is exactly color x, never colorless.
#[test]
fn req_obj_001_cr_202_2d_colorless_hybrid_colors() {
    for c in COLORS {
        assert_eq!(symbol_colors(ManaSymbol::ColorlessHybrid(c)), color_mask(c));
        assert_ne!(
            symbol_colors(ManaSymbol::ColorlessHybrid(c)),
            ColorBitmask::NONE
        );
    }
    assert_eq!(colors_of("{C/W}"), mask(1));
    assert_eq!(colors_of("{C/W}{C/U}"), mask(1 | 2));
    assert_eq!(colors_of("{C/W}{1}"), mask(1));
}

/// REQ-OBJ-001, CR 202.2b, 107.4c: `{C}` and `{1}` stay colorless, unlike `{C/x}`.
#[test]
fn req_obj_001_cr_202_2b_colorless_symbols_not_confused() {
    assert_eq!(colors_of("{C}"), ColorBitmask::NONE);
    assert_eq!(colors_of("{1}"), ColorBitmask::NONE);
    assert_ne!(colors_of("{C/G}"), colors_of("{C}"));
}

/// REQ-OBJ-001, CR 903.4: `{C/x}` contributes exactly x to color identity.
#[test]
fn req_obj_001_cr_903_4_colorless_hybrid_color_identity() {
    // Synthetic card `test.colorless_hybrid_a`.
    assert_eq!(identity_of("{C/G}{1}"), mask(16));
    assert_eq!(identity_of("{C/W}{C/U}"), mask(1 | 2));
    // Synthetic card `test.colorless_hybrid_b`.
    for (i, c) in COLORS.iter().enumerate() {
        let l = ["W", "U", "B", "R", "G"][i];
        assert_eq!(identity_of(&format!("{{C/{l}}}")), color_mask(*c));
    }
}
