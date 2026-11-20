// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for pass B of the post-layer passes, the effects that modify the rules
//! (REQ-LAYER-004, REQ-DSL-008, REQ-CORE-001; CR 613.11, 613.7, 613.8b, 611.2c, 101.2, 305.2,
//! 305.2b, 402.2, 601.2f): grouping by kind, prohibition over permission, extra land plays,
//! maximum hand size and the cost hook. Black-box: written from the work-item brief only.

#[path = "spec_layers_post_support.rs"]
mod support;

use fineprint_base::ids::PlayerId;
use fineprint_core::layers::{
    EffectOrigin, LayerError, PostLayerEffects, RuleEffect, Verdict, build_rule_set,
};
use fineprint_dsl::ability::{RuleKind, RuleModification};
use support::*;

/// A rule effect for `P1`'s side with `players`.
fn re(t: u64, players: &[PlayerId], m: RuleModification) -> RuleEffect {
    rule_effect(t, fixed(players), m)
}

// ---------------------------------------------------------------------------------------------
// 4. grouping
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004, REQ-DSL-008: `entries(kind)` returns only entries of that kind, in
/// application order (timestamp, then EffectId), with the entry's fields; unrelated kinds do
/// not leak and an unused kind is empty (CR 613.11: "applied in timestamp order").
#[test]
fn entries_grouped_by_kind_in_application_order() {
    // CR 613.11, CR 613.7
    let items = vec![
        (1, re(5, &[P1], rmod_action(RuleKind::Prohibition, "CAST"))),
        (
            2,
            re(2, &[P1, P2], rmod_action(RuleKind::CastPermission, "CAST")),
        ),
        (
            3,
            re(3, &[P2], rmod_int(RuleKind::AdditionalLandPlay, "count", 1)),
        ),
        (
            4,
            re(1, &[P1], rmod_action(RuleKind::Prohibition, "PLAY_LAND")),
        ),
        (5, re(4, &[P1], rmod(RuleKind::Skip))),
        (
            6,
            re(1, &[P2], rmod_action(RuleKind::CastPermission, "CAST")),
        ),
        (
            7,
            re(5, &[P2], rmod_action(RuleKind::Prohibition, "ACTIVATE")),
        ),
    ];
    let env = Env::two();
    let rs = build_rule_set(&rules_registry(items), &env);
    assert_eq!(entry_ids(&rs, RuleKind::Prohibition), eids(&[4, 1, 7]));
    assert_eq!(entry_ids(&rs, RuleKind::CastPermission), eids(&[6, 2]));
    assert_eq!(entry_ids(&rs, RuleKind::AdditionalLandPlay), eids(&[3]));
    assert_eq!(entry_ids(&rs, RuleKind::Skip), eids(&[5]));
    for kind in RuleKind::ALL {
        for e in rs.entries(*kind) {
            assert_eq!(e.modification.kind, *kind);
        }
    }
    assert!(rs.entries(RuleKind::MaxHandSize).is_empty());
    assert!(rs.entries(RuleKind::PlayPermission).is_empty());
    assert!(rs.entries(RuleKind::CostSet).is_empty());

    // Fields of one entry.
    let e = &rs.entries(RuleKind::CastPermission)[1];
    assert_eq!(e.id, eid(2));
    assert_eq!(e.players, set(&[P1, P2]));
    assert_eq!(e.controller, P1);
    assert_eq!(e.source, Some(oid(100)));
    assert_eq!(e.timestamp, ts(2));
    assert_eq!(
        e.modification,
        rmod_action(RuleKind::CastPermission, "CAST")
    );
}

