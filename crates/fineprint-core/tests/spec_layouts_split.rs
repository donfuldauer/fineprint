// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the SPLIT layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! Combined characteristics in every zone but the stack (CR 709.4, 709.4a-c), one half or both
//! (fused) on the stack (CR 709.3b, 709.4d, 702.102b), and split mana value (CR 202.3d,
//! 709.4b) including `{X}` off and on the stack (CR 202.3e).

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{
    Layout, LayoutError, SplitLayout, base_characteristics, layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{ColorBitmask, FaceKind, LayoutBehavior, TextTokenStream};
use support::{
    Cast, St, ability, assert_chars, chars, contexts, cost, def, mask, non_stack_zones, on_stack,
    printed, query, sample, set, some_cost, split_left, split_right, text,
};

fn split_def() -> fineprint_dsl::card::CardDefinitionDSL {
    sample(LayoutBehavior::Split)
}

/// The literal combined characteristics of the sample split card.
fn expected_combined() -> Characteristics {
    let mut tokens = text(&["test.split_left"]).0;
    tokens.extend(text(&["test.split_right"]).0);
    Characteristics {
        names: set(&["test.split_left", "test.split_right"]),
        mana_cost: some_cost("{X}{R}{2}{U}{U}"),
        color_indicator: None,
        colors: mask(ColorBitmask::RED.bits() | ColorBitmask::BLUE.bits()),
        supertypes: set(&[]),
        card_types: set(&["Instant", "Sorcery"]),
        subtypes: set(&["Arcane"]),
        rules_text: TextTokenStream(tokens),
        abilities: vec![
            ability("test.split_left", 1),
            ability("test.split_right", 1),
        ],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn stack(cast: Cast) -> fineprint_core::layouts::LayoutQuery {
    query(ZoneKind::Stack, cast, St::DEFAULT)
}

/// REQ-OBJ-002, CR 709.4, 709.4a, 709.4b, 709.4c, 202.3d: in every zone but the stack
/// (battlefield included) a split card has both halves combined and the combined mana value with
/// X counted 0, whatever cast choice, status or context is passed.
#[test]
fn split_709_4_combined_in_every_non_stack_zone_cast_ignored() {
    let d = split_def();
    let v = support::vocab();
    let zones = non_stack_zones();
    assert_eq!(zones.len(), 6);
    assert!(zones.contains(&ZoneKind::Battlefield));
    for zone in zones {
        for cast in Cast::ALL {
            for st in St::all() {
                let q = query(zone, cast, st);
                let got = base_characteristics(&d, &q, v).expect("combined chars");
                assert_chars(
                    &got,
                    &expected_combined(),
                    &format!("{zone:?} {cast:?} {st:?}"),
                );
                // CR 202.3d, 202.3e: {X}{R} (X = 0) + {2}{U}{U} = 1 + 4.
                for ctx in [ManaValueContext::OffStack, on_stack(7, 7, 7)] {
                    assert_eq!(
                        mana_value(&d, &q, ctx, v),
                        Ok(5),
                        "{zone:?} {cast:?} {st:?} {ctx:?}"
                    );
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 709.3b, 202.3d: the left half cast: only its characteristics, its mana value
/// with the announced X.
#[test]
fn split_709_3b_left_half_on_stack() {
    let d = split_def();
    let v = support::vocab();
    for st in St::all() {
        let q = query(ZoneKind::Stack, Cast::Left, st);
        let got = base_characteristics(&d, &q, v).expect("left half");
        let expected = Characteristics {
            names: set(&["test.split_left"]),
            mana_cost: some_cost("{X}{R}"),
            color_indicator: None,
            colors: ColorBitmask::RED,
            supertypes: set(&[]),
            card_types: set(&["Instant"]),
            subtypes: set(&[]),
            rules_text: text(&["test.split_left"]),
            abilities: vec![ability("test.split_left", 1)],
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        };
        assert_chars(&got, &expected, &format!("{st:?}"));
        assert_eq!(got, chars(&split_left()));
        assert_eq!(mana_value(&d, &q, on_stack(3, 0, 0), v), Ok(4));
        assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(1));
    }
}

/// REQ-OBJ-002, CR 709.3b, 202.3d: the right half cast: only its characteristics and mana value.
#[test]
fn split_709_3b_right_half_on_stack() {
    let d = split_def();
    let v = support::vocab();
    let q = stack(Cast::Right);
    let got = base_characteristics(&d, &q, v).expect("right half");
    let expected = Characteristics {
        names: set(&["test.split_right"]),
        mana_cost: some_cost("{2}{U}{U}"),
        color_indicator: None,
        colors: ColorBitmask::BLUE,
        supertypes: set(&[]),
        card_types: set(&["Sorcery"]),
        subtypes: set(&["Arcane"]),
        rules_text: text(&["test.split_right"]),
        abilities: vec![ability("test.split_right", 1)],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    };
    assert_chars(&got, &expected, "right");
    assert_eq!(got, chars(&split_right()));
    assert_eq!(mana_value(&d, &q, on_stack(3, 0, 0), v), Ok(4));
    assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(4));
}

/// REQ-OBJ-002, CR 709.4d, 702.102b, 202.3d: a fused split spell has the combined
/// characteristics and the combined mana value, X counting the announced value.
#[test]
fn split_709_4d_fused_on_stack_combined() {
    let d = split_def();
    let v = support::vocab();
    for st in St::all() {
        let q = query(ZoneKind::Stack, Cast::Fused, st);
        let got = base_characteristics(&d, &q, v).expect("fused");
        assert_chars(&got, &expected_combined(), &format!("{st:?}"));
        // (3 + 1) + 4.
        assert_eq!(mana_value(&d, &q, on_stack(3, 0, 0), v), Ok(8));
        assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(5));
    }
}

/// REQ-OBJ-002, CR 202.3d, 202.3e: X counts per occurrence on the stack and 0 off it, even
/// when an on-stack context is passed for an off-stack split card.
#[test]
fn split_202_3e_x_off_and_on_stack_per_occurrence() {
    let v = support::vocab();
    let d = def(
        LayoutBehavior::Split,
        vec![
            support::spell(FaceKind::LeftHalf, "test.xx_left", "{X}{X}{R}", "Instant"),
            support::spell(FaceKind::RightHalf, "test.xy_right", "{Y}{1}{G}", "Sorcery"),
        ],
    );
    let ctx = on_stack(3, 5, 0);
    // Left: 3 + 3 + 1.
    assert_eq!(mana_value(&d, &stack(Cast::Left), ctx, v), Ok(7));
    // Right: 5 + 1 + 1.
    assert_eq!(mana_value(&d, &stack(Cast::Right), ctx, v), Ok(7));
    // Fused: 7 + 7.
    assert_eq!(mana_value(&d, &stack(Cast::Fused), ctx, v), Ok(14));
    assert_eq!(
        mana_value(&d, &stack(Cast::Fused), ManaValueContext::OffStack, v),
        Ok(3)
    );
    // Off the stack: 1 + 2, with any ctx.
    for zone in non_stack_zones() {
        for c in contexts().into_iter().chain([ctx]) {
            let q = query(zone, Cast::None, St::DEFAULT);
            assert_eq!(mana_value(&d, &q, c, v), Ok(3), "{zone:?} {c:?}");
        }
    }
}

/// REQ-OBJ-002, CR 709.3b, 709.4: on the stack a split spell must be one half or fused; any
/// other cast choice is `InvalidCast { Split, Stack }`, for characteristics and mana value.
#[test]
fn split_709_3b_other_casts_on_stack_are_invalid() {
    let d = split_def();
    let v = support::vocab();
    let bad = Cast::all_except(&[Cast::Left, Cast::Right, Cast::Fused]);
    assert_eq!(bad.len(), 7);
    assert!(bad.contains(&Cast::None));
    let err = LayoutError::InvalidCast {
        layout: LayoutBehavior::Split,
        zone: ZoneKind::Stack,
    };
    for cast in bad {
        for st in St::all() {
            let q = query(ZoneKind::Stack, cast, st);
            assert_eq!(
                base_characteristics(&d, &q, v),
                Err(err.clone()),
                "{cast:?} {st:?}"
            );
            for ctx in contexts() {
                assert_eq!(mana_value(&d, &q, ctx, v), Err(err.clone()), "{cast:?}");
            }
        }
    }
    // The default query (cast None) on the stack is invalid too.
    let q = fineprint_core::layouts::LayoutQuery::new(ZoneKind::Stack);
    assert_eq!(base_characteristics(&d, &q, v), Err(err));
}

/// REQ-OBJ-002, CR 709.4b, 202.3a: the combined cost is `None` when neither half has a cost,
/// and the one half's symbols when only one half has a cost.
#[test]
fn split_709_4b_combined_cost_neither_or_one_half() {
    let v = support::vocab();
    let hand = query(ZoneKind::Hand, Cast::None, St::DEFAULT);
    let neither = def(
        LayoutBehavior::Split,
        vec![
            support::spell(FaceKind::LeftHalf, "test.free_left", "", "Instant"),
            support::spell(FaceKind::RightHalf, "test.free_right", "", "Instant"),
        ],
    );
    let got = base_characteristics(&neither, &hand, v).expect("neither");
    assert_eq!(got.mana_cost, None);
    assert_eq!(got.colors, ColorBitmask::NONE);
    assert_eq!(got.card_types, set(&["Instant"]));
    assert_eq!(
        mana_value(&neither, &hand, ManaValueContext::OffStack, v),
        Ok(0)
    );

    let right_only = def(
        LayoutBehavior::Split,
        vec![
            support::spell(FaceKind::LeftHalf, "test.free_left", "", "Instant"),
            support::spell(FaceKind::RightHalf, "test.paid_right", "{1}{U}", "Instant"),
        ],
    );
    let got = base_characteristics(&right_only, &hand, v).expect("right only");
    assert_eq!(got.mana_cost, Some(cost("{1}{U}")));
    assert_eq!(got.colors, ColorBitmask::BLUE);
    assert_eq!(
        mana_value(&right_only, &hand, ManaValueContext::OffStack, v),
        Ok(2)
    );

    let left_only = def(
        LayoutBehavior::Split,
        vec![
            support::spell(FaceKind::LeftHalf, "test.paid_left", "{B}", "Sorcery"),
            support::spell(FaceKind::RightHalf, "test.free_right", "", "Instant"),
        ],
    );
    let got = base_characteristics(&left_only, &hand, v).expect("left only");
    assert_eq!(got.mana_cost, Some(cost("{B}")));
    assert_eq!(got.colors, ColorBitmask::BLACK);
    assert_eq!(got.card_types, set(&["Instant", "Sorcery"]));
    assert_eq!(
        mana_value(&left_only, &hand, ManaValueContext::OffStack, v),
        Ok(1)
    );
    // On the stack the free half has no mana cost (CR 202.3a).
    let q = stack(Cast::Right);
    let got = base_characteristics(&left_only, &q, v).expect("free right half");
    assert_eq!(got.mana_cost, None);
    assert_eq!(mana_value(&left_only, &q, on_stack(4, 0, 0), v), Ok(0));
}

/// REQ-OBJ-002, CR 709.4a: two halves with the same name give one name.
#[test]
fn split_709_4a_equal_half_names_give_one_name() {
    let v = support::vocab();
    let d = def(
        LayoutBehavior::Split,
        vec![
            support::spell(FaceKind::LeftHalf, "test.twin_half", "{R}", "Instant"),
            support::spell(FaceKind::RightHalf, "test.twin_half", "{G}", "Instant"),
        ],
    );
    for zone in non_stack_zones() {
        let got = base_characteristics(&d, &query(zone, Cast::None, St::DEFAULT), v)
            .expect("equal names");
        assert_eq!(got.names, set(&["test.twin_half"]), "{zone:?}");
        assert_eq!(got.names.len(), 1);
    }
    let fused = base_characteristics(&d, &stack(Cast::Fused), v).expect("fused");
    assert_eq!(fused.names, set(&["test.twin_half"]));
}

/// REQ-OBJ-002, CR 709.4, 709.4b, 202.2e: the combined color indicator is the union of the
/// halves' indicators (`None` only when both are `None`), and the colors are the union of
/// both halves' cost and indicator colors.
#[test]
fn split_709_4_color_indicator_and_colors_union() {
    let v = support::vocab();
    let hand = query(ZoneKind::Hand, Cast::None, St::DEFAULT);
    let half = |kind, n: &str, c: &str, ind: Option<ColorBitmask>| {
        let mut f = support::spell(kind, n, c, "Instant");
        f.color_indicator = ind;
        f
    };
    // One indicator.
    let d = def(
        LayoutBehavior::Split,
        vec![
            half(
                FaceKind::LeftHalf,
                "test.ind_l",
                "",
                Some(ColorBitmask::WHITE),
            ),
            half(FaceKind::RightHalf, "test.ind_r", "{B}", None),
        ],
    );
    let got = base_characteristics(&d, &hand, v).expect("one indicator");
    assert_eq!(got.color_indicator, Some(ColorBitmask::WHITE));
    assert_eq!(
        got.colors,
        mask(ColorBitmask::WHITE.bits() | ColorBitmask::BLACK.bits())
    );
    // Two indicators.
    let d = def(
        LayoutBehavior::Split,
        vec![
            half(
                FaceKind::LeftHalf,
                "test.ind_l",
                "{U}",
                Some(ColorBitmask::WHITE),
            ),
            half(
                FaceKind::RightHalf,
                "test.ind_r",
                "",
                Some(ColorBitmask::GREEN),
            ),
        ],
    );
    let got = base_characteristics(&d, &hand, v).expect("two indicators");
    let wg = mask(ColorBitmask::WHITE.bits() | ColorBitmask::GREEN.bits());
    assert_eq!(got.color_indicator, Some(wg));
    assert_eq!(got.colors, mask(wg.bits() | ColorBitmask::BLUE.bits()));
    // Fused on the stack: the same union.
    let fused = base_characteristics(&d, &stack(Cast::Fused), v).expect("fused");
    assert_eq!(fused.color_indicator, Some(wg));
    assert_eq!(fused.colors, got.colors);
    // A single half keeps only its own indicator.
    let right = base_characteristics(&d, &stack(Cast::Right), v).expect("right");
    assert_eq!(right.color_indicator, Some(ColorBitmask::GREEN));
    assert_eq!(right.colors, ColorBitmask::GREEN);
}

/// REQ-OBJ-002, CR 709.4, 709.4c: combined characteristics carry the union of supertypes and
/// subtypes, left text and abilities before right ones, and no power, toughness, loyalty,
/// defense or modifiers even when a half prints them.
#[test]
fn split_709_4c_unions_order_and_no_numbers() {
    let v = support::vocab();
    let mut left = support::spell(FaceKind::LeftHalf, "test.num_l", "{1}", "Instant");
    left.supertypes = support::strings(&["Legendary"]);
    left.subtypes = support::strings(&["Arcane"]);
    left.text = text(&["l1", "l2"]);
    left.abilities = vec![ability("l", 1), ability("l", 2)];
    left.power = Some(printed(1));
    left.toughness = Some(printed(1));
    left.loyalty = Some(3);
    let mut right = support::spell(FaceKind::RightHalf, "test.num_r", "{2}", "Sorcery");
    right.text = text(&["r1"]);
    right.abilities = vec![ability("r", 1)];
    right.defense = Some(2);
    right.hand_modifier = Some(1);
    right.life_modifier = Some(-2);
    let d = def(LayoutBehavior::Split, vec![left, right]);
    for q in [
        query(ZoneKind::Graveyard, Cast::None, St::DEFAULT),
        stack(Cast::Fused),
    ] {
        let got = base_characteristics(&d, &q, v).expect("combined");
        assert_eq!(got.supertypes, set(&["Legendary"]));
        assert_eq!(got.card_types, set(&["Instant", "Sorcery"]));
        assert_eq!(got.subtypes, set(&["Arcane"]));
        assert_eq!(got.rules_text, text(&["l1", "l2", "r1"]));
        assert_eq!(
            got.abilities,
            vec![ability("l", 1), ability("l", 2), ability("r", 1)]
        );
        assert_eq!(got.mana_cost, Some(cost("{1}{2}")));
        assert_eq!(got.power, None);
        assert_eq!(got.toughness, None);
        assert_eq!(got.loyalty, None);
        assert_eq!(got.defense, None);
        assert_eq!(got.hand_modifier, None);
        assert_eq!(got.life_modifier, None);
    }
    // A single half on the stack keeps what it prints.
    let left_cast = base_characteristics(&d, &stack(Cast::Left), v).expect("left");
    assert_eq!(left_cast.power, Some(printed(1)));
    assert_eq!(left_cast.loyalty, Some(3));
}

/// REQ-OBJ-002, CR 709.4, 709.3b: halves are found by face kind, so the right half listed first
/// gives the same results; the `SplitLayout` object agrees with the free functions.
#[test]
fn split_709_4_reverse_face_order_and_layout_object() {
    let v = support::vocab();
    let forward = split_def();
    let reverse = def(LayoutBehavior::Split, vec![split_right(), split_left()]);
    let obj = layout_for(LayoutBehavior::Split).expect("split supported");
    assert_eq!(obj.behavior(), LayoutBehavior::Split);
    assert_eq!(SplitLayout.behavior(), LayoutBehavior::Split);
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            let q = query(zone, cast, St::DEFAULT);
            let a = base_characteristics(&forward, &q, v);
            assert_eq!(
                base_characteristics(&reverse, &q, v),
                a,
                "{zone:?} {cast:?}"
            );
            assert_eq!(obj.base_characteristics(&reverse, &q, v), a);
            assert_eq!(SplitLayout.base_characteristics(&forward, &q, v), a);
            for ctx in contexts() {
                let m = mana_value(&forward, &q, ctx, v);
                assert_eq!(mana_value(&reverse, &q, ctx, v), m);
                assert_eq!(obj.mana_value(&reverse, &q, ctx, v), m);
            }
        }
    }
    // Combined text is still left then right.
    let got = base_characteristics(
        &reverse,
        &query(ZoneKind::Exile, Cast::None, St::DEFAULT),
        v,
    )
    .expect("reverse");
    assert_eq!(
        got.rules_text,
        text(&["test.split_left", "test.split_right"])
    );
}
