// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for copiable values and layer-1a copy effects (REQ-OBJ-002, REQ-LAYER-001):
//! copy of a copy, copy of a face-down object, copy of a prototyped permanent, "as enters"
//! power and toughness, and `copiable_characteristics`. Black-box: written from the work-item
//! brief only.

#[path = "spec_layers_copy_support.rs"]
mod support;

use fineprint_core::layers::{
    Affected, ContinuousEffects, CopiableValues, CopyException, EffectOrigin, LayerError,
    LayerModification, ValueSource, copiable_characteristics, copiable_effect, evaluate,
};
use fineprint_core::layouts::{CastChoice, LayoutQuery, base_characteristics};
use fineprint_core::objects::{Characteristics, ManaValueContext, VariableValues};
use fineprint_core::zones::ZoneKind;
use support::*;

fn env_with(objects: &[(u64, Characteristics)]) -> TestEnv {
    let mut env = TestEnv::new();
    for (n, c) in objects {
        env.put(oid(*n), c.clone());
    }
    env
}

fn copiable(effects: &ContinuousEffects, env: &TestEnv, n: u64) -> Characteristics {
    norm(&copiable_characteristics(effects, env, oid(n)).expect("known object"))
}

// ---------------------------------------------------------------------------------------------
// copiable_effect shape
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, REQ-LAYER-001, CR 613.2a: `copiable_effect` builds a layer-1a, non-CDA
/// `CopyValues` effect with the given source, controller, origin, timestamp and locked set.
#[test]
fn copiable_effect_is_a_layer_1a_copy_values_effect() {
    let v = CopiableValues::plain(rich_a());
    let e = copiable_effect(
        &v,
        Some(oid(7)),
        P2,
        EffectOrigin::Static { granted_at: None },
        ts(42),
        oset(&[oid(3), oid(4)]),
    );
    assert_eq!(e.layer, Layer::L1a);
    assert_eq!(e.source, Some(oid(7)));
    assert_eq!(e.controller, P2);
    assert_eq!(e.origin, EffectOrigin::Static { granted_at: None });
    assert_eq!(e.timestamp, ts(42));
    assert!(!e.is_cda);
    assert_eq!(e.affected, Affected::Objects(oset(&[oid(3), oid(4)])));
    assert_eq!(
        e.modification,
        LayerModification::CopyValues(Box::new(rich_a()))
    );
    assert!(e.modification.belongs_to(Layer::L1a));
}

// ---------------------------------------------------------------------------------------------
// 1. copy of a copy
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 707.2, 707.3: B copies A with an exception, C copies B; C has B's copiable
/// values including the exception, both through `CopiableValues` and through
/// `copiable_characteristics(B)`.
#[test]
fn copy_of_a_copy_707_3_carries_the_first_copys_exception() {
    let env = env_with(&[(1, rich_a()), (2, own_b()), (3, own_c())]);
    let va = CopiableValues::plain(rich_a());
    let vb = va.copy_with(&own_b(), &[CopyException::AddSubtypes(set(&["Mutant"]))]);
    let mut expected_b = rich_a();
    expected_b.subtypes = set(&["Elf", "Mutant"]);
    assert_eq!(vb.characteristics, expected_b);
    assert_eq!(vb.source, ValueSource::Plain);

    let mut effects = ContinuousEffects::new();
    effects
        .register(eid(1), copy_eff(&vb, 10, &[oid(2)]))
        .unwrap();

    // C copies B, reading B's copiable values from the layer system (CR 613.2c).
    let from_layers = CopiableValues::plain(
        copiable_characteristics(&effects, &env, oid(2)).expect("B is known"),
    );
    let vc = from_layers.copy_with(&own_c(), &[]);
    // ... and directly from B's CopiableValues: the same values.
    let vc_direct = vb.copy_with(&own_c(), &[]);
    assert_eq!(norm(&vc.characteristics), norm(&vc_direct.characteristics));
    assert_eq!(norm(&vc_direct.characteristics), norm(&expected_b));

    effects
        .register(eid(2), copy_eff(&vc, 20, &[oid(3)]))
        .unwrap();
    assert_eq!(eval_chars(&effects, &env, oid(2)), norm(&expected_b));
    assert_eq!(eval_chars(&effects, &env, oid(3)), norm(&expected_b));
    // A is untouched.
    assert_eq!(eval_chars(&effects, &env, oid(1)), norm(&rich_a()));
}

