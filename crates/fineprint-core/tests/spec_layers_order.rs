// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the order of effects within a layer (timestamps, ties), selector versus
//! locked sets, and the multi-effect scenarios of SRS REQ-TEST-001 for this unit
//! (REQ-LAYER-001). Black-box: written from the work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use fineprint_core::layers::{ContinuousEffects, LayerModification, evaluate};
use support::*;

// ---------------------------------------------------------------------------------------------
// item 2: ascending timestamp within a layer, both orders
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001, CR 613.3, 613.7: layer 6 RemoveAllAbilities versus AddAbility(flying): the
// later one wins (flying present when the add is later, absent when it is earlier).
#[test]
fn layer_6_remove_all_versus_add_flying_both_orders() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        with_abilities(
            creature("test.static_2_2", 2, 2),
            vec![static_ability("test.own")],
        ),
    );
    for (t_remove, t_add, flying) in [(1, 2, true), (2, 1, false)] {
        let r = registry(&[
            (
                1,
                eff(
                    Layer::L6,
                    t_remove,
                    objs(&[a]),
                    LayerModification::RemoveAllAbilities,
                ),
            ),
            (
                2,
                eff(
                    Layer::L6,
                    t_add,
                    objs(&[a]),
                    LayerModification::AddAbility(kw_ability("test.granted", "flying")),
                ),
            ),
        ]);
        let c = evaluate(&r, &env, a).unwrap().characteristics;
        if flying {
            assert_eq!(c.abilities, vec![kw_ability("test.granted", "flying")]);
        } else {
            assert!(c.abilities.is_empty(), "{:?}", c.abilities);
        }
    }
}

// REQ-LAYER-001, CR 613.4b, 613.7: two layer-7b setters: the later timestamp wins, in both
// orders and whatever the registration order.
#[test]
fn two_7b_setters_last_timestamp_wins_both_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    for (t1, t2, expected) in [(1, 2, spt(5, 6)), (2, 1, spt(1, 3))] {
        let e1 = eff(Layer::L7b, t1, objs(&[a]), set_pt(Some(1), Some(3)));
        let e2 = eff(Layer::L7b, t2, objs(&[a]), set_pt(Some(5), Some(6)));
        let fwd = registry(&[(1, e1.clone()), (2, e2.clone())]);
        let rev = registry(&[(2, e2), (1, e1)]);
        assert_eq!(pt(&evaluate(&fwd, &env, a).unwrap()), expected);
        assert_eq!(pt(&evaluate(&rev, &env, a).unwrap()), expected);
    }
}

// REQ-LAYER-001, CR 613.7 (brief tie-break): at equal timestamps effects apply by ascending
// EffectId, whatever the registration order.
#[test]
fn equal_timestamps_apply_by_ascending_effect_id() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let small = eff(Layer::L7b, 4, objs(&[a]), set_pt(Some(1), Some(1)));
    let big = eff(Layer::L7b, 4, objs(&[a]), set_pt(Some(9), Some(9)));
    // id 1 = 1/1, id 2 = 9/9 -> 9/9 wins
    let r = registry(&[(2, big.clone()), (1, small.clone())]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(9, 9));
    // id 1 = 9/9, id 2 = 1/1 -> 1/1 wins
    let r = registry(&[(1, big), (2, small)]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(1, 1));
}

// REQ-LAYER-001, CR 613.3, 613.7: equal timestamps in layer 6: ascending EffectId decides
// between a removal and an addition.
#[test]
fn equal_timestamps_layer_6_by_effect_id() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let remove = eff(
        Layer::L6,
        3,
        objs(&[a]),
        LayerModification::RemoveAllAbilities,
    );
    let add = eff(
        Layer::L6,
        3,
        objs(&[a]),
        LayerModification::AddAbility(kw_ability("g", "haste")),
    );
    let r = registry(&[(5, add.clone()), (4, remove.clone())]);
    assert!(has_kw(
        &evaluate(&r, &env, a).unwrap().characteristics,
        "haste"
    ));
    let r = registry(&[(4, add), (5, remove)]);
    assert!(!has_kw(
        &evaluate(&r, &env, a).unwrap().characteristics,
        "haste"
    ));
}

