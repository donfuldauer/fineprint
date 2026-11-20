// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for pass A of the post-layer passes, the effects that affect players
//! (REQ-LAYER-004, REQ-DSL-008, REQ-CORE-001; CR 613.10, 613.7, 613.8a, 613.8b, 611.2c):
//! application order, Fixed versus Selector scoping, existence, resolved-origin rules and the
//! optional dependency hook. Black-box: written from the work-item brief only.

#[path = "spec_layers_post_support.rs"]
mod support;

use fineprint_core::layers::{
    EffectOrigin, LayerError, PlayerAffected, PlayerPassEffect, PostLayerEffects, build_rule_set,
    compute_player_properties,
};
use fineprint_dsl::ability::RuleKind;
use support::*;

/// `(id, timestamp, affected)` registered in `order`, each with op `OP_<id>`.
fn registry(items: &[(u64, u64, PlayerAffected)], order: &[usize]) -> PostLayerEffects {
    let mut reg = PostLayerEffects::new();
    for &i in order {
        let (id, t, a) = &items[i];
        reg.register_player(eid(*id), player_effect(*t, a.clone(), &format!("OP_{id}")))
            .expect("register player effect");
    }
    reg
}

fn in_order(items: &[(u64, u64, PlayerAffected)]) -> PostLayerEffects {
    let order: Vec<usize> = (0..items.len()).collect();
    registry(items, &order)
}

