// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the casting procedure, step 6: the total cost and its lock-in (REQ-CAST-001,
//! SRS 7.1 step 6; CR 601.2f, 601.2b, 107.3a, 107.4h, 118.6, 118.6a, 118.7, 118.7a-g, 118.8d,
//! 118.9d).
//!
//! Black-box: written from the work-item brief only, against the pinned public API of
//! `fineprint_core::casting`. Rule entries are built by hand (kind, typed `cost`, `params`).
//! Synthetic costs and ids only (R5).

#[path = "spec_casting_cost_support.rs"]
mod support;

use fineprint_core::casting::*;
use fineprint_core::layers::CostStages;
use fineprint_dsl::ability::{CostAST, RuleKind};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol};
use serde_json::{Value, json};
use support::*;

use SymbolChoice::{First, Life, Second};

fn mcomp(f: impl FnOnce(&mut ManaComponent)) -> ManaComponent {
    let mut m = ManaComponent::default();
    f(&mut m);
    m
}

// ---------------------------------------------------------------------------------------------
// T1: the SRS 7.1 formula in every reduction order (CR 601.2f "in any order")
// ---------------------------------------------------------------------------------------------

/// Runs every permutation of `steps` and checks each against the oracle; then checks that the
/// product's results agree across permutations exactly when the oracle's do. Returns the results.
fn every_order(
    b: &CostBasis,
    st: &CostStages,
    steps: &[ReductionStep],
    ctx: &str,
) -> Vec<ManaPart> {
    let mut product = Vec::new();
    let mut oracle = Vec::new();
    for (i, order) in permutations(steps).iter().enumerate() {
        let got = assert_agrees(b, &[], st, order, &format!("{ctx} perm {i}"))
            .expect("valid order succeeds");
        product.push(*got.mana());
        oracle.push(oracle_total(b, &[], st, order).expect("oracle ok").mana);
    }
    let product_agree = product.windows(2).all(|w| w[0] == w[1]);
    let oracle_agree = oracle.windows(2).all(|w| w[0] == w[1]);
    assert_eq!(
        product_agree, oracle_agree,
        "{ctx}: order dependence differs from the reference model"
    );
    product
}

// REQ-CAST-001, CR 601.2f: three reductions, base {3}{G}{G} (floor reached), all 6 orders.
#[test]
fn req_cast_001_cr_601_2f_three_reductions_every_order_floor_reached() {
    let st = stages(
        vec![],
        vec![red(1, "{2}"), red(2, "{G}{G}"), red(3, "{C}{S}")],
        vec![],
    );
    let steps = steps_in_order(&st);
    let b = basis("{3}{G}{G}");
    let results = every_order(&b, &st, &steps, "{3}{G}{G}");
    assert_eq!(results.len(), 6);
    for r in &results {
        // CR 601.2f: reduced to nothing is {0}, never below, still payable.
        assert_eq!(*r, ManaPart::Payable(ManaComponent::default()));
    }
}

// REQ-CAST-001, CR 601.2f, 118.7b/c: base {5}{R}{G} (floor not reached), all 6 orders; the red
// component, which no reduction names, is untouched.
#[test]
fn req_cast_001_cr_601_2f_three_reductions_every_order_floor_not_reached() {
    let st = stages(
        vec![],
        vec![red(1, "{2}"), red(2, "{G}{G}"), red(3, "{C}{S}")],
        vec![],
    );
    let steps = steps_in_order(&st);
    let b = basis("{5}{R}{G}");
    let results = every_order(&b, &st, &steps, "{5}{R}{G}");
    assert_eq!(results.len(), 6);
    for r in &results {
        let ManaPart::Payable(m) = r else {
            panic!("payable")
        };
        assert_eq!(m.red, 1, "unreduced component untouched");
        assert_eq!(m.green, 0);
        // 5 - 1 (excess G) - 2 - 1 (C absent) - 1 (S) = 0.
        assert_eq!(*m, comp("{R}"));
    }
}

// REQ-CAST-001, CR 601.2f, 118.7a-e, 118.7g: four reductions (one hybrid), 24 orders.
#[test]
fn req_cast_001_cr_601_2f_four_reductions_every_order() {
    let st = stages(
        vec![],
        vec![
            red(11, "{1}"),
            red(12, "{W}{W}"),
            red(13, "{C}{S}{S}"),
            red(14, "{G/U}"),
        ],
        vec![],
    );
    let mut steps = steps_in_order(&st);
    steps[3].hybrid_halves = vec![Second]; // blue half: the cost has no blue, so generic (118.7b)
    let b = basis("{4}{W}{G}{C}{C}{S}");
    let results = every_order(&b, &st, &steps, "four");
    assert_eq!(results.len(), 24);
    for r in &results {
        let ManaPart::Payable(m) = r else {
            panic!("payable")
        };
        assert_eq!(m.green, 1, "green untouched (blue half chosen)");
        assert_eq!(m.snow, 1, "snow never reduced (107.4h, 118.7g)");
        assert_eq!(m.colorless, 1);
        assert_eq!(m.white, 0);
        // 4 - 1 - 1 (excess W) - 2 (S S) - 1 (U absent) = 0, saturating.
        assert_eq!(m.generic, 0);
    }
    // The same reductions with the green half chosen reduce green instead.
    steps[3].hybrid_halves = vec![First];
    let results = every_order(&b, &st, &steps, "four green");
    for r in &results {
        let ManaPart::Payable(m) = r else {
            panic!("payable")
        };
        assert_eq!(m.green, 0);
    }
}

