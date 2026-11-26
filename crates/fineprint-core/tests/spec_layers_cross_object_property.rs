// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for REQ-LAYER-001 (#186), REQ-CORE-001 and REQ-LAYER-005: random small boards
//! (3-8 objects, 2 players, random types and controllers, random layer-2/4/6 effects with random
//! timestamps, 0-2 characteristic-defining objects; CR 604.3, 613.1b, 613.4a, 613.7) satisfy
//! the equality of every entry point (B-3), invariance under permutation and duplication of
//! `objects()` and under registration order (B-4), and every CDA value equals a recount by an
//! independent model written here. A bounded no-panic run feeds arbitrary non-literal JSON as
//! base P/T. A seeded SplitMix64 generator (no clock, R3, R7); fixed seeds, bounded cases.

#[path = "spec_layers_cross_object_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{Affected, ContinuousEffect, LayerModification, evaluate, literal_of};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ValueExpressionAST;
use support::*;

const TYPES: [&str; 3] = ["Creature", "Artifact", "Enchantment"];

/// One random board: the env, the effects, and the CDA ids.
struct Case {
    env: Env,
    effects: Vec<(u64, ContinuousEffect)>,
    cdas: Vec<ObjectId>,
}

fn random_subset(rng: &mut Rng, ids: &[ObjectId], exclude: &[ObjectId]) -> Vec<ObjectId> {
    let pool: Vec<ObjectId> = ids
        .iter()
        .copied()
        .filter(|i| !exclude.contains(i))
        .collect();
    if pool.is_empty() {
        return Vec::new();
    }
    let mut out = BTreeSet::new();
    for _ in 0..=rng.below(2) {
        out.insert(pool[usize::try_from(rng.below(u64::try_from(pool.len()).unwrap())).unwrap()]);
    }
    out.into_iter().collect()
}

fn random_case(rng: &mut Rng) -> Case {
    let n = usize::try_from(rng.below(6) + 3).unwrap();
    let mut pool: Vec<u64> = (1..=20).collect();
    rng.shuffle(&mut pool);
    let ids: Vec<ObjectId> = pool[..n].iter().map(|i| o(*i)).collect();
    let n_cda = usize::try_from(rng.below(3)).unwrap();
    let cdas: Vec<ObjectId> = ids[..n_cda].to_vec();
    let mut env = Env::new();
    for (i, id) in ids.iter().enumerate() {
        let ctrl = rng.below(2) + 1;
        let c = if i < n_cda {
            cda_counter()
        } else {
            match rng.below(3) {
                0 => noncreature(TYPES[usize::try_from(rng.below(2) + 1).unwrap()]),
                _ => creature(1, 1),
            }
        };
        env = env.with(*id, ctrl, c);
    }
    let mut effects = Vec::new();
    for k in 0..rng.below(7) {
        let t = rng.below(6) + 1;
        let player = rng.below(2) + 1;
        let effect = match rng.below(6) {
            0 => eff(
                Layer::L2,
                t,
                objs(&random_subset(rng, &ids, &[])),
                set_controller(player),
            ),
            1 => {
                let src = ids[usize::try_from(rng.below(u64::try_from(n).unwrap())).unwrap()];
                let ty = TYPES[usize::try_from(rng.below(3)).unwrap()];
                static_eff(
                    Layer::L2,
                    t,
                    src,
                    sel(has_card_type(ty)),
                    set_controller(player),
                )
            }
            2 => eff(
                Layer::L4,
                t,
                objs(&random_subset(rng, &ids, &[])),
                add_card_type("Creature"),
            ),
            3 => eff(
                Layer::L4,
                t,
                objs(&random_subset(rng, &ids, &cdas)),
                set_card_types("Artifact"),
            ),
            4 => {
                let src = ids[usize::try_from(rng.below(u64::try_from(n).unwrap())).unwrap()];
                static_eff(
                    Layer::L6,
                    t,
                    src,
                    Affected::Selector(any()),
                    add_ability("test.p"),
                )
            }
            _ => eff(
                Layer::L6,
                t,
                objs(&random_subset(rng, &ids, &[])),
                add_ability("test.q"),
            ),
        };
        effects.push((k + 1, effect));
    }
    Case { env, effects, cdas }
}

