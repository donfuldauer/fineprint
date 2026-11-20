// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic fixtures for the layer-1a copy spec tests (REQ-OBJ-002, REQ-LAYER-001,
//! REQ-CORE-001). Black-box: written from the work-item brief only.
//!
//! Each `spec_layers_copy_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. It re-exports the layer-test environment of `spec_layers_support.rs`
//! (used read-only) and adds synthetic characteristics and card definitions (R5: invented,
//! neutrally named).

#![allow(dead_code)]

#[path = "spec_layers_support.rs"]
pub mod base;

use std::collections::{BTreeMap, BTreeSet};

pub use base::*;

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    ContinuousEffect, ContinuousEffects, CopiableValues, CopiedCharacteristic, EffectOrigin,
    copiable_effect, evaluate,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::card::{
    CardDefinitionDSL, CardFaceDSL, DSL_VERSION, FaceKind, LayoutBehavior, Origin, ProvenanceDSL,
    SupportStatus,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

// ---------------------------------------------------------------------------------------------
// primitives
// ---------------------------------------------------------------------------------------------

pub fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("bundled vocabulary loads")
}

pub fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

pub fn some_cost(text: &str) -> Option<ManaCostDSL> {
    Some(cost(text))
}

/// A synthetic opaque ability: the JSON object `{key: n}`.
pub fn json_ability(key: &str, n: i64) -> AbilityAST {
    let mut m = BTreeMap::new();
    m.insert(key.to_string(), n);
    via_msgpack(&m)
}

/// A CDA-like, non-literal power/toughness expression: the JSON object `{"CDA": key}`.
pub fn cda_like(key: &str) -> ValueExpressionAST {
    let mut m = BTreeMap::new();
    m.insert("CDA".to_string(), key.to_string());
    via_msgpack(&m)
}

pub fn oset(ids: &[ObjectId]) -> BTreeSet<ObjectId> {
    ids.iter().copied().collect()
}

/// Every `CopiedCharacteristic`, in declaration order.
pub const ALL_COPIED: [CopiedCharacteristic; 15] = [
    CopiedCharacteristic::Names,
    CopiedCharacteristic::ManaCost,
    CopiedCharacteristic::ColorIndicator,
    CopiedCharacteristic::Colors,
    CopiedCharacteristic::Supertypes,
    CopiedCharacteristic::CardTypes,
    CopiedCharacteristic::Subtypes,
    CopiedCharacteristic::RulesText,
    CopiedCharacteristic::Abilities,
    CopiedCharacteristic::Power,
    CopiedCharacteristic::Toughness,
    CopiedCharacteristic::Loyalty,
    CopiedCharacteristic::Defense,
    CopiedCharacteristic::HandModifier,
    CopiedCharacteristic::LifeModifier,
];

/// Writes the field(s) `which` names from `src` into `dst`, as the brief defines
/// `CopyException::NotCopied` (CR 707.9c, 707.9d): one field each, and `Colors` also takes the
/// color indicator.
pub fn take_field(which: CopiedCharacteristic, dst: &mut Characteristics, src: &Characteristics) {
    match which {
        CopiedCharacteristic::Names => dst.names = src.names.clone(),
        CopiedCharacteristic::ManaCost => dst.mana_cost = src.mana_cost.clone(),
        CopiedCharacteristic::ColorIndicator => dst.color_indicator = src.color_indicator,
        CopiedCharacteristic::Colors => {
            dst.colors = src.colors;
            dst.color_indicator = src.color_indicator;
        }
        CopiedCharacteristic::Supertypes => dst.supertypes = src.supertypes.clone(),
        CopiedCharacteristic::CardTypes => dst.card_types = src.card_types.clone(),
        CopiedCharacteristic::Subtypes => dst.subtypes = src.subtypes.clone(),
        CopiedCharacteristic::RulesText => dst.rules_text = src.rules_text.clone(),
        CopiedCharacteristic::Abilities => dst.abilities = src.abilities.clone(),
        CopiedCharacteristic::Power => dst.power = src.power.clone(),
        CopiedCharacteristic::Toughness => dst.toughness = src.toughness.clone(),
        CopiedCharacteristic::Loyalty => dst.loyalty = src.loyalty,
        CopiedCharacteristic::Defense => dst.defense = src.defense,
        CopiedCharacteristic::HandModifier => dst.hand_modifier = src.hand_modifier,
        CopiedCharacteristic::LifeModifier => dst.life_modifier = src.life_modifier,
    }
}

// ---------------------------------------------------------------------------------------------
// synthetic characteristics (every field set and pairwise different between the fixtures)
// ---------------------------------------------------------------------------------------------

