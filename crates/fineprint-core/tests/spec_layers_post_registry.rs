// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the post-layer registry and its robustness (REQ-LAYER-004, REQ-DSL-008,
//! REQ-CORE-001; CR 613.10, 613.11): one id space across both passes, removal, size,
//! determinism under shuffled registration, and arbitrary JSON in rule parameters (R12).
//! Black-box: written from the work-item brief only.

#[path = "spec_layers_post_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::layers::{
    EffectOrigin, LayerError, PlayerPassEffect, PostLayerEffects, RuleEffect, build_rule_set,
    compute_player_properties,
};
use fineprint_dsl::ability::{RuleKind, RuleModification};
use support::*;

// ---------------------------------------------------------------------------------------------
// 10. registry
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: an id already used in either map is a DuplicateEffect, in both directions,
/// and the registry is unchanged (CR 613.10, 613.11).
#[test]
fn duplicate_id_across_both_passes() {
    // CR 613.10, CR 613.11
    let mut reg = PostLayerEffects::new();
    reg.register_player(eid(1), player_effect(1, fixed(&[P1]), "OP_1"))
        .unwrap();
    reg.register_rule(eid(2), rule_effect(1, fixed(&[P1]), rmod(RuleKind::Skip)))
        .unwrap();
    let before = reg.clone();

    let cases: [(u64, bool); 4] = [(1, true), (1, false), (2, true), (2, false)];
    for (id, as_player) in cases {
        let err = if as_player {
            reg.register_player(eid(id), player_effect(5, fixed(&[P2]), "OP_X"))
        } else {
            reg.register_rule(
                eid(id),
                rule_effect(5, fixed(&[P2]), rmod(RuleKind::Evasion)),
            )
        }
        .unwrap_err();
        assert_eq!(
            err,
            LayerError::DuplicateEffect(eid(id)),
            "id {id} player {as_player}"
        );
        assert_eq!(reg, before);
    }
    assert_eq!(
        reg.player_effect(eid(1)).unwrap().modification,
        pmod("OP_1")
    );
    assert!(reg.rule_effect(eid(1)).is_none());
    assert_eq!(
        reg.rule_effect(eid(2)).unwrap().modification,
        rmod(RuleKind::Skip)
    );
    assert!(reg.player_effect(eid(2)).is_none());
}

/// REQ-LAYER-004: `remove` returns true then false; `len` counts both maps; `is_empty`
/// (CR 613.10, 613.11).
#[test]
fn remove_len_is_empty() {
    // CR 613.10, CR 613.11
    let mut reg = PostLayerEffects::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert_eq!(reg, PostLayerEffects::default());
    reg.register_player(eid(1), player_effect(1, fixed(&[P1]), "OP_1"))
        .unwrap();
    reg.register_rule(eid(2), rule_effect(1, fixed(&[P1]), rmod(RuleKind::Skip)))
        .unwrap();
    assert_eq!(reg.len(), 2);
    assert!(!reg.is_empty());

    assert!(reg.remove(eid(1)));
    assert!(!reg.remove(eid(1)));
    assert_eq!(reg.len(), 1);
    assert!(reg.player_effect(eid(1)).is_none());
    assert!(reg.remove(eid(2)));
    assert!(!reg.remove(eid(2)));
    assert!(!reg.remove(eid(3)));
    assert!(reg.is_empty());
    assert_eq!(reg, PostLayerEffects::new());

    // A removed id may be used again, in the other map.
    reg.register_rule(eid(1), rule_effect(1, fixed(&[P1]), rmod(RuleKind::Skip)))
        .unwrap();
    assert_eq!(reg.len(), 1);
    // A removed effect no longer applies.
    let props = compute_player_properties(&reg, &Env::two());
    assert!(props.applied(P1).is_empty());
}

/// The mixed set of effects registered in [`determinism_under_shuffled_registration`].
fn mixed_items() -> Vec<(u64, Result<PlayerPassEffect, RuleEffect>)> {
    let mut out: Vec<(u64, Result<PlayerPassEffect, RuleEffect>)> = Vec::new();
    for i in 0..12u64 {
        let affected = match i % 4 {
            0 => fixed(&[P1]),
            1 => sel(all_players()),
            2 => sel(opponents()),
            _ => fixed(&[P2, P3]),
        };
        out.push((
            i + 1,
            Ok(player_effect(i % 3 + 1, affected, &format!("OP_{i}"))),
        ));
    }
    let kinds = [
        RuleKind::Prohibition,
        RuleKind::CastPermission,
        RuleKind::AdditionalLandPlay,
        RuleKind::MaxHandSize,
        RuleKind::CostReduction,
        RuleKind::CostSet,
    ];
    for i in 0..18u64 {
        let kind = kinds[usize::try_from(i).unwrap() % kinds.len()];
        let mut m = rmod_action(kind, if i % 2 == 0 { "CAST" } else { "PLAY_LAND" });
        m.params
            .insert("delta".to_owned(), jv(&(i64::try_from(i).unwrap() - 9)));
        let players = if i % 3 == 0 {
            sel(you())
        } else {
            fixed(&[P1, P2])
        };
        let mut e = rule_effect(i % 4 + 1, players, m);
        if i % 5 == 0 {
            e.origin = EffectOrigin::Resolved;
        }
        out.push((100 + i, Err(e)));
    }
    out
}

