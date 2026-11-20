// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property and determinism tests for face-down objects (REQ-OBJ-003, REQ-OBJ-002,
//! REQ-LAYER-001, REQ-CORE-006; CR 708.2, 613.2b, 613.2c, 613.7f, 708.2b, 708.8, 712.13,
//! 731.2j): face down hides everything whatever the base, the copy effects and their order;
//! down then up is the identity on copiable values with strictly increasing timestamps; every
//! refusal changes nothing; replaying a sequence gives identical digests and timestamps.
//! Pseudo-random inputs come from the hand-written LCG of the layer-test support with fixed
//! seeds (no `rand`, R3), as in the other spec_layers_*_property.rs files. Black-box: written
//! from the work-item brief only.

#[path = "spec_layers_facedown_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::{EffectId, ObjectId, Timestamp};
use fineprint_core::layers::{
    CopiableValues, EffectOrigin, FaceDownRefusal, FaceDownTarget, FaceUpRefusal,
    LayerModification, StampReason, evaluate_all, evaluated_digest, turn_face_down, turn_face_up,
};
use fineprint_core::layouts::{FaceDownCause, FaceDownProfile};
use fineprint_core::objects::Characteristics;
use support::*;

const SEEDS: u64 = 200;
const OBJECTS: u64 = 3;

fn pick<'a>(rng: &mut Lcg, pool: &[&'a str]) -> &'a str {
    pool[rng.below(pool.len() as u64) as usize]
}

fn pick_set(rng: &mut Lcg, pool: &[&str]) -> BTreeSet<String> {
    pool.iter()
        .filter(|_| rng.below(2) == 0)
        .map(|s| (*s).to_string())
        .collect()
}

fn opt_i64(rng: &mut Lcg) -> Option<i64> {
    if rng.below(2) == 0 {
        None
    } else {
        Some(rng.below(9) as i64 - 2)
    }
}

/// Random synthetic characteristics with literal power and toughness; every name, text and
/// ability carries `tag` so a leak is recognisable.
fn random_chars(rng: &mut Lcg, tag: u64) -> Characteristics {
    let costs = ["{1}", "{2}{G}", "{X}{U}", "{W}{B}", "{3}{R}{R}", "{5}"];
    let names = (0..1 + rng.below(2))
        .map(|i| format!("test.fdrand_{tag}_{i}"))
        .collect();
    let mut abilities: Vec<AbilityAST> = (0..1 + rng.below(3))
        .map(|i| static_ability(&format!("test.fdrand_{tag}.ab{i}")))
        .collect();
    if rng.below(2) == 0 {
        abilities.push(json_ability(&format!("test.fdrand_{tag}.etb"), 1));
    }
    if rng.below(3) == 0 {
        abilities.push(kw_ability(&format!("test.fdrand_{tag}.kw"), "flying"));
    }
    Characteristics {
        names,
        mana_cost: some_cost(pick(rng, &costs)),
        color_indicator: if rng.below(3) == 0 {
            Some(mask(rng.below(32) as u8))
        } else {
            None
        },
        colors: mask(rng.below(32) as u8),
        supertypes: pick_set(rng, &["Legendary", "Snow"]),
        card_types: pick_set(rng, &["Creature", "Artifact", "Enchantment"]),
        subtypes: {
            let mut s = pick_set(rng, &["Elf", "Wizard", "Golem"]);
            s.insert("Hydra".to_string());
            s
        },
        rules_text: text(&[&format!("test.fdrand_{tag}")]),
        abilities,
        power: Some(lit(rng.below(9) as i64)),
        toughness: Some(lit(1 + rng.below(9) as i64)),
        loyalty: opt_i64(rng),
        defense: opt_i64(rng),
        hand_modifier: opt_i64(rng),
        life_modifier: opt_i64(rng),
    }
}