// REQ-CAST-001, CR 601.2f: the oracle itself is checked to cover every permutation exactly once.
#[test]
fn req_cast_001_cr_601_2f_permutation_helper_is_exhaustive() {
    let p = permutations(&[1, 2, 3, 4]);
    assert_eq!(p.len(), 24);
    let set: std::collections::BTreeSet<_> = p.into_iter().collect();
    assert_eq!(set.len(), 24);
}

// ---------------------------------------------------------------------------------------------
// T2: CR 118.7a, 118.7g, 107.4h
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 118.7a: {2} off {1}{W}{C}{S} gives {W}{C}{S}.
#[test]
fn req_cast_001_cr_118_7a_generic_reduction_only_generic() {
    let st = stages(vec![], vec![red(1, "{2}")], vec![]);
    let l = total(&basis("{1}{W}{C}{S}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{W}{C}{S}"));
    assert_eq!(text_of(&l), "{W}{C}{S}");
}

// REQ-CAST-001, CR 107.4h, 118.7g: generic reductions never touch {S}; an {S} reduction reduces
// generic only.
#[test]
fn req_cast_001_cr_107_4h_118_7g_snow() {
    let st = stages(vec![], vec![red(1, "{5}")], vec![]);
    let l = total(&basis("{S}{S}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{S}{S}"));

    let st = stages(vec![], vec![red(1, "{S}{S}")], vec![]);
    let l = total(&basis("{3}{S}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{1}{S}"));
    let l = total(&basis("{S}{S}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{S}{S}"), "snow component never reduced");
}

// ---------------------------------------------------------------------------------------------
// T3: CR 118.7b, 118.7c, 118.7d
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 118.7b: a colour the cost lacks reduces generic.
#[test]
fn req_cast_001_cr_118_7b_absent_colour_reduces_generic() {
    let st = stages(vec![], vec![red(1, "{B}")], vec![]);
    let l = total(&basis("{2}{W}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{1}{W}"));
    let st = stages(vec![], vec![red(1, "{C}")], vec![]);
    let l = total(&basis("{2}{W}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{1}{W}"));
}

// REQ-CAST-001, CR 118.7c: excess coloured reduction zeroes the colour, generic takes the rest.
#[test]
fn req_cast_001_cr_118_7c_excess_colour() {
    let st = stages(vec![], vec![red(1, "{U}{U}{U}")], vec![]);
    let l = total(&basis("{4}{U}{B}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{2}{B}"));
}

// REQ-CAST-001, CR 118.7d: the same for colorless.
#[test]
fn req_cast_001_cr_118_7d_excess_colorless() {
    let st = stages(vec![], vec![red(1, "{C}{C}{C}")], vec![]);
    let l = total(&basis("{3}{C}{R}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{1}{R}"));
}

// ---------------------------------------------------------------------------------------------
// T4: CR 118.7e, 118.7f
// ---------------------------------------------------------------------------------------------

fn one_reduction(
    base: &str,
    reduction: &str,
    halves: &[SymbolChoice],
) -> Result<LockedCost, CostError> {
    let st = stages(vec![], vec![red(1, reduction)], vec![]);
    determine_total_cost(&basis(base), &[], &st, &[step_h(1, halves)])
}

// REQ-CAST-001, CR 118.7e: each hybrid kind with each half.
#[test]
fn req_cast_001_cr_118_7e_hybrid_halves() {
    let base = "{3}{W}{U}{C}";
    let cases: [(&str, SymbolChoice, &str); 8] = [
        ("{W/U}", First, "{3}{U}{C}"),
        ("{W/U}", Second, "{3}{W}{C}"),
        ("{2/W}", First, "{1}{W}{U}{C}"),
        ("{2/W}", Second, "{3}{U}{C}"),
        ("{C/U}", First, "{3}{W}{U}"),
        ("{C/U}", Second, "{3}{W}{C}"),
        ("{W/U/P}", First, "{3}{U}{C}"),
        ("{W/U/P}", Second, "{3}{W}{C}"),
    ];
    for (r, half, want) in cases {
        let l = one_reduction(base, r, &[half]).expect("valid halves");
        assert_eq!(payable(&l), comp(want), "{r} {half:?}");
    }
    // 118.7e + 118.7b: the chosen coloured half the cost lacks reduces generic.
    let l = one_reduction("{2}{G}", "{W/U}", &[First]).expect("ok");
    assert_eq!(payable(&l), comp("{1}{G}"));
    // Several hybrid symbols in one entry consume halves in written order.
    let l = one_reduction("{1}{W}{U}", "{W/B}{G/U}", &[First, Second]).expect("ok");
    assert_eq!(payable(&l), comp("{1}"));
    let l = one_reduction("{1}{W}{U}", "{W/B}{G/U}", &[Second, First]).expect("ok");
    assert_eq!(payable(&l), comp("{W}{U}"), "B and G absent: two generic");
}

// REQ-CAST-001, CR 118.7f: a Phyrexian reduction reduces one mana of its colour (no half).
#[test]
fn req_cast_001_cr_118_7f_phyrexian_reduction() {
    let l = one_reduction("{2}{R}{R}", "{R/P}", &[]).expect("ok");
    assert_eq!(payable(&l), comp("{2}{R}"));
    let l = one_reduction("{2}", "{R/P}", &[]).expect("ok");
    assert_eq!(payable(&l), comp("{1}"));
}