/// The copied original: every characteristic set to a value no other fixture uses.
pub fn rich_a() -> Characteristics {
    Characteristics {
        names: set(&["test.copy_source_a"]),
        mana_cost: some_cost("{2}{G}{U}"),
        color_indicator: Some(ColorBitmask::RED),
        colors: mask(2 | 8 | 16),
        supertypes: set(&["Legendary"]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Elf"]),
        rules_text: text(&["test.copy_source_a"]),
        abilities: vec![
            kw_ability("test.copy_source_a.kw", "flying"),
            static_ability("test.copy_source_a.static"),
        ],
        power: Some(lit(3)),
        toughness: Some(lit(4)),
        loyalty: Some(5),
        defense: Some(6),
        hand_modifier: Some(-1),
        life_modifier: Some(2),
    }
}

/// A copier's own values before copying.
pub fn own_b() -> Characteristics {
    Characteristics {
        names: set(&["test.copier_b"]),
        mana_cost: some_cost("{1}{W}"),
        color_indicator: Some(ColorBitmask::BLACK),
        colors: mask(1 | 4),
        supertypes: set(&["Snow"]),
        card_types: set(&["Enchantment"]),
        subtypes: set(&["Shrine"]),
        rules_text: text(&["test.copier_b"]),
        abilities: vec![static_ability("test.copier_b.static")],
        power: Some(lit(1)),
        toughness: Some(lit(1)),
        loyalty: Some(7),
        defense: Some(9),
        hand_modifier: Some(3),
        life_modifier: Some(-4),
    }
}

/// A third object's own values.
pub fn own_c() -> Characteristics {
    Characteristics {
        names: set(&["test.copier_c"]),
        mana_cost: some_cost("{5}"),
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Land"]),
        subtypes: set(&["Gate"]),
        rules_text: text(&["test.copier_c"]),
        abilities: vec![static_ability("test.copier_c.mana")],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A distinct creature used as the values of a second copy effect.
pub fn other_d() -> Characteristics {
    Characteristics {
        names: set(&["test.copy_source_d"]),
        mana_cost: some_cost("{3}{R}"),
        colors: ColorBitmask::RED,
        subtypes: set(&["Goblin"]),
        abilities: vec![kw_ability("test.copy_source_d.kw", "haste")],
        ..creature("test.copy_source_d", 5, 1)
    }
}

/// The face-down values the tests register as their own layer-1b input (CR 708.2): a 2/2
/// creature with no name, no mana cost, no color, no subtypes, no text and no abilities.
pub fn face_down_2_2() -> Characteristics {
    Characteristics {
        names: BTreeSet::new(),
        ..creature("unused", 2, 2)
    }
}

// ---------------------------------------------------------------------------------------------
// effects and evaluation
// ---------------------------------------------------------------------------------------------

/// `copiable_effect(values, None, P1, Resolved, ts(t), ids)`.
pub fn copy_eff(values: &CopiableValues, t: u64, ids: &[ObjectId]) -> ContinuousEffect {
    copiable_effect(values, None, P1, EffectOrigin::Resolved, ts(t), oset(ids))
}

/// A layer-1b face-down effect giving `ids` the [`face_down_2_2`] values (a plain test input).
pub fn face_down_eff(t: u64, ids: &[ObjectId]) -> ContinuousEffect {
    eff(
        Layer::L1b,
        t,
        objs(ids),
        fineprint_core::layers::LayerModification::FaceDown(Box::new(face_down_2_2())),
    )
}

/// The evaluated characteristics of `id`, literals normalised ([`norm`]).
pub fn eval_chars(effects: &ContinuousEffects, env: &TestEnv, id: ObjectId) -> Characteristics {
    norm(
        &evaluate(effects, env, id)
            .expect("evaluation succeeds")
            .characteristics,
    )
}

// ---------------------------------------------------------------------------------------------
// synthetic card definitions
// ---------------------------------------------------------------------------------------------

/// A blank face of `kind`.
pub fn blank_face(kind: FaceKind, face_name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: face_name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: Vec::new(),
        subtypes: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: text(&[face_name]),
        abilities: Vec::new(),
    }
}

/// A creature face with a printed (bare JSON integer) power and toughness, one subtype and one
/// ability named after the face.
pub fn creature_face(
    kind: FaceKind,
    face_name: &str,
    mana: &str,
    subtype: &str,
    pt: (i64, i64),
) -> CardFaceDSL {
    CardFaceDSL {
        mana_cost: ManaCostDSL::parse_optional(mana).expect("synthetic cost"),
        card_types: vec!["Creature".to_string()],
        subtypes: vec![subtype.to_string()],
        power: Some(bare(pt.0)),
        toughness: Some(bare(pt.1)),
        abilities: vec![json_ability(face_name, 1)],
        ..blank_face(kind, face_name)
    }
}

/// A synthetic definition with id `id`, `layout` and `faces`.
pub fn def(id: &str, layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: id.to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::Parser,
            parser_version: Some("0.1.0".to_owned()),
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_owned(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout,
        faces,
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities: BTreeMap::new(),
        digital_only: false,
        related_tokens: None,
    }
}

/// The characteristics `Characteristics::from_face` gives for `face`.
pub fn chars_of(face: &CardFaceDSL) -> Characteristics {
    Characteristics::from_face(face, vocab()).expect("valid synthetic face")
}

/// PROTOTYPE: a colorless `{7}` 6/6 artifact creature Construct whose prototype face is a
/// `{X}{2}{R}{R}` 3/2 (the prototype face's own name, types and abilities are not used).
pub fn proto_def() -> CardDefinitionDSL {
    let main = CardFaceDSL {
        card_types: vec!["Artifact".to_string(), "Creature".to_string()],
        ..creature_face(
            FaceKind::Main,
            "test.proto_full_6_6",
            "{7}",
            "Construct",
            (6, 6),
        )
    };
    let proto = CardFaceDSL {
        mana_cost: some_cost("{X}{2}{R}{R}"),
        power: Some(bare(3)),
        toughness: Some(bare(2)),
        abilities: vec![json_ability("test.proto_inset", 1)],
        ..blank_face(FaceKind::Prototype, "test.proto_inset")
    };
    def(
        "test.proto_card",
        LayoutBehavior::Prototype,
        vec![main, proto],
    )
}

/// MUTATE: `{2}{B}{B}` 4/4 Wizard.
pub fn mutant_def() -> CardDefinitionDSL {
    def(
        "test.mutant_4_4",
        LayoutBehavior::Mutate,
        vec![creature_face(
            FaceKind::Main,
            "test.mutant_4_4",
            "{2}{B}{B}",
            "Wizard",
            (4, 4),
        )],
    )
}

/// MUTATE: `{5}{G}` 5/5 Beast.
pub fn mutant_new_def() -> CardDefinitionDSL {
    def(
        "test.mutant_5_5",
        LayoutBehavior::Mutate,
        vec![creature_face(
            FaceKind::Main,
            "test.mutant_5_5",
            "{5}{G}",
            "Beast",
            (5, 5),
        )],
    )
}

/// NORMAL: `{1}{U}` 1/1 Human, the permanent mutated onto.
pub fn merge_base_def() -> CardDefinitionDSL {
    def(
        "test.merge_base_1_1",
        LayoutBehavior::Normal,
        vec![creature_face(
            FaceKind::Main,
            "test.merge_base_1_1",
            "{1}{U}",
            "Human",
            (1, 1),
        )],
    )
}

/// NORMAL: `{W}` 2/3 Soldier, merged underneath.
pub fn merge_under_def() -> CardDefinitionDSL {
    def(
        "test.merge_under_2_3",
        LayoutBehavior::Normal,
        vec![creature_face(
            FaceKind::Main,
            "test.merge_under_2_3",
            "{W}",
            "Soldier",
            (2, 3),
        )],
    )
}

pub const MELD_A: &str = "test.meld_a";
pub const MELD_B: &str = "test.meld_b";

/// Meld card A: front `{2}{W}` 2/2 Human; carries the combined back face, a `{9}{B}` 9/9
/// legendary artifact creature Golem with a red color indicator.
pub fn meld_a() -> CardDefinitionDSL {
    let front = creature_face(
        FaceKind::Main,
        "test.meld_a_front",
        "{2}{W}",
        "Human",
        (2, 2),
    );
    let whole = CardFaceDSL {
        supertypes: vec!["Legendary".to_string()],
        card_types: vec!["Artifact".to_string(), "Creature".to_string()],
        color_indicator: Some(ColorBitmask::RED),
        ..creature_face(
            FaceKind::MeldResult,
            "test.meld_whole",
            "{9}{B}",
            "Golem",
            (9, 9),
        )
    };
    let mut d = def(MELD_A, LayoutBehavior::Meld, vec![front, whole]);
    d.meld_partner = Some(MELD_B.to_owned());
    d
}

/// Meld card B: front `{X}{3}{U}` 1/4 Wizard.
pub fn meld_b() -> CardDefinitionDSL {
    let front = creature_face(
        FaceKind::Main,
        "test.meld_b_front",
        "{X}{3}{U}",
        "Wizard",
        (1, 4),
    );
    let mut d = def(MELD_B, LayoutBehavior::Meld, vec![front]);
    d.meld_partner = Some(MELD_A.to_owned());
    d
}
