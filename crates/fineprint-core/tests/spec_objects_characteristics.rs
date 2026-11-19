// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::objects::Characteristics`. Black-box: written from the
//! work-item brief only.
//!
//! The one-to-one mapping of a `CardFaceDSL` to `Characteristics` (CR 109.3), derived colors
//! (CR 202.2), the no-cost versus `{0}` distinction (CR 202.3a), mana value through the
//! characteristics, and when printed power and toughness apply (CR 208.3) (REQ-OBJ-001).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_core::objects::{
    CdaColors, Characteristics, ManaValueContext, ManaValueSubject, ObjectError, VariableValues,
    base_colors, mana_value,
};
use fineprint_dsl::card::{
    AbilityAST, CardFaceDSL, ColorBitmask, FaceKind, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("bundled vocabulary loads")
}

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits in 0..=31")
}

/// Round-trips a serialisable value through MessagePack into one of the DSL placeholder types
/// (which wrap a JSON value), so no direct `serde_json` dependency is needed.
fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

fn printed(n: i64) -> ValueExpressionAST {
    via_msgpack(&n)
}

fn text_tokens(tokens: &[&str]) -> TextTokenStream {
    via_msgpack(&tokens.to_vec())
}

fn ability(key: &str, n: i64) -> AbilityAST {
    let mut m = BTreeMap::new();
    m.insert(key.to_string(), n);
    via_msgpack(&m)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A plain synthetic face: no cost, no types, nothing printed.
fn blank_face(face_name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: FaceKind::Main,
        name: face_name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: Vec::new(),
        subtypes: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
    }
}

fn from_face(face: &CardFaceDSL) -> Characteristics {
    Characteristics::from_face(face, vocab()).expect("valid synthetic face")
}

fn on_stack(x: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y: 0, z: 0 })
}

/// A vanilla 2/2 creature face with the given cost.
fn creature_face(face_name: &str, mana_cost: Option<ManaCostDSL>) -> CardFaceDSL {
    CardFaceDSL {
        mana_cost,
        card_types: strings(&["Creature"]),
        power: Some(printed(2)),
        toughness: Some(printed(2)),
        ..blank_face(face_name)
    }
}

// ---------------------------------------------------------------------------------------------
// Case 14: from_face mapping (CR 109.3)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 109.3: every characteristic of a face maps one to one.
#[test]
fn chars_109_3_every_field_maps_one_to_one() {
    let v = vocab();
    let land = v
        .subtypes_of("Land")
        .find(|s| !v.is_subtype_of("Artifact", s))
        .expect("land subtype")
        .to_string();
    let artifact = v
        .subtypes_of("Artifact")
        .find(|s| !v.is_subtype_of("Land", s))
        .expect("artifact subtype")
        .to_string();
    let face = CardFaceDSL {
        face_kind: FaceKind::Main,
        name: "test.everything_3_4".to_string(),
        mana_cost: Some(cost("{2}{W}{U}")),
        color_indicator: Some(mask(4)),
        supertypes: strings(&["Legendary", "Snow", "Legendary"]),
        card_types: strings(&["Artifact", "Land", "Artifact"]),
        subtypes: vec![land.clone(), artifact.clone(), land.clone()],
        power: Some(printed(3)),
        toughness: Some(printed(4)),
        loyalty: Some(5),
        defense: Some(6),
        hand_modifier: Some(-1),
        life_modifier: Some(7),
        text: text_tokens(&["tok.one", "tok.two"]),
        abilities: vec![ability("a", 1), ability("b", 2)],
    };
    let ch = from_face(&face);
    assert_eq!(ch.names, set(&["test.everything_3_4"]));
    assert_eq!(ch.names.len(), 1);
    assert_eq!(ch.mana_cost, face.mana_cost);
    assert_eq!(ch.color_indicator, Some(mask(4)));
    assert_eq!(ch.supertypes, set(&["Legendary", "Snow"]));
    assert_eq!(ch.card_types, set(&["Artifact", "Land"]));
    assert_eq!(ch.subtypes, set(&[land.as_str(), artifact.as_str()]));
    assert_eq!(ch.power, Some(printed(3)));
    assert_eq!(ch.toughness, Some(printed(4)));
    assert_eq!(ch.loyalty, Some(5));
    assert_eq!(ch.defense, Some(6));
    assert_eq!(ch.hand_modifier, Some(-1));
    assert_eq!(ch.life_modifier, Some(7));
    assert_eq!(ch.rules_text, face.text);
    assert_eq!(ch.abilities, face.abilities);
    // CR 202.2: colors from cost {W}{U} plus indicator black.
    assert_eq!(ch.colors, mask(1 | 2 | 4));
    // The whole value, as one struct.
    let expected = Characteristics {
        names: set(&["test.everything_3_4"]),
        mana_cost: Some(cost("{2}{W}{U}")),
        color_indicator: Some(mask(4)),
        colors: mask(7),
        supertypes: set(&["Legendary", "Snow"]),
        card_types: set(&["Artifact", "Land"]),
        subtypes: set(&[land.as_str(), artifact.as_str()]),
        rules_text: text_tokens(&["tok.one", "tok.two"]),
        abilities: vec![ability("a", 1), ability("b", 2)],
        power: Some(printed(3)),
        toughness: Some(printed(4)),
        loyalty: Some(5),
        defense: Some(6),
        hand_modifier: Some(-1),
        life_modifier: Some(7),
    };
    assert_eq!(ch, expected);
}