/// REQ-OBJ-002, CR 707.3: a copy of a copy registered in the opposite timestamp order (C's
/// effect earlier than B's) still has B's copied values, because they were taken when C's
/// effect was built.
#[test]
fn copy_of_a_copy_is_independent_of_timestamp_order_between_objects() {
    let env = env_with(&[(1, rich_a()), (2, own_b()), (3, own_c())]);
    let vb = CopiableValues::plain(rich_a())
        .copy_with(&own_b(), &[CopyException::AddSubtypes(set(&["Mutant"]))]);
    let vc = vb.copy_with(&own_c(), &[]);
    let mut effects = ContinuousEffects::new();
    effects
        .register(eid(2), copy_eff(&vc, 5, &[oid(3)]))
        .unwrap();
    effects
        .register(eid(1), copy_eff(&vb, 50, &[oid(2)]))
        .unwrap();
    assert_eq!(
        eval_chars(&effects, &env, oid(3)),
        eval_chars(&effects, &env, oid(2))
    );
}

/// REQ-OBJ-002, CR 707.2b: changing the original after B's copy effect was built changes
/// neither the built effect nor B's evaluated characteristics.
#[test]
fn copy_707_2b_later_change_of_the_original_does_not_change_the_copy() {
    let mut env = env_with(&[(1, rich_a()), (2, own_b())]);
    let va = CopiableValues::plain(
        copiable_characteristics(&ContinuousEffects::new(), &env, oid(1)).unwrap(),
    );
    let vb = va.copy_with(&own_b(), &[]);
    let built = copy_eff(&vb, 10, &[oid(2)]);
    let mut effects = ContinuousEffects::new();
    effects.register(eid(1), built.clone()).unwrap();

    // The original changes: new name, new power.
    let mut changed = rich_a();
    changed.names = set(&["test.copy_source_a_changed"]);
    changed.power = Some(lit(9));
    env.put(oid(1), changed.clone());

    assert_eq!(effects.get(eid(1)), Some(&built));
    assert_eq!(eval_chars(&effects, &env, oid(2)), norm(&rich_a()));
    assert_eq!(eval_chars(&effects, &env, oid(1)), norm(&changed));
    assert_eq!(vb.characteristics, rich_a());
}

/// REQ-OBJ-002, CR 707.2: a copy takes only copiable values: counters and non-copy effects on
/// the original are not copied.
#[test]
fn copy_707_2_status_counters_and_other_effects_are_not_copied() {
    let mut env = env_with(&[(1, rich_a()), (2, own_b())]);
    env.counter(oid(1), "+1/+1", 2, 3);
    let mut effects = registry(&[
        (
            1,
            eff(
                Layer::L4,
                1,
                objs(&[oid(1)]),
                add_card_types(&["Enchantment"]),
            ),
        ),
        (2, eff(Layer::L7c, 2, objs(&[oid(1)]), modify_pt(1, 1))),
        (
            3,
            eff(
                Layer::L6,
                3,
                objs(&[oid(1)]),
                LayerModification::AddAbility(kw_ability("granted", "trample")),
            ),
        ),
    ]);
    let va = CopiableValues::plain(copiable_characteristics(&effects, &env, oid(1)).unwrap());
    let vb = va.copy_with(&own_b(), &[]);
    effects
        .register(eid(4), copy_eff(&vb, 10, &[oid(2)]))
        .unwrap();
    let got = eval_chars(&effects, &env, oid(2));
    assert_eq!(got, norm(&rich_a()));
    assert!(!got.card_types.contains("Enchantment"));
    assert!(!has_kw(&got, "trample"));
    // The original itself still shows its counters and effects.
    let a = evaluate(&effects, &env, oid(1)).unwrap();
    assert_eq!(pt(&a), spt(3 + 1 + 2, 4 + 1 + 2));
}

