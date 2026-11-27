// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the canonical encodings of the layer values stored in the game state
// (REQ-NFR-006, REQ-CORE-006, REQ-LAYER-001, REQ-CORE-002; CR-005): every field and every
// variant reaches the bytes (a change of any one changes the encoding), the documented tags,
// the empty encodings, and the DSL-value fallback. Synthetic values and neutral names only.

use std::collections::BTreeSet;

use fineprint_base::encode::{encode_to_vec, put_bytes};
use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter};
use fineprint_dsl::text::RewritableCategory;
use serde::Serialize;

use super::encode::{layer_tag, put_json, put_typed};
use super::*;
use crate::objects::Characteristics;

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn ability(tag: &str) -> AbilityAST {
    test_ability(&format!("test.ability.{tag}"))
}

/// A one-token synthetic rules text whose literal is `text`.
fn word_text(text: &str) -> TextTokenStream {
    serde_json::from_value(serde_json::json!([{"id": "t1", "kind": "LITERAL", "text": text}]))
        .unwrap()
}

fn chars() -> Characteristics {
    Characteristics {
        names: set(["test.encode_creature"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: set(["Testkin"]),
        rules_text: TextTokenStream::default(),
        abilities: vec![ability("a")],
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn selector(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

/// Asserts the encodings of `items` are pairwise distinct.
fn all_distinct<T: fineprint_base::encode::CanonicalEncode + std::fmt::Debug>(items: &[T]) {
    let bytes: Vec<Vec<u8>> = items.iter().map(encode_to_vec).collect();
    for i in 0..bytes.len() {
        for j in i + 1..bytes.len() {
            assert_ne!(bytes[i], bytes[j], "{:?} vs {:?}", items[i], items[j]);
        }
    }
}

/// Every characteristics value differing from `chars()` in exactly one field.
fn characteristics_variants() -> Vec<Characteristics> {
    let base = chars();
    let one = |f: &dyn Fn(&mut Characteristics)| {
        let mut c = base.clone();
        f(&mut c);
        c
    };
    vec![
        base.clone(),
        one(&|c| c.names = set(["test.other"])),
        one(&|c| c.mana_cost = Some("{1}{G}".parse().unwrap())),
        one(&|c| c.color_indicator = Some(ColorBitmask::RED)),
        one(&|c| c.colors = ColorBitmask::BLUE),
        one(&|c| c.supertypes = set(["Legendary"])),
        one(&|c| c.card_types = set(["Artifact"])),
        one(&|c| c.subtypes = set(["Otherkin"])),
        one(&|c| c.rules_text = word_text("w")),
        one(&|c| c.abilities.push(ability("b"))),
        one(&|c| c.abilities.clear()),
        one(&|c| c.power = Some(literal(3))),
        one(&|c| c.power = None),
        one(&|c| c.toughness = Some(literal(3))),
        one(&|c| c.loyalty = Some(4)),
        one(&|c| c.defense = Some(4)),
        one(&|c| c.hand_modifier = Some(-1)),
        one(&|c| c.life_modifier = Some(-1)),
    ]
}

fn provenance(source: ValueSource, zero: bool, pinned: bool) -> ManaValueProvenance {
    ManaValueProvenance {
        source,
        zero_when_copied: zero,
        colors_pinned: pinned,
    }
}

/// One value of every modification variant, plus payload variations.
fn modifications() -> Vec<LayerModification> {
    let mut out: Vec<LayerModification> = characteristics_variants()
        .into_iter()
        .map(|c| LayerModification::CopyValues(Box::new(c)))
        .collect();
    out.extend(
        characteristics_variants()
            .into_iter()
            .map(|c| LayerModification::FaceDown(Box::new(c))),
    );
    for p in [
        provenance(ValueSource::Plain, false, false),
        provenance(ValueSource::Plain, false, true),
        provenance(ValueSource::Fixed { mana_value: 3 }, false, false),
        provenance(ValueSource::Fixed { mana_value: 3 }, true, false),
        provenance(ValueSource::Fixed { mana_value: 4 }, true, false),
        provenance(ValueSource::CopyOfBackFaceOrMelded, false, false),
    ] {
        out.push(LayerModification::CopyValuesWithProvenance {
            characteristics: Box::new(chars()),
            provenance: p,
        });
    }
    out.push(LayerModification::ApplyCopyExceptions(Vec::new()));
    out.push(LayerModification::ApplyCopyExceptions(vec![
        CopyException::GainAbility(ability("g")),
    ]));
    out.push(LayerModification::SetController(PlayerId::new(1)));
    out.push(LayerModification::SetController(PlayerId::new(2)));
    out.push(LayerModification::SetText(TextTokenStream::default()));
    out.push(LayerModification::SetText(word_text("w")));
    for (category, from, to) in [
        (RewritableCategory::ColorWord, "a", "b"),
        (RewritableCategory::BasicLandType, "a", "b"),
        (RewritableCategory::CreatureType, "a", "b"),
        (RewritableCategory::Number, "a", "b"),
        (RewritableCategory::CardName, "a", "b"),
        (RewritableCategory::ColorWord, "c", "b"),
        (RewritableCategory::ColorWord, "a", "c"),
    ] {
        out.push(LayerModification::ReplaceTextWord {
            category,
            from: from.into(),
            to: to.into(),
        });
    }
    let s = || set(["X"]);
    let e = BTreeSet::new;
    out.push(LayerModification::AddTypes {
        supertypes: s(),
        card_types: e(),
        subtypes: e(),
    });
    out.push(LayerModification::AddTypes {
        supertypes: e(),
        card_types: s(),
        subtypes: e(),
    });
    out.push(LayerModification::AddTypes {
        supertypes: e(),
        card_types: e(),
        subtypes: s(),
    });
    out.push(LayerModification::SetTypes {
        supertypes: Some(s()),
        card_types: None,
        subtypes: None,
    });
    out.push(LayerModification::SetTypes {
        supertypes: None,
        card_types: Some(s()),
        subtypes: None,
    });
    out.push(LayerModification::SetTypes {
        supertypes: None,
        card_types: None,
        subtypes: Some(s()),
    });
    out.push(LayerModification::SetTypes {
        supertypes: None,
        card_types: None,
        subtypes: Some(e()),
    });
    out.push(LayerModification::RemoveSubtypes(s()));
    out.push(LayerModification::SetColors(ColorBitmask::RED));
    out.push(LayerModification::AddColors(ColorBitmask::RED));
    out.push(LayerModification::AddAbility(ability("x")));
    out.push(LayerModification::AddAbility(ability("y")));
    out.push(LayerModification::RemoveAllAbilities);
    out.push(LayerModification::RemoveKeyword("flying".into()));
    out.push(LayerModification::SetPt {
        power: Some(1),
        toughness: None,
    });
    out.push(LayerModification::SetPt {
        power: None,
        toughness: Some(1),
    });
    out.push(LayerModification::ModifyPt {
        power: 1,
        toughness: 0,
    });
    out.push(LayerModification::ModifyPt {
        power: 0,
        toughness: 1,
    });
    out.push(LayerModification::SwitchPt);
    out
}

fn copy_exceptions() -> Vec<CopyException> {
    let mut out = vec![
        CopyException::GainAbility(ability("g")),
        CopyException::SetNames(set(["N"])),
        CopyException::SetManaCost(None),
        CopyException::SetManaCost(Some("{1}".parse().unwrap())),
        CopyException::SetColors(ColorBitmask::RED),
        CopyException::SetSupertypes(set(["N"])),
        CopyException::SetCardTypes(set(["N"])),
        CopyException::SetSubtypes(set(["N"])),
        CopyException::AddSupertypes(set(["N"])),
        CopyException::AddCardTypes(set(["N"])),
        CopyException::AddSubtypes(set(["N"])),
        CopyException::SetPower(literal(1)),
        CopyException::SetToughness(literal(1)),
    ];
    for c in [
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
    ] {
        out.push(CopyException::NotCopied(c));
    }
    out
}

fn base_effect() -> ContinuousEffect {
    ContinuousEffect {
        source: Some(o(1)),
        controller: PlayerId::new(1),
        timestamp: Timestamp::new(5),
        layer: Layer::L6,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected: Affected::Objects([o(1)].into_iter().collect()),
        modification: LayerModification::RemoveAllAbilities,
    }
}

// REQ-NFR-006, REQ-CORE-006: every field of a ContinuousEffect, every variant and payload of
// its origin, affected set and modification, every layer, every copy exception and copied
// characteristic, every characteristics field and provenance flag reaches the encoding.
#[test]
fn every_field_and_variant_changes_the_encoding() {
    let base = base_effect();
    let one = |f: &dyn Fn(&mut ContinuousEffect)| {
        let mut c = base.clone();
        f(&mut c);
        c
    };
    let mut effects = vec![
        base.clone(),
        one(&|c| c.source = None),
        one(&|c| c.source = Some(o(2))),
        one(&|c| c.controller = PlayerId::new(2)),
        one(&|c| c.timestamp = Timestamp::new(6)),
        one(&|c| c.is_cda = true),
        one(&|c| {
            c.origin = EffectOrigin::Static {
                granted_at: Some(Timestamp::new(1)),
            }
        }),
        one(&|c| {
            c.origin = EffectOrigin::Static {
                granted_at: Some(Timestamp::new(2)),
            }
        }),
        one(&|c| c.origin = EffectOrigin::Resolved),
        one(&|c| c.affected = Affected::Objects(BTreeSet::new())),
        one(&|c| c.affected = Affected::Objects([o(1), o(2)].into_iter().collect())),
        one(&|c| c.affected = Affected::Selector(selector(PredicateAST::IsCreature))),
        one(&|c| c.affected = Affected::Selector(selector(PredicateAST::IsLand))),
    ];
    for layer in Layer::ALL {
        if *layer != Layer::L6 {
            effects.push(one(&|c| c.layer = *layer));
        }
        effects.push(one(&|c| c.affected = Affected::SameAsLayer(*layer)));
    }
    for m in modifications() {
        if m != LayerModification::RemoveAllAbilities {
            effects.push(one(&|c| c.modification = m.clone()));
        }
    }
    all_distinct(&effects);
    all_distinct(&copy_exceptions());
    all_distinct(&modifications());
}

// REQ-NFR-006: the documented one-byte tags.
#[test]
fn documented_tags() {
    let tags: Vec<u8> = Layer::ALL.iter().map(|l| layer_tag(*l)).collect();
    assert_eq!(tags, (0..=10).collect::<Vec<u8>>());
    assert_eq!(encode_to_vec(&EffectOrigin::Resolved), [1]);
    assert_eq!(
        encode_to_vec(&EffectOrigin::Static { granted_at: None }),
        [0, 0]
    );
    assert_eq!(encode_to_vec(&LayerModification::RemoveAllAbilities), [13]);
    assert_eq!(encode_to_vec(&LayerModification::SwitchPt), [17]);
    assert_eq!(encode_to_vec(&Affected::SameAsLayer(Layer::L7d)), [2, 10]);
    assert_eq!(encode_to_vec(&ValueSource::CopyOfBackFaceOrMelded), [2]);
    assert_eq!(
        encode_to_vec(&CopyException::NotCopied(
            CopiedCharacteristic::LifeModifier
        )),
        [12, 14]
    );
    let mut holder = vec![1];
    holder.extend_from_slice(&7_u64.to_le_bytes());
    assert_eq!(
        encode_to_vec(&CounterHolder::Player(PlayerId::new(7))),
        holder
    );
    holder[0] = 0;
    assert_eq!(encode_to_vec(&CounterHolder::Object(o(7))), holder);
    let modification = LayerModification::ReplaceTextWord {
        category: RewritableCategory::CardName,
        from: String::new(),
        to: String::new(),
    };
    assert_eq!(encode_to_vec(&modification), [6, 4, 0, 0, 0, 0, 0, 0, 0, 0]);
    // A DSL value: tag 0 then its RFC 8785 bytes.
    let mut want = vec![12, 0];
    put_bytes(
        &mut want,
        br#"{"abilityId":"a","keyword":"K","kind":"KEYWORD","params":{"a":1,"b":[true]}}"#,
    );
    let value: AbilityAST = serde_json::from_value(serde_json::json!({
        "params": {"b": [true], "a": 1}, "kind": "KEYWORD", "keyword": "K", "abilityId": "a"
    }))
    .unwrap();
    assert_eq!(encode_to_vec(&LayerModification::AddAbility(value)), want);
}

// REQ-NFR-006, REQ-CORE-002: the registry and ledger encodings: empty is put_len(0); the links,
// and each object, counter and sticker entry reach the bytes.
#[test]
fn registry_and_ledger_encodings() {
    assert_eq!(encode_to_vec(&ContinuousEffects::new()), [0, 0, 0, 0]);
    assert_eq!(encode_to_vec(&TimestampLedger::new()), [0, 0, 0, 0]);
    // A linked part encodes its anchor; the same effects unlinked encode differently.
    let anchor = ContinuousEffect {
        layer: Layer::L4,
        origin: EffectOrigin::Resolved,
        modification: LayerModification::RemoveSubtypes(BTreeSet::new()),
        ..base_effect()
    };
    let part = ContinuousEffect {
        layer: Layer::L7d,
        affected: Affected::SameAsLayer(Layer::L4),
        modification: LayerModification::SwitchPt,
        ..anchor.clone()
    };
    let mut grouped = ContinuousEffects::new();
    grouped
        .register_group(
            &NoObjects,
            vec![(EffectId::new(1), anchor), (EffectId::new(2), part)],
        )
        .unwrap();
    let mut orphan = grouped.clone();
    orphan.remove(EffectId::new(1));
    let mut two = orphan.clone();
    two.register(
        EffectId::new(1),
        grouped.get(EffectId::new(1)).unwrap().clone(),
    )
    .unwrap();
    assert_eq!(two.len(), grouped.len());
    assert_ne!(encode_to_vec(&two), encode_to_vec(&grouped));
    assert_eq!(&encode_to_vec(&grouped)[..4], [2, 0, 0, 0]);
    assert_eq!(super::encode::effect_links(&grouped).len(), 1);
    // The ledger: one entry of each kind, each distinct.
    let mut ids = IdGenerator::new();
    let mut objects = TimestampLedger::new();
    objects
        .stamp_object(&mut ids, o(1), StampReason::EntersZone)
        .unwrap();
    let mut ids = IdGenerator::new();
    let mut counters = TimestampLedger::new();
    counters
        .put_counters(&mut ids, CounterHolder::Object(o(1)), "k")
        .unwrap();
    let mut ids = IdGenerator::new();
    let mut stickers = TimestampLedger::new();
    stickers.put_sticker(&mut ids, o(1), 0).unwrap();
    let mut ids = IdGenerator::new();
    let mut player = TimestampLedger::new();
    player
        .put_counters(&mut ids, CounterHolder::Player(PlayerId::new(1)), "k")
        .unwrap();
    all_distinct(&[
        TimestampLedger::new(),
        objects.clone(),
        counters,
        stickers,
        player,
    ]);
    let mut want = vec![1, 0, 0, 0, 0];
    want.extend_from_slice(&1_u64.to_le_bytes());
    want.extend_from_slice(&1_u64.to_le_bytes());
    assert_eq!(encode_to_vec(&objects), want);
}

/// An environment with no objects.
struct NoObjects;

impl LayerEnvironment for NoObjects {
    fn objects(&self) -> Vec<ObjectId> {
        Vec::new()
    }

    fn base(&self, _: ObjectId) -> Option<ObjectBase> {
        None
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }
}

#[derive(Serialize)]
struct Big {
    n: u64,
}

struct Refuses;

impl Serialize for Refuses {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("refused"))
    }
}

// REQ-NFR-006, REQ-CORE-006: a DSL value without an RFC 8785 form (an integer beyond 2^53-1)
// is tag 1 and its structural encoding: deterministic, independent of member order, distinct
// for distinct values; a value with no JSON form at all is tag 2.
#[test]
fn dsl_value_fallback() {
    let enc = |v: serde_json::Value| {
        let mut out = Vec::new();
        put_json(&mut out, &v);
        out
    };
    let big = enc(serde_json::json!({"z": u64::MAX, "a": [1, "s", null, false, {"k": 1.5}]}));
    assert_eq!(big[0], 1);
    assert_eq!(
        big,
        enc(serde_json::json!({"a": [1, "s", null, false, {"k": 1.5}], "z": u64::MAX}))
    );
    assert_ne!(
        big,
        enc(serde_json::json!({"z": u64::MAX - 1, "a": [1, "s", null, false, {"k": 1.5}]}))
    );
    assert_ne!(
        big,
        enc(serde_json::json!({"z": u64::MAX, "a": [1, "s", null, true, {"k": 1.5}]}))
    );
    assert_eq!(enc(serde_json::json!(1))[0], 0);
    // Typed values: canonical JSON, the structural fallback, then tag 2.
    let mut out = Vec::new();
    put_typed(&mut out, &selector(PredicateAST::IsCreature));
    assert_eq!(out[0], 0);
    let mut out = Vec::new();
    put_typed(&mut out, &Big { n: u64::MAX });
    assert_eq!(out[0], 1);
    let mut other = Vec::new();
    put_typed(&mut other, &Big { n: u64::MAX - 1 });
    assert_ne!(out, other);
    let mut out = Vec::new();
    put_typed(&mut out, &Refuses);
    assert_eq!(out, [2]);
}

/// A synthetic neutral ability (a STATIC ability with one RULE `SKIP` effect) with ability id
/// `id` (R5; #232: replaces the former string placeholder abilities).
fn test_ability(id: &str) -> AbilityAST {
    serde_json::from_value(serde_json::json!({
        "abilityId": id,
        "kind": "STATIC",
        "effects": [{"type": "RULE", "kind": "SKIP"}],
    }))
    .expect("valid synthetic ability")
}

// ---------------------------------------------------------------------------------------------
// #232 golden bytes: typed DSL values encode exactly as the JSON wrappers did
// ---------------------------------------------------------------------------------------------

/// `v` decoded as `T` (valid synthetic content).
fn typed<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).unwrap()
}

/// Lower-case hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The golden abilities (synthetic, R5): a KEYWORD (with and without parameters), a STATIC and a
/// TRIGGERED ability with a `POPULATE` effect.
fn golden_abilities() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "keyword",
            serde_json::json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD"}),
        ),
        (
            "keyword_params",
            serde_json::json!({"abilityId": "a2", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
                "params": {"cost": "{2}"}}),
        ),
        (
            "static",
            serde_json::json!({"abilityId": "a3", "kind": "STATIC",
                "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        ),
        (
            "triggered",
            serde_json::json!({"abilityId": "a4", "kind": "TRIGGERED",
                "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}},
                "effect": [{"op": "POPULATE"}]}),
        ),
    ]
}

/// The golden token streams: empty, and one token of each kind (synthetic, R5).
fn golden_texts() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("text_empty", serde_json::json!([])),
        (
            "text_tokens",
            serde_json::json!([
                {"id": "t1", "kind": "NAME_REF", "ref": "SELF"},
                {"id": "t2", "kind": "LITERAL", "text": " gets +1/+1 for each "},
                {"id": "t3", "kind": "WORD", "category": "CREATURE_TYPE", "value": "TestType"},
                {"id": "t4", "kind": "WORD", "category": "NUMBER", "value": "2"},
                {"id": "t5", "kind": "NAME_REF", "ref": "OTHER", "name": "test.other"}
            ]),
        ),
    ]
}

