// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-002 items 3 (dependency, CR 613.8, 613.8a, 613.8b), 4 (the SRS 6.2
//! timestamp-not-dependency case, CR 613.7, 613.4b) and 6 (counters ordered among effects by
//! their counter timestamp, CR 613.7c, 613.4c). Black-box: written from the work-item brief and
//! SRS 6.2 only; synthetic reproductions of the SRS 6.2 canonical examples with neutral names
//! (`test.remover`, `test.dependent`, `test.type_changer`). Every scenario also checks
//! `evaluate == evaluate_with_order(..).0` (item 9).

#[path = "spec_layers_ordering_support.rs"]
mod support;

use fineprint_core::layers::{
    Affected, KEYWORD_COUNTER_KINDS, LayerModification, OrderItem, Selection,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ColorBitmask;
use support::*;

/// The subject: a creature with one marker ability (keyword `test.marker`).
fn marked_creature() -> fineprint_core::objects::Characteristics {
    let mut c = creature(2, 2);
    c.abilities = vec![keyword_ability("test.marker", "test.ability.marker")];
    c
}

// ---------------------------------------------------------------------------------------------
// 3a: ability-removal dependency (SRS 6.2 canonical example, synthetic)
// ---------------------------------------------------------------------------------------------

const REMOVER: u64 = 10;
const DEPENDENT: u64 = 20;

/// `test.remover` (layer 6, RemoveAllAbilities on every object) and `test.dependent` (layer 6,
/// applies only while the object has the marker ability, adds a marker ability).
fn removal_pair(remover_ts: u64, dependent_ts: u64) -> fineprint_core::layers::ContinuousEffects {
    registry(&[
        (
            REMOVER,
            eff(
                Layer::L6,
                remover_ts,
                Affected::Selector(any()),
                LayerModification::RemoveAllAbilities,
            ),
        ),
        (
            DEPENDENT,
            eff(
                Layer::L6,
                dependent_ts,
                sel(has_kw("test.marker")),
                add_ability("test.dependent_marker"),
            ),
        ),
    ])
}

/// REQ-LAYER-002, CR 613.8a, 613.8b: the dependent effect is earlier; applying the remover
/// changes what the dependent applies to, so the remover applies first (chosen by Dependency)
/// and the dependent waits until just after it, then no longer applies.
#[test]
fn ability_removal_dependency_dependent_earlier() {
    let (ev, rec) = run(&removal_pair(2, 1), &Env::subject(marked_creature()));
    assert!(ev.characteristics.abilities.is_empty());
    assert_eq!(
        steps_in(&rec, Layer::L6),
        vec![
            fx(Layer::L6, REMOVER, Selection::Dependency, true),
            fx(Layer::L6, DEPENDENT, Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8a, 613.8b, 613.7: the remover is earlier; it applies first by
/// timestamp, the dependent no longer applies; the result equals the other timestamp order.
#[test]
fn ability_removal_dependency_remover_earlier() {
    let env = Env::subject(marked_creature());
    let (ev, rec) = run(&removal_pair(1, 2), &env);
    assert!(ev.characteristics.abilities.is_empty());
    assert_eq!(
        steps_in(&rec, Layer::L6),
        vec![
            fx(Layer::L6, REMOVER, Selection::Timestamp, true),
            fx(Layer::L6, DEPENDENT, Selection::Timestamp, false),
        ]
    );
    let (other, _) = run(&removal_pair(2, 1), &env);
    assert_eq!(ev, other, "identical across the two timestamp orders");
}

/// REQ-LAYER-002, CR 613.8a, 613.8b: an observable variant. `test.remover` removes only the
/// marker keyword; `test.dependent` (earlier) adds a second ability only while the object has
/// the marker. Dependency puts the remover first, so the second ability is never added, in both
/// timestamp orders (timestamp order alone would have added it when the dependent is earlier).
#[test]
fn keyword_removal_dependency_is_observable_both_orders() {
    let env = Env::subject(marked_creature());
    let mut results = Vec::new();
    for (remover_ts, dependent_ts, remover_sel) in
        [(2, 1, Selection::Dependency), (1, 2, Selection::Timestamp)]
    {
        let r = registry(&[
            (
                REMOVER,
                eff(
                    Layer::L6,
                    remover_ts,
                    subject(),
                    LayerModification::RemoveKeyword("test.marker".to_owned()),
                ),
            ),
            (
                DEPENDENT,
                eff(
                    Layer::L6,
                    dependent_ts,
                    sel(has_kw("test.marker")),
                    add_ability("test.dependent_marker"),
                ),
            ),
        ]);
        let (ev, rec) = run(&r, &env);
        assert!(!has_keyword(&ev.characteristics, "test.marker"));
        assert!(!has_keyword(&ev.characteristics, "test.dependent_marker"));
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![
                fx(Layer::L6, REMOVER, remover_sel, true),
                fx(Layer::L6, DEPENDENT, Selection::Timestamp, false),
            ]
        );
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}

// ---------------------------------------------------------------------------------------------
// 3b: type-change dependency (SRS 6.2 canonical example, synthetic)
// ---------------------------------------------------------------------------------------------

const TYPE_CHANGER: u64 = 10;
const CREATURE_ONLY: u64 = 20;

/// REQ-LAYER-002, CR 613.8a, 613.8b: layer 4, `test.type_changer` (AddTypes Creature) and an
/// effect that applies only to creatures (adds a subtype). With the creatures-only effect
/// earlier, the type change still applies first and the creatures-only effect then applies;
/// the reverse timestamp order gives the same characteristics.
#[test]
fn type_change_dependency_add_types_both_orders() {
    let env = Env::subject(noncreature("Artifact"));
    let mut results = Vec::new();
    for (changer_ts, dependent_ts, changer_sel) in
        [(2, 1, Selection::Dependency), (1, 2, Selection::Timestamp)]
    {
        let r = registry(&[
            (
                TYPE_CHANGER,
                eff(Layer::L4, changer_ts, subject(), add_card_type("Creature")),
            ),
            (
                CREATURE_ONLY,
                eff(
                    Layer::L4,
                    dependent_ts,
                    sel(is_creature()),
                    add_subtype("TestKind"),
                ),
            ),
        ]);
        let (ev, rec) = run(&r, &env);
        assert_eq!(ev.characteristics.card_types, set(["Artifact", "Creature"]));
        assert_eq!(ev.characteristics.subtypes, set(["TestKind"]));
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                fx(Layer::L4, TYPE_CHANGER, changer_sel, true),
                fx(Layer::L4, CREATURE_ONLY, Selection::Timestamp, true),
            ]
        );
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}

/// REQ-LAYER-002, CR 613.8a, 613.8b: the same dependency with a SetTypes type change (the card
/// types become exactly Creature), both timestamp orders, identical results.
#[test]
fn type_change_dependency_set_types_both_orders() {
    let env = Env::subject(noncreature("Artifact"));
    let mut results = Vec::new();
    for (changer_ts, dependent_ts) in [(2, 1), (1, 2)] {
        let r = registry(&[
            (
                TYPE_CHANGER,
                eff(
                    Layer::L4,
                    changer_ts,
                    subject(),
                    LayerModification::SetTypes {
                        supertypes: None,
                        card_types: Some(set(["Creature"])),
                        subtypes: None,
                    },
                ),
            ),
            (
                CREATURE_ONLY,
                eff(
                    Layer::L4,
                    dependent_ts,
                    sel(is_creature()),
                    add_subtype("TestKind"),
                ),
            ),
        ]);
        let (ev, rec) = run(&r, &env);
        assert_eq!(ev.characteristics.card_types, set(["Creature"]));
        assert_eq!(ev.characteristics.subtypes, set(["TestKind"]));
        assert_eq!(
            items_in(&rec, Layer::L4),
            vec![item(TYPE_CHANGER), item(CREATURE_ONLY)]
        );
        assert!(steps_in(&rec, Layer::L4).iter().all(|s| s.applied));
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}

// ---------------------------------------------------------------------------------------------
// 3c: waiting for every effect depended on; released dependents in timestamp order
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.8b: two creatures-only effects both depend on one type change; they
/// are released together and apply just after it, in timestamp order relative to each other
/// (both timestamp orders of the two dependents; their ids oppose one of the orders).
#[test]
fn released_dependents_apply_in_timestamp_order() {
    for (d1_ts, d2_ts) in [(1, 2), (2, 1)] {
        let r = registry(&[
            (30, eff(Layer::L4, 3, subject(), add_card_type("Creature"))),
            (
                10,
                eff(Layer::L4, d1_ts, sel(is_creature()), add_subtype("KindOne")),
            ),
            (
                20,
                eff(Layer::L4, d2_ts, sel(is_creature()), add_subtype("KindTwo")),
            ),
        ]);
        let (ev, rec) = run(&r, &Env::subject(noncreature("Artifact")));
        assert_eq!(ev.characteristics.subtypes, set(["KindOne", "KindTwo"]));
        let (first, second) = if d1_ts < d2_ts { (10, 20) } else { (20, 10) };
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                fx(Layer::L4, 30, Selection::Dependency, true),
                fx(Layer::L4, first, Selection::Timestamp, true),
                fx(Layer::L4, second, Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: an effect that applies to objects that are exactly one of
/// creature and artifact depends on both type changes (one adds Creature, one adds Artifact),
/// and still depends on the second after the first is applied. It waits until just after both
/// are applied, so it does not apply (the object is then both). Timestamp order alone would
/// have applied it first (to a plain enchantment, so not at all) and recorded it first; the
/// record shows it last, in both timestamp orders of the two type changes.
#[test]
fn dependent_waits_for_all_effects_it_depends_on() {
    for (a1_ts, a2_ts) in [(2, 3), (3, 2)] {
        let r = registry(&[
            (
                10,
                eff(
                    Layer::L4,
                    1,
                    sel(xor(is_creature(), has_card_type("Artifact"))),
                    add_subtype("Waited"),
                ),
            ),
            (
                20,
                eff(Layer::L4, a1_ts, subject(), add_card_type("Creature")),
            ),
            (
                30,
                eff(Layer::L4, a2_ts, subject(), add_card_type("Artifact")),
            ),
        ]);
        let (ev, rec) = run(&r, &Env::subject(noncreature("Enchantment")));
        assert_eq!(
            ev.characteristics.card_types,
            set(["Artifact", "Creature", "Enchantment"])
        );
        assert!(!ev.characteristics.subtypes.contains("Waited"));
        let (first, second) = if a1_ts < a2_ts { (20, 30) } else { (30, 20) };
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                fx(Layer::L4, first, Selection::Dependency, true),
                fx(Layer::L4, second, Selection::Dependency, true),
                fx(Layer::L4, 10, Selection::Timestamp, false),
            ]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 3d: independent effects in timestamp order
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.7, 613.8a: effects in the same layer with no 613.8a (b) relation apply
/// in timestamp order (not id order), each recorded as Timestamp; a locked-set effect that does
/// not include the object is recorded as not applied.
#[test]
fn independent_effects_apply_in_timestamp_order() {
    let r = registry(&[
        (1, eff(Layer::L4, 2, subject(), add_subtype("KindA"))),
        (2, eff(Layer::L4, 1, subject(), add_subtype("KindB"))),
        (
            3,
            eff(Layer::L4, 4, sel(is_creature()), add_subtype("KindS")),
        ),
        (4, eff(Layer::L4, 3, elsewhere(), add_subtype("KindX"))),
        (
            5,
            eff(
                Layer::L5,
                6,
                subject(),
                LayerModification::AddColors(ColorBitmask::RED),
            ),
        ),
        (
            6,
            eff(
                Layer::L5,
                5,
                subject(),
                LayerModification::AddColors(ColorBitmask::BLUE),
            ),
        ),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
    assert_eq!(
        ev.characteristics.subtypes,
        set(["KindA", "KindB", "KindS"])
    );
    assert_eq!(
        rec.steps,
        vec![
            fx(Layer::L4, 2, Selection::Timestamp, true),
            fx(Layer::L4, 1, Selection::Timestamp, true),
            fx(Layer::L4, 4, Selection::Timestamp, false),
            fx(Layer::L4, 3, Selection::Timestamp, true),
            fx(Layer::L5, 6, Selection::Timestamp, true),
            fx(Layer::L5, 5, Selection::Timestamp, true),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// 3f: no dependency across layers
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.8a: a layer-4 effect that applies only to red objects and a layer-5
/// effect that makes the object red are in different layers, so never dependent: layer order
/// wins, the layer-4 effect is evaluated first and does not apply, in both timestamp orders.
#[test]
fn different_layers_never_dependent_both_orders() {
    for (t4, t5) in [(2, 1), (1, 2)] {
        let r = registry(&[
            (
                1,
                eff(Layer::L4, t4, sel(has_color("red")), add_subtype("RedKind")),
            ),
            (
                2,
                eff(
                    Layer::L5,
                    t5,
                    subject(),
                    LayerModification::SetColors(ColorBitmask::RED),
                ),
            ),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
        assert_eq!(ev.characteristics.colors, ColorBitmask::RED);
        assert!(!ev.characteristics.subtypes.contains("RedKind"));
        assert_eq!(
            rec.steps,
            vec![
                fx(Layer::L4, 1, Selection::Timestamp, false),
                fx(Layer::L5, 2, Selection::Timestamp, true),
            ]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 4: timestamp, not dependency (SRS 6.2 layer-7 case)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.4b, 613.7: two layer-7b effects set P/T to different values; they do
/// not satisfy 613.8a (b), so the later timestamp wins, swapping the timestamps swaps the
/// result, and every step is recorded as Timestamp.
#[test]
fn two_set_pt_effects_follow_timestamp_order() {
    for (t1, t2, expected, first, second) in [(1, 2, (5, 5), 1, 2), (2, 1, (2, 2), 2, 1)] {
        let r = registry(&[
            (1, eff(Layer::L7b, t1, subject(), set_pt(2, 2))),
            (2, eff(Layer::L7b, t2, subject(), set_pt(5, 5))),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(1, 1)));
        assert_eq!(pt(&ev.characteristics), Some(expected));
        assert_eq!(
            rec.steps,
            vec![
                fx(Layer::L7b, first, Selection::Timestamp, true),
                fx(Layer::L7b, second, Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.4b, 613.4c, 613.7: a 7b SetPt and a 7c ModifyPt are in different
/// sublayers: 7b applies before 7c whatever the timestamps, and both are recorded as Timestamp.
#[test]
fn set_then_modify_pt_in_sublayer_order_both_orders() {
    for (t_set, t_mod) in [(1, 2), (2, 1)] {
        let r = registry(&[
            (1, eff(Layer::L7c, t_mod, subject(), modify_pt(1, 1))),
            (2, eff(Layer::L7b, t_set, subject(), set_pt(2, 2))),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(1, 1)));
        assert_eq!(pt(&ev.characteristics), Some((3, 3)));
        assert_eq!(
            rec.steps,
            vec![
                fx(Layer::L7b, 2, Selection::Timestamp, true),
                fx(Layer::L7c, 1, Selection::Timestamp, true),
            ]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 6: counters as steps (CR 613.7c, 613.4c; keyword counters in layer 6)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.7c, 613.4c, 613.7: a P/T counter kind is one step of layer 7c, ordered
/// among the 7c effects by its counter timestamp and recorded as `OrderItem::Counter(kind)`.
#[test]
fn pt_counters_ordered_by_counter_timestamp() {
    let r = registry(&[
        (1, eff(Layer::L7c, 1, subject(), modify_pt(1, 0))),
        (2, eff(Layer::L7c, 3, subject(), modify_pt(0, 1))),
    ]);
    let env = Env::subject(creature(2, 2)).counter("+1/+1", 1, 2);
    let (ev, rec) = run(&r, &env);
    assert_eq!(pt(&ev.characteristics), Some((4, 4)));
    assert_eq!(
        rec.steps,
        vec![
            fx(Layer::L7c, 1, Selection::Timestamp, true),
            ctr(Layer::L7c, "+1/+1", Selection::Timestamp, true),
            fx(Layer::L7c, 2, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.7c, 613.7: at equal timestamps an effect step comes before a counter
/// step (the pre-existing tie-break).
#[test]
fn pt_counter_after_effect_at_equal_timestamp() {
    let r = registry(&[(1, eff(Layer::L7c, 4, subject(), modify_pt(1, 1)))]);
    let env = Env::subject(creature(2, 2)).counter("-1/-1", 1, 4);
    let (ev, rec) = run(&r, &env);
    assert_eq!(pt(&ev.characteristics), Some((2, 2)));
    assert_eq!(
        rec.steps,
        vec![
            fx(Layer::L7c, 1, Selection::Timestamp, true),
            ctr(Layer::L7c, "-1/-1", Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.4b, 613.4c, 613.7c: a P/T counter with an earlier counter timestamp
/// still applies in 7c, after a later 7b SetPt (sublayer order wins over timestamps).
#[test]
fn pt_counter_after_set_pt_whatever_the_timestamps() {
    let r = registry(&[(1, eff(Layer::L7b, 5, subject(), set_pt(0, 0)))]);
    let env = Env::subject(creature(2, 2)).counter("+1/+1", 1, 1);
    let (ev, rec) = run(&r, &env);
    assert_eq!(pt(&ev.characteristics), Some((1, 1)));
    assert_eq!(
        rec.steps,
        vec![
            fx(Layer::L7b, 1, Selection::Timestamp, true),
            ctr(Layer::L7c, "+1/+1", Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.7c, 613.7: a keyword counter is one step of layer 6, ordered among the
/// layer-6 effects by its counter timestamp: before a later RemoveAllAbilities it is removed,
/// after an earlier one it stays. The record names `OrderItem::Counter(kind)`.
#[test]
fn keyword_counter_ordered_by_counter_timestamp_both_orders() {
    let kind = KEYWORD_COUNTER_KINDS[0];
    for (counter_ts, keeps) in [(1, false), (3, true)] {
        let r = registry(&[(
            1,
            eff(
                Layer::L6,
                2,
                subject(),
                LayerModification::RemoveAllAbilities,
            ),
        )]);
        let env = Env::subject(creature(2, 2)).counter(kind, 1, counter_ts);
        let (ev, rec) = run(&r, &env);
        assert_eq!(has_keyword(&ev.characteristics, kind), keeps);
        let counter = ctr(Layer::L6, kind, Selection::Timestamp, true);
        let effect = fx(Layer::L6, 1, Selection::Timestamp, true);
        let expected = if keeps {
            vec![effect, counter]
        } else {
            vec![counter, effect]
        };
        assert_eq!(rec.steps, expected);
        assert!(
            rec.steps
                .iter()
                .any(|s| s.item == OrderItem::Counter(kind.to_owned()))
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Existence of a static effect's generating ability (CR 613.8a (b), "existence"; review F5)
// ---------------------------------------------------------------------------------------------

const GEN_REMOVER: u64 = 30;
const GENERATED: u64 = 40;

/// A creature whose abilities are exactly the given keywords.
fn creature_with(kws: &[&str]) -> fineprint_core::objects::Characteristics {
    let mut c = creature(2, 2);
    c.abilities = kws
        .iter()
        .map(|kw| keyword_ability(kw, &format!("test.ability.{kw}")))
        .collect();
    c
}

/// A (`GEN_REMOVER`): resolved, layer 6, every object, removes keyword `removed`.
/// B (`GENERATED`): `b` (built with `b_ts`), layer 6, every object, adds `test.b_marker`.
fn existence_pair(
    a_ts: u64,
    removed: &str,
    b: fn(Layer, u64, Affected, LayerModification) -> fineprint_core::layers::ContinuousEffect,
    b_ts: u64,
) -> fineprint_core::layers::ContinuousEffects {
    registry(&[
        (
            GEN_REMOVER,
            eff(
                Layer::L6,
                a_ts,
                Affected::Selector(any()),
                LayerModification::RemoveKeyword(removed.to_owned()),
            ),
        ),
        (
            GENERATED,
            b(
                Layer::L6,
                b_ts,
                Affected::Selector(any()),
                add_ability("test.b_marker"),
            ),
        ),
    ])
}

/// B as a static effect of the subject's own ability.
fn b_static(
    l: Layer,
    t: u64,
    a: Affected,
    m: LayerModification,
) -> fineprint_core::layers::ContinuousEffect {
    static_eff(l, t, o(SUBJECT), a, m)
}

/// B as a resolved effect whose source is the subject.
fn b_resolved(
    l: Layer,
    t: u64,
    a: Affected,
    m: LayerModification,
) -> fineprint_core::layers::ContinuousEffect {
    fineprint_core::layers::ContinuousEffect {
        source: Some(o(SUBJECT)),
        ..eff(l, t, a, m)
    }
}

/// B as a static effect with no source.
fn b_static_sourceless(
    l: Layer,
    t: u64,
    a: Affected,
    m: LayerModification,
) -> fineprint_core::layers::ContinuousEffect {
    fineprint_core::layers::ContinuousEffect {
        source: None,
        ..static_eff(l, t, o(SUBJECT), a, m)
    }
}

/// The environment: the subject has exactly `kws`; B is generated by `test.gen_ability`.
fn existence_env(kws: &[&str]) -> Env {
    Env::subject(creature_with(kws)).generated_by(e(GENERATED), "test.gen_ability")
}

/// REQ-LAYER-002, CR 613.8a, 613.8b: B (static, generated by the subject's `test.gen_ability`,
/// selector matches every object) is earlier; A (later) removes `test.gen_ability`. Applying A
/// would change B's existence, so B depends on A: A applies first (chosen by Dependency), then B
/// no longer exists and does not apply. No `test.b_marker` is granted.
#[test]
fn generating_ability_removed_static_effect_earlier() {
    let env = existence_env(&["test.gen_ability"]);
    let (ev, rec) = run(&existence_pair(2, "test.gen_ability", b_static, 1), &env);
    assert!(!has_keyword(&ev.characteristics, "test.b_marker"));
    assert!(!has_keyword(&ev.characteristics, "test.gen_ability"));
    assert!(ev.characteristics.abilities.is_empty());
    assert_eq!(
        steps_in(&rec, Layer::L6),
        vec![
            fx(Layer::L6, GEN_REMOVER, Selection::Dependency, true),
            fx(Layer::L6, GENERATED, Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8a, 613.7: A (removes `test.gen_ability`) is earlier and applies by
/// timestamp; B's generating ability no longer exists, so B does not apply. The result equals
/// the other timestamp order (no `test.b_marker`).
#[test]
fn generating_ability_removed_remover_earlier_both_orders_agree() {
    let env = existence_env(&["test.gen_ability"]);
    let (ev, rec) = run(&existence_pair(1, "test.gen_ability", b_static, 2), &env);
    assert!(!has_keyword(&ev.characteristics, "test.b_marker"));
    assert!(ev.characteristics.abilities.is_empty());
    assert_eq!(
        steps_in(&rec, Layer::L6),
        vec![
            fx(Layer::L6, GEN_REMOVER, Selection::Timestamp, true),
            fx(Layer::L6, GENERATED, Selection::Timestamp, false),
        ]
    );
    assert!(
        env.hook_effects().contains(&e(GENERATED)),
        "the hook is consulted for the static effect"
    );
    let (other, _) = run(
        &existence_pair(2, "test.gen_ability", b_static, 1),
        &existence_env(&["test.gen_ability"]),
    );
    assert_eq!(ev, other, "identical across the two timestamp orders");
}

/// REQ-LAYER-002, CR 613.8a, 613.7 (control): A removes `test.other`, not the generating
/// ability. B's existence is unchanged, so there is no dependency: both steps by Timestamp, both
/// applied, `test.b_marker` granted, in both timestamp orders.
#[test]
fn generating_ability_survives_control_both_orders() {
    let mut results = Vec::new();
    for (a_ts, b_ts, expected) in [
        (1, 2, [GEN_REMOVER, GENERATED]),
        (2, 1, [GENERATED, GEN_REMOVER]),
    ] {
        let env = existence_env(&["test.gen_ability", "test.other"]);
        let (ev, rec) = run(&existence_pair(a_ts, "test.other", b_static, b_ts), &env);
        assert!(
            has_keyword(&ev.characteristics, "test.b_marker"),
            "{a_ts}/{b_ts}"
        );
        assert!(has_keyword(&ev.characteristics, "test.gen_ability"));
        assert!(!has_keyword(&ev.characteristics, "test.other"));
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![
                fx(Layer::L6, expected[0], Selection::Timestamp, true),
                fx(Layer::L6, expected[1], Selection::Timestamp, true),
            ],
            "A at {a_ts}, B at {b_ts}"
        );
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}

/// REQ-LAYER-002, CR 613.8a: a `Resolved` effect is not generated by an ability, so the
/// existence hook is never consulted for it even with a `generated_by` entry and a source: B
/// applies and grants `test.b_marker` in both timestamp orders, both steps by Timestamp.
#[test]
fn resolved_effect_ignores_generating_ability_both_orders() {
    let mut results = Vec::new();
    for (a_ts, b_ts, expected) in [
        (1, 2, [GEN_REMOVER, GENERATED]),
        (2, 1, [GENERATED, GEN_REMOVER]),
    ] {
        let env = existence_env(&["test.gen_ability"]);
        let (ev, rec) = run(
            &existence_pair(a_ts, "test.gen_ability", b_resolved, b_ts),
            &env,
        );
        assert!(
            has_keyword(&ev.characteristics, "test.b_marker"),
            "{a_ts}/{b_ts}"
        );
        assert!(!has_keyword(&ev.characteristics, "test.gen_ability"));
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![
                fx(Layer::L6, expected[0], Selection::Timestamp, true),
                fx(Layer::L6, expected[1], Selection::Timestamp, true),
            ],
            "A at {a_ts}, B at {b_ts}"
        );
        assert!(
            env.hook_effects().is_empty(),
            "hook consulted for a resolved effect: {:?}",
            env.hook_calls.borrow()
        );
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}

/// REQ-LAYER-002, CR 613.8a: a `Static` effect with no source is not subject to the existence
/// hook: B applies and grants `test.b_marker` in both timestamp orders, both steps by Timestamp.
#[test]
fn sourceless_static_effect_ignores_generating_ability_both_orders() {
    let mut results = Vec::new();
    for (a_ts, b_ts, expected) in [
        (1, 2, [GEN_REMOVER, GENERATED]),
        (2, 1, [GENERATED, GEN_REMOVER]),
    ] {
        let env = existence_env(&["test.gen_ability"]);
        let (ev, rec) = run(
            &existence_pair(a_ts, "test.gen_ability", b_static_sourceless, b_ts),
            &env,
        );
        assert!(
            has_keyword(&ev.characteristics, "test.b_marker"),
            "{a_ts}/{b_ts}"
        );
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![
                fx(Layer::L6, expected[0], Selection::Timestamp, true),
                fx(Layer::L6, expected[1], Selection::Timestamp, true),
            ],
            "A at {a_ts}, B at {b_ts}"
        );
        assert!(
            env.hook_effects().is_empty(),
            "hook consulted for a sourceless effect: {:?}",
            env.hook_calls.borrow()
        );
        results.push(ev);
    }
    assert_eq!(results[0], results[1]);
}