// REQ-CAST-001, CR 118.7e: wrong halves count, Life on a hybrid, extra elements -> BadChoice.
#[test]
fn req_cast_001_cr_118_7e_bad_halves() {
    for (r, halves) in [
        ("{W/U}", vec![]),
        ("{W/U}", vec![First, First]),
        ("{W/U}", vec![Life]),
        ("{2/W}", vec![Life]),
        ("{C/U}", vec![Life]),
        ("{W/U/P}", vec![Life]),
        ("{R/P}", vec![First]),
        ("{2}", vec![Second]),
    ] {
        assert_eq!(
            one_reduction("{3}{W}{U}", r, &halves).err(),
            Some(CostError::BadChoice),
            "{r} {halves:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// T5: CR 601.2f, 118.7 floor
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f, 118.7: reduced past nothing is {0}, payable, never Unpayable.
#[test]
fn req_cast_001_cr_601_2f_floor_is_zero() {
    let st = stages(vec![], vec![red(1, "{9}{G}{C}"), red(2, "{W}")], vec![]);
    let l = total(&basis("{2}{G}"), &[], &st, &[step(2), step(1)]);
    assert_eq!(*l.mana(), ManaPart::Payable(ManaComponent::default()));
    assert!(payable(&l).is_zero());
    assert_eq!(payable(&l).total(), 0);
    assert_eq!(text_of(&l), "{0}");
    assert_eq!(
        payable(&l).to_mana_cost().expect("prints"),
        ManaCostDSL::parse("{0}").expect("parses")
    );
}

// ---------------------------------------------------------------------------------------------
// T6: total-cost effects after the floor (CR 601.2f "Then ... effects that directly affect")
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f: a minimum above the floored cost raises it.
#[test]
fn req_cast_001_cr_601_2f_minimum_after_floor() {
    let st = stages(vec![], vec![red(1, "{4}")], vec![min_entry(2, json!(3))]);
    let l = total(&basis("{2}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{3}"));
    // A minimum below the cost leaves it.
    let st = stages(vec![], vec![], vec![min_entry(2, json!(2))]);
    let l = total(&basis("{1}{W}{U}"), &[], &st, &[]);
    assert_eq!(payable(&l), comp("{1}{W}{U}"));
    // Coloured and snow mana count toward the total; only generic is added.
    let st = stages(vec![], vec![], vec![min_entry(2, json!(4))]);
    let l = total(&basis("{W}{S}"), &[], &st, &[]);
    assert_eq!(payable(&l), comp("{2}{W}{S}"));
}

// REQ-CAST-001, CR 601.2f: effects see the post-reduction value. Before reduction {4} already
// meets the minimum 2 and the reduction would leave {0}; after reduction {0} rises to {2}.
#[test]
fn req_cast_001_cr_601_2f_total_effects_see_post_reduction_value() {
    let st = stages(vec![], vec![red(1, "{4}")], vec![min_entry(2, json!(2))]);
    let l = total(&basis("{4}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{2}"));
    // Set applied before a {R} reduction would give {R}; after, {R}{R} stands.
    let st = stages(vec![], vec![red(1, "{R}")], vec![set_entry(2, "{R}{R}")]);
    let l = total(&basis("{2}{G}"), &[], &st, &[step(1)]);
    assert_eq!(payable(&l), comp("{R}{R}"));
}

// REQ-CAST-001, CR 601.2f, 118.8d: set replaces only the mana component.
#[test]
fn req_cast_001_cr_601_2f_set_keeps_other_and_life() {
    let b = CostBasis::from_mana_cost(
        Some(&mc("{3}{B/P}")),
        None,
        &[Life],
        vec![CostAST::TapSource],
    )
    .expect("basis");
    let st = stages(
        vec![inc(1, vec![pay("{1}"), energy(2)])],
        vec![],
        vec![entry(
            3,
            RuleKind::CostSet,
            Some(vec![pay("{1}"), energy(9), pay("{G}")]),
            json!({}),
        )],
    );
    let l = total(&b, &[], &st, &[]);
    assert_eq!(payable(&l), comp("{1}{G}"));
    assert_eq!(l.phyrexian_life(), 2);
    assert_eq!(l.other(), &[CostAST::TapSource, energy(2)]);
}

// REQ-CAST-001, CR 601.2f: list order of total effects is kept (never re-sorted).
#[test]
fn req_cast_001_cr_601_2f_minimum_and_set_list_order() {
    let free = CostBasis::free(vec![]);
    let min_then_set = stages(
        vec![],
        vec![],
        vec![min_entry(9, json!(3)), set_entry(1, "{G}")],
    );
    let set_then_min = stages(
        vec![],
        vec![],
        vec![set_entry(1, "{G}"), min_entry(9, json!(3))],
    );
    let a = total(&free, &[], &min_then_set, &[]);
    let b = total(&free, &[], &set_then_min, &[]);
    assert_eq!(payable(&a), comp("{G}"));
    assert_eq!(payable(&b), comp("{2}{G}"));
    assert_ne!(a, b);
}

// ---------------------------------------------------------------------------------------------
// T7: increases and additional costs (CR 601.2f, 118.8d)
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f, 118.8d: component-wise sums, `other` order, Phyrexian life.
#[test]
fn req_cast_001_cr_601_2f_increases_and_additional() {
    let b = CostBasis::from_mana_cost(Some(&mc("{1}{W}")), None, &[], vec![CostAST::TapSource])
        .expect("basis");
    let add1 = CostBasis::from_mana_cost(Some(&mc("{B}{R/P}")), None, &[Life], vec![energy(1)])
        .expect("add1");
    let add2 = CostBasis::from_mana_cost(
        Some(&mc("{2}{G/P}")),
        None,
        &[Life],
        vec![CostAST::UntapSource],
    )
    .expect("add2");
    let st = stages(
        vec![
            inc(5, vec![pay("{1}"), energy(5)]),
            inc(4, vec![CostAST::Forage, pay("{S}{C}")]),
        ],
        vec![],
        vec![],
    );
    let l = total(&b, &[add1, add2], &st, &[]);
    assert_eq!(payable(&l), comp("{4}{W}{B}{C}{S}"));
    assert_eq!(l.phyrexian_life(), 4);
    assert_eq!(
        l.other(),
        &[
            CostAST::TapSource,
            energy(1),
            CostAST::UntapSource,
            energy(5),
            CostAST::Forage
        ]
    );
}

// REQ-CAST-001, CR 601.2f: a non-mana-only increase adds to `other` only.
#[test]
fn req_cast_001_cr_601_2f_non_mana_increase() {
    let st = stages(vec![inc(1, vec![CostAST::UntapSource])], vec![], vec![]);
    let l = total(&basis("{1}"), &[], &st, &[]);
    assert_eq!(payable(&l), comp("{1}"));
    assert_eq!(l.other(), &[CostAST::UntapSource]);
}

// REQ-CAST-001, CR 601.2f (S3): entries whose kind does not match their list are ignored.
#[test]
fn req_cast_001_cr_601_2f_mismatched_kinds_ignored() {
    let st = stages(
        vec![
            red(1, "{1}"),
            set_entry(2, "{9}"),
            entry(
                3,
                RuleKind::CostMinimum,
                Some(vec![pay("{3}")]),
                json!({"minimum": 9}),
            ),
        ],
        vec![inc(4, vec![pay("{1}")]), set_entry(5, "{7}")],
        vec![inc(6, vec![pay("{1}"), energy(1)]), red(7, "{2}")],
    );
    let order = [step(5), step(4)];
    let l = total(&basis("{2}{U}"), &[], &st, &order);
    assert_eq!(payable(&l), comp("{2}{U}"));
    assert!(l.other().is_empty());
    // A wrong-kind entry still occupies its slot: leaving it out of the order is an error.
    assert_eq!(
        determine_total_cost(&basis("{2}{U}"), &[], &st, &[step(4)]).err(),
        Some(CostError::BadReductionOrder)
    );
}

// ---------------------------------------------------------------------------------------------
// T8: alternative and unpayable costs (CR 118.6, 118.6a, 118.9d)
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 118.6, 118.6a: no mana cost stays unpayable under increases, additional costs,
// reductions and total effects, while `other` and life still accumulate.
#[test]
fn req_cast_001_cr_118_6a_unpayable_stays_unpayable() {
    let b = CostBasis::from_mana_cost(None, None, &[], vec![CostAST::TapSource]).expect("basis");
    assert_eq!(b.mana, ManaPart::Unpayable);
    assert_eq!(b.phyrexian_life, 0);
    let add = CostBasis::from_mana_cost(Some(&mc("{2}{W/P}")), None, &[Life], vec![energy(3)])
        .expect("add");
    let st = stages(
        vec![inc(1, vec![pay("{3}"), CostAST::Forage])],
        vec![red(2, "{1}")],
        vec![min_entry(3, json!(4)), set_entry(4, "{1}")],
    );
    let l = total(&b, &[add], &st, &[step(2)]);
    assert_eq!(*l.mana(), ManaPart::Unpayable);
    assert_eq!(l.phyrexian_life(), 2);
    assert_eq!(l.other(), &[CostAST::TapSource, energy(3), CostAST::Forage]);
}

// REQ-CAST-001, CR 118.6a, 118.9d: an alternative "free" cost ({0}) takes increases and reductions.
#[test]
fn req_cast_001_cr_118_9d_free_basis_is_zero_base() {
    let free = CostBasis::free(vec![energy(1)]);
    assert_eq!(free.mana, ManaPart::Payable(ManaComponent::default()));
    assert_eq!(free.phyrexian_life, 0);
    let st = stages(
        vec![inc(1, vec![pay("{2}{R}")])],
        vec![red(2, "{1}")],
        vec![],
    );
    let l = total(&free, &[], &st, &[step(2)]);
    assert_eq!(payable(&l), comp("{1}{R}"));
    assert_eq!(l.other(), &[energy(1)]);
    let st = stages(vec![], vec![red(2, "{5}")], vec![]);
    let l = total(&free, &[], &st, &[step(2)]);
    assert_eq!(*l.mana(), ManaPart::Payable(ManaComponent::default()));
    // An alternative cost with mana takes them the same way (118.9d).
    let alt = basis("{2}{U}");
    let st = stages(
        vec![inc(1, vec![pay("{1}")])],
        vec![red(2, "{U}{U}")],
        vec![],
    );
    let l = total(&alt, &[basis("{B}")], &st, &[step(2)]);
    assert_eq!(payable(&l), comp("{2}{B}"));
}

// REQ-CAST-001, CR 118.6a: an additional cost with no mana cost is refused.
#[test]
fn req_cast_001_cr_118_6a_unpayable_additional() {
    let un = CostBasis::from_mana_cost(None, None, &[], vec![]).expect("basis");
    assert_eq!(
        determine_total_cost(
            &basis("{1}"),
            std::slice::from_ref(&un),
            &CostStages::default(),
            &[]
        )
        .err(),
        Some(CostError::UnpayableAdditional)
    );
    assert_eq!(
        determine_total_cost(
            &basis("{1}"),
            &[basis("{2}"), un],
            &CostStages::default(),
            &[]
        )
        .err(),
        Some(CostError::UnpayableAdditional)
    );
}

// ---------------------------------------------------------------------------------------------
// T9: variables and from_mana_cost (CR 107.3a, 601.2b)
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 107.3a: each {X} is the announced value.
#[test]
fn req_cast_001_cr_107_3a_x_values() {
    let b = CostBasis::from_mana_cost(Some(&mc("{X}{X}{G}")), Some(3), &[], vec![]).expect("x=3");
    assert_eq!(b.mana, ManaPart::Payable(comp("{6}{G}")));
    let b = CostBasis::from_mana_cost(Some(&mc("{X}{G}")), Some(0), &[], vec![]).expect("x=0");
    assert_eq!(b.mana, ManaPart::Payable(comp("{G}")));
    let b = CostBasis::from_mana_cost(Some(&mc("{X}")), Some(0), &[], vec![]).expect("x=0");
    assert_eq!(b.mana, ManaPart::Payable(ManaComponent::default()));
    assert_eq!(
        CostBasis::from_mana_cost(Some(&mc("{X}{R}")), None, &[], vec![]).err(),
        Some(CostError::MissingX)
    );
    for t in ["{Y}", "{Z}{1}"] {
        for x in [None, Some(2)] {
            assert_eq!(
                CostBasis::from_mana_cost(Some(&mc(t)), x, &[], vec![]).err(),
                Some(CostError::UnsupportedVariable),
                "{t} {x:?}"
            );
        }
    }
}

// REQ-CAST-001, CR 601.2b: every symbol kind of from_mana_cost and its choices.
#[test]
fn req_cast_001_cr_601_2b_from_mana_cost_symbols() {
    let b = CostBasis::from_mana_cost(Some(&mc("{2}{W}{C}{S}{0}")), None, &[], vec![energy(1)])
        .expect("plain");
    assert_eq!(
        b,
        CostBasis {
            mana: ManaPart::Payable(mcomp(|m| {
                m.generic = 2;
                m.white = 1;
                m.colorless = 1;
                m.snow = 1;
            })),
            phyrexian_life: 0,
            other: vec![energy(1)],
        }
    );
    let cases: Vec<(&str, Vec<SymbolChoice>, &str, u64)> = vec![
        ("{W/U}", vec![First], "{W}", 0),
        ("{W/U}", vec![Second], "{U}", 0),
        ("{2/B}", vec![First], "{2}", 0),
        ("{2/B}", vec![Second], "{B}", 0),
        ("{C/R}", vec![First], "{C}", 0),
        ("{C/R}", vec![Second], "{R}", 0),
        ("{G/P}", vec![First], "{G}", 0),
        ("{G/P}", vec![Life], "{0}", 2),
        ("{R/W/P}", vec![First], "{R}", 0),
        ("{R/W/P}", vec![Second], "{W}", 0),
        ("{R/W/P}", vec![Life], "{0}", 2),
        ("{1}{W/U}{G/P}{U/P}", vec![Second, Life, Life], "{1}{U}", 4),
        ("{W/U}{1}{G/P}", vec![First, First], "{1}{W}{G}", 0),
    ];
    for (t, ch, want, life) in cases {
        let b = CostBasis::from_mana_cost(Some(&mc(t)), None, &ch, vec![]).expect(t);
        assert_eq!(b.mana, ManaPart::Payable(comp(want)), "{t} {ch:?}");
        assert_eq!(b.phyrexian_life, life, "{t} {ch:?}");
    }
    for (t, ch) in [
        ("{W/U}", vec![Life]),
        ("{2/B}", vec![Life]),
        ("{C/R}", vec![Life]),
        ("{G/P}", vec![Second]),
        ("{W/U}", vec![]),
        ("{W/U}", vec![First, First]),
        ("{1}", vec![First]),
    ] {
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mc(t)), None, &ch, vec![]).err(),
            Some(CostError::BadChoice),
            "{t} {ch:?}"
        );
    }
    assert_eq!(
        CostBasis::from_mana_cost(None, None, &[First], vec![]).err(),
        Some(CostError::BadChoice)
    );
}