/// The golden copy characteristics: every DSL position holds a valid typed value.
fn golden_copy_characteristics() -> Characteristics {
    Characteristics {
        names: set(["test.golden_creature"]),
        mana_cost: Some("{1}{G}".parse().unwrap()),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: set(["Testkin"]),
        rules_text: word_text("abc"),
        abilities: golden_abilities()
            .into_iter()
            .map(|(_, v)| typed(v))
            .collect(),
        power: Some(typed(serde_json::json!({"LITERAL": 2}))),
        toughness: Some(typed(
            serde_json::json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1}, "rhs": "X"}}),
        )),
        loyalty: None,
        defense: Some(4),
        hand_modifier: None,
        life_modifier: Some(-1),
    }
}

/// The golden face-down characteristics: the 2/2 ward {2} profile with typed values.
fn golden_face_down_characteristics() -> Characteristics {
    Characteristics {
        names: BTreeSet::new(),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities: vec![typed(serde_json::json!({"abilityId": "facedown:ward",
            "kind": "KEYWORD", "keyword": "WARD", "params": {"cost": "{2}"}}))],
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

// REQ-NFR-006, REQ-DSL-003, REQ-DSL-010, REQ-DSL-012 (#232): the state-hash leaf bytes of valid
// typed DSL values are exactly those the JSON wrappers produced before the migration (pinned
// from the base tree 242545c): value expressions, token streams, abilities of three kinds, and
// characteristics inside a copy and a face-down modification.
#[test]
fn typed_dsl_values_keep_their_golden_bytes() {
    let enc = |m: &dyn fineprint_base::encode::CanonicalEncode| {
        let mut out = Vec::new();
        m.encode_canonical(&mut out);
        hex(&out)
    };
    let literal_3 = CopyException::SetPower(typed(serde_json::json!({"LITERAL": 3})));
    assert_eq!(enc(&literal_3), GOLDEN_LITERAL);
    let math = CopyException::SetPower(typed(
        serde_json::json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1}, "rhs": "X"}}),
    ));
    assert_eq!(enc(&math), GOLDEN_MATH);
    let texts: Vec<String> = golden_texts()
        .into_iter()
        .map(|(_, v)| enc(&LayerModification::SetText(typed(v))))
        .collect();
    assert_eq!(texts, [GOLDEN_TEXT_EMPTY, GOLDEN_TEXT_TOKENS]);
    let abilities: Vec<String> = golden_abilities()
        .into_iter()
        .map(|(_, v)| enc(&LayerModification::AddAbility(typed(v))))
        .collect();
    assert_eq!(
        abilities,
        [
            GOLDEN_KEYWORD,
            GOLDEN_KEYWORD_PARAMS,
            GOLDEN_STATIC,
            GOLDEN_TRIGGERED
        ]
    );
    let copy = LayerModification::CopyValues(Box::new(golden_copy_characteristics()));
    assert_eq!(enc(&copy), GOLDEN_COPY);
    let face_down = LayerModification::FaceDown(Box::new(golden_face_down_characteristics()));
    assert_eq!(enc(&face_down), GOLDEN_FACE_DOWN);
    // The face-down ward {2} profile encodes as the golden face-down characteristics.
    let profile = crate::layouts::FaceDownProfile::with_ward_2();
    assert_eq!(
        enc(&LayerModification::FaceDown(Box::new(
            profile.characteristics().clone()
        ))),
        GOLDEN_FACE_DOWN
    );
}

