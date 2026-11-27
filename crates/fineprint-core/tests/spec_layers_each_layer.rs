// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for each layer of the layer system, the registry and its errors, and base
//! immutability (REQ-LAYER-001). Black-box: written from the work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use fineprint_base::ids::{ObjectId, PlayerId};

use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, LayerError, LayerModification, evaluate,
    evaluate_all,
};
use fineprint_core::objects::Characteristics;
use support::*;

// ---------------------------------------------------------------------------------------------
// item 6: no layer 8
// ---------------------------------------------------------------------------------------------

/// The ordinal of a layer; the exhaustive match without a wildcard arm fails to compile if a
/// variant is added (or one is missing).
fn layer_ordinal(l: Layer) -> u8 {
    match l {
        Layer::L1a => 0,
        Layer::L1b => 1,
        Layer::L2 => 2,
        Layer::L3 => 3,
        Layer::L4 => 4,
        Layer::L5 => 5,
        Layer::L6 => 6,
        Layer::L7a => 7,
        Layer::L7b => 8,
        Layer::L7c => 9,
        Layer::L7d => 10,
    }
}

const ALL_LAYERS: [Layer; 11] = [
    Layer::L1a,
    Layer::L1b,
    Layer::L2,
    Layer::L3,
    Layer::L4,
    Layer::L5,
    Layer::L6,
    Layer::L7a,
    Layer::L7b,
    Layer::L7c,
    Layer::L7d,
];

// REQ-LAYER-001, CR 613.1: the layers are 1a, 1b, 2-6, 7a-7d and there is no layer 8 (the
// match in `layer_ordinal` is exhaustive without a wildcard arm: 11 variants).
#[test]
fn layer_enum_has_exactly_eleven_layers_and_no_layer_8() {
    let ords: Vec<u8> = ALL_LAYERS.iter().map(|l| layer_ordinal(*l)).collect();
    assert_eq!(ords, (0..11).collect::<Vec<u8>>());
}

// ---------------------------------------------------------------------------------------------
// item 1: each layer
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001, CR 613.2a, 707.2: layer 1a CopyValues replaces all characteristics.
#[test]
fn layer_1a_copy_values_replaces_all_characteristics() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.original_1_1", 1, 1),
            vec![kw_ability("a1", "reach")],
        ),
    );
    let copied = Characteristics {
        subtypes: set(&["Testkin"]),
        rules_text: text(&["test.copied_word"]),
        abilities: vec![static_ability("test.copied_static")],
        ..with_colors(creature("test.copied_3_4", 3, 4), 2)
    };
    let r = registry(&[(
        1,
        eff(
            Layer::L1a,
            1,
            objs(&[a]),
            LayerModification::CopyValues(Box::new(copied.clone())),
        ),
    )]);
    let e = evaluate(&r, &env, a).unwrap();
    assert_eq!(norm(&e.characteristics), copied);
    assert_eq!(e.controller, Some(P1));
}

// REQ-LAYER-001, CR 613.2b, 708.2: layer 1b FaceDown replaces all characteristics.
#[test]
fn layer_1b_face_down_replaces_all_characteristics() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.hidden_5_5", 5, 5),
            vec![kw_ability("a1", "trample")],
        ),
    );
    let face_down = Characteristics {
        names: BTreeSet::new(),
        ..creature("unused", 2, 2)
    };
    let r = registry(&[(
        1,
        eff(
            Layer::L1b,
            1,
            objs(&[a]),
            LayerModification::FaceDown(Box::new(face_down.clone())),
        ),
    )]);
    assert_eq!(
        norm(&evaluate(&r, &env, a).unwrap().characteristics),
        face_down
    );
}

// REQ-LAYER-001, CR 613.2a, 613.2b: 1b applies after 1a whatever the timestamps, so the
// face-down values win when both apply (both timestamp orders).
#[test]
fn layer_1b_after_1a_wins_in_both_timestamp_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.base_1_1", 1, 1));
    let copied = creature("test.copied_3_3", 3, 3);
    let face_down = Characteristics {
        names: BTreeSet::new(),
        ..creature("unused", 2, 2)
    };
    for (t_copy, t_down) in [(1, 2), (2, 1)] {
        let r = registry(&[
            (
                1,
                eff(
                    Layer::L1a,
                    t_copy,
                    objs(&[a]),
                    LayerModification::CopyValues(Box::new(copied.clone())),
                ),
            ),
            (
                2,
                eff(
                    Layer::L1b,
                    t_down,
                    objs(&[a]),
                    LayerModification::FaceDown(Box::new(face_down.clone())),
                ),
            ),
        ]);
        assert_eq!(
            norm(&evaluate(&r, &env, a).unwrap().characteristics),
            face_down,
            "copy at {t_copy}, face-down at {t_down}"
        );
    }
}

