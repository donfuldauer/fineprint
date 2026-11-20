// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the layer cache (REQ-LAYER-005, SRS 6.5): `StateVersion`, `Invalidation`,
//! `CacheStats`, `LayerCache` bookkeeping (version, hits, misses, invalidations, entries),
//! agreement with the uncached `layers::evaluate` / `layers::evaluate_all`, error handling and
//! `evaluated_digest`. Black-box: written from the FP-035 brief only. Every test passes with and
//! without the `full-recompute` feature: assertions that differ by mode branch on
//! `FULL_RECOMPUTE`.

#[path = "spec_layers_cache_support.rs"]
mod cache_support;

use std::collections::{BTreeMap, BTreeSet};

use cache_support::support::*;
use cache_support::*;
use fineprint_base::ids::{ObjectId, Timestamp};
use fineprint_core::layers::{
    CacheStats, ContinuousEffects, Evaluated, FULL_RECOMPUTE, Invalidation, LayerCache,
    LayerEnvironment, LayerError, LayerModification, MatchContext, ObjectBase, StateVersion,
    evaluate, evaluate_all, evaluated_digest,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::expr::ObjectSelectorAST;

/// REQ-LAYER-005: `FULL_RECOMPUTE` mirrors the cargo feature `full-recompute`.
#[test]
fn full_recompute_constant_matches_the_feature() {
    assert_eq!(FULL_RECOMPUTE, cfg!(feature = "full-recompute"));
}

/// REQ-LAYER-005: a new cache is at version 0, empty, with zero stats; `Default` agrees with
/// `new`.
#[test]
fn new_cache_starts_at_version_zero_and_default_agrees() {
    let c = LayerCache::new();
    assert_eq!(c.version().get(), 0);
    assert_eq!(c.version(), StateVersion::default());
    assert_eq!(c.stats(), CacheStats::default());
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 0,
            misses: 0,
            invalidations: 0
        }
    );
    assert_eq!(c.len(), 0);
    assert!(c.is_empty());
    let d = LayerCache::default();
    assert_eq!(d.version(), c.version());
    assert_eq!(d.stats(), c.stats());
    assert_eq!(d.len(), c.len());
    assert_eq!(StateVersion::default().get(), 0);
}

/// REQ-LAYER-005: `Invalidation::ALL` lists the 10 variants, distinct, in declaration order.
#[test]
fn invalidation_all_has_every_variant_once_in_order() {
    let expected = [
        Invalidation::ZoneChange,
        Invalidation::ControlChange,
        Invalidation::CounterChange,
        Invalidation::EffectRegistration,
        Invalidation::EffectExpiry,
        Invalidation::Timestamp,
        Invalidation::Designation,
        Invalidation::Attachment,
        Invalidation::Status,
        Invalidation::PlayerValue,
    ];
    assert_eq!(Invalidation::ALL, &expected[..]);
    let distinct: BTreeSet<Invalidation> = Invalidation::ALL.iter().copied().collect();
    assert_eq!(distinct.len(), 10);
    // exhaustive match: a new variant breaks compilation here, reminding the list owner.
    for cause in Invalidation::ALL {
        match cause {
            Invalidation::ZoneChange
            | Invalidation::ControlChange
            | Invalidation::CounterChange
            | Invalidation::EffectRegistration
            | Invalidation::EffectExpiry
            | Invalidation::Timestamp
            | Invalidation::Designation
            | Invalidation::Attachment
            | Invalidation::Status
            | Invalidation::PlayerValue => {}
        }
    }
}

/// REQ-LAYER-005: every `invalidate(cause)` bumps the version by exactly 1, returns the new
/// version and counts one invalidation, in both modes; versions order by their number.
#[test]
fn every_invalidation_bumps_version_by_one_and_counts() {
    let mut c = LayerCache::new();
    let mut previous = c.version();
    for (i, cause) in Invalidation::ALL.iter().enumerate() {
        let returned = c.invalidate(*cause);
        let n = u64::try_from(i).expect("fits") + 1;
        assert_eq!(returned.get(), n, "{cause:?}");
        assert_eq!(c.version(), returned, "{cause:?}");
        assert!(returned > previous, "{cause:?}: StateVersion ordering");
        assert_eq!(c.stats().invalidations, n, "{cause:?}");
        assert_eq!(c.stats().hits, 0);
        assert_eq!(c.stats().misses, 0);
        previous = returned;
    }
    // twice the same cause still counts twice
    let a = c.invalidate(Invalidation::Status);
    let b = c.invalidate(Invalidation::Status);
    assert_eq!(b.get(), a.get() + 1);
    assert_eq!(c.stats().invalidations, 12);
    let mut sorted = [b, StateVersion::default(), a];
    sorted.sort();
    assert_eq!(sorted, [StateVersion::default(), a, b]);
}

