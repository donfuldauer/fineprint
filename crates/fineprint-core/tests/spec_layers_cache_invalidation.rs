// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for layer-cache invalidation (REQ-LAYER-005, SRS 6.5): for every
//! `Invalidation::ALL` cause, a warm cache over the standard synthetic board (4 players, 24
//! objects; effects in layers 2, 4, 5, 6, 7a CDA, 7b, 7c; a locked-set resolved effect; a
//! +1/+1 counter; a dependency pair; a lock-in group) sees the matching environment change,
//! the caller declares it, and the cache then equals a fresh `layers::evaluate_all`. Registry
//! changes are detected by the cache itself. Black-box: written from the FP-035 brief only.
//! Every test passes with and without the `full-recompute` feature.
//!
//! Every mutation is checked to be observable (the fresh result before and after differs), so
//! a cache that kept serving the warm entries would fail.

#[path = "spec_layers_cache_support.rs"]
mod cache_support;

use std::collections::BTreeMap;

use cache_support::support::*;
use cache_support::*;
use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    ContinuousEffects, Evaluated, FULL_RECOMPUTE, Invalidation, LayerCache, LayerEnvironment,
    LayerError, evaluate, evaluate_all,
};
use fineprint_dsl::ability::Layer;

/// A warm cache over `(effects, env)`.
fn warm(effects: &ContinuousEffects, env: &CacheEnv) -> LayerCache {
    let mut c = LayerCache::new();
    assert_cache_fresh("warm-up", &mut c, effects, env);
    assert_cache_fresh("warm", &mut c, effects, env);
    if FULL_RECOMPUTE {
        assert_eq!(c.len(), 0);
        assert_eq!(c.stats().hits, 0);
    } else {
        assert_eq!(c.len(), env.objects().len());
        assert!(c.stats().hits > 0);
    }
    assert_eq!(c.stats().invalidations, 0);
    c
}

/// Declares `cause`, checks the version bump and emptiness, then requires fresh results.
fn declare_and_check(
    what: &str,
    cache: &mut LayerCache,
    cause: Invalidation,
    effects: &ContinuousEffects,
    env: &CacheEnv,
) {
    let before = cache.version();
    let inv = cache.stats().invalidations;
    let v = cache.invalidate(cause);
    assert_eq!(v.get(), before.get() + 1, "{what}: version +1");
    assert_eq!(cache.version(), v);
    assert_eq!(cache.stats().invalidations, inv + 1);
    assert_eq!(cache.len(), 0, "{what}: invalidation drops every entry");
    assert_cache_fresh(what, cache, effects, env);
}

fn fresh(effects: &ContinuousEffects, env: &CacheEnv) -> BTreeMap<ObjectId, Evaluated> {
    evaluate_all(effects, env).expect("fresh")
}

/// Runs `mutate` against a warm cache on the standard board and declares `cause`; requires the
/// change to be observable and the cache to equal a fresh evaluation afterwards.
fn env_change_scenario(cause: Invalidation, mutate: impl FnOnce(&mut CacheEnv)) {
    let mut env = standard_board();
    let effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    let before = fresh(&effects, &env);
    mutate(&mut env);
    let after = fresh(&effects, &env);
    assert_ne!(before, after, "{cause:?}: the mutation must be observable");
    declare_and_check(&format!("{cause:?}"), &mut cache, cause, &effects, &env);
    // the registry did not change: no extra invalidation
    assert_eq!(cache.stats().invalidations, 1, "{cause:?}");
}

/// REQ-LAYER-005: zone change: an object leaves the battlefield and another leaves the object
/// list (and a third enters).
#[test]
fn zone_change() {
    env_change_scenario(Invalidation::ZoneChange, |env| {
        env.set_on_battlefield(oid(ids::TAPPABLE), false);
        env.inner.bases.remove(&oid(ids::VANILLA));
        env.put(300, creature("test.newcomer_4_4", 4, 4), P2);
    });
}

/// REQ-LAYER-005: zone change: an object removed from `objects()` is then unknown to the cache
/// (`UnknownObject`), not served from a stale entry.
#[test]
fn zone_change_removed_object_is_unknown() {
    let mut env = standard_board();
    let effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    let gone = oid(ids::VANILLA);
    env.inner.bases.remove(&gone);
    cache.invalidate(Invalidation::ZoneChange);
    assert_eq!(
        cache.evaluate(&effects, &env, gone),
        Err(LayerError::UnknownObject(gone))
    );
    assert_eq!(
        evaluate(&effects, &env, gone),
        Err(LayerError::UnknownObject(gone))
    );
    assert_cache_fresh("after removal", &mut cache, &effects, &env);
}

