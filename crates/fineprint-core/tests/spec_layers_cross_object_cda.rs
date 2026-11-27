// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-001 (#186, acceptance scenarios S-1 to S-4): a synthetic
//! characteristic-defining power/toughness (CR 604.3, evaluated in layer 7a, CR 613.4a) that
//! counts the creatures its controller controls sees the layer-2 controller (CR 613.1b) and the
//! layer-4 card types of every other object. Every scenario has a control without the effect,
//! runs with the CDA object's id both lower and higher than the objects it counts, and checks
//! that every entry point agrees (`evaluate`, `evaluate_with_order`, `evaluate_all`,
//! `LayerCache`) under any permutation or duplication of `objects()` and both registration
//! orders (REQ-CORE-001, REQ-LAYER-005). Black-box: written from the brief only.

#[path = "spec_layers_cross_object_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{ContinuousEffect, Evaluated, LayerModification};
use fineprint_dsl::ability::Layer;
use support::*;

/// The board: C (CDA, P1), K1, K2 (creatures, P1), K3 (creature, P2), N (artifact, P1).
#[derive(Clone, Copy, Debug)]
struct Board {
    c: ObjectId,
    k1: ObjectId,
    k2: ObjectId,
    k3: ObjectId,
    n: ObjectId,
}

/// Both layouts: the CDA object's id lowest, then highest (brief B-4).
fn layouts() -> [Board; 2] {
    [
        Board {
            c: o(1),
            k1: o(2),
            k2: o(3),
            k3: o(4),
            n: o(5),
        },
        Board {
            c: o(9),
            k1: o(2),
            k2: o(3),
            k3: o(4),
            n: o(5),
        },
    ]
}

fn env_for(b: Board) -> Env {
    let mut k1 = creature(2, 2);
    k1.subtypes = set(["TestKindTarget"]);
    Env::new()
        .with(b.c, P1, cda_counter())
        .with(b.k1, P1, k1)
        .with(b.k2, P1, creature(2, 2))
        .with(b.k3, P2, creature(2, 2))
        .with(b.n, P1, noncreature("Artifact"))
}

/// Runs the invariance checks and returns the result map.
fn run(b: Board, list: &[(u64, ContinuousEffect)], ctx: &str) -> BTreeMap<ObjectId, Evaluated> {
    check_invariant(list, &env_for(b), &format!("{ctx} {b:?}"))
}

/// The CDA's value (power == toughness asserted).
fn cda_value(all: &BTreeMap<ObjectId, Evaluated>, id: ObjectId) -> i64 {
    let (pw, th) = pt_of(&all[&id]);
    assert_eq!(pw, th, "power and toughness share the expression");
    pw
}

fn c_value(b: Board, list: &[(u64, ContinuousEffect)], ctx: &str) -> i64 {
    cda_value(&run(b, list, ctx), b.c)
}

// ---------------------------------------------------------------------------------------------
// S-1: layer-2 control (CR 613.1b, 604.3, 613.4a)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-001, CR 604.3, 613.4a: control: without effects C counts itself, K1 and K2 = 3.
#[test]
fn req_layer_001_s1_control_no_effects_counts_three() {
    for b in layouts() {
        assert_eq!(c_value(b, &[], "control"), 3);
    }
}

/// REQ-LAYER-001, CR 613.1b, 604.3: a resolved layer-2 SetController(P2) on K1 (locked set)
/// lowers C to 2; K1's evaluated controller is P2 (S-4).
#[test]
fn req_layer_001_s1a_layer2_locked_control_change_lowers_count() {
    for b in layouts() {
        let list = [(1, eff(Layer::L2, 1, objs(&[b.k1]), set_controller(P2)))];
        let all = run(b, &list, "s1a locked");
        assert_eq!(cda_value(&all, b.c), 2);
        assert_eq!(all[&b.k1].controller, Some(p(P2)));
        assert_eq!(all[&b.c].controller, Some(p(P1)));
    }
}

/// REQ-LAYER-001, CR 613.1b, 611.3, 613.5: a static layer-2 SetController(P2) whose selector
/// picks K1 (by subtype) lowers C to 2.
#[test]
fn req_layer_001_s1a_layer2_static_selector_control_change_lowers_count() {
    for b in layouts() {
        let list = [(
            1,
            static_eff(
                Layer::L2,
                1,
                b.k3,
                sel(has_subtype("TestKindTarget")),
                set_controller(P2),
            ),
        )];
        let all = run(b, &list, "s1a static");
        assert_eq!(cda_value(&all, b.c), 2);
        assert_eq!(all[&b.k1].controller, Some(p(P2)));
    }
}

