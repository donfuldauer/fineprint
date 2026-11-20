// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for turning a permanent face down and face up (REQ-OBJ-003, REQ-OBJ-002,
//! REQ-LAYER-001; CR 613.7f, 708.2a, 708.2b, 712.12, 712.13, 731.2j, 708.8, 708.10, 701.40g,
//! 701.58g, 731.2g): the new timestamps, the registered effect, every refusal and its
//! precedence, and that a refusal changes nothing. Black-box: written from the work-item brief
//! only.

#[path = "spec_layers_facedown_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, Timestamp};
use fineprint_core::layers::{
    CopiableValues, EffectOrigin, FaceDownRefusal, FaceDownTarget, FaceUpRefusal, LayerError,
    LayerModification, StampReason, evaluate, face_down_effect, turn_face_down, turn_face_up,
};
use fineprint_core::layouts::FaceDownProfile;
use fineprint_core::objects::Characteristics;
use support::*;

const X: u64 = 1;
const Y: u64 = 2;

fn world() -> FdWorld {
    let mut w = FdWorld::new();
    w.env.put(oid(X), secret_face_up());
    w.env.put(oid(Y), own_b());
    w
}

fn profiles() -> [(FaceDownProfile, Characteristics); 2] {
    [
        (FaceDownProfile::default_2_2(), expected_default()),
        (FaceDownProfile::with_ward_2(), expected_ward()),
    ]
}

fn down(
    w: &mut FdWorld,
    effect: EffectId,
    profile: &FaceDownProfile,
    target: &FaceDownTarget,
    object: ObjectId,
) -> Result<Timestamp, FaceDownRefusal> {
    turn_face_down(
        &mut w.effects,
        effect,
        profile,
        target,
        None,
        P1,
        EffectOrigin::Resolved,
        object,
        &mut w.ledger,
        &mut w.ids,
    )
}

fn up(
    w: &mut FdWorld,
    effect: EffectId,
    object: ObjectId,
    instant_or_sorcery: bool,
) -> Result<Timestamp, FaceUpRefusal> {
    turn_face_up(
        &mut w.effects,
        effect,
        object,
        instant_or_sorcery,
        &mut w.ledger,
        &mut w.ids,
    )
}

fn plain() -> FaceDownTarget {
    FaceDownTarget::default()
}

fn target(already: bool, dfc: bool, merged_dfc: bool) -> FaceDownTarget {
    FaceDownTarget {
        already_face_down: already,
        double_faced_permanent: dfc,
        merged_contains_double_faced: merged_dfc,
    }
}

fn add_type(t: &str) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(&[t]),
        subtypes: BTreeSet::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// turning face down
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-003, CR 613.7f, 708.2a: turning face down returns a timestamp strictly later than
/// every earlier one from `ids`, stamps the object with it, registers exactly the
/// `face_down_effect` with that timestamp under the given id, and the object then evaluates as
/// the profile.
#[test]
fn req_obj_003_613_7f_turn_face_down_success() {
    for (p, want) in profiles() {
        let mut w = world();
        let mut earlier = Vec::new();
        for _ in 0..3 {
            earlier.push(w.next_ts());
        }
        // An object timestamp from the ledger, too.
        let entered = w
            .ledger
            .stamp_object(&mut w.ids, oid(X), StampReason::EntersZone)
            .unwrap();
        earlier.push(entered);
        let before_len = w.effects.len();

        let y_before = w.chars(oid(Y));
        let t = ok_ts(down(&mut w, eid(7), &p, &plain(), oid(X)), "turn face down");
        assert!(earlier.iter().all(|e| t > *e), "strictly later");
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t));
        assert_eq!(
            w.ids.last_issued(),
            t.get(),
            "exactly one timestamp consumed"
        );
        assert_eq!(w.effects.len(), before_len + 1);
        assert_eq!(
            w.effects.get(eid(7)),
            Some(&face_down_effect(
                &p,
                None,
                P1,
                EffectOrigin::Resolved,
                t,
                oid(X)
            ))
        );
        assert_eq!(w.chars(oid(X)), want);
        assert_eq!(w.chars(oid(Y)), y_before, "other object untouched");
        assert_eq!(w.ledger.object_timestamp(oid(Y)), None);
    }
}