/// The independent model: controllers after layer 2 and card types after layer 4, applying each
/// layer's effects in (timestamp, EffectId) order (CR 613.7); a layer-2 selector reads the base
/// card types (nothing before layer 2 changes them, CR 613.1).
fn oracle(case: &Case) -> BTreeMap<ObjectId, (u64, BTreeSet<String>)> {
    let mut sorted: Vec<&(u64, ContinuousEffect)> = case.effects.iter().collect();
    sorted.sort_by_key(|(id, e)| (e.timestamp, *id));
    let mut out = BTreeMap::new();
    for (id, base) in &case.env.bases {
        let base_types = base.characteristics.card_types.clone();
        let applies = |e: &ContinuousEffect| match &e.affected {
            Affected::Objects(s) => s.contains(id),
            Affected::Selector(sel) => match sel {
                fineprint_dsl::expr::ObjectSelectorAST::Filter(f) => match f.predicate.as_deref() {
                    None => true,
                    Some(fineprint_dsl::expr::PredicateAST::HasCardType {
                        value: fineprint_dsl::expr::WordRef::Name(t),
                    }) => base_types.contains(t),
                    Some(_) => unreachable!("only type selectors are generated"),
                },
                fineprint_dsl::expr::ObjectSelectorAST::Ref(_) => false,
            },
            Affected::SameAsLayer(_) => false,
        };
        let mut ctrl = base.controller.unwrap();
        let mut types = base_types.clone();
        for (_, e) in &sorted {
            if !applies(e) {
                continue;
            }
            match &e.modification {
                LayerModification::SetController(pl) => ctrl = *pl,
                LayerModification::AddTypes { card_types, .. } => {
                    types.extend(card_types.iter().cloned());
                }
                LayerModification::SetTypes {
                    card_types: Some(ct),
                    ..
                } => types = ct.clone(),
                _ => {}
            }
        }
        let n = if ctrl == p(P1) { P1 } else { P2 };
        out.insert(*id, (n, types));
    }
    out
}

