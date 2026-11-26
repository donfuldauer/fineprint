// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests: the real environment `GameEnvironment` answers effect existence from real
//! ability identity (T4); the PR #187 review scenario through it in both timestamp orders (T3);
//! the PR #202 review scenario (an in-force "lose all abilities" stops the entrant's own
//! "enters tapped") through it (T5). REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-001, REQ-DSL-004.
//! Black-box: written from the work-item brief (#183 FP-144) only.

#[path = "spec_layers_state_support.rs"]
mod support;

use fineprint_base::ids::{EffectId, ObjectId};
use fineprint_core::layers::{
    Evaluated, LayerEnvironment, LayerModification, ManaValueProvenance, OrderItem, OrderRecord,
    OrderStep, Selection, StampReason, evaluate, evaluate_all, evaluate_with_order,
};
use fineprint_core::replacement::{
    EtbAction, EtbApplied, EtbContext, EtbEffect, EtbEffects, EtbEntry, EtbScope, applicable,
    project,
};
use fineprint_core::state::{GameEnvironment, GameState, GeneratingAbility};
use support::*;

// ---------------------------------------------------------------------------------------------
// PR #187 scenario
// ---------------------------------------------------------------------------------------------

const S_A: u64 = 1;
const S_B: u64 = 2;
const T: u64 = 3;

/// B's generating ability (scenario PR #187).
fn b_ability() -> AbilityAST {
    // #232 FP-164: a valid inert ability instead of `{"synthetic":"marker_granter"}`.
    ability("marker_granter")
}

/// A's generating ability.
fn a_ability() -> AbilityAST {
    ability("test.ability_remover")
}