/// REQ-OBJ-003, CR 613.7f: the source, controller and origin are carried into the registered
/// effect.
#[test]
fn req_obj_003_turn_face_down_carries_source_controller_origin() {
    let mut w = world();
    let p = FaceDownProfile::default_2_2();
    let origin = EffectOrigin::Static { granted_at: None };
    let t = ok_ts(
        turn_face_down(
            &mut w.effects,
            eid(3),
            &p,
            &plain(),
            Some(oid(Y)),
            P2,
            origin.clone(),
            oid(X),
            &mut w.ledger,
            &mut w.ids,
        ),
        "turn face down",
    );
    assert_eq!(
        w.effects.get(eid(3)),
        Some(&face_down_effect(&p, Some(oid(Y)), P2, origin, t, oid(X)))
    );
}

fn refusal_name(r: &Result<Timestamp, FaceDownRefusal>) -> &'static str {
    match r {
        Ok(_) => "Ok",
        Err(FaceDownRefusal::AlreadyFaceDown) => "AlreadyFaceDown",
        Err(FaceDownRefusal::DoubleFacedPermanent) => "DoubleFacedPermanent",
        Err(FaceDownRefusal::MergedContainsDoubleFaced) => "MergedContainsDoubleFaced",
        Err(FaceDownRefusal::Layer(_)) => "Layer",
    }
}

/// The refusal the brief's check order gives for these flags.
fn expected_refusal(t: &FaceDownTarget) -> &'static str {
    if t.already_face_down {
        "AlreadyFaceDown"
    } else if t.double_faced_permanent {
        "DoubleFacedPermanent"
    } else if t.merged_contains_double_faced {
        "MergedContainsDoubleFaced"
    } else {
        "Ok"
    }
}

/// A world with some state in it: an earlier effect, stamped objects and issued timestamps.
fn busy_world() -> FdWorld {
    let mut w = world();
    let t1 = w.next_ts();
    w.reg(
        eid(1),
        eff(Layer::L7c, t1.get(), objs(&[oid(X)]), modify_pt(1, 1)),
    );
    w.ledger
        .stamp_object(&mut w.ids, oid(X), StampReason::EntersZone)
        .unwrap();
    w.ledger
        .stamp_object(&mut w.ids, oid(Y), StampReason::EntersZone)
        .unwrap();
    w
}

/// REQ-OBJ-003, CR 708.2b, 712.13, 731.2j: each refusal, alone and in every combination, is
/// reported in the order AlreadyFaceDown, DoubleFacedPermanent, MergedContainsDoubleFaced and
/// leaves `effects`, `ledger` and `ids` equal to clones taken before (nothing registered, no
/// timestamp consumed), so the object's characteristics do not change.
#[test]
fn req_obj_003_708_2b_712_13_731_2j_refusals_change_nothing_in_order() {
    for bits in 1u8..8 {
        let t = target(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
        for (p, _) in profiles() {
            let mut w = busy_world();
            let chars_before = w.chars(oid(X));
            let snap = w.snapshot();
            let r = down(&mut w, eid(9), &p, &t, oid(X));
            assert_eq!(refusal_name(&r), expected_refusal(&t), "{t:?}");
            w.assert_unchanged(&snap, &format!("{t:?}"));
            assert!(w.effects.get(eid(9)).is_none());
            assert_eq!(w.chars(oid(X)), chars_before);
        }
    }
}

/// REQ-OBJ-003, CR 708.2b: an object that is already face down (registered with the effect)
/// can't be turned face down again: a second attempt with another profile changes nothing,
/// including its characteristics and copiable values.
#[test]
fn req_obj_003_708_2b_second_turn_face_down_changes_nothing() {
    let mut w = world();
    let t = ok_ts(
        down(
            &mut w,
            eid(1),
            &FaceDownProfile::default_2_2(),
            &plain(),
            oid(X),
        ),
        "first",
    );
    let snap = w.snapshot();
    let chars = w.chars(oid(X));
    let cc = w.copiable(oid(X));
    let r = down(
        &mut w,
        eid(2),
        &FaceDownProfile::with_ward_2(),
        &target(true, false, false),
        oid(X),
    );
    assert!(matches!(r, Err(FaceDownRefusal::AlreadyFaceDown)));
    w.assert_unchanged(&snap, "second");
    assert_eq!(w.chars(oid(X)), chars);
    assert_eq!(w.copiable(oid(X)), cc);
    assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t));
}