fn random_profile(rng: &mut Lcg) -> (FaceDownProfile, Characteristics) {
    let causes = [
        FaceDownCause::Morph,
        FaceDownCause::Megamorph,
        FaceDownCause::Manifest,
        FaceDownCause::ManifestDread,
        FaceDownCause::Disguise,
        FaceDownCause::Cloak,
    ];
    let cause = causes[rng.below(6) as usize];
    let want = match cause {
        FaceDownCause::Disguise | FaceDownCause::Cloak => expected_ward(),
        _ => expected_default(),
    };
    (FaceDownProfile::for_cause(cause), want)
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

/// Every string a leak of `c` would show: names, rules-text words, abilities' Debug text.
fn leak_markers(c: &Characteristics) -> Vec<String> {
    let mut v: Vec<String> = c.names.iter().cloned().collect();
    v.extend(c.rules_text.0.iter().map(|w| format!("{w:?}")));
    v.extend(c.abilities.iter().map(|a| format!("{a:?}")));
    v.push("Hydra".to_string());
    v
}

/// REQ-OBJ-003, REQ-LAYER-002, CR 708.2, 613.2b, 613.2c (property a): for random bases, random
/// layer-1a copy effects and random timestamps and registration orders, a face-down object's
/// names, mana cost, subtypes, rules text and abilities (and every other field) are the
/// profile's, and nothing of the base or the copied values shows.
#[test]
fn req_obj_003_prop_face_down_hides_everything() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let mut env = TestEnv::new();
        let base = random_chars(&mut rng, 1);
        env.put(oid(1), base.clone());
        let (p, want) = random_profile(&mut rng);
        let n_copies = rng.below(3);
        let copies: Vec<Characteristics> = (0..n_copies)
            .map(|k| random_chars(&mut rng, 10 + k))
            .collect();
        let mut list = Vec::new();
        for (k, c) in copies.iter().enumerate() {
            let v = CopiableValues::plain(c.clone());
            list.push((k as u64 + 2, copy_eff(&v, 1 + rng.below(8), &[oid(1)])));
        }
        list.push((1, fd_eff(&p, 1 + rng.below(8), oid(1))));
        rng.shuffle(&mut list);
        let effects = registry(&list);
        let got = eval_chars(&effects, &env, oid(1));
        assert_eq!(got.names, want.names, "seed {seed}");
        assert_eq!(got.mana_cost, want.mana_cost, "seed {seed}");
        assert_eq!(got.subtypes, want.subtypes, "seed {seed}");
        assert_eq!(got.rules_text, want.rules_text, "seed {seed}");
        assert_eq!(got.abilities, want.abilities, "seed {seed}");
        assert_eq!(got, want, "seed {seed}");
        let debug = format!("{got:?}");
        for source in std::iter::once(&base).chain(copies.iter()) {
            for m in leak_markers(source) {
                assert!(!debug.contains(&m), "seed {seed}: leaked {m}");
            }
        }
        assert_eq!(
            copy::base::norm(
                &fineprint_core::layers::copiable_characteristics(&effects, &env, oid(1)).unwrap()
            ),
            want,
            "seed {seed}: copiable"
        );
    }
}

/// REQ-OBJ-003, REQ-OBJ-002, CR 708.8, 613.7f (property b): turning face down then face up is
/// the identity on the copiable characteristics (and on the evaluated characteristics) for
/// random copy and later-layer effects, and the two timestamps strictly increase.
#[test]
fn req_obj_003_prop_down_then_up_is_identity_on_copiable_values() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let mut w = FdWorld::new();
        for n in 1..=OBJECTS {
            w.env.put(oid(n), random_chars(&mut rng, n));
        }
        let target_obj = oid(1 + rng.below(OBJECTS));
        let mut next_eid = 1;
        for k in 0..rng.below(4) {
            let t = w.next_ts();
            let affected = oid(1 + rng.below(OBJECTS));
            let e = match rng.below(3) {
                0 => copy_eff(
                    &CopiableValues::plain(random_chars(&mut rng, 100 + k)),
                    t.get(),
                    &[affected],
                ),
                1 => eff(Layer::L7c, t.get(), objs(&[affected]), modify_pt(1, 2)),
                _ => eff(
                    Layer::L6,
                    t.get(),
                    objs(&[affected]),
                    LayerModification::AddAbility(kw_ability("test.fdrand.granted", "trample")),
                ),
            };
            w.reg(eid(next_eid), e);
            next_eid += 1;
        }
        if rng.below(2) == 0 {
            w.ledger
                .stamp_object(&mut w.ids, target_obj, StampReason::EntersZone)
                .unwrap();
        }
        let before_copiable: Vec<Characteristics> =
            (1..=OBJECTS).map(|n| w.copiable(oid(n))).collect();
        let before_chars: Vec<Characteristics> = (1..=OBJECTS).map(|n| w.chars(oid(n))).collect();
        let effects_before = w.effects.clone();
        let last_before = w.ids.last_issued();

        let (p, want) = random_profile(&mut rng);
        let fd_id = eid(next_eid);
        let t_down = ok_ts(
            down(&mut w, fd_id, &p, &FaceDownTarget::default(), target_obj),
            "down",
        );
        assert!(t_down.get() > last_before, "seed {seed}");
        assert_eq!(w.copiable(target_obj), want, "seed {seed}");
        let t_up = ok_ts(up(&mut w, fd_id, target_obj), "up");
        assert!(t_up > t_down, "seed {seed}");
        assert_eq!(w.ledger.object_timestamp(target_obj), Some(t_up));

        let after_copiable: Vec<Characteristics> =
            (1..=OBJECTS).map(|n| w.copiable(oid(n))).collect();
        let after_chars: Vec<Characteristics> = (1..=OBJECTS).map(|n| w.chars(oid(n))).collect();
        assert_eq!(after_copiable, before_copiable, "seed {seed}");
        assert_eq!(after_chars, before_chars, "seed {seed}");
        assert_eq!(w.effects, effects_before, "seed {seed}: registry restored");
    }
}

