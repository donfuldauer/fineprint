// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property and exhaustive-order tests for the casting total cost (REQ-CAST-001, SRS 7.1 step 6;
//! CR 601.2f, 118.7, 118.7a-g, 107.4h).
//!
//! Black-box: written from the work-item brief only, against the pinned public API of
//! `fineprint_core::casting`. Inputs come from `Pcg64Dxsm` with fixed seeds (R3); there are no
//! new dev-dependencies. Synthetic costs and ids only (R5).

#[path = "spec_casting_cost_support.rs"]
mod support;

use fineprint_core::casting::*;
use fineprint_dsl::ability::CostAST;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};
use serde_json::json;
use support::*;

const SEEDS: [u64; 3] = [0xc057_0001, 0xc057_0002, 0xc057_0003];
const ITERATIONS: usize = 600;

/// A random plain-symbol reduction or increase cost text component.
fn plain_cost(g: &mut Gen) -> ManaCostDSL {
    g.cost_of(4, Gen::plain_symbol)
}

/// Random reductions (possibly with hybrid / Phyrexian symbols) and matching valid steps.
fn random_reductions(
    g: &mut Gen,
    first_id: u64,
) -> (Vec<fineprint_core::layers::RuleEntry>, Vec<ReductionStep>) {
    let n = g.below(4);
    let mut entries = Vec::new();
    let mut steps = Vec::new();
    for k in 0..n {
        let id = first_id + k;
        let cost = g.cost_of(3, Gen::any_fixed_symbol);
        let halves: Vec<SymbolChoice> = cost
            .symbols()
            .iter()
            .filter(|s| is_half_symbol(**s))
            .map(|_| g.valid_half())
            .collect();
        entries.push(entry(
            id,
            fineprint_dsl::ability::RuleKind::CostReduction,
            Some(vec![CostAST::PayMana { mana: cost }]),
            json!({}),
        ));
        steps.push(step_h(id, &halves));
    }
    g.0.shuffle(&mut steps);
    (entries, steps)
}

fn random_increases(g: &mut Gen, first_id: u64) -> Vec<fineprint_core::layers::RuleEntry> {
    (0..g.below(3))
        .map(|k| {
            let mut items = vec![CostAST::PayMana {
                mana: plain_cost(g),
            }];
            if g.chance(1, 2) {
                items.push(g.other_item());
            }
            inc(first_id + k, items)
        })
        .collect()
}

fn random_additional(g: &mut Gen) -> Vec<CostBasis> {
    (0..g.below(3)).map(|_| g.payable_basis()).collect()
}

/// base + additional + increases, computed by the product with no reductions and no effects.
fn pre_reduction(
    b: &CostBasis,
    additional: &[CostBasis],
    increases: &[fineprint_core::layers::RuleEntry],
) -> LockedCost {
    let st = stages(increases.to_vec(), vec![], vec![]);
    total(b, additional, &st, &[])
}

// REQ-CAST-001, CR 601.2f (T14 i, ii): with no reductions and no total effects the result is
// exactly base + additional + increases; reductions never raise the total.
#[test]
fn req_cast_001_cr_601_2f_prop_sum_and_never_exceeds() {
    for seed in SEEDS {
        let mut g = Gen::new(seed);
        for i in 0..ITERATIONS {
            let b = g.payable_basis();
            let additional = random_additional(&mut g);
            let increases = random_increases(&mut g, 100);
            let pre = pre_reduction(&b, &additional, &increases);
            // (ii) exact sum, computed independently.
            let ManaPart::Payable(mut want) = b.mana else {
                unreachable!("payable basis")
            };
            let mut life = b.phyrexian_life;
            let mut other = b.other.clone();
            for a in &additional {
                let ManaPart::Payable(am) = a.mana else {
                    unreachable!("payable")
                };
                want = add(&want, &am).expect("small");
                life += a.phyrexian_life;
                other.extend(a.other.iter().cloned());
            }
            for e in &increases {
                for item in e.modification.cost.iter().flatten() {
                    match item {
                        CostAST::PayMana { mana } => {
                            let m = comp(&mana.to_string());
                            want = add(&want, &m).expect("small");
                        }
                        o => other.push(o.clone()),
                    }
                }
            }
            assert_eq!(payable(&pre), want, "seed {seed:#x} iter {i}");
            assert_eq!(pre.phyrexian_life(), life);
            assert_eq!(pre.other(), other.as_slice());
            // (i) reductions never exceed the pre-reduction total.
            let (reds, steps) = random_reductions(&mut g, 200);
            let st = stages(increases.clone(), reds, vec![]);
            let got = assert_agrees(&b, &additional, &st, &steps, &format!("{seed:#x}/{i}"))
                .expect("valid inputs");
            assert!(payable(&got).total() <= payable(&pre).total());
            // (iv) nothing reduces snow (CR 107.4h, 118.7g); colors and colorless only fall.
            let (after, before) = (payable(&got), payable(&pre));
            assert_eq!(after.snow, before.snow, "seed {seed:#x} iter {i}");
            assert!(after.colorless <= before.colorless);
            for c in Color::ALL {
                assert!(after.colored(c) <= before.colored(c));
            }
            assert!(after.generic <= before.generic);
            assert_eq!(got.phyrexian_life(), pre.phyrexian_life());
            assert_eq!(got.other(), pre.other());
        }
    }
}

