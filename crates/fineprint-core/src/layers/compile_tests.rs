// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the compile step from the DSL REPLACE_TEXT_WORD effect to
// LayerModification::ReplaceTextWord and of resolve_value_words: REQ-DSL-012 (SRS 14.5; CR 612.1,
// 612.2, 613.1c), REQ-LAYER-002 (ordering of compiled text effects; CR 613.8), REQ-DSL-013 (no
// card name or id read), REQ-CORE-001 (determinism, no panic on arbitrary input; R12). Synthetic
// objects only (R5): object names are `test.*`; word values are rules vocabulary.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::{CharacteristicEffect, Layer, Modification, PtValue};
use fineprint_dsl::card::{self, ColorBitmask};
use fineprint_dsl::expr::{
    ObjectSelectorAST, PlayerAttr, PredicateAST, ReferenceAST, SelectorFilter, ValueExpressionAST,
    WordRef,
};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream};
use serde_json::{Value, json};

use super::*;
use crate::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

const CW: RewritableCategory = RewritableCategory::ColorWord;
const BLT: RewritableCategory = RewritableCategory::BasicLandType;
const SUPPORTED: [RewritableCategory; 4] = [
    RewritableCategory::ColorWord,
    RewritableCategory::BasicLandType,
    RewritableCategory::CreatureType,
    RewritableCategory::Number,
];
const CARD_NAME_ERR: LayerError =
    LayerError::Unsupported("REPLACE_TEXT_WORD of category CARD_NAME (CR 612.2)");
const NOT_TEXT_ERR: LayerError = LayerError::Unsupported("not a text-changing modification");
const MALFORMED_ERR: LayerError =
    LayerError::Unsupported("REPLACE_TEXT_WORD with malformed members");
const LAYER_ERR: LayerError =
    LayerError::Unsupported("REPLACE_TEXT_WORD outside layer 3 (CR 613.1c)");

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn word(id: &str, category: RewritableCategory, value: &str) -> TextToken {
    TextToken::Word {
        id: id.to_owned(),
        category,
        value: value.to_owned(),
    }
}

fn lit(id: &str, text: &str) -> TextToken {
    TextToken::Literal {
        id: id.to_owned(),
        text: text.to_owned(),
    }
}

fn stream(tokens: Vec<TextToken>) -> TextTokenStream {
    TextTokenStream::new(tokens).unwrap()
}

fn tref(id: &str) -> WordRef {
    WordRef::TextToken(id.to_owned())
}

fn name(n: &str) -> WordRef {
    WordRef::Name(n.to_owned())
}

fn has_token_ref<T: std::fmt::Debug>(ast: &T) -> bool {
    format!("{ast:?}").contains("TextToken(")
}

/// A DSL CHAR effect in `layer` with `modification`, affecting the source.
fn char_effect(layer: Layer, modification: Modification) -> CharacteristicEffect {
    CharacteristicEffect {
        layer,
        affected: fineprint_dsl::ability::Affected::Selector(ObjectSelectorAST::Ref(
            ReferenceAST::SelfRef,
        )),
        modification,
        is_cda: None,
    }
}

/// `op` with the members of the JSON object `members` as params.
fn raw(op: &str, members: &Value) -> Modification {
    let mut m = Modification::new(op);
    m.params = members
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    m
}

fn effect(t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: PlayerId::new(1),
        timestamp: Timestamp::new(t),
        layer: Layer::L3,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

fn only(ids: &[u64]) -> Affected {
    Affected::Objects(ids.iter().map(|&n| o(n)).collect())
}

/// Registers each `(id, effect)` through the validating `register`.
fn registry(list: Vec<(u64, ContinuousEffect)>) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, eff) in list {
        r.register(e(id), eff).unwrap();
    }
    r
}

/// The compiled modification of a DSL `REPLACE_TEXT_WORD` built from its parts.
fn compiled(category: RewritableCategory, from: &str, to: &str) -> LayerModification {
    compile_text_modification(&Modification::replace_text_word(category, from, to)).unwrap()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic creature with `subtypes` and the given rules text.
fn object(name: &str, subtypes: &[&str], text: card::TextTokenStream) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: set(subtypes),
        rules_text: text,
        abilities: Vec::new(),
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

#[derive(Clone, Default)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
}

