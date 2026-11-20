// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the layer system (REQ-LAYER-001, REQ-CORE-002, R3): determinism under
//! registration order, base immutability, and strict monotonicity of the timestamp ledger.
//! Pseudo-random inputs come from a hand-written LCG (no `rand`). Black-box: written from the
//! work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::ids::{ObjectId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, CounterHolder, LayerModification,
    SimultaneousStamp, StampReason, TimestampLedger, evaluate, evaluate_all, stamp_resolved_effect,
    stamp_static_and_resolved,
};
use fineprint_core::objects::Characteristics;
use support::*;

const SEEDS: u64 = 200;
const OBJECTS: u64 = 5;

fn random_env(rng: &mut Lcg) -> TestEnv {
    let mut env = TestEnv::new();
    let kinds = ["+1/+1", "-1/-1", "flying", "trample", "ward", "+2/+0"];
    for n in 1..=OBJECTS {
        let c = match rng.below(5) {
            0 => creature(
                &format!("test.random_creature_{n}"),
                1 + rng.below(4) as i64,
                1 + rng.below(4) as i64,
            ),
            1 => enchantment(&format!("test.random_enchantment_{n}")),
            2 => land(&format!("test.random_land_{n}")),
            3 => artifact(&format!("test.random_artifact_{n}")),
            _ => with_abilities(
                creature(&format!("test.random_flier_{n}"), 2, 2),
                vec![kw_ability("k", "flying"), static_ability("test.s")],
            ),
        };
        let c = with_colors(c, (rng.below(32)) as u8);
        let controller = if rng.below(2) == 0 { P1 } else { P2 };
        env.put_full(oid(n), c, Some(controller), rng.below(6) != 0);
        for kind in kinds {
            if rng.below(4) == 0 {
                env.counter(oid(n), kind, 1 + rng.below(3) as i64, rng.below(12));
            }
        }
    }
    env
}

fn random_affected(rng: &mut Lcg) -> Affected {
    if rng.below(2) == 0 {
        let set: BTreeSet<ObjectId> = (1..=OBJECTS)
            .filter(|_| rng.below(2) == 0)
            .map(oid)
            .collect();
        Affected::Objects(set)
    } else {
        let slots = [
            SEL_CREATURE,
            SEL_WHITE_CREATURE,
            SEL_NONCREATURE_ENCHANTMENT,
            SEL_ENCHANTMENT_CREATURE,
            SEL_CONTROLLED_BY_P2,
            SEL_HAS_FLYING,
            SEL_ALL,
            SEL_ENCHANTMENT,
        ];
        by_sel(slots[rng.below(slots.len() as u64) as usize])
    }
}

fn random_effect(rng: &mut Lcg) -> ContinuousEffect {
    let (layer, m) = match rng.below(14) {
        0 => (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(creature("test.copy_source", 3, 3))),
        ),
        1 => (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(Characteristics {
                names: BTreeSet::new(),
                ..creature("unused", 2, 2)
            })),
        ),
        2 => (
            Layer::L2,
            LayerModification::SetController(if rng.below(2) == 0 { P1 } else { P2 }),
        ),
        3 => (
            Layer::L3,
            LayerModification::SetText(text(&["test.random_word"])),
        ),
        4 => (Layer::L4, add_card_types(&["Creature"])),
        5 => (
            Layer::L4,
            LayerModification::SetTypes {
                supertypes: None,
                card_types: Some(set(&["Enchantment"])),
                subtypes: None,
            },
        ),
        6 => (
            Layer::L5,
            LayerModification::SetColors(mask(rng.below(32) as u8)),
        ),
        7 => (Layer::L5, LayerModification::AddColors(mask(1))),
        8 => (
            Layer::L6,
            LayerModification::AddAbility(kw_ability("test.granted", "flying")),
        ),
        9 => (Layer::L6, LayerModification::RemoveAllAbilities),
        10 => (Layer::L6, LayerModification::RemoveKeyword("flying".into())),
        11 => (
            if rng.below(2) == 0 {
                Layer::L7a
            } else {
                Layer::L7b
            },
            set_pt(
                Some(rng.below(5) as i64),
                if rng.below(2) == 0 {
                    None
                } else {
                    Some(rng.below(5) as i64)
                },
            ),
        ),
        12 => (
            Layer::L7c,
            modify_pt(rng.below(5) as i64 - 2, rng.below(5) as i64 - 2),
        ),
        _ => (Layer::L7d, LayerModification::SwitchPt),
    };
    // small timestamp range so that ties happen
    let mut e = eff(layer, rng.below(8), random_affected(rng), m);
    e.source = if rng.below(2) == 0 {
        Some(oid(1 + rng.below(OBJECTS)))
    } else {
        None
    };
    e
}

