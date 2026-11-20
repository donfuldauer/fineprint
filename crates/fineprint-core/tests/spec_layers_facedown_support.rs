// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic fixtures for the face-down spec tests (REQ-OBJ-003, REQ-OBJ-002,
//! REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-004, REQ-CORE-006; CR 708, 613.2b). Black-box:
//! written from the work-item brief only.
//!
//! Each `spec_layers_facedown_*.rs` test file includes this file with `#[path]`; on its own it
//! is an empty test target. It re-exports the layer-1a copy fixtures of
//! `spec_layers_copy_support.rs` (which re-export the layer-test environment of
//! `spec_layers_support.rs`; both used read-only) and adds the face-down fixtures. Every card
//! and value here is synthetic and neutrally named (R5).

#![allow(dead_code)]

#[path = "spec_layers_copy_support.rs"]
pub mod copy;

use std::collections::{BTreeMap, BTreeSet};

pub use copy::*;

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, Timestamp};
use fineprint_core::layers::{
    ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification, TimestampLedger,
    face_down_effect,
};
use fineprint_core::layouts::FaceDownProfile;
use fineprint_core::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// expected profile values (from the brief, written independently of the implementation)
// ---------------------------------------------------------------------------------------------

/// CR 708.2a: a 2/2 face-down creature with no text, no name, no subtypes and no mana cost
/// (colorless, no color indicator, no supertypes, no abilities, no other characteristic).
pub fn expected_default() -> Characteristics {
    Characteristics {
        names: BTreeSet::new(),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(lit(2)),
        toughness: Some(lit(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The pinned JSON fields of the ward {2} ability (CR 702.168a, 701.58a).
pub fn expected_ward_fields() -> BTreeMap<String, String> {
    [
        ("kind", "KEYWORD"),
        ("abilityId", "facedown:ward"),
        ("keyword", "ward"),
        ("cost", "{2}"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect()
}

/// The JSON fields of an ability, read as string values (panics when a value is not a string).
pub fn ability_fields(a: &AbilityAST) -> BTreeMap<String, String> {
    via_msgpack(a)
}

/// Whether `a` is exactly the pinned ward {2} ability.
pub fn is_ward_2(a: &AbilityAST) -> bool {
    ability_fields_opt(a).is_some_and(|m| m == expected_ward_fields())
}

fn ability_fields_opt(a: &AbilityAST) -> Option<BTreeMap<String, String>> {
    let bytes = rmp_serde::to_vec_named(a).ok()?;
    rmp_serde::from_slice(&bytes).ok()
}

/// CR 702.168a, 701.58a: the default plus exactly the ward {2} ability. The ability value is
/// taken from `with_ward_2` (its JSON representation is not pinned byte for byte) after checking
/// it has exactly the pinned fields.
pub fn expected_ward() -> Characteristics {
    let p = FaceDownProfile::with_ward_2();
    let abilities = &p.characteristics().abilities;
    assert_eq!(abilities.len(), 1, "with_ward_2 has exactly one ability");
    assert!(is_ward_2(&abilities[0]), "with_ward_2 ability fields");
    Characteristics {
        abilities: p.characteristics().abilities.clone(),
        ..expected_default()
    }
}

// ---------------------------------------------------------------------------------------------
// face-up synthetic objects with distinctive marker strings (for leak checks)
// ---------------------------------------------------------------------------------------------

pub const SECRET_NAME: &str = "test.fdsecret_name_q7";
pub const SECRET_SUBTYPE: &str = "Hydra";
pub const SECRET_TEXT: &str = "test.fdsecret_text_q7";
pub const SECRET_ETB: &str = "test.fdsecret_etb_q7";
pub const SECRET_STATIC: &str = "test.fdsecret_static_q7";
pub const SECRET_COST: &str = "{4}{G}{G}";

/// A face-up 5/5 legendary green creature with a name, mana cost, subtypes, rules text, an
/// ETB-like opaque ability, a static ability, colors and every other characteristic set.
pub fn secret_face_up() -> Characteristics {
    Characteristics {
        names: set(&[SECRET_NAME]),
        mana_cost: some_cost(SECRET_COST),
        color_indicator: Some(ColorBitmask::GREEN),
        colors: ColorBitmask::GREEN,
        supertypes: set(&["Legendary"]),
        card_types: set(&["Creature", "Artifact"]),
        subtypes: set(&[SECRET_SUBTYPE, "Elf"]),
        rules_text: text(&[SECRET_TEXT]),
        abilities: vec![json_ability(SECRET_ETB, 1), static_ability(SECRET_STATIC)],
        power: Some(lit(5)),
        toughness: Some(lit(5)),
        loyalty: Some(3),
        defense: Some(4),
        hand_modifier: Some(1),
        life_modifier: Some(2),
    }
}

/// The marker strings whose presence in an opponent-visible value would be a leak.
pub fn secret_markers() -> Vec<String> {
    let mut v: Vec<String> = [
        SECRET_NAME,
        SECRET_SUBTYPE,
        SECRET_TEXT,
        SECRET_ETB,
        SECRET_STATIC,
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    v.push(format!("{:?}", cost(SECRET_COST)));
    v
}

/// Asserts that `debug` contains none of the [`secret_markers`].
pub fn assert_no_secret(debug: &str, ctx: &str) {
    for m in secret_markers() {
        assert!(!debug.contains(&m), "{ctx}: leaked {m:?} in {debug}");
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// `face_down_effect(profile, None, P1, Resolved, ts(t), object)`.
pub fn fd_eff(profile: &FaceDownProfile, t: u64, object: ObjectId) -> ContinuousEffect {
    face_down_effect(profile, None, P1, EffectOrigin::Resolved, ts(t), object)
}

/// `face_down_effect` with a timestamp value.
pub fn fd_eff_at(profile: &FaceDownProfile, t: Timestamp, object: ObjectId) -> ContinuousEffect {
    face_down_effect(profile, None, P1, EffectOrigin::Resolved, t, object)
}

/// Whether `effects` holds a FaceDown modification affecting `object`.
pub fn has_face_down_effect_on(effects: &ContinuousEffects, object: ObjectId) -> bool {
    effects.iter().any(|(_, e)| {
        matches!(e.modification, LayerModification::FaceDown(_))
            && matches!(&e.affected, fineprint_core::layers::Affected::Objects(s) if s.contains(&object))
    })
}

/// `Ok(t)` or a panic naming `ctx` (no `Debug` bound on the error type is assumed).
pub fn ok_ts<E>(r: Result<Timestamp, E>, ctx: &str) -> Timestamp {
    match r {
        Ok(t) => t,
        Err(_) => panic!("{ctx}: expected Ok(timestamp), got an error"),
    }
}

// ---------------------------------------------------------------------------------------------
// a mutable test world for turning face down and face up
// ---------------------------------------------------------------------------------------------

/// The registry, timestamp ledger, timestamp generator and environment of one test.
#[derive(Clone)]
pub struct FdWorld {
    pub env: TestEnv,
    pub effects: ContinuousEffects,
    pub ledger: TimestampLedger,
    pub ids: IdGenerator<Timestamp>,
}

impl Default for FdWorld {
    fn default() -> FdWorld {
        FdWorld::new()
    }
}

impl FdWorld {
    pub fn new() -> FdWorld {
        FdWorld {
            env: TestEnv::new(),
            effects: ContinuousEffects::new(),
            ledger: TimestampLedger::new(),
            ids: IdGenerator::new(),
        }
    }

    /// Issues the next timestamp from the world's generator.
    pub fn next_ts(&mut self) -> Timestamp {
        self.ids.issue().expect("timestamp issued")
    }

    /// Registers `e` under `id`.
    pub fn reg(&mut self, id: EffectId, e: ContinuousEffect) {
        self.effects.register(id, e).expect("register succeeds");
    }

    /// The evaluated, literal-normalised characteristics of `id`.
    pub fn chars(&self, id: ObjectId) -> Characteristics {
        eval_chars(&self.effects, &self.env, id)
    }

    /// The copiable characteristics of `id`, literal-normalised.
    pub fn copiable(&self, id: ObjectId) -> Characteristics {
        norm(
            &fineprint_core::layers::copiable_characteristics(&self.effects, &self.env, id)
                .expect("known object"),
        )
    }

    /// A snapshot of the mutable parts (effects, ledger, ids) for unchanged-state checks.
    pub fn snapshot(&self) -> (ContinuousEffects, TimestampLedger, IdGenerator<Timestamp>) {
        (self.effects.clone(), self.ledger.clone(), self.ids.clone())
    }

    /// Asserts the mutable parts equal `snap` (by `PartialEq` and by `Debug` text).
    pub fn assert_unchanged(
        &self,
        snap: &(ContinuousEffects, TimestampLedger, IdGenerator<Timestamp>),
        ctx: &str,
    ) {
        assert_eq!(self.effects, snap.0, "{ctx}: effects changed");
        assert_eq!(self.ledger, snap.1, "{ctx}: ledger changed");
        assert_eq!(self.ids, snap.2, "{ctx}: ids changed");
        assert_eq!(
            format!("{:?}", self.effects),
            format!("{:?}", snap.0),
            "{ctx}"
        );
        assert_eq!(
            format!("{:?}", self.ledger),
            format!("{:?}", snap.1),
            "{ctx}"
        );
        assert_eq!(format!("{:?}", self.ids), format!("{:?}", snap.2), "{ctx}");
    }
}
