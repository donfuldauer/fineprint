// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the layer cache: REQ-LAYER-005 (cache keyed by (stateVersion, ObjectId), sound
// invalidation, full-recompute mode, digest for the differential test), REQ-CORE-001
// (determinism). Every test passes in both modes: `cargo test -p fineprint-core` and
// `cargo test -p fineprint-core --features full-recompute`. Synthetic objects only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};

use fineprint_dsl::mana::ManaCostDSL;

use super::*;
use crate::objects::{Characteristics, ManaValueContext};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// The synthetic creature `name` with the given P/T.
fn creature(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: set(&["Bear"]),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The synthetic artifact `name` without P/T.
fn artifact(name: &str) -> Characteristics {
    Characteristics {
        card_types: set(&["Artifact"]),
        subtypes: BTreeSet::new(),
        power: None,
        toughness: None,
        colors: ColorBitmask::NONE,
        ..creature(name, 0, 0)
    }
}

/// A creature whose power and toughness are a characteristic-defining expression (CR 604.3)
/// whose value the environment supplies per object.
fn cda_creature(name: &str) -> Characteristics {
    let expr = ValueExpressionAST::Memory("test.cda".to_owned());
    Characteristics {
        power: Some(expr.clone()),
        toughness: Some(expr),
        ..creature(name, 0, 0)
    }
}

fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

fn is_creature() -> ObjectSelectorAST {
    filter(PredicateAST::IsCreature)
}

fn tapped() -> ObjectSelectorAST {
    filter(PredicateAST::Tapped)
}

fn designated() -> ObjectSelectorAST {
    filter(PredicateAST::HasDesignation {
        kind: "test.designation".to_owned(),
    })
}

fn artifacts() -> ObjectSelectorAST {
    filter(PredicateAST::HasCardType {
        value: WordRef::Name("Artifact".to_owned()),
    })
}

/// A synthetic keyword ability with the keyword name of `word` ([`keyword_id`]).
fn keyword(word: &str) -> AbilityAST {
    AbilityAST::Keyword(fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new(format!("test.{word}")),
        keyword_id(word),
    ))
}

/// A resolved effect controlled by player 1 without a source.
fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

/// A static effect of `source`.
fn stat(
    source: ObjectId,
    layer: Layer,
    t: u64,
    sel: ObjectSelectorAST,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        origin: EffectOrigin::Static { granted_at: None },
        ..eff(layer, t, Affected::Selector(sel), m)
    }
}

fn modify(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

fn locked(ids: &[u64]) -> Affected {
    Affected::Objects(ids.iter().map(|&n| o(n)).collect())
}

/// A synthetic environment: every input of the layer system the caller owns.
#[derive(Clone, Debug, Default)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    tapped: BTreeSet<ObjectId>,
    designated: BTreeSet<ObjectId>,
    /// The value of a characteristic-defining expression, per controller (a player value).
    player_value: BTreeMap<PlayerId, i64>,
    /// The base mana-value provenance; plain for any object not listed (round 3).
    base_provenance: BTreeMap<ObjectId, ManaValueProvenance>,
}

impl Env {
    fn put(&mut self, id: ObjectId, c: Characteristics, controller: u64) {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(controller)),
                on_battlefield: true,
            },
        );
    }

    fn set_counter(&mut self, id: ObjectId, kind: &str, amount: i64, t: u64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts.insert((id, kind.to_owned()), ts(t));
    }

    /// Four objects: two creatures, an artifact and a creature with a CDA.
    fn board() -> Env {
        let mut env = Env::default();
        env.put(o(1), creature("test.flier_2_2", 2, 2), 1);
        env.put(o(2), creature("test.bear_3_3", 3, 3), 2);
        env.put(o(3), artifact("test.relic"), 1);
        env.put(o(4), cda_creature("test.counting_beast"), 2);
        env.player_value.insert(p(1), 1);
        env.player_value.insert(p(2), 4);
        env
    }
}

fn holds(pred: &PredicateAST, ctx: &MatchContext<'_>, env: &Env) -> bool {
    let c = &ctx.candidate_view.characteristics;
    match pred {
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasCardType {
            value: WordRef::Name(t),
        } => c.card_types.contains(t),
        PredicateAST::Tapped => env.tapped.contains(&ctx.candidate),
        PredicateAST::HasDesignation { .. } => env.designated.contains(&ctx.candidate),
        _ => false,
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        self.base_provenance.get(&id).cloned().unwrap_or_default()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.counter_ts.get(&(id, kind.to_owned())).copied()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Filter(f) => {
                f.predicate.as_ref().is_some_and(|q| holds(q, ctx, self))
            }
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        literal_of(expr).or_else(|| {
            let controller = self.bases.get(&object)?.controller?;
            self.player_value.get(&controller).copied()
        })
    }
}