// REQ-LAYER-001, CR 613.1b: layer 2 SetController sets the evaluated controller; the
// characteristics are untouched.
#[test]
fn layer_2_set_controller() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let b = env.put(oid(2), creature("test.vanilla_3_3", 3, 3));
    let r = registry(&[(
        1,
        eff(
            Layer::L2,
            1,
            objs(&[a]),
            LayerModification::SetController(P2),
        ),
    )]);
    let ea = evaluate(&r, &env, a).unwrap();
    assert_eq!(ea.controller, Some(P2));
    assert_eq!(norm(&ea.characteristics), *env.base_chars(a));
    assert_eq!(evaluate(&r, &env, b).unwrap().controller, Some(P1));
}

// REQ-LAYER-001, CR 613.1b: an object with no controller gains one from a layer-2 effect.
#[test]
fn layer_2_set_controller_on_object_without_controller() {
    let mut env = TestEnv::new();
    let a = env.put_full(oid(1), creature("test.vanilla_2_2", 2, 2), None, false);
    assert_eq!(
        evaluate(&ContinuousEffects::new(), &env, a)
            .unwrap()
            .controller,
        None
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L2,
            1,
            objs(&[a]),
            LayerModification::SetController(P2),
        ),
    )]);
    assert_eq!(evaluate(&r, &env, a).unwrap().controller, Some(P2));
}

// REQ-LAYER-001, CR 613.1c: layer 3 SetText replaces the rules text only.
#[test]
fn layer_3_set_text_replaces_rules_text() {
    let mut env = TestEnv::new();
    let base = Characteristics {
        rules_text: text(&["test.old_word"]),
        ..creature("test.texted_1_1", 1, 1)
    };
    let a = env.put(oid(1), base.clone());
    let r = registry(&[(
        1,
        eff(
            Layer::L3,
            1,
            objs(&[a]),
            LayerModification::SetText(text(&["test.new_word", "test.other_word"])),
        ),
    )]);
    let e = evaluate(&r, &env, a).unwrap();
    let expected = Characteristics {
        rules_text: text(&["test.new_word", "test.other_word"]),
        ..base
    };
    assert_eq!(norm(&e.characteristics), expected);
}

fn typed_base() -> Characteristics {
    Characteristics {
        supertypes: set(&["Legendary"]),
        subtypes: set(&["Testkin", "Probe"]),
        ..with_abilities(artifact("test.typed_artifact"), vec![])
    }
}

