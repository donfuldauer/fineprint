// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for face-down objects with copy and merge effects (REQ-OBJ-003, REQ-OBJ-002,
//! REQ-LAYER-001, REQ-LAYER-002; CR 707.2, 707.3, 708.2, 708.10, 613.2a, 613.2b, 731.2a,
//! 731.2e, 731.2f). Includes the REQ-TEST-001 regression scenario "mutate over and under with a
//! face-down component", in both merge orders and both timestamp orders. Black-box: written
//! from the work-item brief only.

#[path = "spec_layers_facedown_support.rs"]
mod support;

use fineprint_base::ids::{EffectId, ObjectId, Timestamp};
use fineprint_core::layers::{
    CopiableValues, EffectOrigin, FaceDownRefusal, FaceDownTarget, FaceUpRefusal, copiable_values,
    merge_effect, turn_face_down, turn_face_up,
};
use fineprint_core::layouts::{
    FaceDownProfile, LayoutQuery, MergeSide, MergedComponent, MergedPermanent,
};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::CardDefinitionDSL;
use support::*;

const X: u64 = 1;
const B: u64 = 2;

fn profiles() -> [(FaceDownProfile, Characteristics); 2] {
    [
        (FaceDownProfile::default_2_2(), expected_default()),
        (FaceDownProfile::with_ward_2(), expected_ward()),
    ]
}

fn comp(def: &CardDefinitionDSL) -> MergedComponent<'_> {
    MergedComponent {
        def,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token: false,
        face_up: true,
    }
}

fn single(def: &CardDefinitionDSL) -> Characteristics {
    assert_eq!(def.faces.len(), 1, "single-face synthetic definition");
    chars_of(&def.faces[0])
}

fn down(
    w: &mut FdWorld,
    effect: EffectId,
    profile: &FaceDownProfile,
    object: ObjectId,
) -> Result<Timestamp, FaceDownRefusal> {
    turn_face_down(
        &mut w.effects,
        effect,
        profile,
        &FaceDownTarget::default(),
        None,
        P1,
        EffectOrigin::Resolved,
        object,
        &mut w.ledger,
        &mut w.ids,
    )
}

fn up(w: &mut FdWorld, effect: EffectId, object: ObjectId) -> Result<Timestamp, FaceUpRefusal> {
    turn_face_up(
        &mut w.effects,
        effect,
        object,
        false,
        &mut w.ledger,
        &mut w.ids,
    )
}

/// REQ-OBJ-003, REQ-OBJ-002, CR 707.2, 707.3, 708.2: a copy of a face-down object copies its
/// face-down values: the copier (with no face-down effect of its own) shows exactly the
/// profile, is not face down, and no face-down effect is created for it.
#[test]
fn req_obj_003_707_2_copy_of_face_down_object_has_the_profile() {
    for (p, want) in profiles() {
        let mut w = FdWorld::new();
        w.env.put(oid(X), secret_face_up());
        w.env.put(oid(B), own_b());
        ok_ts(down(&mut w, eid(1), &p, oid(X)), "down");
        let values = copiable_values(&w.effects, &w.env, oid(X)).unwrap();
        assert_eq!(norm(&values.characteristics), want);
        assert!(values.provenance().is_plain());
        let len = w.effects.len();
        let t = w.next_ts();
        w.reg(eid(2), copy_eff(&values, t.get(), &[oid(B)]));
        assert_eq!(w.effects.len(), len + 1);
        assert_eq!(w.chars(oid(B)), want);
        assert!(
            !has_face_down_effect_on(&w.effects, oid(B)),
            "not face down"
        );
        let ev = fineprint_core::layers::evaluate(&w.effects, &w.env, oid(B)).unwrap();
        assert_eq!(ev.mana_value(ManaValueContext::OffStack), 0);
        assert_no_secret(&format!("{:?}", ev.characteristics), "copier");

        // The original turning face up does not change the copy (a snapshot).
        ok_ts(up(&mut w, eid(1), oid(X)), "up");
        assert_eq!(w.chars(oid(B)), want);
        assert_eq!(w.chars(oid(X)), norm(&secret_face_up()));
    }
}

/// REQ-OBJ-003, CR 708.10, 613.2: a face-down permanent that becomes a copy keeps the profile
/// (the copy timestamp before or after the face-down one); turned face up it has the copied
/// values.
#[test]
fn req_obj_003_708_10_face_down_becomes_a_copy_both_orders() {
    let copied = CopiableValues::plain(other_d()).copy_with(&secret_face_up(), &[]);
    for (p, want) in profiles() {
        for copy_first in [true, false] {
            let mut w = FdWorld::new();
            w.env.put(oid(X), secret_face_up());
            if copy_first {
                let t = w.next_ts();
                w.reg(eid(2), copy_eff(&copied, t.get(), &[oid(X)]));
            }
            ok_ts(down(&mut w, eid(1), &p, oid(X)), "down");
            if !copy_first {
                let t = w.next_ts();
                w.reg(eid(2), copy_eff(&copied, t.get(), &[oid(X)]));
            }
            assert_eq!(w.chars(oid(X)), want, "copy first {copy_first}");
            assert_eq!(w.copiable(oid(X)), want, "copy first {copy_first}");
            ok_ts(up(&mut w, eid(1), oid(X)), "up");
            assert_eq!(w.chars(oid(X)), norm(&other_d()), "copy first {copy_first}");
        }
    }
}