impl Env {
    fn with(mut self, id: ObjectId, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(PlayerId::new(1)),
                on_battlefield: true,
            },
        );
        self
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        matches!(selector, ObjectSelectorAST::Ref(ReferenceAST::SelfRef))
            && ctx.source == Some(ctx.candidate)
    }
}

fn text_of(r: &ContinuousEffects, env: &Env, id: ObjectId) -> TextTokenStream {
    evaluate(r, env, id)
        .unwrap()
        .characteristics
        .rules_text
        .clone()
}

fn value_of(s: &TextTokenStream, id: &str) -> String {
    resolve_word_ref(s, &tref(id)).unwrap()
}

// ---------------------------------------------------------------------------------------------
// compile_text_modification / compile_text_effect
// ---------------------------------------------------------------------------------------------

// REQ-DSL-012; CR 612.1, 613.1c: a well-formed REPLACE_TEXT_WORD of every supported category
// compiles into ReplaceTextWord with the same category and words, and registers in layer 3.
#[test]
fn req_dsl_012_well_formed_compiles_and_registers() {
    for category in SUPPORTED {
        let m = Modification::replace_text_word(category, "Swamp", "Island");
        let want = LayerModification::ReplaceTextWord {
            category,
            from: "Swamp".to_owned(),
            to: "Island".to_owned(),
        };
        assert_eq!(compile_text_modification(&m), Ok(want.clone()));
        assert_eq!(
            compile_text_effect(&char_effect(Layer::L3, m)),
            Ok(want.clone())
        );
        let mut r = ContinuousEffects::new();
        assert_eq!(r.register(e(1), effect(1, only(&[1]), want)), Ok(()));
    }
}

// REQ-DSL-012: the documented wire form, parsed by the DSL decoder, compiles.
#[test]
fn req_dsl_012_parsed_dsl_effect_compiles() {
    let c: CharacteristicEffect = serde_json::from_str(
        r#"{"type":"CHAR","layer":"3","affected":{"ref":"SELF"},"modification":{"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp","to":"Island"}}"#,
    )
    .unwrap();
    assert_eq!(
        compile_text_effect(&c),
        Ok(LayerModification::ReplaceTextWord {
            category: CW,
            from: "Swamp".to_owned(),
            to: "Island".to_owned(),
        })
    );
}

// REQ-DSL-012; CR 612.2: category CARD_NAME is refused with the registration error.
#[test]
fn req_dsl_012_card_name_is_rejected_cr_612_2() {
    let m = Modification::replace_text_word(RewritableCategory::CardName, "Swamp", "Island");
    assert_eq!(compile_text_modification(&m), Err(CARD_NAME_ERR));
    assert_eq!(
        compile_text_effect(&char_effect(Layer::L3, m)),
        Err(CARD_NAME_ERR)
    );
    // The same error as registering the raw modification; CARD_NAME is checked before words.
    let raw = LayerModification::ReplaceTextWord {
        category: RewritableCategory::CardName,
        from: String::new(),
        to: "Island".to_owned(),
    };
    let mut r = ContinuousEffects::new();
    assert_eq!(
        r.register(e(1), effect(1, only(&[1]), raw)),
        Err(CARD_NAME_ERR)
    );
    let m = Modification::replace_text_word(RewritableCategory::CardName, "", "Island");
    assert_eq!(compile_text_modification(&m), Err(CARD_NAME_ERR));
}