/// REQ-CORE-001, REQ-LAYER-004: the same effects registered in 20 shuffled orders give equal
/// registries, equal PlayerProperties, equal entries for every kind and equal derived values
/// (CR 613.10, 613.11, 613.7).
#[test]
fn determinism_under_shuffled_registration() {
    // CR 613.10, CR 613.11, CR 613.7
    let env = Env::new(&[P1, P2, P3]).depends(1, 5).depends(101, 107);
    let mut rng = Rng::new(0x0035_0010);
    let mut reference = None;
    for round in 0..20 {
        let mut items = mixed_items();
        rng.shuffle(&mut items);
        let mut reg = PostLayerEffects::new();
        for (id, e) in items {
            match e {
                Ok(p) => reg.register_player(eid(id), p).unwrap(),
                Err(r) => reg.register_rule(eid(id), r).unwrap(),
            }
        }
        let props = compute_player_properties(&reg, &env);
        let rs = build_rule_set(&reg, &env);
        let mut observed = Vec::new();
        for p in [P1, P2, P3] {
            observed.push(format!("{:?}", applied_full(&props, p)));
            observed.push(format!("{:?}", rs.max_hand_size(p)));
            observed.push(format!("{:?}", rs.land_plays_allowed(p)));
            for a in ["CAST", "PLAY_LAND"] {
                for k in [RuleKind::CastPermission, RuleKind::PlayPermission] {
                    observed.push(format!("{:?}", rs.query(p, a, k)));
                }
            }
            let s = rs.cost_stages(p);
            for list in [&s.grants, &s.increases, &s.reductions, &s.total_effects] {
                observed.push(format!(
                    "{:?}",
                    list.iter().map(entry_view).collect::<Vec<_>>()
                ));
            }
        }
        for kind in RuleKind::ALL {
            observed.push(format!("{:?}", entries_view(&rs, *kind)));
        }
        match &reference {
            None => reference = Some((reg, observed)),
            Some((r, o)) => {
                assert_eq!(&reg, r, "round {round}: registry");
                assert_eq!(&observed, o, "round {round}: results");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 11. robustness: arbitrary JSON in params (R12)
// ---------------------------------------------------------------------------------------------

/// Odd parameter values: null, bool, string, arrays, objects and integers at the extremes.
fn odd_values<V: serde::de::DeserializeOwned>() -> Vec<V> {
    let mut obj = BTreeMap::new();
    obj.insert("set".to_owned(), i64::MAX);
    vec![
        jv(&()),
        jv(&true),
        jv(&false),
        jv("PLAY_LAND"),
        jv(""),
        jv(&Vec::<i64>::new()),
        jv(&vec![1i64, 2]),
        jv(&vec!["CAST"]),
        jv(&obj),
        jv(&BTreeMap::<String, i64>::new()),
        jv(&i64::MAX),
        jv(&(i64::MAX - 1)),
        jv(&i64::MIN),
        jv(&(i64::MIN + 1)),
        jv(&u64::MAX),
        jv(&0i64),
        jv(&-1i64),
    ]
}

/// Exercises every RuleSet query for every player.
fn exercise(reg: &PostLayerEffects) {
    let env = Env::new(&[P1, P2]);
    let rs = build_rule_set(reg, &env);
    let _ = compute_player_properties(reg, &env);
    for p in [P1, P2, P9] {
        let _ = rs.max_hand_size(p);
        let _ = rs.land_plays_allowed(p);
        for a in ["CAST", "PLAY_LAND", ""] {
            for k in [
                RuleKind::CastPermission,
                RuleKind::PlayPermission,
                RuleKind::TimingPermission,
                RuleKind::Prohibition,
            ] {
                let _ = rs.query(p, a, k);
            }
        }
        let mut s = rs.cost_stages(p);
        let order: Vec<_> = s.reductions.iter().rev().map(|e| e.id).collect();
        s.reorder_reductions(&order).unwrap();
    }
    for kind in RuleKind::ALL {
        let _ = rs.entries(*kind);
    }
}

/// REQ-LAYER-004, REQ-DSL-008: odd JSON types in "delta", "set", "unlimited", "count" and
/// "action" never panic, alone or stacked, including integers near i64::MAX whose sum
/// overflows (CR 402.2, 305.2, 613.11; R12).
#[test]
fn odd_json_params_never_panic() {
    // CR 402.2, CR 305.2, CR 613.11
    let keys = ["delta", "set", "unlimited", "count", "action"];
    let kinds = [
        RuleKind::MaxHandSize,
        RuleKind::AdditionalLandPlay,
        RuleKind::Prohibition,
        RuleKind::PlayPermission,
        RuleKind::CostReduction,
    ];
    for kind in kinds {
        for key in keys {
            // One entry per odd value, all stacked in one registry, so sums overflow.
            let mut reg = PostLayerEffects::new();
            for (i, v) in odd_values().into_iter().enumerate() {
                let mut m = RuleModification::new(kind);
                m.params.insert(key.to_owned(), v);
                let id = u64::try_from(i).unwrap() + 1;
                reg.register_rule(eid(id), rule_effect(id, fixed(&[P1, P2]), m))
                    .unwrap();
            }
            exercise(&reg);
            // And each value alone.
            for v in odd_values() {
                let mut m = RuleModification::new(kind);
                m.params.insert(key.to_owned(), v);
                exercise(&rules_registry(vec![(1, rule_effect(1, fixed(&[P1]), m))]));
            }
        }
    }
}

/// REQ-LAYER-004: huge deltas and counts saturate or are ignored without an overflow panic
/// (debug builds panic on unchecked overflow) (CR 402.2, 305.2; R12).
#[test]
fn huge_delta_and_count_do_not_overflow() {
    // CR 402.2, CR 305.2
    let items = vec![
        (
            1,
            rule_effect(
                1,
                fixed(&[P1]),
                rmod_int(RuleKind::MaxHandSize, "delta", i64::MAX),
            ),
        ),
        (
            2,
            rule_effect(
                2,
                fixed(&[P1]),
                rmod_int(RuleKind::MaxHandSize, "delta", i64::MAX),
            ),
        ),
        (
            3,
            rule_effect(
                3,
                fixed(&[P2]),
                rmod_int(RuleKind::MaxHandSize, "delta", i64::MIN),
            ),
        ),
        (
            4,
            rule_effect(
                4,
                fixed(&[P2]),
                rmod_int(RuleKind::MaxHandSize, "delta", i64::MIN),
            ),
        ),
        (
            5,
            rule_effect(
                5,
                fixed(&[P1]),
                rmod_int(RuleKind::AdditionalLandPlay, "count", i64::MAX),
            ),
        ),
        (
            6,
            rule_effect(
                6,
                fixed(&[P1]),
                rmod_int(RuleKind::AdditionalLandPlay, "count", i64::MAX),
            ),
        ),
        (
            7,
            rule_effect(
                7,
                fixed(&[P2]),
                rmod_int(RuleKind::MaxHandSize, "set", i64::MAX),
            ),
        ),
        (
            8,
            rule_effect(8, fixed(&[P2]), rmod_int(RuleKind::MaxHandSize, "delta", 1)),
        ),
        (
            9,
            rule_effect(
                9,
                fixed(&[P2]),
                rmod_int(RuleKind::AdditionalLandPlay, "count", 1),
            ),
        ),
        (
            10,
            rule_effect(
                10,
                fixed(&[P2]),
                rmod_int(RuleKind::AdditionalLandPlay, "count", i64::MAX),
            ),
        ),
    ];
    let reg = rules_registry(items);
    let rs = build_rule_set(&reg, &Env::two());
    let _ = rs.max_hand_size(P1);
    let _ = rs.max_hand_size(P2);
    let _ = rs.land_plays_allowed(P1);
    let _ = rs.land_plays_allowed(P2);
    exercise(&reg);
}

/// REQ-LAYER-004: an "action" that is not a string applies to no action, so it neither
/// prohibits nor permits (brief: "Compared by string equality"; CR 101.2).
#[test]
fn non_string_action_applies_to_no_action() {
    // CR 101.2
    for v in odd_values() {
        let mut prohib = RuleModification::new(RuleKind::Prohibition);
        prohib.params.insert("action".to_owned(), v);
        let mut perm = rmod_action(RuleKind::PlayPermission, "PLAY_LAND");
        perm.params.insert("count".to_owned(), jv(&1i64));
        let mut probe = prohib.params.clone();
        probe.insert("action".to_owned(), jv("PLAY_LAND"));
        let is_string_play_land = probe == prohib.params;
        let reg = rules_registry(vec![
            (1, rule_effect(1, fixed(&[P1]), prohib)),
            (2, rule_effect(2, fixed(&[P1]), perm)),
        ]);
        let rs = build_rule_set(&reg, &Env::two());
        if is_string_play_land {
            assert_eq!(rs.land_plays_allowed(P1), 0);
            continue;
        }
        assert_eq!(
            rs.query(P1, "CAST", RuleKind::CastPermission),
            fineprint_core::layers::Verdict::NoEffect
        );
        assert_eq!(
            rs.query(P1, "PLAY_LAND", RuleKind::PlayPermission),
            fineprint_core::layers::Verdict::Permitted { by: eid(2) }
        );
        assert_eq!(rs.land_plays_allowed(P1), 1);
    }
}
