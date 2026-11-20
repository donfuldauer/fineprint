// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the layer cache (REQ-LAYER-005, REQ-CORE-001): seeded pseudo-random
//! sequences of registry operations (register, register_group, remove, set_timestamp),
//! environment changes (each declared with the matching `Invalidation`) and queries of random
//! objects in random order; after every step every queried object equals a fresh
//! `layers::evaluate`, and the cached `evaluated_digest` equals the uncached one. Also
//! registration-order and query-order independence and the dependency/lock-in pairs in both
//! timestamp orders. Black-box: written from the FP-035 brief only; deterministic (seeded LCG,
//! no clock). Passes with and without the `full-recompute` feature.

#[path = "spec_layers_cache_support.rs"]
mod cache_support;

use cache_support::support::*;
use cache_support::*;
use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    ContinuousEffect, ContinuousEffects, FULL_RECOMPUTE, Invalidation, LayerCache,
    LayerEnvironment, LayerModification, evaluate, evaluate_all, evaluated_digest,
};
use fineprint_dsl::ability::Layer;

const SEEDS: [u64; 6] = [1, 2, 3, 17, 4242, 0xDEAD_BEEF];
const STEPS: usize = 60;
const MAX_EFFECTS: usize = 18;

fn pick<T: Copy>(rng: &mut Lcg, v: &[T]) -> T {
    v[usize::try_from(rng.below(v.len() as u64)).expect("fits")]
}

fn small(rng: &mut Lcg, n: u64) -> i64 {
    i64::try_from(rng.below(n)).expect("fits")
}

/// A random single-layer effect over the selectors of the cache environment.
fn random_effect(rng: &mut Lcg, objects: &[ObjectId]) -> ContinuousEffect {
    let affected = match rng.below(9) {
        0 => by_sel(SEL_CREATURE),
        1 => by_sel(SEL_TAPPED),
        2 => by_sel(SEL_DESIGNATED),
        3 => by_sel(SEL_HAS_ATTACHMENT),
        4 => by_sel(SEL_RANKED_CONTROLLER),
        5 => by_sel(SEL_HAS_FLYING),
        6 => by_sel(SEL_CONTROLLED_BY_P2),
        7 => by_sel(SEL_NONCREATURE_ENCHANTMENT),
        _ => {
            let a = pick(rng, objects);
            let b = pick(rng, objects);
            objs(&[a, b])
        }
    };
    let t = 1 + rng.below(200);
    let (layer, m) = match rng.below(10) {
        0 => (
            Layer::L2,
            LayerModification::SetController(pick(rng, &PLAYERS)),
        ),
        1 => (Layer::L4, add_card_types(&["Creature"])),
        2 => (Layer::L4, add_card_types(&["Artifact"])),
        3 => (
            Layer::L5,
            LayerModification::AddColors(mask(1 << rng.below(5))),
        ),
        4 => (
            Layer::L6,
            LayerModification::RemoveKeyword("flying".to_string()),
        ),
        5 => (
            Layer::L6,
            LayerModification::AddAbility(kw_ability("test.prop.flying", "flying")),
        ),
        6 => (Layer::L6, LayerModification::RemoveAllAbilities),
        7 => (
            Layer::L7b,
            set_pt(Some(small(rng, 6)), Some(1 + small(rng, 6))),
        ),
        8 => (Layer::L7c, modify_pt(small(rng, 5) - 2, small(rng, 5) - 2)),
        _ => (Layer::L7d, LayerModification::SwitchPt),
    };
    eff(layer, t, affected, m)
}