// REQ-LAYER-001, CR 613.1d: layer 4 AddTypes is a union of each type line.
#[test]
fn layer_4_add_types_is_a_union() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), typed_base());
    let r = registry(&[(
        1,
        eff(
            Layer::L4,
            1,
            objs(&[a]),
            LayerModification::AddTypes {
                supertypes: set(&["Snow"]),
                card_types: set(&["Creature", "Artifact"]),
                subtypes: set(&["Golemkin"]),
            },
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.supertypes, set(&["Legendary", "Snow"]));
    assert_eq!(c.card_types, set(&["Artifact", "Creature"]));
    assert_eq!(c.subtypes, set(&["Golemkin", "Probe", "Testkin"]));
}

// REQ-LAYER-001, CR 613.1d: layer 4 SetTypes: Some(x) replaces that line, None leaves it.
#[test]
fn layer_4_set_types_some_replaces_none_keeps() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), typed_base());
    let r = registry(&[(
        1,
        eff(
            Layer::L4,
            1,
            objs(&[a]),
            LayerModification::SetTypes {
                supertypes: None,
                card_types: Some(set(&["Enchantment"])),
                subtypes: Some(BTreeSet::new()),
            },
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.supertypes, set(&["Legendary"]));
    assert_eq!(c.card_types, set(&["Enchantment"]));
    assert_eq!(c.subtypes, BTreeSet::new());

    let r = registry(&[(
        1,
        eff(
            Layer::L4,
            1,
            objs(&[a]),
            LayerModification::SetTypes {
                supertypes: Some(set(&["Snow"])),
                card_types: None,
                subtypes: None,
            },
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.supertypes, set(&["Snow"]));
    assert_eq!(c.card_types, set(&["Artifact"]));
    assert_eq!(c.subtypes, set(&["Probe", "Testkin"]));
}

// REQ-LAYER-001, CR 613.1d: layer 4 RemoveSubtypes removes only the listed subtypes.
#[test]
fn layer_4_remove_subtypes() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), typed_base());
    let r = registry(&[(
        1,
        eff(
            Layer::L4,
            1,
            objs(&[a]),
            LayerModification::RemoveSubtypes(set(&["Probe", "Absentkin"])),
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.subtypes, set(&["Testkin"]));
    assert_eq!(c.card_types, set(&["Artifact"]));
    assert_eq!(c.supertypes, set(&["Legendary"]));
}

// REQ-LAYER-001, CR 613.1e: layer 5 SetColors replaces the colors; the color indicator is
// untouched.
#[test]
fn layer_5_set_colors_replaces_colors_not_indicator() {
    let mut env = TestEnv::new();
    let base = Characteristics {
        color_indicator: Some(mask(4)),
        ..with_colors(creature("test.black_2_2", 2, 2), 4 | 16)
    };
    let a = env.put(oid(1), base);
    let r = registry(&[(
        1,
        eff(
            Layer::L5,
            1,
            objs(&[a]),
            LayerModification::SetColors(mask(8)),
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.colors, mask(8));
    assert_eq!(c.color_indicator, Some(mask(4)));
}

// REQ-LAYER-001, CR 613.1e: layer 5 AddColors is a union of the colors.
#[test]
fn layer_5_add_colors_is_a_union() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), with_colors(creature("test.blue_1_1", 1, 1), 2));
    let r = registry(&[(
        1,
        eff(
            Layer::L5,
            1,
            objs(&[a]),
            LayerModification::AddColors(mask(1 | 2)),
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.colors, mask(1 | 2));
    assert_eq!(c.color_indicator, None);
}

// REQ-LAYER-001, CR 613.1f: layer 6 AddAbility adds the ability and keeps the others.
#[test]
fn layer_6_add_ability() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.static_2_2", 2, 2),
            vec![static_ability("test.own")],
        ),
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L6,
            1,
            objs(&[a]),
            LayerModification::AddAbility(kw_ability("test.granted", "flying")),
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(c.abilities.len(), 2);
    assert!(c.abilities.contains(&static_ability("test.own")));
    assert!(c.abilities.contains(&kw_ability("test.granted", "flying")));
    assert_eq!(count_kw(&c, "flying"), 1);
}

// REQ-LAYER-001, CR 613.1f: layer 6 RemoveAllAbilities removes every ability.
#[test]
fn layer_6_remove_all_abilities() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.loaded_2_2", 2, 2),
            vec![
                static_ability("test.own"),
                kw_ability("k1", "flying"),
                kw_ability("k2", "trample"),
            ],
        ),
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L6,
            1,
            objs(&[a]),
            LayerModification::RemoveAllAbilities,
        ),
    )]);
    assert!(
        evaluate(&r, &env, a)
            .unwrap()
            .characteristics
            .abilities
            .is_empty()
    );
}

// REQ-LAYER-001, CR 613.1f: layer 6 RemoveKeyword removes every keyword ability with that
// keyword and nothing else.
#[test]
fn layer_6_remove_keyword_removes_every_instance_only() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.loaded_2_2", 2, 2),
            vec![
                kw_ability("k1", "flying"),
                static_ability("test.own"),
                kw_ability("k2", "flying"),
                kw_ability("k3", "trample"),
            ],
        ),
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L6,
            1,
            objs(&[a]),
            LayerModification::RemoveKeyword("flying".to_string()),
        ),
    )]);
    let c = evaluate(&r, &env, a).unwrap().characteristics;
    assert_eq!(
        c.abilities,
        vec![static_ability("test.own"), kw_ability("k3", "trample")]
    );
}

/// Evaluates `abilities` on one creature under a layer-6 `RemoveKeyword(k)` and returns the
/// remaining abilities.
fn remaining_after_remove_keyword(abilities: Vec<AbilityAST>, k: &str) -> Vec<AbilityAST> {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(creature("test.loaded_2_2", 2, 2), abilities),
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L6,
            1,
            objs(&[a]),
            LayerModification::RemoveKeyword(k.to_string()),
        ),
    )]);
    evaluate(&r, &env, a).unwrap().characteristics.abilities
}

// REQ-LAYER-001, REQ-TEST-001, CR 702.11d, 702.11e, 613.1f: removing hexproof also removes every
// "hexproof from [quality]" ability; look-alikes and other keywords stay, in original order.
#[test]
fn remove_keyword_hexproof_removes_variants() {
    let left = remaining_after_remove_keyword(
        vec![
            kw_ability("k1", "hexproof"),
            kw_ability("k2", "hexproof from red"),
            kw_ability("k3", "hexproof from artifacts"),
            kw_ability("k4", "flying"),
            kw_ability("k5", "hexproofish"),
            static_ability("test.own"),
            kw_ability("k6", "ward 2"),
        ],
        "hexproof",
    );
    assert_eq!(
        left,
        vec![
            kw_ability("k4", "flying"),
            kw_ability("k5", "hexproofish"),
            static_ability("test.own"),
            kw_ability("k6", "ward 2"),
        ]
    );
}