/// A registry with selector effects in layers 4, 6 and 7c, a locked set and a 7b effect.
fn registry() -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    r.register(
        e(1),
        stat(o(1), Layer::L7c, 10, is_creature(), modify(1, 1)),
    )
    .unwrap();
    r.register(
        e(2),
        stat(
            o(2),
            Layer::L6,
            11,
            is_creature(),
            LayerModification::AddAbility(keyword("flying")),
        ),
    )
    .unwrap();
    r.register(
        e(3),
        eff(
            Layer::L7b,
            12,
            locked(&[2]),
            LayerModification::SetPt {
                power: Some(5),
                toughness: Some(5),
            },
        ),
    )
    .unwrap();
    r
}

fn pt_of(ev: &Evaluated) -> (Option<i64>, Option<i64>) {
    let c = &ev.characteristics;
    (
        c.power.as_ref().and_then(literal_of),
        c.toughness.as_ref().and_then(literal_of),
    )
}

// ---------------------------------------------------------------------------------------------
// Basic API
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-005: the mode constant is the compile-time feature; a new cache is empty at
// version 0 with zero stats; StateVersion and Invalidation::ALL as documented.
#[test]
fn new_cache_and_constants() {
    assert_eq!(FULL_RECOMPUTE, cfg!(feature = "full-recompute"));
    let c = LayerCache::new();
    assert_eq!(c.version(), StateVersion::default());
    assert_eq!(c.version().get(), 0);
    assert_eq!(c.stats(), CacheStats::default());
    assert_eq!(c.len(), 0);
    assert!(c.is_empty());
    assert_eq!(
        Invalidation::ALL,
        &[
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
        ]
    );
    let distinct: BTreeSet<Invalidation> = Invalidation::ALL.iter().copied().collect();
    assert_eq!(distinct.len(), 10);
}

// REQ-LAYER-005: every cause bumps the version by exactly 1, drops every entry and counts one
// invalidation.
#[test]
fn every_cause_bumps_version_and_drops_entries() {
    let env = Env::board();
    let r = registry();
    let mut c = LayerCache::new();
    for (i, &cause) in Invalidation::ALL.iter().enumerate() {
        c.evaluate_all(&r, &env).unwrap();
        assert_eq!(c.len(), if FULL_RECOMPUTE { 0 } else { 4 });
        let before = c.version().get();
        let v = c.invalidate(cause);
        assert_eq!(v.get(), before + 1, "{cause:?}");
        assert_eq!(c.version(), v);
        assert!(c.is_empty(), "{cause:?}");
        assert_eq!(c.stats().invalidations, i as u64 + 1);
    }
    assert_eq!(c.version().get(), Invalidation::ALL.len() as u64);
}

// REQ-LAYER-005: hits and misses. Cached mode: the first evaluation of an object is a miss and
// stores it, the second is a hit returning an equal value. Full-recompute mode: always a miss,
// nothing stored.
#[test]
fn hit_and_miss_counting() {
    let env = Env::board();
    let r = registry();
    let mut c = LayerCache::new();
    let first = c.evaluate(&r, &env, o(1)).unwrap();
    assert_eq!(first, evaluate(&r, &env, o(1)).unwrap());
    assert_eq!(c.stats().misses, 1);
    assert_eq!(c.stats().hits, 0);
    let second = c.evaluate(&r, &env, o(1)).unwrap();
    assert_eq!(second, first);
    let all = c.evaluate_all(&r, &env).unwrap();
    assert_eq!(all, evaluate_all(&r, &env).unwrap());
    let s = c.stats();
    if FULL_RECOMPUTE {
        assert_eq!(
            s,
            CacheStats {
                hits: 0,
                misses: 6,
                invalidations: 0
            }
        );
        assert!(c.is_empty());
    } else {
        // o(1) hit twice (evaluate and evaluate_all); o(2), o(3), o(4) missed once.
        assert_eq!(
            s,
            CacheStats {
                hits: 2,
                misses: 4,
                invalidations: 0
            }
        );
        assert_eq!(c.len(), 4);
        c.evaluate_all(&r, &env).unwrap();
        assert_eq!(c.stats().hits, 6);
        assert_eq!(c.stats().misses, 4);
    }
    assert_eq!(c.version().get(), 0);
}

// REQ-LAYER-005: errors are returned, count as a miss and are never stored.
#[test]
fn errors_are_not_cached() {
    let env = Env::board();
    let r = registry();
    let mut c = LayerCache::new();
    for _ in 0..2 {
        assert_eq!(
            c.evaluate(&r, &env, o(99)),
            Err(LayerError::UnknownObject(o(99)))
        );
    }
    assert!(c.is_empty());
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 0,
            misses: 2,
            invalidations: 0
        }
    );
}

/// An environment whose object list names an object it has no base for.
struct Broken(Env);