/// REQ-LAYER-005: zone change of `on_battlefield` alone (the off-battlefield object comes in,
/// a battlefield object goes out), compared with a fresh evaluation.
#[test]
fn zone_change_on_battlefield_flag_only() {
    let mut env = standard_board();
    let effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    env.set_on_battlefield(oid(ids::OFF_FIELD), true);
    env.set_on_battlefield(oid(ids::HOST), false);
    declare_and_check(
        "on_battlefield",
        &mut cache,
        Invalidation::ZoneChange,
        &effects,
        &env,
    );
}

/// REQ-LAYER-005: control change in the base (the P2-only anthem starts applying).
#[test]
fn control_change() {
    env_change_scenario(Invalidation::ControlChange, |env| {
        env.set_controller(oid(ids::VANILLA), P2);
    });
}

/// REQ-LAYER-005: counter change: a +1/+1 counter is added and another removed.
#[test]
fn counter_change() {
    env_change_scenario(Invalidation::CounterChange, |env| {
        env.inner.counter(oid(ids::VANILLA), "+1/+1", 3, 60);
        env.inner.counters.remove(&oid(ids::COUNTERED));
    });
}

/// REQ-LAYER-005: designation change (a selector reads the designation).
#[test]
fn designation_change() {
    env_change_scenario(Invalidation::Designation, |env| {
        env.designate(oid(ids::DESIGNEE), false);
        env.designate(oid(ids::VANILLA), true);
    });
}

/// REQ-LAYER-005: attachment change (a selector reads `attached_here`).
#[test]
fn attachment_change() {
    env_change_scenario(Invalidation::Attachment, |env| {
        env.detach_all(oid(ids::HOST));
        env.attach(oid(ids::VANILLA), oid(ids::AURA));
    });
}

/// REQ-LAYER-005: status change (a selector reads the tapped flag).
#[test]
fn status_change() {
    env_change_scenario(Invalidation::Status, |env| {
        env.tap(oid(ids::TAPPABLE), false);
        env.tap(oid(ids::VANILLA), true);
    });
}

/// REQ-LAYER-005: player value change read by a CDA's `eval_value` only.
#[test]
fn player_value_change_read_by_cda() {
    env_change_scenario(Invalidation::PlayerValue, |env| {
        // P2 controls the CDA creature; 5 -> 8 stays below the condition threshold of 10
        env.set_player_value(P2, PV_RANK, 8);
    });
}

/// REQ-LAYER-005: player value change read by a condition (selector).
#[test]
fn player_value_change_read_by_condition() {
    env_change_scenario(Invalidation::PlayerValue, |env| {
        env.set_player_value(P1, PV_RANK, 11);
        env.set_player_value(P3, PV_RANK, 0);
    });
}

/// REQ-LAYER-005: timestamp change of a counter (a keyword counter against a layer-6
/// ability remover, CR 613.7c as cited by the existing REQ-LAYER-002 spec tests): flipping the
/// counter timestamp before/after the remover flips the result, in both directions.
#[test]
fn timestamp_change_counter_both_orders() {
    let mut env = standard_board();
    let effects = standard_effects(&env);
    let kw = oid(ids::KW_COUNTER);
    // standard board: counter at 5, remover at 30: the counter's flying is removed
    let mut cache = warm(&effects, &env);
    let early = fresh(&effects, &env);
    assert!(!has_kw(&early[&kw].characteristics, "flying"));
    env.inner.counter(kw, "flying", 1, 50);
    let late = fresh(&effects, &env);
    assert!(has_kw(&late[&kw].characteristics, "flying"), "later wins");
    declare_and_check(
        "counter later",
        &mut cache,
        Invalidation::Timestamp,
        &effects,
        &env,
    );
    env.inner.counter(kw, "flying", 1, 5);
    assert_eq!(fresh(&effects, &env), early);
    declare_and_check(
        "counter earlier",
        &mut cache,
        Invalidation::Timestamp,
        &effects,
        &env,
    );
    assert_eq!(cache.stats().invalidations, 2);
}