// REQ-LAYER-001, R3 (determinism): 200 seeds, 6-12 random effects across layers; the
// evaluate_all result is identical for every registration order, equals per-object evaluate,
// is stable when repeated, and never mutates the base.
#[test]
fn evaluation_is_independent_of_registration_order() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let env = random_env(&mut rng);
        let n = 6 + rng.below(7) as usize;
        let effects: Vec<(u64, ContinuousEffect)> = (0..n)
            .map(|i| (100 + i as u64 * 3 + rng.below(3), random_effect(&mut rng)))
            .collect();

        let reference_registry = registry(&effects);
        let bases_before = env.bases.clone();
        let reference = evaluate_all(&reference_registry, &env)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        assert_eq!(reference.len(), OBJECTS as usize);
        for (id, e) in &reference {
            assert_eq!(
                *e,
                evaluate(&reference_registry, &env, *id).unwrap(),
                "seed {seed}"
            );
        }

        for _ in 0..4 {
            let mut order: Vec<usize> = (0..n).collect();
            rng.shuffle(&mut order);
            let mut r = ContinuousEffects::new();
            for i in &order {
                let (id, e) = &effects[*i];
                r.register(eid(*id), e.clone()).unwrap();
            }
            assert_eq!(r, reference_registry, "seed {seed}");
            assert_eq!(
                evaluate_all(&r, &env).unwrap(),
                reference,
                "seed {seed}, order {order:?}"
            );
        }
        assert_eq!(evaluate_all(&reference_registry, &env).unwrap(), reference);
        assert_eq!(env.bases, bases_before, "seed {seed}: base mutated");
    }
}

// REQ-LAYER-001 (CR 613.4): for random P/T effects on one creature, the result equals a
// reference model of the brief's semantics: 7a, 7b setters in (timestamp, EffectId) order, then
// 7c modifications (and P/T counters), then 7d switches.
#[test]
fn pt_matches_a_reference_model() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed ^ 0xA5A5);
        let mut env = TestEnv::new();
        let base_p = rng.below(5) as i64;
        let base_t = rng.below(5) as i64;
        let a = env.put(oid(1), creature("test.model_creature", base_p, base_t));
        let plus = rng.below(3) as i64;
        if plus > 0 {
            env.counter(a, "+1/+1", plus, rng.below(10));
        }
        let n = 6 + rng.below(7);
        let mut effects = Vec::new();
        for i in 0..n {
            let t = rng.below(6);
            let e = match rng.below(4) {
                0 => eff(
                    Layer::L7a,
                    t,
                    objs(&[a]),
                    set_pt(Some(rng.below(6) as i64), None),
                ),
                1 => eff(
                    Layer::L7b,
                    t,
                    objs(&[a]),
                    set_pt(
                        if rng.below(2) == 0 {
                            None
                        } else {
                            Some(rng.below(6) as i64)
                        },
                        Some(rng.below(6) as i64),
                    ),
                ),
                2 => eff(
                    Layer::L7c,
                    t,
                    objs(&[a]),
                    modify_pt(rng.below(7) as i64 - 3, rng.below(7) as i64 - 3),
                ),
                _ => eff(Layer::L7d, t, objs(&[a]), LayerModification::SwitchPt),
            };
            effects.push((i + 1, e));
        }
        // reference model
        let (mut p, mut tough) = (base_p, base_t);
        for layer in [Layer::L7a, Layer::L7b, Layer::L7c, Layer::L7d] {
            let mut in_layer: Vec<&(u64, ContinuousEffect)> =
                effects.iter().filter(|(_, e)| e.layer == layer).collect();
            in_layer.sort_by_key(|(id, e)| (e.timestamp, *id));
            for (_, e) in in_layer {
                match &e.modification {
                    LayerModification::SetPt { power, toughness } => {
                        if let Some(v) = power {
                            p = *v;
                        }
                        if let Some(v) = toughness {
                            tough = *v;
                        }
                    }
                    LayerModification::ModifyPt { power, toughness } => {
                        p += power;
                        tough += toughness;
                    }
                    LayerModification::SwitchPt => std::mem::swap(&mut p, &mut tough),
                    _ => unreachable!(),
                }
            }
            if layer == Layer::L7c {
                // counters are additive: their position within 7c does not matter
                p += plus;
                tough += plus;
            }
        }
        let r = registry(&effects);
        assert_eq!(
            pt(&evaluate(&r, &env, a).unwrap()),
            spt(p, tough),
            "seed {seed}"
        );
    }
}

