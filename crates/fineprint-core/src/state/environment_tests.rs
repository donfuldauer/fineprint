// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Scenario tests through the real game-state environment ([`GameEnvironment`]):
//! generating-ability existence for static effects (the PR #187 review scenario, S1) and for a
//! permanent's own ETB replacement effect (the PR #202 review scenario, S2), the hook contract
//! (D4), and the layer-cache invalidation table (D7) (REQ-LAYER-001, REQ-LAYER-002,
//! REQ-LAYER-005, REQ-CORE-001, REQ-CORE-002). Synthetic cards and neutral names only.

use std::collections::BTreeMap;

use fineprint_base::ids::{EffectId, ObjectId};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST};

use super::effects_tests::{
    ability, creature, o, only, p, resolved_effect, selector, static_effect,
};
use super::*;
use crate::layers::{
    Affected, CounterHolder, Evaluated, FULL_RECOMPUTE, Invalidation, LayerCache, LayerEnvironment,
    LayerModification, ManaValueProvenance, MatchContext, ObjectBase, OrderItem, Selection,
    StampReason, evaluate, evaluate_all, evaluate_with_order, evaluated_digest,
};
use crate::objects::Characteristics;
use crate::replacement::{
    EtbAction, EtbApplied, EtbContext, EtbEffect, EtbEffects, EtbEntry, EtbScope, applicable,
    project,
};

/// The keyword the test world reads as "has at least one ability".
const ANY_ABILITY: &str = "test.any_ability";

/// A synthetic object world. Selector semantics (the world's own, R4-neutral): a filter whose
/// predicate is `HAS_KEYWORD test.any_ability` matches a candidate with at least one ability;
/// `NOT` negates; `IS_PERMANENT` matches everything; `IS_CREATURE` matches creatures. A base
/// power `"test.life"` is the life `life`.
#[derive(Default)]
struct World {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    life: i64,
}

impl World {
    fn with(mut self, id: ObjectId, c: Characteristics) -> World {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(1)),
                on_battlefield: true,
            },
        );
        self
    }
}

fn holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::Not(q) => !holds(q, view),
        PredicateAST::IsPermanent => true,
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasKeyword { keyword } if keyword == ANY_ABILITY => !c.abilities.is_empty(),
        _ => false,
    }
}