/// REQ-LAYER-005: timestamp change of an effect with `set_timestamp` (two layer-7b setters,
/// later timestamp wins, CR 613.7 as cited by the existing spec tests), in both orders. The
/// registry change is detected by the cache; the caller's declaration then bumps once more.
#[test]
fn timestamp_change_set_timestamp_both_orders() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let subject = oid(ids::TWO_SETTERS);
    let mut cache = warm(&effects, &env);
    let b_wins = fresh(&effects, &env);
    assert_eq!(pt(&b_wins[&subject]), spt(7, 7));
    for (a_ts, expected) in [(99, spt(1, 1)), (19, spt(7, 7))] {
        let v0 = cache.version().get();
        let i0 = cache.stats().invalidations;
        effects
            .set_timestamp(eid(eids::SETTER_A), ts(a_ts))
            .expect("known effect");
        let now = fresh(&effects, &env);
        assert_eq!(pt(&now[&subject]), expected);
        // detected without a declaration
        assert_eq!(
            cache.evaluate(&effects, &env, subject),
            Ok(now[&subject].clone())
        );
        assert_eq!(cache.version().get(), v0 + 1);
        assert_eq!(cache.stats().invalidations, i0 + 1);
        declare_and_check(
            &format!("set_timestamp {a_ts}"),
            &mut cache,
            Invalidation::Timestamp,
            &effects,
            &env,
        );
        assert_eq!(cache.version().get(), v0 + 2);
    }
}

/// REQ-LAYER-005: effect registration is detected without a declaration (version +1 exactly,
/// one invalidation), and a later declaration still gives fresh results.
#[test]
fn effect_registration_detected_without_declaration() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    let before = fresh(&effects, &env);
    effects
        .register(
            eid(600),
            eff(
                Layer::L4,
                70,
                by_sel(SEL_CREATURE),
                add_card_types(&["Artifact"]),
            ),
        )
        .expect("register");
    assert_ne!(fresh(&effects, &env), before, "observable");
    let v0 = cache.version().get();
    assert_cache_fresh("registered, undeclared", &mut cache, &effects, &env);
    assert_eq!(cache.version().get(), v0 + 1, "exactly one bump");
    assert_eq!(cache.stats().invalidations, 1);
    declare_and_check(
        "registered, declared",
        &mut cache,
        Invalidation::EffectRegistration,
        &effects,
        &env,
    );
    assert_eq!(cache.stats().invalidations, 2);
}

/// REQ-LAYER-005: a `register_group` (multi-layer lock-in effect) is detected without a
/// declaration.
#[test]
fn effect_group_registration_detected_without_declaration() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    effects.remove(eid(eids::GROUP_L4));
    effects.remove(eid(eids::GROUP_L7B));
    let mut cache = warm(&effects, &env);
    let before = fresh(&effects, &env);
    effects
        .register_group(&env, lockin_group(80))
        .expect("register_group");
    assert_ne!(fresh(&effects, &env), before, "observable");
    assert_cache_fresh("group registered", &mut cache, &effects, &env);
    assert_eq!(cache.version().get(), 1);
    assert_eq!(cache.stats().invalidations, 1);
    declare_and_check(
        "group declared",
        &mut cache,
        Invalidation::EffectRegistration,
        &effects,
        &env,
    );
}

/// REQ-LAYER-005: effect expiry (removal) is detected without a declaration (version +1
/// exactly), then declared.
#[test]
fn effect_expiry_detected_without_declaration() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    let before = fresh(&effects, &env);
    assert!(effects.remove(eid(eids::STEAL)));
    assert_ne!(fresh(&effects, &env), before, "observable");
    let v0 = cache.version().get();
    assert_cache_fresh("expired, undeclared", &mut cache, &effects, &env);
    assert_eq!(cache.version().get(), v0 + 1);
    assert_eq!(cache.stats().invalidations, 1);
    declare_and_check(
        "expired, declared",
        &mut cache,
        Invalidation::EffectExpiry,
        &effects,
        &env,
    );
    // removing the lock-in anchor (its linked part stays registered, applying to nothing)
    assert!(effects.remove(eid(eids::GROUP_L4)));
    assert_cache_fresh("anchor removed", &mut cache, &effects, &env);
    assert_eq!(cache.version().get(), v0 + 3);
}