/// REQ-OBJ-001, CR 109.3: a blank face maps to empty sets and absent values.
#[test]
fn chars_109_3_blank_face_maps_to_empty() {
    let ch = from_face(&blank_face("test.blank"));
    assert_eq!(ch.names, set(&["test.blank"]));
    assert_eq!(ch.mana_cost, None);
    assert_eq!(ch.color_indicator, None);
    assert_eq!(ch.colors, ColorBitmask::NONE);
    assert!(ch.supertypes.is_empty());
    assert!(ch.card_types.is_empty());
    assert!(ch.subtypes.is_empty());
    assert_eq!(ch.power, None);
    assert_eq!(ch.toughness, None);
    assert_eq!(ch.loyalty, None);
    assert_eq!(ch.defense, None);
    assert_eq!(ch.hand_modifier, None);
    assert_eq!(ch.life_modifier, None);
    assert_eq!(ch.rules_text, TextTokenStream(Vec::new()));
    assert!(ch.abilities.is_empty());
}

/// REQ-OBJ-001, CR 202.3a: a no-cost face keeps `None`, a `{0}` face keeps `Some`, and the two
/// characteristics differ even though both have mana value 0.
#[test]
fn chars_202_3a_no_cost_differs_from_zero_cost() {
    let none = from_face(&blank_face("test.costless"));
    let zero = from_face(&CardFaceDSL {
        mana_cost: Some(cost("{0}")),
        ..blank_face("test.costless")
    });
    assert_eq!(none.mana_cost, None);
    assert_eq!(zero.mana_cost, Some(cost("{0}")));
    assert_ne!(none, zero);
    for ctx in [ManaValueContext::OffStack, on_stack(3)] {
        assert_eq!(none.mana_value(ctx), 0);
        assert_eq!(zero.mana_value(ctx), 0);
    }
}

/// REQ-OBJ-001, CR 202.2: colors are `base_colors(cost, indicator, CdaColors::None)` for a cost-only face.
#[test]
fn chars_202_2_colors_cost_only_face() {
    let c = cost("{1}{B}{G/P}");
    let face = CardFaceDSL {
        mana_cost: Some(c.clone()),
        ..blank_face("test.cost_only")
    };
    let ch = from_face(&face);
    assert_eq!(ch.colors, base_colors(Some(&c), None, CdaColors::None));
    assert_eq!(ch.colors, mask(4 | 16));
}

