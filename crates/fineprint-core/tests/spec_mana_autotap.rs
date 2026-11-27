// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Example and exhaustive spec tests for the auto-tap solver `preview_auto_tap` (REQ-API-002,
//! REQ-CORE-006; CR 106.4, 106.6, 106.6a, 107.4a, 107.4b, 107.4c, 107.4h, 118.3, 118.3a,
//! 118.3c, 601.2g, 601.2h, 605.3a, 605.3b, 605.3c). Black-box: written from the work-item brief
//! only. Every name is invented and neutral (R5).

#[path = "spec_mana_autotap_support.rs"]
mod support;

use fineprint_core::mana::{
    MAX_SOLUTIONS, MAX_SOURCES, ManaOutput, ManaPool, ManaRestriction, ManaRider, ManaSource,
    ManaUnit, PaymentPlan, SEARCH_BUDGET, plan_payment, preview_auto_tap,
};
use support::*;

const A: &str = "test.ability_a";
const BB: &str = "test.ability_b";

// ---------------------------------------------------------------------------------------------
// Pinned constants
// ---------------------------------------------------------------------------------------------

/// REQ-API-002: the pinned limits.
#[test]
fn req_api_002_pinned_constants() {
    assert_eq!(MAX_SOURCES, 64);
    assert_eq!(MAX_SOLUTIONS, 32);
    assert_eq!(SEARCH_BUDGET, 200_000);
}

// ---------------------------------------------------------------------------------------------
// Requirement 1: produced units and pool_after
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 1): every unit of one output carries the output's restrictions and
/// riders and the entry's source and snow-ness (CR 106.6a, 106.4); `produced` follows activation
/// order then `types` order; `pool_after` appends it after the existing units (CR 605.3b).
#[test]
fn req_api_002_produced_units_and_pool_after() {
    // CR 106.4, CR 106.6a, CR 605.3b
    let r1 = ManaRestriction::only_for(["spell", "test.tag_x"]);
    let r2 = ManaRestriction::only_for(["spell"]);
    let rider = ManaRider::new("test.rider_a");
    let sources = vec![
        ManaSource::new(
            obj(7),
            A,
            vec![
                ManaOutput::new(&[R, G])
                    .restricted(r1.clone())
                    .restricted(r2.clone())
                    .with_rider(rider.clone()),
            ],
        )
        .snow(),
        entry(8, BB, &[W]),
    ];
    let pool = pool_of(&[ManaUnit::new(U, obj(1000))]);
    let cost = mc(0, 1, 1, 0, 1, 1, 0, 0);
    let p = purpose(&["spell"]);
    let sols = solve(&pool, &sources, &cost, &p, MAX_SOLUTIONS);
    assert_eq!(shape(&sols), vec![vec![(0, 0), (1, 0)]]);
    let s = &sols[0];
    let unit = |t| ManaUnit {
        mana_type: t,
        source: obj(7),
        source_is_snow: true,
        restrictions: vec![r1.clone(), r2.clone()],
        riders: vec![rider.clone()],
    };
    let expected = vec![unit(R), unit(G), ManaUnit::new(W, obj(8))];
    assert_eq!(s.produced, expected);
    assert_eq!(s.activations[0].source, obj(7));
    assert_eq!(s.activations[0].ability_id, A);
    assert_eq!(s.activations[1].source, obj(8));
    assert_eq!(s.activations[1].ability_id, BB);
    let after = s.pool_after(&pool);
    assert_eq!(after.len(), 4);
    assert_eq!(
        after.units()[0],
        pool.units()[0],
        "existing index unchanged"
    );
    assert_eq!(&after.units()[1..], expected.as_slice());
    assert_eq!(s.sources_used(), 2);
    // A purpose that does not satisfy both restrictions cannot use the two-restriction units.
    assert!(solve(&pool, &sources, &cost, &purpose(&["test.tag_x"]), 32).is_empty());
}

/// REQ-API-002 (requirement 1): `pool_after` on a different pool appends the same units to it.
#[test]
fn req_api_002_pool_after_appends_to_given_pool() {
    // CR 106.4
    let sources = vec![entry(3, A, &[R])];
    let sols = solve(&ManaPool::default(), &sources, &reds(1), &any_purpose(), 1);
    assert_eq!(sols.len(), 1);
    let other = pool_of(&[ManaUnit::new(G, obj(1)), ManaUnit::new(B, obj(2))]);
    let after = sols[0].pool_after(&other);
    assert_eq!(
        after.units(),
        &[
            ManaUnit::new(G, obj(1)),
            ManaUnit::new(B, obj(2)),
            ManaUnit::new(R, obj(3))
        ]
    );
    assert_eq!(other.len(), 2, "the argument is not changed");
}