// REQ-CAST-001, CR 107.3a (R12): a huge X never panics.
#[test]
fn req_cast_001_cr_107_3a_huge_x_no_panic() {
    let r = CostBasis::from_mana_cost(Some(&mc("{X}{X}")), Some(u64::MAX), &[], vec![]);
    assert!(matches!(r, Err(CostError::Overflow)) || r.is_ok(), "{r:?}");
}

// ---------------------------------------------------------------------------------------------
// T10: malformed untrusted entries (R12)
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f (R12): malformed COST_MINIMUM params are ignored.
#[test]
fn req_cast_001_cr_601_2f_malformed_minimum_ignored() {
    let beyond_u64: Value = serde_json::from_str("18446744073709551616000").expect("json");
    let bad = [
        None,
        Some(json!(-1)),
        Some(json!(3.0)),
        Some(json!(2.5)),
        Some(json!("3")),
        Some(json!([3])),
        Some(json!({"n": 3})),
        Some(json!(null)),
        Some(json!(true)),
        Some(beyond_u64),
    ];
    for v in bad {
        let params = match &v {
            Some(v) => json!({ "minimum": v, "test.unrelated": 1 }),
            None => json!({ "test.unrelated": 7 }),
        };
        let st = stages(
            vec![],
            vec![],
            vec![entry(1, RuleKind::CostMinimum, None, params)],
        );
        let l = total(&basis("{1}"), &[], &st, &[]);
        assert_eq!(payable(&l), comp("{1}"), "{v:?}");
    }
}