// REQ-LAYER-001, REQ-TEST-001, CR 702.11e, 613.1f: RemoveKeyword("hexproof from red") matches
// that keyword exactly.
#[test]
fn remove_keyword_variant_is_exact() {
    let left = remaining_after_remove_keyword(
        vec![
            kw_ability("k1", "hexproof"),
            kw_ability("k2", "hexproof from red"),
            kw_ability("k3", "hexproof from blue"),
        ],
        "hexproof from red",
    );
    assert_eq!(
        left,
        vec![
            kw_ability("k1", "hexproof"),
            kw_ability("k3", "hexproof from blue")
        ]
    );
}

// REQ-LAYER-001, REQ-TEST-001, CR 702.11e, 613.1f: variant removal is specific to hexproof;
// RemoveKeyword("flying") leaves "flying from red" alone.
#[test]
fn remove_keyword_non_hexproof_stays_exact() {
    let left = remaining_after_remove_keyword(
        vec![
            kw_ability("k1", "flying"),
            kw_ability("k2", "flying from red"),
            kw_ability("k3", "protection from red"),
            kw_ability("k4", "shadow"),
        ],
        "flying",
    );
    assert_eq!(
        left,
        vec![
            kw_ability("k2", "flying from red"),
            kw_ability("k3", "protection from red"),
            kw_ability("k4", "shadow"),
        ]
    );
}

// REQ-LAYER-001, CR 613.4a, 613.4b: SetPt in 7a and 7b sets Some values and leaves None
// values.
#[test]
fn layer_7a_7b_set_pt_some_and_none() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_3", 2, 3));
    for layer in [Layer::L7a, Layer::L7b] {
        let r = registry(&[(1, mk(layer, 1, objs(&[a]), set_pt(Some(5), None)))]);
        assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(5, 3), "{layer:?}");
        let r = registry(&[(1, mk(layer, 1, objs(&[a]), set_pt(None, Some(7))))]);
        assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(2, 7), "{layer:?}");
        let r = registry(&[(1, mk(layer, 1, objs(&[a]), set_pt(None, None)))]);
        assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(2, 3), "{layer:?}");
    }
}

// REQ-LAYER-001, CR 613.4a, 613.4b: 7b applies after 7a whatever the timestamps.
#[test]
fn layer_7b_applies_after_7a_in_both_timestamp_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    for (t7a, t7b) in [(1, 2), (2, 1)] {
        let r = registry(&[
            (
                1,
                cda_eff(Layer::L7a, t7a, objs(&[a]), set_pt(Some(4), Some(4))),
            ),
            (2, eff(Layer::L7b, t7b, objs(&[a]), set_pt(None, Some(1)))),
        ]);
        assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(4, 1));
    }
}

// REQ-LAYER-001, CR 613.4c: 7c ModifyPt adds to power and toughness (negative values
// subtract).
#[test]
fn layer_7c_modify_pt() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(3, -1))),
        (2, eff(Layer::L7c, 2, objs(&[a]), modify_pt(-1, 4))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(4, 5));
}

// REQ-LAYER-001, CR 613.4d: 7d SwitchPt switches power and toughness.
#[test]
fn layer_7d_switch_pt() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_1_3", 1, 3));
    let r = registry(&[(
        1,
        eff(Layer::L7d, 1, objs(&[a]), LayerModification::SwitchPt),
    )]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(3, 1));
}

// REQ-LAYER-001, CR 613.4b, 613.4c, 613.5 example: a 7c +5/+0 with an EARLIER timestamp than
// a 7b setter still applies after it.
#[test]
fn layer_7c_with_earlier_timestamp_applies_after_7b() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(5, 0))),
        (2, eff(Layer::L7b, 5, objs(&[a]), set_pt(Some(0), Some(1)))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(5, 1));
}

// REQ-LAYER-001, CR 613.4d (first example): 1/3 with +0/+1 then a switch is 4/1; a later
// +5/+0 makes it 4/6 (7c applies before 7d even with a later timestamp).
#[test]
fn layer_7d_switch_with_later_7c_example() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_1_3", 1, 3));
    let mut r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(0, 1))),
        (
            2,
            eff(Layer::L7d, 2, objs(&[a]), LayerModification::SwitchPt),
        ),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(4, 1));
    r.register(eid(3), eff(Layer::L7c, 3, objs(&[a]), modify_pt(5, 0)))
        .unwrap();
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(4, 6));
}

// REQ-LAYER-001, CR 613.4d (second and third examples): removing the +0/+1 before the
// switch ends gives 3/1; two switches cancel (1/4).
#[test]
fn layer_7d_switch_examples_two_and_three() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_1_3", 1, 3));
    let mut r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(0, 1))),
        (
            2,
            eff(Layer::L7d, 2, objs(&[a]), LayerModification::SwitchPt),
        ),
    ]);
    assert!(r.remove(eid(1)));
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(3, 1));

    let r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(0, 1))),
        (
            2,
            eff(Layer::L7d, 2, objs(&[a]), LayerModification::SwitchPt),
        ),
        (
            3,
            eff(Layer::L7d, 3, objs(&[a]), LayerModification::SwitchPt),
        ),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(1, 4));
}