impl LayerEnvironment for Broken {
    fn objects(&self) -> Vec<ObjectId> {
        vec![o(1), o(50), o(2), o(60)]
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.0.base(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.0.matches(selector, ctx)
    }
}

// REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-001: evaluate_all visits env.objects() by ascending
// id (1, 2, 50, 60 for the listed 1, 50, 2, 60) and returns the first error, as
// layers::evaluate_all does; the entries computed before the error (1 and 2) stay valid.
#[test]
fn evaluate_all_first_error_rule() {
    let env = Broken(Env::board());
    let r = registry();
    let mut c = LayerCache::new();
    assert_eq!(c.evaluate_all(&r, &env), evaluate_all(&r, &env));
    assert_eq!(
        c.evaluate_all(&r, &env),
        Err(LayerError::UnknownObject(o(50)))
    );
    assert_eq!(c.len(), if FULL_RECOMPUTE { 0 } else { 2 });
    assert_eq!(c.evaluate(&r, &env, o(1)), evaluate(&r, &env, o(1)));
}

// ---------------------------------------------------------------------------------------------
// Registry change detection
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-005: the first call takes a snapshot without an invalidation; an equal registry,
// even another instance, never invalidates.
#[test]
fn first_call_and_equal_registry_do_not_invalidate() {
    let env = Env::board();
    let r = registry();
    let mut c = LayerCache::new();
    c.evaluate(&r, &env, o(2)).unwrap();
    assert_eq!(c.stats().invalidations, 0);
    assert_eq!(c.version().get(), 0);
    let other = registry();
    c.evaluate_all(&other, &env).unwrap();
    c.evaluate(&other.clone(), &env, o(2)).unwrap();
    assert_eq!(c.stats().invalidations, 0);
    assert_eq!(c.version().get(), 0);
}

/// Asserts that `change` applied to the registry makes the next cache call invalidate once
/// with a fresh, correct result, and that the call after it is not an invalidation.
fn assert_detected(
    c: &mut LayerCache,
    r: &mut ContinuousEffects,
    env: &Env,
    change: impl FnOnce(&mut ContinuousEffects),
) {
    c.evaluate_all(r, env).unwrap();
    let inv = c.stats().invalidations;
    let v = c.version().get();
    change(r);
    assert_eq!(
        c.evaluate_all(r, env).unwrap(),
        evaluate_all(r, env).unwrap()
    );
    assert_eq!(c.stats().invalidations, inv + 1);
    assert_eq!(c.version().get(), v + 1);
    assert_eq!(
        c.evaluate(r, env, o(1)).unwrap(),
        evaluate(r, env, o(1)).unwrap()
    );
    assert_eq!(c.stats().invalidations, inv + 1);
}

// REQ-LAYER-005: register, register_group, remove, set_timestamp and a replaced registry are
// each detected and invalidate with EffectRegistration.
#[test]
fn registry_changes_are_detected() {
    let env = Env::board();
    let mut r = registry();
    let mut c = LayerCache::new();
    assert_detected(&mut c, &mut r, &env, |r| {
        r.register(e(4), eff(Layer::L7c, 13, locked(&[1]), modify(2, 0)))
            .unwrap();
    });
    assert_eq!(
        pt_of(&c.evaluate(&r, &env, o(1)).unwrap()),
        (Some(5), Some(3))
    );
    assert_detected(&mut c, &mut r, &env, |r| {
        let parts = vec![
            (
                e(20),
                stat(
                    o(3),
                    Layer::L4,
                    14,
                    artifacts(),
                    LayerModification::AddTypes {
                        supertypes: BTreeSet::new(),
                        card_types: set(&["Creature"]),
                        subtypes: BTreeSet::new(),
                    },
                ),
            ),
            (
                e(21),
                eff(
                    Layer::L7b,
                    14,
                    Affected::SameAsLayer(Layer::L4),
                    LayerModification::SetPt {
                        power: Some(4),
                        toughness: Some(4),
                    },
                ),
            ),
        ];
        r.register_group(&Env::board(), parts).unwrap();
    });
    assert_eq!(
        pt_of(&c.evaluate(&r, &env, o(3)).unwrap()),
        (Some(5), Some(5))
    );
    assert_detected(&mut c, &mut r, &env, |r| {
        assert!(r.remove(e(4)));
    });
    assert_detected(&mut c, &mut r, &env, |r| {
        r.set_timestamp(e(1), ts(1)).unwrap();
    });
    assert_detected(&mut c, &mut r, &env, |r| {
        *r = ContinuousEffects::new();
    });
    assert_eq!(
        pt_of(&c.evaluate(&r, &env, o(1)).unwrap()),
        (Some(2), Some(2))
    );
    assert_eq!(c.stats().invalidations, 5);
    assert_eq!(c.version().get(), 5);
}

// ---------------------------------------------------------------------------------------------
// Declared environment changes
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-005: an environment change the cache cannot see is picked up after the declared
// invalidation. Without the declaration the cached mode keeps serving the value of the old
// version (the caller's contract), while full-recompute mode is always fresh.
#[test]
fn environment_changes_need_a_declared_invalidation() {
    let mut env = Env::board();
    let r = registry();
    let mut c = LayerCache::new();
    let before = c.evaluate(&r, &env, o(1)).unwrap();
    assert_eq!(pt_of(&before), (Some(3), Some(3)));
    env.set_counter(o(1), "+1/+1", 2, 20);
    let undeclared = c.evaluate(&r, &env, o(1)).unwrap();
    if FULL_RECOMPUTE {
        assert_eq!(undeclared, evaluate(&r, &env, o(1)).unwrap());
    } else {
        assert_eq!(undeclared, before);
    }
    c.invalidate(Invalidation::CounterChange);
    let after = c.evaluate(&r, &env, o(1)).unwrap();
    assert_eq!(pt_of(&after), (Some(5), Some(5)));
    assert_eq!(after, evaluate(&r, &env, o(1)).unwrap());
}

/// One environment change of the given cause; returns whether it changed anything.
fn change_env(env: &mut Env, cause: Invalidation, x: u64) {
    let id = o(x % 4 + 1);
    match cause {
        Invalidation::ZoneChange => {
            if let Some(b) = env.bases.get_mut(&id) {
                b.on_battlefield = !b.on_battlefield;
            }
        }
        Invalidation::ControlChange => {
            if let Some(b) = env.bases.get_mut(&id) {
                b.controller = Some(p(x % 2 + 1));
            }
        }
        Invalidation::CounterChange => {
            let kind = if x.is_multiple_of(3) {
                "-1/-1"
            } else {
                "+1/+1"
            };
            let amount = i64::try_from(x % 3).unwrap_or(0) + 1;
            env.set_counter(id, kind, amount, 5);
        }
        Invalidation::Timestamp => {
            for t in env.counter_ts.values_mut() {
                *t = ts(x % 30);
            }
        }
        Invalidation::Designation => {
            if !env.designated.remove(&id) {
                env.designated.insert(id);
            }
        }
        Invalidation::Attachment => {
            env.attached.entry(id).or_default().push(o(x % 4 + 1));
        }
        Invalidation::Status => {
            if !env.tapped.remove(&id) {
                env.tapped.insert(id);
            }
        }
        Invalidation::PlayerValue => {
            env.player_value
                .insert(p(x % 2 + 1), i64::try_from(x % 7).unwrap_or(0));
        }
        Invalidation::EffectRegistration | Invalidation::EffectExpiry => {}
    }
}

// REQ-LAYER-005: for every cause, a matching environment change followed by its invalidation
// gives results equal to a fresh evaluation, object by object and for the whole board.
#[test]
fn every_cause_with_its_change_stays_sound() {
    let mut r = registry();
    r.register(e(5), stat(o(3), Layer::L7c, 30, tapped(), modify(-1, 0)))
        .unwrap();
    r.register(
        e(6),
        stat(
            o(3),
            Layer::L6,
            31,
            designated(),
            LayerModification::AddAbility(keyword("vigilance")),
        ),
    )
    .unwrap();
    let mut env = Env::board();
    let mut c = LayerCache::new();
    for (i, &cause) in Invalidation::ALL.iter().enumerate() {
        c.evaluate_all(&r, &env).unwrap();
        change_env(&mut env, cause, i as u64 + 1);
        if cause == Invalidation::EffectExpiry {
            r.remove(e(1));
        }
        c.invalidate(cause);
        for id in env.objects() {
            assert_eq!(
                c.evaluate(&r, &env, id),
                evaluate(&r, &env, id),
                "{cause:?}"
            );
        }
        assert_eq!(
            c.evaluate_all(&r, &env),
            evaluate_all(&r, &env),
            "{cause:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Full-recompute mode
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-005: full-recompute mode stores nothing and never hits; versions, invalidations
// and registry-change detection behave as in cached mode.
#[cfg(feature = "full-recompute")]
#[test]
fn full_recompute_never_stores() {
    let env = Env::board();
    let mut r = registry();
    let mut c = LayerCache::new();
    for _ in 0..3 {
        assert_eq!(
            c.evaluate_all(&r, &env).unwrap(),
            evaluate_all(&r, &env).unwrap()
        );
        assert_eq!(c.len(), 0);
    }
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 0,
            misses: 12,
            invalidations: 0
        }
    );
    r.remove(e(3));
    c.evaluate(&r, &env, o(2)).unwrap();
    assert_eq!(c.stats().invalidations, 1);
    assert_eq!(c.invalidate(Invalidation::Status).get(), 2);
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 0,
            misses: 13,
            invalidations: 2
        }
    );
}