// ---------------------------------------------------------------------------------------------
// Requirement 2: candidate rules
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 2): a `{W}`-or-`{U}` entry yields one solution per output, never both
/// outputs at once (one activation produces exactly one alternative, CR 605.3b).
#[test]
fn req_api_002_multi_output_entry_yields_separate_solutions() {
    // CR 605.3b, CR 107.4b
    let sources = vec![entry_alts(5, A, &[&[W], &[U]])];
    let sols = solve(
        &ManaPool::default(),
        &sources,
        &mc(1, 0, 0, 0, 0, 0, 0, 0),
        &any_purpose(),
        32,
    );
    assert_eq!(shape(&sols), vec![vec![(0, 0)], vec![(0, 1)]]);
    // {W}{U} needs both outputs, which one entry cannot give.
    assert!(
        solve(
            &ManaPool::default(),
            &sources,
            &mc(0, 1, 1, 0, 0, 0, 0, 0),
            &any_purpose(),
            32
        )
        .is_empty()
    );
    // Only the matching output pays a colored symbol (CR 107.4a).
    let sols = solve(
        &ManaPool::default(),
        &sources,
        &mc(0, 0, 1, 0, 0, 0, 0, 0),
        &any_purpose(),
        32,
    );
    assert_eq!(shape(&sols), vec![vec![(0, 1)]]);
}

/// REQ-API-002 (requirement 2): two `{T}` entries of one source are never both used (CR 118.3);
/// two `{T}` entries of different sources, or a `{T}` and a non-`{T}` entry of one source, can be.
#[test]
fn req_api_002_tap_rule() {
    // CR 118.3, CR 605.3c
    let p = any_purpose();
    let empty = ManaPool::default();
    // Same source, different abilities, both {T}.
    let same = vec![entry(4, A, &[R]).taps(), entry(4, BB, &[R]).taps()];
    assert!(solve(&empty, &same, &reds(2), &p, 32).is_empty());
    assert_eq!(
        shape(&solve(&empty, &same, &reds(1), &p, 32)),
        vec![vec![(0, 0)], vec![(1, 0)]]
    );
    // Same ability listed twice with {T}.
    let twice_tap = vec![entry(4, A, &[R]).taps(), entry(4, A, &[R]).taps()];
    assert!(solve(&empty, &twice_tap, &reds(2), &p, 32).is_empty());
    // Different sources, both {T}.
    let diff = vec![entry(4, A, &[R]).taps(), entry(5, A, &[R]).taps()];
    assert_eq!(
        shape(&solve(&empty, &diff, &reds(2), &p, 32)),
        vec![vec![(0, 0), (1, 0)]]
    );
    // One {T}, one not, same source.
    let mixed = vec![entry(4, A, &[R]).taps(), entry(4, BB, &[R])];
    assert_eq!(
        shape(&solve(&empty, &mixed, &reds(2), &p, 32)),
        vec![vec![(0, 0), (1, 0)]]
    );
}

/// REQ-API-002 (requirement 2): a non-`{T}` mana ability listed twice is activated once per
/// listing and pays `{R}{R}` (CR 605.3b, 605.3c, 118.3c).
#[test]
fn req_api_002_repeated_listing_pays_twice() {
    // CR 605.3b, CR 605.3c, CR 118.3c
    let sources = vec![entry(3, A, &[R]), entry(3, A, &[R])];
    let sols = solve(&ManaPool::default(), &sources, &reds(2), &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(0, 0), (1, 0)]]);
    assert_eq!(
        sols[0].produced,
        vec![ManaUnit::new(R, obj(3)), ManaUnit::new(R, obj(3))]
    );
    assert_eq!(sols[0].plan.spent, vec![0, 1]);
}

/// REQ-API-002 (requirement 2): entries beyond index 63 are never used.
#[test]
fn req_api_002_entries_beyond_max_sources_ignored() {
    // CR 601.2g
    let mut sources: Vec<ManaSource> = (0..64).map(|i| entry(i + 1, A, &[G])).collect();
    sources.extend((64..70).map(|i| entry(i + 1, A, &[R])));
    let p = any_purpose();
    assert!(solve(&ManaPool::default(), &sources, &reds(1), &p, 32).is_empty());
    let sols = solve(
        &ManaPool::default(),
        &sources,
        &mc(0, 0, 0, 0, 0, 1, 0, 0),
        &p,
        32,
    );
    assert_eq!(sols.len(), 32);
    assert!(
        sols.iter()
            .all(|s| s.activations.iter().all(|a| a.index < 64))
    );
    let sols = solve(
        &ManaPool::default(),
        &sources,
        &mc(1, 0, 0, 0, 0, 0, 0, 0),
        &p,
        32,
    );
    assert!(
        sols.iter()
            .all(|s| s.activations.iter().all(|a| a.index < 64))
    );
}

