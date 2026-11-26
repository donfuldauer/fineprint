// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for copiable values and layer-1a copy effects (REQ-CORE-001, REQ-OBJ-002,
//! REQ-LAYER-001): a plain copy preserves everything and is idempotent, evaluation is
//! independent of registration order, copying never changes the original, and copying nothing
//! gives the copier's own values. Pseudo-random inputs come from the hand-written LCG of the
//! layer-test support (no `rand`, R3). Black-box: written from the work-item brief only.

#[path = "spec_layers_copy_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_core::layers::{
    ContinuousEffect, ContinuousEffects, CopiableValues, CopyException, LayerModification,
    copiable_characteristics, evaluate,
};
use fineprint_core::objects::Characteristics;
use support::*;

const SEEDS: u64 = 200;
const OBJECTS: u64 = 4;

fn pick<'a>(rng: &mut Lcg, pool: &[&'a str]) -> &'a str {
    pool[rng.below(pool.len() as u64) as usize]
}

fn pick_set(rng: &mut Lcg, pool: &[&str]) -> BTreeSet<String> {
    pool.iter()
        .filter(|_| rng.below(3) == 0)
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

/// A random characteristics value; `literal_pt` keeps power and toughness literal (for
/// evaluation), otherwise they may be CDA-like expressions.
fn random_chars(rng: &mut Lcg, tag: u64, literal_pt: bool) -> Characteristics {
    let costs = ["{1}", "{2}{G}", "{X}{U}", "{W}{B}", "{0}", "{3}{R}{R}"];
    let pt_value = |rng: &mut Lcg| -> Option<ValueExpressionAST> {
        match rng.below(if literal_pt { 3 } else { 4 }) {
            0 => None,
            1 => Some(lit(rng.below(7) as i64)),
            2 => Some(lit(rng.below(7) as i64)),
            _ => Some(cda_like("count")),
        }
    };
    let n_names = rng.below(3);
    let names = (0..n_names)
        .map(|i| format!("test.random_{tag}_{i}"))
        .collect();
    let abilities = (0..rng.below(3))
        .map(|i| static_ability(&format!("test.random_{tag}.ab{i}")))
        .collect();
    Characteristics {
        names,
        mana_cost: if rng.below(4) == 0 {
            None
        } else {
            some_cost(pick(rng, &costs))
        },
        color_indicator: if rng.below(3) == 0 {
            Some(mask(rng.below(32) as u8))
        } else {
            None
        },
        colors: mask(rng.below(32) as u8),
        supertypes: pick_set(rng, &["Legendary", "Snow", "Basic"]),
        card_types: pick_set(rng, &["Creature", "Artifact", "Enchantment", "Land"]),
        subtypes: pick_set(rng, &["Elf", "Wizard", "Golem", "Shrine"]),
        rules_text: text(&[&format!("test.random_{tag}")]),
        abilities,
        power: pt_value(rng),
        toughness: pt_value(rng),
        loyalty: opt_i64(rng),
        defense: opt_i64(rng),
        hand_modifier: opt_i64(rng),
        life_modifier: opt_i64(rng),
    }
}

fn random_values(rng: &mut Lcg, tag: u64) -> CopiableValues {
    let c = random_chars(rng, tag, false);
    match rng.below(3) {
        0 => CopiableValues::plain(c),
        1 => CopiableValues::nonmodal_back_face(c, some_cost("{2}{G}")),
        _ => CopiableValues::nonmodal_back_face(c, None)
            .copy_with(&random_chars(rng, tag + 1000, false), &[]),
    }
}

fn random_exception(rng: &mut Lcg) -> CopyException {
    match rng.below(14) {
        0 => CopyException::GainAbility(static_ability("test.gained")),
        1 => CopyException::SetNames(set(&["test.renamed"])),
        2 => CopyException::SetManaCost(some_cost("{4}")),
        3 => CopyException::SetColors(mask(rng.below(32) as u8)),
        4 => CopyException::SetSupertypes(set(&["Snow"])),
        5 => CopyException::SetCardTypes(set(&["Enchantment"])),
        6 => CopyException::SetSubtypes(set(&["Spirit"])),
        7 => CopyException::AddSupertypes(set(&["Legendary"])),
        8 => CopyException::AddCardTypes(set(&["Creature"])),
        9 => CopyException::AddSubtypes(set(&["Mutant"])),
        10 => CopyException::SetPower(lit(rng.below(5) as i64)),
        11 => CopyException::SetToughness(lit(rng.below(5) as i64)),
        _ => CopyException::NotCopied(ALL_COPIED[rng.below(ALL_COPIED.len() as u64) as usize]),
    }
}

/// REQ-OBJ-002, REQ-CORE-001, CR 707.2, 707.3: a copy with no exception preserves every
/// characteristic of the original, and a copy of the copy equals the copy.
#[test]
fn prop_plain_copy_preserves_everything_and_is_idempotent() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let original = random_values(&mut rng, 1);
        let own1 = random_chars(&mut rng, 2, false);
        let own2 = random_chars(&mut rng, 3, false);
        let copy = original.copy_with(&own1, &[]);
        assert_eq!(
            copy.characteristics, original.characteristics,
            "seed {seed}"
        );
        let copy2 = copy.copy_with(&own2, &[]);
        assert_eq!(copy2.characteristics, copy.characteristics, "seed {seed}");
        assert_eq!(copy2.source, copy.source, "seed {seed}");
    }
}