// ---------------------------------------------------------------------------------------------
// 2. copy of a face-down object
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 708.2, 613.2b, 613.2c: the copiable values of a face-down object are its
/// face-down values.
#[test]
fn face_down_708_2_copiable_characteristics_are_the_face_down_values() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let effects = registry(&[(1, face_down_eff(5, &[oid(1)]))]);
    assert_eq!(copiable(&effects, &env, 1), norm(&face_down_2_2()));
}

/// REQ-OBJ-002, CR 707.2 example, 708.2: a copy of a face-down object is a face-up colorless
/// 2/2 creature with no name, no subtypes, no abilities and no mana cost; none of the face-down
/// object's face-up values appear on the copy.
#[test]
fn face_down_707_2_example_copy_is_the_face_down_2_2_and_face_up() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let mut effects = registry(&[(1, face_down_eff(5, &[oid(1)]))]);
    let vx = CopiableValues::plain(copiable_characteristics(&effects, &env, oid(1)).unwrap());
    let vy = vx.copy_with(&own_b(), &[]);
    effects
        .register(eid(2), copy_eff(&vy, 6, &[oid(2)]))
        .unwrap();

    let y = eval_chars(&effects, &env, oid(2));
    assert_eq!(y, norm(&face_down_2_2()));
    assert!(y.names.is_empty());
    assert_eq!(y.mana_cost, None);
    assert_eq!(y.colors, ColorBitmask::NONE);
    assert_eq!(y.color_indicator, None);
    assert_eq!(y.card_types, set(&["Creature"]));
    assert!(y.supertypes.is_empty());
    assert!(y.subtypes.is_empty());
    assert!(y.abilities.is_empty());
    assert_eq!(pt(&evaluate(&effects, &env, oid(2)).unwrap()), spt(2, 2));
    // Y has no layer-1b effect: it is face up (status is not copied, CR 707.2).
    for (_, e) in effects.iter() {
        if e.layer == Layer::L1b {
            assert_eq!(e.affected, objs(&[oid(1)]));
        }
    }
}

/// REQ-OBJ-002, CR 707.2 example: nothing of the face-down object's face-up values (names,
/// cost, colors, color indicator, types, subtypes, text, abilities, P/T, loyalty, defense,
/// modifiers) reaches a copy made while it is face down.
#[test]
fn face_down_copy_shows_no_face_up_value_anywhere() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let mut effects = registry(&[(1, face_down_eff(5, &[oid(1)]))]);
    let vy = CopiableValues::plain(copiable_characteristics(&effects, &env, oid(1)).unwrap())
        .copy_with(&own_b(), &[]);
    effects
        .register(eid(2), copy_eff(&vy, 6, &[oid(2)]))
        .unwrap();
    let a = rich_a();
    for c in [
        vy.characteristics.clone(),
        eval_chars(&effects, &env, oid(2)),
    ] {
        assert!(c.names.is_disjoint(&a.names));
        assert_ne!(c.mana_cost, a.mana_cost);
        assert_ne!(c.colors, a.colors);
        assert_ne!(c.color_indicator, a.color_indicator);
        assert!(c.supertypes.is_disjoint(&a.supertypes));
        assert!(c.subtypes.is_disjoint(&a.subtypes));
        assert!(!c.card_types.contains("Artifact"));
        assert_ne!(c.rules_text, a.rules_text);
        for ab in &a.abilities {
            assert!(!c.abilities.contains(ab));
        }
        assert_ne!(literal_of(c.power.as_ref().unwrap()), Some(3));
        assert_ne!(literal_of(c.toughness.as_ref().unwrap()), Some(4));
        assert_eq!(c.loyalty, None);
        assert_eq!(c.defense, None);
        assert_eq!(c.hand_modifier, None);
        assert_eq!(c.life_modifier, None);
    }
}

/// REQ-OBJ-002, CR 708.10, 613.2b: a face-down permanent that becomes a copy keeps its
/// face-down characteristics while its layer-1b effect remains (1b applies after 1a); without
/// that effect it shows the copied values.
#[test]
fn face_down_708_10_copy_effect_on_a_face_down_permanent() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let vd = CopiableValues::plain(other_d()).copy_with(&rich_a(), &[]);
    for (fd_ts, copy_ts) in [(5, 6), (6, 5)] {
        let mut effects = registry(&[
            (1, face_down_eff(fd_ts, &[oid(1)])),
            (2, copy_eff(&vd, copy_ts, &[oid(1)])),
        ]);
        assert_eq!(eval_chars(&effects, &env, oid(1)), norm(&face_down_2_2()));
        assert_eq!(copiable(&effects, &env, 1), norm(&face_down_2_2()));
        assert!(effects.remove(eid(1)));
        assert_eq!(eval_chars(&effects, &env, oid(1)), norm(&other_d()));
        assert_eq!(copiable(&effects, &env, 1), norm(&other_d()));
    }
}