/// A random environment change and the cause the caller must declare.
fn random_env_change(rng: &mut Lcg, env: &mut CacheEnv, next_id: &mut u64) -> Invalidation {
    let objects = env.objects();
    let any = pick(rng, &objects);
    match rng.below(10) {
        0 => {
            if rng.below(2) == 0 && objects.len() > 8 {
                env.inner.bases.remove(&any);
            } else if rng.below(2) == 0 {
                let on = env.inner.bases[&any].on_battlefield;
                env.set_on_battlefield(any, !on);
            } else {
                *next_id += 1;
                env.put(
                    *next_id,
                    creature(&format!("test.prop_new_{next_id}"), 1, 1),
                    pick(rng, &PLAYERS),
                );
            }
            Invalidation::ZoneChange
        }
        1 => {
            env.set_controller(any, pick(rng, &PLAYERS));
            Invalidation::ControlChange
        }
        2 => {
            let kind = pick(rng, &["+1/+1", "-1/-1", "flying"]);
            if rng.below(3) == 0 {
                env.inner.counters.remove(&any);
            } else {
                env.inner
                    .counter(any, kind, 1 + small(rng, 3), rng.below(200));
            }
            Invalidation::CounterChange
        }
        3 => {
            // re-stamp every counter on `any`
            let kinds: Vec<String> = env.inner.counters(any).keys().cloned().collect();
            for k in kinds {
                env.inner.counter_ts.insert((any, k), ts(rng.below(200)));
            }
            Invalidation::Timestamp
        }
        4 => {
            let on = rng.below(2) == 0;
            env.designate(any, on);
            Invalidation::Designation
        }
        5 => {
            if rng.below(2) == 0 {
                env.detach_all(any);
            } else {
                let other = pick(rng, &objects);
                env.attach(any, other);
            }
            Invalidation::Attachment
        }
        6 | 7 => {
            let on = rng.below(2) == 0;
            env.tap(any, on);
            Invalidation::Status
        }
        _ => {
            env.set_player_value(pick(rng, &PLAYERS), PV_RANK, small(rng, 20));
            Invalidation::PlayerValue
        }
    }
}

/// Queries random objects in random order and checks each against a fresh evaluation, then
/// checks the whole board and its digest.
fn check_step(
    what: &str,
    rng: &mut Lcg,
    cache: &mut LayerCache,
    effects: &ContinuousEffects,
    env: &CacheEnv,
) {
    let mut objects = env.objects();
    rng.shuffle(&mut objects);
    let k = 1 + usize::try_from(rng.below(objects.len() as u64)).expect("fits");
    let fresh = evaluate_all(effects, env).expect("fresh");
    for id in objects.iter().take(k) {
        let expected = Ok(fresh[id].clone());
        assert_eq!(
            cache.evaluate(effects, env, *id),
            expected,
            "{what}: object {id:?}"
        );
        assert_eq!(
            expected,
            evaluate(effects, env, *id),
            "{what}: per-object reference"
        );
        // a repeat hits in cached mode and is identical in both modes
        assert_eq!(
            cache.evaluate(effects, env, *id),
            expected,
            "{what}: repeat"
        );
    }
    let uncached = evaluated_digest(&fresh);
    let cached = cached_digest(cache, effects, env);
    assert_eq!(cached, uncached, "{what}: digest");
    if FULL_RECOMPUTE {
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.stats().hits, 0);
    } else {
        assert_eq!(cache.len(), objects.len());
    }
}

