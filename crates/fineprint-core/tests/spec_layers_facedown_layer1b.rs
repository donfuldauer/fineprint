// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the layer-1b face-down effect (REQ-OBJ-003, REQ-OBJ-002, REQ-LAYER-001,
//! REQ-LAYER-002; CR 613.2b, 613.2c, 708.2, 708.2a, 708.3, 708.4, 613.1, 613.2): its shape, the
//! evaluated profile, mana value, ordering against layer-1a copy and merge effects in both
//! timestamp orders, later layers on top, copiable values, and the LKI look-back. Black-box:
//! written from the work-item brief only.

#[path = "spec_layers_facedown_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::layers::{
    Affected, ContinuousEffects, CopiableValues, EffectOrigin, LayerModification, LayeredProvider,
    copiable_characteristics, copiable_values, evaluate, face_down_effect, merge_effect,
};
use fineprint_core::layouts::{FaceDownProfile, LayoutQuery, MergedComponent, MergedPermanent};
use fineprint_core::memory::CharacteristicsProvider;
use fineprint_core::objects::{Characteristics, ManaValueContext, VariableValues};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::CardDefinitionDSL;
use support::*;

const X: u64 = 1;
const OTHER: u64 = 2;

fn profiles() -> [(FaceDownProfile, Characteristics); 2] {
    [
        (FaceDownProfile::default_2_2(), expected_default()),
        (FaceDownProfile::with_ward_2(), expected_ward()),
    ]
}

fn env() -> TestEnv {
    let mut e = TestEnv::new();
    e.put(oid(X), secret_face_up());
    e.put(oid(OTHER), own_b());
    e
}

fn comp(def: &CardDefinitionDSL) -> MergedComponent<'_> {
    MergedComponent {
        def,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token: false,
        face_up: true,
    }
}

fn add_subtype(s: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::new(),
        subtypes: set(&[s]),
    }
}

/// REQ-OBJ-003, CR 613.2b, 708.2: `face_down_effect` is a layer-1b, non-CDA effect on exactly
/// the one object, whose modification is `FaceDown` of the profile's characteristics; the other
/// arguments are carried as given.
#[test]
fn req_obj_003_613_2b_face_down_effect_shape() {
    for (p, _) in profiles() {
        let e = face_down_effect(&p, Some(oid(9)), P2, EffectOrigin::Resolved, ts(41), oid(X));
        assert_eq!(e.layer, Layer::L1b);
        assert!(!e.is_cda);
        assert_eq!(e.affected, Affected::Objects(oset(&[oid(X)])));
        assert_eq!(
            e.modification,
            LayerModification::FaceDown(Box::new(p.characteristics().clone()))
        );
        assert_eq!(e.timestamp, ts(41));
        assert_eq!(e.source, Some(oid(9)));
        assert_eq!(e.controller, P2);
        assert_eq!(e.origin, EffectOrigin::Resolved);
        assert!(e.modification.belongs_to(Layer::L1b));

        let static_origin = EffectOrigin::Static { granted_at: None };
        let e2 = face_down_effect(&p, None, P1, static_origin.clone(), ts(3), oid(OTHER));
        assert_eq!(e2.source, None);
        assert_eq!(e2.origin, static_origin);
        assert_eq!(e2.affected, Affected::Objects(oset(&[oid(OTHER)])));

        // Registers without error.
        let mut r = ContinuousEffects::new();
        r.register(eid(1), e.clone()).expect("registers");
        assert_eq!(r.get(eid(1)), Some(&e));
    }
}