// REQ-LAYER-001, CR 613.3, 613.7: two layer-2 control changes: the later one wins.
#[test]
fn two_control_changes_later_wins_both_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    for (t2, t3, expected) in [(1, 2, P3), (2, 1, P2)] {
        let r = registry(&[
            (
                1,
                eff(
                    Layer::L2,
                    t2,
                    objs(&[a]),
                    LayerModification::SetController(P2),
                ),
            ),
            (
                2,
                eff(
                    Layer::L2,
                    t3,
                    objs(&[a]),
                    LayerModification::SetController(P3),
                ),
            ),
        ]);
        assert_eq!(evaluate(&r, &env, a).unwrap().controller, Some(expected));
    }
}

// REQ-LAYER-001, CR 613.3, 613.7: layer 5 SetColors versus AddColors in both orders.
#[test]
fn set_colors_versus_add_colors_both_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), with_colors(creature("test.green_2_2", 2, 2), 16));
    for (t_set, t_add, expected) in [(1, 2, 2 | 4), (2, 1, 2)] {
        let r = registry(&[
            (
                1,
                eff(
                    Layer::L5,
                    t_set,
                    objs(&[a]),
                    LayerModification::SetColors(mask(2)),
                ),
            ),
            (
                2,
                eff(
                    Layer::L5,
                    t_add,
                    objs(&[a]),
                    LayerModification::AddColors(mask(4)),
                ),
            ),
        ]);
        assert_eq!(
            evaluate(&r, &env, a).unwrap().characteristics.colors,
            mask(expected)
        );
    }
}

// REQ-LAYER-001, CR 613.3, 613.7: layer 4 SetTypes versus AddTypes in both orders.
#[test]
fn set_types_versus_add_types_both_orders() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), artifact("test.plain_artifact"));
    let set_ench = LayerModification::SetTypes {
        supertypes: None,
        card_types: Some(set(&["Enchantment"])),
        subtypes: None,
    };
    for (t_set, t_add, expected) in [
        (1, 2, set(&["Creature", "Enchantment"])),
        (2, 1, set(&["Enchantment"])),
    ] {
        let r = registry(&[
            (1, eff(Layer::L4, t_set, objs(&[a]), set_ench.clone())),
            (
                2,
                eff(Layer::L4, t_add, objs(&[a]), add_card_types(&["Creature"])),
            ),
        ]);
        assert_eq!(
            evaluate(&r, &env, a).unwrap().characteristics.card_types,
            expected
        );
    }
}

// ---------------------------------------------------------------------------------------------
// item 2 / item 11: ability-removing + enchantment-animating enchantments, both timestamp orders
// ---------------------------------------------------------------------------------------------

/// The synthetic battlefield: a global "creatures lose all abilities and are 1/1" enchantment
/// (`test.global_ability_remover`), a "noncreature enchantments are 4/4 creatures" enchantment
/// (`test.global_animate_like`), another enchantment and a plain creature.
fn ability_removal_and_animation(t_remove: u64, t_animate: u64) -> (TestEnv, ContinuousEffects) {
    let mut env = TestEnv::new();
    let h = env.put(oid(10), enchantment("test.global_ability_remover"));
    let o = env.put(oid(11), enchantment("test.global_animate_like"));
    env.put(
        oid(12),
        with_abilities(
            creature("test.flier_3_3", 3, 3),
            vec![
                kw_ability("f", "flying"),
                static_ability("test.flier_static"),
            ],
        ),
    );
    env.put(oid(13), enchantment("test.bystander_enchantment"));
    let effects = registry(&[
        // CR 613.1d: layer 4, noncreature enchantments become creatures.
        (
            1,
            static_eff(
                o,
                None,
                Layer::L4,
                t_animate,
                by_sel(SEL_NONCREATURE_ENCHANTMENT),
                add_card_types(&["Creature"]),
            ),
        ),
        // CR 613.4b: layer 7b, those (now enchantment creatures) are 4/4.
        (
            2,
            static_eff(
                o,
                None,
                Layer::L7b,
                t_animate,
                by_sel(SEL_ENCHANTMENT_CREATURE),
                set_pt(Some(4), Some(4)),
            ),
        ),
        // CR 613.1f: layer 6, creatures lose all abilities.
        (
            3,
            static_eff(
                h,
                None,
                Layer::L6,
                t_remove,
                by_sel(SEL_CREATURE),
                LayerModification::RemoveAllAbilities,
            ),
        ),
        // CR 613.4b: layer 7b, creatures are 1/1.
        (
            4,
            static_eff(
                h,
                None,
                Layer::L7b,
                t_remove,
                by_sel(SEL_CREATURE),
                set_pt(Some(1), Some(1)),
            ),
        ),
    ]);
    (env, effects)
}