/// REQ-LAYER-005: repeated evaluation of one object: cached mode misses once then hits;
/// full-recompute mode misses every time, never hits and stores nothing. The value is always
/// the fresh `layers::evaluate`.
#[test]
fn hit_and_miss_counting_for_one_object() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let id = oid(ids::TWO_SETTERS);
    let expected = evaluate(&effects, &env, id).expect("fresh");
    let mut c = LayerCache::new();
    for k in 1..=5u64 {
        assert_eq!(c.evaluate(&effects, &env, id), Ok(expected.clone()));
        let s = c.stats();
        if FULL_RECOMPUTE {
            assert_eq!(s.hits, 0);
            assert_eq!(s.misses, k);
            assert_eq!(c.len(), 0);
            assert!(c.is_empty());
        } else {
            assert_eq!(s.misses, 1);
            assert_eq!(s.hits, k - 1);
            assert_eq!(c.len(), 1);
            assert!(!c.is_empty());
        }
        assert_eq!(s.invalidations, 0, "the first call is not an invalidation");
        assert_eq!(c.version().get(), 0);
    }
}

/// REQ-LAYER-005: in cached mode `len` grows by exactly one per distinct object evaluated and
/// not on repeats; in full-recompute mode it stays 0.
#[test]
fn len_grows_by_one_per_distinct_object() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let mut c = LayerCache::new();
    let objects = env.objects();
    for (i, id) in objects.iter().enumerate() {
        c.evaluate(&effects, &env, *id).expect("known");
        c.evaluate(&effects, &env, *id).expect("known");
        if FULL_RECOMPUTE {
            assert_eq!(c.len(), 0);
        } else {
            assert_eq!(c.len(), i + 1);
        }
    }
    let n = u64::try_from(objects.len()).expect("fits");
    if FULL_RECOMPUTE {
        assert_eq!(c.stats().misses, 2 * n);
        assert_eq!(c.stats().hits, 0);
    } else {
        assert_eq!(c.stats().misses, n);
        assert_eq!(c.stats().hits, n);
    }
}

/// REQ-LAYER-005, REQ-CORE-001: `evaluate_all` equals `layers::evaluate_all` cold, warm and
/// after an invalidation; an invalidation empties the cache.
#[test]
fn evaluate_all_matches_uncached_cold_warm_and_after_invalidation() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let expected = evaluate_all(&effects, &env).expect("fresh");
    let n = env.objects().len();
    let mut c = LayerCache::new();
    assert_eq!(c.evaluate_all(&effects, &env), Ok(expected.clone()), "cold");
    let after_cold = c.stats();
    if FULL_RECOMPUTE {
        assert_eq!(c.len(), 0);
    } else {
        assert_eq!(c.len(), n);
        assert_eq!(after_cold.misses, u64::try_from(n).expect("fits"));
    }
    assert_eq!(c.evaluate_all(&effects, &env), Ok(expected.clone()), "warm");
    if FULL_RECOMPUTE {
        assert_eq!(c.stats().hits, 0);
        assert_eq!(c.stats().misses, 2 * u64::try_from(n).expect("fits"));
    } else {
        assert_eq!(c.stats().hits, u64::try_from(n).expect("fits"));
        assert_eq!(c.stats().misses, after_cold.misses);
    }
    for cause in Invalidation::ALL {
        let v = c.invalidate(*cause);
        assert_eq!(c.len(), 0, "{cause:?}: invalidation drops every entry");
        assert_eq!(
            c.evaluate_all(&effects, &env),
            Ok(expected.clone()),
            "{cause:?}"
        );
        assert_eq!(c.version(), v, "an equal registry never invalidates");
    }
    assert_eq!(
        c.stats().invalidations,
        u64::try_from(Invalidation::ALL.len()).expect("fits")
    );
}

/// REQ-LAYER-005: an unknown object gives `LayerError::UnknownObject(id)` cold and warm, is
/// never stored and counts as a miss.
#[test]
fn unknown_object_is_an_error_never_stored() {
    let env = standard_board();
    let effects = standard_effects(&env);
    let missing = oid(999_999);
    assert_eq!(
        evaluate(&effects, &env, missing),
        Err(LayerError::UnknownObject(missing))
    );
    let mut c = LayerCache::new();
    assert_eq!(
        c.evaluate(&effects, &env, missing),
        Err(LayerError::UnknownObject(missing)),
        "cold"
    );
    assert_eq!(c.len(), 0);
    assert_eq!(c.stats().misses, 1);
    c.evaluate_all(&effects, &env).expect("warm up");
    let len = c.len();
    let misses = c.stats().misses;
    let hits = c.stats().hits;
    assert_eq!(
        c.evaluate(&effects, &env, missing),
        Err(LayerError::UnknownObject(missing)),
        "warm"
    );
    assert_eq!(c.len(), len, "an error is never stored");
    assert_eq!(c.stats().misses, misses + 1, "an error counts as a miss");
    assert_eq!(c.stats().hits, hits);
    assert_eq!(
        c.evaluate(&effects, &env, missing),
        Err(LayerError::UnknownObject(missing)),
        "again"
    );
    assert_eq!(c.stats().misses, misses + 2);
    assert_eq!(c.stats().invalidations, 0);
}

