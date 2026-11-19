// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the ADVENTURER, OMEN and PROTOTYPE layouts of `fineprint_core::layouts`
//! (REQ-OBJ-002). Black-box: written from the work-item brief only.
//!
//! Alternative characteristics only on the stack as an Adventure or Omen (CR 716.3b, 716.4,
//! 721.3b, 721.4); a prototyped spell or permanent has only the alternative mana cost, power and
//! toughness and the colors of that cost (CR 719.3b), everything else unchanged (CR 719.4,
//! 719.5); mana value from the selected cost (CR 202.3, 202.3e).

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{
    AdventurerLayout, Layout, LayoutError, OmenLayout, PrototypeLayout, base_characteristics,
    layout_for, mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask, LayoutBehavior};
use support::{
    Cast, St, ability, adventure_face, adventurer_main, assert_chars, chars, contexts, def, mask,
    non_stack_zones, omen_face, omen_main, on_stack, printed, proto_face, proto_main, query,
    sample, set, some_cost, text,
};

// ---------------------------------------------------------------------------------------------
// ADVENTURER and OMEN
// ---------------------------------------------------------------------------------------------

fn expected_adventure() -> Characteristics {
    Characteristics {
        names: set(&["test.adv_spell"]),
        mana_cost: some_cost("{X}{1}{R}"),
        color_indicator: None,
        colors: ColorBitmask::RED,
        supertypes: set(&[]),
        card_types: set(&["Instant"]),
        subtypes: set(&["Adventure"]),
        rules_text: text(&["test.adv_spell"]),
        abilities: vec![ability("test.adv_spell", 1)],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn expected_adventurer_main() -> Characteristics {
    Characteristics {
        names: set(&["test.adv_creature"]),
        mana_cost: some_cost("{3}{G}"),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: set(&[]),
        card_types: set(&["Creature"]),
        subtypes: set(&["Human"]),
        rules_text: text(&["test.adv_creature"]),
        abilities: vec![ability("test.adv_creature", 1)],
        power: Some(printed(4)),
        toughness: Some(printed(3)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn expected_omen() -> Characteristics {
    Characteristics {
        names: set(&["test.omen_spell"]),
        mana_cost: some_cost("{X}{B}"),
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: set(&[]),
        card_types: set(&["Sorcery"]),
        subtypes: set(&["Omen"]),
        rules_text: text(&["test.omen_spell"]),
        abilities: vec![ability("test.omen_spell", 1)],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// One alternative-face layout under test.
struct AltCase {
    def: CardDefinitionDSL,
    reverse: CardDefinitionDSL,
    alt_cast: Cast,
    main: Characteristics,
    alt: Characteristics,
    main_mv: i64,
    /// Mana value of the alternative face with X = 3 on the stack.
    alt_mv_x3: i64,
    /// Mana value of the alternative face off the stack context (X = 0).
    alt_mv_x0: i64,
}

fn alt_cases() -> Vec<AltCase> {
    vec![
        AltCase {
            def: sample(LayoutBehavior::Adventurer),
            reverse: def(
                LayoutBehavior::Adventurer,
                vec![adventure_face(), adventurer_main()],
            ),
            alt_cast: Cast::Adventure,
            main: expected_adventurer_main(),
            alt: expected_adventure(),
            main_mv: 4,
            alt_mv_x3: 5,
            alt_mv_x0: 2,
        },
        AltCase {
            def: sample(LayoutBehavior::Omen),
            reverse: def(LayoutBehavior::Omen, vec![omen_face(), omen_main()]),
            alt_cast: Cast::Omen,
            main: chars(&omen_main()),
            alt: expected_omen(),
            main_mv: 5,
            alt_mv_x3: 4,
            alt_mv_x0: 1,
        },
    ]
}

/// REQ-OBJ-002, CR 716.3b, 721.3b, 202.3e: on the stack as an Adventure (Omen) the spell has
/// only the alternative face's characteristics (name, cost, colors, types, text, abilities, no
/// P/T) and its mana value counts X.
#[test]
fn alt_716_3b_721_3b_stack_as_alternative_has_only_alternative() {
    let v = support::vocab();
    for case in alt_cases() {
        assert_eq!(case.main.names.len(), 1);
        for s in St::all() {
            let q = query(ZoneKind::Stack, case.alt_cast, s);
            let got = base_characteristics(&case.def, &q, v).expect("alternative");
            assert_chars(&got, &case.alt, &format!("{:?} {s:?}", case.def.layout));
            assert_eq!(got.power, None);
            assert_eq!(got.toughness, None);
            assert_eq!(
                mana_value(&case.def, &q, on_stack(3, 0, 0), v),
                Ok(case.alt_mv_x3)
            );
            assert_eq!(
                mana_value(&case.def, &q, ManaValueContext::OffStack, v),
                Ok(case.alt_mv_x0)
            );
        }
    }
}

/// REQ-OBJ-002, CR 716.4, 721.4: on the stack not as an Adventure (Omen), cast None, the card
/// has only its normal characteristics and mana value.
#[test]
fn alt_716_4_721_4_stack_cast_normally_has_main() {
    let v = support::vocab();
    for case in alt_cases() {
        let q = query(ZoneKind::Stack, Cast::None, St::DEFAULT);
        let got = base_characteristics(&case.def, &q, v).expect("main");
        assert_chars(&got, &case.main, &format!("{:?}", case.def.layout));
        assert_eq!(
            mana_value(&case.def, &q, on_stack(3, 0, 0), v),
            Ok(case.main_mv)
        );
    }
}

/// REQ-OBJ-002, CR 716.3b, 716.4, 721.3b, 721.4: on the stack every cast choice other than None
/// and the layout's own alternative is `InvalidCast` (an Adventurer cast as an Omen and the
/// reverse included).
#[test]
fn alt_716_4_721_4_other_casts_on_stack_invalid() {
    let v = support::vocab();
    for case in alt_cases() {
        let bad = Cast::all_except(&[Cast::None, case.alt_cast]);
        assert_eq!(bad.len(), 8);
        let err = LayoutError::InvalidCast {
            layout: case.def.layout,
            zone: ZoneKind::Stack,
        };
        for cast in bad {
            let q = query(ZoneKind::Stack, cast, St::DEFAULT);
            assert_eq!(
                base_characteristics(&case.def, &q, v),
                Err(err.clone()),
                "{cast:?}"
            );
            assert_eq!(
                mana_value(&case.def, &q, ManaValueContext::OffStack, v),
                Err(err.clone())
            );
        }
    }
}

/// REQ-OBJ-002, CR 716.4, 721.4: in every zone but the stack the card has only its normal
/// characteristics, whatever cast choice or status is passed.
#[test]
fn alt_716_4_721_4_main_in_every_other_zone_cast_ignored() {
    let v = support::vocab();
    for case in alt_cases() {
        for zone in non_stack_zones() {
            for cast in Cast::ALL {
                for s in St::all() {
                    let q = query(zone, cast, s);
                    let got = base_characteristics(&case.def, &q, v).expect("main");
                    assert_chars(&got, &case.main, &format!("{zone:?} {cast:?} {s:?}"));
                    for ctx in contexts() {
                        assert_eq!(mana_value(&case.def, &q, ctx, v), Ok(case.main_mv));
                    }
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 716.3b, 721.3b: faces are found by kind (alternative face listed first), and
/// the layout objects agree with the free functions.
#[test]
fn alt_716_3b_reverse_face_order_and_layout_objects() {
    let v = support::vocab();
    let objs: [&dyn Layout; 2] = [&AdventurerLayout, &OmenLayout];
    for (case, obj) in alt_cases().into_iter().zip(objs) {
        assert_eq!(obj.behavior(), case.def.layout);
        let via = layout_for(case.def.layout).expect("supported");
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                let q = query(zone, cast, St::DEFAULT);
                let a = base_characteristics(&case.def, &q, v);
                assert_eq!(base_characteristics(&case.reverse, &q, v), a);
                assert_eq!(obj.base_characteristics(&case.reverse, &q, v), a);
                assert_eq!(via.base_characteristics(&case.def, &q, v), a);
                let m = mana_value(&case.def, &q, on_stack(3, 0, 0), v);
                assert_eq!(obj.mana_value(&case.reverse, &q, on_stack(3, 0, 0), v), m);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// PROTOTYPE
// ---------------------------------------------------------------------------------------------

fn expected_proto_main() -> Characteristics {
    Characteristics {
        names: set(&["test.proto_full"]),
        mana_cost: some_cost("{7}"),
        color_indicator: Some(ColorBitmask::BLUE),
        colors: ColorBitmask::BLUE,
        supertypes: set(&[]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&[]),
        rules_text: text(&["test.proto_full"]),
        abilities: vec![ability("test.proto_full", 1)],
        power: Some(printed(6)),
        toughness: Some(printed(6)),
        loyalty: Some(2),
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// Main's characteristics except the prototype cost and P/T, colors from that cost plus Main's
/// indicator; name, types, text, abilities and loyalty stay Main's.
fn expected_prototyped() -> Characteristics {
    Characteristics {
        mana_cost: some_cost("{X}{2}{R}{R}"),
        colors: mask(ColorBitmask::BLUE.bits() | ColorBitmask::RED.bits()),
        power: Some(printed(3)),
        toughness: Some(printed(2)),
        ..expected_proto_main()
    }
}

/// REQ-OBJ-002, CR 719.3b, 719.5, 202.3e: a prototyped spell (stack) and the permanent it
/// becomes (battlefield) have the prototype cost, power and toughness and its colors (with
/// Main's indicator); name, types, text, abilities and loyalty are Main's; mana value from the
/// prototype cost, X counted on the stack only (CR 202.3e, 107.3m: X is 0 for the permanent).
#[test]
fn prototype_719_3b_prototyped_on_stack_and_battlefield() {
    let d = sample(LayoutBehavior::Prototype);
    let v = support::vocab();
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        for s in St::all() {
            let q = query(zone, Cast::Prototype, s);
            let got = base_characteristics(&d, &q, v).expect("prototyped");
            assert_chars(&got, &expected_prototyped(), &format!("{zone:?} {s:?}"));
            // The prototype face's own name, types, text and abilities never appear.
            assert!(!got.names.contains("test.proto_inset"));
            assert_eq!(got.card_types, set(&["Artifact", "Creature"]));
            // X + 2 + 1 + 1, X counted on the stack only (CR 202.3e, 107.3m).
            let with_x = if zone == ZoneKind::Stack { 7 } else { 4 };
            assert_eq!(mana_value(&d, &q, on_stack(3, 0, 0), v), Ok(with_x));
            assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(4));
        }
    }
}

/// REQ-OBJ-002, CR 719.3b, 105.2: the prototyped colors come from the prototype cost; a Main
/// face without an indicator and with a different-colored cost loses its own cost's color.
#[test]
fn prototype_719_3b_colors_from_prototype_cost() {
    let v = support::vocab();
    let mut main = proto_main();
    main.color_indicator = None;
    main.mana_cost = some_cost("{5}{G}");
    let d = def(LayoutBehavior::Prototype, vec![main, proto_face()]);
    let q = query(ZoneKind::Battlefield, Cast::Prototype, St::DEFAULT);
    let got = base_characteristics(&d, &q, v).expect("prototyped");
    assert_eq!(got.colors, ColorBitmask::RED);
    assert_eq!(got.color_indicator, None);
    let normal = base_characteristics(
        &d,
        &query(ZoneKind::Battlefield, Cast::None, St::DEFAULT),
        v,
    )
    .expect("normal");
    assert_eq!(normal.colors, ColorBitmask::GREEN);
    assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(4));
}

/// REQ-OBJ-002, CR 719.4: on the stack or battlefield when not cast prototyped (cast None) a
/// prototype card has only its normal characteristics.
#[test]
fn prototype_719_4_cast_normally_has_main() {
    let d = sample(LayoutBehavior::Prototype);
    let v = support::vocab();
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        let q = query(zone, Cast::None, St::DEFAULT);
        let got = base_characteristics(&d, &q, v).expect("normal");
        assert_chars(&got, &expected_proto_main(), &format!("{zone:?}"));
        assert_eq!(got, chars(&proto_main()));
        assert_eq!(mana_value(&d, &q, on_stack(3, 0, 0), v), Ok(7));
    }
}

/// REQ-OBJ-002, CR 719.3b, 719.4: on the stack or battlefield every cast choice other than None
/// and Prototype is `InvalidCast`.
#[test]
fn prototype_719_4_other_casts_on_stack_and_battlefield_invalid() {
    let d = sample(LayoutBehavior::Prototype);
    let v = support::vocab();
    let bad = Cast::all_except(&[Cast::None, Cast::Prototype]);
    assert_eq!(bad.len(), 8);
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        let err = LayoutError::InvalidCast {
            layout: LayoutBehavior::Prototype,
            zone,
        };
        for cast in bad.iter().copied() {
            let q = query(zone, cast, St::DEFAULT);
            assert_eq!(
                base_characteristics(&d, &q, v),
                Err(err.clone()),
                "{zone:?} {cast:?}"
            );
            assert_eq!(
                mana_value(&d, &q, ManaValueContext::OffStack, v),
                Err(err.clone())
            );
        }
    }
}

/// REQ-OBJ-002, CR 719.4: in every zone except the stack and battlefield a prototype card has
/// only its normal characteristics, whatever cast choice is passed (Prototype included).
#[test]
fn prototype_719_4_main_in_other_zones_cast_ignored() {
    let d = sample(LayoutBehavior::Prototype);
    let v = support::vocab();
    for zone in support::hidden_or_other_zones() {
        for cast in Cast::ALL {
            for s in St::all() {
                let q = query(zone, cast, s);
                let got = base_characteristics(&d, &q, v).expect("normal");
                assert_chars(&got, &expected_proto_main(), &format!("{zone:?} {cast:?}"));
                assert_eq!(mana_value(&d, &q, ManaValueContext::OffStack, v), Ok(7));
            }
        }
    }
}

/// REQ-OBJ-002, CR 719.3b: the prototype face listed first works, and `PrototypeLayout` agrees
/// with the free functions.
#[test]
fn prototype_719_3b_reverse_face_order_and_layout_object() {
    let v = support::vocab();
    let forward = sample(LayoutBehavior::Prototype);
    let reverse = def(LayoutBehavior::Prototype, vec![proto_face(), proto_main()]);
    assert_eq!(PrototypeLayout.behavior(), LayoutBehavior::Prototype);
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            let q = query(zone, cast, St::DEFAULT);
            let a = base_characteristics(&forward, &q, v);
            assert_eq!(base_characteristics(&reverse, &q, v), a);
            assert_eq!(PrototypeLayout.base_characteristics(&reverse, &q, v), a);
            for ctx in contexts() {
                assert_eq!(
                    PrototypeLayout.mana_value(&reverse, &q, ctx, v),
                    mana_value(&forward, &q, ctx, v)
                );
            }
        }
    }
}
