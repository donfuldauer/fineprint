// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the controller of a sourced static effect (REQ-LAYER-001, REQ-LAYER-002;
//! CR 109.5, 611.3a, 613.1b, 613.1d, 613.7, 613.8, 611.2c, 113.8, 611.3b): the controller a
//! static ability's effect uses for selector matching ("you", CR 109.5) is the source's current
//! controller as evaluated after layer 2, whatever the timestamp of the control change relative
//! to the static; effects of resolved spells/abilities and sourceless effects keep their stored
//! controller.
//!
//! Black-box: written from the FP-152 brief only (issue #203). Every object here is synthetic
//! and neutrally named (R5). Every test passes with and without the `full-recompute` feature.

#[path = "spec_layers_support.rs"]
mod support;

use std::collections::BTreeMap;
use std::rc::Rc;

use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, Invalidation, LayerCache,
    LayerEnvironment, LayerModification, MatchContext, ObjectBase, evaluate, evaluate_all,
    evaluate_with_order,
};
use fineprint_dsl::expr::ObjectSelectorAST;
use support::*;

// ---------------------------------------------------------------------------------------------
// synthetic board
// ---------------------------------------------------------------------------------------------

/// Selector slot of this file: "artifacts you control" — the candidate, as evaluated so far, is
/// an Artifact and its controller is the effect's controller (`MatchContext::effect_controller`).
const SEL_ARTIFACTS_YOU_CONTROL: u32 = 50;

/// The static source S ("artifacts you control are creatures").
const S: u64 = 10;
/// P1's artifact.
const A1: u64 = 1;
/// P2's artifact.
const A2: u64 = 2;
/// The object whose effect changes control of S (layer 2).
const THIEF: u64 = 20;
/// An object id that is never in the environment.
const GONE: u64 = 99;

/// Effect ids.
const E_STATIC: u64 = 1;
const E_STEAL: u64 = 2;

/// Timestamp of the static of S.
const T_STATIC: u64 = 10;

fn add_ayc_selector(env: &mut TestEnv) {
    env.add_selector(
        sel(SEL_ARTIFACTS_YOU_CONTROL),
        Rc::new(|c: &MatchContext<'_>| {
            c.candidate_view
                .characteristics
                .card_types
                .contains("Artifact")
                && c.candidate_view.controller == Some(c.effect_controller)
        }),
    );
}

/// S (controlled by `s_controller` by base), A1 (P1), A2 (P2) and the thief (P2), all on the
/// battlefield.
fn board(s_controller: PlayerId) -> TestEnv {
    let mut env = TestEnv::new();
    env.put_full(
        oid(S),
        enchantment("test.static_source"),
        Some(s_controller),
        true,
    );
    env.put_full(oid(A1), artifact("test.artifact_a1"), Some(P1), true);
    env.put_full(oid(A2), artifact("test.artifact_a2"), Some(P2), true);
    env.put_full(
        oid(THIEF),
        enchantment("test.control_source"),
        Some(P2),
        true,
    );
    add_ayc_selector(&mut env);
    env
}

/// The layer-4 static of `source`: "artifacts you control are creatures", stored controller
/// `stored`.
fn ayc_static(source: Option<ObjectId>, stored: PlayerId, t: u64) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller: stored,
        timestamp: ts(t),
        layer: Layer::L4,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected: by_sel(SEL_ARTIFACTS_YOU_CONTROL),
        modification: add_card_types(&["Creature"]),
    }
}

/// A layer-2 effect of the thief (stored controller P2) giving `to` control of `target`.
fn steal(target: u64, to: PlayerId, t: u64, origin: EffectOrigin) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(oid(THIEF)),
        controller: P2,
        timestamp: ts(t),
        layer: Layer::L2,
        is_cda: false,
        origin,
        affected: objs(&[oid(target)]),
        modification: LayerModification::SetController(to),
    }
}

fn origins() -> [EffectOrigin; 2] {
    [
        EffectOrigin::Resolved,
        EffectOrigin::Static { granted_at: None },
    ]
}