impl ObjectWorld for World {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Filter(f) => f
                .predicate
                .as_ref()
                .is_some_and(|q| holds(q, ctx.candidate_view)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    fn eval_value(&self, expr: &ValueExpressionAST, _: ObjectId) -> Option<i64> {
        (expr.0.as_str() == Some("test.life")).then_some(self.life)
    }
}

/// "Objects with no abilities".
fn no_abilities() -> Affected {
    Affected::Selector(selector(PredicateAST::Not(Box::new(
        PredicateAST::HasKeyword {
            keyword: ANY_ABILITY.to_owned(),
        },
    ))))
}

fn marker() -> AbilityAST {
    ability("marker")
}

fn has_marker(ev: &Evaluated) -> bool {
    ev.characteristics.abilities.contains(&marker())
}

/// The layer-6 steps of `object`'s evaluation: (item, selection, applied).
fn layer6(
    state: &GameState,
    world: &World,
    object: ObjectId,
) -> (Evaluated, Vec<(OrderItem, Selection, bool)>) {
    let env = GameEnvironment::new(state, world);
    let (ev, record) = evaluate_with_order(state.effects(), &env, object).unwrap();
    assert_eq!(evaluate(state.effects(), &env, object).unwrap(), ev);
    let steps = record
        .steps
        .iter()
        .filter(|s| s.layer == Layer::L6)
        .map(|s| (s.item.clone(), s.selection, s.applied))
        .collect();
    (ev, steps)
}

// ---------------------------------------------------------------------------------------------
// S1: the PR #187 review scenario
// ---------------------------------------------------------------------------------------------

/// test.state_granter (`sB`, object 1) has the ability `bAbility`; its static effect B (layer 6)
/// grants `marker` to objects with no abilities. Effect A (layer 6, a resolved "lose all
/// abilities") affects every object of the world, `sB` included. `a_older` picks the timestamp
/// order. Returns the state, the world and the ids (A, B).
fn s1_scene(a_older: bool, with_a: bool) -> (GameState, World, Option<EffectId>, EffectId) {
    let b_ability = ability("b_granter");
    let world = World::default()
        .with(o(1), creature(vec![b_ability.clone()]))
        .with(o(2), creature(vec![ability("t")]))
        .with(o(3), creature(Vec::new()));
    let mut s = GameState::new(187, 2, 20).unwrap();
    // CR 613.7b, 613.7d, 613.7a: B shares its source's timestamp; A gets one as it resolves.
    let (t_a, t_b) = if a_older {
        let t_a = s.stamp_resolved_effect().unwrap();
        (t_a, s.stamp_object(o(1), StampReason::EntersZone).unwrap())
    } else {
        let t_b = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
        (s.stamp_resolved_effect().unwrap(), t_b)
    };
    let a = with_a.then(|| {
        s.register_effect(
            resolved_effect(
                t_a,
                Layer::L6,
                only(&[1, 2, 3]),
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap()
    });
    let b = s
        .register_effect(
            static_effect(
                o(1),
                t_b,
                Layer::L6,
                no_abilities(),
                LayerModification::AddAbility(marker()),
            ),
            Some(GeneratingAbility::new(o(1), b_ability)),
        )
        .unwrap();
    (s, world, a, b)
}

// REQ-LAYER-002, REQ-LAYER-001: CR 613.8a, 613.8b, 611.3, 113.6, 613.7. Through the real game
// state environment, in both timestamp orders: A removes every ability of sB, B's generating
// ability included, so B (layer 6) does not exist after A and grants no marker; sB ends with no
// abilities. B is never ordered after A by a dependency: with A older, A applies first by
// timestamp and B does not apply; with B older, B comes first by timestamp and does not apply
// (sB still has an ability, so B's selector does not match it), then A applies. CR 613.8a(b):
// applying A changes B's existence, so B waits for A (CR 613.8b); either way the final result is
// the one CR 613.8 gives: A applies, B does not apply, no marker.
#[test]
fn s1_removed_generating_ability_both_orders() {
    for a_older in [true, false] {
        let (s, world, a, b) = s1_scene(a_older, true);
        let a = a.unwrap();
        let (ev, steps) = layer6(&s, &world, o(1));
        assert!(!has_marker(&ev), "a_older={a_older}");
        assert!(ev.characteristics.abilities.is_empty(), "a_older={a_older}");
        let mut want = vec![
            (OrderItem::Effect(a), Selection::Timestamp, true),
            (OrderItem::Effect(b), Selection::Timestamp, false),
        ];
        if !a_older {
            want.reverse();
        }
        assert_eq!(steps, want, "a_older={a_older}");
        assert!(
            steps
                .iter()
                .all(|(_, selection, _)| *selection != Selection::Dependency)
        );
        // No object gets the marker: on another object the hook judges sB as the layers give
        // it (its ability removed by A), so B does not exist there either.
        let env = GameEnvironment::new(&s, &world);
        let view = evaluate(s.effects(), &env, o(1)).unwrap();
        assert!(!env.generating_ability_exists(b, o(1), Some(&view)));
        assert!(!env.generating_ability_exists(b, o(1), None));
        for object in [o(2), o(3)] {
            let (ev, steps) = layer6(&s, &world, object);
            assert!(!has_marker(&ev), "a_older={a_older} {object:?}");
            assert!(ev.characteristics.abilities.is_empty());
            assert!(
                steps
                    .iter()
                    .all(|(item, _, applied)| *item != OrderItem::Effect(b) || !applied)
            );
        }
        // Deterministic and stable through the wire.
        let back = GameState::deserialize(&s.serialize().unwrap()).unwrap();
        assert_eq!(layer6(&back, &world, o(1)), layer6(&s, &world, o(1)));
    }
}

// REQ-LAYER-002, REQ-LAYER-001: CR 613.8a, 613.8b, 611.3: A is the static effect of sA
// (object 4, ability `aAbility`) and removes every ability of every object, sA's own included.
// An effect never depends on itself, so A keeps applying to every object; B's generating
// ability is removed from sB, so in both timestamp orders no object gets the marker.
#[test]
fn s1_static_remover_removing_its_own_ability() {
    for a_older in [true, false] {
        let (mut s, mut world, a, b) = s1_scene(a_older, true);
        let a = a.unwrap();
        let t_a = s.effects().get(a).unwrap().timestamp;
        assert!(s.remove_effect(a));
        let a_ability = ability("a_remover");
        world = world.with(o(4), creature(vec![a_ability.clone()]));
        let mut remover = static_effect(
            o(4),
            t_a,
            Layer::L6,
            only(&[1, 2, 3, 4]),
            LayerModification::RemoveAllAbilities,
        );
        remover.controller = p(2);
        let a = s
            .register_effect(remover, Some(GeneratingAbility::new(o(4), a_ability)))
            .unwrap();
        for object in [o(1), o(2), o(3), o(4)] {
            let (ev, steps) = layer6(&s, &world, object);
            assert!(!has_marker(&ev), "a_older={a_older} {object:?}");
            assert!(ev.characteristics.abilities.is_empty(), "{object:?}");
            assert!(steps.contains(&(OrderItem::Effect(a), Selection::Timestamp, true)));
            assert!(
                steps
                    .iter()
                    .all(|(item, _, applied)| *item != OrderItem::Effect(b) || !applied)
            );
        }
    }
}

// REQ-LAYER-002 (S1 control): CR 613.8a: without A, B's generating ability exists: B grants the
// marker to an object with no abilities, and not to objects that have one (sB itself, object 2).
#[test]
fn s1_control_without_remover_b_applies() {
    for a_older in [true, false] {
        let (s, world, a, b) = s1_scene(a_older, false);
        assert!(a.is_none());
        let (ev3, steps3) = layer6(&s, &world, o(3));
        assert!(has_marker(&ev3));
        assert_eq!(
            steps3,
            vec![(OrderItem::Effect(b), Selection::Timestamp, true)]
        );
        let (ev1, _) = layer6(&s, &world, o(1));
        assert!(!has_marker(&ev1));
        let (ev2, _) = layer6(&s, &world, o(2));
        assert!(!has_marker(&ev2));
    }
}

// REQ-LAYER-002 (S1 control): CR 613.8a, 613.8b: a remover that leaves B's generating ability
// (it removes only object 2's and object 3's abilities) leaves B in existence: object 2 loses
// its ability, then B's selector matches it, so B depends on the remover and applies after it in
// both timestamp orders.
#[test]
fn s1_control_remover_spares_source() {
    for a_older in [true, false] {
        let (mut s, world, a, b) = s1_scene(a_older, true);
        let a = a.unwrap();
        let t_a = s.effects().get(a).unwrap().timestamp;
        assert!(s.remove_effect(a));
        let a2 = s
            .register_effect(
                resolved_effect(
                    t_a,
                    Layer::L6,
                    only(&[2, 3]),
                    LayerModification::RemoveAllAbilities,
                ),
                None,
            )
            .unwrap();
        let (ev, steps) = layer6(&s, &world, o(2));
        assert!(has_marker(&ev), "a_older={a_older}");
        let first = if a_older {
            Selection::Timestamp
        } else {
            Selection::Dependency
        };
        assert_eq!(
            steps,
            vec![
                (OrderItem::Effect(a2), first, true),
                (OrderItem::Effect(b), Selection::Timestamp, true)
            ],
            "a_older={a_older}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// S2: the PR #202 review scenario
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-002, REQ-LAYER-001: CR 614.12, 613.8a, 611.3: an in-force layer-6 "lose all
// abilities" effect affects the entering permanent, whose own ThisPermanent ETB "enters tapped"
// effect is linked to the permanent's own ability through link_effect_generator. Through the
// real environment the projected permanent has no abilities, so the ETB effect does not apply;
// removing the lose-all effect makes it apply.
#[test]
fn s2_etb_effect_needs_its_generating_ability() {
    let enters_tapped = ability("enters_tapped");
    let mut world = World::default().with(o(5), creature(vec![enters_tapped.clone()]));
    world.bases.get_mut(&o(5)).unwrap().on_battlefield = false;
    let mut s = GameState::new(202, 2, 20).unwrap();
    let ts = s.stamp_resolved_effect().unwrap();
    let lose_all = s
        .register_effect(
            resolved_effect(
                ts,
                Layer::L6,
                only(&[5]),
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap();
    let etb_id = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb_id, GeneratingAbility::new(o(5), enters_tapped))
        .unwrap();
    let mut etb = EtbEffects::new();
    etb.add(
        EtbEffect::new(etb_id, EtbAction::EntersTapped)
            .with_source(o(5))
            .with_scope(EtbScope::ThisPermanent),
    )
    .unwrap();
    let entry = EtbEntry {
        object: o(5),
        controller: p(1),
        own_statics: Vec::new(),
    };
    let check = |s: &GameState, want: &[EffectId]| {
        let env = GameEnvironment::new(s, &world);
        let ctx = EtbContext {
            env: &env,
            continuous: s.effects(),
            effects: &etb,
        };
        assert_eq!(
            applicable(&ctx, &entry, &EtbApplied::default()).unwrap(),
            want
        );
        project(&ctx, &entry, &EtbApplied::default()).unwrap()
    };
    let projected = check(&s, &[]);
    assert!(projected.evaluated.characteristics.abilities.is_empty());
    assert!(!projected.tapped);
    // Control: without the lose-all effect the ability exists and the effect applies.
    let mut control = s.clone();
    assert!(control.remove_effect(lose_all));
    let projected = check(&control, &[etb_id]);
    assert_eq!(projected.evaluated.characteristics.abilities.len(), 1);
    // Unlinked (not modelled): the hook answers true, so the effect applies even after lose-all.
    let mut unlinked = s.clone();
    assert!(unlinked.remove_effect(etb_id));
    check(&unlinked, &[etb_id]);
}

// ---------------------------------------------------------------------------------------------
// D4: the hook and the delegation
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-002: CR 613.8a: generating_ability_exists from the state's links, by ability value,
// with the documented base fallback when no view is given; everything else delegates.
#[test]
fn hook_contract_and_delegation() {
    let g = ability("g");
    let mut world = World::default()
        .with(o(1), creature(vec![g.clone(), g.clone()]))
        .with(o(2), creature(Vec::new()));
    world
        .counters
        .insert(o(1), BTreeMap::from([("flying".to_owned(), 1)]));
    world.attached.insert(o(1), vec![o(2)]);
    world.life = 13;
    let mut s = GameState::new(1, 2, 20).unwrap();
    let ts = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let id = s
        .register_effect(
            static_effect(
                o(1),
                ts,
                Layer::L6,
                only(&[2]),
                LayerModification::AddAbility(marker()),
            ),
            Some(GeneratingAbility::new(o(1), g.clone())),
        )
        .unwrap();
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(o(9), g.clone()))
        .unwrap();
    let env = GameEnvironment::new(&s, &world);
    let view = |abilities: Vec<AbilityAST>| Evaluated {
        characteristics: creature(abilities),
        controller: Some(p(1)),
        mana_value_provenance: ManaValueProvenance::plain(),
    };
    // No link: not modelled.
    assert!(env.generating_ability_exists(EffectId::new(77), o(1), None));
    // The view decides when given; duplicates are one identity.
    assert!(env.generating_ability_exists(id, o(1), Some(&view(vec![g.clone()]))));
    assert!(env.generating_ability_exists(id, o(1), Some(&view(vec![g.clone(), g.clone()]))));
    assert!(!env.generating_ability_exists(id, o(1), Some(&view(vec![ability("other")]))));
    assert!(!env.generating_ability_exists(id, o(1), Some(&view(Vec::new()))));
    // Another source than the link's: false.
    assert!(!env.generating_ability_exists(id, o(2), Some(&view(vec![g.clone()]))));
    // No view: the source as the layers give it; an unknown source has no abilities.
    assert!(env.generating_ability_exists(id, o(1), None));
    assert!(!env.generating_ability_exists(etb, o(9), None));
    let bare = World::default().with(o(1), creature(Vec::new()));
    assert!(!GameEnvironment::new(&s, &bare).generating_ability_exists(id, o(1), None));
    let mut removed = s.clone();
    let rt = removed.stamp_resolved_effect().unwrap();
    removed
        .register_effect(
            resolved_effect(
                rt,
                Layer::L6,
                only(&[1]),
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap();
    let removed_env = GameEnvironment::new(&removed, &world);
    assert!(!removed_env.generating_ability_exists(id, o(1), None));
    assert!(!has_marker(
        &evaluate(removed.effects(), &removed_env, o(2)).unwrap()
    ));
    // One level deep: inside the source's evaluation, a static effect of yet another source is
    // judged on that source's base (object 2 has no abilities, so its remover does not exist).
    let mut nested = s.clone();
    let t2 = nested.stamp_object(o(2), StampReason::EntersZone).unwrap();
    nested
        .register_effect(
            static_effect(
                o(2),
                t2,
                Layer::L6,
                only(&[1]),
                LayerModification::RemoveAllAbilities,
            ),
            Some(GeneratingAbility::new(o(2), ability("remover"))),
        )
        .unwrap();
    let nested_env = GameEnvironment::new(&nested, &world);
    assert!(nested_env.generating_ability_exists(id, o(1), None));
    // Counter timestamps come from the state's ledger (CR 613.7c), objects only.
    assert_eq!(env.counter_timestamp(o(1), "flying"), None);
    let mut s2 = s.clone();
    let ct = s2
        .stamp_counters(CounterHolder::Object(o(1)), "flying")
        .unwrap();
    s2.stamp_counters(CounterHolder::Player(p(1)), "poison")
        .unwrap();
    let env2 = GameEnvironment::new(&s2, &world);
    assert_eq!(env2.counter_timestamp(o(1), "flying"), Some(ct));
    assert_eq!(env2.counter_timestamp(o(1), "poison"), None);
    // Delegation to the world.
    assert_eq!(env.objects(), [o(1), o(2)]);
    assert_eq!(env.base(o(2)), world.base(o(2)));
    assert_eq!(env.base(o(8)), None);
    assert!(env.base_mana_value_provenance(o(1)).is_plain());
    assert_eq!(env.counters(o(1)), world.counters(o(1)));
    assert_eq!(env.attached_here(o(1)), [o(2)]);
    assert!(env.attached_here(o(2)).is_empty());
    assert_eq!(
        env.eval_value(&ValueExpressionAST("test.life".into()), o(1)),
        Some(13)
    );
    assert_eq!(env.eval_value(&ValueExpressionAST(5.into()), o(1)), None);
    assert!(std::ptr::eq(env.state(), &s));
    assert_eq!(env.world().objects(), world.objects());
    // The trait defaults.
    struct Minimal;
    impl ObjectWorld for Minimal {
        fn objects(&self) -> Vec<ObjectId> {
            Vec::new()
        }
        fn base(&self, _: ObjectId) -> Option<ObjectBase> {
            None
        }
        fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
            false
        }
    }
    let minimal = GameEnvironment::new(&s, &Minimal);
    assert!(minimal.counters(o(1)).is_empty());
    assert!(minimal.attached_here(o(1)).is_empty());
    assert!(minimal.base_mana_value_provenance(o(1)).is_plain());
    assert_eq!(
        minimal.eval_value(&ValueExpressionAST(5.into()), o(1)),
        Some(5)
    );
    // Evaluation through the environment: object 2 gets the marker (CR 613.1f).
    let ev = evaluate(s.effects(), &env, o(2)).unwrap();
    assert!(has_marker(&ev));
}

// ---------------------------------------------------------------------------------------------
// D7: the layer-cache invalidation table
// ---------------------------------------------------------------------------------------------

/// Asserts the cached evaluation equals a full recompute.
fn same_as_full(cache: &mut LayerCache, s: &GameState, world: &World) {
    let env = GameEnvironment::new(s, world);
    let cached = cache.evaluate_all(s.effects(), &env).unwrap();
    let full = evaluate_all(s.effects(), &env).unwrap();
    assert_eq!(cached, full);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&full));
}

// REQ-LAYER-005, REQ-LAYER-001, REQ-CORE-002: after each kind of GameState mutation, a cache
// that is told the invalidation of the environment documentation's table equals a full
// recompute through GameEnvironment (cached and full-recompute builds both run it).
#[test]
fn d7_cache_with_documented_invalidations_equals_full_recompute() {
    let mut base = creature(vec![ability("own")]);
    base.power = Some(ValueExpressionAST("test.life".into()));
    let mut world = World::default()
        .with(o(1), base)
        .with(o(2), creature(Vec::new()));
    world
        .counters
        .insert(o(1), BTreeMap::from([("flying".to_owned(), 1)]));
    world.life = 20;
    let mut s = GameState::new(7, 2, 20).unwrap();
    let mut cache = LayerCache::new();
    same_as_full(&mut cache, &s, &world);

    // Registration: detected by the cache itself.
    let ts = s.stamp_resolved_effect().unwrap();
    let remover = s
        .register_effect(
            resolved_effect(
                ts,
                Layer::L6,
                only(&[1]),
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap();
    same_as_full(&mut cache, &s, &world);
    let env = GameEnvironment::new(&s, &world);
    assert!(
        evaluate(s.effects(), &env, o(1))
            .unwrap()
            .characteristics
            .abilities
            .is_empty()
    );

    // A counter stamp later than the remover (CR 613.7c): Invalidation::Timestamp.
    s.stamp_counters(CounterHolder::Object(o(1)), "flying")
        .unwrap();
    if !FULL_RECOMPUTE {
        // Undeclared, the cache would still hold the old value.
        let env = GameEnvironment::new(&s, &world);
        let stale = cache.evaluate(s.effects(), &env, o(1)).unwrap();
        assert_ne!(stale, evaluate(s.effects(), &env, o(1)).unwrap());
    }
    cache.invalidate(Invalidation::Timestamp);
    same_as_full(&mut cache, &s, &world);
    let env = GameEnvironment::new(&s, &world);
    assert_eq!(
        evaluate(s.effects(), &env, o(1))
            .unwrap()
            .characteristics
            .abilities
            .len(),
        1
    );

    // Clearing the counter stamp: Invalidation::Timestamp.
    s.clear_counter_stamp(CounterHolder::Object(o(1)), "flying");
    cache.invalidate(Invalidation::Timestamp);
    same_as_full(&mut cache, &s, &world);

    // Object, sticker and simultaneous stamps: Invalidation::Timestamp.
    s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    s.stamp_sticker(o(1), 1).unwrap();
    s.forget_object_stamps(o(2));
    cache.invalidate(Invalidation::Timestamp);
    same_as_full(&mut cache, &s, &world);

    // A static effect with its link, and an ETB link: detected / nothing to declare.
    let ts1 = s.timestamps().object_timestamp(o(1)).unwrap();
    s.register_effect(
        static_effect(
            o(1),
            ts1,
            Layer::L6,
            no_abilities(),
            LayerModification::AddAbility(marker()),
        ),
        Some(GeneratingAbility::new(o(1), ability("own"))),
    )
    .unwrap();
    same_as_full(&mut cache, &s, &world);
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(o(2), ability("e")))
        .unwrap();
    same_as_full(&mut cache, &s, &world);

    // Removal: detected (EffectExpiry may be declared).
    assert!(s.remove_effect(remover));
    same_as_full(&mut cache, &s, &world);
    cache.invalidate(Invalidation::EffectExpiry);
    same_as_full(&mut cache, &s, &world);

    // A player value the world reads (a CDA power): Invalidation::PlayerValue.
    s.player_mut(p(1)).unwrap().set_life(5);
    world.life = s.player(p(1)).unwrap().life();
    cache.invalidate(Invalidation::PlayerValue);
    same_as_full(&mut cache, &s, &world);
    let env = GameEnvironment::new(&s, &world);
    assert_eq!(
        evaluate(s.effects(), &env, o(1))
            .unwrap()
            .characteristics
            .power,
        Some(crate::layers::literal(5))
    );

    // Generators and the PRNG: nothing to declare.
    s.stamp_resolved_effect().unwrap();
    s.prng_mut().next_u64();
    same_as_full(&mut cache, &s, &world);
}