/// REQ-LAYER-004: a Selector rule effect resolves to the players the environment matches
/// (CR 613.11).
#[test]
fn entries_resolve_selectors_to_players() {
    // CR 613.11
    let mut opp = rule_effect(1, sel(opponents()), rmod(RuleKind::Skip));
    opp.controller = P2;
    let items = vec![
        (1, opp),
        (2, rule_effect(2, sel(you()), rmod(RuleKind::Skip))),
        (3, rule_effect(3, sel(all_players()), rmod(RuleKind::Skip))),
    ];
    let rs = build_rule_set(&rules_registry(items), &Env::new(&[P1, P2, P3]));
    let es = rs.entries(RuleKind::Skip);
    assert_eq!(ids(es), eids(&[1, 2, 3]));
    assert_eq!(es[0].players, set(&[P1, P3]));
    assert_eq!(es[0].controller, P2);
    assert_eq!(es[1].players, set(&[P1]));
    assert_eq!(es[2].players, set(&[P1, P2, P3]));
}

/// REQ-LAYER-004: a resolved rule effect with a selector is accepted and applies to whatever
/// the selector matches when the rule set is built, so it can affect players it did not
/// affect when it began (CR 611.2c; #35 D4).
#[test]
fn resolved_rule_effect_selects_at_build_time() {
    // CR 611.2c
    let mut e = rule_effect(
        1,
        sel(all_players()),
        rmod_int(RuleKind::MaxHandSize, "set", 2),
    );
    e.origin = EffectOrigin::Resolved;
    let reg = rules_registry(vec![(1, e)]);
    let two = build_rule_set(&reg, &Env::two());
    assert_eq!(two.max_hand_size(P2), Some(2));
    assert_eq!(two.max_hand_size(P3), Some(7));
    let three = build_rule_set(&reg, &Env::new(&[P1, P2, P3]));
    assert_eq!(three.max_hand_size(P3), Some(2));
    assert_eq!(
        three.entries(RuleKind::MaxHandSize)[0].players,
        set(&[P1, P2, P3])
    );
}

/// REQ-LAYER-004: `effect_exists` false drops a rule effect (CR 613.11).
#[test]
fn rule_effect_that_does_not_exist_is_dropped() {
    // CR 613.11
    let items = vec![
        (
            1,
            re(1, &[P1], rmod_action(RuleKind::Prohibition, "PLAY_LAND")),
        ),
        (2, re(2, &[P1], rmod_int(RuleKind::MaxHandSize, "set", 3))),
    ];
    let reg = rules_registry(items);
    let rs = build_rule_set(&reg, &Env::two().without(1).without(2));
    assert!(rs.entries(RuleKind::Prohibition).is_empty());
    assert_eq!(rs.land_plays_allowed(P1), 1);
    assert_eq!(rs.max_hand_size(P1), Some(7));
    assert_eq!(
        rs.query(P1, "PLAY_LAND", RuleKind::PlayPermission),
        Verdict::NoEffect
    );
}

/// REQ-LAYER-004: the dependency hook orders pass B too: a dependent waits for its base
/// (CR 613.8b), and a 2-cycle applies in timestamp order.
#[test]
fn pass_b_dependency_orders_entries() {
    // CR 613.8a, CR 613.8b
    let items = || {
        vec![
            (1, re(1, &[P1], rmod_int(RuleKind::MaxHandSize, "set", 4))),
            (2, re(2, &[P1], rmod_int(RuleKind::MaxHandSize, "delta", 1))),
            (3, re(3, &[P1], rmod_int(RuleKind::MaxHandSize, "set", 9))),
        ]
    };
    // 1 (set 4) depends on 3 (set 9). Free initially: 2 (ts 2) and 3 (ts 3): 2, then 3, then 1.
    let rs = build_rule_set(&rules_registry(items()), &Env::two().depends(1, 3));
    assert_eq!(entry_ids(&rs, RuleKind::MaxHandSize), eids(&[2, 3, 1]));
    // 7 + 1 = 8, set 9, set 4.
    assert_eq!(rs.max_hand_size(P1), Some(4));

    // 2-cycle between 1 and 2: 3 depends on nothing and applies first; then the loop applies
    // in timestamp order, 1 then 2 (brief: the earliest-key effect applies only when none is
    // free).
    let rs = build_rule_set(
        &rules_registry(items()),
        &Env::two().depends(1, 2).depends(2, 1),
    );
    assert_eq!(entry_ids(&rs, RuleKind::MaxHandSize), eids(&[3, 1, 2]));
    // set 9, set 4, +1.
    assert_eq!(rs.max_hand_size(P1), Some(5));
}