// REQ-LAYER-001, CR 613.1: an effect modifies only the objects it applies to.
#[test]
fn effects_modify_only_affected_objects() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let b = env.put(oid(2), creature("test.vanilla_3_3", 3, 3));
    let r = registry(&[
        (1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(1, 1))),
        (
            2,
            eff(
                Layer::L5,
                1,
                objs(&[b]),
                LayerModification::SetColors(mask(16)),
            ),
        ),
    ]);
    let ea = evaluate(&r, &env, a).unwrap();
    let eb = evaluate(&r, &env, b).unwrap();
    assert_eq!(pt(&ea), spt(3, 3));
    assert_eq!(ea.characteristics.colors, ColorBitmask::NONE);
    assert_eq!(pt(&eb), spt(3, 3));
    assert_eq!(eb.characteristics.colors, mask(16));
}

// REQ-LAYER-001, REQ-LAYER-002, CR 613.1: a layer-2 change is visible to a later selector
// (controller of the evaluated view), and the MatchContext carries the effect's source and
// controller and the candidate. CR 109.5, 611.3a: the controller of a static effect with a
// source is the source's evaluated controller (P2, who controls `src`), not the controller
// stored at registration (P1).
#[test]
fn selector_sees_layer_2_result_and_context_fields() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let b = env.put(oid(2), creature("test.vanilla_3_3", 3, 3));
    let src = env.put_full(oid(3), enchantment("test.anthem_source"), Some(P2), true);
    type Seen = Vec<(Option<ObjectId>, PlayerId, ObjectId)>;
    let log: Rc<RefCell<Seen>> = Rc::new(RefCell::new(Vec::new()));
    let log2 = Rc::clone(&log);
    env.add_selector(
        sel(100),
        Rc::new(move |c| {
            log2.borrow_mut()
                .push((c.source, c.effect_controller, c.candidate));
            c.candidate_view.controller == Some(P2)
        }),
    );
    let mut boost = static_eff(
        src,
        None,
        Layer::L7c,
        9,
        Affected::Selector(sel(100)),
        modify_pt(1, 1),
    );
    boost.controller = P1;
    let r = registry(&[
        (
            1,
            eff(
                Layer::L2,
                10,
                objs(&[a]),
                LayerModification::SetController(P2),
            ),
        ),
        (2, boost),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(3, 3));
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(3, 3));
    let seen = log.borrow().clone();
    assert!(seen.contains(&(Some(src), P2, a)));
    assert!(seen.contains(&(Some(src), P2, b)));
    assert!(seen.iter().all(|(s, p, _)| *s == Some(src) && *p == P2));
}

// ---------------------------------------------------------------------------------------------
// item 7: registry, errors and base immutability
// ---------------------------------------------------------------------------------------------

/// An effect for layer `l`: a static CDA in layer 7a (CR 613.4a), a resolved effect elsewhere.
fn mk(l: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    if l == Layer::L7a {
        cda_eff(l, t, affected, m)
    } else {
        eff(l, t, affected, m)
    }
}

/// One valid modification per layer.
fn valid_for(l: Layer) -> LayerModification {
    match l {
        Layer::L1a => LayerModification::CopyValues(Box::new(creature("test.c", 1, 1))),
        Layer::L1b => LayerModification::FaceDown(Box::new(creature("test.f", 2, 2))),
        Layer::L2 => LayerModification::SetController(P2),
        Layer::L3 => LayerModification::SetText(text(&["test.word"])),
        Layer::L4 => add_card_types(&["Creature"]),
        Layer::L5 => LayerModification::AddColors(mask(1)),
        Layer::L6 => LayerModification::RemoveAllAbilities,
        Layer::L7a | Layer::L7b => set_pt(Some(1), None),
        Layer::L7c => modify_pt(1, 1),
        Layer::L7d => LayerModification::SwitchPt,
    }
}