/// REQ-LAYER-005: every `Invalidation::ALL` cause, one after another on one cache, each with its
/// own observable change; after each declaration the cache equals a fresh evaluation.
#[test]
fn every_cause_in_turn_on_one_cache() {
    let mut env = standard_board();
    let mut effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    for cause in Invalidation::ALL {
        let before = fresh(&effects, &env);
        match cause {
            Invalidation::ZoneChange => {
                env.inner.bases.remove(&oid(ids::LOCKED));
            }
            Invalidation::ControlChange => env.set_controller(oid(ids::FLIER), P2),
            Invalidation::CounterChange => env.inner.counter(oid(ids::HOST), "-1/-1", 1, 70),
            Invalidation::EffectRegistration => effects
                .register(
                    eid(700),
                    eff(Layer::L7c, 71, by_sel(SEL_ALL), modify_pt(0, 1)),
                )
                .expect("register"),
            Invalidation::EffectExpiry => {
                assert!(effects.remove(eid(eids::ANTHEM)));
            }
            Invalidation::Timestamp => env.inner.counter(oid(ids::KW_COUNTER), "flying", 1, 72),
            Invalidation::Designation => env.designate(oid(ids::HOST), true),
            Invalidation::Attachment => env.attach(oid(ids::FLIER), oid(ids::AURA)),
            Invalidation::Status => env.tap(oid(ids::HOST), true),
            Invalidation::PlayerValue => env.set_player_value(P2, PV_RANK, 30),
        }
        assert_ne!(fresh(&effects, &env), before, "{cause:?}: observable");
        cache.invalidate(*cause);
        assert_eq!(cache.len(), 0);
        assert_cache_fresh(&format!("{cause:?}"), &mut cache, &effects, &env);
    }
    // 10 declarations + 2 detected registry changes
    assert_eq!(cache.stats().invalidations, 12);
    assert_eq!(cache.version().get(), 12);
}

/// REQ-LAYER-005: an equal-by-value registry (a clone) never invalidates; a different registry
/// object with different content invalidates exactly once per change; switching back is a change
/// too.
#[test]
fn equal_registry_never_invalidates_different_registry_once_per_change() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    for _ in 0..3 {
        let clone = effects.clone();
        assert_cache_fresh("clone", &mut cache, &clone, &env);
    }
    assert_eq!(cache.stats().invalidations, 0);
    assert_eq!(cache.version().get(), 0);
    let mut other = effects.clone();
    other
        .register(
            eid(800),
            eff(Layer::L7c, 81, by_sel(SEL_ALL), modify_pt(2, 0)),
        )
        .expect("register");
    let entries = |c: &LayerCache| c.stats().invalidations;
    assert_cache_fresh("other", &mut cache, &other, &env);
    assert_eq!(entries(&cache), 1);
    assert_cache_fresh("other again", &mut cache, &other.clone(), &env);
    assert_eq!(entries(&cache), 1, "same content: no new invalidation");
    assert_cache_fresh("back to the first", &mut cache, &effects, &env);
    assert_eq!(entries(&cache), 2);
    assert_cache_fresh("first again", &mut cache, &effects, &env);
    assert_eq!(entries(&cache), 2);
    assert_eq!(cache.version().get(), 2);
    // a registry that differs only in an effect's timestamp is a different registry
    let mut retimed = effects.clone();
    retimed
        .set_timestamp(eid(eids::ANTHEM), ts(1))
        .expect("known");
    let r = cache.evaluate(&retimed, &env, oid(ids::VANILLA));
    assert_eq!(r, evaluate(&retimed, &env, oid(ids::VANILLA)));
    assert_eq!(entries(&cache), 3);
    if !FULL_RECOMPUTE {
        assert_eq!(cache.len(), 1);
    }
}

/// REQ-LAYER-005: the dependency pair (CR 613.8a, 613.8b as cited by the existing REQ-LAYER-002
/// spec tests) under the cache in both timestamp orders, switching the order with
/// `set_timestamp`: each order equals the fresh evaluation, and the two orders agree.
#[test]
fn dependency_pair_both_orders_under_cache() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let mut cache = warm(&effects, &env);
    let mut results = Vec::new();
    for (remover, dependent) in [(24, 25), (25, 24), (24, 25)] {
        effects
            .set_timestamp(eid(eids::DEP_REMOVER), ts(remover))
            .expect("known");
        effects
            .set_timestamp(eid(eids::DEP_DEPENDENT), ts(dependent))
            .expect("known");
        assert_cache_fresh(
            &format!("dep {remover}/{dependent}"),
            &mut cache,
            &effects,
            &env,
        );
        results.push(
            cache
                .evaluate(&effects, &env, oid(ids::FLIER))
                .expect("known"),
        );
    }
    assert_eq!(results[0], results[1], "dependency, not timestamp, decides");
    assert_eq!(results[0], results[2]);
}

/// REQ-LAYER-005: the state the cache reads is not mutated by evaluation (inputs unchanged).
#[test]
fn evaluation_leaves_inputs_unchanged() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let effects_before = effects.clone();
    let objects_before = env.objects();
    let mut cache = LayerCache::new();
    cache.evaluate_all(&effects, &env).expect("ok");
    cache.evaluate_all(&effects, &env).expect("ok");
    assert_eq!(effects, effects_before);
    assert_eq!(env.objects(), objects_before);
}
