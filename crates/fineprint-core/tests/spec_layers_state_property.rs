// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: the layer cache through the real `GameEnvironment` agrees with a fresh
//! evaluation after every state operation, and over random operation sequences hash,
//! serialization round trip and recomputation agree (T9; REQ-LAYER-001, REQ-LAYER-002,
//! REQ-CORE-001, REQ-CORE-006, REQ-NFR-006). Black-box: written from the work-item brief
//! (#183 FP-144) only.

#[path = "spec_layers_state_support.rs"]
mod support;

use fineprint_base::ids::EffectId;
use fineprint_core::layers::{
    Affected, ContinuousEffect, CounterHolder, EffectOrigin, Invalidation, LayerCache,
    LayerModification, OrderItem, StampReason, evaluate_all, evaluate_with_order, evaluated_digest,
    keyword_counter_ability, literal,
};
use fineprint_core::state::{GameEnvironment, GameState, GeneratingAbility};
use support::*;

const OBJECTS: u64 = 5;

/// Five objects: creatures with P/T and one own ability each, except o5 (no abilities).
fn prop_world() -> World {
    let mut w = World::new();
    for n in 1..=OBJECTS {
        let abilities = if n == OBJECTS {
            vec![]
        } else {
            vec![own_ability(n)]
        };
        let mut c = chars(&format!("test.object_{n}"), &["Creature"], abilities);
        c.power = Some(literal(n as i64));
        c.toughness = Some(literal(6 - n as i64));
        w.put(oid(n), c, if n % 2 == 0 { P2 } else { P1 });
    }
    w.counter(oid(2), "+1/+1", 2);
    w
}