// REQ-LAYER-005: cached mode stores one entry per evaluated object at the current version and
// answers repeats from it.
#[cfg(not(feature = "full-recompute"))]
#[test]
fn cached_mode_stores_per_version() {
    let env = Env::board();
    let mut r = registry();
    let mut c = LayerCache::new();
    for _ in 0..3 {
        assert_eq!(
            c.evaluate_all(&r, &env).unwrap(),
            evaluate_all(&r, &env).unwrap()
        );
        assert_eq!(c.len(), 4);
    }
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 8,
            misses: 4,
            invalidations: 0
        }
    );
    r.remove(e(3));
    c.evaluate(&r, &env, o(2)).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c.stats().invalidations, 1);
    assert_eq!(c.invalidate(Invalidation::Status).get(), 2);
    assert!(c.is_empty());
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 8,
            misses: 5,
            invalidations: 2
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-005: equal maps give equal digests; the digest is the documented SHA-256; any
// change to a characteristic, the controller or the set of objects changes it.
#[test]
fn digest_is_canonical_and_sensitive() {
    let env = Env::board();
    let r = registry();
    let map = evaluate_all(&r, &env).unwrap();
    let d = evaluated_digest(&map);
    assert_eq!(d, evaluated_digest(&map.clone()));
    let mut c = LayerCache::new();
    assert_eq!(evaluated_digest(&c.evaluate_all(&r, &env).unwrap()), d);
    assert_eq!(evaluated_digest(&c.evaluate_all(&r, &env).unwrap()), d);
    assert_eq!(d.to_hex().len(), 64);
    assert_ne!(evaluated_digest(&BTreeMap::new()), d);

    type Edit = fn(&mut Evaluated);
    let edits: Vec<(&str, Edit)> = vec![
        ("names", |v| {
            v.characteristics.names.insert("test.extra".to_owned());
        }),
        ("mana_cost", |v| {
            v.characteristics.mana_cost = Some("{1}".parse().unwrap());
        }),
        ("color_indicator", |v| {
            v.characteristics.color_indicator = Some(ColorBitmask::RED);
        }),
        ("colors", |v| v.characteristics.colors = ColorBitmask::BLUE),
        ("supertypes", |v| {
            v.characteristics.supertypes.insert("Legendary".to_owned());
        }),
        ("card_types", |v| {
            v.characteristics.card_types.insert("Artifact".to_owned());
        }),
        ("subtypes", |v| {
            v.characteristics.subtypes.insert("Wolf".to_owned());
        }),
        ("rules_text", |v| {
            v.characteristics.rules_text = serde_json::from_value(
                serde_json::json!([{"id": "t", "kind": "LITERAL", "text": "t"}]),
            )
            .unwrap();
        }),
        ("abilities", |v| {
            v.characteristics.abilities.push(keyword("reach"))
        }),
        ("power", |v| v.characteristics.power = Some(literal(9))),
        ("toughness", |v| v.characteristics.toughness = None),
        ("loyalty", |v| v.characteristics.loyalty = Some(3)),
        ("defense", |v| v.characteristics.defense = Some(3)),
        ("hand_modifier", |v| {
            v.characteristics.hand_modifier = Some(1)
        }),
        ("life_modifier", |v| {
            v.characteristics.life_modifier = Some(-1)
        }),
        ("controller", |v| v.controller = Some(p(3))),
        ("no controller", |v| v.controller = None),
    ];
    let mut seen = BTreeSet::from([d]);
    for (what, edit) in edits {
        let mut changed = map.clone();
        edit(changed.get_mut(&o(1)).unwrap());
        let d2 = evaluated_digest(&changed);
        assert_ne!(d2, d, "{what}");
        assert!(seen.insert(d2), "{what} collides");
    }
    // The same values under another object id.
    let mut moved = map.clone();
    let v = moved.remove(&o(4)).unwrap();
    moved.insert(o(5), v);
    assert_ne!(evaluated_digest(&moved), d);
}