/// Every modification variant, with the layers it belongs to.
fn all_modifications() -> Vec<(LayerModification, Vec<Layer>)> {
    vec![
        (
            LayerModification::CopyValues(Box::new(creature("test.c", 1, 1))),
            vec![Layer::L1a],
        ),
        (
            LayerModification::FaceDown(Box::new(creature("test.f", 2, 2))),
            vec![Layer::L1b],
        ),
        (LayerModification::SetController(P2), vec![Layer::L2]),
        (LayerModification::SetText(text(&["w"])), vec![Layer::L3]),
        (add_card_types(&["Creature"]), vec![Layer::L4]),
        (
            LayerModification::SetTypes {
                supertypes: None,
                card_types: None,
                subtypes: None,
            },
            vec![Layer::L4],
        ),
        (
            LayerModification::RemoveSubtypes(set(&["X"])),
            vec![Layer::L4],
        ),
        (LayerModification::SetColors(mask(1)), vec![Layer::L5]),
        (LayerModification::AddColors(mask(1)), vec![Layer::L5]),
        (
            LayerModification::AddAbility(kw_ability("k", "flying")),
            vec![Layer::L6],
        ),
        (LayerModification::RemoveAllAbilities, vec![Layer::L6]),
        (
            LayerModification::RemoveKeyword("flying".to_string()),
            vec![Layer::L6],
        ),
        (set_pt(Some(1), Some(1)), vec![Layer::L7a, Layer::L7b]),
        (modify_pt(1, 1), vec![Layer::L7c]),
        (LayerModification::SwitchPt, vec![Layer::L7d]),
    ]
}

// REQ-LAYER-001, CR 613.1-613.4: every modification registers in its own layer(s) and is
// rejected with LayerMismatch { layer } in every other layer (at least one wrong variant per
// layer).
#[test]
fn register_rejects_every_wrong_layer_modification_pair() {
    let mut wrong_per_layer = [0usize; 11];
    let mut next = 1u64;
    for (m, ok_layers) in all_modifications() {
        for l in ALL_LAYERS {
            let mut r = ContinuousEffects::new();
            let res = r.register(eid(next), mk(l, 1, objs(&[oid(1)]), m.clone()));
            next += 1;
            if ok_layers.contains(&l) {
                assert_eq!(res, Ok(()), "{m:?} in {l:?}");
                assert_eq!(r.len(), 1);
            } else {
                assert_eq!(
                    res,
                    Err(LayerError::LayerMismatch { layer: l }),
                    "{m:?} in {l:?}"
                );
                assert!(r.is_empty(), "a rejected effect is not stored");
                wrong_per_layer[usize::from(layer_ordinal(l))] += 1;
            }
        }
    }
    assert!(wrong_per_layer.iter().all(|n| *n >= 1));
}

// REQ-LAYER-001: a duplicate EffectId is rejected and the first effect is kept.
#[test]
fn register_duplicate_effect_id() {
    let first = eff(Layer::L7c, 1, objs(&[oid(1)]), modify_pt(1, 1));
    let mut r = registry(&[(7, first.clone())]);
    let res = r.register(eid(7), eff(Layer::L7c, 2, objs(&[oid(1)]), modify_pt(9, 9)));
    assert_eq!(res, Err(LayerError::DuplicateEffect(eid(7))));
    assert_eq!(r.len(), 1);
    assert_eq!(r.get(eid(7)), Some(&first));
}

// REQ-LAYER-001, CR 613.6: Affected::SameAsLayer is rejected as Unsupported in this unit.
#[test]
fn register_same_as_layer_is_unsupported() {
    let mut r = ContinuousEffects::new();
    let res = r.register(
        eid(1),
        eff(
            Layer::L7b,
            1,
            Affected::SameAsLayer(Layer::L4),
            set_pt(Some(3), Some(3)),
        ),
    );
    assert!(matches!(res, Err(LayerError::Unsupported(_))), "{res:?}");
    assert!(r.is_empty());
}

// REQ-LAYER-001: get, len, is_empty, remove (true then false) and set_timestamp
// (UnknownEffect on a missing id).
#[test]
fn registry_get_remove_set_timestamp() {
    let mut r = ContinuousEffects::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    let e = eff(Layer::L7c, 4, objs(&[oid(1)]), modify_pt(1, 1));
    r.register(eid(3), e.clone()).unwrap();
    assert!(!r.is_empty());
    assert_eq!(r.get(eid(3)), Some(&e));
    assert_eq!(r.get(eid(4)), None);

    assert_eq!(r.set_timestamp(eid(3), ts(11)), Ok(()));
    assert_eq!(r.get(eid(3)).unwrap().timestamp, ts(11));
    assert_eq!(
        r.set_timestamp(eid(4), ts(1)),
        Err(LayerError::UnknownEffect(eid(4)))
    );

    assert!(r.remove(eid(3)));
    assert!(!r.remove(eid(3)));
    assert!(r.is_empty());
    assert_eq!(r.get(eid(3)), None);
    assert_eq!(
        r.set_timestamp(eid(3), ts(1)),
        Err(LayerError::UnknownEffect(eid(3)))
    );
}