// ---------------------------------------------------------------------------------------------
// 3. copy of a prototyped permanent
// ---------------------------------------------------------------------------------------------

fn battlefield(cast: CastChoice) -> LayoutQuery {
    LayoutQuery::new(ZoneKind::Battlefield).with_cast(cast)
}

/// REQ-OBJ-002, CR 719.2a, 719.3b, 719.3d: a copy of a permanent that was a prototyped spell
/// has the prototype power, toughness, mana cost and colors, and the mana value of the
/// prototype cost; its characteristics are the layout's prototyped characteristics.
#[test]
fn prototype_719_3d_copy_of_a_prototyped_permanent() {
    let d = proto_def();
    let proto =
        base_characteristics(&d, &battlefield(CastChoice::Prototype), vocab()).expect("prototyped");
    let original = CopiableValues::plain(proto.clone());
    let copy = original.copy_with(&own_b(), &[]);
    assert_eq!(copy.characteristics, proto);
    assert_eq!(copy.source, ValueSource::Plain);
    let c = &copy.characteristics;
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(3));
    assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(2));
    assert_eq!(c.mana_cost, some_cost("{X}{2}{R}{R}"));
    assert_eq!(c.colors, ColorBitmask::RED);
    // The prototype face's own name and abilities are not part of it (CR 719.5).
    assert_eq!(c.names, set(&["test.proto_full_6_6"]));
    assert_eq!(c.card_types, set(&["Artifact", "Creature"]));
    // CR 202.3e: X is 0 off the stack: {X}{2}{R}{R} is 4.
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 4);
    assert_eq!(original.mana_value(ManaValueContext::OffStack), 4);

    // Through the layer system: the copier shows the prototyped values.
    let env = env_with(&[(1, proto.clone()), (2, own_b())]);
    let effects = registry(&[(1, copy_eff(&copy, 10, &[oid(2)]))]);
    let y = evaluate(&effects, &env, oid(2)).unwrap();
    assert_eq!(pt(&y), spt(3, 2));
    assert_eq!(y.characteristics.colors, ColorBitmask::RED);
    assert_eq!(y.characteristics.mana_cost, some_cost("{X}{2}{R}{R}"));
}

/// REQ-OBJ-002, CR 719.4, 719.2a: a copy of the same card on the battlefield not cast as a
/// prototype has the normal values.
#[test]
fn prototype_copy_of_a_not_prototyped_permanent_has_normal_values() {
    let d = proto_def();
    let normal = base_characteristics(&d, &battlefield(CastChoice::None), vocab()).expect("normal");
    let copy = CopiableValues::plain(normal.clone()).copy_with(&own_b(), &[]);
    assert_eq!(copy.characteristics, normal);
    let c = &copy.characteristics;
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(6));
    assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(6));
    assert_eq!(c.mana_cost, some_cost("{7}"));
    assert_eq!(c.colors, ColorBitmask::NONE);
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 7);
}

// ---------------------------------------------------------------------------------------------
// 5. "as enters" power and toughness
// ---------------------------------------------------------------------------------------------

fn as_enters(p: i64, t: i64) -> [CopyException; 2] {
    [
        CopyException::SetPower(lit(p)),
        CopyException::SetToughness(lit(t)),
    ]
}