/// `sA` (with A's ability), `sB` (with B's ability) and `t` (no abilities).
fn scenario_world() -> World {
    let mut w = World::new();
    w.put(
        oid(S_A),
        chars("test.ability_remover", &["Enchantment"], vec![a_ability()]),
        P1,
    );
    w.put(
        oid(S_B),
        chars("test.marker_granter", &["Enchantment"], vec![b_ability()]),
        P1,
    );
    w.put(oid(T), chars("test.plain", &["Creature"], vec![]), P2);
    w
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    AThenB,
    BThenA,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BLink {
    Linked,
    Unlinked,
}

/// The scenario state: objects stamped in `order` (CR 613.7d), B (and A if `with_a`) registered
/// as static layer-6 effects with the timestamps of their sources (CR 613.7a); A and B are
/// registered in `a_first` order of ids. Returns the state and the ids of A (if any) and B.
fn scenario(
    order: Order,
    with_a: bool,
    b_link: BLink,
    a_first: bool,
) -> (GameState, Option<EffectId>, EffectId) {
    let mut s = GameState::new(187, 2, 20).unwrap();
    for _ in 0..3 {
        s.issue_object_id().unwrap();
    }
    s.stamp_object(oid(T), StampReason::EntersZone).unwrap();
    let (ta, tb) = match order {
        Order::AThenB => {
            let a = s.stamp_object(oid(S_A), StampReason::EntersZone).unwrap();
            let b = s.stamp_object(oid(S_B), StampReason::EntersZone).unwrap();
            (a, b)
        }
        Order::BThenA => {
            let b = s.stamp_object(oid(S_B), StampReason::EntersZone).unwrap();
            let a = s.stamp_object(oid(S_A), StampReason::EntersZone).unwrap();
            (a, b)
        }
    };
    assert_eq!(ta < tb, order == Order::AThenB);
    let a = static_sel(
        oid(S_A),
        ta,
        Layer::L6,
        SEL_ALL,
        LayerModification::RemoveAllAbilities,
    );
    let b = static_sel(
        oid(S_B),
        tb,
        Layer::L6,
        SEL_NO_ABILITIES,
        LayerModification::AddAbility(marker()),
    );
    let reg_a = |s: &mut GameState| {
        s.register_effect(
            a.clone(),
            Some(GeneratingAbility::new(oid(S_A), a_ability())),
        )
        .unwrap()
    };
    let reg_b = |s: &mut GameState| match b_link {
        BLink::Linked => s
            .register_effect(
                b.clone(),
                Some(GeneratingAbility::new(oid(S_B), b_ability())),
            )
            .unwrap(),
        BLink::Unlinked => {
            let id = s.issue_effect_id().unwrap();
            s.effects_mut().register(id, b.clone()).unwrap();
            id
        }
    };
    let (ida, idb) = if !with_a {
        (None, reg_b(&mut s))
    } else if a_first {
        let ida = reg_a(&mut s);
        (Some(ida), reg_b(&mut s))
    } else {
        let idb = reg_b(&mut s);
        (Some(reg_a(&mut s)), idb)
    };
    (s, ida, idb)
}

fn steps_of(record: &OrderRecord, id: EffectId) -> Vec<(usize, &OrderStep)> {
    record
        .steps
        .iter()
        .enumerate()
        .filter(|(_, st)| st.item == OrderItem::Effect(id))
        .collect()
}

fn has_marker(e: &Evaluated) -> bool {
    has_ability(&e.characteristics, &marker())
}

/// The objects of the scenario that end with the marker, evaluated through `GameEnvironment`
/// with both `evaluate_all` and per-object `evaluate` (which must agree).
fn marked_objects(s: &GameState, w: &World) -> Vec<u64> {
    let env = GameEnvironment::new(s, w);
    let all = evaluate_all(s.effects(), &env).unwrap();
    for o in [S_A, S_B, T] {
        assert_eq!(evaluate(s.effects(), &env, oid(o)).unwrap(), all[&oid(o)]);
    }
    all.iter()
        .filter(|(_, e)| has_marker(e))
        .map(|(id, _)| id.get())
        .collect()
}

// REQ-LAYER-002, REQ-LAYER-001, REQ-CORE-001, CR 613.8a, CR 613.7, CR 611.3b, CR 113.6
// (T3, scenario PR #187), timestamp order A before B, on B's source: A (layer 6, "remove all
// abilities" from every object) applies first by timestamp and removes B's generating ability,
// so B no longer exists and does not apply; B is not ordered after A by a (false) dependency:
// both steps are chosen by timestamp (CR 613.7).
#[test]
fn pr187_a_before_b_on_source_of_b() {
    let w = scenario_world();
    for a_first in [true, false] {
        let (s, ida, idb) = scenario(Order::AThenB, true, BLink::Linked, a_first);
        let ida = ida.unwrap();
        let env = GameEnvironment::new(&s, &w);
        let (eval, record) = evaluate_with_order(s.effects(), &env, oid(S_B)).unwrap();
        assert!(!has_marker(&eval));
        assert!(eval.characteristics.abilities.is_empty());
        let a_steps = steps_of(&record, ida);
        let b_steps = steps_of(&record, idb);
        assert_eq!(a_steps.len(), 1);
        assert_eq!(b_steps.len(), 1);
        assert!(a_steps[0].0 < b_steps[0].0, "A precedes B by timestamp");
        assert!(a_steps[0].1.applied);
        assert!(
            !b_steps[0].1.applied,
            "B's ability is gone: B does not apply"
        );
        assert_eq!(
            b_steps[0].1.selection,
            Selection::Timestamp,
            "B is chosen by timestamp, not released by a dependency"
        );
        assert_eq!(a_steps[0].1.selection, Selection::Timestamp);
    }
}

// REQ-LAYER-002, REQ-LAYER-001, REQ-CORE-001, CR 613.8a, CR 613.8b, CR 611.3b (T3, scenario
// PR #187), timestamp order B before A, on B's source. Per CR 613.8a(b) applying A would change
// the existence of B, so B depends on A (a genuine dependency, rules-researcher answer Q6) and
// per CR 613.8b waits until A has applied; once A applied, B no longer exists and does not
// apply.
#[test]
fn pr187_b_before_a_on_source_of_b() {
    let w = scenario_world();
    for a_first in [true, false] {
        let (s, ida, idb) = scenario(Order::BThenA, true, BLink::Linked, a_first);
        let ida = ida.unwrap();
        let env = GameEnvironment::new(&s, &w);
        let (eval, record) = evaluate_with_order(s.effects(), &env, oid(S_B)).unwrap();
        assert!(!has_marker(&eval));
        assert!(eval.characteristics.abilities.is_empty());
        let a_steps = steps_of(&record, ida);
        let b_steps = steps_of(&record, idb);
        assert_eq!(a_steps.len(), 1);
        assert_eq!(b_steps.len(), 1);
        assert!(a_steps[0].1.applied);
        assert!(!b_steps[0].1.applied, "B does not apply to its own source");
    }
}

// REQ-LAYER-002, CR 613.8a, CR 613.8b (T3, scenario PR #187), timestamp order B before A, the
// order record on B's source: A applies, B does not apply, and B is not ordered after A by a
// false dependency (the brief's T3 clause). Whether the record must show a CR 613.8a existence
// dependency on B's own source is not asserted here: the order record of an existence
// dependency on B's own source: #242.
#[test]
fn pr187_b_before_a_on_source_of_b_order_record() {
    let w = scenario_world();
    for a_first in [true, false] {
        let (s, ida, idb) = scenario(Order::BThenA, true, BLink::Linked, a_first);
        let ida = ida.unwrap();
        let env = GameEnvironment::new(&s, &w);
        let (_, record) = evaluate_with_order(s.effects(), &env, oid(S_B)).unwrap();
        let a_steps = steps_of(&record, ida);
        let b_steps = steps_of(&record, idb);
        assert!(!a_steps.is_empty(), "A has a step: {:?}", record.steps);
        assert!(a_steps.iter().all(|(_, st)| st.applied), "A applies");
        assert!(!b_steps.is_empty(), "B has a step: {:?}", record.steps);
        assert!(
            b_steps.iter().all(|(_, st)| !st.applied),
            "B does not apply"
        );
        for (_, st) in &b_steps {
            assert!(
                !matches!(st.selection, Selection::Dependency | Selection::Loop),
                "B is not ordered after A by a false dependency"
            );
        }
    }
}

// REQ-LAYER-002, REQ-LAYER-001, REQ-CORE-001, CR 613.8a, CR 611.3b, CR 113.6 (T3, scenario
// PR #187, issue acceptance criterion "no marker results"), timestamp order A before B, every
// object: A removes all abilities from every object, B's source included, so B's generating
// ability no longer exists and B does not apply to any object: neither `t` (the brief: "the
// marker is never present on t in the A-before-B order") nor A's own source.
#[test]
fn pr187_a_before_b_no_object_gets_the_marker() {
    let w = scenario_world();
    for a_first in [true, false] {
        let (s, _, idb) = scenario(Order::AThenB, true, BLink::Linked, a_first);
        assert_eq!(
            marked_objects(&s, &w),
            Vec::<u64>::new(),
            "objects with the marker (1 = sA, 2 = sB, 3 = t), a_first {a_first}"
        );
        // No false dependency on t either.
        let env = GameEnvironment::new(&s, &w);
        let (_, record_t) = evaluate_with_order(s.effects(), &env, oid(T)).unwrap();
        for (_, st) in steps_of(&record_t, idb) {
            assert!(
                !matches!(st.selection, Selection::Dependency | Selection::Loop),
                "B is not ordered after A by a false dependency on t"
            );
        }
    }
}

// REQ-LAYER-002, REQ-LAYER-001, REQ-CORE-001, CR 613.8a, CR 613.8b, CR 611.3b (T3, scenario
// PR #187, issue acceptance criterion "no marker results"), timestamp order B before A, every
// object: the same result as in the other order (rules-researcher answer Q6): no marker.
#[test]
fn pr187_b_before_a_no_object_gets_the_marker() {
    let w = scenario_world();
    for a_first in [true, false] {
        let (s, _, _) = scenario(Order::BThenA, true, BLink::Linked, a_first);
        assert_eq!(
            marked_objects(&s, &w),
            Vec::<u64>::new(),
            "objects with the marker (1 = sA, 2 = sB, 3 = t), a_first {a_first}"
        );
    }
}

// REQ-LAYER-002, REQ-LAYER-001, CR 611.3b, CR 613.5 (T3 control): without A, the object with
// no abilities gets the marker; B's source keeps its own ability and does not.
#[test]
fn pr187_control_without_a_marker_applies() {
    let w = scenario_world();
    for order in [Order::AThenB, Order::BThenA] {
        let (s, ida, _) = scenario(order, false, BLink::Linked, true);
        assert!(ida.is_none());
        let env = GameEnvironment::new(&s, &w);
        let t = evaluate(s.effects(), &env, oid(T)).unwrap();
        assert!(has_marker(&t), "{order:?}");
        let sb = evaluate(s.effects(), &env, oid(S_B)).unwrap();
        assert!(!has_marker(&sb));
        assert!(has_ability(&sb.characteristics, &b_ability()));
    }
}

// REQ-LAYER-001, REQ-LAYER-002 (T3 control): B registered through `effects_mut().register`
// with no link is treated as existing (legacy behaviour), so with A before B it applies to
// B's own source after A removed that source's abilities.
#[test]
fn pr187_control_unlinked_b_applies() {
    let w = scenario_world();
    let (s, _, idb) = scenario(Order::AThenB, true, BLink::Unlinked, true);
    assert_eq!(s.effect_generator(idb), None);
    let env = GameEnvironment::new(&s, &w);
    let sb = evaluate(s.effects(), &env, oid(S_B)).unwrap();
    assert!(has_marker(&sb), "an unlinked effect always exists");
    let t = evaluate(s.effects(), &env, oid(T)).unwrap();
    assert!(has_marker(&t));
}

// ---------------------------------------------------------------------------------------------
// T4: existence through GameEnvironment::generating_ability_exists
// ---------------------------------------------------------------------------------------------

fn view(abilities: Vec<AbilityAST>) -> Evaluated {
    Evaluated {
        characteristics: chars("test.marker_granter", &["Enchantment"], abilities),
        controller: Some(P1),
        mana_value_provenance: ManaValueProvenance::plain(),
    }
}

// REQ-LAYER-002, REQ-LAYER-001, REQ-DSL-004, CR 613.8a, CR 611.3b, CR 113.6 (T4): a linked
// effect exists iff its generating ability (by value) is among the source's abilities: the
// evaluated ones when the source is under evaluation, else the base ones; an unknown source
// does not exist; an unlinked id always exists; removing the effect removes its link.
#[test]
fn generating_ability_existence() {
    let w = scenario_world();
    let (mut s, _, idb) = scenario(Order::AThenB, false, BLink::Linked, true);
    let sb = oid(S_B);
    {
        let env = GameEnvironment::new(&s, &w);
        // Linked, present in the evaluated view (identity by value: a fresh equal value).
        assert!(env.generating_ability_exists(idb, sb, Some(&view(vec![b_ability()]))));
        assert!(env.generating_ability_exists(
            idb,
            sb,
            Some(&view(vec![ability("test.other"), b_ability()]))
        ));
        // Linked, absent from the evaluated view (even though the base has it).
        assert!(!env.generating_ability_exists(idb, sb, Some(&view(vec![]))));
        assert!(!env.generating_ability_exists(
            idb,
            sb,
            Some(&view(vec![ability("test.marker_granter_other")]))
        ));
        // No view: the base abilities decide.
        assert!(env.generating_ability_exists(idb, sb, None));
    }
    // No view, base without the ability.
    let mut stripped = w.clone();
    stripped.put(
        sb,
        chars(
            "test.marker_granter",
            &["Enchantment"],
            vec![ability("test.other")],
        ),
        P1,
    );
    assert!(!GameEnvironment::new(&s, &stripped).generating_ability_exists(idb, sb, None));
    // No view, unknown source.
    let mut gone = w.clone();
    gone.bases.remove(&sb);
    assert!(!GameEnvironment::new(&s, &gone).generating_ability_exists(idb, sb, None));
    // Unlinked ids: always true.
    let unlinked = s.issue_effect_id().unwrap();
    {
        let env = GameEnvironment::new(&s, &gone);
        assert!(env.generating_ability_exists(unlinked, sb, None));
        assert!(env.generating_ability_exists(unlinked, sb, Some(&view(vec![]))));
        assert!(env.generating_ability_exists(EffectId::new(9_999), sb, Some(&view(vec![]))));
    }
    // After remove_effect the link is gone.
    assert_eq!(
        s.effect_generator(idb),
        Some(&GeneratingAbility::new(sb, b_ability()))
    );
    assert!(s.remove_effect(idb));
    assert_eq!(s.effect_generator(idb), None);
    let env = GameEnvironment::new(&s, &stripped);
    assert!(env.generating_ability_exists(idb, sb, Some(&view(vec![]))));
}

// REQ-LAYER-001, REQ-CORE-001, CR 613.7c (T4): the environment's objects, bases and counters
// come from the world; counter timestamps come from the state's ledger.
#[test]
fn environment_reads_world_and_ledger() {
    let mut w = scenario_world();
    w.counter(oid(T), "+1/+1", 2);
    let mut s = GameState::new(1, 2, 20).unwrap();
    let t = s
        .stamp_counters(
            fineprint_core::layers::CounterHolder::Object(oid(T)),
            "+1/+1",
        )
        .unwrap();
    let env = GameEnvironment::new(&s, &w);
    assert_eq!(env.objects(), vec![oid(S_A), oid(S_B), oid(T)]);
    assert_eq!(env.base(oid(T)), w.bases.get(&oid(T)).cloned());
    assert_eq!(env.base(ObjectId::new(99)), None);
    assert_eq!(env.counters(oid(T)).get("+1/+1"), Some(&2));
    assert_eq!(env.counter_timestamp(oid(T), "+1/+1"), Some(t));
    assert_eq!(env.counter_timestamp(oid(T), "-1/-1"), None);
    assert_eq!(env.counter_timestamp(oid(S_A), "+1/+1"), None);
}

// ---------------------------------------------------------------------------------------------
// T5: PR #202 scenario
// ---------------------------------------------------------------------------------------------

const ENTRANT: u64 = 1;
const OTHER: u64 = 2;

fn etb_ability() -> AbilityAST {
    ability("test.enters_tapped")
}

fn other_etb_ability() -> AbilityAST {
    ability("test.others_enter_tapped")
}

// REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-001, CR 614.12, CR 613.1f, CR 611.3b (T5, scenario
// PR #202): an in-force layer-6 "lose all abilities" effect that applies to the entering
// permanent stops the permanent's own "enters tapped" from applying (the permanent as it would
// exist on the battlefield lacks the generating ability, CR 614.12); once that effect is gone
// it applies. An `OtherPermanents` effect of another source is judged by its own link.
#[test]
fn pr202_lose_all_abilities_stops_own_enters_tapped() {
    let mut w = World::new();
    w.put_off(
        oid(ENTRANT),
        chars("test.tapped_entrant", &["Creature"], vec![etb_ability()]),
        P1,
    );
    w.put(
        oid(OTHER),
        chars("test.tapper", &["Artifact"], vec![other_etb_ability()]),
        P2,
    );
    let mut s = GameState::new(202, 2, 20).unwrap();
    s.issue_object_id().unwrap();
    s.issue_object_id().unwrap();
    s.stamp_object(oid(OTHER), StampReason::EntersZone).unwrap();
    let t = s.stamp_resolved_effect().unwrap();
    let a = s
        .register_effect(
            resolved(
                t,
                Layer::L6,
                &[ENTRANT],
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap();
    let own = s.issue_effect_id().unwrap();
    s.link_effect_generator(own, GeneratingAbility::new(oid(ENTRANT), etb_ability()))
        .unwrap();
    let other = s.issue_effect_id().unwrap();
    s.link_effect_generator(
        other,
        GeneratingAbility::new(oid(OTHER), other_etb_ability()),
    )
    .unwrap();
    let mut etb = EtbEffects::new();
    etb.add(
        EtbEffect::new(own, EtbAction::EntersTapped)
            .with_source(oid(ENTRANT))
            .with_scope(EtbScope::ThisPermanent),
    )
    .unwrap();
    etb.add(
        EtbEffect::new(
            other,
            EtbAction::EntersWithCounters {
                kind: "test.counter".into(),
                amount: 1,
            },
        )
        .with_source(oid(OTHER))
        .with_scope(EtbScope::OtherPermanents),
    )
    .unwrap();
    let entry = EtbEntry {
        object: oid(ENTRANT),
        controller: P1,
        own_statics: Vec::new(),
    };
    let none = EtbApplied::default();

    // With A in force.
    {
        let env = GameEnvironment::new(&s, &w);
        let ctx = EtbContext {
            env: &env,
            continuous: s.effects(),
            effects: &etb,
        };
        let projected = project(&ctx, &entry, &none).unwrap();
        assert!(projected.evaluated.characteristics.abilities.is_empty());
        assert!(!projected.tapped);
        let list = applicable(&ctx, &entry, &none).unwrap();
        assert!(
            !list.contains(&own),
            "lose-all-abilities in force: own enters-tapped does not apply ({list:?})"
        );
        assert!(
            list.contains(&other),
            "the other source's effect is judged by its own link"
        );
    }

    // After A is removed.
    assert!(s.remove_effect(a));
    let env = GameEnvironment::new(&s, &w);
    let ctx = EtbContext {
        env: &env,
        continuous: s.effects(),
        effects: &etb,
    };
    let projected = project(&ctx, &entry, &none).unwrap();
    assert!(has_ability(
        &projected.evaluated.characteristics,
        &etb_ability()
    ));
    let list = applicable(&ctx, &entry, &none).unwrap();
    assert!(
        list.contains(&own),
        "own enters-tapped applies again ({list:?})"
    );
    assert!(list.contains(&other));
    // Applying it makes the projection tapped.
    let mut applied = EtbApplied::default();
    applied.effects.insert(own);
    assert!(project(&ctx, &entry, &applied).unwrap().tapped);

    // Control: the own effect linked to an ability the entrant does not have never applies.
    let mut s2 = GameState::new(202, 2, 20).unwrap();
    s2.issue_object_id().unwrap();
    let own2 = s2.issue_effect_id().unwrap();
    s2.link_effect_generator(
        own2,
        GeneratingAbility::new(oid(ENTRANT), ability("test.absent")),
    )
    .unwrap();
    let mut etb2 = EtbEffects::new();
    etb2.add(
        EtbEffect::new(own2, EtbAction::EntersTapped)
            .with_source(oid(ENTRANT))
            .with_scope(EtbScope::ThisPermanent),
    )
    .unwrap();
    let env2 = GameEnvironment::new(&s2, &w);
    let ctx2 = EtbContext {
        env: &env2,
        continuous: s2.effects(),
        effects: &etb2,
    };
    assert!(!applicable(&ctx2, &entry, &none).unwrap().contains(&own2));
}