/// REQ-API-002 (requirement 2): an entry without outputs, or whose output has no types, is never
/// used.
#[test]
fn req_api_002_empty_outputs_never_used() {
    // CR 605.3b
    let sources = vec![
        ManaSource::new(obj(1), A, Vec::new()),
        ManaSource::new(obj(2), A, vec![ManaOutput::new(&[])]),
        entry(3, A, &[R]),
    ];
    let p = any_purpose();
    let sols = solve(&ManaPool::default(), &sources, &reds(1), &p, 32);
    assert_eq!(shape(&sols), vec![vec![(2, 0)]]);
    assert!(solve(&ManaPool::default(), &sources[..2], &reds(1), &p, 32).is_empty());
    // Even for {0} they are not activated.
    let sols = solve(&ManaPool::default(), &sources[..2], &zero(), &p, 32);
    assert_eq!(shape(&sols), vec![Vec::<(usize, usize)>::new()]);
}

// ---------------------------------------------------------------------------------------------
// Requirement 3: validity, no wasted activation
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 3): an activation whose mana the plan would not spend (the pool
/// already pays, lowest index first) is not a solution (CR 601.2h, 118.3a).
#[test]
fn req_api_002_no_wasted_activation() {
    // CR 601.2h, CR 118.3a
    let pool = pool_of(&[ManaUnit::new(R, obj(1000))]);
    let sources = vec![entry(3, A, &[R]), entry(4, A, &[G])];
    let sols = solve(&pool, &sources, &reds(1), &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![Vec::<(usize, usize)>::new()]);
    assert_eq!(sols[0].plan.spent, vec![0]);
    // {1}{R}: the pool pays R, one entry pays the generic.
    let sols = solve(
        &pool,
        &sources,
        &mc(1, 0, 0, 0, 1, 0, 0, 0),
        &any_purpose(),
        32,
    );
    assert_eq!(shape(&sols), vec![vec![(0, 0)], vec![(1, 0)]]);
}

// ---------------------------------------------------------------------------------------------
// Requirement 4: unpayable costs
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 4): unpayable costs give an empty result (CR 601.2h: no partial
/// payment; CR 107.4a, 107.4c, 107.4h, 106.6).
#[test]
fn req_api_002_unpayable_costs_are_empty() {
    // CR 601.2h, CR 107.4a, CR 107.4c, CR 107.4h, CR 106.6
    let empty = ManaPool::default();
    let p = purpose(&["spell"]);
    // Colored shortfall.
    let s1 = vec![entry(1, A, &[R]), entry(2, A, &[G, G])];
    assert!(solve(&empty, &s1, &reds(2), &p, 32).is_empty());
    // {C} with only colored sources.
    let s2 = vec![entry(1, A, &[R]), entry_alts(2, A, &[&[W], &[U, B]])];
    assert!(solve(&empty, &s2, &mc(0, 0, 0, 0, 0, 0, 1, 0), &p, 32).is_empty());
    // {S} with no snow source and no snow pool unit.
    let pool = pool_of(&[ManaUnit::new(C, obj(1000))]);
    assert!(solve(&pool, &s2, &mc(0, 0, 0, 0, 0, 0, 0, 1), &p, 32).is_empty());
    // Only restricted mana whose restriction does not match.
    let rpool =
        pool_of(&[ManaUnit::new(R, obj(1000)).restricted(ManaRestriction::only_for(["ability"]))]);
    let s3 = vec![ManaSource::new(
        obj(1),
        A,
        vec![ManaOutput::new(&[R]).restricted(ManaRestriction::only_for(["ability"]))],
    )];
    assert!(solve(&rpool, &s3, &reds(1), &p, 32).is_empty());
    // Total shortfall.
    assert!(solve(&empty, &s1, &mc(4, 0, 0, 0, 0, 0, 0, 0), &p, 32).is_empty());
    // Empty pool and empty sources with a non-zero cost.
    assert!(solve(&empty, &[], &reds(1), &p, 32).is_empty());
    assert!(solve(&empty, &[], &mc(1, 0, 0, 0, 0, 0, 0, 0), &p, 32).is_empty());
}

/// REQ-API-002 (requirement 4): a component of `u64::MAX` returns (empty), with no panic or hang,
/// for every component.
#[test]
fn req_api_002_huge_cost_component_returns_empty() {
    // CR 601.2h
    let pool = pool_of(&[ManaUnit::new(R, obj(1000)).snow()]);
    let sources: Vec<ManaSource> = (0..20)
        .map(|i| entry_alts(i + 1, A, &[&[R], &[C, W]]).snow())
        .collect();
    let m = u64::MAX;
    for cost in [
        mc(m, 0, 0, 0, 0, 0, 0, 0),
        mc(0, m, 0, 0, 0, 0, 0, 0),
        mc(0, 0, m, 0, 0, 0, 0, 0),
        mc(0, 0, 0, m, 0, 0, 0, 0),
        mc(0, 0, 0, 0, m, 0, 0, 0),
        mc(0, 0, 0, 0, 0, m, 0, 0),
        mc(0, 0, 0, 0, 0, 0, m, 0),
        mc(0, 0, 0, 0, 0, 0, 0, m),
        mc(m, m, m, m, m, m, m, m),
    ] {
        assert!(solve(&pool, &sources, &cost, &any_purpose(), usize::MAX).is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Requirement 5: {0} and pool-only payments
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 5): `{0}` gives exactly one solution with no activation, nothing
/// produced and the empty plan (CR 601.2h).
#[test]
fn req_api_002_zero_cost_single_empty_solution() {
    // CR 601.2h
    let pool = pool_of(&[ManaUnit::new(R, obj(1000))]);
    let sources = vec![entry(1, A, &[R]), entry_alts(2, A, &[&[W], &[U]])];
    for (pl, src) in [
        (ManaPool::default(), Vec::new()),
        (pool.clone(), Vec::new()),
        (pool.clone(), sources.clone()),
        (ManaPool::default(), sources.clone()),
    ] {
        let sols = solve(&pl, &src, &mc(0, 0, 0, 0, 0, 0, 0, 0), &any_purpose(), 32);
        assert_eq!(sols.len(), 1);
        assert!(sols[0].activations.is_empty());
        assert!(sols[0].produced.is_empty());
        assert_eq!(sols[0].plan, PaymentPlan::default());
        assert_eq!(sols[0].pool_after(&pl), pl);
    }
}

/// REQ-API-002 (requirement 5): a cost the pool alone pays gives a zero-activation first solution,
/// followed by solutions that use sources (CR 601.2g, 601.2h).
#[test]
fn req_api_002_pool_only_first() {
    // CR 601.2g, CR 601.2h
    let pool = pool_of(&[ManaUnit::new(R, obj(1000)), ManaUnit::new(G, obj(1001))]);
    let sources = vec![entry(1, A, &[R]), entry(2, A, &[U])];
    let sols = solve(
        &pool,
        &sources,
        &mc(1, 0, 0, 0, 1, 0, 0, 0),
        &any_purpose(),
        32,
    );
    assert!(sols[0].activations.is_empty());
    assert!(sols[0].produced.is_empty());
    assert_eq!(
        Some(sols[0].plan.clone()),
        plan_payment(&pool, &mc(1, 0, 0, 0, 1, 0, 0, 0), &any_purpose())
    );
    assert_eq!(shape(&sols)[0], Vec::<(usize, usize)>::new());
}

// ---------------------------------------------------------------------------------------------
// Requirement 6: rank order
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 6), hand-computed: cost `{R}`, entries 0 = `{R}`-or-`{G}` dual,
/// 1 = basic `{R}`, 2 = `{R}{R}`. All candidates have K1 = 1. Keys: basic (1, 0, 1, [(1,0)]),
/// dual output 0 (1, 0, 2, [(0,0)]), `{R}{R}` (1, 1, 1, [(2,0)]); dual output 1 (G) pays nothing
/// and is not a solution; pairs waste an activation. Order: basic (K3 1 < 2), dual, then
/// `{R}{R}` (K2 = 1 beats its lower K3) (CR 107.4a, 601.2h).
#[test]
fn req_api_002_rank_hand_example() {
    // CR 107.4a, CR 601.2h
    let sources = vec![
        entry_alts(1, A, &[&[R], &[G]]),
        entry(2, A, &[R]),
        entry(3, A, &[R, R]),
    ];
    let pool = ManaPool::default();
    let sols = solve(&pool, &sources, &reds(1), &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(1, 0)], vec![(0, 0)], vec![(2, 0)]]);
    let keys: Vec<RankKey> = sols.iter().map(|s| rank_key(&pool, &sources, s)).collect();
    assert_eq!(
        keys,
        vec![
            (1, 0, 1, vec![(1, 0)]),
            (1, 0, 2, vec![(0, 0)]),
            (1, 1, 1, vec![(2, 0)])
        ]
    );
    assert_eq!(
        sols,
        reference(&pool, &sources, &reds(1), &any_purpose(), 32)
    );
}