/// REQ-LAYER-001, CR 613.1b, 604.3: a layer-2 effect on C itself moves C to P2; C then counts
/// P2's creatures (K3 and C) = 2; with K1 also moved to P2, 3. The controller is read from
/// the evaluated C, not its base.
#[test]
fn req_layer_001_s1b_control_change_on_cda_object_counts_new_controller() {
    for b in layouts() {
        let only_c = [(1, eff(Layer::L2, 1, objs(&[b.c]), set_controller(P2)))];
        let all = run(b, &only_c, "s1b c only");
        assert_eq!(cda_value(&all, b.c), 2);
        assert_eq!(all[&b.c].controller, Some(p(P2)));

        let both = [
            (1, eff(Layer::L2, 1, objs(&[b.c]), set_controller(P2))),
            (2, eff(Layer::L2, 2, objs(&[b.k1]), set_controller(P2))),
        ];
        assert_eq!(c_value(b, &both, "s1b c and k1"), 3);
        let both_swapped = [
            (1, eff(Layer::L2, 2, objs(&[b.c]), set_controller(P2))),
            (2, eff(Layer::L2, 1, objs(&[b.k1]), set_controller(P2))),
        ];
        assert_eq!(c_value(b, &both_swapped, "s1b c and k1 swapped ts"), 3);
    }
}