// REQ-LAYER-001, REQ-TEST-001, CR 613.1, 613.4b, 613.5, 613.7: ability-removing enchantment EARLIER than
// the enchantment-animating enchantment: in 7b the 1/1 applies first and the 4/4 last, so enchantment-creatures
// are 4/4 and the plain creature 1/1; all of them lost their abilities in layer 6 (the
// layer-6 selector sees the layer-4 result).
#[test]
fn ability_removal_before_animation() {
    let (env, effects) = ability_removal_and_animation(1, 2);
    for e in [oid(10), oid(11), oid(13)] {
        let ev = evaluate(&effects, &env, e).unwrap();
        assert_eq!(
            ev.characteristics.card_types,
            set(&["Creature", "Enchantment"]),
            "{e:?}"
        );
        assert!(ev.characteristics.abilities.is_empty(), "{e:?}");
        assert_eq!(pt(&ev), spt(4, 4), "{e:?}");
    }
    let c = evaluate(&effects, &env, oid(12)).unwrap();
    assert_eq!(c.characteristics.card_types, set(&["Creature"]));
    assert!(c.characteristics.abilities.is_empty());
    assert_eq!(pt(&c), spt(1, 1));
}

// REQ-LAYER-001, REQ-TEST-001, CR 613.1, 613.4b, 613.5, 613.7: ability-removing enchantment LATER than
// the enchantment-animating enchantment: the 1/1 applies last, so every creature (enchantments included) is 1/1
// with no abilities.
#[test]
fn ability_removal_after_animation() {
    let (env, effects) = ability_removal_and_animation(2, 1);
    for e in [oid(10), oid(11), oid(13)] {
        let ev = evaluate(&effects, &env, e).unwrap();
        assert_eq!(
            ev.characteristics.card_types,
            set(&["Creature", "Enchantment"]),
            "{e:?}"
        );
        assert!(ev.characteristics.abilities.is_empty(), "{e:?}");
        assert_eq!(pt(&ev), spt(1, 1), "{e:?}");
    }
    let c = evaluate(&effects, &env, oid(12)).unwrap();
    assert!(c.characteristics.abilities.is_empty());
    assert_eq!(pt(&c), spt(1, 1));
}

// REQ-LAYER-001, CR 613.1, 613.5: without the layer-4 effect, the ability-removing effects leave
// the enchantments alone (the selector really depends on the layer-4 result).
#[test]
fn ability_removal_alone_spares_noncreature_enchantments() {
    let (env, mut effects) = ability_removal_and_animation(1, 2);
    assert!(effects.remove(eid(1)));
    let ev = evaluate(&effects, &env, oid(13)).unwrap();
    assert_eq!(norm(&ev.characteristics), *env.base_chars(oid(13)));
    assert_eq!(pt(&ev), (None, None));
}

// ---------------------------------------------------------------------------------------------
// item 3: selector (CR 611.3a) versus locked set (CR 611.2c)
// ---------------------------------------------------------------------------------------------

fn white_and_black() -> TestEnv {
    let mut env = TestEnv::new();
    env.put(oid(1), with_colors(creature("test.white_2_2", 2, 2), 1));
    env.put(oid(2), with_colors(creature("test.black_2_2", 2, 2), 4));
    env
}