/// REQ-API-002 (requirement 6, K1): fewer activations first even when K2, K3 and K4 favour the
/// other: `{R}{R}` by one `{R}{R}{R}` entry at index 2 (K2 = 1) before two basics at 0 and 1.
#[test]
fn req_api_002_rank_k1_first() {
    // CR 601.2h
    let sources = vec![
        entry(1, A, &[R]),
        entry(2, A, &[R]),
        entry(3, A, &[R, R, R]),
    ];
    let pool = ManaPool::default();
    let sols = solve(&pool, &sources, &reds(2), &any_purpose(), 32);
    // [0, 2] and [1, 2] are valid too: the plan spends the lowest indices, so each of their
    // activations contributes; they rank after [0, 1] on K2 (2 > 0).
    assert_eq!(
        shape(&sols),
        vec![
            vec![(2, 0)],
            vec![(0, 0), (1, 0)],
            vec![(0, 0), (2, 0)],
            vec![(1, 0), (2, 0)]
        ]
    );
    let k = |i: usize| rank_key(&pool, &sources, &sols[i]);
    assert_eq!(k(0).1, 1);
    assert_eq!(k(1).1, 0);
    assert_eq!(
        sols,
        reference(&pool, &sources, &reds(2), &any_purpose(), 32)
    );
}