// REQ-NFR-006 (#232): a typed value encodes exactly as `put_json` of its JSON tree, on the
// canonical path and on the structural fallback; a value with no JSON tree is tag 2.
#[test]
fn put_typed_matches_put_json_of_the_tree() {
    for (_, v) in golden_abilities().into_iter().chain(golden_texts()) {
        let mut tree = Vec::new();
        put_json(&mut tree, &v);
        let mut typed_bytes = Vec::new();
        put_typed(&mut typed_bytes, &v);
        assert_eq!(typed_bytes, tree);
    }
    let unsafe_number = serde_json::json!({"n": u64::MAX});
    let mut tree = Vec::new();
    put_json(&mut tree, &unsafe_number);
    let mut typed_bytes = Vec::new();
    put_typed(&mut typed_bytes, &unsafe_number);
    assert_eq!(tree[0], 1);
    assert_eq!(typed_bytes, tree);
}

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_LITERAL: &str = "0a000d0000007b224c49544552414c223a337d";

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_MATH: &str = concat!(
    "0a00330000007b224d415448223a7b226c6873223a7b224c49544552414c223a317d2c226f70223a22414444",
    "222c22726873223a2258227d7d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_TEXT_EMPTY: &str = "0500020000005b5d";

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_TEXT_TOKENS: &str = concat!(
    "05002a0100005b7b226964223a227431222c226b696e64223a224e414d455f524546222c22726566223a2253",
    "454c46227d2c7b226964223a227432222c226b696e64223a224c49544552414c222c2274657874223a222067",
    "657473202b312f2b3120666f72206561636820227d2c7b2263617465676f7279223a2243524541545552455f",
    "54595045222c226964223a227433222c226b696e64223a22574f5244222c2276616c7565223a225465737454",
    "797065227d2c7b2263617465676f7279223a224e554d424552222c226964223a227434222c226b696e64223a",
    "22574f5244222c2276616c7565223a2232227d2c7b226964223a227435222c226b696e64223a224e414d455f",
    "524546222c226e616d65223a22746573742e6f74686572222c22726566223a224f54484552227d5d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_KEYWORD: &str = concat!(
    "0c003c0000007b226162696c6974794964223a226131222c226b6579776f7264223a22544553545f4b455957",
    "4f5244222c226b696e64223a224b4559574f5244227d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_KEYWORD_PARAMS: &str = concat!(
    "0c00540000007b226162696c6974794964223a226132222c226b6579776f7264223a22544553545f4b455957",
    "4f5244222c226b696e64223a224b4559574f5244222c22706172616d73223a7b22636f7374223a227b327d22",
    "7d7d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_STATIC: &str = concat!(
    "0c004c0000007b226162696c6974794964223a226133222c2265666665637473223a5b7b226b696e64223a22",
    "534b4950222c2274797065223a2252554c45227d5d2c226b696e64223a22535441544943227d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_TRIGGERED: &str = concat!(
    "0c007c0000007b226162696c6974794964223a226134222c22656666656374223a5b7b226f70223a22504f50",
    "554c415445227d5d2c226b696e64223a22545249474745524544222c2274726967676572223a7b226d6f6465",
    "223a225354415445222c227374617465223a7b2270726564223a2249535f4352454154555245227d7d7d"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_COPY: &str = concat!(
    "000100000014000000746573742e676f6c64656e5f637265617475726501060000007b317d7b477d00100000",
    "0000010000000800000043726561747572650100000007000000546573746b696e002b0000005b7b22696422",
    "3a227431222c226b696e64223a224c49544552414c222c2274657874223a22616263227d5d04000000003c00",
    "00007b226162696c6974794964223a226131222c226b6579776f7264223a22544553545f4b4559574f524422",
    "2c226b696e64223a224b4559574f5244227d00540000007b226162696c6974794964223a226132222c226b65",
    "79776f7264223a22544553545f4b4559574f5244222c226b696e64223a224b4559574f5244222c2270617261",
    "6d73223a7b22636f7374223a227b327d227d7d004c0000007b226162696c6974794964223a226133222c2265",
    "666665637473223a5b7b226b696e64223a22534b4950222c2274797065223a2252554c45227d5d2c226b696e",
    "64223a22535441544943227d007c0000007b226162696c6974794964223a226134222c22656666656374223a",
    "5b7b226f70223a22504f50554c415445227d5d2c226b696e64223a22545249474745524544222c2274726967",
    "676572223a7b226d6f6465223a225354415445222c227374617465223a7b2270726564223a2249535f435245",
    "4154555245227d7d7d01000d0000007b224c49544552414c223a327d0100330000007b224d415448223a7b22",
    "6c6873223a7b224c49544552414c223a317d2c226f70223a22414444222c22726873223a2258227d7d000104",
    "000000000000000001ffffffffffffffff"
);

/// Golden bytes (hex) pinned from the base tree 242545c (#232).
const GOLDEN_FACE_DOWN: &str = concat!(
    "030000000000000000000000010000000800000043726561747572650000000000020000005b5d0100000000",
    "570000007b226162696c6974794964223a2266616365646f776e3a77617264222c226b6579776f7264223a22",
    "57415244222c226b696e64223a224b4559574f5244222c22706172616d73223a7b22636f7374223a227b327d",
    "227d7d01000d0000007b224c49544552414c223a327d01000d0000007b224c49544552414c223a327d000000",
    "00"
);