// REQ-LAYER-001, CR 613.7: set_timestamp changes the application order within a layer.
#[test]
fn set_timestamp_changes_order_within_layer() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let mut r = registry(&[
        (1, eff(Layer::L7b, 1, objs(&[a]), set_pt(Some(1), Some(1)))),
        (2, eff(Layer::L7b, 2, objs(&[a]), set_pt(Some(5), Some(5)))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(5, 5));
    r.set_timestamp(eid(1), ts(3)).unwrap();
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(1, 1));
}

// REQ-LAYER-001: iter() yields ascending EffectId whatever the registration order.
#[test]
fn iter_is_ascending_effect_id() {
    let ids = [9u64, 2, 30, 4, 1];
    let mut r = ContinuousEffects::new();
    for (i, id) in ids.iter().enumerate() {
        r.register(
            eid(*id),
            eff(
                Layer::L7c,
                i as u64,
                objs(&[oid(1)]),
                modify_pt(*id as i64, 0),
            ),
        )
        .unwrap();
    }
    let got: Vec<(u64, i64)> = r
        .iter()
        .map(|(id, e)| {
            let p = match e.modification {
                LayerModification::ModifyPt { power, .. } => power,
                _ => unreachable!(),
            };
            (id.get(), p)
        })
        .collect();
    assert_eq!(got, vec![(1, 1), (2, 2), (4, 4), (9, 9), (30, 30)]);
}

// REQ-LAYER-001: registries with the same content compare equal whatever the registration
// order (ordered by EffectId), and Default equals new().
#[test]
fn registry_equality_ignores_registration_order() {
    let e1 = eff(Layer::L7c, 1, objs(&[oid(1)]), modify_pt(1, 1));
    let e2 = eff(
        Layer::L6,
        2,
        objs(&[oid(1)]),
        LayerModification::RemoveAllAbilities,
    );
    let a = registry(&[(1, e1.clone()), (2, e2.clone())]);
    let b = registry(&[(2, e2), (1, e1)]);
    assert_eq!(a, b);
    assert_eq!(ContinuousEffects::default(), ContinuousEffects::new());
}

// REQ-LAYER-001: an unknown object is an UnknownObject error.
#[test]
fn evaluate_unknown_object() {
    let mut env = TestEnv::new();
    env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    assert_eq!(
        evaluate(&ContinuousEffects::new(), &env, oid(99)),
        Err(LayerError::UnknownObject(oid(99)))
    );
}

fn sample_env() -> TestEnv {
    let mut env = TestEnv::new();
    env.put(
        oid(1),
        with_abilities(
            with_colors(creature("test.white_2_2", 2, 2), 1),
            vec![kw_ability("k1", "flying"), static_ability("test.s1")],
        ),
    );
    env.put(oid(2), enchantment("test.plain_enchantment"));
    env.put(oid(3), land("test.plain_land"));
    env.put_full(oid(4), creature("test.offzone_4_4", 4, 4), None, false);
    env.put_full(
        oid(5),
        Characteristics {
            power: Some(lit(3)),
            toughness: Some(lit(1)),
            ..artifact("test.printed_pt_artifact")
        },
        None,
        false,
    );
    env.put(
        oid(6),
        Characteristics {
            power: None,
            toughness: None,
            ..creature("test.no_pt_creature", 0, 0)
        },
    );
    env.counter(oid(1), "+1/+1", 2, 3);
    env.counter(oid(3), "flying", 1, 4);
    env
}