// REQ-LAYER-001, CR 611.3a, 613.5: a static "white creatures get +1/+1" (Selector) follows
// layer-5 color changes: a creature turned white gets it, one turned red loses it.
#[test]
fn selector_effect_follows_color_changes() {
    let env = white_and_black();
    let (w, b) = (oid(1), oid(2));
    let anthem = eff(Layer::L7c, 1, by_sel(SEL_WHITE_CREATURE), modify_pt(1, 1));
    let r = registry(&[(1, anthem.clone())]);
    assert_eq!(pt(&evaluate(&r, &env, w).unwrap()), spt(3, 3));
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(2, 2));
    let r = registry(&[
        (1, anthem),
        (
            2,
            eff(
                Layer::L5,
                2,
                objs(&[b]),
                LayerModification::SetColors(mask(1)),
            ),
        ),
        (
            3,
            eff(
                Layer::L5,
                3,
                objs(&[w]),
                LayerModification::SetColors(mask(8)),
            ),
        ),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(3, 3));
    assert_eq!(pt(&evaluate(&r, &env, w).unwrap()), spt(2, 2));
}

// REQ-LAYER-001, CR 611.2c: a resolved "white creatures get +1/+1" locks its set (the white
// creatures at creation): a creature turned white later does not get it, one turned red keeps
// it.
#[test]
fn locked_set_effect_ignores_color_changes() {
    let env = white_and_black();
    let (w, b) = (oid(1), oid(2));
    let locked = eff(Layer::L7c, 1, objs(&[w]), modify_pt(1, 1));
    let r = registry(&[
        (1, locked),
        (
            2,
            eff(
                Layer::L5,
                2,
                objs(&[b]),
                LayerModification::SetColors(mask(1)),
            ),
        ),
        (
            3,
            eff(
                Layer::L5,
                3,
                objs(&[w]),
                LayerModification::SetColors(mask(8)),
            ),
        ),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(2, 2));
    assert_eq!(pt(&evaluate(&r, &env, w).unwrap()), spt(3, 3));
}

// REQ-LAYER-001, CR 611.2c, 611.3a: both versions together: the creature turned white gets
// the bonus only from the Selector effect, the creature turned red only from the locked one.
#[test]
fn selector_and_locked_set_together() {
    let env = white_and_black();
    let (w, b) = (oid(1), oid(2));
    let r = registry(&[
        (
            1,
            eff(Layer::L7c, 1, by_sel(SEL_WHITE_CREATURE), modify_pt(1, 1)),
        ),
        (2, eff(Layer::L7c, 2, objs(&[w]), modify_pt(1, 1))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, w).unwrap()), spt(4, 4));
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(2, 2));
    let mut r2 = r.clone();
    r2.register(
        eid(3),
        eff(
            Layer::L5,
            3,
            objs(&[b]),
            LayerModification::SetColors(mask(1)),
        ),
    )
    .unwrap();
    r2.register(
        eid(4),
        eff(
            Layer::L5,
            4,
            objs(&[w]),
            LayerModification::SetColors(mask(8)),
        ),
    )
    .unwrap();
    assert_eq!(pt(&evaluate(&r2, &env, b).unwrap()), spt(3, 3));
    assert_eq!(pt(&evaluate(&r2, &env, w).unwrap()), spt(3, 3));
}

// REQ-LAYER-001, CR 611.2c: a locked set never reaches an object outside it, even one that
// would have matched (a white creature that arrived later).
#[test]
fn locked_set_never_reaches_a_later_object() {
    let mut env = white_and_black();
    let late = env.put(
        oid(3),
        with_colors(creature("test.late_white_2_2", 2, 2), 1),
    );
    let r = registry(&[(1, eff(Layer::L7c, 1, objs(&[oid(1)]), modify_pt(1, 1)))]);
    assert_eq!(pt(&evaluate(&r, &env, late).unwrap()), spt(2, 2));
}