/// REQ-LAYER-001, CR 613.1b, 613.7 (timestamp order): two layer-2 effects on K1, both timestamp
/// orders; the later one wins and C follows (P1 later: 3; P2 later: 2).
#[test]
fn req_layer_001_s1c_two_control_effects_later_timestamp_wins() {
    for b in layouts() {
        for (t_to_p2, t_to_p1, expected, k1_ctrl) in [(1, 2, 3, P1), (2, 1, 2, P2)] {
            let list = [
                (
                    1,
                    eff(Layer::L2, t_to_p2, objs(&[b.k1]), set_controller(P2)),
                ),
                (
                    2,
                    eff(Layer::L2, t_to_p1, objs(&[b.k1]), set_controller(P1)),
                ),
            ];
            let all = run(b, &list, "s1c");
            assert_eq!(
                cda_value(&all, b.c),
                expected,
                "ts p2={t_to_p2} p1={t_to_p1}"
            );
            assert_eq!(all[&b.k1].controller, Some(p(k1_ctrl)));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// S-2: layer-4 type change (CR 613.1d via 613.1 order, 604.3, 613.4a)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-001, CR 604.3, 613.4a: a resolved layer-4 AddTypes{Creature} on N (locked set)
/// raises C from 3 to 4.
#[test]
fn req_layer_001_s2_layer4_add_creature_locked_raises_count() {
    for b in layouts() {
        assert_eq!(c_value(b, &[], "s2 control"), 3);
        let list = [(
            1,
            eff(Layer::L4, 1, objs(&[b.n]), add_card_type("Creature")),
        )];
        let all = run(b, &list, "s2 locked");
        assert_eq!(cda_value(&all, b.c), 4);
        assert!(all[&b.n].characteristics.card_types.contains("Creature"));
    }
}

/// REQ-LAYER-001, CR 611.3, 613.5, 604.3: a static layer-4 AddTypes{Creature} for every
/// artifact (selector, source N) raises C to 4.
#[test]
fn req_layer_001_s2_layer4_add_creature_static_selector_raises_count() {
    for b in layouts() {
        let list = [(
            1,
            static_eff(
                Layer::L4,
                1,
                b.n,
                sel(has_card_type("Artifact")),
                add_card_type("Creature"),
            ),
        )];
        assert_eq!(c_value(b, &list, "s2 static"), 4);
    }
}

/// REQ-LAYER-001, CR 604.3, 613.4a: a layer-4 SetTypes{Artifact} on K1 leaves it without
/// Creature and lowers C to 2.
#[test]
fn req_layer_001_s2_layer4_set_types_removes_creature_lowers_count() {
    for b in layouts() {
        let list = [(
            1,
            eff(Layer::L4, 1, objs(&[b.k1]), set_card_types("Artifact")),
        )];
        assert_eq!(c_value(b, &list, "s2 set types"), 2);
    }
}

/// REQ-LAYER-001, CR 604.3, 613.4a: AddTypes on N and SetTypes on K1 together, both timestamp
/// orders: 3 (one up, one down).
#[test]
fn req_layer_001_s2_layer4_add_and_set_types_both_orders() {
    for b in layouts() {
        for (ta, tb) in [(1, 2), (2, 1)] {
            let list = [
                (
                    1,
                    eff(Layer::L4, ta, objs(&[b.n]), add_card_type("Creature")),
                ),
                (
                    2,
                    eff(Layer::L4, tb, objs(&[b.k1]), set_card_types("Artifact")),
                ),
            ];
            assert_eq!(c_value(b, &list, "s2 add+set"), 3);
        }
    }
}

/// REQ-LAYER-001, CR 613.1, 613.4a: effects of layers 3, 6, 7b, 7c and 7d on other objects do
/// not change card types and leave C at 3 (control); a layer-4 effect alongside them is seen.
#[test]
fn req_layer_001_s2_other_layers_on_others_do_not_change_count() {
    for b in layouts() {
        let others = vec![
            (
                1,
                eff(
                    Layer::L3,
                    1,
                    objs(&[b.k1, b.n]),
                    LayerModification::SetText(text("test.word_other")),
                ),
            ),
            (
                2,
                eff(Layer::L6, 2, objs(&[b.k2, b.n]), add_ability("test.grant")),
            ),
            (
                3,
                eff(
                    Layer::L7b,
                    3,
                    objs(&[b.k1]),
                    LayerModification::SetPt {
                        power: Some(0),
                        toughness: Some(1),
                    },
                ),
            ),
            (
                4,
                eff(Layer::L7c, 4, objs(&[b.k2, b.k3]), modify_pt(-1, -1)),
            ),
            (
                5,
                eff(Layer::L7d, 5, objs(&[b.k3]), LayerModification::SwitchPt),
            ),
        ];
        assert_eq!(c_value(b, &others, "s2 other layers"), 3);
        let mut with_l4 = others.clone();
        with_l4.push((
            6,
            eff(Layer::L4, 6, objs(&[b.n]), add_card_type("Creature")),
        ));
        assert_eq!(c_value(b, &with_l4, "s2 other layers + l4"), 4);
    }
}

/// REQ-LAYER-001, CR 613.1b, 604.3: N becomes a creature (layer 4) while a layer-2 effect hands
/// N to P2: C (P1) does not count it (3); when C is handed to P2 too, C counts K3, N and itself
/// (3), and without N's type change only K3 and itself (2). Both timestamp orders.
#[test]
fn req_layer_001_s2_type_change_counts_only_with_matching_controller() {
    for b in layouts() {
        for (t2, t4) in [(1, 2), (2, 1)] {
            let n_to_p2 = (1, eff(Layer::L2, t2, objs(&[b.n]), set_controller(P2)));
            let n_creature = (
                2,
                eff(Layer::L4, t4, objs(&[b.n]), add_card_type("Creature")),
            );
            let c_to_p2 = (3, eff(Layer::L2, 3, objs(&[b.c]), set_controller(P2)));
            assert_eq!(
                c_value(b, &[n_to_p2.clone(), n_creature.clone()], "n to p2"),
                3
            );
            assert_eq!(
                c_value(
                    b,
                    &[n_to_p2.clone(), n_creature.clone(), c_to_p2.clone()],
                    "n and c to p2"
                ),
                3
            );
            assert_eq!(
                c_value(
                    b,
                    &[n_to_p2.clone(), c_to_p2.clone()],
                    "c to p2, n not creature"
                ),
                2
            );
            // Control: N a creature but staying with P1.
            assert_eq!(
                c_value(b, std::slice::from_ref(&n_creature), "n creature p1"),
                4
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// S-3: many CDA objects (CR 604.3, 613.4a)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-001, CR 604.3, 613.4a, 613.1b: C1 (P1) and C2 (P2) each count their own
/// controller's creatures, with and without control and type effects.
#[test]
fn req_layer_001_s3_two_cda_objects_each_sees_its_controller() {
    // Both id layouts: CDAs lowest, then highest.
    for (c1, c2, k1, k2, k3, n) in [(1, 2, 3, 4, 5, 6), (8, 9, 3, 4, 5, 6)] {
        let (c1, c2, k1, k2, k3, n) = (o(c1), o(c2), o(k1), o(k2), o(k3), o(n));
        let env = Env::new()
            .with(c1, P1, cda_counter())
            .with(c2, P2, cda_counter())
            .with(k1, P1, creature(1, 1))
            .with(k2, P1, creature(1, 1))
            .with(k3, P2, creature(1, 1))
            .with(n, P2, noncreature("Artifact"));
        let value = |list: &[(u64, ContinuousEffect)], ctx: &str| {
            let all = check_invariant(list, &env, ctx);
            (pt_of(&all[&c1]).0, pt_of(&all[&c2]).0)
        };
        assert_eq!(value(&[], "s3 control"), (3, 2));
        let k1_to_p2 = (1, eff(Layer::L2, 1, objs(&[k1]), set_controller(P2)));
        assert_eq!(value(std::slice::from_ref(&k1_to_p2), "s3 k1"), (2, 3));
        let n_creature = (2, eff(Layer::L4, 2, objs(&[n]), add_card_type("Creature")));
        assert_eq!(value(std::slice::from_ref(&n_creature), "s3 n"), (3, 3));
        assert_eq!(
            value(&[k1_to_p2.clone(), n_creature.clone()], "s3 k1 n"),
            (2, 4)
        );
        let c2_to_p1 = (3, eff(Layer::L2, 3, objs(&[c2]), set_controller(P1)));
        assert_eq!(value(std::slice::from_ref(&c2_to_p1), "s3 c2"), (4, 4));
        assert_eq!(value(&[k1_to_p2, n_creature, c2_to_p1], "s3 all"), (3, 3));
    }
}

// ---------------------------------------------------------------------------------------------
// S-4: controller and P/T of the result (CR 613.1b, 613.4a-613.4d)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-001, CR 613.4a, 613.4c: the CDA value is set first (7a), then a +1/+1 counter and
/// a layer-7c +1/+1 effect on C apply: 3 + 1 + 1 = 5; control without them: 3.
#[test]
fn req_layer_001_s4_cda_then_layer_7c_counter_and_effect() {
    for b in layouts() {
        let env = env_for(b).counter(b.c, "+1/+1", 1, 1);
        let list = [(1, eff(Layer::L7c, 2, objs(&[b.c]), modify_pt(1, 1)))];
        let all = check_invariant(&list, &env, "s4 7c");
        assert_eq!(pt_of(&all[&b.c]), (5, 5));
        let all = check_invariant(&[], &env, "s4 counter only");
        assert_eq!(pt_of(&all[&b.c]), (4, 4));
        let all = check_invariant(&[], &env_for(b), "s4 control");
        assert_eq!(pt_of(&all[&b.c]), (3, 3));
    }
}

/// REQ-LAYER-001, CR 613.4b, 613.4c, 613.4d, 613.1b: a non-CDA object keeps the literal path:
/// 2/2, set to 4/6 (7b), +1/+0 (7c), switched (7d) = 6/5, and its layer-2 controller P2 is
/// reported; the CDA (7a) on C is then 2.
#[test]
fn req_layer_001_s4_non_cda_pt_and_controller_unchanged() {
    for b in layouts() {
        let list = [
            (1, eff(Layer::L2, 1, objs(&[b.k1]), set_controller(P2))),
            (
                2,
                eff(
                    Layer::L7b,
                    2,
                    objs(&[b.k1]),
                    LayerModification::SetPt {
                        power: Some(4),
                        toughness: Some(6),
                    },
                ),
            ),
            (3, eff(Layer::L7c, 3, objs(&[b.k1]), modify_pt(1, 0))),
            (
                4,
                eff(Layer::L7d, 4, objs(&[b.k1]), LayerModification::SwitchPt),
            ),
        ];
        let all = check_invariant(&list, &env_for(b), "s4 non-cda");
        assert_eq!(pt_of(&all[&b.k1]), (6, 5));
        assert_eq!(all[&b.k1].controller, Some(p(P2)));
        assert_eq!(pt_of(&all[&b.k2]), (2, 2));
        assert_eq!(all[&b.k3].controller, Some(p(P2)));
        assert_eq!(cda_value(&all, b.c), 2);
        // Control: no effects, K1 2/2 under P1.
        let all = check_invariant(&[], &env_for(b), "s4 control");
        assert_eq!(pt_of(&all[&b.k1]), (2, 2));
        assert_eq!(all[&b.k1].controller, Some(p(P1)));
    }
}

/// REQ-LAYER-001, CR 613.4a, 613.4b: a layer-7b SetPt on C overrides the 7a value (it applies
/// after it): 1/1 plus a +1/+1 layer-7c effect = 2/2.
#[test]
fn req_layer_001_s4_layer_7b_overrides_cda() {
    for b in layouts() {
        let list = [
            (
                1,
                eff(
                    Layer::L7b,
                    1,
                    objs(&[b.c]),
                    LayerModification::SetPt {
                        power: Some(1),
                        toughness: Some(1),
                    },
                ),
            ),
            (2, eff(Layer::L7c, 2, objs(&[b.c]), modify_pt(1, 1))),
        ];
        let all = check_invariant(&list, &env_for(b), "s4 7b");
        assert_eq!(pt_of(&all[&b.c]), (2, 2));
    }
}