// REQ-CAST-001, CR 601.2f (R12): a u64::MAX minimum never panics. S3 makes it a valid
// non-negative integer while T10 lists it among malformed values; see docs/DECISIONS.md
// "#53 FP-051 tester - u64::MAX minimum".
#[test]
fn req_cast_001_cr_601_2f_huge_minimum_no_panic() {
    let st = stages(vec![], vec![], vec![min_entry(1, json!(u64::MAX))]);
    match determine_total_cost(&basis("{1}{W}"), &[], &st, &[]) {
        Ok(l) => {
            let m = payable(&l);
            assert!(
                m == comp("{1}{W}") || (m.total() == u64::MAX && m.white == 1),
                "{m:?}"
            );
            if m.generic > MAX_GENERIC {
                assert_eq!(m.to_mana_cost().err(), Some(CostError::Overflow));
            }
        }
        Err(e) => assert_eq!(e, CostError::Overflow),
    }
}

// REQ-CAST-001, CR 601.2f (R12): other malformed entries are ignored without panic.
#[test]
fn req_cast_001_cr_601_2f_malformed_entries_ignored() {
    let b = basis("{3}{G}");
    // COST_SET with no PayMana, with None cost, and with a hybrid symbol.
    for e in [
        entry(
            1,
            RuleKind::CostSet,
            Some(vec![CostAST::TapSource]),
            json!({}),
        ),
        entry(1, RuleKind::CostSet, None, json!({"minimum": 1})),
        set_entry(1, "{W/U}"),
        set_entry(1, "{X}"),
    ] {
        let st = stages(vec![], vec![], vec![e]);
        let l = total(&b, &[], &st, &[]);
        assert_eq!(payable(&l), comp("{3}{G}"));
        assert!(l.other().is_empty());
    }
    // COST_INCREASE with None cost, or a non-plain symbol (whole entry ignored).
    for e in [
        entry(1, RuleKind::CostIncrease, None, json!({"test.k": "v"})),
        inc(1, vec![pay("{X}"), energy(1)]),
        inc(1, vec![pay("{1}"), pay("{W/P}"), CostAST::Forage]),
        inc(1, vec![pay("{2/G}")]),
    ] {
        let st = stages(vec![e], vec![], vec![]);
        let l = total(&b, &[], &st, &[]);
        assert_eq!(payable(&l), comp("{3}{G}"));
        assert!(l.other().is_empty());
    }
    // A reduction with {X} is ignored but keeps its slot, and its halves are not checked.
    let st = stages(
        vec![],
        vec![
            entry(
                1,
                RuleKind::CostReduction,
                Some(vec![pay("{X}{1}{W/U}")]),
                json!({}),
            ),
            red(2, "{1}"),
        ],
        vec![],
    );
    let l = total(&b, &[], &st, &[step_h(1, &[Life, Life]), step(2)]);
    assert_eq!(payable(&l), comp("{2}{G}"));
    assert_eq!(
        determine_total_cost(&b, &[], &st, &[step(2)]).err(),
        Some(CostError::BadReductionOrder)
    );
    // Unrelated params on a reduction do not matter; non-PayMana items are ignored.
    let st = stages(
        vec![],
        vec![entry(
            3,
            RuleKind::CostReduction,
            Some(vec![CostAST::TapSource, pay("{G}")]),
            json!({"minimum": 50, "test.x": [1, 2]}),
        )],
        vec![],
    );
    let l = total(&b, &[], &st, &[step(3)]);
    assert_eq!(payable(&l), comp("{3}"));
    assert!(l.other().is_empty());
    // A wrong-kind entry in the reductions list: ignored, slot kept, halves unchecked.
    let st = stages(
        vec![],
        vec![entry(
            4,
            RuleKind::CostMinimum,
            Some(vec![pay("{W/U}")]),
            json!({"minimum": 9}),
        )],
        vec![],
    );
    let l = total(&b, &[], &st, &[step(4)]);
    assert_eq!(payable(&l), comp("{3}{G}"));
}