// REQ-CORE-002, CR 613.7: every timestamp the ledger and the stamp functions issue is
// strictly greater than every earlier one (one monotonic counter), over random operation
// sequences.
#[test]
fn ledger_stamps_are_strictly_increasing() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed.wrapping_mul(31) + 7);
        let mut tsg = tgen();
        let mut ledger = TimestampLedger::new();
        let mut high = Timestamp::new(0);
        let check = |t: Timestamp, what: &str, high: &mut Timestamp| {
            assert!(t > *high, "seed {seed}: {what} {t:?} not after {high:?}");
            *high = t;
        };
        for _ in 0..40 {
            let o = oid(1 + rng.below(6));
            match rng.below(7) {
                0 => {
                    let reasons = [
                        StampReason::EntersZone,
                        StampReason::Attached,
                        StampReason::TurnedFaceUpOrDown,
                        StampReason::TransformedOrConverted,
                        StampReason::SpecialCardTurnedFaceUp,
                        StampReason::GameStart,
                        StampReason::JoinedMergedPermanent,
                    ];
                    let reason = reasons.into_iter().nth(rng.below(7) as usize).unwrap();
                    let t = ledger.stamp_object(&mut tsg, o, reason).unwrap();
                    check(t, "object", &mut high);
                    assert_eq!(ledger.object_timestamp(o), Some(t));
                    // stickers follow right after their host
                    for s in 0..3u32 {
                        if let Some(st) = ledger.sticker_timestamp(o, s) {
                            assert!(st > t, "seed {seed}: sticker before its host");
                            if st > high {
                                high = st;
                            }
                        }
                    }
                    assert_eq!(high.get(), tsg.last_issued(), "seed {seed}");
                }
                1 => {
                    let t = ledger
                        .put_sticker(&mut tsg, o, rng.below(3) as u32)
                        .unwrap();
                    check(t, "sticker", &mut high);
                }
                2 => {
                    let kind = if rng.below(2) == 0 { "+1/+1" } else { "flying" };
                    let holder = if rng.below(4) == 0 {
                        CounterHolder::Player(P1)
                    } else {
                        CounterHolder::Object(o)
                    };
                    let t = ledger.put_counters(&mut tsg, holder, kind).unwrap();
                    check(t, "counter", &mut high);
                }
                3 => {
                    let t = stamp_resolved_effect(&mut tsg).unwrap();
                    check(t, "resolved", &mut high);
                }
                4 => {
                    let (s, r) = stamp_static_and_resolved(&mut tsg).unwrap();
                    check(s, "static", &mut high);
                    check(r, "resolved", &mut high);
                }
                5 => {
                    let count = 1 + rng.below(4);
                    let batch: Vec<SimultaneousStamp> = (0..count)
                        .map(|i| SimultaneousStamp {
                            object: oid(10 + i),
                            player: if rng.below(2) == 0 { P1 } else { P2 },
                            choice: rng.below(3) as u32,
                        })
                        .collect();
                    let out = ledger
                        .stamp_objects_simultaneously(
                            &mut tsg,
                            &[P2, P1],
                            &batch,
                            StampReason::EntersZone,
                        )
                        .unwrap();
                    assert_eq!(out.len(), count as usize);
                    for (_, t) in out {
                        check(t, "simultaneous", &mut high);
                    }
                    // no stickers on objects 10..: the generator is exactly at `high`
                    assert_eq!(high.get(), tsg.last_issued(), "seed {seed}");
                }
                _ => ledger.forget_object(o),
            }
        }
    }
}