/// An environment listing an object it has no base for (to exercise the first-error rule).
struct ListsGhosts {
    env: CacheEnv,
    ghosts: Vec<u64>,
}

impl LayerEnvironment for ListsGhosts {
    fn objects(&self) -> Vec<ObjectId> {
        let mut v = self.env.objects();
        v.extend(self.ghosts.iter().map(|g| oid(*g)));
        v.sort();
        v
    }
    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.env.base(id)
    }
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.env.counters(id)
    }
    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.env.counter_timestamp(id, kind)
    }
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.env.attached_here(id)
    }
    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.env.matches(selector, ctx)
    }
    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.env.eval_value(expr, object)
    }
}

/// REQ-LAYER-005: `evaluate_all` returns the same first error as `layers::evaluate_all`, cold
/// and warm, and stores no error.
#[test]
fn evaluate_all_returns_the_first_error_like_uncached() {
    let env = ListsGhosts {
        env: standard_board(),
        ghosts: vec![50, 60],
    };
    let effects = standard_effects(&env.env);
    let expected = evaluate_all(&effects, &env);
    assert!(expected.is_err(), "the reference reports an error");
    let mut c = LayerCache::new();
    assert_eq!(c.evaluate_all(&effects, &env), expected, "cold");
    assert_eq!(c.evaluate_all(&effects, &env), expected, "warm");
    assert_eq!(
        c.evaluate(&effects, &env, oid(50)),
        Err(LayerError::UnknownObject(oid(50)))
    );
    // no error is stored: at most the real objects are cached
    assert!(c.len() <= env.env.objects().len());
    if FULL_RECOMPUTE {
        assert_eq!(c.len(), 0);
    }
}

/// REQ-LAYER-005: the first ever evaluation is not an invalidation; an equal registry never
/// invalidates; a changed registry invalidates once (`EffectRegistration`), in both modes.
#[test]
fn first_call_is_not_an_invalidation_and_registry_change_is() {
    let env = standard_board();
    let mut effects = standard_effects(&env);
    let mut c = LayerCache::new();
    c.evaluate(&effects, &env, oid(ids::VANILLA))
        .expect("known");
    assert_eq!(c.version().get(), 0);
    assert_eq!(c.stats().invalidations, 0);
    let clone = effects.clone();
    c.evaluate(&clone, &env, oid(ids::VANILLA)).expect("known");
    assert_eq!(c.version().get(), 0, "an equal registry never invalidates");
    assert_eq!(c.stats().invalidations, 0);
    effects
        .register(
            eid(500),
            eff(Layer::L7c, 90, by_sel(SEL_ALL), modify_pt(1, 0)),
        )
        .expect("register");
    let got = c.evaluate(&effects, &env, oid(ids::VANILLA));
    assert_eq!(got, evaluate(&effects, &env, oid(ids::VANILLA)));
    assert_eq!(c.version().get(), 1);
    assert_eq!(c.stats().invalidations, 1);
    if !FULL_RECOMPUTE {
        assert_eq!(c.len(), 1, "dropped, then the one new entry");
    }
    c.evaluate(&effects, &env, oid(ids::VANILLA))
        .expect("known");
    assert_eq!(c.version().get(), 1);
    assert_eq!(c.stats().invalidations, 1);
}

/// REQ-LAYER-005: a cache warmed on one board answers a different board (different registry)
/// correctly; an empty registry and an empty board work.
#[test]
fn empty_registry_and_empty_board() {
    let empty_env = CacheEnv::new();
    let none = ContinuousEffects::new();
    let mut c = LayerCache::new();
    assert_eq!(c.evaluate_all(&none, &empty_env), Ok(BTreeMap::new()));
    assert_eq!(c.len(), 0);
    let env = standard_board();
    assert_eq!(
        c.evaluate_all(&none, &env),
        evaluate_all(&none, &env),
        "same (empty) registry, the caller declares the zone change"
    );
    c.invalidate(Invalidation::ZoneChange);
    assert_eq!(c.evaluate_all(&none, &env), evaluate_all(&none, &env));
    let effects = standard_effects(&env);
    let v = c.version();
    assert_eq!(c.evaluate_all(&effects, &env), evaluate_all(&effects, &env));
    assert_eq!(c.version().get(), v.get() + 1);
}