/// A typed value expression of `levels` nested `MATH` nodes around `leaf`, deeper than the
/// canonical JSON depth budget when `levels` is large.
fn deep_value(levels: usize, leaf: i64) -> ValueExpressionAST {
    let mut v = literal(leaf);
    for _ in 0..levels {
        v = ValueExpressionAST::Math {
            op: fineprint_dsl::expr::MathOp::Add,
            lhs: Box::new(v),
            rhs: Box::new(ValueExpressionAST::X),
        };
    }
    v
}

// REQ-LAYER-005: a DSL tree that RFC 8785 canonical JSON cannot encode (#232: a typed value
// expression nested past the depth budget; the former example, an integer outside the safe
// range, can no longer be held by a typed ability) still has a deterministic encoding (the
// MessagePack fallback) that tells values apart.
#[test]
fn digest_encodes_values_outside_canonical_json() {
    let mut a = creature("test.big", 1, 1);
    a.power = Some(deep_value(200, 1));
    let mut b = a.clone();
    b.power = Some(deep_value(200, 2));
    let one = |c: Characteristics| {
        BTreeMap::from([(
            o(1),
            Evaluated {
                characteristics: c,
                controller: Some(p(1)),
                mana_value_provenance: ManaValueProvenance::plain(),
            },
        )])
    };
    assert!(fineprint_dsl::canonical::to_canonical_vec(&a.power).is_err());
    let da = evaluated_digest(&one(a.clone()));
    assert_eq!(da, evaluated_digest(&one(a)));
    assert_ne!(da, evaluated_digest(&one(b)));
}

