// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-001 (#186): the direct path for literal power/toughness (B-2, S-6;
//! CR 604.3 applies only to non-literal values), the equality of every entry point (B-3,
//! REQ-LAYER-005), order independence (B-4, REQ-CORE-001) and the evaluated universe (B-5).
//! Black-box: written from the brief only.

#[path = "spec_layers_cross_object_support.rs"]
mod support;

use fineprint_core::layers::{
    LayerCache, LayerEnvironment, LayerError, evaluate, evaluate_all, evaluate_with_order,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ValueExpressionAST;
use support::*;

/// #232 FP-164: the former bare-JSON-integer literal form no longer exists (refused as a typed
/// value expression), so both helpers give the single literal form `Literal(n)`.
fn json_lit(n: i64) -> ValueExpressionAST {
    assert!(serde_json::from_value::<ValueExpressionAST>(serde_json::json!(n)).is_err());
    ValueExpressionAST::Literal(n)
}

fn tagged_lit(n: i64) -> ValueExpressionAST {
    serde_json::from_value(serde_json::json!({ "LITERAL": n })).expect("valid literal")
}

/// Objects 1-3 listed, 4 not listed; every power and toughness a literal in both forms.
fn literal_env() -> Env {
    let mut a = creature(0, 0);
    a.power = Some(json_lit(3));
    a.toughness = Some(json_lit(4));
    let mut b = creature(0, 0);
    b.power = Some(tagged_lit(5));
    b.toughness = Some(tagged_lit(6));
    let mut c = creature(0, 0);
    c.power = Some(json_lit(7));
    c.toughness = Some(tagged_lit(8));
    let mut d = creature(0, 0);
    d.power = Some(tagged_lit(9));
    d.toughness = Some(json_lit(10));
    Env::new()
        .with(o(1), P1, a)
        .with(o(2), P2, b)
        .with(o(3), P1, c)
        .with(o(4), P2, d)
        .listing(vec![o(1), o(2), o(3)])
}

/// REQ-LAYER-001, CR 604.3: literal P/T (JSON integer and `{"LITERAL": n}`, power and
/// toughness, listed or not) never reaches `eval_value` or `eval_value_with_view` through any
/// entry point (B-2, S-6).
#[test]
fn req_layer_001_s6_literal_pt_never_reaches_eval_value() {
    let env = literal_env();
    let list = [
        (1, eff(Layer::L2, 1, objs(&[o(1)]), set_controller(P2))),
        (
            2,
            eff(Layer::L4, 2, objs(&[o(2)]), add_card_type("Artifact")),
        ),
        (3, eff(Layer::L7c, 3, objs(&[o(3), o(4)]), modify_pt(1, 1))),
    ];
    let all = check_invariant(&list, &env, "s6");
    assert_eq!(pt_of(&all[&o(1)]), (3, 4));
    assert_eq!(pt_of(&all[&o(2)]), (5, 6));
    assert_eq!(pt_of(&all[&o(3)]), (8, 9));
    // The unlisted object evaluates too, still without a value call.
    let effects = registry(&list);
    env.clear_records();
    let d = evaluate(&effects, &env, o(4)).unwrap();
    assert_eq!(pt_of(&d), (10, 11));
    let mut cache = LayerCache::new();
    assert_eq!(cache.evaluate(&effects, &env, o(4)).unwrap(), d);
    assert_eq!(evaluate_with_order(&effects, &env, o(4)).unwrap().0, d);
    evaluate_all(&effects, &env).unwrap();
    assert_eq!(env.value_call_count(), 0, "no value call for literals");
    // check_invariant used fresh copies; recheck on a fresh env through every entry point.
    let fresh = literal_env();
    for id in [o(1), o(2), o(3), o(4)] {
        evaluate(&effects, &fresh, id).unwrap();
        evaluate_with_order(&effects, &fresh, id).unwrap();
        LayerCache::new().evaluate(&effects, &fresh, id).unwrap();
    }
    evaluate_all(&effects, &fresh).unwrap();
    LayerCache::new().evaluate_all(&effects, &fresh).unwrap();
    assert_eq!(fresh.value_call_count(), 0, "no value call for literals");
}

/// REQ-LAYER-001, CR 604.3: with a CDA object present, value calls are made only for the CDA
/// object, never for the literal ones (B-2).
#[test]
fn req_layer_001_b2_value_calls_only_for_the_cda_object() {
    let env = literal_env()
        .with(o(5), P1, cda_counter())
        .listing(vec![o(1), o(2), o(3), o(5)]);
    let effects = registry(&[]);
    let all = evaluate_all(&effects, &env).unwrap();
    // P1 controls 1, 3 and 5.
    assert_eq!(pt_of(&all[&o(5)]), (3, 3));
    for id in [o(1), o(2), o(3), o(4), o(5)] {
        evaluate(&effects, &env, id).unwrap();
    }
    assert!(!env.view_calls.borrow().is_empty());
    assert!(env.plain_calls.borrow().is_empty());
    for call in env.view_calls.borrow().iter() {
        assert_eq!(call.object, o(5));
    }
}

/// REQ-LAYER-001, REQ-LAYER-005, REQ-CORE-001, CR 604.3, 613.4a: every entry point agrees
/// for every object, under reversed, shuffled and duplicated `objects()`, both registration
/// orders and both timestamp orders of two effects; the CDA id lowest and highest (B-3, B-4).
#[test]
fn req_layer_001_b3_b4_entry_points_agree_any_order() {
    for (c, k1, k2, n) in [(1, 2, 3, 4), (9, 2, 3, 4), (3, 1, 9, 2)] {
        let (c, k1, k2, n) = (o(c), o(k1), o(k2), o(n));
        let env = Env::new()
            .with(c, P1, cda_counter())
            .with(k1, P1, creature(1, 1))
            .with(k2, P2, creature(1, 1))
            .with(n, P1, noncreature("Artifact"));
        for (t1, t2) in [(1, 2), (2, 1)] {
            let list = [
                (1, eff(Layer::L2, t1, objs(&[k2]), set_controller(P1))),
                (2, eff(Layer::L4, t2, objs(&[n]), add_card_type("Creature"))),
            ];
            let all = check_invariant(&list, &env, "b3b4");
            assert_eq!(pt_of(&all[&c]).0, 4);
            // Evaluating twice gives the same (no hidden state).
            assert_eq!(check_invariant(&list, &env, "b3b4 again"), all);
        }
        assert_eq!(pt_of(&check_invariant(&[], &env, "b3b4 control")[&c]).0, 2);
    }
}

/// REQ-LAYER-001, CR 604.3: an object with a base that is not listed still evaluates, and the
/// 7a view of it contains it (counted as P1's creature: 2); a listed CDA does not see the
/// unlisted object (B-5, B-1).
#[test]
fn req_layer_001_b5_unlisted_object_with_base_evaluates_and_sees_itself() {
    let (c, k1, k3, x) = (o(2), o(3), o(4), o(1));
    let env = Env::new()
        .with(c, P1, cda_counter())
        .with(k1, P1, creature(1, 1))
        .with(k3, P2, creature(1, 1))
        .with(x, P1, cda_counter())
        .listing(vec![k3, c, k1]);
    let effects = registry(&[]);
    let ev = evaluate(&effects, &env, x).unwrap();
    // x counts c, k1 and itself; it is in its own view.
    assert_eq!(pt_of(&ev), (3, 3));
    {
        // Only the calls for x itself are specified (c's own 7a value is not, B-1).
        let calls = env.view_calls.borrow();
        let mine: Vec<_> = calls.iter().filter(|call| call.object == x).collect();
        assert!(!mine.is_empty());
        for call in mine {
            assert_eq!(call.objects, vec![x, c, k1, k3]);
            assert!(call.snaps.values().all(Option::is_some));
        }
    }
    assert_eq!(LayerCache::new().evaluate(&effects, &env, x).unwrap(), ev);
    env.clear_records();
    let all = evaluate_all(&effects, &env).unwrap();
    assert!(!all.contains_key(&x));
    assert_eq!(pt_of(&all[&c]), (2, 2), "c counts itself and k1, not x");
    for call in env.view_calls.borrow().iter() {
        assert_eq!(call.object, c);
        assert_eq!(call.objects, vec![c, k1, k3]);
    }
}

/// REQ-LAYER-001: an id with no base is `UnknownObject` from `evaluate` and the cache;
/// `evaluate_all` returns the first error in ascending id order when `objects()` lists ids
/// without a base (B-5).
#[test]
fn req_layer_001_b5_unknown_objects_are_errors() {
    let env = Env::new()
        .with(o(2), P1, cda_counter())
        .with(o(3), P1, creature(1, 1));
    let effects = registry(&[]);
    assert_eq!(
        evaluate(&effects, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
    assert_eq!(
        LayerCache::new().evaluate(&effects, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
    let bad = env.relisted(vec![o(9), o(3), o(7), o(2)]);
    assert_eq!(
        evaluate_all(&effects, &bad),
        Err(LayerError::UnknownObject(o(7)))
    );
    assert_eq!(
        LayerCache::new().evaluate_all(&effects, &bad),
        Err(LayerError::UnknownObject(o(7)))
    );
    // Listed objects with a base still evaluate one at a time.
    assert_eq!(pt_of(&evaluate(&effects, &bad, o(2)).unwrap()), (2, 2));
}

/// REQ-LAYER-001, REQ-CORE-001: evaluating never mutates the base (B-5).
#[test]
fn req_layer_001_b5_evaluation_never_mutates_base() {
    let env = Env::new()
        .with(o(1), P1, cda_counter())
        .with(o(2), P1, creature(1, 1))
        .with(o(3), P2, noncreature("Artifact"))
        .counter(o(1), "+1/+1", 2, 1);
    let list = [
        (1, eff(Layer::L2, 1, objs(&[o(1)]), set_controller(P2))),
        (
            2,
            eff(Layer::L4, 2, objs(&[o(3)]), add_card_type("Creature")),
        ),
        (
            3,
            eff(Layer::L6, 3, objs(&[o(2)]), add_ability("test.grant")),
        ),
        (4, eff(Layer::L7c, 4, objs(&[o(1)]), modify_pt(1, 1))),
    ];
    let before: Vec<_> = env.base_ids().iter().map(|id| env.base(*id)).collect();
    let all = check_invariant(&list, &env, "b5 base");
    // P2 controls o(1) and o(3): 2, +2 counters, +1 effect.
    assert_eq!(pt_of(&all[&o(1)]), (5, 5));
    let effects = registry(&list);
    for id in env.base_ids() {
        evaluate(&effects, &env, id).unwrap();
    }
    evaluate_all(&effects, &env).unwrap();
    let after: Vec<_> = env.base_ids().iter().map(|id| env.base(*id)).collect();
    assert_eq!(before, after);
    assert_eq!(
        env.base(o(1)).unwrap().characteristics.power,
        Some(count_expr())
    );
}