// REQ-CAST-001, CR 601.2f (R12): bounded seeded sweep of random bases, stages, orders and halves:
// never panics, Ok results agree with the reference model and print/parse round-trip.
#[test]
fn req_cast_001_cr_601_2f_seeded_sweep_no_panic() {
    let (mut oks, mut errs) = (0u32, 0u32);
    for seed in [0x5eed_0001_u64, 0x5eed_0002, 0x5eed_0003, 0x5eed_0004] {
        let mut g = Gen::new(seed);
        for i in 0..2000 {
            let unpayable = g.chance(1, 8);
            let b = if unpayable {
                CostBasis::from_mana_cost(None, None, &[], vec![g.other_item()]).expect("none")
            } else if g.chance(1, 8) {
                CostBasis::free(vec![])
            } else {
                g.payable_basis()
            };
            let mut additional = Vec::new();
            for _ in 0..g.below(3) {
                if !unpayable && g.chance(1, 20) {
                    additional.push(CostBasis::from_mana_cost(None, None, &[], vec![]).expect("n"));
                } else {
                    additional.push(g.payable_basis());
                }
            }
            let st = random_stages(&mut g);
            let order = random_order(&mut g, &st, !unpayable);
            let ctx = format!("seed {seed:#x} iter {i}");
            let result = assert_agrees(&b, &additional, &st, &order, &ctx);
            if result.is_ok() {
                oks += 1;
            } else {
                errs += 1;
            }
            if let Ok(l) = result
                && let ManaPart::Payable(m) = l.mana()
            {
                let printed = m.to_mana_cost().expect("small values print");
                assert_eq!(Some(printed.to_string()), oracle_text(m), "{ctx}");
                let reparsed = ManaCostDSL::parse(&printed.to_string()).expect("parses");
                assert_eq!(reparsed, printed, "{ctx}");
                let back =
                    CostBasis::from_mana_cost(Some(&reparsed), None, &[], vec![]).expect("plain");
                assert_eq!(back.mana, ManaPart::Payable(*m), "{ctx}");
            }
        }
    }
    // The sweep exercises both successful and refused inputs.
    assert!(oks > 1000 && errs > 200, "oks {oks} errs {errs}");
}

fn random_params(g: &mut Gen) -> Value {
    match g.below(8) {
        0 => json!({}),
        1 => json!({"minimum": -2}),
        2 => json!({"minimum": "4"}),
        3 => json!({"minimum": 1.5}),
        4 => json!({"test.other": g.below(4)}),
        _ => json!({"minimum": g.below(7)}),
    }
}