fn is_creature(e: &Evaluated) -> bool {
    e.characteristics.card_types.contains("Creature")
}

fn all(effects: &ContinuousEffects, env: &dyn LayerEnvironment) -> BTreeMap<ObjectId, Evaluated> {
    evaluate_all(effects, env).expect("evaluate_all succeeds")
}

/// Asserts that exactly `creatures` among A1, A2 are creatures (both stay artifacts and keep
/// their base controllers), and that `evaluate` / `evaluate_with_order` agree with
/// `evaluate_all` for every object (REQ-LAYER-001).
fn assert_board(
    effects: &ContinuousEffects,
    env: &TestEnv,
    creatures: &[u64],
    s_controller: PlayerId,
    what: &str,
) {
    let m = all(effects, env);
    for (a, owner) in [(A1, P1), (A2, P2)] {
        let e = &m[&oid(a)];
        assert_eq!(
            is_creature(e),
            creatures.contains(&a),
            "{what}: object {a} creature?"
        );
        assert!(e.characteristics.card_types.contains("Artifact"), "{what}");
        assert_eq!(e.controller, Some(owner), "{what}: object {a} controller");
    }
    assert_eq!(m[&oid(S)].controller, Some(s_controller), "{what}: S");
    assert!(!is_creature(&m[&oid(S)]), "{what}: S is not an artifact");
    for (id, e) in &m {
        assert_eq!(
            &evaluate(effects, env, *id).expect("evaluate"),
            e,
            "{what}: evaluate({id:?})"
        );
        assert_eq!(
            &evaluate_with_order(effects, env, *id)
                .expect("evaluate_with_order")
                .0,
            e,
            "{what}: evaluate_with_order({id:?})"
        );
    }
}

fn reg(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    registry(list)
}