// REQ-DSL-012; CR 612.2: an empty or noncharacter word is refused with exactly the error
// registration gives for the same modification.
#[test]
fn req_dsl_012_invalid_words_match_registration_error() {
    for category in SUPPORTED {
        for (from, to) in [
            ("", "Island"),
            ("Swamp", ""),
            ("Sw\u{fdd0}amp", "Island"),
            ("Swamp", "Isl\u{fffe}and"),
        ] {
            let m = Modification::replace_text_word(category, from, to);
            let got = compile_text_modification(&m);
            let mut r = ContinuousEffects::new();
            let reg = r.register(
                e(1),
                effect(
                    1,
                    only(&[1]),
                    LayerModification::ReplaceTextWord {
                        category,
                        from: from.to_owned(),
                        to: to.to_owned(),
                    },
                ),
            );
            assert!(got.is_err(), "{category:?} {from:?} {to:?}");
            assert_eq!(got.map(|_| ()), reg, "{category:?} {from:?} {to:?}");
            assert_eq!(
                compile_text_effect(&char_effect(Layer::L3, m)).map(|_| ()),
                reg
            );
        }
    }
}

// REQ-DSL-012: from == to compiles; it is an ordinary replacement that changes nothing.
#[test]
fn req_dsl_012_same_word_compiles_as_no_op() {
    let m = compiled(BLT, "Swamp", "Swamp");
    let text = stream(vec![word("t1", BLT, "Swamp")]);
    let env = Env::default().with(o(1), object("test.bear_1", &[], text.clone()));
    let r = registry(vec![(1, effect(1, only(&[1]), m))]);
    assert_eq!(text_of(&r, &env, o(1)), text);
}

// REQ-DSL-012: malformed members (unknown or missing category, missing or non-string words,
// extra members, typed power) are Unsupported, never a panic.
#[test]
fn req_dsl_012_malformed_members_are_unsupported() {
    let cases = [
        json!({"category": "TEST_CATEGORY", "from": "a", "to": "b"}),
        json!({"category": "color_word", "from": "a", "to": "b"}),
        json!({"from": "a", "to": "b"}),
        json!({"category": 3, "from": "a", "to": "b"}),
        json!({"category": "COLOR_WORD", "to": "b"}),
        json!({"category": "COLOR_WORD", "from": "a"}),
        json!({"category": "COLOR_WORD", "from": 1, "to": "b"}),
        json!({"category": "COLOR_WORD", "from": "a", "to": null}),
        json!({"category": "COLOR_WORD", "from": "a", "to": "b", "extra": true}),
    ];
    for members in &cases {
        let m = raw(Modification::REPLACE_TEXT_WORD, members);
        assert_eq!(
            compile_text_modification(&m),
            Err(MALFORMED_ERR),
            "{members}"
        );
        assert_eq!(
            compile_text_effect(&char_effect(Layer::L3, m)),
            Err(MALFORMED_ERR)
        );
    }
    let mut m = Modification::replace_text_word(CW, "a", "b");
    m.power = Some(PtValue::Int(1));
    assert_eq!(compile_text_modification(&m), Err(MALFORMED_ERR));
}

// REQ-DSL-012: any other op is not a text-changing modification, in any layer.
#[test]
fn req_dsl_012_other_ops_are_unsupported() {
    let mut pt = Modification::new("SET_BASE_PT");
    pt.power = Some(PtValue::Int(1));
    pt.toughness = Some(PtValue::Int(1));
    for m in [
        Modification::new("SET_TEXT"),
        Modification::new("TEST_OP"),
        raw(
            "TEST_OP",
            &json!({"category": "COLOR_WORD", "from": "a", "to": "b"}),
        ),
        pt,
    ] {
        assert_eq!(compile_text_modification(&m), Err(NOT_TEXT_ERR));
        for &layer in Layer::ALL {
            assert_eq!(
                compile_text_effect(&char_effect(layer, m.clone())),
                Err(NOT_TEXT_ERR)
            );
        }
    }
}

// REQ-DSL-012; CR 613.1c: a REPLACE_TEXT_WORD effect declared in any layer but 3 is refused;
// errors of the modification are reported first.
#[test]
fn req_dsl_012_non_layer_3_is_unsupported_cr_613_1c() {
    for &layer in Layer::ALL {
        let ok = compile_text_effect(&char_effect(
            layer,
            Modification::replace_text_word(CW, "Swamp", "Island"),
        ));
        if layer == Layer::L3 {
            assert!(ok.is_ok());
        } else {
            assert_eq!(ok, Err(LAYER_ERR), "{layer}");
            let card = Modification::replace_text_word(RewritableCategory::CardName, "a", "b");
            assert_eq!(
                compile_text_effect(&char_effect(layer, card)),
                Err(CARD_NAME_ERR)
            );
        }
    }
}