// ---------------------------------------------------------------------------------------------
// Property test
// ---------------------------------------------------------------------------------------------

/// A fixed-seed linear congruential generator (no randomness crate; REQ-CORE-001).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random effect for the property test.
fn random_effect(g: &mut Lcg, t: u64) -> ContinuousEffect {
    let source = o(g.below(4) + 1);
    let sel = match g.below(4) {
        0 => is_creature(),
        1 => tapped(),
        2 => designated(),
        _ => artifacts(),
    };
    let (layer, m) = match g.below(6) {
        0 => (
            Layer::L4,
            LayerModification::AddTypes {
                supertypes: BTreeSet::new(),
                card_types: set(&["Creature"]),
                subtypes: BTreeSet::new(),
            },
        ),
        1 => (
            Layer::L4,
            LayerModification::SetTypes {
                supertypes: None,
                card_types: Some(set(&["Artifact"])),
                subtypes: None,
            },
        ),
        2 => (Layer::L6, LayerModification::AddAbility(keyword("flying"))),
        3 => (Layer::L6, LayerModification::RemoveAllAbilities),
        4 => (
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(1),
                toughness: Some(1),
            },
        ),
        _ => (Layer::L7c, modify(1, -1)),
    };
    if g.below(3) == 0 {
        let ids: Vec<u64> = (1..=4).filter(|_| g.below(2) == 0).collect();
        eff(layer, t, locked(&ids), m)
    } else {
        stat(source, layer, t, sel, m)
    }
}

// REQ-LAYER-005, REQ-CORE-001: random sequences of registry operations and environment changes
// (each environment change followed by its declared invalidation); after every step the cache
// agrees with a fresh evaluation, object by object and for the whole board, and with the
// digest. In cached mode some evaluations are hits.
#[test]
fn property_cache_equals_full_recompute() {
    for seed in 0..24_u64 {
        let mut g = Lcg(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut env = Env::board();
        let mut r = ContinuousEffects::new();
        let mut c = LayerCache::new();
        let mut next_id = 100_u64;
        for step in 0..60_u64 {
            match g.below(5) {
                0 => {
                    next_id += 1;
                    let t = g.below(40);
                    let effect = random_effect(&mut g, t);
                    r.register(e(next_id), effect).unwrap();
                }
                1 => {
                    let ids: Vec<EffectId> = r.iter().map(|(id, _)| id).collect();
                    if !ids.is_empty() {
                        let pick = ids[usize::try_from(g.below(ids.len() as u64)).unwrap()];
                        if g.below(2) == 0 {
                            r.remove(pick);
                        } else {
                            r.set_timestamp(pick, ts(g.below(40))).unwrap();
                        }
                    }
                }
                2 => {
                    let cause = Invalidation::ALL
                        [usize::try_from(g.below(Invalidation::ALL.len() as u64)).unwrap()];
                    change_env(&mut env, cause, g.next());
                    c.invalidate(cause);
                }
                _ => {}
            }
            let fresh = evaluate_all(&r, &env).unwrap();
            let id = o(g.below(5) + 1);
            assert_eq!(
                c.evaluate(&r, &env, id),
                evaluate(&r, &env, id),
                "seed {seed} step {step}"
            );
            let cached = c.evaluate_all(&r, &env).unwrap();
            assert_eq!(cached, fresh, "seed {seed} step {step}");
            assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
        }
        let s = c.stats();
        if FULL_RECOMPUTE {
            assert_eq!(s.hits, 0);
            assert!(c.is_empty());
        } else {
            assert!(s.hits > 0, "seed {seed}");
        }
        assert_eq!(c.version().get(), s.invalidations);
    }
}

// REQ-LAYER-005, REQ-CORE-001, REQ-OBJ-002; CR 202.3b, docs/DECISIONS.md "#38 FP-037 —
// CodeRabbit round 3: base mana-value provenance from the environment": with an environment that
// supplies a nonmodal back-face base provenance (front {3}{R}), the cache and the uncached
// evaluator agree (results and digest, also under full-recompute), and the digest differs from
// the same base with the default plain provenance. After the base provenance changes and the
// base change is declared (ZoneChange), the cache returns the new value.
#[test]
fn base_mana_value_provenance_cached_and_fresh_agree() {
    let r = registry();
    let plain_env = Env::board();
    let mut env = Env::board();
    let front = ManaCostDSL::parse("{3}{R}").unwrap();
    let back = env.bases[&o(1)].characteristics.clone();
    assert_eq!(back.mana_cost, None);
    let v = CopiableValues::nonmodal_back_face(back, Some(front));
    env.base_provenance.insert(o(1), v.provenance());
    let mut c = LayerCache::new();
    for _ in 0..2 {
        let cached = c.evaluate_all(&r, &env).unwrap();
        let fresh = evaluate_all(&r, &env).unwrap();
        assert_eq!(cached, fresh);
        assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
        assert_eq!(cached[&o(1)].mana_value(ManaValueContext::OffStack), 4);
        assert_eq!(cached[&o(1)].mana_value_provenance, v.provenance());
        assert_eq!(c.evaluate(&r, &env, o(1)), evaluate(&r, &env, o(1)));
    }
    let plain = evaluate_all(&r, &plain_env).unwrap();
    assert!(plain[&o(1)].mana_value_provenance.is_plain());
    assert_eq!(plain[&o(1)].mana_value(ManaValueContext::OffStack), 0);
    assert_ne!(
        evaluated_digest(&evaluate_all(&r, &env).unwrap()),
        evaluated_digest(&plain)
    );
    // The base provenance changes back to plain: declared as a base change.
    env.base_provenance.clear();
    c.invalidate(Invalidation::ZoneChange);
    let cached = c.evaluate_all(&r, &env).unwrap();
    assert_eq!(cached, plain);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&plain));
    assert_eq!(cached[&o(1)].mana_value(ManaValueContext::OffStack), 0);
}