/// One registrable effect of kind `k` and its generator.
fn pool(
    s: &mut GameState,
    k: u64,
    src: u64,
    target: u64,
) -> (ContinuousEffect, Option<GeneratingAbility>) {
    let src_ts = s.timestamps().object_timestamp(oid(src)).unwrap_or(ts(0));
    let linked =
        |e: ContinuousEffect| (e, Some(GeneratingAbility::new(oid(src), own_ability(src))));
    let resolved_now = |s: &mut GameState, layer, objects: &[u64], m| {
        let t = s.stamp_resolved_effect().unwrap();
        (resolved(t, layer, objects, m), None)
    };
    match k % 9 {
        0 => resolved_now(
            s,
            Layer::L7c,
            &[target],
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
        1 => resolved_now(
            s,
            Layer::L5,
            &[target, 5],
            LayerModification::SetColors(ColorBitmask::GREEN),
        ),
        2 => linked(static_sel(
            oid(src),
            src_ts,
            Layer::L6,
            SEL_ALL,
            LayerModification::RemoveAllAbilities,
        )),
        3 => linked(static_sel(
            oid(src),
            src_ts,
            Layer::L6,
            SEL_NO_ABILITIES,
            LayerModification::AddAbility(marker()),
        )),
        4 => linked(static_sel(
            oid(src),
            src_ts,
            Layer::L7b,
            SEL_CREATURE,
            LayerModification::SetPt {
                power: Some(0),
                toughness: Some(1),
            },
        )),
        5 => resolved_now(
            s,
            Layer::L4,
            &[target],
            LayerModification::AddTypes {
                supertypes: set(&[]),
                card_types: set(&["Artifact"]),
                subtypes: set(&[]),
            },
        ),
        6 => {
            // Linked to an ability its source never has: never exists.
            let e = static_sel(
                oid(src),
                src_ts,
                Layer::L6,
                SEL_ALL,
                LayerModification::AddAbility(ability("test.never")),
            );
            (
                e,
                Some(GeneratingAbility::new(oid(src), ability("test.absent"))),
            )
        }
        7 => resolved_now(s, Layer::L7d, &[target], LayerModification::SwitchPt),
        _ => {
            let mut e = static_sel(
                oid(src),
                src_ts,
                Layer::L7a,
                SEL_NONE,
                LayerModification::SetPt {
                    power: Some(7),
                    toughness: Some(7),
                },
            );
            e.is_cda = true;
            e.affected = Affected::Objects(objs(&[src]));
            e.origin = EffectOrigin::Static { granted_at: None };
            linked(e)
        }
    }
}

/// Applies one random operation; returns the invalidation to declare, if any.
fn step(s: &mut GameState, rng: &mut Rng, registered: &mut Vec<EffectId>) -> Option<Invalidation> {
    let src = 1 + rng.below(OBJECTS - 1);
    let target = 1 + rng.below(OBJECTS);
    match rng.below(6) {
        0 | 1 => {
            let (e, g) = pool(s, rng.below(9), src, target);
            registered.push(s.register_effect(e, g).unwrap());
            // Detected by the cache itself (registry change).
            None
        }
        2 => {
            if registered.is_empty() {
                return None;
            }
            let i = rng.below(registered.len() as u64) as usize;
            assert!(s.remove_effect(registered.remove(i)));
            None
        }
        3 => {
            s.stamp_object(oid(target), StampReason::EntersZone)
                .unwrap();
            Some(Invalidation::Timestamp)
        }
        4 => {
            s.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
                .unwrap();
            Some(Invalidation::Timestamp)
        }
        _ => {
            let id = s.issue_effect_id().unwrap();
            s.link_effect_generator(id, GeneratingAbility::new(oid(src), own_ability(src)))
                .unwrap();
            Some(Invalidation::EffectRegistration)
        }
    }
}

fn start() -> GameState {
    let mut s = GameState::new(0x7E57, 2, 20).unwrap();
    s.set_active_player(Some(P1)).unwrap();
    for _ in 0..OBJECTS {
        s.issue_object_id().unwrap();
    }
    for n in 1..=OBJECTS {
        s.stamp_object(oid(n), StampReason::EntersZone).unwrap();
    }
    s
}

/// Compares the cached and fresh evaluations of `s` through `GameEnvironment`.
fn assert_cache_agrees(cache: &mut LayerCache, s: &GameState, w: &World, what: &str) {
    let env = GameEnvironment::new(s, w);
    let fresh = evaluate_all(s.effects(), &env).unwrap();
    let cached = cache.evaluate_all(s.effects(), &env).unwrap();
    assert_eq!(
        evaluated_digest(&cached),
        evaluated_digest(&fresh),
        "{what}"
    );
    assert_eq!(cached, fresh, "{what}");
    // A second cached pass agrees too.
    let again = cache.evaluate_all(s.effects(), &env).unwrap();
    assert_eq!(
        evaluated_digest(&again),
        evaluated_digest(&fresh),
        "{what} (again)"
    );
}

// REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-001, CR 613.1, CR 613.7 (T9): after each of
// register_effect, remove_effect, stamp_object (declared as `Invalidation::Timestamp`) and a
// link change (declared as `Invalidation::EffectRegistration`), the cache through
// GameEnvironment equals a fresh `evaluate_all`.
#[test]
fn cache_matches_fresh_after_each_operation() {
    let w = prop_world();
    let mut s = start();
    let mut cache = LayerCache::new();
    assert_cache_agrees(&mut cache, &s, &w, "start");

    let (e, g) = pool(&mut s, 3, 1, 5);
    let grant = s.register_effect(e, g).unwrap();
    assert_cache_agrees(&mut cache, &s, &w, "register grant");
    let (e, g) = pool(&mut s, 2, 2, 1);
    let remover = s.register_effect(e, g).unwrap();
    assert_cache_agrees(&mut cache, &s, &w, "register remover");
    let (e, g) = pool(&mut s, 0, 1, 2);
    s.register_effect(e, g).unwrap();
    assert_cache_agrees(&mut cache, &s, &w, "register modify");

    // Re-stamping the remover's source changes the order of the remover and the grant only
    // through the effects' own timestamps; the ledger change is declared.
    s.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "stamp_object");

    s.stamp_counters(CounterHolder::Object(oid(2)), "+1/+1")
        .unwrap();
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "stamp_counters");

    let id = s.issue_effect_id().unwrap();
    s.link_effect_generator(id, GeneratingAbility::new(oid(3), own_ability(3)))
        .unwrap();
    cache.invalidate(Invalidation::EffectRegistration);
    assert_cache_agrees(&mut cache, &s, &w, "link");

    assert!(s.remove_effect(remover));
    assert_cache_agrees(&mut cache, &s, &w, "remove remover");
    assert!(s.remove_effect(grant));
    assert_cache_agrees(&mut cache, &s, &w, "remove grant");
}

/// The position of effect `id` among the steps of `rec`.
fn position_of(rec: &fineprint_core::layers::OrderRecord, id: EffectId) -> usize {
    rec.steps
        .iter()
        .position(|st| st.item == OrderItem::Effect(id))
        .expect("the effect is a step of the record")
}