/// REQ-LAYER-001, REQ-LAYER-005, REQ-CORE-001, CR 604.3, 613.1b, 613.4a, 613.7: on random
/// boards every entry point agrees under any `objects()` permutation/duplication and
/// registration order, controllers and card types match the model, and every CDA value equals
/// the model's recount (B-3, B-4, S-1, S-2, S-3).
#[test]
fn req_layer_001_property_cda_matches_model_and_is_order_independent() {
    let mut rng = Rng::new(0x0186_F146);
    for case_no in 0..48 {
        let case = random_case(&mut rng);
        let ctx = format!("case {case_no}");
        let all = check_invariant(&case.effects, &case.env, &ctx);
        // A random registration order gives the same result too.
        let mut shuffled = case.effects.clone();
        rng.shuffle(&mut shuffled);
        let again = check_invariant(&shuffled, &case.env, &format!("{ctx} shuffled"));
        assert_eq!(again, all, "{ctx}: random registration order");

        let model = oracle(&case);
        for (id, (ctrl, types)) in &model {
            assert_eq!(
                all[id].controller,
                Some(p(*ctrl)),
                "{ctx}: controller {id:?}"
            );
            assert_eq!(
                &all[id].characteristics.card_types, types,
                "{ctx}: types {id:?}"
            );
        }
        for cda in &case.cdas {
            let mine = model[cda].0;
            let expected = model
                .values()
                .filter(|(c, t)| *c == mine && t.contains("Creature"))
                .count();
            let expected = i64::try_from(expected).unwrap();
            assert_eq!(
                pt_of(&all[cda]),
                (expected, expected),
                "{ctx}: CDA {cda:?} effects {:?}",
                case.effects
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Bounded no-panic run (R12: untrusted expressions)
// ---------------------------------------------------------------------------------------------

fn random_json(rng: &mut Rng, depth: u32) -> serde_json::Value {
    use serde_json::{Value, json};
    let pick = if depth == 0 {
        rng.below(5)
    } else {
        rng.below(8)
    };
    match pick {
        0 => Value::Null,
        1 => json!(rng.below(2) == 0),
        2 => json!(i64::from_ne_bytes(rng.next_u64().to_ne_bytes())),
        3 => json!(format!("test.s{}", rng.below(100))),
        4 => json!(rng.next_u64()),
        5 => Value::Array(
            (0..rng.below(3))
                .map(|_| random_json(rng, depth - 1))
                .collect(),
        ),
        6 => {
            let keys = ["LITERAL", COUNT_KEY, "TEST_OTHER", ""];
            let mut m = serde_json::Map::new();
            for _ in 0..=rng.below(2) {
                let k = keys[usize::try_from(rng.below(4)).unwrap()];
                m.insert(k.to_owned(), random_json(rng, depth - 1));
            }
            Value::Object(m)
        }
        _ => json!({ "LITERAL": random_json(rng, depth - 1) }),
    }
}

/// REQ-LAYER-001, REQ-CORE-001, CR 604.3, 208.5: arbitrary JSON as base power and toughness,
/// with an environment answering `None`, large, negative or extreme values, never panics; a
/// literal keeps its number, `None` gives 0, `Some(v)` gives `v` (and `v + 1` after a layer-7c
/// +1/+1 effect when that does not overflow).
#[test]
fn req_layer_001_property_arbitrary_value_expressions_never_panic() {
    let mut rng = Rng::new(0x0BAD_0186);
    let answers = [
        None,
        Some(0),
        Some(7),
        Some(-5),
        Some(1_000_000_000_000_000),
        Some(-1_000_000_000_000_000),
        Some(i64::MAX),
        Some(i64::MIN),
    ];
    for case_no in 0..200 {
        // #232 FP-164: base P/T are typed; random JSON that does not decode as a value
        // expression is replaced by a random valid one (a literal or a non-literal MEMORY key).
        let typed = |rng: &mut Rng| {
            let v = random_json(rng, 3);
            serde_json::from_value::<ValueExpressionAST>(v).unwrap_or_else(|_| {
                if rng.below(2) == 0 {
                    ValueExpressionAST::Literal(i64::try_from(rng.below(41)).unwrap() - 20)
                } else {
                    ValueExpressionAST::Memory(format!("test.m{}", rng.below(5)))
                }
            })
        };
        let pw = typed(&mut rng);
        let th = typed(&mut rng);
        let answer = answers[case_no % answers.len()];
        let mut c = creature(0, 0);
        c.power = Some(pw.clone());
        c.toughness = Some(th.clone());
        let mut env = Env::new().with(o(1), P1, c).with(o(2), P2, creature(1, 1));
        env.value_mode = ValueMode::Fixed(answer);
        let expect = |x: &ValueExpressionAST| literal_of(x).unwrap_or(answer.unwrap_or(0));
        let ctx = format!("case {case_no}: {pw:?} {th:?} -> {answer:?}");
        let all = check_invariant(&[], &env, &ctx);
        assert_eq!(pt_of(&all[&o(1)]), (expect(&pw), expect(&th)), "{ctx}");

        let bump = [(1, eff(Layer::L7c, 1, objs(&[o(1)]), modify_pt(1, 1)))];
        let fresh = env.relisted(env.base_ids());
        let ev =
            evaluate(&registry(&bump), &fresh, o(1)).unwrap_or_else(|e| panic!("{ctx}: {e:?}"));
        let got = pt_of(&ev);
        // Far from the i64 bounds the +1/+1 is exact; near them only the absence of a panic is
        // required (overflow handling is not this unit's).
        let moderate = -(i64::MAX / 2)..(i64::MAX / 2);
        if moderate.contains(&expect(&pw)) {
            assert_eq!(got.0, expect(&pw) + 1, "{ctx}");
        }
        if moderate.contains(&expect(&th)) {
            assert_eq!(got.1, expect(&th) + 1, "{ctx}");
        }
    }
}