fn random_cost_items(g: &mut Gen, any_symbols: bool) -> Option<Vec<CostAST>> {
    if g.chance(1, 10) {
        return None;
    }
    let n = 1 + g.below(3);
    Some(
        (0..n)
            .map(|_| {
                if g.chance(1, 4) {
                    g.other_item()
                } else if any_symbols && g.chance(1, 15) {
                    CostAST::PayMana { mana: mc("{X}") }
                } else if any_symbols && g.chance(1, 2) {
                    CostAST::PayMana {
                        mana: g.cost_of(3, Gen::any_fixed_symbol),
                    }
                } else {
                    CostAST::PayMana {
                        mana: g.cost_of(3, Gen::plain_symbol),
                    }
                }
            })
            .collect(),
    )
}

const KINDS: [RuleKind; 5] = [
    RuleKind::CostIncrease,
    RuleKind::CostReduction,
    RuleKind::CostMinimum,
    RuleKind::CostSet,
    RuleKind::AltCostGrant,
];

fn random_stages(g: &mut Gen) -> CostStages {
    let mut next_id = 1u64;
    let mut list = |g: &mut Gen, native: RuleKind, n: u64, any_symbols: bool| {
        (0..n)
            .map(|_| {
                let kind = if g.chance(1, 8) {
                    KINDS[g.below(5) as usize]
                } else {
                    native
                };
                // Wrong-kind entries in the reductions list carry no hybrid symbols (see
                // DECISIONS "#53 FP-051 tester - wrong-kind reductions").
                let any = any_symbols && kind == native;
                let id = next_id;
                next_id += 1;
                entry(id, kind, random_cost_items(g, any), random_params(g))
            })
            .collect::<Vec<_>>()
    };
    let n_inc = g.below(3);
    let increases = list(g, RuleKind::CostIncrease, n_inc, true);
    let n_red = g.below(5);
    let reductions = list(g, RuleKind::CostReduction, n_red, true);
    let n_tot = g.below(3);
    let mut total_effects = list(g, RuleKind::CostMinimum, n_tot, false);
    for e in &mut total_effects {
        if g.chance(1, 2) && e.modification.kind == RuleKind::CostMinimum {
            e.modification.kind = RuleKind::CostSet;
        }
    }
    CostStages {
        grants: Vec::new(),
        increases,
        reductions,
        total_effects,
    }
}

fn random_order(g: &mut Gen, st: &CostStages, may_corrupt: bool) -> Vec<ReductionStep> {
    let mut order: Vec<ReductionStep> = st
        .reductions
        .iter()
        .map(|e| {
            let n = halves_needed(e);
            let malformed_cr =
                e.modification.kind == RuleKind::CostReduction && reduction_symbols(e).is_none();
            let halves = if malformed_cr {
                // ignored entries do not check their halves
                (0..g.below(3)).map(|_| g.any_choice()).collect()
            } else {
                (0..n).map(|_| g.valid_half()).collect()
            };
            ReductionStep {
                id: e.id,
                hybrid_halves: halves,
            }
        })
        .collect();
    g.0.shuffle(&mut order);
    if may_corrupt && g.chance(1, 10) {
        match g.below(5) {
            0 if !order.is_empty() => {
                order.pop();
            }
            1 => order.push(step(900)),
            2 if !order.is_empty() => {
                let first = order[0].clone();
                order.push(first);
            }
            3 if !order.is_empty() => {
                let k = g.below(order.len() as u64) as usize;
                order[k].hybrid_halves.push(g.any_choice());
            }
            _ if !order.is_empty() => {
                let k = g.below(order.len() as u64) as usize;
                if let Some(h) = order[k].hybrid_halves.first_mut() {
                    *h = Life;
                }
            }
            _ => {}
        }
    }
    order
}

// ---------------------------------------------------------------------------------------------
// T11: reduction order validation (S4)
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f: the order must be a permutation of the reduction ids.
#[test]
fn req_cast_001_cr_601_2f_order_validation() {
    let st = stages(
        vec![],
        vec![red(1, "{1}"), red(2, "{1}"), red(3, "{G}")],
        vec![],
    );
    let b = basis("{4}{G}");
    let bad: Vec<Vec<ReductionStep>> = vec![
        vec![step(1), step(2)],                   // missing
        vec![step(1), step(2), step(3), step(4)], // extra
        vec![step(1), step(2), step(2)],          // repeated (same length)
        vec![step(1), step(2), step(3), step(3)], // repeated (longer)
        vec![step(1), step(2), step(9)],          // unknown
        vec![],                                   // empty
    ];
    for order in bad {
        assert_eq!(
            determine_total_cost(&b, &[], &st, &order).err(),
            Some(CostError::BadReductionOrder),
            "{order:?}"
        );
    }
    // A non-empty order with no reductions in the stages.
    assert_eq!(
        determine_total_cost(&b, &[], &CostStages::default(), &[step(1)]).err(),
        Some(CostError::BadReductionOrder)
    );
    // Empty stages and empty order: Ok, cost unchanged.
    let l = total(&b, &[], &CostStages::default(), &[]);
    assert_eq!(payable(&l), comp("{4}{G}"));
    assert_eq!(l.phyrexian_life(), 0);
    assert!(l.other().is_empty());
    // Every valid permutation is accepted.
    for order in permutations(&steps_in_order(&st)) {
        assert_eq!(payable(&total(&b, &[], &st, &order)), comp("{2}"));
    }
}

// ---------------------------------------------------------------------------------------------
// T12: lock-in and determinism (CR 601.2f "locked in")
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f: once locked, later effects do not change the earlier result.
#[test]
fn req_cast_001_cr_601_2f_lock_in() {
    let b = basis("{2}{W}");
    let st = stages(vec![inc(1, vec![pay("{1}")])], vec![red(2, "{W}")], vec![]);
    let first = total(&b, &[], &st, &[step(2)]);
    let snapshot = first.clone();
    let later = stages(
        vec![inc(1, vec![pay("{5}"), CostAST::TapSource])],
        vec![],
        vec![set_entry(3, "{G}{G}")],
    );
    let second = total(&b, &[], &later, &[]);
    assert_ne!(first, second);
    assert_eq!(first, snapshot);
    assert_eq!(payable(&first), comp("{3}"));
    assert!(first.other().is_empty());
}