// ---------------------------------------------------------------------------------------------
// prohibition versus permission (deterministic cases; the property test is separate)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: a prohibition beats a permission for the same action regardless of
/// timestamps; a permission alone permits; nothing gives NoEffect (CR 101.2).
#[test]
fn query_prohibition_beats_permission() {
    // CR 101.2
    for (perm_t, prohib_t) in [(1, 2), (2, 1)] {
        let items = vec![
            (
                1,
                re(
                    perm_t,
                    &[P1, P2],
                    rmod_action(RuleKind::CastPermission, "CAST"),
                ),
            ),
            (
                2,
                re(prohib_t, &[P1], rmod_action(RuleKind::Prohibition, "CAST")),
            ),
        ];
        let rs = build_rule_set(&rules_registry(items), &Env::two());
        for kind in [
            RuleKind::CastPermission,
            RuleKind::PlayPermission,
            RuleKind::TimingPermission,
        ] {
            assert_eq!(
                rs.query(P1, "CAST", kind),
                Verdict::Prohibited { by: eid(2) }
            );
        }
        assert_eq!(
            rs.query(P2, "CAST", RuleKind::CastPermission),
            Verdict::Permitted { by: eid(1) }
        );
        assert_eq!(
            rs.query(P2, "CAST", RuleKind::PlayPermission),
            Verdict::NoEffect
        );
        assert_eq!(
            rs.query(P1, "ACTIVATE", RuleKind::CastPermission),
            Verdict::NoEffect
        );
    }
}

/// REQ-LAYER-004: `Prohibited{by}` and `Permitted{by}` name the earliest entry in application
/// order; an entry without "action" applies to no action (CR 101.2, 613.7).
#[test]
fn query_names_the_earliest_entry_and_needs_an_action() {
    // CR 101.2, CR 613.7
    let items = vec![
        (9, re(4, &[P1], rmod_action(RuleKind::Prohibition, "CAST"))),
        (8, re(3, &[P1], rmod_action(RuleKind::Prohibition, "CAST"))),
        (7, re(3, &[P1], rmod_action(RuleKind::Prohibition, "CAST"))),
        (1, re(1, &[P1], rmod(RuleKind::Prohibition))),
        (
            5,
            re(6, &[P2], rmod_action(RuleKind::TimingPermission, "CAST")),
        ),
        (
            4,
            re(6, &[P2], rmod_action(RuleKind::TimingPermission, "CAST")),
        ),
        (
            3,
            re(7, &[P2], rmod_action(RuleKind::TimingPermission, "CAST")),
        ),
        (2, re(1, &[P2], rmod(RuleKind::TimingPermission))),
    ];
    let rs = build_rule_set(&rules_registry(items), &Env::two());
    assert_eq!(
        rs.query(P1, "CAST", RuleKind::CastPermission),
        Verdict::Prohibited { by: eid(7) }
    );
    assert_eq!(
        rs.query(P2, "CAST", RuleKind::TimingPermission),
        Verdict::Permitted { by: eid(4) }
    );
    // Entries without "action" (1, 2) apply to no action.
    assert_eq!(
        rs.query(P1, "PLAY_LAND", RuleKind::PlayPermission),
        Verdict::NoEffect
    );
    assert_eq!(
        rs.query(P2, "PLAY_LAND", RuleKind::TimingPermission),
        Verdict::NoEffect
    );
    assert_eq!(
        rs.query(P1, "", RuleKind::CastPermission),
        Verdict::NoEffect
    );
}

