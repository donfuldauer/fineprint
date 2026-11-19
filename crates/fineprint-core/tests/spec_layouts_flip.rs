// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the FLIP layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! Normal characteristics everywhere but a flipped permanent (CR 710.2); a flipped permanent
//! uses the alternative name, text box, type line, power and toughness (CR 710.1b, 710.2) but
//! keeps its color and mana cost (CR 710.1c), so its mana value is unchanged.

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{FlipLayout, Layout, base_characteristics, layout_for, mana_value};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, LayoutBehavior};
use support::{
    Cast, St, ability, assert_chars, chars, contexts, def, flip_flipped, flip_main, on_stack,
    printed, query, sample, set, some_cost, text,
};

fn flip_def() -> CardDefinitionDSL {
    sample(LayoutBehavior::Flip)
}

/// The literal normal characteristics of the sample flip card.
fn expected_top() -> Characteristics {
    Characteristics {
        names: set(&["test.flip_top"]),
        mana_cost: some_cost("{2}{B}"),
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human", "Wizard"]),
        rules_text: text(&["test.flip_top"]),
        abilities: vec![ability("test.flip_top", 1)],
        power: Some(printed(1)),
        toughness: Some(printed(3)),
        loyalty: Some(1),
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The literal flipped characteristics: the Flipped face's name, text box, type line and P/T
/// (CR 710.1b, 710.2), with the top's cost and colors (CR 710.1c) and the top's loyalty, defense
/// and hand and life modifiers, which CR 710.1b and 710.2 do not replace.
fn expected_flipped() -> Characteristics {
    Characteristics {
        names: set(&["test.flip_bottom"]),
        mana_cost: some_cost("{2}{B}"),
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: set(&["Legendary"]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Wizard"]),
        rules_text: text(&["test.flip_bottom"]),
        abilities: vec![ability("test.flip_bottom", 1)],
        power: Some(printed(4)),
        toughness: Some(printed(4)),
        loyalty: Some(1),
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// REQ-OBJ-002, CR 710.2, 202.3: unflipped, in every zone and with every cast choice, a flip
/// card has its normal characteristics and the mana value of its own cost.
#[test]
fn flip_710_2_unflipped_every_zone_normal() {
    let d = flip_def();
    let v = support::vocab();
    assert_eq!(expected_top(), chars(&flip_main()));
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            for back_up in [false, true] {
                for copying_back_face in [false, true] {
                    let st = St {
                        flipped: false,
                        back_up,
                        copying_back_face,
                    };
                    let q = query(zone, cast, st);
                    let got = base_characteristics(&d, &q, v).expect("normal");
                    assert_chars(&got, &expected_top(), &format!("{zone:?} {cast:?} {st:?}"));
                    for ctx in contexts() {
                        assert_eq!(mana_value(&d, &q, ctx, v), Ok(3));
                    }
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 710.1b, 710.2, 710.4: `flipped = true` outside the battlefield is ignored
/// (only a permanent can be flipped).
#[test]
fn flip_710_1b_flipped_status_ignored_off_battlefield() {
    let d = flip_def();
    let v = support::vocab();
    for zone in ZoneKind::ALL
        .into_iter()
        .filter(|z| *z != ZoneKind::Battlefield)
    {
        for cast in Cast::ALL {
            let q = query(zone, cast, St::flipped());
            let got = base_characteristics(&d, &q, v).expect("normal");
            assert_chars(&got, &expected_top(), &format!("{zone:?} {cast:?}"));
            assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(3));
        }
    }
}

/// REQ-OBJ-002, CR 710.1b, 710.1c, 710.2: a flipped permanent has the Flipped face's name, text,
/// abilities, type line and P/T, but the normal mana cost, color indicator and colors, even
/// though the Flipped face declares a different cost and indicator. Loyalty, defense and the
/// hand and life modifiers are not among the characteristics CR 710.1b and 710.2 replace, so
/// they stay the normal face's although the Flipped face declares other values.
#[test]
fn flip_710_1c_flipped_on_battlefield_keeps_cost_and_color() {
    let d = flip_def();
    let v = support::vocab();
    // The Flipped face really does declare other values.
    assert_eq!(flip_flipped().mana_cost, some_cost("{9}{W}"));
    assert_eq!(flip_flipped().color_indicator, Some(ColorBitmask::GREEN));
    assert_eq!(flip_flipped().loyalty, None);
    assert_eq!(flip_flipped().defense, Some(3));
    assert_eq!(flip_flipped().hand_modifier, Some(-1));
    assert_eq!(flip_flipped().life_modifier, Some(4));
    for cast in Cast::ALL {
        for back_up in [false, true] {
            for copying_back_face in [false, true] {
                let st = St {
                    flipped: true,
                    back_up,
                    copying_back_face,
                };
                let q = query(ZoneKind::Battlefield, cast, st);
                let got = base_characteristics(&d, &q, v).expect("flipped");
                assert_chars(&got, &expected_flipped(), &format!("{cast:?} {st:?}"));
            }
        }
    }
}

/// REQ-OBJ-002, CR 710.1c, 202.3, 202.3e, 107.3m: flipping does not change the mana value:
/// always the normal cost's; X counts only on the stack, so it is 0 for the permanent whatever
/// context is passed.
#[test]
fn flip_710_1c_mana_value_unchanged_by_flipping() {
    let v = support::vocab();
    let d = flip_def();
    let flipped = query(ZoneKind::Battlefield, Cast::None, St::flipped());
    let unflipped = query(ZoneKind::Battlefield, Cast::None, St::DEFAULT);
    for ctx in contexts() {
        assert_eq!(mana_value(&d, &flipped, ctx, v), Ok(3));
        assert_eq!(mana_value(&d, &unflipped, ctx, v), Ok(3));
    }
    // A top face with X: the flipped face's cost never counts.
    let mut top = flip_main();
    top.mana_cost = some_cost("{X}{B}");
    let d = def(LayoutBehavior::Flip, vec![top, flip_flipped()]);
    assert_eq!(
        mana_value(&d, &flipped, ManaValueContext::OffStack, v),
        Ok(1)
    );
    assert_eq!(mana_value(&d, &flipped, on_stack(4, 0, 0), v), Ok(1));
    let stack = query(ZoneKind::Stack, Cast::None, St::DEFAULT);
    assert_eq!(mana_value(&d, &stack, on_stack(4, 0, 0), v), Ok(5));
    assert_eq!(
        FlipLayout.mana_value(&d, &stack, on_stack(4, 0, 0), v),
        Ok(5)
    );
    // A top face with no mana cost: 0, and the flipped permanent has no cost either.
    let mut top = flip_main();
    top.mana_cost = None;
    let d = def(LayoutBehavior::Flip, vec![top, flip_flipped()]);
    let got = base_characteristics(&d, &flipped, v).expect("flipped");
    assert_eq!(got.mana_cost, None);
    assert_eq!(got.colors, ColorBitmask::NONE);
    assert_eq!(
        mana_value(&d, &flipped, ManaValueContext::OffStack, v),
        Ok(0)
    );
}

/// REQ-OBJ-002, CR 710.1c: a top face with a color indicator keeps it when flipped.
#[test]
fn flip_710_1c_top_indicator_kept() {
    let v = support::vocab();
    let mut top = flip_main();
    top.mana_cost = None;
    top.color_indicator = Some(ColorBitmask::WHITE);
    let d = def(LayoutBehavior::Flip, vec![top, flip_flipped()]);
    let got = base_characteristics(
        &d,
        &query(ZoneKind::Battlefield, Cast::None, St::flipped()),
        v,
    )
    .expect("flipped");
    assert_eq!(got.color_indicator, Some(ColorBitmask::WHITE));
    assert_eq!(got.colors, ColorBitmask::WHITE);
    assert_eq!(got.names, set(&["test.flip_bottom"]));
}

/// REQ-OBJ-002, CR 710.2: the Flipped face listed first gives the same results, and the
/// `FlipLayout` object agrees with the free functions.
#[test]
fn flip_710_2_reverse_face_order_and_layout_object() {
    let v = support::vocab();
    let forward = flip_def();
    let reverse = def(LayoutBehavior::Flip, vec![flip_flipped(), flip_main()]);
    let obj = layout_for(LayoutBehavior::Flip).expect("flip supported");
    assert_eq!(obj.behavior(), LayoutBehavior::Flip);
    for zone in ZoneKind::ALL {
        for st in St::all() {
            let q = query(zone, Cast::None, st);
            let a = base_characteristics(&forward, &q, v);
            assert!(a.is_ok());
            assert_eq!(base_characteristics(&reverse, &q, v), a);
            assert_eq!(FlipLayout.base_characteristics(&reverse, &q, v), a);
            assert_eq!(obj.base_characteristics(&forward, &q, v), a);
            assert_eq!(
                FlipLayout.mana_value(&reverse, &q, ManaValueContext::OffStack, v),
                Ok(3)
            );
        }
    }
}