// REQ-LAYER-005, REQ-CORE-002, CR 613.7, CR 613.7a (as cited by `restamp_static_effects`): a
// ledger-only `stamp_object` leaves the registered remover's own timestamp, so the order record
// is unchanged (control); `restamp_static_effects` gives the remover the new object timestamp,
// which moves it after the grant in the fresh order record, and the cache (with the timestamp
// change declared) agrees with a fresh evaluation both times.
#[test]
fn restamp_static_effects_moves_the_remover_in_the_order_record() {
    let w = prop_world();
    let mut s = start();
    let mut cache = LayerCache::new();
    // The grant's source (o3) holds a later timestamp than the remover's source (o2).
    let (e, g) = pool(&mut s, 3, 3, 5);
    let grant = s.register_effect(e, g).unwrap();
    let (e, g) = pool(&mut s, 2, 2, 1);
    let remover = s.register_effect(e, g).unwrap();
    assert_cache_agrees(&mut cache, &s, &w, "registered");
    let order = |s: &GameState| {
        let env = GameEnvironment::new(s, &w);
        evaluate_with_order(s.effects(), &env, oid(5)).unwrap()
    };
    let (_, before) = order(&s);
    assert!(
        position_of(&before, remover) < position_of(&before, grant),
        "the remover starts before the grant"
    );

    // Control: the ledger-only operation leaves the registry, so the order record.
    let old = s.timestamps().object_timestamp(oid(2)).unwrap();
    let new = s.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    assert!(new > old);
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "stamp_object only");
    let (_, control) = order(&s);
    assert_eq!(control, before, "stamp_object alone changes no order");

    // The re-stamp moves the remover's effect to the new object timestamp.
    s.restamp_static_effects(oid(2), old, new).unwrap();
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "restamp_static_effects");
    let (_, after) = order(&s);
    assert_ne!(after, before, "the order record changed");
    assert!(
        position_of(&after, remover) > position_of(&after, grant),
        "the remover now follows the grant"
    );
}

// REQ-LAYER-005, REQ-LAYER-001, CR 122.1b (keyword counters), CR 613.1f, CR 613.7c: a `flying`
// counter grants flying in layer 6 at its counter timestamp; a layer-6 `RemoveKeyword("flying")`
// effect stamped between the two counter stamps removes it while the counter is older and not
// once the counter is re-stamped after it. The cache (timestamp change declared) agrees with a
// fresh evaluation each time.
#[test]
fn restamped_flying_counter_outlives_a_later_keyword_removal() {
    let mut w = prop_world();
    w.counter(oid(1), "flying", 1);
    let mut s = start();
    let mut cache = LayerCache::new();
    let flying = keyword_counter_ability("flying");
    let has_flying = |s: &GameState| {
        let env = GameEnvironment::new(s, &w);
        let e = evaluate_all(s.effects(), &env).unwrap();
        e[&oid(1)].characteristics.abilities.contains(&flying)
    };
    s.stamp_counters(CounterHolder::Object(oid(1)), "flying")
        .unwrap();
    let t = s.stamp_resolved_effect().unwrap();
    s.register_effect(
        resolved(
            t,
            Layer::L6,
            &[1],
            LayerModification::RemoveKeyword("flying".into()),
        ),
        None,
    )
    .unwrap();
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "counter older than the removal");
    assert!(!has_flying(&s), "removal after the counter: no flying");

    let c2 = s
        .stamp_counters(CounterHolder::Object(oid(1)), "flying")
        .unwrap();
    assert!(c2 > t);
    cache.invalidate(Invalidation::Timestamp);
    assert_cache_agrees(&mut cache, &s, &w, "counter re-stamped after the removal");
    assert!(has_flying(&s), "counter after the removal: flying");
}

// REQ-CORE-001, REQ-CORE-006, REQ-NFR-006, REQ-LAYER-001, REQ-LAYER-002 (T9 property): over 200
// fixed-seed random sequences of 12 operations, after every operation the cache equals a fresh
// evaluation, the state round-trips through serialize/deserialize with equal value, hash and
// evaluation, and replaying the same sequence gives the same hash.
#[test]
fn random_sequences_agree() {
    let w = prop_world();
    for seq in 0..200u64 {
        let mut rng = Rng::new(0xF144_0000 + seq);
        let mut s = start();
        let mut cache = LayerCache::new();
        let mut registered = Vec::new();
        let mut hashes = Vec::new();
        for op in 0..12 {
            if let Some(cause) = step(&mut s, &mut rng, &mut registered) {
                cache.invalidate(cause);
            }
            let what = format!("sequence {seq} op {op}");
            assert_cache_agrees(&mut cache, &s, &w, &what);
            let d = GameState::deserialize(&s.serialize().unwrap()).unwrap();
            assert_eq!(d, s, "{what}");
            assert_eq!(d.state_hash(), s.state_hash(), "{what}");
            let env_s = GameEnvironment::new(&s, &w);
            let env_d = GameEnvironment::new(&d, &w);
            assert_eq!(
                evaluated_digest(&evaluate_all(s.effects(), &env_s).unwrap()),
                evaluated_digest(&evaluate_all(d.effects(), &env_d).unwrap()),
                "{what}"
            );
            hashes.push(s.state_hash());
        }
        // Replay.
        let mut rng = Rng::new(0xF144_0000 + seq);
        let mut r = start();
        let mut registered = Vec::new();
        for h in &hashes {
            step(&mut r, &mut rng, &mut registered);
            assert_eq!(&r.state_hash(), h, "sequence {seq} replay");
        }
    }
}