fn sample_effects() -> ContinuousEffects {
    registry(&[
        (
            1,
            eff(
                Layer::L4,
                5,
                by_sel(SEL_NONCREATURE_ENCHANTMENT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            2,
            eff(
                Layer::L6,
                6,
                by_sel(SEL_CREATURE),
                LayerModification::RemoveKeyword("flying".into()),
            ),
        ),
        (
            3,
            eff(
                Layer::L7b,
                7,
                by_sel(SEL_ENCHANTMENT_CREATURE),
                set_pt(Some(2), Some(2)),
            ),
        ),
        (
            4,
            eff(
                Layer::L7c,
                2,
                objs(&[oid(1), oid(2), oid(3)]),
                modify_pt(1, 0),
            ),
        ),
        (
            5,
            eff(
                Layer::L5,
                1,
                by_sel(SEL_ALL),
                LayerModification::AddColors(mask(8)),
            ),
        ),
        (
            6,
            eff(
                Layer::L2,
                1,
                objs(&[oid(2)]),
                LayerModification::SetController(P2),
            ),
        ),
    ])
}

// REQ-LAYER-001: evaluation never mutates the base definitions: evaluating every object twice
// gives equal results and the environment's base map is unchanged.
#[test]
fn base_definitions_are_never_mutated() {
    let env = sample_env();
    let effects = sample_effects();
    let bases_before = env.bases.clone();
    let counters_before = env.counters.clone();
    let effects_before = effects.clone();
    for id in env.bases.keys() {
        let first = evaluate(&effects, &env, *id).unwrap();
        let second = evaluate(&effects, &env, *id).unwrap();
        assert_eq!(first, second);
    }
    let all1 = evaluate_all(&effects, &env).unwrap();
    let all2 = evaluate_all(&effects, &env).unwrap();
    assert_eq!(all1, all2);
    assert_eq!(env.bases, bases_before);
    assert_eq!(env.counters, counters_before);
    assert_eq!(effects, effects_before);
}

// REQ-LAYER-001, CR 613.1: with zero effects (and no counters) evaluation returns the base
// characteristics and controller unchanged: the values of an object's characteristics start
// from the actual object (P/T already literal, or absent).
#[test]
fn zero_effects_return_the_base() {
    let mut env = sample_env();
    env.counters.clear();
    env.counter_ts.clear();
    let effects = ContinuousEffects::new();
    for (id, b) in &env.bases {
        if *id == oid(6) {
            continue; // excluded fixture: a creature without P/T values gains literal 0/0 (CR 208.5)
        }
        let e = evaluate(&effects, &env, *id).unwrap();
        assert_eq!(norm(&e.characteristics), b.characteristics, "{id:?}");
        assert_eq!(e.controller, b.controller, "{id:?}");
    }
}

// REQ-LAYER-001, CR 208.5 (brief): a creature with no power/toughness value has 0/0 after
// evaluation; a noncreature permanent keeps None; a noncreature off the battlefield with
// printed values keeps them.
#[test]
fn pt_presence_and_missing_values() {
    let env = sample_env();
    let effects = ContinuousEffects::new();
    assert_eq!(pt(&evaluate(&effects, &env, oid(6)).unwrap()), spt(0, 0));
    assert_eq!(pt(&evaluate(&effects, &env, oid(2)).unwrap()), (None, None));
    assert_eq!(pt(&evaluate(&effects, &env, oid(5)).unwrap()), spt(3, 1));
    assert_eq!(pt(&evaluate(&effects, &env, oid(4)).unwrap()), spt(4, 4));
}

// REQ-LAYER-001, CR 613.1: evaluate_all evaluates env.objects() and equals evaluate per
// object. Spot checks: control change (CR 613.1b, layer 2), ability changes (CR 613.1f,
// layer 6) and P/T (CR 613.4, layer 7 sublayers, counters per CR 122.1a, 613.4c).
#[test]
fn evaluate_all_equals_per_object_evaluate() {
    let env = sample_env();
    let effects = sample_effects();
    let all = evaluate_all(&effects, &env).unwrap();
    assert_eq!(
        all.keys().copied().collect::<Vec<_>>(),
        env.bases.keys().copied().collect::<Vec<_>>()
    );
    for (id, e) in &all {
        assert_eq!(*e, evaluate(&effects, &env, *id).unwrap());
    }
    // a spot check of the sample: the enchantment became a P2-controlled red 3/2 creature
    // (control: CR 613.1b; P/T: CR 613.4).
    let e2 = &all[&oid(2)];
    assert_eq!(e2.controller, Some(P2));
    assert!(e2.characteristics.card_types.contains("Creature"));
    assert_eq!(e2.characteristics.colors, mask(8));
    assert_eq!(pt(e2), spt(3, 2));
    // the white creature lost its printed flying (CR 613.1f); its counters still count
    // (CR 122.1a, 613.4c).
    let e1 = &all[&oid(1)];
    assert!(!has_kw(&e1.characteristics, "flying"));
    assert_eq!(pt(e1), spt(5, 4));
    // the land keeps the flying from its counter (CR 122.1b, 613.1f; counter at 4, a removal only hits creatures).
    assert!(has_kw(&all[&oid(3)].characteristics, "flying"));
}

// REQ-LAYER-001: one valid modification per layer registers.
#[test]
fn one_valid_modification_per_layer_registers() {
    for (i, l) in ALL_LAYERS.iter().enumerate() {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register(eid(i as u64 + 1), mk(*l, 1, objs(&[oid(1)]), valid_for(*l))),
            Ok(())
        );
    }
}

// REQ-LAYER-001: LayerError implements std::error::Error and Display.
#[test]
fn layer_error_is_an_error_with_display() {
    let errs = vec![
        LayerError::DuplicateEffect(eid(1)),
        LayerError::UnknownEffect(eid(1)),
        LayerError::UnknownObject(oid(1)),
        LayerError::LayerMismatch { layer: Layer::L2 },
        LayerError::Unsupported("sameAsLayer"),
        LayerError::UnknownPlayer(P1),
        LayerError::DuplicateObject(oid(1)),
        LayerError::IdExhausted,
    ];
    for e in errs {
        let as_err: &dyn std::error::Error = &e;
        assert!(!as_err.to_string().is_empty());
        assert_eq!(e.clone(), e);
    }
}