// REQ-CAST-001, CR 601.2f: same inputs, equal results; additional order changes only `other`.
#[test]
fn req_cast_001_cr_601_2f_determinism_and_additional_order() {
    let b = basis("{1}{U}");
    let adds = vec![
        CostBasis::from_mana_cost(Some(&mc("{W/P}")), None, &[Life], vec![energy(1)]).expect("a"),
        CostBasis::from_mana_cost(Some(&mc("{2}{C}")), None, &[], vec![CostAST::TapSource])
            .expect("b"),
        CostBasis::from_mana_cost(Some(&mc("{B/P}{S}")), None, &[First], vec![CostAST::Forage])
            .expect("c"),
    ];
    let st = stages(
        vec![inc(1, vec![pay("{1}")])],
        vec![red(2, "{U}{B}")],
        vec![],
    );
    let reference = total(&b, &adds, &st, &[step(2)]);
    assert_eq!(reference, total(&b, &adds, &st, &[step(2)]));
    for perm in permutations(&adds) {
        let l = total(&b, &perm, &st, &[step(2)]);
        assert_eq!(l.mana(), reference.mana());
        assert_eq!(l.phyrexian_life(), reference.phyrexian_life());
        let want: Vec<CostAST> = perm.iter().flat_map(|a| a.other.clone()).collect();
        assert_eq!(l.other(), want.as_slice());
    }
    assert_eq!(payable(&reference), comp("{4}{C}{S}"));
    assert_eq!(reference.phyrexian_life(), 2);
}

// ---------------------------------------------------------------------------------------------
// T13: ManaComponent helpers and to_mana_cost
// ---------------------------------------------------------------------------------------------

// REQ-CAST-001, CR 601.2f, 118.7: canonical text, totals and overflow.
#[test]
fn req_cast_001_cr_601_2f_to_mana_cost_canonical() {
    let cases: Vec<(ManaComponent, &str)> = vec![
        (
            mcomp(|m| {
                m.generic = 2;
                m.white = 1;
                m.colorless = 1;
                m.snow = 1;
            }),
            "{2}{W}{C}{S}",
        ),
        (
            mcomp(|m| {
                m.green = 2;
                m.white = 1;
                m.red = 1;
                m.blue = 1;
                m.black = 1;
            }),
            "{W}{U}{B}{R}{G}{G}",
        ),
        (ManaComponent::default(), "{0}"),
        (mcomp(|m| m.snow = 2), "{S}{S}"),
        (mcomp(|m| m.generic = 7), "{7}"),
        (mcomp(|m| m.generic = MAX_GENERIC), "{1000000}"),
    ];
    for (m, want) in cases {
        let printed = m.to_mana_cost().expect("prints");
        assert_eq!(printed.to_string(), want);
        assert_eq!(Some(want.to_owned()), oracle_text(&m));
        assert_eq!(printed, ManaCostDSL::parse(want).expect("parses"));
    }
    assert_eq!(
        mcomp(|m| m.generic = MAX_GENERIC + 1).to_mana_cost().err(),
        Some(CostError::Overflow)
    );
    let m = mcomp(|m| {
        m.generic = 3;
        m.blue = 2;
        m.green = 1;
        m.colorless = 4;
        m.snow = 5;
    });
    assert_eq!(m.total(), 15);
    assert_eq!(m.colored(Color::Blue), 2);
    assert_eq!(m.colored(Color::Green), 1);
    assert_eq!(m.colored(Color::White), 0);
    assert!(!m.is_zero());
    assert!(ManaComponent::default().is_zero());
    assert_eq!(
        mcomp(|m| {
            m.generic = u64::MAX;
            m.snow = 3;
        })
        .total(),
        u64::MAX,
        "saturating"
    );
    for c in Color::ALL {
        let mut m = ManaComponent::default();
        *col_mut(&mut m, c) = 3;
        assert_eq!(m.colored(c), 3);
        assert_eq!(m.total(), 3);
    }
    // Unused symbol import guard: the symbol type is part of the pinned surface.
    let _ = ManaSymbol::Snow;
}

// REQ-CAST-001, CR 601.2f, 118.7e (R12): a COST_REDUCTION with no `cost` member is ignored like a
// COST_INCREASE without one (its halves unchecked, its slot kept); one whose cost has no PayMana
// is well formed, reduces nothing and needs zero halves.
#[test]
fn req_cast_001_cr_118_7e_reduction_without_mana() {
    let b = basis("{3}");
    let st = stages(
        vec![],
        vec![entry(1, RuleKind::CostReduction, None, json!({}))],
        vec![],
    );
    let l = total(&b, &[], &st, &[step_h(1, &[First])]);
    assert_eq!(payable(&l), comp("{3}"));
    assert_eq!(
        determine_total_cost(&b, &[], &st, &[]).err(),
        Some(CostError::BadReductionOrder)
    );
    let st = stages(
        vec![],
        vec![entry(
            1,
            RuleKind::CostReduction,
            Some(vec![CostAST::TapSource]),
            json!({}),
        )],
        vec![],
    );
    assert_eq!(payable(&total(&b, &[], &st, &[step(1)])), comp("{3}"));
    assert_eq!(
        determine_total_cost(&b, &[], &st, &[step_h(1, &[First])]).err(),
        Some(CostError::BadChoice)
    );
}
