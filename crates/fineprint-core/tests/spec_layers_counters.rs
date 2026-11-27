// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for counters in the layer system (keyword counters in layer 6, P/T counters in
//! layer 7c), P/T presence and non-literal base P/T (REQ-LAYER-001). Black-box: written from
//! the work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use fineprint_core::layers::{
    ContinuousEffects, LayerModification, evaluate, keyword_counter_ability,
};
use fineprint_core::objects::Characteristics;
use support::*;

/// The keyword-counter kinds of CR 122.1b (without variants).
const KEYWORD_COUNTERS: [&str; 15] = [
    "flying",
    "first strike",
    "double strike",
    "deathtouch",
    "decayed",
    "exalted",
    "haste",
    "hexproof",
    "indestructible",
    "lifelink",
    "menace",
    "reach",
    "shadow",
    "trample",
    "vigilance",
];

// REQ-LAYER-001, CR 122.1b, 613.1f: a flying counter gives exactly one keyword ability
// `flying` with ability id "counter:flying", whatever the amount.
#[test]
fn flying_counter_gives_exactly_one_keyword_whatever_the_amount() {
    for amount in [1, 2, 5] {
        let mut env = TestEnv::new();
        let a = env.put(
            oid(1),
            with_abilities(
                creature("test.vanilla_2_2", 2, 2),
                vec![static_ability("test.own")],
            ),
        );
        env.counter(a, "flying", amount, 3);
        let c = evaluate(&ContinuousEffects::new(), &env, a)
            .unwrap()
            .characteristics;
        assert_eq!(
            keywords(&c),
            vec![("counter:flying".to_string(), "flying".to_string())],
            "amount {amount}"
        );
        assert!(c.abilities.contains(&static_ability("test.own")));
        assert_eq!(c.abilities.len(), 2);
        // P/T untouched by a keyword counter.
        assert_eq!(
            pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
            spt(2, 2)
        );
    }
}

// REQ-LAYER-001, CR 122.1b: the exact ability for a keyword counter is
// `AbilityAST::Keyword { base.ability_id: "counter:<kind>", keyword: <kind> }`.
#[test]
fn keyword_counter_ability_has_the_exact_form() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    env.counter(a, "first strike", 1, 3);
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    assert_eq!(
        c.abilities,
        vec![kw_ability("counter:first strike", "first strike")]
    );
}

// REQ-LAYER-001, CR 122.1b, 613.1f: each of the 15 keyword-counter kinds gives its keyword.
#[test]
fn all_fifteen_keyword_counter_kinds_work() {
    for kind in KEYWORD_COUNTERS {
        let mut env = TestEnv::new();
        let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
        env.counter(a, kind, 1, 2);
        let c = evaluate(&ContinuousEffects::new(), &env, a)
            .unwrap()
            .characteristics;
        assert_eq!(
            keywords(&c),
            vec![(format!("counter:{kind}"), kind.to_string())],
            "{kind}"
        );
    }
    // all at once: one ability each.
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    for (i, kind) in KEYWORD_COUNTERS.iter().enumerate() {
        env.counter(a, kind, 1 + i as i64 % 3, 2);
    }
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    for kind in KEYWORD_COUNTERS {
        assert_eq!(count_kw(&c, kind), 1, "{kind}");
    }
    assert_eq!(c.abilities.len(), 15);
}

// REQ-LAYER-001, CR 122.1b: "ward" is not a keyword-counter kind, and other kinds add
// nothing either.
#[test]
fn ward_and_other_counters_add_no_ability() {
    let mut env = TestEnv::new();
    let base = with_abilities(
        creature("test.vanilla_2_2", 2, 2),
        vec![static_ability("test.own")],
    );
    let a = env.put(oid(1), base.clone());
    env.counter(a, "ward", 1, 2);
    env.counter(a, "test.charge", 4, 2);
    env.counter(a, "Flying", 1, 2);
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    assert_eq!(norm(&c), base);
}

// REQ-LAYER-001, CR 122.1b: a keyword counter works on a noncreature permanent too.
#[test]
fn keyword_counter_on_noncreature_permanent() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), artifact("test.plain_artifact"));
    env.counter(a, "indestructible", 1, 2);
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    assert_eq!(count_kw(&c, "indestructible"), 1);
    assert_eq!(c.power, None);
}