/// REQ-OBJ-003, REQ-LAYER-001, CR 708.2, 708.2a, 708.3: a permanent that is face down (here as
/// if it entered face down) has exactly the profile's characteristics, every field: its name,
/// cost, colors, subtypes, text, ETB-like ability, other abilities and 5/5 are gone: the
/// object's evaluated characteristics are exactly the profile's, so the face-up ETB-like
/// ability is absent from them. Other objects are untouched. CR 708.3: "Objects that are put
/// onto the battlefield face down are turned face down before they enter the battlefield, so
/// the permanent's enters-the-battlefield abilities won't trigger (if triggered) or have any
/// effect (if static)." Trigger suppression (that no enters-the-battlefield trigger is
/// dispatched) is not covered here.
#[test]
fn req_obj_003_708_3_face_down_permanent_is_exactly_the_profile() {
    for (p, want) in profiles() {
        let effects = registry(&[(1, fd_eff(&p, 10, oid(X)))]);
        let got = eval_chars(&effects, &env(), oid(X));
        assert_eq!(got.names, want.names);
        assert_eq!(got.mana_cost, want.mana_cost);
        assert_eq!(got.color_indicator, want.color_indicator);
        assert_eq!(got.colors, want.colors);
        assert_eq!(got.supertypes, want.supertypes);
        assert_eq!(got.card_types, want.card_types);
        assert_eq!(got.subtypes, want.subtypes);
        assert_eq!(got.rules_text, want.rules_text);
        assert_eq!(got.abilities, want.abilities);
        assert_eq!(got.power, want.power);
        assert_eq!(got.toughness, want.toughness);
        assert_eq!(got.loyalty, want.loyalty);
        assert_eq!(got.defense, want.defense);
        assert_eq!(got.hand_modifier, want.hand_modifier);
        assert_eq!(got.life_modifier, want.life_modifier);
        assert_eq!(got, want);
        assert!(!got.abilities.iter().any(|a| {
            ability_fields_contains(a, SECRET_ETB) || ability_fields_contains(a, SECRET_STATIC)
        }));
        let e = evaluate(&effects, &env(), oid(X)).unwrap();
        assert_eq!(pt(&e), spt(2, 2));
        // The other object is untouched.
        assert_eq!(
            eval_chars(&effects, &env(), oid(OTHER)),
            eval_chars(&ContinuousEffects::new(), &env(), oid(OTHER))
        );
    }
}

fn ability_fields_contains(a: &AbilityAST, marker: &str) -> bool {
    format!("{a:?}").contains(marker)
}

/// REQ-OBJ-003, CR 708.4, 708.2a: a face-down spell (not on the battlefield) has only the
/// face-down characteristics, so anything that looks at the spell sees the profile.
#[test]
fn req_obj_003_708_4_face_down_spell_has_the_profile() {
    for (p, want) in profiles() {
        let mut e = TestEnv::new();
        e.put_full(oid(X), secret_face_up(), Some(P1), false);
        let effects = registry(&[(1, fd_eff(&p, 5, oid(X)))]);
        assert_eq!(eval_chars(&effects, &e, oid(X)), want);
        let ev = evaluate(&effects, &e, oid(X)).unwrap();
        // CR 708.2a, 202.3: no mana cost, so mana value 0 on the stack whatever X is.
        assert_eq!(
            ev.mana_value(ManaValueContext::OnStack(VariableValues {
                x: 7,
                y: 1,
                z: 2
            })),
            0
        );
        assert_eq!(ev.mana_value(ManaValueContext::OffStack), 0);
    }
}

/// REQ-OBJ-003, CR 708.2a, 202.3a: the mana value of a face-down object is 0 and its
/// provenance is plain, even over a layer-1a copy whose values carry a fixed mana value.
#[test]
fn req_obj_003_708_2a_face_down_mana_value_zero_plain_provenance() {
    let fixed = CopiableValues::nonmodal_back_face(other_d(), some_cost("{3}{G}{G}"));
    for (p, _) in profiles() {
        // The copy alone is not plain (control).
        let only_copy = registry(&[(1, copy_eff(&fixed, 10, &[oid(X)]))]);
        let ev = evaluate(&only_copy, &env(), oid(X)).unwrap();
        assert!(!ev.mana_value_provenance.is_plain(), "control: fixed");
        assert_ne!(ev.mana_value(ManaValueContext::OffStack), 0, "control");

        for (copy_t, fd_t) in [(10, 20), (20, 10)] {
            let effects = registry(&[
                (1, copy_eff(&fixed, copy_t, &[oid(X)])),
                (2, fd_eff(&p, fd_t, oid(X))),
            ]);
            let ev = evaluate(&effects, &env(), oid(X)).unwrap();
            assert!(
                ev.mana_value_provenance.is_plain(),
                "copy {copy_t} fd {fd_t}"
            );
            assert_eq!(ev.mana_value(ManaValueContext::OffStack), 0);
            assert_eq!(ev.characteristics.mana_cost, None);
        }
        // Plain base: also 0.
        let effects = registry(&[(1, fd_eff(&p, 1, oid(X)))]);
        let ev = evaluate(&effects, &env(), oid(X)).unwrap();
        assert!(ev.mana_value_provenance.is_plain());
        assert_eq!(ev.mana_value(ManaValueContext::OffStack), 0);
    }
}