/// REQ-OBJ-003, CR 708.2b, 712.13, 731.2j (property c): for random flag combinations with at
/// least one flag set, random registries, ledgers and generators, `turn_face_down` is refused
/// with the first applicable refusal and `effects`, `ledger` and `ids` are unchanged.
#[test]
fn req_obj_003_prop_refusals_change_nothing() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let mut w = FdWorld::new();
        for n in 1..=OBJECTS {
            w.env.put(oid(n), random_chars(&mut rng, n));
        }
        for k in 0..rng.below(4) {
            let t = w.next_ts();
            let affected = oid(1 + rng.below(OBJECTS));
            let e = if rng.below(2) == 0 {
                fd_eff_at(&FaceDownProfile::default_2_2(), t, affected)
            } else {
                eff(Layer::L7c, t.get(), objs(&[affected]), modify_pt(1, 1))
            };
            w.reg(eid(k + 1), e);
        }
        for _ in 0..rng.below(3) {
            let o = oid(1 + rng.below(OBJECTS));
            w.ledger
                .stamp_object(&mut w.ids, o, StampReason::EntersZone)
                .unwrap();
        }
        let bits = 1 + rng.below(7);
        let t = FaceDownTarget {
            already_face_down: bits & 1 != 0,
            double_faced_permanent: bits & 2 != 0,
            merged_contains_double_faced: bits & 4 != 0,
        };
        let (p, _) = random_profile(&mut rng);
        // Sometimes also an effect id that is already in use: the flag refusal still comes first.
        let effect = if rng.below(2) == 0 && !w.effects.is_empty() {
            eid(1)
        } else {
            eid(50)
        };
        let target_obj = oid(1 + rng.below(OBJECTS));
        let snap = w.snapshot();
        let chars_before: Vec<Characteristics> = (1..=OBJECTS).map(|n| w.chars(oid(n))).collect();
        let r = down(&mut w, effect, &p, &t, target_obj);
        let ok = match r {
            Err(FaceDownRefusal::AlreadyFaceDown) => t.already_face_down,
            Err(FaceDownRefusal::DoubleFacedPermanent) => {
                !t.already_face_down && t.double_faced_permanent
            }
            Err(FaceDownRefusal::MergedContainsDoubleFaced) => {
                !t.already_face_down && !t.double_faced_permanent && t.merged_contains_double_faced
            }
            _ => false,
        };
        assert!(ok, "seed {seed}: wrong outcome for {t:?}");
        w.assert_unchanged(&snap, &format!("seed {seed}"));
        let chars_after: Vec<Characteristics> = (1..=OBJECTS).map(|n| w.chars(oid(n))).collect();
        assert_eq!(chars_after, chars_before, "seed {seed}");
    }
}

/// One deterministic operation sequence: objects, a copy, turning face down, later-layer
/// effects, turning face up, turning face down again. Returns the digest of every object after
/// each step and every returned timestamp.
fn run_sequence(seed: u64) -> (Vec<fineprint_base::hash::Digest>, Vec<Timestamp>) {
    let mut rng = Lcg::new(seed);
    let mut w = FdWorld::new();
    for n in 1..=OBJECTS {
        w.env.put(oid(n), random_chars(&mut rng, n));
    }
    let mut digests = Vec::new();
    let mut stamps = Vec::new();
    let snap = |w: &FdWorld| evaluated_digest(&evaluate_all(&w.effects, &w.env).unwrap());

    let t = w.next_ts();
    stamps.push(t);
    let copied = CopiableValues::plain(w.env.base_chars(oid(2)).clone());
    w.reg(eid(1), copy_eff(&copied, t.get(), &[oid(1)]));
    digests.push(snap(&w));

    let (p, _) = random_profile(&mut rng);
    stamps.push(ok_ts(
        down(&mut w, eid(2), &p, &FaceDownTarget::default(), oid(1)),
        "down",
    ));
    digests.push(snap(&w));

    let t = w.next_ts();
    stamps.push(t);
    w.reg(
        eid(3),
        eff(Layer::L7c, t.get(), objs(&[oid(1)]), modify_pt(2, 2)),
    );
    digests.push(snap(&w));

    stamps.push(ok_ts(up(&mut w, eid(2), oid(1)), "up"));
    digests.push(snap(&w));

    stamps.push(ok_ts(
        down(
            &mut w,
            eid(4),
            &FaceDownProfile::with_ward_2(),
            &FaceDownTarget::default(),
            oid(3),
        ),
        "down 3",
    ));
    digests.push(snap(&w));
    (digests, stamps)
}

/// REQ-CORE-006, REQ-OBJ-003, R3: replaying the same operation sequence twice gives identical
/// evaluated digests after every step and identical timestamps; the face-down steps change the
/// digest (the check is meaningful).
#[test]
fn req_core_006_req_obj_003_replay_is_deterministic() {
    for seed in 0..20 {
        let a = run_sequence(seed);
        let b = run_sequence(seed);
        assert_eq!(a, b, "seed {seed}");
        assert_ne!(
            a.0[0], a.0[1],
            "seed {seed}: turning face down changes the digest"
        );
        assert_ne!(
            a.0[2], a.0[3],
            "seed {seed}: turning face up changes the digest"
        );
        assert!(
            a.1.windows(2).all(|p| p[0] < p[1]),
            "seed {seed}: {:?}",
            a.1
        );
    }
}