// ---------------------------------------------------------------------------------------------
// 6. the CR 101.2 example, both timestamp orders
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: "You may play an additional land this turn" and "You can't play lands this
/// turn": the "can't" effect wins in both timestamp orders; without it two lands may be played
/// (CR 101.2 example, CR 305.2, CR 305.2b).
#[test]
fn cant_play_lands_beats_additional_land_in_both_timestamp_orders() {
    // CR 101.2, CR 305.2, CR 305.2b
    for (extra_t, cant_t) in [(1, 2), (2, 1)] {
        for reversed in [false, true] {
            let mut items = vec![
                (
                    1,
                    re(
                        extra_t,
                        &[P1],
                        rmod_int(RuleKind::AdditionalLandPlay, "count", 1),
                    ),
                ),
                (
                    2,
                    re(
                        cant_t,
                        &[P1],
                        rmod_action(RuleKind::Prohibition, "PLAY_LAND"),
                    ),
                ),
            ];
            if reversed {
                items.reverse();
            }
            let mut reg = rules_registry(items);
            let rs = build_rule_set(&reg, &Env::two());
            assert_eq!(rs.land_plays_allowed(P1), 0, "ts {extra_t}/{cant_t}");
            assert_eq!(
                rs.query(P1, "PLAY_LAND", RuleKind::PlayPermission),
                Verdict::Prohibited { by: eid(2) }
            );
            assert_eq!(rs.land_plays_allowed(P2), 1);

            assert!(reg.remove(eid(2)));
            let rs = build_rule_set(&reg, &Env::two());
            assert_eq!(rs.land_plays_allowed(P1), 2, "ts {extra_t}/{cant_t}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 7. maximum hand size
// ---------------------------------------------------------------------------------------------

fn hand(effects: &[(u64, RuleModification)]) -> Option<i64> {
    let items = effects
        .iter()
        .enumerate()
        .map(|(i, (t, m))| (u64::try_from(i).unwrap() + 1, re(*t, &[P1], m.clone())))
        .collect();
    build_rule_set(&rules_registry(items), &Env::two()).max_hand_size(P1)
}

fn hset(n: i64) -> RuleModification {
    rmod_int(RuleKind::MaxHandSize, "set", n)
}

fn hdelta(n: i64) -> RuleModification {
    rmod_int(RuleKind::MaxHandSize, "delta", n)
}

fn hunlimited() -> RuleModification {
    let mut m = rmod(RuleKind::MaxHandSize);
    m.params.insert("unlimited".to_owned(), jv(&true));
    m
}

/// REQ-LAYER-004: the maximum hand size is normally seven (CR 402.2); `set` replaces it
/// (CR 613.11).
#[test]
fn max_hand_size_default_and_set() {
    // CR 402.2, CR 613.11
    assert_eq!(hand(&[]), Some(7));
    assert_eq!(hand(&[(1, hset(5))]), Some(5));
    // Later set wins (timestamp order, CR 613.11), whatever the registration order.
    assert_eq!(hand(&[(2, hset(3)), (1, hset(5))]), Some(3));
    assert_eq!(hand(&[(1, hset(5)), (2, hset(3))]), Some(3));
    // set after delta replaces it; delta after set adds.
    assert_eq!(hand(&[(1, hdelta(4)), (2, hset(5))]), Some(5));
    assert_eq!(hand(&[(2, hdelta(4)), (1, hset(5))]), Some(9));
}

/// REQ-LAYER-004: deltas add in application order; -2 and +1 give 6 in every order
/// (CR 402.2, 613.11).
#[test]
fn max_hand_size_deltas_in_several_orders() {
    // CR 402.2, CR 613.11
    for (a, b) in [(1, 2), (2, 1)] {
        assert_eq!(hand(&[(a, hdelta(-2)), (b, hdelta(1))]), Some(6));
        assert_eq!(hand(&[(b, hdelta(1)), (a, hdelta(-2))]), Some(6));
    }
    assert_eq!(
        hand(&[(1, hdelta(-2)), (1, hdelta(1)), (1, hdelta(1))]),
        Some(7)
    );
}

/// REQ-LAYER-004: `unlimited` means no maximum until a later `set`; a delta on no maximum stays
/// no maximum (CR 402.2, 613.11).
#[test]
fn max_hand_size_unlimited() {
    // CR 402.2, CR 613.11
    assert_eq!(hand(&[(1, hunlimited())]), None);
    assert_eq!(hand(&[(1, hunlimited()), (2, hset(4))]), Some(4));
    assert_eq!(hand(&[(2, hset(4)), (1, hunlimited())]), Some(4));
    assert_eq!(hand(&[(1, hset(4)), (2, hunlimited())]), None);
    assert_eq!(hand(&[(2, hunlimited()), (1, hset(4))]), None);
    assert_eq!(hand(&[(1, hunlimited()), (2, hdelta(-3))]), None);
    assert_eq!(hand(&[(1, hunlimited()), (2, hdelta(2))]), None);
    assert_eq!(
        hand(&[(1, hunlimited()), (2, hdelta(2)), (3, hset(1))]),
        Some(1)
    );
}

/// REQ-LAYER-004: the result clamps at zero (CR 402.2, 613.11).
#[test]
fn max_hand_size_clamps_at_zero() {
    // CR 402.2, CR 613.11
    assert_eq!(hand(&[(1, hdelta(-10))]), Some(0));
    assert_eq!(hand(&[(1, hset(1)), (2, hdelta(-5))]), Some(0));
    assert_eq!(hand(&[(1, hset(-3))]), Some(0));
    assert_eq!(hand(&[(1, hdelta(-7))]), Some(0));
}

/// REQ-LAYER-004: an entry with no recognised key is ignored (CR 613.11).
#[test]
fn max_hand_size_entry_without_recognised_key_is_ignored() {
    // CR 613.11
    let none = rmod(RuleKind::MaxHandSize);
    let other = rmod_int(RuleKind::MaxHandSize, "size", 2);
    assert_eq!(hand(&[(1, none.clone())]), Some(7));
    assert_eq!(hand(&[(1, other.clone())]), Some(7));
    assert_eq!(hand(&[(1, hset(5)), (2, none), (3, other)]), Some(5));
}

/// REQ-LAYER-004: a hand-size effect changes only the players it applies to (CR 402.2: each
/// player has a maximum hand size; CR 613.11).
#[test]
fn max_hand_size_per_player() {
    // CR 402.2, CR 613.11
    let mut opp = rule_effect(2, sel(opponents()), hdelta(-1));
    opp.controller = P1;
    let items = vec![
        (1, re(1, &[P1], hset(5))),
        (2, opp),
        (3, re(3, &[P3], hunlimited())),
    ];
    let rs = build_rule_set(&rules_registry(items), &Env::new(&[P1, P2, P3]));
    assert_eq!(rs.max_hand_size(P1), Some(5));
    assert_eq!(rs.max_hand_size(P2), Some(6));
    assert_eq!(rs.max_hand_size(P3), None);
}

// ---------------------------------------------------------------------------------------------
// 8. extra land plays
// ---------------------------------------------------------------------------------------------

fn lands(effects: Vec<(PlayerIdList, RuleModification)>) -> (i64, i64) {
    let items = effects
        .into_iter()
        .enumerate()
        .map(|(i, (ps, m))| {
            let n = u64::try_from(i).unwrap() + 1;
            (n, re(n, &ps, m))
        })
        .collect();
    let rs = build_rule_set(&rules_registry(items), &Env::two());
    (rs.land_plays_allowed(P1), rs.land_plays_allowed(P2))
}

type PlayerIdList = Vec<PlayerId>;

fn extra(n: i64) -> RuleModification {
    rmod_int(RuleKind::AdditionalLandPlay, "count", n)
}

/// REQ-LAYER-004: one land per turn by default; additional land plays add their counts
/// (CR 305.2: "continuous effects may increase this number").
#[test]
fn land_plays_default_and_counts() {
    // CR 305.2
    assert_eq!(lands(vec![]), (1, 1));
    assert_eq!(lands(vec![(vec![P1], extra(1))]), (2, 1));
    assert_eq!(lands(vec![(vec![P1], extra(2))]), (3, 1));
    assert_eq!(
        lands(vec![(vec![P1], extra(1)), (vec![P1], extra(2))]),
        (4, 1)
    );
    // A missing count counts 1.
    assert_eq!(
        lands(vec![(vec![P1], rmod(RuleKind::AdditionalLandPlay))]),
        (2, 1)
    );
}

/// REQ-LAYER-004, REQ-DSL-008: a negative or non-integer count counts 0 (CR 305.2).
#[test]
fn land_plays_ignore_bad_counts() {
    // CR 305.2
    let mut text = rmod(RuleKind::AdditionalLandPlay);
    text.params.insert("count".to_owned(), jv("2"));
    assert_eq!(lands(vec![(vec![P1], extra(-3))]), (1, 1));
    assert_eq!(lands(vec![(vec![P1], text.clone())]), (1, 1));
    assert_eq!(
        lands(vec![
            (vec![P1], extra(-3)),
            (vec![P1], text),
            (vec![P1], extra(1))
        ]),
        (2, 1)
    );
}

/// REQ-LAYER-004: additional land plays are per player (CR 305.2).
#[test]
fn land_plays_per_player() {
    // CR 305.2
    assert_eq!(
        lands(vec![
            (vec![P1], extra(1)),
            (vec![P2], extra(2)),
            (vec![P1, P2], extra(1)),
        ]),
        (3, 4)
    );
}

/// REQ-LAYER-004: two prohibitions still give 0, and a prohibition about another action does
/// not stop land plays (CR 101.2, 305.2b).
#[test]
fn land_plays_prohibited() {
    // CR 101.2, CR 305.2b
    let no_lands = || rmod_action(RuleKind::Prohibition, "PLAY_LAND");
    assert_eq!(
        lands(vec![
            (vec![P1], extra(2)),
            (vec![P1], no_lands()),
            (vec![P1, P2], no_lands()),
        ]),
        (0, 0)
    );
    assert_eq!(
        lands(vec![
            (vec![P1], extra(2)),
            (vec![P1], rmod_action(RuleKind::Prohibition, "CAST")),
            (vec![P1], rmod(RuleKind::Prohibition)),
        ]),
        (3, 1)
    );
}

// ---------------------------------------------------------------------------------------------
// 9. cost hook
// ---------------------------------------------------------------------------------------------

fn cost_registry() -> PostLayerEffects {
    let mut items = vec![
        // P1's entries (timestamps chosen so kind order and timestamp order differ).
        (1, re(7, &[P1], rmod(RuleKind::CostIncrease))),
        (2, re(1, &[P1], rmod(RuleKind::CostIncrease))),
        (3, re(8, &[P1], rmod(RuleKind::CostReduction))),
        (4, re(3, &[P1], rmod(RuleKind::CostReduction))),
        (5, re(5, &[P1, P2], rmod(RuleKind::CostReduction))),
        (6, re(9, &[P1], rmod(RuleKind::CostMinimum))),
        (7, re(4, &[P1], rmod(RuleKind::CostMinimum))),
        (8, re(2, &[P1], rmod(RuleKind::CostSet))),
        (9, re(6, &[P1], rmod(RuleKind::CostSet))),
        (10, re(10, &[P1], rmod(RuleKind::AltCostGrant))),
        (11, re(2, &[P1], rmod(RuleKind::AdditionalCostRequire))),
        // P2-only entries.
        (20, re(1, &[P2], rmod(RuleKind::CostIncrease))),
        (21, re(1, &[P2], rmod(RuleKind::CostSet))),
        (22, re(1, &[P2], rmod(RuleKind::AltCostGrant))),
        // Not a cost kind.
        (30, re(1, &[P1], rmod(RuleKind::PaymentMethod))),
    ];
    items.reverse();
    rules_registry(items)
}

/// REQ-LAYER-004: `cost_stages` separates grants, increases, reductions (timestamp order by
/// default) and total-cost effects (COST_MINIMUM then COST_SET, each in timestamp order), only
/// for the queried player (CR 601.2f, 613.11).
#[test]
fn cost_stages_separate_the_601_2f_stages() {
    // CR 601.2f, CR 613.11
    let rs = build_rule_set(&cost_registry(), &Env::two());
    let s = rs.cost_stages(P1);
    assert_eq!(ids(&s.grants), eids(&[11, 10]));
    assert_eq!(ids(&s.increases), eids(&[2, 1]));
    assert_eq!(ids(&s.reductions), eids(&[4, 5, 3]));
    assert_eq!(ids(&s.total_effects), eids(&[7, 6, 8, 9]));

    let s2 = rs.cost_stages(P2);
    assert_eq!(ids(&s2.grants), eids(&[22]));
    assert_eq!(ids(&s2.increases), eids(&[20]));
    assert_eq!(ids(&s2.reductions), eids(&[5]));
    assert_eq!(ids(&s2.total_effects), eids(&[21]));

    let s3 = rs.cost_stages(P3);
    assert!(s3.grants.is_empty() && s3.increases.is_empty());
    assert!(s3.reductions.is_empty() && s3.total_effects.is_empty());
}

/// REQ-LAYER-004: "If multiple cost reductions apply, the player may apply them in any order"
/// (CR 601.2f): a valid permutation reorders exactly.
#[test]
fn reorder_reductions_valid_permutation() {
    // CR 601.2f
    let rs = build_rule_set(&cost_registry(), &Env::two());
    for perm in permutations(3) {
        let base = [4, 5, 3];
        let order: Vec<u64> = perm.iter().map(|&i| base[i]).collect();
        let mut s = rs.cost_stages(P1);
        s.reorder_reductions(&eids(&order)).unwrap();
        assert_eq!(ids(&s.reductions), eids(&order));
        // Other stages are untouched.
        assert_eq!(ids(&s.increases), eids(&[2, 1]));
        assert_eq!(ids(&s.total_effects), eids(&[7, 6, 8, 9]));
    }
}

/// REQ-LAYER-004: an order that is not a permutation of the current reductions is rejected with
/// Unsupported and leaves the order unchanged (CR 601.2f).
#[test]
fn reorder_reductions_rejects_non_permutations() {
    // CR 601.2f
    let rs = build_rule_set(&cost_registry(), &Env::two());
    let bad: [&[u64]; 6] = [
        &[4, 5, 99],    // unknown id
        &[4, 5],        // missing id
        &[4, 4, 5],     // duplicate id
        &[4, 5, 3, 99], // extra id
        &[3, 4, 5, 5],  // duplicate plus all
        &[],            // empty
    ];
    for order in bad {
        let mut s = rs.cost_stages(P1);
        let err = s.reorder_reductions(&eids(order)).unwrap_err();
        assert!(
            matches!(err, LayerError::Unsupported(_)),
            "{order:?}: {err:?}"
        );
        assert_eq!(ids(&s.reductions), eids(&[4, 5, 3]), "{order:?}");
    }
    // An empty reduction list accepts only the empty order.
    let mut s = rs.cost_stages(P3);
    s.reorder_reductions(&[]).unwrap();
    assert!(s.reorder_reductions(&eids(&[4])).is_err());
    assert!(s.reductions.is_empty());
}
