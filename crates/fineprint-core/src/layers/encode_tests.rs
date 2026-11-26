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
    AbilityAST(format!("test.ability.{tag}").into())
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
        rules_text: TextTokenStream(Vec::new()),
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
        one(&|c| c.rules_text = TextTokenStream(vec!["w".into()])),
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
    out.push(LayerModification::SetText(TextTokenStream(Vec::new())));
    out.push(LayerModification::SetText(TextTokenStream(vec![
        "w".into(),
    ])));
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
    put_bytes(&mut want, br#"{"a":1,"b":[true,null]}"#);
    let value = AbilityAST(serde_json::json!({"b": [true, null], "a": 1}));
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
        put_json(&mut out, &AbilityAST(v));
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