// REQ-DSL-012, REQ-DSL-013, REQ-CORE-001; R12: bounded random modifications and effects never
// panic; every Ok result is a ReplaceTextWord that registration accepts in layer 3, equal across
// both entry points and repeated calls.
#[test]
fn req_dsl_012_random_inputs_never_panic_and_ok_registers() {
    const KEYS: [&str; 5] = ["category", "from", "to", "extra", "power"];
    const WORDS: [&str; 7] = [
        "Swamp",
        "Island",
        "",
        "\u{fdd0}",
        "2",
        "test.word",
        "\u{e9}",
    ];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f15_0195_0002);
    let mut pick = |n: usize| usize::try_from(rng.next_bounded(n as u64).unwrap()).unwrap();
    let mut oks = 0;
    for i in 0..4000_u64 {
        let op = [Modification::REPLACE_TEXT_WORD, "TEST_OP", "SET_TEXT"][pick(4) % 3];
        let mut m = Modification::new(op);
        for _ in 0..pick(5) {
            let key = KEYS[pick(KEYS.len())];
            let value = match pick(5) {
                0 => Value::Null,
                1 => json!(pick(9)),
                2 => {
                    let c = RewritableCategory::ALL[pick(RewritableCategory::ALL.len())];
                    json!(c.as_str())
                }
                _ => json!(WORDS[pick(WORDS.len())]),
            };
            m.params.insert(key.to_owned(), value);
        }
        let layer = Layer::ALL[pick(Layer::ALL.len())];
        let got = compile_text_modification(&m);
        assert_eq!(got, compile_text_modification(&m));
        let via_effect = compile_text_effect(&char_effect(layer, m.clone()));
        match &got {
            Ok(lm) => {
                oks += 1;
                assert!(matches!(lm, LayerModification::ReplaceTextWord { .. }));
                let mut r = ContinuousEffects::new();
                assert_eq!(
                    r.register(e(i + 1), effect(1, only(&[1]), lm.clone())),
                    Ok(())
                );
                if layer == Layer::L3 {
                    assert_eq!(via_effect, got);
                } else {
                    assert_eq!(via_effect, Err(LAYER_ERR));
                }
            }
            Err(err) => assert_eq!(via_effect.as_ref(), Err(err)),
        }
    }
    assert!(oks > 0, "the generator reaches well-formed modifications");
}

// ---------------------------------------------------------------------------------------------
// End to end: a compiled text-changing effect changes what a dependent ability applies to
// ---------------------------------------------------------------------------------------------

/// The synthetic card's rules text: a COLOR_WORD "Swamp" word (t2) read by a dependent ability,
/// and a BASIC_LAND_TYPE "Swamp" word (t3) of another category.
fn card_text() -> TextTokenStream {
    stream(vec![
        lit("t1", "Destroy target "),
        word("t2", CW, "Swamp"),
        word("t3", BLT, "Swamp"),
        TextToken::NameRef {
            id: "t4".to_owned(),
            kind: NameRefKind::SelfRef,
            name: None,
        },
    ])
}

/// The synthetic card (1) and two other creatures with subtype Swamp (2) and Island (3).
fn board() -> Env {
    Env::default()
        .with(o(1), object("test.text_changer_1", &[], card_text()))
        .with(
            o(2),
            object(
                "test.swamp_creature_2",
                &["Swamp"],
                card::TextTokenStream::default(),
            ),
        )
        .with(
            o(3),
            object(
                "test.island_creature_3",
                &["Island"],
                card::TextTokenStream::default(),
            ),
        )
}

/// The registry with the DSL effect, compiled, on the card (none when `with_effect` is false).
fn card_registry(with_effect: bool) -> ContinuousEffects {
    if !with_effect {
        return ContinuousEffects::new();
    }
    let dsl: CharacteristicEffect = serde_json::from_value(json!({
        "type": "CHAR",
        "layer": "3",
        "affected": {"ref": "SELF"},
        "modification": {"op": "REPLACE_TEXT_WORD", "category": "COLOR_WORD", "from": "Swamp",
                         "to": "Island"},
    }))
    .unwrap();
    let m = compile_text_effect(&dsl).unwrap();
    registry(vec![(1, effect(1, only(&[1]), m))])
}

