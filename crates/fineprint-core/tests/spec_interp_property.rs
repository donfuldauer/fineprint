// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property-style spec tests for the interpreter (REQ-DSL-003, REQ-CORE-006, REQ-CORE-001):
//! evaluation is repeatable, selector results never depend on the order the environment lists
//! objects in, and MATH never panics and follows the saturation / rounding / zero rules. Random
//! inputs come from a local integer LCG (R3; no `rand`). Black-box: written from the work-item
//! brief only.

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_core::interp::{InterpError, eval_value, select_objects};
use support::*;

const CASES: u64 = 300;

/// A random battlefield/graveyard population with random keys (ties are likely on purpose).
fn random_env(rng: &mut Lcg) -> Env {
    let mut env = Env::three(Some(rng.pick(&[P1, P2, P3])));
    let n = rng.below(12);
    let mut used = Vec::new();
    for _ in 0..n {
        let id = 1 + rng.below(40);
        if used.contains(&id) {
            continue;
        }
        used.push(id);
        let zone = rng.pick(&[
            Zone::Battlefield,
            Zone::Battlefield,
            Zone::Graveyard,
            Zone::Exile,
        ]);
        let owner = rng.pick(&[P1, P2, P3]);
        let types: &[&str] = rng.pick(&[
            &["Creature"][..],
            &["Land"][..],
            &["Artifact", "Creature"][..],
            &["Instant"][..],
        ]);
        let mut v = view(id, zone, owner, types);
        v.timestamp = ts(rng.below(5));
        v.mana_value = rng.below(4) as i64;
        v.power = rng.pick(&[None, Some(-1), Some(0), Some(2), Some(2)]);
        v.toughness = rng.pick(&[None, Some(1), Some(3)]);
        v.zone_position = rng.below(3) as u32;
        v.tapped = rng.below(2) == 0;
        v.characteristics.colors = colors(rng.below(32) as u8);
        if rng.below(3) == 0 {
            v.counters.insert("charge".into(), rng.below(3) as i64);
        }
        env.add(v);
    }
    env
}

fn random_predicate(rng: &mut Lcg, depth: u32) -> PredicateAST {
    let leaf = |rng: &mut Lcg| match rng.below(9) {
        0 => PredicateAST::IsCreature,
        1 => PredicateAST::IsLand,
        2 => PredicateAST::Tapped,
        3 => PredicateAST::IsMulticolored,
        4 => PredicateAST::HasColor {
            value: name(rng.pick(&["red", "U", "green", "W"])),
        },
        5 => PredicateAST::Power {
            cmp: rng.pick(Cmp::ALL),
            value: blit(rng.below(3) as i64),
        },
        6 => PredicateAST::ControlledBy {
            player: pref(ReferenceAST::EachOpponent),
        },
        7 => PredicateAST::HasCounter {
            kind: "charge".into(),
            compare: None,
        },
        _ => PredicateAST::ManaValue {
            cmp: rng.pick(Cmp::ALL),
            value: blit(rng.below(4) as i64),
        },
    };
    if depth == 0 {
        return leaf(rng);
    }
    match rng.below(4) {
        0 => PredicateAST::And(vec![
            random_predicate(rng, depth - 1),
            random_predicate(rng, depth - 1),
        ]),
        1 => PredicateAST::Or(vec![
            random_predicate(rng, depth - 1),
            random_predicate(rng, depth - 1),
        ]),
        2 => PredicateAST::Not(Box::new(random_predicate(rng, depth - 1))),
        _ => leaf(rng),
    }
}

fn random_selector(rng: &mut Lcg) -> ObjectSelectorAST {
    let zones = match rng.below(3) {
        0 => None,
        1 => Some(vec![Zone::Graveyard, Zone::Battlefield]),
        _ => Some(vec![Zone::Exile]),
    };
    let predicate = if rng.below(4) == 0 {
        None
    } else {
        Some(random_predicate(rng, 2))
    };
    let zones = if zones.is_none() && predicate.is_none() {
        Some(vec![Zone::Battlefield])
    } else {
        zones
    };
    let ordering = if rng.below(3) == 0 {
        None
    } else {
        Some(order(rng.pick(OrderKey::ALL), rng.pick(Direction::ALL)))
    };
    let count = match rng.below(3) {
        0 => None,
        1 => Some(lit(rng.below(6) as i64 - 1)),
        _ => Some(ValueExpressionAST::X),
    };
    filter(zones, predicate, ordering, count)
}