/// REQ-OBJ-002, CR 613.2a, 707.2: an "as enters" P/T setting is a non-copy layer-1a change:
/// only power and toughness change, the source is kept.
#[test]
fn as_enters_613_2a_modified_changes_only_pt() {
    let a = CopiableValues::plain(rich_a());
    let m = a.modified(&as_enters(7, 8));
    let mut expected = rich_a();
    expected.power = Some(lit(7));
    expected.toughness = Some(lit(8));
    assert_eq!(m.characteristics, expected);
    assert_eq!(m.source, ValueSource::Plain);
    // The source is unchanged for a back face too (CR 202.3b MV stays the front's).
    let back = CopiableValues::nonmodal_back_face(own_c(), some_cost("{1}{G}"));
    let mb = back.modified(&as_enters(1, 2));
    assert_eq!(mb.source, back.source);
    assert_eq!(mb.mana_value(ManaValueContext::OffStack), 2);
    // `self` is unchanged.
    assert_eq!(a.characteristics, rich_a());
}

/// REQ-OBJ-002, CR 613.2a, 707.2: registered through `copiable_effect`, the "as enters" values
/// apply in layer 1a; a later copy sees the set power and toughness.
#[test]
fn as_enters_613_2a_values_are_copiable() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let m = CopiableValues::plain(rich_a()).modified(&as_enters(7, 8));
    let mut effects = registry(&[(1, copy_eff(&m, 3, &[oid(1)]))]);
    let x = evaluate(&effects, &env, oid(1)).unwrap();
    assert_eq!(pt(&x), spt(7, 8));
    let mut expected = rich_a();
    expected.power = Some(lit(7));
    expected.toughness = Some(lit(8));
    assert_eq!(norm(&x.characteristics), norm(&expected));
    assert_eq!(copiable(&effects, &env, 1), norm(&expected));

    let vy = CopiableValues::plain(copiable_characteristics(&effects, &env, oid(1)).unwrap())
        .copy_with(&own_b(), &[]);
    effects
        .register(eid(2), copy_eff(&vy, 9, &[oid(2)]))
        .unwrap();
    assert_eq!(pt(&evaluate(&effects, &env, oid(2)).unwrap()), spt(7, 8));
}