// ---------------------------------------------------------------------------------------------
// evaluated_digest
// ---------------------------------------------------------------------------------------------

fn board_map() -> BTreeMap<ObjectId, Evaluated> {
    let env = standard_board();
    let effects = standard_effects(&env);
    evaluate_all(&effects, &env).expect("fresh")
}

/// REQ-LAYER-005, REQ-CORE-001: equal maps give equal digests; the empty map's digest is stable;
/// the digest does not depend on how the map was built.
#[test]
fn digest_of_equal_maps_is_equal_and_build_order_free() {
    let a = board_map();
    let b = board_map();
    assert_eq!(evaluated_digest(&a), evaluated_digest(&b));
    assert_eq!(evaluated_digest(&a), evaluated_digest(&a.clone()));
    let empty = BTreeMap::new();
    let e1 = evaluated_digest(&empty);
    let e2 = evaluated_digest(&BTreeMap::new());
    assert_eq!(e1, e2);
    assert_ne!(e1, evaluated_digest(&a));
    // build the same map in descending insertion order and from a shuffled list
    let mut rev = BTreeMap::new();
    for (k, v) in a.iter().rev() {
        rev.insert(*k, v.clone());
    }
    let mut pairs: Vec<_> = a.iter().map(|(k, v)| (*k, v.clone())).collect();
    Lcg::new(7).shuffle(&mut pairs);
    let shuffled: BTreeMap<_, _> = pairs.into_iter().collect();
    assert_eq!(evaluated_digest(&rev), evaluated_digest(&a));
    assert_eq!(evaluated_digest(&shuffled), evaluated_digest(&a));
}

/// REQ-LAYER-005: changing only a controller, only a power, only an added keyword, only the set
/// of objects, or only the id of an entry changes the digest.
#[test]
fn digest_changes_with_any_single_change() {
    let a = board_map();
    let base = evaluated_digest(&a);
    let id = oid(ids::VANILLA);

    let mut controller = a.clone();
    let e = controller.get_mut(&id).expect("present");
    e.controller = Some(if e.controller == Some(P4) { P3 } else { P4 });
    assert_ne!(evaluated_digest(&controller), base, "controller");

    let mut controller_none = a.clone();
    controller_none.get_mut(&id).expect("present").controller = None;
    assert_ne!(evaluated_digest(&controller_none), base, "controller None");

    let mut power = a.clone();
    let e = power.get_mut(&id).expect("present");
    let p = e
        .characteristics
        .power
        .as_ref()
        .and_then(literal_of)
        .expect("literal power");
    e.characteristics.power = Some(fineprint_core::layers::literal(p + 1));
    assert_ne!(evaluated_digest(&power), base, "power");

    let mut keyword = a.clone();
    keyword
        .get_mut(&id)
        .expect("present")
        .characteristics
        .abilities
        .push(kw_ability("test.digest.trample", "trample"));
    assert_ne!(evaluated_digest(&keyword), base, "keyword");

    let mut more = a.clone();
    let extra = a[&id].clone();
    more.insert(oid(77_777), extra.clone());
    assert_ne!(evaluated_digest(&more), base, "one more object");

    let mut fewer = a.clone();
    fewer.remove(&id);
    assert_ne!(evaluated_digest(&fewer), base, "one object fewer");

    let mut moved = a.clone();
    let v = moved.remove(&id).expect("present");
    moved.insert(oid(88_888), v);
    assert_ne!(evaluated_digest(&moved), base, "entry under another id");

    // swapping the values of two entries changes the digest too
    let mut swapped = a.clone();
    let x = swapped[&oid(ids::VANILLA)].clone();
    let y = swapped[&oid(ids::FLIER)].clone();
    assert_ne!(x, y);
    swapped.insert(oid(ids::VANILLA), y);
    swapped.insert(oid(ids::FLIER), x);
    assert_ne!(evaluated_digest(&swapped), base, "swapped values");
}

/// REQ-LAYER-005: a one-element map over a modification of each layer-7 kind digests
/// differently from the unmodified one (the encoding covers power and toughness).
#[test]
fn digest_covers_toughness() {
    let mut env = CacheEnv::new();
    env.put(1, creature("test.digest_2_2", 2, 2), P1);
    let plain = evaluate_all(&ContinuousEffects::new(), &env).expect("fresh");
    let r = registry(&[(
        1,
        eff(
            Layer::L7c,
            1,
            objs(&[oid(1)]),
            LayerModification::ModifyPt {
                power: 0,
                toughness: 1,
            },
        ),
    )]);
    let tougher = evaluate_all(&r, &env).expect("fresh");
    assert_ne!(plain, tougher);
    assert_ne!(evaluated_digest(&plain), evaluated_digest(&tougher));
}