// REQ-LAYER-001, CR 613.1f, 613.7c: a keyword counter applies in layer 6 at its timestamp: a
// RemoveAllAbilities at T removes it when the counter is earlier, not when it is later; at the
// same timestamp effects apply before counters (brief tie-break) so the keyword stays.
#[test]
fn keyword_counter_versus_remove_all_abilities_both_orders() {
    for (counter_ts, present) in [(3, false), (7, true), (5, true)] {
        let mut env = TestEnv::new();
        let a = env.put(
            oid(1),
            with_abilities(
                creature("test.vanilla_2_2", 2, 2),
                vec![static_ability("test.own")],
            ),
        );
        env.counter(a, "flying", 2, counter_ts);
        let r = registry(&[(
            1,
            eff(
                Layer::L6,
                5,
                objs(&[a]),
                LayerModification::RemoveAllAbilities,
            ),
        )]);
        let c = evaluate(&r, &env, a).unwrap().characteristics;
        assert!(
            !c.abilities.contains(&static_ability("test.own")),
            "printed ability removed (counter at {counter_ts})"
        );
        assert_eq!(
            has_kw(&c, "flying"),
            present,
            "counter at {counter_ts}, removal at 5"
        );
    }
}

// REQ-LAYER-001, CR 613.1f, 613.7c: a RemoveKeyword at T versus a keyword counter: same
// timestamp order rule.
#[test]
fn keyword_counter_versus_remove_keyword_both_orders() {
    for (counter_ts, present) in [(2, false), (9, true)] {
        let mut env = TestEnv::new();
        let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
        env.counter(a, "trample", 1, counter_ts);
        let r = registry(&[(
            1,
            eff(
                Layer::L6,
                5,
                objs(&[a]),
                LayerModification::RemoveKeyword("trample".to_string()),
            ),
        )]);
        let c = evaluate(&r, &env, a).unwrap().characteristics;
        assert_eq!(has_kw(&c, "trample"), present, "counter at {counter_ts}");
    }
}

// REQ-LAYER-001, REQ-TEST-001, CR 702.11e, 122.1b, 613.1f, 613.7c: a variant keyword counter
// ("hexproof from red") is removed by a later RemoveKeyword("hexproof") and kept by an earlier
// one; the kept ability carries the full keyword string.
#[test]
fn remove_keyword_hexproof_vs_variant_counter_both_timestamp_orders() {
    for (counter_ts, present) in [(2, false), (9, true)] {
        let mut env = TestEnv::new();
        let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
        env.counter(a, "hexproof from red", 1, counter_ts);
        let r = registry(&[(
            1,
            eff(
                Layer::L6,
                5,
                objs(&[a]),
                LayerModification::RemoveKeyword("hexproof".to_string()),
            ),
        )]);
        let c = evaluate(&r, &env, a).unwrap().characteristics;
        assert_eq!(
            has_kw(&c, "hexproof from red"),
            present,
            "counter at {counter_ts}"
        );
    }
}

// REQ-LAYER-001, CR 613.7c (brief): a counter without a counter timestamp is treated as
// timestamp 0, so any removal with a later timestamp removes its keyword.
#[test]
fn counter_without_timestamp_counts_as_timestamp_zero() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    env.counter_no_ts(a, "haste", 1);
    assert!(has_kw(
        &evaluate(&ContinuousEffects::new(), &env, a)
            .unwrap()
            .characteristics,
        "haste"
    ));
    let r = registry(&[(
        1,
        eff(
            Layer::L6,
            1,
            objs(&[a]),
            LayerModification::RemoveAllAbilities,
        ),
    )]);
    assert!(!has_kw(
        &evaluate(&r, &env, a).unwrap().characteristics,
        "haste"
    ));
}

// REQ-LAYER-001, CR 122.1a, 613.4c: "+1/+1" x3, "-1/-1" x1 and "+2/+0" x2 add up in 7c.
#[test]
fn pt_counters_add_up_in_7c() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_1_1", 1, 1));
    env.counter(a, "+1/+1", 3, 2);
    env.counter(a, "-1/-1", 1, 3);
    env.counter(a, "+2/+0", 2, 4);
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    // power 1 + 3 - 1 + 4 = 7; toughness 1 + 3 - 1 + 0 = 3
    assert_eq!(pt(&e), spt(7, 3));
    assert!(e.characteristics.abilities.is_empty());
}