/// REQ-OBJ-003, CR 702.168a, 701.58a: the ward variant evaluates with exactly one keyword,
/// ward {2}, and the default variant with none.
#[test]
fn req_obj_003_702_168a_ward_variant_evaluates_with_ward_2() {
    let effects = registry(&[(1, fd_eff(&FaceDownProfile::with_ward_2(), 3, oid(X)))]);
    let got = eval_chars(&effects, &env(), oid(X));
    assert_eq!(
        keywords(&got),
        vec![("facedown:ward".to_string(), "ward".to_string())]
    );
    assert_eq!(count_kw(&got, "ward"), 1);
    assert_eq!(got.abilities.len(), 1);
    assert!(is_ward_2(&got.abilities[0]));

    let effects = registry(&[(1, fd_eff(&FaceDownProfile::default_2_2(), 3, oid(X)))]);
    let got = eval_chars(&effects, &env(), oid(X));
    assert!(keywords(&got).is_empty());
    assert!(got.abilities.is_empty());
}

/// REQ-OBJ-003, CR 708.2: a custom profile (a card effect's own list) evaluates to exactly that
/// list.
#[test]
fn req_obj_003_708_2_custom_profile_evaluates_as_listed() {
    let listed = Characteristics {
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Golem"]),
        ..creature("test.fd_custom_listed", 2, 2)
    };
    let effects = registry(&[(
        1,
        fd_eff(&FaceDownProfile::custom(listed.clone()), 3, oid(X)),
    )]);
    assert_eq!(eval_chars(&effects, &env(), oid(X)), norm(&listed));
}

/// REQ-OBJ-003, REQ-LAYER-002, CR 613.1, 613.2, 613.2b, 708.10: a layer-1a copy effect and the
/// layer-1b face-down effect on one object give the profile in both timestamp orders (and both
/// registration orders): layer 1b always applies after 1a.
#[test]
fn req_obj_003_613_2_copy_and_face_down_both_timestamp_orders() {
    let copied = CopiableValues::plain(other_d()).copy_with(&secret_face_up(), &[]);
    for (p, want) in profiles() {
        for (copy_t, fd_t) in [(10, 20), (20, 10)] {
            for (copy_id, fd_id) in [(1, 2), (2, 1)] {
                let effects = registry(&[
                    (copy_id, copy_eff(&copied, copy_t, &[oid(X)])),
                    (fd_id, fd_eff(&p, fd_t, oid(X))),
                ]);
                assert_eq!(
                    eval_chars(&effects, &env(), oid(X)),
                    want,
                    "copy {copy_t} fd {fd_t} ids {copy_id}/{fd_id}"
                );
            }
        }
    }
}

/// REQ-OBJ-003, REQ-LAYER-002, CR 613.2a, 613.2b, 731.2a: a merge effect (layer 1a) and the
/// face-down effect give the profile in both timestamp orders, mutated or not.
#[test]
fn req_obj_003_731_2a_merge_and_face_down_both_timestamp_orders() {
    let (m, b) = (mutant_def(), merge_base_def());
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], mutated).unwrap();
        for (p, want) in profiles() {
            for (merge_t, fd_t) in [(10, 20), (20, 10)] {
                let effects = registry(&[
                    (
                        1,
                        merge_effect(&mp, vocab(), oid(X), P1, ts(merge_t)).unwrap(),
                    ),
                    (2, fd_eff(&p, fd_t, oid(X))),
                ]);
                assert_eq!(
                    eval_chars(&effects, &env(), oid(X)),
                    want,
                    "mutated {mutated} merge {merge_t} fd {fd_t}"
                );
            }
        }
    }
}