/// REQ-OBJ-001, CR 202.2e: colors for an indicator-only face.
#[test]
fn chars_202_2e_colors_indicator_only_face() {
    let face = CardFaceDSL {
        color_indicator: Some(mask(8 | 2)),
        ..blank_face("test.indicator_only")
    };
    let ch = from_face(&face);
    assert_eq!(
        ch.colors,
        base_colors(None, Some(mask(8 | 2)), CdaColors::None)
    );
    assert_eq!(ch.colors, mask(10));
}

/// REQ-OBJ-001, CR 202.2, CR 202.2e: colors for a face with both cost and indicator.
#[test]
fn chars_202_2_colors_mixed_face() {
    let c = cost("{W/U}{2}");
    let face = CardFaceDSL {
        mana_cost: Some(c.clone()),
        color_indicator: Some(mask(16)),
        ..blank_face("test.mixed")
    };
    let ch = from_face(&face);
    assert_eq!(
        ch.colors,
        base_colors(Some(&c), Some(mask(16)), CdaColors::None)
    );
    assert_eq!(ch.colors, mask(1 | 2 | 16));
}

/// REQ-OBJ-001, CR 205.2a: an unknown card type in a face is an error.
#[test]
fn chars_205_2a_invalid_card_type_errors() {
    let face = CardFaceDSL {
        card_types: strings(&["Notacardtype"]),
        ..blank_face("test.bad_type")
    };
    assert_eq!(
        Characteristics::from_face(&face, vocab()),
        Err(ObjectError::UnknownCardType("Notacardtype".to_string()))
    );
}

/// REQ-OBJ-001, CR 205.4a: an unknown supertype in a face is an error.
#[test]
fn chars_205_4a_invalid_supertype_errors() {
    let face = CardFaceDSL {
        supertypes: strings(&["Notasupertype"]),
        card_types: strings(&["Creature"]),
        ..blank_face("test.bad_supertype")
    };
    assert_eq!(
        Characteristics::from_face(&face, vocab()),
        Err(ObjectError::UnknownSupertype("Notasupertype".to_string()))
    );
}

/// REQ-OBJ-001, CR 205.3d: a subtype not correlated with the face's card types is an error.
#[test]
fn chars_205_3d_uncorrelated_subtype_errors() {
    let v = vocab();
    let s = v
        .subtypes_of("Land")
        .find(|s| !v.is_subtype_of("Creature", s))
        .expect("land-only subtype")
        .to_string();
    let face = CardFaceDSL {
        card_types: strings(&["Creature"]),
        subtypes: vec![s.clone()],
        ..blank_face("test.bad_subtype")
    };
    assert_eq!(
        Characteristics::from_face(&face, v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: s })
    );
}

/// REQ-OBJ-001, CR 202.3, CR 202.3e: `Characteristics::mana_value` equals the plain subject's
/// mana value for the same cost, including `{X}` on the stack.
#[test]
fn chars_202_3_mana_value_matches_plain_subject() {
    let costs = [
        None,
        Some(cost("{0}")),
        Some(cost("{3}{U}{U}")),
        Some(cost("{X}{R}")),
        Some(cost("{X}{X}{Y}")),
        Some(cost("{2/B}{W/U}{G/P}{C}{S}")),
    ];
    let ctxs = [
        ManaValueContext::OffStack,
        on_stack(0),
        on_stack(3),
        ManaValueContext::OnStack(VariableValues { x: 2, y: 5, z: 0 }),
    ];
    for c in &costs {
        let ch = from_face(&CardFaceDSL {
            mana_cost: c.clone(),
            ..blank_face("test.mv_probe")
        });
        for ctx in ctxs {
            assert_eq!(
                ch.mana_value(ctx),
                mana_value(
                    &ManaValueSubject::Plain {
                        mana_cost: c.as_ref()
                    },
                    ctx
                ),
                "{c:?} {ctx:?}"
            );
        }
    }
    let x = from_face(&CardFaceDSL {
        mana_cost: Some(cost("{X}{R}")),
        ..blank_face("test.x_spell")
    });
    assert_eq!(x.mana_value(ManaValueContext::OffStack), 1);
    assert_eq!(x.mana_value(on_stack(3)), 4);
}