/// REQ-OBJ-003, CR 613.7f: a registration failure (an effect id already in use) is `Layer(..)`
/// (the registry's `DuplicateEffect`) and leaves everything unchanged; a flag refusal is
/// reported before it.
#[test]
fn req_obj_003_turn_face_down_registration_failure_is_layer_and_unchanged() {
    let mut w = busy_world();
    let snap = w.snapshot();
    let r = down(
        &mut w,
        eid(1),
        &FaceDownProfile::default_2_2(),
        &plain(),
        oid(X),
    );
    assert!(
        matches!(r, Err(FaceDownRefusal::Layer(LayerError::DuplicateEffect(e))) if e == eid(1)),
        "got {}",
        refusal_name(&r)
    );
    w.assert_unchanged(&snap, "duplicate id");

    for t in [
        target(true, false, false),
        target(false, true, false),
        target(false, false, true),
    ] {
        let r = down(&mut w, eid(1), &FaceDownProfile::default_2_2(), &t, oid(X));
        assert_eq!(refusal_name(&r), expected_refusal(&t), "{t:?} before Layer");
        w.assert_unchanged(&snap, "flag before layer");
    }
}

/// REQ-OBJ-003, REQ-CORE-002, CR 613.7f: an exhausted timestamp generator is `Layer(..)` and
/// leaves everything unchanged.
#[test]
fn req_obj_003_turn_face_down_exhausted_ids_is_layer_and_unchanged() {
    let mut w = world();
    w.ids = IdGenerator::from_last_issued(u64::MAX);
    let snap = w.snapshot();
    let r = down(
        &mut w,
        eid(1),
        &FaceDownProfile::default_2_2(),
        &plain(),
        oid(X),
    );
    assert!(
        matches!(r, Err(FaceDownRefusal::Layer(_))),
        "got {}",
        refusal_name(&r)
    );
    w.assert_unchanged(&snap, "exhausted");
}

/// REQ-OBJ-003, CR 712.12, 708.3: a double-faced card may enter the battlefield (or be cast)
/// face down: building the effect directly is not refused and the object has the profile.
#[test]
fn req_obj_003_712_12_dfc_entering_face_down_is_not_refused() {
    for (p, want) in profiles() {
        let mut w = FdWorld::new();
        let front = Characteristics {
            subtypes: set(&["Werewolf"]),
            ..creature("test.fd_dfc_front", 3, 3)
        };
        w.env.put(oid(X), front);
        let t = w.next_ts();
        w.reg(eid(1), fd_eff_at(&p, t, oid(X)));
        assert_eq!(w.chars(oid(X)), want);
        // And it turns face up front face up (712.12a): the base again.
        let t2 = ok_ts(up(&mut w, eid(1), oid(X), false), "turn up");
        assert!(t2 > t);
        assert_eq!(w.chars(oid(X)).names, set(&["test.fd_dfc_front"]));
    }
}

// ---------------------------------------------------------------------------------------------
// turning face up
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-003, CR 708.8, 613.7f (acceptance criterion: turning face up restores copiable
/// values with a new timestamp): the characteristics and copiable values revert to the base,
/// the face-down effect is gone, and the object has a new, strictly later timestamp.
#[test]
fn req_obj_003_708_8_turn_face_up_reverts_with_new_timestamp() {
    for (p, want) in profiles() {
        let mut w = world();
        let base_chars = w.chars(oid(X));
        let base_copiable = w.copiable(oid(X));
        let t_down = ok_ts(down(&mut w, eid(5), &p, &plain(), oid(X)), "down");
        assert_eq!(w.chars(oid(X)), want);
        assert_eq!(w.copiable(oid(X)), want);
        let len = w.effects.len();

        let t_up = ok_ts(up(&mut w, eid(5), oid(X), false), "up");
        assert!(t_up > t_down);
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t_up));
        assert_eq!(
            w.ids.last_issued(),
            t_up.get(),
            "exactly one timestamp consumed"
        );
        assert!(w.effects.get(eid(5)).is_none(), "face-down effect removed");
        assert_eq!(w.effects.len(), len - 1);
        assert!(!has_face_down_effect_on(&w.effects, oid(X)));
        assert_eq!(w.chars(oid(X)), base_chars);
        assert_eq!(w.copiable(oid(X)), base_copiable);
        assert_eq!(w.chars(oid(X)), norm(&secret_face_up()));
    }
}