/// REQ-LAYER-005, REQ-CORE-001: random interleavings of registry operations, declared
/// environment changes and queries; the cache always equals a fresh evaluation, and the
/// version and invalidation count follow the declarations and detected registry changes.
#[test]
fn random_sequences_match_fresh_evaluation() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed);
        let mut env = big_board(24 + rng.below(8), seed);
        let mut effects = big_effects(&env, 24);
        let mut cache = LayerCache::new();
        let mut next_eid = 1000u64;
        let mut next_oid = 5000u64;
        let mut expected_invalidations = 0u64;
        check_step(
            &format!("seed {seed} start"),
            &mut rng,
            &mut cache,
            &effects,
            &env,
        );
        for step in 0..STEPS {
            let what = format!("seed {seed} step {step}");
            let snapshot = effects.clone();
            let objects = env.objects();
            // keep the registry small enough for the dependency analysis to stay fast: past
            // `MAX_EFFECTS` a registration draw becomes a removal draw
            let mut op = rng.below(7);
            if effects.len() >= MAX_EFFECTS && op <= 2 {
                op = 3;
            }
            match op {
                0 | 1 => {
                    next_eid += 1;
                    effects
                        .register(eid(next_eid), random_effect(&mut rng, &objects))
                        .expect("register");
                }
                2 => {
                    next_eid += 2;
                    // a multi-layer lock-in group (CR 613.6, as cited by the lock-in spec tests)
                    let t = 1 + rng.below(200);
                    let anchor_sel =
                        pick(&mut rng, &[SEL_CREATURE, SEL_TAPPED, SEL_WHITE_CREATURE]);
                    let parts = vec![
                        (
                            eid(next_eid - 1),
                            eff(
                                Layer::L4,
                                t,
                                by_sel(anchor_sel),
                                add_card_types(&["Artifact"]),
                            ),
                        ),
                        (
                            eid(next_eid),
                            eff(
                                Layer::L7b,
                                t,
                                fineprint_core::layers::Affected::SameAsLayer(Layer::L4),
                                set_pt(Some(small(&mut rng, 5)), Some(1 + small(&mut rng, 5))),
                            ),
                        ),
                    ];
                    effects.register_group(&env, parts).expect("register_group");
                }
                3 => {
                    let ids: Vec<_> = effects.iter().map(|(id, _)| id).collect();
                    if !ids.is_empty() {
                        let victim = pick(&mut rng, &ids);
                        assert!(effects.remove(victim));
                    }
                }
                4 => {
                    let ids: Vec<_> = effects.iter().map(|(id, _)| id).collect();
                    if !ids.is_empty() {
                        let target = pick(&mut rng, &ids);
                        effects
                            .set_timestamp(target, ts(1 + rng.below(200)))
                            .expect("known");
                    }
                }
                _ => {
                    let cause = random_env_change(&mut rng, &mut env, &mut next_oid);
                    let v = cache.version().get();
                    let nv = cache.invalidate(cause);
                    assert_eq!(nv.get(), v + 1);
                    assert_eq!(cache.len(), 0);
                    expected_invalidations += 1;
                }
            }
            if effects != snapshot {
                expected_invalidations += 1;
            }
            check_step(&what, &mut rng, &mut cache, &effects, &env);
            assert_eq!(
                cache.stats().invalidations,
                expected_invalidations,
                "{what}: invalidation count"
            );
            assert_eq!(
                cache.version().get(),
                expected_invalidations,
                "{what}: version"
            );
        }
    }
}

/// REQ-LAYER-005, REQ-CORE-001: the same effect set registered in two different orders yields
/// identical cached results and `evaluated_digest`, equal to the uncached digest.
#[test]
fn registration_order_independence() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed ^ 0x5555);
        let env = big_board(30, seed);
        let objects = env.objects();
        let list: Vec<(u64, ContinuousEffect)> = (1..=12)
            .map(|i| (i, random_effect(&mut rng, &objects)))
            .collect();
        let forward = registry(&list);
        let mut rev = list.clone();
        rev.reverse();
        let backward = registry(&rev);
        let mut shuffled = list.clone();
        rng.shuffle(&mut shuffled);
        let mixed = registry(&shuffled);
        let mut c1 = LayerCache::new();
        let mut c2 = LayerCache::new();
        let mut c3 = LayerCache::new();
        let d1 = cached_digest(&mut c1, &forward, &env);
        let d2 = cached_digest(&mut c2, &backward, &env);
        let d3 = cached_digest(&mut c3, &mixed, &env);
        assert_eq!(d1, d2, "seed {seed}");
        assert_eq!(d1, d3, "seed {seed}");
        assert_eq!(d1, fresh_digest(&forward, &env));
        // one cache across the reorderings: the registries are equal, so no invalidation
        let mut one = LayerCache::new();
        assert_eq!(cached_digest(&mut one, &forward, &env), d1);
        assert_eq!(cached_digest(&mut one, &backward, &env), d1);
        assert_eq!(cached_digest(&mut one, &mixed, &env), d1);
        assert_eq!(
            one.stats().invalidations,
            0,
            "equal registries never invalidate"
        );
    }
}