/// REQ-OBJ-002, CR 613.2, 613.7: two "as enters" effects on one object apply in timestamp
/// order: the later one wins, whichever effect id or registration order it has.
#[test]
fn as_enters_two_effects_later_timestamp_wins_both_ways() {
    let env = env_with(&[(1, rich_a())]);
    let first = CopiableValues::plain(rich_a()).modified(&as_enters(5, 6));
    let second = CopiableValues::plain(rich_a()).modified(&as_enters(2, 9));
    for (t1, t2, want) in [(10, 20, spt(2, 9)), (20, 10, spt(5, 6))] {
        for reversed in [false, true] {
            let pairs = [
                (1, copy_eff(&first, t1, &[oid(1)])),
                (2, copy_eff(&second, t2, &[oid(1)])),
            ];
            let mut effects = ContinuousEffects::new();
            let order: [usize; 2] = if reversed { [1, 0] } else { [0, 1] };
            for i in order {
                effects
                    .register(eid(pairs[i].0), pairs[i].1.clone())
                    .unwrap();
            }
            assert_eq!(
                pt(&evaluate(&effects, &env, oid(1)).unwrap()),
                want,
                "ts {t1}/{t2} reversed {reversed}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 8. copiable_characteristics
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 707.2, 613.2c: counters and effects of layers 2-7 do not appear in the
/// copiable characteristics.
#[test]
fn copiable_characteristics_613_2c_ignore_counters_and_layers_2_to_7() {
    let mut env = env_with(&[(1, rich_a()), (2, own_b())]);
    env.counter(oid(1), "+1/+1", 3, 1);
    env.counter(oid(1), "trample", 1, 2);
    let effects = registry(&[
        (
            1,
            eff(
                Layer::L2,
                1,
                objs(&[oid(1)]),
                LayerModification::SetController(P2),
            ),
        ),
        (
            2,
            eff(
                Layer::L4,
                2,
                objs(&[oid(1)]),
                add_card_types(&["Enchantment"]),
            ),
        ),
        (
            3,
            eff(
                Layer::L5,
                3,
                objs(&[oid(1)]),
                LayerModification::SetColors(ColorBitmask::WHITE),
            ),
        ),
        (
            4,
            eff(
                Layer::L6,
                4,
                objs(&[oid(1)]),
                LayerModification::AddAbility(kw_ability("granted", "vigilance")),
            ),
        ),
        (
            5,
            eff(Layer::L7b, 5, objs(&[oid(1)]), set_pt(Some(0), Some(1))),
        ),
        (6, eff(Layer::L7c, 6, objs(&[oid(1)]), modify_pt(2, 2))),
    ]);
    assert_eq!(copiable(&effects, &env, 1), norm(&rich_a()));
    // The full evaluation does show them.
    let full = eval_chars(&effects, &env, oid(1));
    assert_ne!(full, norm(&rich_a()));
}

/// REQ-OBJ-002, CR 613.2a, 613.2b, 613.2c: a layer-1a copy effect and a layer-1b face-down
/// effect do appear in the copiable characteristics.
#[test]
fn copiable_characteristics_include_layers_1a_and_1b() {
    let env = env_with(&[(1, rich_a()), (2, own_b())]);
    let v = CopiableValues::plain(other_d()).copy_with(&rich_a(), &[]);
    let mut effects = registry(&[
        (1, copy_eff(&v, 4, &[oid(1)])),
        (2, eff(Layer::L7c, 5, objs(&[oid(1)]), modify_pt(1, 1))),
    ]);
    assert_eq!(copiable(&effects, &env, 1), norm(&other_d()));
    effects
        .register(eid(3), face_down_eff(6, &[oid(1)]))
        .unwrap();
    assert_eq!(copiable(&effects, &env, 1), norm(&face_down_2_2()));
    // Other objects are not affected.
    assert_eq!(copiable(&effects, &env, 2), norm(&own_b()));
}

/// REQ-OBJ-002: an unknown object is an error.
#[test]
fn copiable_characteristics_unknown_object_is_an_error() {
    let env = env_with(&[(1, rich_a())]);
    assert_eq!(
        copiable_characteristics(&ContinuousEffects::new(), &env, oid(99)),
        Err(LayerError::UnknownObject(oid(99)))
    );
}

/// REQ-OBJ-002, REQ-CORE-001, CR 613.2, 613.7: two layer-1a effects apply in timestamp order
/// (the later wins); with equal timestamps, in ascending effect id (the higher id wins), in
/// any registration order.
#[test]
fn copiable_characteristics_two_1a_effects_timestamp_then_effect_id() {
    let env = env_with(&[(1, rich_a())]);
    let vp = CopiableValues::plain(other_d());
    let vq = CopiableValues::plain(own_c());
    // (ts of eid 1 = P, ts of eid 2 = Q, winner)
    let cases = [(10, 20, own_c()), (20, 10, other_d()), (15, 15, own_c())];
    for (tp, tq, want) in cases {
        for reversed in [false, true] {
            let mut effects = ContinuousEffects::new();
            let p = copy_eff(&vp, tp, &[oid(1)]);
            let q = copy_eff(&vq, tq, &[oid(1)]);
            if reversed {
                effects.register(eid(2), q).unwrap();
                effects.register(eid(1), p).unwrap();
            } else {
                effects.register(eid(1), p).unwrap();
                effects.register(eid(2), q).unwrap();
            }
            assert_eq!(copiable(&effects, &env, 1), norm(&want), "{tp}/{tq}");
            assert_eq!(eval_chars(&effects, &env, oid(1)), norm(&want), "{tp}/{tq}");
        }
    }
    // Equal timestamps, ids swapped: the other one wins.
    let mut effects = ContinuousEffects::new();
    effects
        .register(eid(2), copy_eff(&vp, 15, &[oid(1)]))
        .unwrap();
    effects
        .register(eid(1), copy_eff(&vq, 15, &[oid(1)]))
        .unwrap();
    assert_eq!(copiable(&effects, &env, 1), norm(&other_d()));
}

/// REQ-OBJ-002, CR 202.3e: a plain value's mana value with {X} is 0 off the stack and the
/// chosen number on it (also asserted for copies in spec_layers_copy_mana_value.rs).
#[test]
fn plain_values_mana_value_uses_own_cost() {
    let v = CopiableValues::plain(rich_a());
    assert_eq!(v.mana_value(ManaValueContext::OffStack), 4);
    assert_eq!(
        v.mana_value(ManaValueContext::OnStack(VariableValues {
            x: 3,
            y: 0,
            z: 0
        })),
        4
    );
}