// REQ-DSL-003, REQ-CORE-001: the selector result is independent of the order objects_in returns,
// and evaluating twice gives the same answer.
#[test]
fn req_dsl_003_selector_independent_of_listing_order() {
    let mut rng = Lcg::new(0x57);
    for case in 0..CASES {
        let base = random_env(&mut rng);
        let sel = random_selector(&mut rng);
        let mut c = ctx(rng.pick(&[P1, P2, P3]));
        c.x = Some(rng.below(5) as i64);
        let mut reference = base.clone();
        reference.list_order = ListOrder::Insertion;
        let want = select_objects(&reference, &c, &sel);
        assert_eq!(
            select_objects(&reference, &c, &sel),
            want,
            "case {case}: repeat"
        );
        for lo in [
            ListOrder::Reverse,
            ListOrder::Shuffled(case),
            ListOrder::Shuffled(!case),
        ] {
            let mut e = base.clone();
            e.list_order = lo;
            assert_eq!(
                select_objects(&e, &c, &sel),
                want,
                "case {case} {lo:?} {sel:?}"
            );
        }
        // Without an ordering the result is ObjectId ascending; never any duplicate.
        let got = want.expect("no error").items;
        if let ObjectSelectorAST::Filter(f) = &sel
            && f.ordering.is_none()
        {
            let mut sorted = got.clone();
            sorted.sort();
            assert_eq!(got, sorted, "case {case}: no ordering means id ascending");
        }
        let mut dedup = got.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(dedup.len(), got.len(), "case {case}: no duplicates");
    }
}

// REQ-DSL-003, REQ-CORE-001: value evaluation is repeatable and listing-order independent.
#[test]
fn req_dsl_003_values_repeatable() {
    let mut rng = Lcg::new(0x5EED);
    for case in 0..CASES {
        let base = random_env(&mut rng);
        let sel = random_selector(&mut rng);
        let values = [
            count(sel.clone()),
            distinct(rng.pick(DistinctAttr::ALL), sel.clone()),
            math(
                rng.pick(MathOp::ALL),
                count(sel.clone()),
                lit(rng.below(7) as i64 - 3),
            ),
        ];
        let mut c = ctx(rng.pick(&[P1, P2, P3]));
        c.x = Some(rng.below(5) as i64);
        for v in &values {
            let first = eval_value(&base, &c, v);
            assert_eq!(eval_value(&base, &c, v), first, "case {case}: repeat {v:?}");
            let mut shuffled = base.clone();
            shuffled.list_order = ListOrder::Shuffled(case.wrapping_mul(31));
            assert_eq!(
                eval_value(&shuffled, &c, v),
                first,
                "case {case}: order {v:?}"
            );
        }
    }
}

/// The MATH reference model, computed in 128-bit integers.
fn model(op: MathOp, l: i64, r: i64) -> Result<i64, InterpError> {
    let (a, b) = (i128::from(l), i128::from(r));
    let clamp = |x: i128| -> i64 {
        i64::try_from(x.clamp(i128::from(i64::MIN), i128::from(i64::MAX))).expect("clamped")
    };
    Ok(match op {
        MathOp::Add => clamp(a + b),
        MathOp::Sub => clamp(a - b),
        MathOp::Mul => clamp(a * b),
        MathOp::DivFloor | MathOp::DivCeil => {
            if b == 0 {
                return Err(InterpError::DivisionByZero);
            }
            let q = a / b; // truncates toward zero
            let exact = q * b == a;
            let negative = (a < 0) != (b < 0);
            let rounded = match op {
                MathOp::DivFloor if !exact && negative => q - 1,
                MathOp::DivCeil if !exact && !negative => q + 1,
                _ => q,
            };
            clamp(rounded)
        }
        MathOp::Min => l.min(r),
        MathOp::Max => l.max(r),
    })
}

// REQ-DSL-003, REQ-CORE-006: MATH never panics on extreme operands, for every operator, and
// matches the saturating / floor / ceil / zero model.
#[test]
fn req_dsl_003_req_core_006_math_never_panics_and_matches_model() {
    let env = Env::new();
    let c = ctx(P1);
    let interesting = [
        i64::MIN,
        i64::MIN + 1,
        -7,
        -3,
        -2,
        -1,
        0,
        1,
        2,
        3,
        7,
        i64::MAX - 1,
        i64::MAX,
    ];
    for op in MathOp::ALL.iter().copied() {
        for &l in &interesting {
            for &r in &interesting {
                assert_eq!(
                    eval_value(&env, &c, &math(op, lit(l), lit(r))),
                    model(op, l, r),
                    "{op:?}({l}, {r})"
                );
            }
        }
    }
    // Random small and large operands.
    let mut rng = Lcg::new(0xA11);
    for _ in 0..2000 {
        let op = rng.pick(MathOp::ALL);
        let pick = |rng: &mut Lcg| -> i64 {
            match rng.below(4) {
                0 => rng.pick(&interesting),
                1 => rng.below(21) as i64 - 10,
                _ => rng.next_u64().wrapping_mul(2_654_435_761) as i64,
            }
        };
        let (l, r) = (pick(&mut rng), pick(&mut rng));
        assert_eq!(
            eval_value(&env, &c, &math(op, lit(l), lit(r))),
            model(op, l, r),
            "{op:?}({l}, {r})"
        );
    }
}