/// REQ-OBJ-003, CR 613.7f, 708.10: a copy effect registered while face down (timestamp after
/// the face-down effect) does not show; after turning face up the object has a new timestamp
/// strictly later than the copy effect's, the ledger records it, and the copied values show.
#[test]
fn req_obj_003_708_10_face_up_timestamp_after_copy_registered_in_between() {
    let copied = CopiableValues::plain(other_d()).copy_with(&secret_face_up(), &[]);
    for (p, want) in profiles() {
        let mut w = world();
        let t_down = ok_ts(down(&mut w, eid(1), &p, &plain(), oid(X)), "down");
        let t_copy = w.next_ts();
        w.reg(eid(2), copy_eff(&copied, t_copy.get(), &[oid(X)]));
        assert!(t_copy > t_down);
        assert_eq!(w.chars(oid(X)), want, "still the profile while face down");

        let t_up = ok_ts(up(&mut w, eid(1), oid(X), false), "up");
        assert!(t_up > t_copy && t_up > t_down);
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t_up));
        assert_eq!(w.chars(oid(X)), norm(&other_d()), "the copied values");
        assert!(w.effects.get(eid(2)).is_some(), "the copy stays");
    }
}

/// REQ-OBJ-003, CR 708.10: copy, then face down, then face up shows the copied values, not the
/// base; while face down it shows the profile.
#[test]
fn req_obj_003_708_10_copy_then_down_then_up_shows_the_copy() {
    let copied = CopiableValues::plain(other_d()).copy_with(&secret_face_up(), &[]);
    for (p, want) in profiles() {
        let mut w = world();
        let t_copy = w.next_ts();
        w.reg(eid(1), copy_eff(&copied, t_copy.get(), &[oid(X)]));
        assert_eq!(w.chars(oid(X)), norm(&other_d()));
        ok_ts(down(&mut w, eid(2), &p, &plain(), oid(X)), "down");
        assert_eq!(w.chars(oid(X)), want);
        assert_eq!(w.copiable(oid(X)), want);
        ok_ts(up(&mut w, eid(2), oid(X), false), "up");
        assert_eq!(w.chars(oid(X)), norm(&other_d()));
        assert_eq!(w.copiable(oid(X)), norm(&other_d()));
        assert_ne!(w.chars(oid(X)), norm(&secret_face_up()));
    }
}

/// REQ-OBJ-003, CR 613.7f: both directions stamp: down, up, down, up give four strictly
/// increasing timestamps, each recorded in the ledger.
#[test]
fn req_obj_003_613_7f_both_directions_stamp_strictly_increasing() {
    let mut w = world();
    let p = FaceDownProfile::default_2_2();
    let mut stamps = Vec::new();
    for round in 0..2u64 {
        let id = eid(10 + round);
        let t = ok_ts(down(&mut w, id, &p, &plain(), oid(X)), "down");
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t));
        assert_eq!(w.chars(oid(X)), expected_default());
        stamps.push(t);
        let t = ok_ts(up(&mut w, id, oid(X), false), "up");
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t));
        assert_eq!(w.chars(oid(X)), norm(&secret_face_up()));
        stamps.push(t);
    }
    assert_eq!(stamps.len(), 4);
    assert!(stamps.windows(2).all(|p| p[0] < p[1]), "{stamps:?}");
}

/// REQ-OBJ-003, CR 708.8: effects applied to the face-down permanent (a layer-7c +1/+1 and a
/// layer-4 type addition) still apply after it turns face up.
#[test]
fn req_obj_003_708_8_other_effects_still_apply_after_face_up() {
    for (p, want) in profiles() {
        let mut w = world();
        ok_ts(down(&mut w, eid(1), &p, &plain(), oid(X)), "down");
        let t1 = w.next_ts();
        w.reg(
            eid(2),
            eff(Layer::L7c, t1.get(), objs(&[oid(X)]), modify_pt(1, 1)),
        );
        let t2 = w.next_ts();
        w.reg(
            eid(3),
            eff(
                Layer::L4,
                t2.get(),
                objs(&[oid(X)]),
                add_type("Enchantment"),
            ),
        );
        let ev = evaluate(&w.effects, &w.env, oid(X)).unwrap();
        assert_eq!(pt(&ev), spt(3, 3));
        assert!(ev.characteristics.card_types.contains("Enchantment"));
        assert_eq!(ev.characteristics.abilities, want.abilities);

        ok_ts(up(&mut w, eid(1), oid(X), false), "up");
        assert!(w.effects.get(eid(2)).is_some() && w.effects.get(eid(3)).is_some());
        let ev = evaluate(&w.effects, &w.env, oid(X)).unwrap();
        assert_eq!(pt(&ev), spt(6, 6));
        assert!(ev.characteristics.card_types.contains("Enchantment"));
        assert!(ev.characteristics.card_types.contains("Creature"));
        assert_eq!(ev.characteristics.names, set(&[SECRET_NAME]));
    }
}