// ---------------------------------------------------------------------------------------------
// Case 14: pt_applies (CR 208.3) and other printed numbers
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 208.3: a noncreature with printed P/T has it off the battlefield, not on it.
#[test]
fn chars_208_3_noncreature_with_printed_pt() {
    let face = CardFaceDSL {
        mana_cost: Some(cost("{3}")),
        card_types: strings(&["Artifact"]),
        power: Some(printed(4)),
        toughness: Some(printed(3)),
        ..blank_face("test.vehicle_like_4_3")
    };
    let ch = from_face(&face);
    assert_eq!(ch.power, Some(printed(4)));
    assert_eq!(ch.toughness, Some(printed(3)));
    assert!(ch.pt_applies(false));
    assert!(!ch.pt_applies(true));
}

/// REQ-OBJ-001, CR 208.3: a creature with printed P/T has it everywhere.
#[test]
fn chars_208_3_creature_with_pt() {
    let ch = from_face(&creature_face("test.vanilla_2_2", Some(cost("{1}{G}"))));
    assert!(ch.pt_applies(false));
    assert!(ch.pt_applies(true));
}

/// REQ-OBJ-001, CR 208.3, CR 208.5: a noncreature with no printed P/T never has power and
/// toughness; a creature has them everywhere (a missing value is 0, CR 208.5). Corrected after
/// CodeRabbit round 1 on PR #170: a creature with no printed P/T has power and toughness.
#[test]
fn chars_208_3_no_printed_pt() {
    let plain = from_face(&CardFaceDSL {
        card_types: strings(&["Enchantment"]),
        ..blank_face("test.plain_enchantment")
    });
    assert!(!plain.pt_applies(false));
    assert!(!plain.pt_applies(true));
    let creature = from_face(&CardFaceDSL {
        card_types: strings(&["Creature"]),
        ..blank_face("test.cda_creature")
    });
    assert!(creature.pt_applies(false));
    assert!(creature.pt_applies(true));
}

/// REQ-OBJ-001, CR 109.3, CR 208.3: a planeswalker exposes its printed loyalty unchanged and has
/// no power or toughness.
#[test]
fn chars_109_3_planeswalker_loyalty() {
    let ch = from_face(&CardFaceDSL {
        mana_cost: Some(cost("{2}{U}{U}")),
        card_types: strings(&["Planeswalker"]),
        loyalty: Some(4),
        ..blank_face("test.walker_4")
    });
    assert_eq!(ch.loyalty, Some(4));
    assert_eq!(ch.defense, None);
    assert!(!ch.pt_applies(false));
    assert!(!ch.pt_applies(true));
}

/// REQ-OBJ-001, CR 109.3, CR 208.3: a battle exposes its printed defense unchanged and has no
/// power or toughness.
#[test]
fn chars_109_3_battle_defense() {
    let ch = from_face(&CardFaceDSL {
        mana_cost: Some(cost("{2}{R}")),
        card_types: strings(&["Battle"]),
        defense: Some(5),
        ..blank_face("test.battle_5")
    });
    assert_eq!(ch.defense, Some(5));
    assert_eq!(ch.loyalty, None);
    assert!(!ch.pt_applies(false));
    assert!(!ch.pt_applies(true));
}

/// REQ-OBJ-001, CR 109.3: a vanguard exposes its hand and life modifiers unchanged, including
/// negative and zero values.
#[test]
fn chars_109_3_vanguard_modifiers() {
    for (hand, life) in [(-1i64, 0i64), (0, -5), (2, 4), (0, 0), (-3, -7)] {
        let ch = from_face(&CardFaceDSL {
            card_types: strings(&["Vanguard"]),
            hand_modifier: Some(hand),
            life_modifier: Some(life),
            ..blank_face("test.vanguard")
        });
        assert_eq!(ch.hand_modifier, Some(hand));
        assert_eq!(ch.life_modifier, Some(life));
        assert!(!ch.pt_applies(false));
    }
}