// REQ-LAYER-001, CR 122.1a, 613.4b, 613.4c: P/T counters apply in 7c, after a 7b setter even
// when the setter's timestamp is later than the counters', and before a 7d switch.
#[test]
fn pt_counters_apply_after_7b_and_before_7d() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    env.counter(a, "+1/+1", 2, 1);
    env.counter(a, "+0/+3", 1, 1);
    let r = registry(&[(1, eff(Layer::L7b, 9, objs(&[a]), set_pt(Some(0), Some(1))))]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(2, 6));
    let r = registry(&[
        (1, eff(Layer::L7b, 9, objs(&[a]), set_pt(Some(0), Some(1)))),
        (
            2,
            eff(Layer::L7d, 0, objs(&[a]), LayerModification::SwitchPt),
        ),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(6, 2));
}

// REQ-LAYER-001, CR 122.1a, 613.4c: kinds that are not exactly "+X/+Y" or "-X/-Y" do not
// change P/T.
#[test]
fn malformed_pt_counter_kinds_do_nothing() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    for kind in [
        "+1/-1",
        "1/1",
        "+1/+1 ",
        "+a/+1",
        "+1+1",
        "-1/+1",
        "test.charge",
    ] {
        env.counter(a, kind, 2, 1);
    }
    assert_eq!(
        pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
        spt(2, 2)
    );
}

// REQ-LAYER-001, CR 208.3, 122.1a: a P/T counter on a noncreature battlefield permanent does
// not create P/T.
#[test]
fn pt_counter_on_noncreature_permanent_creates_no_pt() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), artifact("test.plain_artifact"));
    env.counter(a, "+1/+1", 2, 1);
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    assert_eq!(e.characteristics.power, None);
    assert_eq!(e.characteristics.toughness, None);
    // a 7c effect does not create P/T either
    let r = registry(&[(1, eff(Layer::L7c, 2, objs(&[a]), modify_pt(3, 3)))]);
    let e = evaluate(&r, &env, a).unwrap();
    assert_eq!(pt(&e), (None, None));
}

// REQ-LAYER-001, CR 208.3, 613.1d: P/T presence is decided after layers 1-6: a noncreature
// permanent animated in layer 4 has P/T (missing values are 0) and its counters count.
#[test]
fn pt_presence_decided_after_layer_4() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), artifact("test.plain_artifact"));
    env.counter(a, "+1/+1", 2, 1);
    let r = registry(&[(
        1,
        eff(Layer::L4, 5, objs(&[a]), add_card_types(&["Creature"])),
    )]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(2, 2));
}

// REQ-LAYER-001, CR 208.3: a creature turned into a noncreature in layer 4 loses its P/T:
// they are left None and 7a-7d do nothing.
#[test]
fn creature_made_noncreature_has_no_pt() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    let r = registry(&[
        (
            1,
            eff(
                Layer::L4,
                1,
                objs(&[a]),
                LayerModification::SetTypes {
                    supertypes: None,
                    card_types: Some(set(&["Artifact"])),
                    subtypes: None,
                },
            ),
        ),
        (2, eff(Layer::L7c, 2, objs(&[a]), modify_pt(1, 1))),
    ]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), (None, None));
}

/// A noncreature artifact card with printed 2/1, off the battlefield.
fn artifact_card_2_1_off_battlefield(env: &mut TestEnv) -> fineprint_base::ids::ObjectId {
    env.put_full(
        oid(1),
        Characteristics {
            power: Some(lit(2)),
            toughness: Some(lit(1)),
            ..artifact("test.vehicle_like_card")
        },
        None,
        false,
    )
}

// REQ-LAYER-001, CR 122.1a, 208.3: a noncreature card off the battlefield with printed P/T
// keeps that P/T (presence unchanged) but a +X/+Y counter does not change it: +X/+Y counters
// apply only to a creature or a creature card.
#[test]
fn noncreature_off_battlefield_with_printed_pt_ignores_counters() {
    let mut env = TestEnv::new();
    let a = artifact_card_2_1_off_battlefield(&mut env);
    env.counter(a, "+1/+1", 1, 1);
    assert_eq!(
        pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
        spt(2, 1)
    );
}

// REQ-LAYER-001, CR 122.1a: a -X/-Y counter on that noncreature artifact card changes nothing
// either.
#[test]
fn minus_counter_on_noncreature_card_with_printed_pt_changes_nothing() {
    let mut env = TestEnv::new();
    let a = artifact_card_2_1_off_battlefield(&mut env);
    env.counter(a, "-1/-1", 1, 1);
    assert_eq!(
        pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
        spt(2, 1)
    );
}