/// Which objects of `env` (evaluated with `r`) a resolved `HAS_SUBTYPE`/`HAS_COLOR` predicate
/// over names picks.
fn picks(pred: &PredicateAST, r: &ContinuousEffects, env: &Env) -> Vec<u64> {
    let all = evaluate_all(r, env).unwrap();
    all.iter()
        .filter(|(_, ev)| match pred {
            PredicateAST::HasSubtype {
                value: WordRef::Name(t),
            } => ev.characteristics.subtypes.contains(t),
            PredicateAST::HasColor {
                value: WordRef::Name(_),
            } => true,
            _ => false,
        })
        .map(|(id, _)| id.get())
        .collect()
}

// REQ-DSL-012 (acceptance), REQ-LAYER-001; CR 612.1, 612.2, 613.1c: a synthetic card's
// text-changing effect, compiled from the DSL and registered, changes the evaluated word t2
// from Swamp to Island, so a dependent ability's HAS_SUBTYPE / HAS_COLOR word reference reads
// Island and applies to the Island creature instead of the Swamp creature. Without the effect
// it reads Swamp. The other-category word t3 is never rewritten (CR 612.2).
#[test]
fn req_dsl_012_compiled_effect_changes_dependent_ability() {
    let env = board();
    let subtype = PredicateAST::HasSubtype { value: tref("t2") };
    let color = PredicateAST::HasColor { value: tref("t2") };
    for (with_effect, expected, applies_to) in [(true, "Island", 3), (false, "Swamp", 2)] {
        let r = card_registry(with_effect);
        let text = text_of(&r, &env, o(1));
        assert_eq!(value_of(&text, "t2"), expected);
        assert_eq!(value_of(&text, "t3"), "Swamp");
        let resolved = resolve_predicate_words(&subtype, &text).unwrap();
        assert_eq!(
            resolved,
            PredicateAST::HasSubtype {
                value: name(expected)
            }
        );
        assert_eq!(picks(&resolved, &r, &env), vec![applies_to]);
        assert_eq!(
            resolve_predicate_words(&color, &text).unwrap(),
            PredicateAST::HasColor {
                value: name(expected)
            }
        );
        // The base text is never mutated.
        assert_eq!(env.bases[&o(1)].characteristics.rules_text, card_text());
    }
}

// REQ-DSL-012 (gap G4); CR 612.1: a word reference reachable only from a value expression
// (a count's selector predicate, a devotion colour, arithmetic over them) resolves through
// resolve_value_words against the evaluated text: Island with the compiled effect, Swamp
// without; the result holds no TextToken reference.
#[test]
fn req_dsl_012_value_expression_words_follow_compiled_effect() {
    let env = board();
    let count = |w: WordRef| {
        Box::new(ValueExpressionAST::Count(Box::new(
            ObjectSelectorAST::Filter(SelectorFilter {
                zones: None,
                count: None,
                ordering: None,
                predicate: Some(Box::new(PredicateAST::HasSubtype { value: w })),
            }),
        )))
    };
    let devotion = |w: WordRef| {
        Box::new(ValueExpressionAST::PlayerAttr {
            reference: ReferenceAST::Controller,
            attr: PlayerAttr::Devotion(vec![w, name("test.fixed")]),
        })
    };
    let expr = |a: WordRef, b: WordRef| ValueExpressionAST::Math {
        op: fineprint_dsl::expr::MathOp::Add,
        lhs: count(a),
        rhs: devotion(b),
    };
    let v = expr(tref("t2"), tref("t2"));
    for (with_effect, expected) in [(true, "Island"), (false, "Swamp")] {
        let text = text_of(&card_registry(with_effect), &env, o(1));
        let resolved = resolve_value_words(&v, &text).unwrap();
        assert!(!has_token_ref(&resolved));
        assert_eq!(resolved, expr(name(expected), name(expected)));
    }
}