// ---------------------------------------------------------------------------------------------
// Whole-board fill (#190 FP-147)
// ---------------------------------------------------------------------------------------------

/// [`Env`] that counts the calls of [`LayerEnvironment::counters`]: a board pass asks it once
/// per object of its universe, so `calls / objects` is the number of board passes run.
struct Counting {
    inner: Env,
    /// Hidden from the environment: `base` answers `None` for these ids.
    hidden: BTreeSet<ObjectId>,
    /// What `objects` lists; the inner environment's objects when `None`.
    listed: Option<Vec<ObjectId>>,
    calls: std::cell::Cell<u64>,
}

impl Counting {
    fn new(inner: Env) -> Counting {
        Counting {
            inner,
            hidden: BTreeSet::new(),
            listed: None,
            calls: std::cell::Cell::new(0),
        }
    }
}

impl LayerEnvironment for Counting {
    fn objects(&self) -> Vec<ObjectId> {
        self.listed.clone().unwrap_or_else(|| self.inner.objects())
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        if self.hidden.contains(&id) {
            return None;
        }
        self.inner.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.calls.set(self.calls.get() + 1);
        self.inner.counters(id)
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.inner.counter_timestamp(id, kind)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.inner.matches(selector, ctx)
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.inner.eval_value(expr, object)
    }

    fn generating_ability_exists(
        &self,
        _effect: EffectId,
        _source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        // CR 613.8a, 611.3: a static effect exists while its source has an ability.
        source_view.is_none_or(|v| !v.characteristics.abilities.is_empty())
    }
}

/// A registry with static effects of other objects whose existence is judged on their source's
/// view (CR 613.8a), a dependency in layer 4 and a locked set.
fn foreign_registry() -> ContinuousEffects {
    let mut r = registry();
    let types = |card: &[&str]| LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(card),
        subtypes: BTreeSet::new(),
    };
    r.register(
        e(10),
        stat(o(3), Layer::L4, 30, artifacts(), types(&["Creature"])),
    )
    .unwrap();
    r.register(
        e(11),
        stat(
            o(1),
            Layer::L6,
            5,
            is_creature(),
            LayerModification::AddAbility(keyword("vigilance")),
        ),
    )
    .unwrap();
    r.register(
        e(12),
        stat(o(2), Layer::L7c, 31, is_creature(), modify(2, 0)),
    )
    .unwrap();
    r.register(
        e(13),
        eff(
            Layer::L6,
            40,
            locked(&[2]),
            LayerModification::RemoveAllAbilities,
        ),
    )
    .unwrap();
    r
}

// REQ-LAYER-005, REQ-LAYER-001: a cached `evaluate_all` on a cold cache runs one board pass for
// the whole board (observed through the per-object `counters` calls of the pass: one per
// object, not one per object per object) and stores every object; in full-recompute mode it
// stores nothing. A single `evaluate` stores only its own object.
#[test]
fn req_layer_005_cold_evaluate_all_runs_one_board_pass() {
    let env = Counting::new(Env::board());
    let r = foreign_registry();
    let objects = u64::try_from(env.objects().len()).unwrap();
    let mut c = LayerCache::new();
    let all = c.evaluate_all(&r, &env).unwrap();
    assert_eq!(env.calls.get(), objects, "one board pass");
    assert_eq!(all, evaluate_all(&r, &env).unwrap());
    assert_eq!(c.len(), if FULL_RECOMPUTE { 0 } else { all.len() });
    env.calls.set(0);
    c.evaluate_all(&r, &env).unwrap();
    assert_eq!(env.calls.get(), if FULL_RECOMPUTE { objects } else { 0 });
    let mut single = LayerCache::new();
    single.evaluate(&r, &env, o(2)).unwrap();
    assert_eq!(single.len(), usize::from(!FULL_RECOMPUTE));
}