// REQ-CAST-001, CR 118.7a (T14 iii): a generic-only reduction never changes any non-generic
// component, and lowers generic by exactly min(generic, n).
#[test]
fn req_cast_001_cr_118_7a_prop_generic_only_reduction() {
    for seed in SEEDS {
        let mut g = Gen::new(seed ^ 0xa);
        for i in 0..ITERATIONS {
            let b = g.payable_basis();
            let n = g.below(8) as u32;
            let r = entry(
                1,
                fineprint_dsl::ability::RuleKind::CostReduction,
                Some(vec![CostAST::PayMana {
                    mana: ManaCostDSL::new(vec![ManaSymbol::Generic(n)]).expect("generic"),
                }]),
                json!({}),
            );
            let st = stages(vec![], vec![r], vec![]);
            let got = payable(&total(&b, &[], &st, &[step(1)]));
            let ManaPart::Payable(before) = b.mana else {
                unreachable!()
            };
            let mut want = before;
            want.generic = before.generic.saturating_sub(u64::from(n));
            assert_eq!(got, want, "seed {seed:#x} iter {i}");
        }
    }
}

// REQ-CAST-001, CR 601.2f, 118.7b-d (T14 v): two reductions touching disjoint components agree
// in both orders; and every random reduction set gives the oracle's result in every order.
#[test]
fn req_cast_001_cr_601_2f_prop_orders() {
    // Disjoint pairs: (generic, colorless-present), (white, black), (green, colorless).
    let pairs = [("{2}", "{C}"), ("{W}{W}", "{B}"), ("{G}", "{C}{C}")];
    for seed in SEEDS {
        let mut g = Gen::new(seed ^ 0xb);
        for i in 0..ITERATIONS / 3 {
            // A base that has enough of every named component so nothing spills into generic.
            let extra = plain_cost(&mut g).to_string();
            let b = basis(&format!("{{5}}{{W}}{{W}}{{B}}{{G}}{{C}}{{C}}{{C}}{extra}"));
            for (x, y) in pairs {
                let st = stages(vec![], vec![red(1, x), red(2, y)], vec![]);
                let ab = total(&b, &[], &st, &[step(1), step(2)]);
                let ba = total(&b, &[], &st, &[step(2), step(1)]);
                assert_eq!(ab, ba, "seed {seed:#x} iter {i} {x} {y}");
            }
            // Every order of a random reduction set agrees with the reference model.
            let (reds, steps) = random_reductions(&mut g, 10);
            let st = stages(vec![], reds, vec![]);
            let mut results = Vec::new();
            let mut oracle = Vec::new();
            for order in permutations(&steps) {
                let got =
                    assert_agrees(&b, &[], &st, &order, &format!("{seed:#x}/{i}")).expect("valid");
                results.push(*got.mana());
                oracle.push(oracle_total(&b, &[], &st, &order).expect("ok").mana);
            }
            assert_eq!(
                results.windows(2).all(|w| w[0] == w[1]),
                oracle.windows(2).all(|w| w[0] == w[1])
            );
        }
    }
}

// REQ-CAST-001, CR 601.2f (S7): determinism and the additional-order invariance.
#[test]
fn req_cast_001_cr_601_2f_prop_determinism() {
    for seed in SEEDS {
        let mut g = Gen::new(seed ^ 0xc);
        for i in 0..ITERATIONS / 3 {
            let b = g.payable_basis();
            let additional = random_additional(&mut g);
            let increases = random_increases(&mut g, 100);
            let (reds, steps) = random_reductions(&mut g, 200);
            let effects = if g.chance(1, 2) {
                vec![min_entry(300, json!(g.below(9)))]
            } else {
                vec![]
            };
            let st = stages(increases, reds, effects);
            let a = total(&b, &additional, &st, &steps);
            assert_eq!(
                a,
                total(&b, &additional, &st, &steps),
                "seed {seed:#x} iter {i}"
            );
            let mut rev = additional.clone();
            rev.reverse();
            let r = total(&b, &rev, &st, &steps);
            assert_eq!(r.mana(), a.mana());
            assert_eq!(r.phyrexian_life(), a.phyrexian_life());
        }
    }
}

// REQ-CAST-001, CR 601.2b (S1): random costs with random choices agree with the reference model
// of from_mana_cost (errors included).
#[test]
fn req_cast_001_cr_601_2b_prop_from_mana_cost() {
    for seed in SEEDS {
        let mut g = Gen::new(seed ^ 0xd);
        for i in 0..ITERATIONS {
            let cost = g.cost_of(5, |g| {
                if g.chance(1, 12) {
                    ManaSymbol::Variable(fineprint_dsl::mana::Variable::X)
                } else {
                    g.any_fixed_symbol()
                }
            });
            let has_x = cost
                .symbols()
                .iter()
                .any(|s| matches!(s, ManaSymbol::Variable(_)));
            let x = if has_x && g.chance(3, 4) {
                Some(g.below(5))
            } else {
                None
            };
            let n = cost
                .symbols()
                .iter()
                .filter(|s| is_choice_symbol(**s))
                .count();
            let mut choices: Vec<SymbolChoice> = cost
                .symbols()
                .iter()
                .filter(|s| is_choice_symbol(**s))
                .map(|s| g.valid_choice(*s))
                .collect();
            if g.chance(1, 6) {
                if g.chance(1, 2) || n == 0 {
                    choices.push(g.any_choice());
                } else {
                    let k = g.below(n as u64) as usize;
                    choices[k] = g.any_choice();
                }
            }
            let got = CostBasis::from_mana_cost(Some(&cost), x, &choices, vec![]);
            let want = oracle_basis(Some(&cost), x, &choices, vec![]);
            match (&got, &want) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "seed {seed:#x} iter {i} {cost}"),
                (Err(e), Err(es)) => {
                    assert!(
                        es.contains(e),
                        "seed {seed:#x} iter {i} {cost}: {e:?} not in {es:?}"
                    );
                }
                _ => panic!("seed {seed:#x} iter {i} {cost} {choices:?}: {got:?} vs {want:?}"),
            }
        }
    }
}