// REQ-DSL-012: resolve_value_words reports the first unresolvable token (unknown id, or a
// token that is not a word) and leaves literals and names unchanged.
#[test]
fn req_dsl_012_resolve_value_words_errors_and_identity() {
    let text = card_text();
    let devotion = |w: WordRef| ValueExpressionAST::PlayerAttr {
        reference: ReferenceAST::Controller,
        attr: PlayerAttr::Devotion(vec![w]),
    };
    assert_eq!(
        resolve_value_words(&devotion(tref("t9")), &text),
        Err(TextWordError::UnknownToken("t9".to_owned()))
    );
    assert_eq!(
        resolve_value_words(&devotion(tref("t1")), &text),
        Err(TextWordError::NotAWord("t1".to_owned()))
    );
    assert_eq!(
        resolve_value_words(&devotion(tref("t4")), &text),
        Err(TextWordError::NotAWord("t4".to_owned()))
    );
    for v in [ValueExpressionAST::Literal(3), devotion(name("Swamp"))] {
        assert_eq!(resolve_value_words(&v, &text), Ok(v.clone()));
    }
}

// ---------------------------------------------------------------------------------------------
// Ordering of compiled effects (both timestamp orders)
// ---------------------------------------------------------------------------------------------

fn single(tokens: Vec<TextToken>) -> Env {
    Env::default().with(o(1), object("test.bear_1", &[], stream(tokens)))
}

// REQ-DSL-012, REQ-LAYER-002; CR 613.8: compiled Swamp->Island and Island->Forest on the same
// text give the #37 result in both timestamp orders (the Swamp word ends as Forest), exactly as
// the hand-built modifications do.
#[test]
fn req_dsl_012_compiled_chain_in_both_orders_cr_613_8() {
    let env = single(vec![word("w", BLT, "Swamp"), word("x", BLT, "Plains")]);
    for (ta, tb) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (1, effect(ta, only(&[1]), compiled(BLT, "Swamp", "Island"))),
            (2, effect(tb, only(&[1]), compiled(BLT, "Island", "Forest"))),
        ]);
        let text = text_of(&r, &env, o(1));
        assert_eq!(value_of(&text, "w"), "Forest", "A at {ta}");
        assert_eq!(value_of(&text, "x"), "Plains");
        let hand = registry(vec![
            (
                1,
                effect(
                    ta,
                    only(&[1]),
                    LayerModification::ReplaceTextWord {
                        category: BLT,
                        from: "Swamp".to_owned(),
                        to: "Island".to_owned(),
                    },
                ),
            ),
            (
                2,
                effect(
                    tb,
                    only(&[1]),
                    LayerModification::ReplaceTextWord {
                        category: BLT,
                        from: "Island".to_owned(),
                        to: "Forest".to_owned(),
                    },
                ),
            ),
        ]);
        assert_eq!(text_of(&hand, &env, o(1)), text);
    }
}

// REQ-DSL-012, REQ-LAYER-002; CR 613.8, 613.1c: a compiled replacement with a SetText on the
// same object: the SetText applies first in both timestamp orders and the replacement rewrites
// its text (DECISIONS "#37 FP-036 — Dependency between text effects").
#[test]
fn req_dsl_012_compiled_replace_after_set_text_in_both_orders_cr_613_8() {
    let env = single(vec![word("x1", BLT, "Plains")]);
    let new_text = stream(vec![lit("l", "n1"), word("w", BLT, "Swamp")]);
    let want = stream(vec![lit("l", "n1"), word("w", BLT, "Island")]);
    for (t_set, t_rep) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (
                1,
                effect(
                    t_set,
                    only(&[1]),
                    LayerModification::SetText(new_text.clone()),
                ),
            ),
            (
                2,
                effect(t_rep, only(&[1]), compiled(BLT, "Swamp", "Island")),
            ),
        ]);
        assert_eq!(
            text_of(&r, &env, o(1)),
            want,
            "SetText at {t_set}, replace at {t_rep}"
        );
    }
}