/// REQ-API-002 (requirement 6, K2): less waste first even when K3 and K4 favour the other: a
/// `{R}{R}` entry at index 0 (K2 1, K3 1) after an `{R}`-or-`{G}` entry at index 1 (K2 0, K3 2).
#[test]
fn req_api_002_rank_k2_before_k3() {
    // CR 601.2h
    let sources = vec![entry(1, A, &[R, R]), entry_alts(2, A, &[&[R], &[G]])];
    let pool = ManaPool::default();
    let sols = solve(&pool, &sources, &reds(1), &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(1, 0)], vec![(0, 0)]]);
    assert_eq!(rank_key(&pool, &sources, &sols[0]).2, 2);
    assert_eq!(rank_key(&pool, &sources, &sols[1]).2, 1);
}

/// REQ-API-002 (requirement 6, K3): less flexibility lost first even when K4 favours the other:
/// a three-colour entry at index 0 after a basic at index 1; K3 counts distinct types across all
/// outputs, so `{R}`-or-`{R}` counts 1.
#[test]
fn req_api_002_rank_k3_before_k4() {
    // CR 601.2h
    let sources = vec![
        entry_alts(1, A, &[&[R], &[G, W]]),
        entry(2, A, &[R]),
        entry_alts(3, A, &[&[R], &[R]]),
    ];
    let pool = ManaPool::default();
    let sols = solve(&pool, &sources, &reds(1), &any_purpose(), 32);
    assert_eq!(
        shape(&sols),
        vec![vec![(1, 0)], vec![(2, 0)], vec![(2, 1)], vec![(0, 0)]]
    );
    assert_eq!(rank_key(&pool, &sources, &sols[3]).2, 3);
    assert_eq!(rank_key(&pool, &sources, &sols[1]).2, 1);
}