/// REQ-OBJ-003, REQ-LAYER-001, CR 708.2, 613.2c, 613.1d, 613.1f, 613.4c: layer 2-7 effects
/// still apply on top of the profile (a layer-4 subtype, a layer-6 ability, a layer-7c +1/+1):
/// the object is a 3/3 with those additions and still has no name; whatever the timestamps.
#[test]
fn req_obj_003_613_2c_later_layers_apply_on_top_of_the_profile() {
    for (p, want) in profiles() {
        for fd_t in [1, 50] {
            let effects = registry(&[
                (
                    1,
                    eff(Layer::L4, 10, objs(&[oid(X)]), add_subtype("Spirit")),
                ),
                (
                    2,
                    eff(
                        Layer::L6,
                        11,
                        objs(&[oid(X)]),
                        LayerModification::AddAbility(kw_ability("test.fd_granted", "flying")),
                    ),
                ),
                (3, eff(Layer::L7c, 12, objs(&[oid(X)]), modify_pt(1, 1))),
                (4, fd_eff(&p, fd_t, oid(X))),
            ]);
            let ev = evaluate(&effects, &env(), oid(X)).unwrap();
            let got = norm(&ev.characteristics);
            assert_eq!(pt(&ev), spt(3, 3), "fd {fd_t}");
            assert!(got.names.is_empty(), "no name");
            assert_eq!(got.mana_cost, None);
            assert_eq!(got.subtypes, set(&["Spirit"]));
            assert_eq!(got.card_types, set(&["Creature"]));
            let mut want_abilities = want.abilities.clone();
            want_abilities.push(kw_ability("test.fd_granted", "flying"));
            assert_eq!(got.abilities, want_abilities);
            assert!(got.rules_text.0.is_empty());

            // CR 613.2c, 707.2: copiable values are the profile, without layers 2-7.
            let cc = norm(&copiable_characteristics(&effects, &env(), oid(X)).unwrap());
            assert_eq!(cc, want, "copiable characteristics");
            let cv = copiable_values(&effects, &env(), oid(X)).unwrap();
            assert_eq!(norm(&cv.characteristics), want, "copiable values");
            assert!(cv.provenance().is_plain());
            assert_eq!(cv.mana_value(ManaValueContext::OffStack), 0);
        }
    }
}

/// REQ-OBJ-003, CR 613.2c, 707.2: the copiable values of a face-down object over a fixed-value
/// copy are the profile with plain provenance.
#[test]
fn req_obj_003_613_2c_copiable_values_are_the_profile_plain() {
    let fixed = CopiableValues::nonmodal_back_face(other_d(), some_cost("{3}{G}{G}"));
    for (p, want) in profiles() {
        for (copy_t, fd_t) in [(10, 20), (20, 10)] {
            let effects = registry(&[
                (1, copy_eff(&fixed, copy_t, &[oid(X)])),
                (2, fd_eff(&p, fd_t, oid(X))),
            ]);
            let cv = copiable_values(&effects, &env(), oid(X)).unwrap();
            assert_eq!(norm(&cv.characteristics), want);
            assert!(cv.provenance().is_plain(), "copy {copy_t} fd {fd_t}");
            assert_eq!(cv.mana_value(ManaValueContext::OffStack), 0);
        }
    }
}

/// REQ-OBJ-003, REQ-CORE-003, CR 603.10 (look-back inference), 708.2: a face-down permanent
/// evaluated just before it leaves (the effect still registered) has the profile, through the
/// LKI provider and through `evaluate`.
#[test]
fn req_obj_003_lki_look_back_sees_the_profile() {
    for (p, want) in profiles() {
        let effects = registry(&[(1, fd_eff(&p, 4, oid(X)))]);
        let env = env();
        let provider = LayeredProvider::new(&effects, &env);
        let lki = provider.characteristics(oid(X)).expect("known");
        assert_eq!(norm(&lki), want);
        assert_no_secret(&format!("{lki:?}"), "lki");
        assert_eq!(
            norm(&evaluate(&effects, &env, oid(X)).unwrap().characteristics),
            want
        );
    }
}