// ---------------------------------------------------------------------------------------------
// 1-3. layer-2 control change of the source vs the layer-4 static, both timestamp orders
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 109.5, 611.3a, 613.1b, 613.1d — order A: the layer-2 effect giving P2
/// control of S is EARLIER than S's static. Layer 2 applies before layer 4 (613.1b, 613.1d), so
/// "artifacts you control" selects P2's artifact: A2 is a creature, A1 is not; S is controlled
/// by P2. Both origins of the control-changing effect.
#[test]
fn req_layer_002_cr_109_5_order_a_control_change_earlier_than_static() {
    let env = board(P1);
    for origin in origins() {
        let effects = reg(&[
            (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
            (E_STEAL, steal(S, P2, T_STATIC - 5, origin.clone())),
        ]);
        assert_board(&effects, &env, &[A2], P2, &format!("order A {origin:?}"));
    }
}

/// REQ-LAYER-002, REQ-LAYER-001, CR 109.5, 611.3a, 613.1b, 613.1d, 613.7, 613.8 — order B: the
/// layer-2 effect is LATER than S's static. Timestamps order only within a layer, so the result
/// is the same as order A: A2 is a creature, A1 is not; `evaluate` and `evaluate_with_order`
/// equal the `evaluate_all` entries (purity).
#[test]
fn req_layer_002_cr_613_7_order_b_control_change_later_than_static() {
    let env = board(P1);
    for origin in origins() {
        let effects = reg(&[
            (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
            (E_STEAL, steal(S, P2, T_STATIC + 5, origin.clone())),
        ]);
        assert_board(&effects, &env, &[A2], P2, &format!("order B {origin:?}"));
        let m = all(&effects, &env);
        for a in [A1, A2] {
            assert_eq!(
                evaluate(&effects, &env, oid(a)).expect("evaluate"),
                m[&oid(a)],
                "order B {origin:?}: evaluate({a}) equals evaluate_all"
            );
        }
    }
}

/// REQ-LAYER-002, CR 109.5, 611.3a — both orders and both registration orders give the same
/// whole-board result (the timestamp of a layer-2 effect relative to a layer-4 one is
/// irrelevant, CR 613.7, 613.8).
#[test]
fn req_layer_002_cr_613_8_both_orders_same_board() {
    let env = board(P1);
    let mut results = Vec::new();
    for t in [T_STATIC - 5, T_STATIC + 5] {
        for flip in [false, true] {
            let st = (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC));
            let sl = (E_STEAL, steal(S, P2, t, EffectOrigin::Resolved));
            let list = if flip { vec![sl, st] } else { vec![st, sl] };
            results.push(all(&reg(&list), &env));
        }
    }
    for r in &results[1..] {
        assert_eq!(r, &results[0]);
    }
}

/// REQ-LAYER-002, CR 109.5, 611.3a — control returns: without the control-changing effect
/// (never registered, or removed again), S is P1's and "artifacts you control" selects A1 only.
#[test]
fn req_layer_002_cr_611_3a_control_returns_to_p1() {
    let env = board(P1);
    let only_static = reg(&[(E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC))]);
    assert_board(&only_static, &env, &[A1], P1, "never stolen");

    for t in [T_STATIC - 5, T_STATIC + 5] {
        let mut effects = reg(&[
            (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
            (E_STEAL, steal(S, P2, t, EffectOrigin::Resolved)),
        ]);
        assert!(effects.remove(eid(E_STEAL)));
        assert_board(&effects, &env, &[A1], P1, &format!("removed t={t}"));
        assert_eq!(all(&effects, &env), all(&only_static, &env));
    }
}

/// REQ-LAYER-002, CR 109.5, 613.1b — the source changes control twice in layer 2 (P2, then back
/// to P1 by a later effect): the latest layer-2 effect wins (CR 613.7) and "you" is P1 again.
#[test]
fn req_layer_002_cr_613_1b_stolen_and_returned_in_layer_2() {
    let env = board(P1);
    let effects = reg(&[
        (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
        (E_STEAL, steal(S, P2, 3, EffectOrigin::Resolved)),
        (3, steal(S, P1, 20, EffectOrigin::Resolved)),
    ]);
    assert_board(&effects, &env, &[A1], P1, "stolen then returned");
    let effects = reg(&[
        (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
        (E_STEAL, steal(S, P1, 3, EffectOrigin::Resolved)),
        (3, steal(S, P2, 20, EffectOrigin::Resolved)),
    ]);
    assert_board(&effects, &env, &[A2], P2, "latest gives P2");
}

/// REQ-LAYER-002, CR 109.5 ("the current controller of the object it's on") — a static whose
/// stored controller is stale (P1) while the source's base controller is P2, with no layer-2
/// effect: "you" is P2, so A2 is a creature and A1 is not.
#[test]
fn req_layer_002_cr_109_5_stale_stored_controller_uses_source_controller() {
    let env = board(P2);
    let effects = reg(&[(E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC))]);
    assert_board(&effects, &env, &[A2], P2, "stale stored P1, base P2");
}

/// REQ-LAYER-002, CR 109.5, 613.1b — the candidate's own control change also counts: when A1
/// is given to P2 in layer 2 and S is P1's, "artifacts you control" selects nothing; when S is
/// also given to P2, it selects both A1 and A2.
#[test]
fn req_layer_002_cr_613_1b_candidate_and_source_both_change_control() {
    let env = board(P1);
    let m = all(
        &reg(&[
            (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
            (E_STEAL, steal(A1, P2, 5, EffectOrigin::Resolved)),
        ]),
        &env,
    );
    assert!(!is_creature(&m[&oid(A1)]));
    assert!(!is_creature(&m[&oid(A2)]));
    for t in [5, 15] {
        let m = all(
            &reg(&[
                (E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC)),
                (E_STEAL, steal(A1, P2, 5, EffectOrigin::Resolved)),
                (3, steal(S, P2, t, EffectOrigin::Resolved)),
            ]),
            &env,
        );
        assert!(is_creature(&m[&oid(A1)]), "t={t}");
        assert!(is_creature(&m[&oid(A2)]), "t={t}");
        assert_eq!(m[&oid(A1)].controller, Some(P2));
    }
}

// ---------------------------------------------------------------------------------------------
// 4. the layer cache
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, REQ-LAYER-005, CR 109.5, 611.3a — the same scenario through `LayerCache`, the
/// control change added and removed with the invalidations declared (EffectRegistration,
/// ControlChange, EffectExpiry): cached results equal fresh `evaluate_all` at every step, in
/// both timestamp orders, whole-board and per object.
#[test]
fn req_layer_002_cr_611_3a_cache_follows_control_change() {
    let env = board(P1);
    for t in [T_STATIC - 5, T_STATIC + 5] {
        let mut cache = LayerCache::new();
        let mut effects = reg(&[(E_STATIC, ayc_static(Some(oid(S)), P1, T_STATIC))]);

        let step = |cache: &mut LayerCache, effects: &ContinuousEffects, creatures: &[u64]| {
            let fresh = all(effects, &env);
            let cached = cache.evaluate_all(effects, &env).expect("cached");
            assert_eq!(cached, fresh, "t={t}");
            for (id, e) in &fresh {
                assert_eq!(&cache.evaluate(effects, &env, *id).expect("cached"), e);
            }
            for a in [A1, A2] {
                assert_eq!(
                    is_creature(&cached[&oid(a)]),
                    creatures.contains(&a),
                    "t={t} object {a}"
                );
            }
        };

        step(&mut cache, &effects, &[A1]);
        // warm again: unchanged
        step(&mut cache, &effects, &[A1]);

        effects
            .register(eid(E_STEAL), steal(S, P2, t, EffectOrigin::Resolved))
            .expect("register");
        cache.invalidate(Invalidation::EffectRegistration);
        cache.invalidate(Invalidation::ControlChange);
        step(&mut cache, &effects, &[A2]);
        step(&mut cache, &effects, &[A2]);

        assert!(effects.remove(eid(E_STEAL)));
        cache.invalidate(Invalidation::EffectExpiry);
        cache.invalidate(Invalidation::ControlChange);
        step(&mut cache, &effects, &[A1]);
    }
}

// ---------------------------------------------------------------------------------------------
// 5. no control change: unchanged behaviour
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 109.5 — no control-changing effect, stored controller equal to the
/// source's base controller: S controlled by P1 makes only A1 a creature; S controlled by P2
/// makes only A2 a creature.
#[test]
fn req_layer_002_cr_109_5_no_control_change_unchanged() {
    for (controller, expected) in [(P1, A1), (P2, A2)] {
        let env = board(controller);
        let effects = reg(&[(E_STATIC, ayc_static(Some(oid(S)), controller, T_STATIC))]);
        assert_board(&effects, &env, &[expected], controller, "no layer 2");
        // a layer-2 effect on an unrelated object changes nothing for the static
        let effects = reg(&[
            (E_STATIC, ayc_static(Some(oid(S)), controller, T_STATIC)),
            (E_STEAL, steal(THIEF, P1, 5, EffectOrigin::Resolved)),
        ]);
        assert_board(&effects, &env, &[expected], controller, "unrelated layer 2");
    }
}

// ---------------------------------------------------------------------------------------------
// 6. effects that are not statics keep their stored controller
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 611.2c, 113.8 — a `Resolved` effect with source S and stored controller P1
/// keeps P1 as its controller after S changes control to P2 in layer 2 (both timestamp orders):
/// A1 is a creature, A2 is not. A sourceless effect keeps its stored controller too (P1 and P2).
#[test]
fn req_layer_002_cr_611_2c_resolved_and_sourceless_keep_stored_controller() {
    let env = board(P1);
    for t in [T_STATIC - 5, T_STATIC + 5] {
        let mut resolved = ayc_static(Some(oid(S)), P1, T_STATIC);
        resolved.origin = EffectOrigin::Resolved;
        let effects = reg(&[
            (E_STATIC, resolved),
            (E_STEAL, steal(S, P2, t, EffectOrigin::Resolved)),
        ]);
        assert_board(&effects, &env, &[A1], P2, &format!("resolved t={t}"));

        for (stored, expected) in [(P1, A1), (P2, A2)] {
            for origin in [
                EffectOrigin::Resolved,
                EffectOrigin::Static { granted_at: None },
            ] {
                let mut sourceless = ayc_static(None, stored, T_STATIC);
                sourceless.origin = origin.clone();
                let effects = reg(&[
                    (E_STATIC, sourceless),
                    (
                        E_STEAL,
                        steal(S, if stored == P1 { P2 } else { P1 }, t, origin.clone()),
                    ),
                ]);
                let s_ctl = if stored == P1 { P2 } else { P1 };
                assert_board(
                    &effects,
                    &env,
                    &[expected],
                    s_ctl,
                    &format!("sourceless {origin:?} stored {stored:?} t={t}"),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 7. a static whose source is outside the evaluated universe
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 109.5 — a static effect whose source has no base in the environment falls
/// back to its stored controller without panicking: stored P1 selects A1, stored P2 selects A2;
/// `evaluate` agrees with `evaluate_all`.
#[test]
fn req_layer_002_cr_109_5_unknown_source_falls_back_to_stored_controller() {
    let env = board(P1);
    for (stored, expected) in [(P1, A1), (P2, A2)] {
        let effects = reg(&[(E_STATIC, ayc_static(Some(oid(GONE)), stored, T_STATIC))]);
        assert_board(
            &effects,
            &env,
            &[expected],
            P1,
            &format!("unknown source, stored {stored:?}"),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 9. property test
// ---------------------------------------------------------------------------------------------

/// The environment seen through a reordered object list with duplicates (contract: results do
/// not depend on the order or duplicates of `objects()`).
struct Reordered<'a> {
    inner: &'a TestEnv,
    list: Vec<ObjectId>,
}

impl LayerEnvironment for Reordered<'_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.list.clone()
    }
    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.inner.base(id)
    }
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.inner.counters(id)
    }
    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.inner.counter_timestamp(id, kind)
    }
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.inner.attached_here(id)
    }
    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.inner.matches(selector, ctx)
    }
    fn eval_value(
        &self,
        expr: &fineprint_dsl::card::ValueExpressionAST,
        object: ObjectId,
    ) -> Option<i64> {
        self.inner.eval_value(expr, object)
    }
}

fn player(rng: &mut Lcg) -> PlayerId {
    if rng.below(2) == 0 { P1 } else { P2 }
}

/// Fixed seed (R3: no wall clock).
const SEED: u64 = 0x0203_0152;
const CASES: u64 = 300;

/// REQ-LAYER-001, REQ-LAYER-002, REQ-LAYER-005, CR 109.5, 611.3a, 613.1b, 613.1d, 613.7 — random
/// small boards (2 players, 0-6 artifacts with random controllers, S with a random base and
/// stored controller, 0-2 layer-2 control changes of S and 0-2 of artifacts at random
/// timestamps before or after the static): (a) `evaluate` and `evaluate_with_order` equal
/// `evaluate_all`; (b) an artifact is a creature iff its evaluated controller equals S's final
/// evaluated controller, which is the latest control change of S or its base controller;
/// (c) the result is independent of the order and duplicates of `objects()`; (d) the layer
/// cache equals full recomputation, cold and after declared invalidations.
#[test]
fn req_layer_001_req_layer_002_property_controller_follows_source() {
    let mut rng = Lcg::new(SEED);
    // coverage of the generator: cases where S changed control, final controllers of S, and
    // artifacts that became creatures / stayed noncreatures
    let mut stolen = 0u64;
    let mut finals = [0u64; 2];
    let mut animated = [0u64; 2];
    for case in 0..CASES {
        let n = rng.below(7);
        let s_base = player(&mut rng);
        let stored = player(&mut rng);
        let mut env = TestEnv::new();
        env.put_full(
            oid(S),
            enchantment("test.static_source"),
            Some(s_base),
            true,
        );
        env.put_full(
            oid(THIEF),
            enchantment("test.control_source"),
            Some(P2),
            true,
        );
        for i in 1..=n {
            let c = player(&mut rng);
            env.put_full(
                oid(i),
                artifact(&format!("test.artifact_{i}")),
                Some(c),
                true,
            );
        }
        add_ayc_selector(&mut env);

        // distinct timestamps from a shuffled pool; the static takes the first one
        let mut pool: Vec<u64> = (1..=20).collect();
        rng.shuffle(&mut pool);
        let mut pool = pool.into_iter();
        let t_static = pool.next().expect("pool");
        let static_effect = ayc_static(Some(oid(S)), stored, t_static);

        let mut steals: Vec<(u64, ContinuousEffect)> = Vec::new();
        let mut latest_s: Option<(u64, PlayerId)> = None;
        let mut next_id = 10;
        for _ in 0..rng.below(3) {
            let to = player(&mut rng);
            let t = pool.next().expect("pool");
            let origin = if rng.below(2) == 0 {
                EffectOrigin::Resolved
            } else {
                EffectOrigin::Static { granted_at: None }
            };
            steals.push((next_id, steal(S, to, t, origin)));
            next_id += 1;
            if latest_s.is_none_or(|(lt, _)| t > lt) {
                latest_s = Some((t, to));
            }
        }
        if n > 0 {
            for _ in 0..rng.below(3) {
                let target = 1 + rng.below(n);
                let to = player(&mut rng);
                let t = pool.next().expect("pool");
                steals.push((next_id, steal(target, to, t, EffectOrigin::Resolved)));
                next_id += 1;
            }
        }
        let mut list = vec![(E_STATIC, static_effect.clone())];
        list.extend(steals.iter().cloned());
        rng.shuffle(&mut list);
        let effects = reg(&list);
        let what = format!("case {case}: n={n} base={s_base:?} stored={stored:?} {list:?}");

        let m = all(&effects, &env);

        // (a)
        for (id, e) in &m {
            assert_eq!(
                &evaluate(&effects, &env, *id).expect("evaluate"),
                e,
                "{what}"
            );
            assert_eq!(
                &evaluate_with_order(&effects, &env, *id).expect("ewo").0,
                e,
                "{what}"
            );
        }

        // (b)
        let s_final = latest_s.map_or(s_base, |(_, p)| p);
        if s_final != s_base {
            stolen += 1;
        }
        finals[usize::from(s_final == P2)] += 1;
        assert_eq!(m[&oid(S)].controller, Some(s_final), "{what}: S controller");
        for i in 1..=n {
            let e = &m[&oid(i)];
            assert_eq!(
                is_creature(e),
                e.controller == Some(s_final),
                "{what}: artifact {i} (controller {:?})",
                e.controller
            );
            animated[usize::from(is_creature(e))] += 1;
        }

        // (c)
        let mut objects = env.objects();
        let dup = objects.clone();
        objects.extend(
            dup.iter()
                .take(usize::try_from(rng.below(3)).expect("fits")),
        );
        rng.shuffle(&mut objects);
        let reordered = Reordered {
            inner: &env,
            list: objects,
        };
        assert_eq!(all(&effects, &reordered), m, "{what}: reordered objects");

        // (d) cold, then from a board without the layer-2 effects with declared invalidations
        let mut cache = LayerCache::new();
        assert_eq!(
            cache.evaluate_all(&effects, &env).expect("cached"),
            m,
            "{what}: cold cache"
        );
        let mut cache = LayerCache::new();
        let only_static = reg(&[(E_STATIC, static_effect)]);
        assert_eq!(
            cache.evaluate_all(&only_static, &env).expect("cached"),
            all(&only_static, &env),
            "{what}: static only"
        );
        cache.invalidate(Invalidation::EffectRegistration);
        cache.invalidate(Invalidation::ControlChange);
        assert_eq!(
            cache.evaluate_all(&effects, &env).expect("cached"),
            m,
            "{what}: after registration"
        );
        for (id, e) in &m {
            assert_eq!(&cache.evaluate(&effects, &env, *id).expect("cached"), e);
        }
    }
    assert!(stolen > 0, "some case changes S's controller by layer 2");
    assert!(
        finals[0] > 0 && finals[1] > 0,
        "both players end up controlling S"
    );
    assert!(
        animated[0] > 0 && animated[1] > 0,
        "some artifacts become creatures, some do not"
    );
}