/// REQ-API-002 (requirement 6, K4): a full K1-K3 tie is broken by the `(index, output)` sequence,
/// lexicographically ascending.
#[test]
fn req_api_002_rank_k4_tie_break() {
    // CR 601.2h
    let p = any_purpose();
    let pool = ManaPool::default();
    let three = vec![entry(1, A, &[R]), entry(2, A, &[R]), entry(3, A, &[R])];
    assert_eq!(
        shape(&solve(&pool, &three, &reds(1), &p, 32)),
        vec![vec![(0, 0)], vec![(1, 0)], vec![(2, 0)]]
    );
    let pairs = vec![
        entry(1, A, &[R]),
        entry(2, A, &[G]),
        entry(3, A, &[R]),
        entry(4, A, &[G]),
    ];
    let sols = solve(&pool, &pairs, &mc(0, 0, 0, 0, 1, 1, 0, 0), &p, 32);
    assert_eq!(
        shape(&sols),
        vec![
            vec![(0, 0), (1, 0)],
            vec![(0, 0), (3, 0)],
            vec![(1, 0), (2, 0)],
            vec![(2, 0), (3, 0)]
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Requirement 7: max_solutions
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 7): 0 gives nothing, 1 the best, above `MAX_SOLUTIONS` at most 32
/// (40 valid single-entry solutions exist), and every smaller limit gives a prefix of every
/// larger one.
#[test]
fn req_api_002_max_solutions() {
    // CR 601.2h
    let sources: Vec<ManaSource> = (0..40).map(|i| entry(i + 1, A, &[R])).collect();
    let pool = ManaPool::default();
    let p = any_purpose();
    assert!(solve(&pool, &sources, &reds(1), &p, 0).is_empty());
    let one = solve(&pool, &sources, &reds(1), &p, 1);
    assert_eq!(shape(&one), vec![vec![(0, 0)]]);
    let many = solve(&pool, &sources, &reds(1), &p, 1000);
    assert_eq!(many.len(), MAX_SOLUTIONS);
    let expected: Vec<Vec<(usize, usize)>> = (0..32).map(|i| vec![(i, 0)]).collect();
    assert_eq!(shape(&many), expected);
    assert_eq!(solve(&pool, &sources, &reds(1), &p, usize::MAX), many);
    let all: Vec<_> = (0..=34)
        .map(|n| preview_auto_tap(&pool, &sources, &reds(1), &p, n))
        .collect();
    for n in 0..all.len() {
        assert_eq!(all[n].len(), n.min(32));
        for m in n + 1..all.len() {
            assert_eq!(
                all[n].as_slice(),
                &all[m][..all[n].len()],
                "prefix {n} of {m}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Requirement 8: restrictions, riders, snow
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 8): a restricted output is usable only for a purpose with an allowed
/// tag; a restriction with no tag allows nothing (CR 106.6).
#[test]
fn req_api_002_restricted_output() {
    // CR 106.6, CR 106.6a
    let restricted = vec![ManaSource::new(
        obj(1),
        A,
        vec![ManaOutput::new(&[R]).restricted(ManaRestriction::only_for(["ability"]))],
    )];
    let pool = ManaPool::default();
    assert!(solve(&pool, &restricted, &reds(1), &purpose(&["spell"]), 32).is_empty());
    assert!(solve(&pool, &restricted, &reds(1), &any_purpose(), 32).is_empty());
    let sols = solve(
        &pool,
        &restricted,
        &reds(1),
        &purpose(&["spell", "ability"]),
        32,
    );
    assert_eq!(shape(&sols), vec![vec![(0, 0)]]);
    assert_eq!(
        sols[0].produced[0].restrictions,
        vec![ManaRestriction::only_for(["ability"])]
    );
    let nothing = vec![ManaSource::new(
        obj(1),
        A,
        vec![ManaOutput::new(&[R]).restricted(ManaRestriction::only_for(Vec::<&str>::new()))],
    )];
    for t in [&[][..], &["spell"][..], &TAGS[..]] {
        assert!(solve(&pool, &nothing, &reds(1), &purpose(t), 32).is_empty());
    }
    // A restricted output next to an unrestricted one: the solver picks the usable output.
    let alts = vec![ManaSource::new(
        obj(1),
        A,
        vec![
            ManaOutput::new(&[R]).restricted(ManaRestriction::only_for(["ability"])),
            ManaOutput::new(&[R]),
        ],
    )];
    assert_eq!(
        shape(&solve(&pool, &alts, &reds(1), &purpose(&["spell"]), 32)),
        vec![vec![(0, 1)]]
    );
}

/// REQ-API-002 (requirement 8): a restricted pooled unit is used when allowed and never when not
/// (CR 106.6).
#[test]
fn req_api_002_restricted_pool_unit() {
    // CR 106.6
    let pool =
        pool_of(&[ManaUnit::new(R, obj(1000)).restricted(ManaRestriction::only_for(["ability"]))]);
    let sources = vec![entry(1, A, &[R])];
    let sols = solve(&pool, &sources, &reds(1), &purpose(&["spell"]), 32);
    assert_eq!(shape(&sols), vec![vec![(0, 0)]]);
    assert_eq!(
        sols[0].plan.spent,
        vec![1],
        "the restricted unit is not spent"
    );
    let sols = solve(&pool, &sources, &reds(1), &purpose(&["ability"]), 32);
    assert_eq!(shape(&sols)[0], Vec::<(usize, usize)>::new());
    assert_eq!(sols[0].plan.spent, vec![0]);
    // Most restrictions first: an allowed restricted produced unit is preferred over a plain
    // pooled one, so that activation is a solution and not wasted.
    let plain = pool_of(&[ManaUnit::new(R, obj(1000))]);
    let rs = vec![ManaSource::new(
        obj(1),
        A,
        vec![ManaOutput::new(&[R]).restricted(ManaRestriction::only_for(["spell"]))],
    )];
    let sols = solve(&plain, &rs, &reds(1), &purpose(&["spell"]), 32);
    assert_eq!(shape(&sols), vec![vec![], vec![(0, 0)]]);
    assert_eq!(sols[1].plan.spent, vec![1]);
}

/// REQ-API-002 (requirement 8): riders of spent produced and pooled units appear in
/// `plan.riders_fired` exactly as `plan_payment` reports them (CR 106.6, 106.6a).
#[test]
fn req_api_002_riders_fired() {
    // CR 106.6, CR 106.6a
    let rp = ManaRider::new("test.rider_a");
    let ro = ManaRider::new("test.rider_b");
    let pool = pool_of(&[ManaUnit::new(G, obj(1000)).with_rider(rp.clone())]);
    let sources = vec![ManaSource::new(
        obj(1),
        A,
        vec![ManaOutput::new(&[R, R]).with_rider(ro.clone())],
    )];
    let cost = mc(0, 0, 0, 0, 2, 1, 0, 0);
    let sols = solve(&pool, &sources, &cost, &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(0, 0)]]);
    let s = &sols[0];
    assert_eq!(
        s.plan.riders_fired,
        vec![
            (0, vec![rp.clone()]),
            (1, vec![ro.clone()]),
            (2, vec![ro.clone()])
        ]
    );
    assert_eq!(
        Some(s.plan.clone()),
        plan_payment(&s.pool_after(&pool), &cost, &any_purpose())
    );
}

/// REQ-API-002 (requirement 8): `{S}` is paid only through snow entries or snow pool units
/// (CR 107.4h).
#[test]
fn req_api_002_snow() {
    // CR 107.4h
    let s_cost = mc(0, 0, 0, 0, 0, 0, 0, 1);
    let empty = ManaPool::default();
    let plain = vec![entry(1, A, &[R])];
    assert!(solve(&empty, &plain, &s_cost, &any_purpose(), 32).is_empty());
    let snowy = vec![entry(1, A, &[R]), entry(2, A, &[G]).snow()];
    let sols = solve(&empty, &snowy, &s_cost, &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(1, 0)]]);
    assert!(sols[0].produced[0].source_is_snow);
    let spool = pool_of(&[ManaUnit::new(C, obj(1000)).snow()]);
    let sols = solve(&spool, &plain, &s_cost, &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![Vec::<(usize, usize)>::new()]);
    // {S}{S}: one snow unit in the pool plus the snow entry.
    let sols = solve(
        &spool,
        &snowy,
        &mc(0, 0, 0, 0, 0, 0, 0, 2),
        &any_purpose(),
        32,
    );
    assert_eq!(shape(&sols), vec![vec![(1, 0)]]);
}

// ---------------------------------------------------------------------------------------------
// Requirement 9: determinism and purity
// ---------------------------------------------------------------------------------------------

/// REQ-API-002, REQ-CORE-006 (requirement 9): the same call gives the same output; the inputs are
/// unchanged; empty slices, huge ids, more than `MAX_SOURCES` entries and an empty restriction do
/// not panic.
#[test]
fn req_api_002_req_core_006_determinism_and_edges() {
    // CR 601.2g, CR 106.6
    let pool = pool_of(&[
        ManaUnit::new(W, obj(u64::MAX)).snow(),
        ManaUnit::new(R, obj(u64::MAX - 1))
            .restricted(ManaRestriction::only_for(Vec::<&str>::new())),
    ]);
    let mut sources = vec![
        entry_alts(u64::MAX, "", &[&[R], &[W, U]]).taps(),
        entry(u64::MAX, &"x".repeat(5000), &[R]).taps(),
        entry(0, A, &[G]).snow(),
    ];
    sources.extend((0..80).map(|i| entry(i, BB, &[C])));
    let cost = mc(1, 1, 0, 0, 1, 0, 0, 1);
    let p = purpose(&TAGS);
    let a = solve(&pool, &sources, &cost, &p, 32);
    let b = solve(&pool, &sources, &cost, &p, 32);
    assert_eq!(a, b);
    assert!(!a.is_empty());
    assert!(solve(&ManaPool::default(), &[], &cost, &p, 32).is_empty());
    assert_eq!(solve(&ManaPool::default(), &[], &zero(), &p, 32).len(), 1);
}

/// REQ-API-002, REQ-CORE-006 (requirement 9): 200 identical entries with a large cost return
/// only valid solutions (the call may reach `SEARCH_BUDGET`); an unreachable cost is empty.
#[test]
fn req_api_002_req_core_006_many_identical_entries() {
    // CR 601.2h, CR 605.3b
    let sources: Vec<ManaSource> = (0..200).map(|_| entry(9, A, &[R])).collect();
    let pool = ManaPool::default();
    let p = any_purpose();
    let sols = solve(&pool, &sources, &reds(30), &p, 32);
    for s in &sols {
        assert_eq!(s.activations.len(), 30);
    }
    assert_eq!(sols, solve(&pool, &sources, &reds(30), &p, 32));
    // Only 64 entries are considered: 65 red is unreachable.
    assert!(solve(&pool, &sources, &reds(65), &p, 32).is_empty());
    assert!(solve(&pool, &sources, &mc(1000, 0, 0, 0, 0, 0, 0, 0), &p, 32).is_empty());
    // A mixed large cost over many two-output entries.
    let alts: Vec<ManaSource> = (0..200)
        .map(|i| entry_alts(i % 7, A, &[&[R], &[G, G]]))
        .collect();
    let cost = mc(10, 0, 0, 0, 10, 10, 0, 0);
    let a = solve(&pool, &alts, &cost, &p, 32);
    assert_eq!(a, solve(&pool, &alts, &cost, &p, 32));
}

// ---------------------------------------------------------------------------------------------
// Exhaustive comparison with the reference (requirement 10)
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 10), exhaustive: a fixed 6-entry set (a `{W}`-or-`{U}` dual, a basic
/// `{W}`, an `{R}{R}` `{T}` entry and an `{R}` `{T}` entry sharing its source, a snow `{C}`, an
/// `{G}` restricted to "spell" with a rider) over every cost with generic 0-2, `{W}`, `{U}` 0-1,
/// `{R}` 0-2, `{G}`, `{C}`, `{S}` 0-1, three pools, two purposes and four limits equals the
/// reference exactly (CR 107.4a-c, 107.4h, 106.6, 118.3, 601.2h).
#[test]
fn req_api_002_exhaustive_matches_reference() {
    // CR 107.4a, CR 107.4b, CR 107.4c, CR 107.4h, CR 106.6, CR 118.3, CR 601.2h
    let sources = vec![
        entry_alts(1, A, &[&[W], &[U]]),
        entry(2, A, &[W]),
        entry(3, A, &[R, R]).taps(),
        entry(3, BB, &[R]).taps(),
        entry(4, A, &[C]).snow(),
        ManaSource::new(
            obj(5),
            A,
            vec![
                ManaOutput::new(&[G])
                    .restricted(ManaRestriction::only_for(["spell"]))
                    .with_rider(ManaRider::new("test.rider_a")),
            ],
        ),
    ];
    let pools = [
        ManaPool::default(),
        pool_of(&[ManaUnit::new(R, obj(1000))]),
        pool_of(&[
            ManaUnit::new(G, obj(1000)).snow(),
            ManaUnit::new(W, obj(1001)).restricted(ManaRestriction::only_for(["ability"])),
        ]),
    ];
    let purposes = [purpose(&["spell"]), purpose(&["ability"])];
    let mut checked = 0usize;
    let mut nonempty = 0usize;
    for g in 0..=2 {
        for w in 0..=1 {
            for u in 0..=1 {
                for r in 0..=2 {
                    for gr in 0..=1 {
                        for c in 0..=1 {
                            for s in 0..=1 {
                                let cost = mc(g, w, u, 0, r, gr, c, s);
                                for pool in &pools {
                                    for p in &purposes {
                                        let full = reference(pool, &sources, &cost, p, 32);
                                        for max in [32, 3, 1, 0] {
                                            let got =
                                                preview_auto_tap(pool, &sources, &cost, p, max);
                                            let want: Vec<_> =
                                                full.iter().take(max).cloned().collect();
                                            assert_eq!(
                                                got, want,
                                                "cost {cost:?} pool {pool:?} purpose {p:?} max {max}"
                                            );
                                        }
                                        checked += 1;
                                        nonempty += usize::from(!full.is_empty());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3 * 2 * 2 * 3 * 2 * 2 * 2 * 3 * 2);
    assert!(
        nonempty > 100,
        "the domain exercises payable costs: {nonempty}"
    );
}

/// REQ-API-002: `MAX_SOURCES` is the boundary: entry 63 is used, entry 64 is not.
#[test]
fn req_api_002_max_sources_boundary() {
    // CR 601.2g
    let mut sources: Vec<ManaSource> = (0..63).map(|i| entry(i + 1, A, &[G])).collect();
    sources.push(entry(100, A, &[R]));
    sources.push(entry(101, A, &[U]));
    let pool = ManaPool::default();
    let sols = solve(&pool, &sources, &reds(1), &any_purpose(), 32);
    assert_eq!(shape(&sols), vec![vec![(63, 0)]]);
    assert!(
        solve(
            &pool,
            &sources,
            &mc(0, 0, 1, 0, 0, 0, 0, 0),
            &any_purpose(),
            32
        )
        .is_empty()
    );
}