// ---------------------------------------------------------------------------------------------
// item 4: CR 613.5 second example
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001, CR 613.5 (example), 613.4b, 613.4c, 122.1a: a 2/2 creature with a +1/+1
// counter, a resolved +4/+4, a static +0/+2 and a resolved "becomes 0/1" (7b) is 5/8, for
// every registration order and every assignment of timestamps.
#[test]
fn counter_and_three_effects_give_5_8_in_any_order() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let src = env.put(oid(2), enchantment("test.anthem_0_2"));
    let make = |stamps: &[u64]| {
        vec![
            eff(Layer::L7c, stamps[1], objs(&[a]), modify_pt(4, 4)),
            static_eff(
                src,
                None,
                Layer::L7c,
                stamps[2],
                by_sel(SEL_CREATURE),
                modify_pt(0, 2),
            ),
            eff(Layer::L7b, stamps[3], objs(&[a]), set_pt(Some(0), Some(1))),
        ]
    };
    let mut checked = 0;
    for stamp_perm in permutations(4) {
        let stamps: Vec<u64> = stamp_perm.iter().map(|i| *i as u64 + 1).collect();
        let mut env = env.clone();
        env.counter(a, "+1/+1", 1, stamps[0]);
        let effects = make(&stamps);
        for reg_order in permutations(3) {
            let mut r = ContinuousEffects::new();
            for (n, i) in reg_order.iter().enumerate() {
                r.register(eid(n as u64 + 1), effects[*i].clone()).unwrap();
            }
            assert_eq!(
                pt(&evaluate(&r, &env, a).unwrap()),
                spt(5, 8),
                "stamps {stamps:?}, registration {reg_order:?}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 24 * 6);
}

// ---------------------------------------------------------------------------------------------
// item 11: 613.6 locked set across layers 4 and 7b (locked land animation), at this unit's level
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001, REQ-TEST-001, CR 613.6 (example), 611.2c: a land given a layer-4 "is a
// creature" and a layer-7b "3/3" with the SAME locked set, then a 7c +1/+1, is a 4/4 land
// creature that keeps its land abilities.
#[test]
fn locked_land_animation_pair_then_pumped_is_4_4() {
    let mut env = TestEnv::new();
    let l = env.put(oid(1), land("test.animated_land"));
    let other = env.put(oid(2), land("test.other_land"));
    let r = registry(&[
        (
            1,
            eff(Layer::L4, 1, objs(&[l]), add_card_types(&["Creature"])),
        ),
        (2, eff(Layer::L7b, 1, objs(&[l]), set_pt(Some(3), Some(3)))),
        (3, eff(Layer::L7c, 2, objs(&[l]), modify_pt(1, 1))),
    ]);
    let e = evaluate(&r, &env, l).unwrap();
    assert_eq!(e.characteristics.card_types, set(&["Creature", "Land"]));
    assert_eq!(e.characteristics.abilities, env.base_chars(l).abilities);
    assert_eq!(pt(&e), spt(4, 4));
    let o = evaluate(&r, &env, other).unwrap();
    assert_eq!(norm(&o.characteristics), *env.base_chars(other));
    assert_eq!(pt(&o), (None, None));
}

// REQ-LAYER-001, CR 613.6, 613.4b, 613.4c: the same scenario with a later second "becomes a
// creature, P/T 2/2" locked pair (a later 7b) gives 3/3: the later 7b setter applies after the
// first and the +1/+1 still applies after both.
#[test]
fn land_animated_twice_later_setter_wins_pump_still_applies() {
    let mut env = TestEnv::new();
    let l = env.put(oid(1), land("test.animated_land"));
    let r = registry(&[
        (
            1,
            eff(Layer::L4, 1, objs(&[l]), add_card_types(&["Creature"])),
        ),
        (2, eff(Layer::L7b, 1, objs(&[l]), set_pt(Some(3), Some(3)))),
        (3, eff(Layer::L7c, 2, objs(&[l]), modify_pt(1, 1))),
        (
            4,
            eff(Layer::L4, 3, objs(&[l]), add_card_types(&["Creature"])),
        ),
        (5, eff(Layer::L7b, 3, objs(&[l]), set_pt(Some(2), Some(2)))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, l).unwrap()), spt(3, 3));
}