// REQ-LAYER-005: a cold `evaluate_all` counts one miss per object and a following one one hit
// per object (one more miss per object in full-recompute mode); a listed duplicate is a second
// lookup (a hit in cached mode, a miss in full-recompute mode) with the same result.
#[test]
fn req_layer_005_whole_board_fill_stats() {
    let mut env = Counting::new(Env::board());
    let r = foreign_registry();
    let mut c = LayerCache::new();
    c.evaluate_all(&r, &env).unwrap();
    let cold = CacheStats {
        hits: 0,
        misses: 4,
        invalidations: 0,
    };
    assert_eq!(c.stats(), cold);
    c.evaluate_all(&r, &env).unwrap();
    let (hits, misses) = if FULL_RECOMPUTE { (0, 8) } else { (4, 4) };
    let warm = CacheStats {
        hits,
        misses,
        invalidations: 0,
    };
    assert_eq!(c.stats(), warm);
    // A listed duplicate on a cold cache: five lookups, one board pass.
    env.listed = Some(vec![o(4), o(2), o(1), o(3), o(2)]);
    env.calls.set(0);
    let mut d = LayerCache::new();
    let all = d.evaluate_all(&r, &env).unwrap();
    assert_eq!(env.calls.get(), 4);
    assert_eq!(all, evaluate_all(&r, &env).unwrap());
    let (hits, misses) = if FULL_RECOMPUTE { (0, 5) } else { (1, 4) };
    assert_eq!(
        d.stats(),
        CacheStats {
            hits,
            misses,
            invalidations: 0
        }
    );
}

// REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-001: a cache whose entries were partly stored by
// single `evaluate` calls, then filled by `evaluate_all`, gives the uncached `evaluate_all` map;
// the stored objects are hits and the others misses answered from one board pass.
#[test]
fn req_layer_005_partial_then_whole_board_fill() {
    let env = Counting::new(Env::board());
    let r = foreign_registry();
    let fresh = evaluate_all(&r, &env).unwrap();
    for stored in [
        vec![],
        vec![o(2)],
        vec![o(1), o(4)],
        vec![o(1), o(2), o(3), o(4)],
    ] {
        let mut c = LayerCache::new();
        for &id in &stored {
            assert_eq!(c.evaluate(&r, &env, id).unwrap(), fresh[&id]);
        }
        env.calls.set(0);
        assert_eq!(c.evaluate_all(&r, &env).unwrap(), fresh, "{stored:?}");
        let k = stored.len() as u64;
        let (hits, misses) = if FULL_RECOMPUTE { (0, k + 4) } else { (k, 4) };
        assert_eq!(c.stats().hits, hits, "{stored:?}");
        assert_eq!(c.stats().misses, misses, "{stored:?}");
        // No board pass when every object is stored.
        let passes = u64::from(FULL_RECOMPUTE || k < 4);
        assert_eq!(env.calls.get(), passes * 4, "{stored:?}");
        assert_eq!(c.len(), if FULL_RECOMPUTE { 0 } else { 4 });
    }
}

// REQ-LAYER-005, REQ-CORE-001: the first-error rule with the whole-board fill. Objects 1, 3, 4
// have a base and 2 does not (listed 1, 3, 4, 2): the error is `UnknownObject(2)`, the same as
// the uncached `evaluate_all`; object 1, visited before it, is stored with the result
// `evaluate` gives (its universe skips 2); objects 3 and 4, after the error, are not stored
// (docs/DECISIONS.md "#190 FP-147 - Whole-board cache fill").
#[test]
fn req_layer_005_whole_board_fill_first_error() {
    let mut env = Counting::new(Env::board());
    env.hidden.insert(o(2));
    env.listed = Some(vec![o(1), o(3), o(4), o(2)]);
    let r = foreign_registry();
    let mut c = LayerCache::new();
    assert_eq!(
        c.evaluate_all(&r, &env),
        Err(LayerError::UnknownObject(o(2)))
    );
    assert_eq!(evaluate_all(&r, &env), Err(LayerError::UnknownObject(o(2))));
    assert_eq!(
        c.stats(),
        CacheStats {
            hits: 0,
            misses: 2,
            invalidations: 0
        }
    );
    assert_eq!(c.len(), usize::from(!FULL_RECOMPUTE));
    assert_eq!(c.evaluate(&r, &env, o(1)), evaluate(&r, &env, o(1)));
    assert_eq!(c.stats().hits, u64::from(!FULL_RECOMPUTE));
    // Object 3 was not stored: its lookup is a miss.
    let misses = c.stats().misses;
    assert_eq!(c.evaluate(&r, &env, o(3)), evaluate(&r, &env, o(3)));
    assert_eq!(c.stats().misses, misses + 1);
}