/// REQ-OBJ-002, REQ-CORE-001, CR 707.2b: copying (with any exceptions) or modifying never
/// changes the original's values.
#[test]
fn prop_copy_never_changes_the_original() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let original = random_values(&mut rng, 1);
        let chars_before = original.characteristics.clone();
        let source_before = original.source.clone();
        let own = random_chars(&mut rng, 2, false);
        let own_before = own.clone();
        let ex: Vec<CopyException> = (0..rng.below(5))
            .map(|_| random_exception(&mut rng))
            .collect();
        let ex_before = ex.clone();
        let _ = original.copy_with(&own, &ex);
        let _ = original.modified(&ex);
        assert_eq!(original.characteristics, chars_before, "seed {seed}");
        assert_eq!(original.source, source_before, "seed {seed}");
        assert_eq!(own, own_before, "seed {seed}");
        assert_eq!(ex, ex_before, "seed {seed}");
    }
}

/// REQ-OBJ-002, CR 707.9c: `NotCopied` of every characteristic gives exactly the copier's own
/// values, field by field, whatever the original.
#[test]
fn prop_not_copied_everything_is_own() {
    let all: Vec<CopyException> = ALL_COPIED
        .iter()
        .map(|c| CopyException::NotCopied(*c))
        .collect();
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let original = random_values(&mut rng, 1);
        let own = random_chars(&mut rng, 2, false);
        let mut shuffled = all.clone();
        rng.shuffle(&mut shuffled);
        for ex in [&all, &shuffled] {
            let got = original.copy_with(&own, ex).characteristics;
            assert_eq!(got.names, own.names, "seed {seed}");
            assert_eq!(got.mana_cost, own.mana_cost, "seed {seed}");
            assert_eq!(got.color_indicator, own.color_indicator, "seed {seed}");
            assert_eq!(got.colors, own.colors, "seed {seed}");
            assert_eq!(got.supertypes, own.supertypes, "seed {seed}");
            assert_eq!(got.card_types, own.card_types, "seed {seed}");
            assert_eq!(got.subtypes, own.subtypes, "seed {seed}");
            assert_eq!(got.rules_text, own.rules_text, "seed {seed}");
            assert_eq!(got.abilities, own.abilities, "seed {seed}");
            assert_eq!(got.power, own.power, "seed {seed}");
            assert_eq!(got.toughness, own.toughness, "seed {seed}");
            assert_eq!(got.loyalty, own.loyalty, "seed {seed}");
            assert_eq!(got.defense, own.defense, "seed {seed}");
            assert_eq!(got.hand_modifier, own.hand_modifier, "seed {seed}");
            assert_eq!(got.life_modifier, own.life_modifier, "seed {seed}");
            assert_eq!(got, own, "seed {seed}");
        }
    }
}

fn random_affected_ids(rng: &mut Lcg) -> Vec<fineprint_base::ids::ObjectId> {
    let ids: Vec<_> = (1..=OBJECTS)
        .filter(|_| rng.below(2) == 0)
        .map(oid)
        .collect();
    if ids.is_empty() {
        vec![oid(1 + rng.below(OBJECTS))]
    } else {
        ids
    }
}

fn random_effect(rng: &mut Lcg, env: &TestEnv, k: u64) -> ContinuousEffect {
    // Timestamps in a small range so equal timestamps (effect-id tie-break) occur.
    let t = 1 + rng.below(6);
    let ids = random_affected_ids(rng);
    match rng.below(6) {
        0 | 1 => {
            let src = oid(1 + rng.below(OBJECTS));
            let values = CopiableValues::plain(env.base_chars(src).clone());
            let ex: Vec<CopyException> = (0..rng.below(3)).map(|_| random_exception(rng)).collect();
            let v = values.copy_with(&random_chars(rng, 500 + k, true), &ex);
            copy_eff(&v, t, &ids)
        }
        2 => {
            let m = CopiableValues::plain(random_chars(rng, 600 + k, true)).modified(&[
                CopyException::SetPower(lit(rng.below(6) as i64)),
                CopyException::SetToughness(lit(rng.below(6) as i64)),
            ]);
            copy_eff(&m, t, &ids)
        }
        3 => face_down_eff(t, &ids),
        4 => eff(Layer::L4, t, objs(&ids), add_card_types(&["Artifact"])),
        _ => eff(
            Layer::L6,
            t,
            objs(&ids),
            LayerModification::AddAbility(kw_ability("test.granted", "trample")),
        ),
    }
}

fn snapshot(effects: &ContinuousEffects, env: &TestEnv) -> Vec<(Characteristics, Characteristics)> {
    (1..=OBJECTS)
        .map(|n| {
            (
                norm(
                    &evaluate(effects, env, oid(n))
                        .expect("evaluates")
                        .characteristics,
                ),
                copiable_characteristics(effects, env, oid(n)).expect("known"),
            )
        })
        .collect()
}

/// REQ-CORE-001, REQ-OBJ-002, REQ-LAYER-001, CR 613.2, 613.7: registering the same set of
/// layer-1 effects (copies, "as enters" P/T, face-down) and later-layer effects in any order
/// gives the same evaluated and copiable characteristics for every object.
#[test]
fn prop_registration_order_does_not_matter() {
    for seed in 0..SEEDS {
        let mut rng = Lcg::new(seed);
        let mut env = TestEnv::new();
        for n in 1..=OBJECTS {
            env.put(oid(n), random_chars(&mut rng, n, true));
        }
        let count = 2 + rng.below(5);
        let list: Vec<(u64, ContinuousEffect)> = (0..count)
            .map(|k| (k + 1, random_effect(&mut rng, &env, k)))
            .collect();
        let baseline = snapshot(&registry(&list), &env);
        for _ in 0..4 {
            let mut shuffled = list.clone();
            rng.shuffle(&mut shuffled);
            assert_eq!(
                snapshot(&registry(&shuffled), &env),
                baseline,
                "seed {seed}"
            );
        }
    }
}