/// The merged permanent of the regression scenario: `mutant` merged over or under the base
/// (components listed top first), mutated or merged.
fn merged<'a>(
    m: &'a CardDefinitionDSL,
    b: &'a CardDefinitionDSL,
    side: MergeSide,
    mutated: bool,
) -> MergedPermanent<'a> {
    let components = match side {
        MergeSide::Over => vec![comp(m), comp(b)],
        MergeSide::Under => vec![comp(b), comp(m)],
    };
    MergedPermanent::new(components, mutated).unwrap()
}

/// REQ-OBJ-003, REQ-OBJ-002, REQ-LAYER-002, CR 731.2a, 731.2f, 708.8, 613.2 (REQ-TEST-001:
/// mutate over and under with a face-down component): a merged permanent turned face down
/// shows the profile; after turning face up it shows the merged values (the topmost
/// component's characteristics, 731.2a); for both merge sides, merged or mutated, and both
/// timestamp orders of the merge and the face-down effect.
#[test]
fn req_obj_003_731_2a_merged_face_down_then_up_all_orders() {
    let (m, b) = (mutant_def(), merge_base_def());
    for side in [MergeSide::Over, MergeSide::Under] {
        for mutated in [false, true] {
            let mp = merged(&m, &b, side, mutated);
            let top = norm(&match side {
                MergeSide::Over => single(&m),
                MergeSide::Under => single(&b),
            });
            let merged_values = CopiableValues::merged(&mp, vocab()).unwrap();
            for (p, want) in profiles() {
                for merge_first in [true, false] {
                    let ctx = format!("{side:?} mutated {mutated} merge first {merge_first}");
                    let mut w = FdWorld::new();
                    w.env.put(oid(X), single(&b));
                    if merge_first {
                        let t = w.next_ts();
                        w.reg(eid(2), merge_effect(&mp, vocab(), oid(X), P1, t).unwrap());
                    }
                    let t_down = ok_ts(down(&mut w, eid(1), &p, oid(X)), "down");
                    if !merge_first {
                        let t = w.next_ts();
                        assert!(t > t_down);
                        w.reg(eid(2), merge_effect(&mp, vocab(), oid(X), P1, t).unwrap());
                    }
                    assert_eq!(w.chars(oid(X)), want, "{ctx}: face down");
                    assert_eq!(w.copiable(oid(X)), want, "{ctx}: copiable");

                    ok_ts(up(&mut w, eid(1), oid(X)), "up");
                    let got = w.chars(oid(X));
                    assert_eq!(got, norm(&merged_values.characteristics), "{ctx}: face up");
                    assert_eq!(got.names, top.names, "{ctx}: top's name");
                    assert_eq!(got.power, top.power, "{ctx}: top's power");
                    assert_eq!(got.subtypes, top.subtypes, "{ctx}: top's subtypes");
                }
            }
        }
    }
}

/// REQ-OBJ-003, CR 708.2, 731.2f, 702.140e: a face-down mutated permanent's lower components
/// contribute nothing: with a lower component carrying a distinctive ability, the face-down
/// merged object's abilities are exactly the profile's.
#[test]
fn req_obj_003_731_2f_face_down_lower_component_contributes_nothing() {
    let (m, b, u) = (mutant_def(), merge_base_def(), merge_under_def());
    let distinctive = single(&u).abilities;
    assert_eq!(distinctive.len(), 1);
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], true)
        .unwrap()
        .merge(comp(&u), MergeSide::Under);
    // Control: face up, the mutated permanent has the lower component's ability.
    let face_up_values = CopiableValues::merged(&mp, vocab()).unwrap();
    assert!(
        face_up_values
            .characteristics
            .abilities
            .contains(&distinctive[0])
    );
    for (p, want) in profiles() {
        for merge_first in [true, false] {
            let mut w = FdWorld::new();
            w.env.put(oid(X), single(&b));
            if merge_first {
                let t = w.next_ts();
                w.reg(eid(2), merge_effect(&mp, vocab(), oid(X), P1, t).unwrap());
            }
            ok_ts(down(&mut w, eid(1), &p, oid(X)), "down");
            if !merge_first {
                let t = w.next_ts();
                w.reg(eid(2), merge_effect(&mp, vocab(), oid(X), P1, t).unwrap());
            }
            let got = w.chars(oid(X));
            assert_eq!(got.abilities, want.abilities, "merge first {merge_first}");
            assert!(!got.abilities.contains(&distinctive[0]));
            assert_eq!(got, want);
        }
    }
}