// ---------------------------------------------------------------------------------------------
// 1. order
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004, REQ-CORE-001: player effects apply in timestamp order (CR 613.10: "applied in
/// timestamp order"; CR 613.7: earlier timestamp first), whatever the registration order.
#[test]
fn pass_a_applies_in_timestamp_order_for_every_registration_order() {
    // CR 613.10, CR 613.7
    let items = [
        (10, 3, fixed(&[P1])),
        (11, 1, fixed(&[P1])),
        (12, 2, fixed(&[P1])),
    ];
    for order in permutations(items.len()) {
        let reg = registry(&items, &order);
        let props = compute_player_properties(&reg, &Env::two());
        assert_eq!(
            applied_ids(&props, P1),
            eids(&[11, 12, 10]),
            "registration order {order:?}"
        );
        // The applied modification is the effect's own.
        let full = applied_full(&props, P1);
        assert_eq!(full[0].1, pmod("OP_11"));
        assert_eq!(full[1].1, pmod("OP_12"));
        assert_eq!(full[2].1, pmod("OP_10"));
        assert!(applied_ids(&props, P2).is_empty());
    }
}

/// REQ-LAYER-004: equal timestamps break by ascending EffectId (CR 613.7; the brief's ordering
/// key is (timestamp, EffectId)).
#[test]
fn pass_a_equal_timestamps_break_by_effect_id() {
    // CR 613.7, CR 613.10
    let items = [
        (7, 2, fixed(&[P1, P2])),
        (3, 2, fixed(&[P1, P2])),
        (5, 2, fixed(&[P1, P2])),
        (9, 1, fixed(&[P1, P2])),
    ];
    for order in permutations(items.len()) {
        let reg = registry(&items, &order);
        let props = compute_player_properties(&reg, &Env::two());
        assert_eq!(applied_ids(&props, P1), eids(&[9, 3, 5, 7]), "{order:?}");
        assert_eq!(applied_ids(&props, P2), eids(&[9, 3, 5, 7]), "{order:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// 2. scoping, existence, resolved origin
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: a Fixed set applies to exactly its stored players; a Selector applies to the
/// players the environment matches (CR 613.10).
#[test]
fn pass_a_fixed_and_selector_scoping() {
    // CR 613.10
    let items = [
        (1, 1, fixed(&[P2])),
        (2, 2, sel(you())),
        (3, 3, sel(opponents())),
        (4, 4, sel(all_players())),
        (5, 5, sel(nobody())),
        (6, 6, fixed(&[P1, P3])),
    ];
    let env = Env::new(&[P1, P2, P3]);
    let props = compute_player_properties(&in_order(&items), &env);
    // Controller of every effect is P1.
    assert_eq!(applied_ids(&props, P1), eids(&[2, 4, 6]));
    assert_eq!(applied_ids(&props, P2), eids(&[1, 3, 4]));
    assert_eq!(applied_ids(&props, P3), eids(&[3, 4, 6]));
}

/// REQ-LAYER-004: a player in a Fixed set who is not among `env.players()` is ignored, and
/// `applied` of an unknown player is empty (CR 613.10).
#[test]
fn pass_a_unknown_fixed_player_is_ignored() {
    // CR 613.10
    let items = [(1, 1, fixed(&[P1, P9])), (2, 2, fixed(&[P9]))];
    let props = compute_player_properties(&in_order(&items), &Env::two());
    assert_eq!(applied_ids(&props, P1), eids(&[1]));
    assert!(applied_ids(&props, P2).is_empty());
    assert!(props.applied(P9).is_empty());
    assert!(props.applied(pid(77)).is_empty());
}

/// REQ-LAYER-004: an effect whose `effect_exists` is false is dropped; the others keep their
/// order (CR 613.10).
#[test]
fn pass_a_effect_that_does_not_exist_is_dropped() {
    // CR 613.10
    let items = [
        (1, 1, fixed(&[P1])),
        (2, 2, sel(all_players())),
        (3, 3, fixed(&[P1, P2])),
    ];
    let env = Env::two().without(2);
    let props = compute_player_properties(&in_order(&items), &env);
    assert_eq!(applied_ids(&props, P1), eids(&[1, 3]));
    assert_eq!(applied_ids(&props, P2), eids(&[3]));
}

/// REQ-LAYER-004: a resolved player effect with a selector is rejected with Unsupported and the
/// registry is unchanged; a resolved effect with a Fixed set is accepted (CR 611.2c; #35 D4).
#[test]
fn pass_a_resolved_selector_rejected_resolved_fixed_accepted() {
    // CR 611.2c
    let mut reg = PostLayerEffects::new();
    reg.register_player(eid(1), player_effect(1, fixed(&[P2]), "OP_1"))
        .unwrap();
    let before = reg.clone();

    let mut bad = player_effect(2, sel(all_players()), "OP_2");
    bad.origin = EffectOrigin::Resolved;
    let err = reg.register_player(eid(2), bad).unwrap_err();
    assert!(matches!(err, LayerError::Unsupported(_)), "{err:?}");
    assert_eq!(reg, before);
    assert_eq!(reg.len(), 1);
    assert!(reg.player_effect(eid(2)).is_none());

    // The same id is still free: a resolved Fixed effect is accepted.
    let mut good = player_effect(2, fixed(&[P1, P2]), "OP_2");
    good.origin = EffectOrigin::Resolved;
    reg.register_player(eid(2), good).unwrap();
    assert_eq!(reg.len(), 2);
    let stored = reg.player_effect(eid(2)).expect("stored");
    assert_eq!(stored.origin, EffectOrigin::Resolved);
    assert_eq!(stored.affected, fixed(&[P1, P2]));

    let props = compute_player_properties(&reg, &Env::two());
    assert_eq!(applied_ids(&props, P1), eids(&[2]));
    assert_eq!(applied_ids(&props, P2), eids(&[1, 2]));
}

/// REQ-LAYER-004: a static-origin player effect may use a selector, matched at every
/// computation against the current players (CR 613.10).
#[test]
fn pass_a_static_selector_is_matched_at_each_computation() {
    // CR 613.10
    let reg = in_order(&[(1, 1, sel(all_players()))]);
    let two = compute_player_properties(&reg, &Env::two());
    assert!(applied_ids(&two, P3).is_empty());
    let three = compute_player_properties(&reg, &Env::new(&[P1, P2, P3]));
    assert_eq!(applied_ids(&three, P3), eids(&[1]));
}

/// REQ-LAYER-004: the defaults of `effect_exists` and `depends_on` keep every effect and add
/// no dependency (CR 613.10, 613.7).
#[test]
fn pass_a_default_hooks() {
    // CR 613.10, CR 613.7
    let items = [(2, 2, fixed(&[P1])), (1, 1, fixed(&[P1]))];
    let props = compute_player_properties(&in_order(&items), &PlainEnv(vec![P1]));
    assert_eq!(applied_ids(&props, P1), eids(&[1, 2]));
}

/// REQ-LAYER-004, REQ-DSL-008: maximum hand size is pass B only; a MAX_HAND_SIZE rule effect
/// never appears in PlayerProperties, and a pass A modification named like it does not change
/// the maximum hand size (CR 613.11, CR 402.2; #35 D1).
#[test]
fn max_hand_size_never_in_player_properties() {
    // CR 613.11, CR 402.2
    let mut reg = PostLayerEffects::new();
    reg.register_rule(
        eid(1),
        rule_effect(
            1,
            fixed(&[P1, P2]),
            rmod_int(RuleKind::MaxHandSize, "set", 3),
        ),
    )
    .unwrap();
    let env = Env::two();
    let props = compute_player_properties(&reg, &env);
    assert!(props.applied(P1).is_empty());
    assert!(props.applied(P2).is_empty());
    assert_eq!(build_rule_set(&reg, &env).max_hand_size(P1), Some(3));

    // A player effect whose op merely looks like a hand-size change is not pass B.
    let mut reg2 = PostLayerEffects::new();
    let mut m = pmod("MAX_HAND_SIZE");
    m.params.insert("set".to_owned(), jv(&2i64));
    let pe = PlayerPassEffect {
        modification: m,
        ..player_effect(1, fixed(&[P1]), "X")
    };
    let _accepted_or_rejected = reg2.register_player(eid(2), pe);
    assert_eq!(build_rule_set(&reg2, &env).max_hand_size(P1), Some(7));
}

// ---------------------------------------------------------------------------------------------
// 3. dependency (optional hook)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-004: B depends on A while A has the later timestamp: A applies first (CR 613.8a,
/// 613.8b).
#[test]
fn pass_a_dependent_waits_for_its_base() {
    // CR 613.8a, CR 613.8b
    // B = 1 (ts 1), A = 2 (ts 2); 1 depends on 2.
    let items = [(1, 1, fixed(&[P1])), (2, 2, fixed(&[P1]))];
    for order in permutations(items.len()) {
        let reg = registry(&items, &order);
        let props = compute_player_properties(&reg, &Env::two().depends(1, 2));
        assert_eq!(applied_ids(&props, P1), eids(&[2, 1]), "{order:?}");
    }
}

/// REQ-LAYER-004: two dependents of one base apply just after it, in timestamp order relative
/// to each other; an independent later effect follows (CR 613.8b).
#[test]
fn pass_a_two_dependents_apply_in_timestamp_order_after_base() {
    // CR 613.8b
    // base 3 (ts 3); dependents 2 (ts 2) and 1 (ts 1); independent 4 (ts 4).
    let items = [
        (1, 1, fixed(&[P1])),
        (2, 2, fixed(&[P1])),
        (3, 3, fixed(&[P1])),
        (4, 4, fixed(&[P1])),
    ];
    let env = Env::two().depends(1, 3).depends(2, 3);
    for order in permutations(items.len()) {
        let props = compute_player_properties(&registry(&items, &order), &env);
        assert_eq!(applied_ids(&props, P1), eids(&[3, 1, 2, 4]), "{order:?}");
    }
}

/// REQ-LAYER-004: a dependency loop of two applies in timestamp order (CR 613.8b).
#[test]
fn pass_a_two_cycle_applies_in_timestamp_order() {
    // CR 613.8b
    let items = [(5, 2, fixed(&[P1])), (6, 1, fixed(&[P1]))];
    let env = Env::two().depends(5, 6).depends(6, 5);
    for order in permutations(items.len()) {
        let props = compute_player_properties(&registry(&items, &order), &env);
        assert_eq!(applied_ids(&props, P1), eids(&[6, 5]), "{order:?}");
    }
}

/// REQ-LAYER-004: per the #35 brief ("if none qualifies (every remaining effect is in or behind
/// a cycle), apply the earliest-key remaining effect and continue"), the earliest-key remaining
/// effect applies even when it sits behind the loop rather than in it (CR 613.8b).
#[test]
fn pass_a_everything_blocked_applies_earliest_key() {
    // CR 613.8b
    // 2 <-> 3 is a loop; 1 (ts 1) depends on 2 and sits behind it.
    let items = [
        (1, 1, fixed(&[P1])),
        (2, 2, fixed(&[P1])),
        (3, 3, fixed(&[P1])),
    ];
    let env = Env::two().depends(2, 3).depends(3, 2).depends(1, 2);
    for order in permutations(items.len()) {
        let props = compute_player_properties(&registry(&items, &order), &env);
        assert_eq!(applied_ids(&props, P1), eids(&[1, 2, 3]), "{order:?}");
    }
}

/// REQ-LAYER-004: a chain of three: C depends on B, B depends on A, with timestamps in the
/// opposite order: A, B, C (CR 613.8a, 613.8b).
#[test]
fn pass_a_chain_of_three() {
    // CR 613.8a, CR 613.8b
    // C = 1 (ts 1), B = 2 (ts 2), A = 3 (ts 3).
    let items = [
        (1, 1, fixed(&[P1])),
        (2, 2, fixed(&[P1])),
        (3, 3, fixed(&[P1])),
    ];
    let env = Env::two().depends(1, 2).depends(2, 3);
    for order in permutations(items.len()) {
        let props = compute_player_properties(&registry(&items, &order), &env);
        assert_eq!(applied_ids(&props, P1), eids(&[3, 2, 1]), "{order:?}");
    }
}

/// REQ-LAYER-004: a dependency on an effect that does not exist is ignored: the dependent
/// applies in its normal place (CR 613.8a, 613.8b).
#[test]
fn pass_a_dependency_on_dropped_effect_is_ignored() {
    // CR 613.8a, CR 613.8b
    // 1 (ts 1) depends on 2 (ts 5), which is dropped; 3 (ts 3) is independent.
    let items = [
        (1, 1, fixed(&[P1])),
        (2, 5, fixed(&[P1])),
        (3, 3, fixed(&[P1])),
    ];
    let env = Env::two().depends(1, 2).without(2);
    for order in permutations(items.len()) {
        let props = compute_player_properties(&registry(&items, &order), &env);
        assert_eq!(applied_ids(&props, P1), eids(&[1, 3]), "{order:?}");
    }
}

/// REQ-LAYER-004: dependency is judged across the whole pass, so a dependent's order holds for
/// every player it affects (CR 613.8b).
#[test]
fn pass_a_dependency_order_is_the_same_for_every_affected_player() {
    // CR 613.8b
    let items = [(1, 1, fixed(&[P1, P2])), (2, 2, sel(all_players()))];
    let env = Env::two().depends(1, 2);
    let props = compute_player_properties(&in_order(&items), &env);
    assert_eq!(applied_ids(&props, P1), eids(&[2, 1]));
    assert_eq!(applied_ids(&props, P2), eids(&[2, 1]));
}