// REQ-LAYER-001, CR 122.1a, 613.4c, 613.1d: the same artifact made a creature by a layer-4
// effect while on the battlefield: the counter applies (3/2).
#[test]
fn pt_counter_applies_once_a_layer_4_effect_makes_it_a_creature() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(lit(2)),
            toughness: Some(lit(1)),
            ..artifact("test.vehicle_like_card")
        },
    );
    env.counter(a, "+1/+1", 1, 1);
    let r = registry(&[(
        1,
        eff(Layer::L4, 2, objs(&[a]), add_card_types(&["Creature"])),
    )]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(3, 2));
}

// REQ-LAYER-001, CR 122.1a: a creature card in a zone other than the battlefield with printed
// 2/1 and a +1/+1 counter is 3/2.
#[test]
fn pt_counter_applies_to_creature_card_off_battlefield() {
    let mut env = TestEnv::new();
    let a = env.put_full(oid(1), creature("test.vanilla_2_1", 2, 1), None, false);
    env.counter(a, "+1/+1", 1, 1);
    assert_eq!(
        pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
        spt(3, 2)
    );
}

// REQ-LAYER-001, CR 122.1b, 702.11d, 613.1f: a keyword-counter kind is one of the 15 names, or
// one of them followed by one space and more text (a variant). The ability uses the FULL kind
// string; `keyword_counter_ability` yields exactly that.
#[test]
fn keyword_counter_variant_gives_the_full_string_ability() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
    env.counter(a, "hexproof from red", 3, 2);
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    assert_eq!(
        keywords(&c),
        vec![(
            "counter:hexproof from red".to_string(),
            "hexproof from red".to_string()
        )]
    );
    assert_eq!(c.abilities.len(), 1);
    assert_eq!(
        c.abilities,
        vec![keyword_counter_ability("hexproof from red")]
    );
    assert_eq!(
        c.abilities,
        vec![kw_ability("counter:hexproof from red", "hexproof from red")]
    );
}

// REQ-LAYER-001, CR 122.1b, 613.1f, 613.7c: a variant counter applies in layer 6 at the
// counter's timestamp, in both orders against RemoveAllAbilities (earlier: removed; later:
// kept).
#[test]
fn keyword_counter_variant_versus_remove_all_abilities_both_orders() {
    for (counter_ts, present) in [(3, false), (7, true)] {
        let mut env = TestEnv::new();
        let a = env.put(oid(1), creature("test.vanilla_2_2", 2, 2));
        env.counter(a, "hexproof from red", 1, counter_ts);
        let r = registry(&[(
            1,
            eff(
                Layer::L6,
                5,
                objs(&[a]),
                LayerModification::RemoveAllAbilities,
            ),
        )]);
        let c = evaluate(&r, &env, a).unwrap().characteristics;
        assert_eq!(
            has_kw(&c, "hexproof from red"),
            present,
            "counter at {counter_ts}, removal at 5"
        );
    }
}

// REQ-LAYER-001, CR 122.1b: "ward" and "ward 2" are not keyword counters; neither are kinds
// that merely start with a keyword name without a space.
#[test]
fn ward_and_near_miss_kinds_give_nothing() {
    for kind in ["ward", "ward 2", "flyingx", "hexproofish"] {
        let mut env = TestEnv::new();
        let base = creature("test.vanilla_2_2", 2, 2);
        let a = env.put(oid(1), base.clone());
        env.counter(a, kind, 1, 2);
        let c = evaluate(&ContinuousEffects::new(), &env, a)
            .unwrap()
            .characteristics;
        assert!(c.abilities.is_empty(), "{kind}");
    }
}

// REQ-LAYER-001, CR 122.1b, 702.11d: a variant counter on a noncreature permanent works.
#[test]
fn keyword_counter_variant_on_noncreature_permanent() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), artifact("test.plain_artifact"));
    env.counter(a, "hexproof from red", 1, 2);
    let c = evaluate(&ContinuousEffects::new(), &env, a)
        .unwrap()
        .characteristics;
    assert_eq!(count_kw(&c, "hexproof from red"), 1);
    assert_eq!(c.power, None);
}