fn up_name(r: &Result<Timestamp, FaceUpRefusal>) -> &'static str {
    match r {
        Ok(_) => "Ok",
        Err(FaceUpRefusal::NotFaceDown) => "NotFaceDown",
        Err(FaceUpRefusal::InstantOrSorcery) => "InstantOrSorcery",
        Err(FaceUpRefusal::Layer(_)) => "Layer",
    }
}

/// REQ-OBJ-003, CR 708.8: `turn_face_up` with an unknown effect id, with a non-face-down effect
/// (a copy effect) or with a face-down effect of a different object is `NotFaceDown` and changes
/// nothing; with the instant-or-sorcery flag set the same cases are still `NotFaceDown`.
#[test]
fn req_obj_003_turn_face_up_not_face_down_changes_nothing() {
    let copied = CopiableValues::plain(other_d()).copy_with(&secret_face_up(), &[]);
    let mut w = world();
    ok_ts(
        down(
            &mut w,
            eid(1),
            &FaceDownProfile::default_2_2(),
            &plain(),
            oid(Y),
        ),
        "Y face down",
    );
    let tc = w.next_ts();
    w.reg(eid(2), copy_eff(&copied, tc.get(), &[oid(X)]));
    let snap = w.snapshot();
    for flag in [false, true] {
        for (effect, ctx) in [
            (eid(99), "unknown id"),
            (eid(2), "copy effect"),
            (eid(1), "another object's face-down effect"),
        ] {
            let r = up(&mut w, effect, oid(X), flag);
            assert_eq!(up_name(&r), "NotFaceDown", "{ctx} flag {flag}");
            w.assert_unchanged(&snap, ctx);
        }
    }
    assert_eq!(w.chars(oid(Y)), expected_default());
}

/// REQ-OBJ-003, CR 701.40g, 701.58g, 731.2g: an instant or sorcery card would turn face up:
/// `InstantOrSorcery`, it stays face down, no timestamp is consumed and nothing changes.
#[test]
fn req_obj_003_701_40g_instant_or_sorcery_stays_face_down() {
    for (p, want) in profiles() {
        let mut w = FdWorld::new();
        w.env
            .put(oid(X), blank("test.fd_hidden_sorcery", &["Sorcery"]));
        let t_down = ok_ts(down(&mut w, eid(1), &p, &plain(), oid(X)), "down");
        let snap = w.snapshot();
        let r = up(&mut w, eid(1), oid(X), true);
        assert_eq!(up_name(&r), "InstantOrSorcery");
        w.assert_unchanged(&snap, "instant or sorcery");
        assert_eq!(w.chars(oid(X)), want);
        assert_eq!(w.ledger.object_timestamp(oid(X)), Some(t_down));
        assert!(has_face_down_effect_on(&w.effects, oid(X)));
    }
}

/// REQ-OBJ-003, REQ-CORE-002: turning face up with an exhausted timestamp generator is
/// `Layer(..)` and changes nothing.
#[test]
fn req_obj_003_turn_face_up_exhausted_ids_is_layer_and_unchanged() {
    let mut w = world();
    ok_ts(
        down(
            &mut w,
            eid(1),
            &FaceDownProfile::default_2_2(),
            &plain(),
            oid(X),
        ),
        "down",
    );
    w.ids = IdGenerator::from_last_issued(u64::MAX);
    let snap = w.snapshot();
    let r = up(&mut w, eid(1), oid(X), false);
    assert_eq!(up_name(&r), "Layer");
    w.assert_unchanged(&snap, "exhausted");
    assert_eq!(w.chars(oid(X)), expected_default());
}

/// REQ-OBJ-003, CR 708.2a: turning face down a permanent whose face-down effect was removed
/// (it turned face up) works again with a fresh effect id, and the earlier id is reusable.
#[test]
fn req_obj_003_turn_down_again_after_face_up() {
    let mut w = world();
    let p = FaceDownProfile::with_ward_2();
    ok_ts(down(&mut w, eid(1), &p, &plain(), oid(X)), "down 1");
    ok_ts(up(&mut w, eid(1), oid(X), false), "up 1");
    ok_ts(down(&mut w, eid(1), &p, &plain(), oid(X)), "down 2 same id");
    assert_eq!(w.chars(oid(X)), expected_ward());
}