/// REQ-LAYER-005, REQ-CORE-001: evaluating objects in ascending or descending (or shuffled)
/// order through the cache gives the same results, equal to `layers::evaluate`.
#[test]
fn query_order_independence() {
    for seed in SEEDS {
        let env = big_board(40, seed);
        let effects = big_effects(&env, 40);
        let objects = env.objects();
        let mut asc = LayerCache::new();
        let mut desc = LayerCache::new();
        let mut shuf = LayerCache::new();
        let a: Vec<_> = objects
            .iter()
            .map(|id| asc.evaluate(&effects, &env, *id).expect("known"))
            .collect();
        let mut d: Vec<_> = objects
            .iter()
            .rev()
            .map(|id| desc.evaluate(&effects, &env, *id).expect("known"))
            .collect();
        d.reverse();
        assert_eq!(a, d, "seed {seed}");
        let mut order: Vec<usize> = (0..objects.len()).collect();
        Lcg::new(seed).shuffle(&mut order);
        let mut s = vec![None; objects.len()];
        for i in order {
            s[i] = Some(shuf.evaluate(&effects, &env, objects[i]).expect("known"));
        }
        let s: Vec<_> = s.into_iter().map(|x| x.expect("filled")).collect();
        assert_eq!(a, s, "seed {seed}");
        for (id, got) in objects.iter().zip(&a) {
            assert_eq!(Ok(got.clone()), evaluate(&effects, &env, *id));
        }
        assert_eq!(
            cached_digest(&mut asc, &effects, &env),
            cached_digest(&mut desc, &effects, &env)
        );
    }
}

/// REQ-LAYER-005, REQ-CORE-001: the dependency pair (CR 613.8a, 613.8b) and the lock-in group
/// (CR 613.6) in both timestamp orders, each under a fresh cache and under one warm cache that
/// is switched between the orders; every result equals the uncached one.
#[test]
fn dependency_and_lockin_pairs_both_timestamp_orders() {
    let env = standard_board();
    let mut shared = LayerCache::new();
    let mut dep_digests = Vec::new();
    for (r, d) in [(24, 25), (25, 24)] {
        let mut effects = standard_effects(&env);
        effects
            .set_timestamp(eid(eids::DEP_REMOVER), ts(r))
            .expect("known");
        effects
            .set_timestamp(eid(eids::DEP_DEPENDENT), ts(d))
            .expect("known");
        let mut own = LayerCache::new();
        assert_cache_fresh(&format!("dep {r}/{d} own"), &mut own, &effects, &env);
        assert_cache_fresh(&format!("dep {r}/{d} shared"), &mut shared, &effects, &env);
        dep_digests.push(cached_digest(&mut own, &effects, &env));
    }
    assert_eq!(
        dep_digests[0], dep_digests[1],
        "dependency decides, not timestamps"
    );

    // the lock-in group earlier or later than the layer-4 animation and the 7b setters
    let mut lock_digests = Vec::new();
    for group_ts in [5u64, 95] {
        let mut effects = standard_effects(&env);
        effects
            .set_timestamp(eid(eids::GROUP_L4), ts(group_ts))
            .expect("known");
        effects
            .set_timestamp(eid(eids::GROUP_L7B), ts(group_ts))
            .expect("known");
        // a competing 7b setter on every white creature, between the two group timestamps
        effects
            .register(
                eid(900),
                eff(
                    Layer::L7b,
                    50,
                    by_sel(SEL_WHITE_CREATURE),
                    set_pt(Some(0), Some(9)),
                ),
            )
            .expect("register");
        let mut own = LayerCache::new();
        assert_cache_fresh(&format!("lock {group_ts} own"), &mut own, &effects, &env);
        assert_cache_fresh(
            &format!("lock {group_ts} shared"),
            &mut shared,
            &effects,
            &env,
        );
        lock_digests.push(cached_digest(&mut own, &effects, &env));
    }
    assert_ne!(
        lock_digests[0], lock_digests[1],
        "the later 7b setter wins (CR 613.7), so the orders differ"
    );
}

/// REQ-LAYER-005, REQ-CORE-001: running the same seeded sequence twice yields the same digest
/// sequence (determinism of the cache over identical inputs).
#[test]
fn same_sequence_same_digests() {
    let run = || {
        let mut rng = Lcg::new(99);
        let mut env = big_board(30, 99);
        let effects = big_effects(&env, 30);
        let mut cache = LayerCache::new();
        let mut next = 7000u64;
        let mut out = Vec::new();
        for _ in 0..20 {
            let cause = random_env_change(&mut rng, &mut env, &mut next);
            cache.invalidate(cause);
            out.push(cached_digest(&mut cache, &effects, &env));
        }
        (out, cache.stats(), cache.version())
    };
    assert_eq!(run(), run());
}