// REQ-LAYER-001, CR 208.2, 604.3, 613.4a: a non-literal base power is resolved by
// `eval_value` at the start of layer 7a, then modified like any other value.
#[test]
fn non_literal_base_power_uses_eval_value() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(non_literal_x()),
            ..creature("test.cda_like_x_2", 0, 2)
        },
    );
    env.values.push((non_literal_x(), a, Some(6)));
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    assert_eq!(pt(&e), spt(6, 2));
    let r = registry(&[(1, eff(Layer::L7c, 1, objs(&[a]), modify_pt(1, 1)))]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(7, 3));
    // a 7b setter overrides the evaluated base
    let r = registry(&[(1, eff(Layer::L7b, 1, objs(&[a]), set_pt(Some(1), None)))]);
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(1, 2));
}

// REQ-LAYER-001, CR 208.5: a non-literal base value the environment cannot evaluate counts
// as 0 for a creature; so does a missing value.
#[test]
fn unevaluable_or_missing_base_values_count_as_zero() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(non_literal_memory("test.key")),
            toughness: None,
            ..creature("test.cda_like", 0, 0)
        },
    );
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    assert_eq!(pt(&e), spt(0, 0));
    env.counter(a, "+1/+1", 2, 1);
    assert_eq!(
        pt(&evaluate(&ContinuousEffects::new(), &env, a).unwrap()),
        spt(2, 2)
    );
}

// REQ-LAYER-001, CR 208.2, 604.3: `eval_value` is asked for the object being evaluated (two
// objects with the same expression get their own values).
#[test]
fn eval_value_uses_the_object_id() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(non_literal_x()),
            ..creature("test.cda_a", 0, 1)
        },
    );
    let b = env.put(
        oid(2),
        Characteristics {
            power: Some(non_literal_x()),
            ..creature("test.cda_b", 0, 1)
        },
    );
    env.values.push((non_literal_x(), a, Some(3)));
    env.values.push((non_literal_x(), b, Some(8)));
    let r = ContinuousEffects::new();
    assert_eq!(pt(&evaluate(&r, &env, a).unwrap()), spt(3, 1));
    assert_eq!(pt(&evaluate(&r, &env, b).unwrap()), spt(8, 1));
}

// REQ-LAYER-001, CR 613.4a: after layer 7a the values are literals, and evaluation is a
// fixpoint: the evaluated characteristics used as a base evaluate to themselves (the literal
// form the evaluator emits is one it reads back as the same number).
#[test]
fn evaluated_pt_is_literal_and_a_fixpoint() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(non_literal_x()),
            ..creature("test.cda", 0, 4)
        },
    );
    env.values.push((non_literal_x(), a, Some(5)));
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    assert_eq!(pt(&e), spt(5, 4));
    let mut again = TestEnv::new();
    let b = again.put(oid(2), e.characteristics.clone());
    let e2 = evaluate(&ContinuousEffects::new(), &again, b).unwrap();
    assert_eq!(e2.characteristics, e.characteristics);
}

// REQ-LAYER-001, REQ-DSL-003, CR 613.4a: evaluated P/T are `ValueExpressionAST::Literal(n)`.
// #232 FP-164: the `Characteristics` fields hold the typed AST, so the former bare-JSON-integer
// placeholder form (decision "#32 FP-031") no longer exists: its JSON is refused as a typed value
// expression, and the emitted form is asserted to be exactly `Literal(n)` (`{"LITERAL": n}`).
#[test]
fn evaluated_pt_uses_the_logged_literal_form() {
    assert!(serde_json::from_value::<ValueExpressionAST>(serde_json::json!(5)).is_err());
    assert_eq!(
        serde_json::to_value(lit(5)).unwrap(),
        serde_json::json!({"LITERAL": 5})
    );
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            power: Some(non_literal_x()),
            ..creature("test.cda", 0, 4)
        },
    );
    env.values.push((non_literal_x(), a, Some(5)));
    let e = evaluate(&ContinuousEffects::new(), &env, a).unwrap();
    assert_eq!(e.characteristics.power, Some(lit(5)));
    assert_eq!(e.characteristics.toughness, Some(lit(4)));
    let r = registry(&[(1, eff(Layer::L7b, 1, objs(&[a]), set_pt(Some(2), None)))]);
    let e = evaluate(&r, &env, a).unwrap();
    assert_eq!(e.characteristics.power, Some(lit(2)));

    let b = env.put(
        oid(2),
        Characteristics {
            power: Some(lit(3)),
            toughness: Some(lit(6)),
            ..creature("test.mixed_forms", 0, 0)
        },
    );
    let e = evaluate(&ContinuousEffects::new(), &env, b).unwrap();
    assert_eq!(e.characteristics.power, Some(lit(3)));
    assert_eq!(e.characteristics.toughness, Some(lit(6)));
}
