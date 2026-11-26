// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the compile step from the DSL layer-3 `REPLACE_TEXT_WORD` effect to
//! `LayerModification::ReplaceTextWord` (REQ-DSL-012, REQ-LAYER-002, REQ-DSL-013,
//! REQ-CORE-001; CR 612.2, 612.3, 613.1c, 613.8): `compile_text_modification`,
//! `compile_text_effect` and `resolve_value_words` of `fineprint_core::layers`.
//!
//! Covered: compile success per supported category and `from == to`; rejections (CARD_NAME per
//! CR 612.2, empty or noncharacter words, malformed members, other ops, layers other than 3);
//! registration and application in layer 3 only; the acceptance scenario "a synthetic card's
//! text-changing effect, compiled from the DSL, changes what a dependent ability applies to";
//! value-expression word resolution; both timestamp orders for two compiled replacements and for
//! a compiled replacement with a whole-text `SetText` (CR 613.8); determinism and cache
//! agreement; a bounded random run (R12); granted abilities untouched (CR 612.3).
//!
//! Black-box: written from the #195 (FP-150) brief only. Synthetic objects named `test.*`; word
//! values are rules vocabulary only (R5).

#[path = "spec_layers_support.rs"]
mod support;

use std::rc::Rc;

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, LayerCache, LayerError, LayerModification,
    MatchContext, OrderItem, OrderRecord, TextWordError, compile_text_effect,
    compile_text_modification, decode_rules_text, encode_rules_text, evaluate, evaluate_all,
    evaluate_with_order, evaluated_digest, resolve_predicate_words, resolve_selector_words,
    resolve_value_words,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::{self as dsl, CharacteristicEffect, Layer, Modification};
use fineprint_dsl::card;
use fineprint_dsl::expr::{
    Cmp, HistoryQuery, MathOp, ObjectSelectorAST, PlayerAttr, PredicateAST, ReferenceAST,
    SelectorFilter, ValueExpressionAST, WordRef, Zone,
};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream as Stream};
use serde_json::{Map, Value, json};
use support::{
    Lcg, SEL_ALL, TestEnv, by_sel, creature, eff, eid, enchantment, kw_ability, modify_pt, objs,
    oid, pt, registry, set, spt, static_ability, static_eff,
};

use RewritableCategory::{BasicLandType, CardName, ColorWord, CreatureType, Number};

// ---------------------------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------------------------

const SUPPORTED: [(RewritableCategory, &str); 4] = [
    (ColorWord, "COLOR_WORD"),
    (BasicLandType, "BASIC_LAND_TYPE"),
    (CreatureType, "CREATURE_TYPE"),
    (Number, "NUMBER"),
];

const NOT_L3: [Layer; 10] = [
    Layer::L1a,
    Layer::L1b,
    Layer::L2,
    Layer::L4,
    Layer::L5,
    Layer::L6,
    Layer::L7a,
    Layer::L7b,
    Layer::L7c,
    Layer::L7d,
];

fn lit_t(id: &str, text: &str) -> TextToken {
    TextToken::Literal {
        id: id.into(),
        text: text.into(),
    }
}

fn word(id: &str, category: RewritableCategory, value: &str) -> TextToken {
    TextToken::Word {
        id: id.into(),
        category,
        value: value.into(),
    }
}

fn self_ref(id: &str) -> TextToken {
    TextToken::NameRef {
        id: id.into(),
        kind: NameRefKind::SelfRef,
        name: None,
    }
}

fn stream(tokens: Vec<TextToken>) -> Stream {
    Stream::new(tokens).expect("valid stream")
}

fn enc(tokens: Vec<TextToken>) -> card::TextTokenStream {
    encode_rules_text(&stream(tokens))
}

fn dec(t: &card::TextTokenStream) -> Stream {
    decode_rules_text(t).expect("evaluated rules text decodes")
}

fn with_text(name: &str, tokens: Vec<TextToken>) -> Characteristics {
    Characteristics {
        rules_text: enc(tokens),
        ..enchantment(name)
    }
}

fn token_value(s: &Stream, id: &str) -> String {
    match s.find(id).expect("token present") {
        TextToken::Word { value, .. } => value.clone(),
        other => panic!("not a word: {other:?}"),
    }
}

fn tt(id: &str) -> WordRef {
    WordRef::TextToken(id.into())
}

fn nm(n: &str) -> WordRef {
    WordRef::Name(n.into())
}

/// The DSL wire form of a REPLACE_TEXT_WORD modification.
fn rtw_json(cat: &str, from: &str, to: &str) -> Value {
    json!({"op":"REPLACE_TEXT_WORD","category":cat,"from":from,"to":to})
}

/// A DSL CHAR effect in `layer` with the given modification, parsed from JSON (the path a card
/// definition takes).
fn dsl_effect(layer: &str, modification: &Value) -> CharacteristicEffect {
    let doc = json!({"type":"CHAR","layer":layer,
        "affected":{"zones":["BATTLEFIELD"],"pred":"IS_PERMANENT"},"modification":modification});
    serde_json::from_value(doc).expect("CHAR effect parses")
}

/// A DSL CharacteristicEffect built directly (no parser), so any modification can be used.
fn dsl_effect_raw(layer: Layer, m: Modification) -> CharacteristicEffect {
    CharacteristicEffect {
        layer,
        affected: dsl::Affected::Selector(ObjectSelectorAST::Ref(ReferenceAST::SelfRef)),
        modification: m,
        is_cda: None,
    }
}

/// Compiles the DSL layer-3 `REPLACE_TEXT_WORD` from its wire form (REQ-DSL-012).
fn compiled(cat: &str, from: &str, to: &str) -> LayerModification {
    compile_text_effect(&dsl_effect("3", &rtw_json(cat, from, to))).expect("compiles")
}

/// A modification with the given op and raw params, built without the parser.
fn raw(op: &str, params: &[(&str, Value)]) -> Modification {
    let mut m = Modification::new(op);
    for (k, v) in params {
        m.params.insert((*k).to_string(), v.clone());
    }
    m
}

fn is_unsupported(r: &Result<LayerModification, LayerError>) -> bool {
    matches!(r, Err(LayerError::Unsupported(_)))
}

/// A resolved layer-3 effect on a locked set carrying `m`.
fn l3_eff(t: u64, ids: &[ObjectId], m: LayerModification) -> ContinuousEffect {
    eff(Layer::L3, t, objs(ids), m)
}

fn eval_text(r: &ContinuousEffects, env: &TestEnv, id: ObjectId) -> Stream {
    dec(&evaluate(r, env, id)
        .expect("evaluate")
        .characteristics
        .rules_text)
}

fn l3_steps(rec: &OrderRecord) -> Vec<OrderItem> {
    rec.steps
        .iter()
        .filter(|s| s.layer == Layer::L3)
        .map(|s| s.item.clone())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// walkers: is any WordRef::TextToken left?
// ---------------------------------------------------------------------------------------------

fn word_is_token(w: &WordRef) -> bool {
    matches!(w, WordRef::TextToken(_))
}

fn pred_has_token(p: &PredicateAST) -> bool {
    match p {
        PredicateAST::And(v) | PredicateAST::Or(v) => v.iter().any(pred_has_token),
        PredicateAST::Not(b) => pred_has_token(b),
        PredicateAST::HasCardType { value }
        | PredicateAST::HasSubtype { value }
        | PredicateAST::HasSupertype { value }
        | PredicateAST::HasColor { value } => word_is_token(value),
        PredicateAST::HasCounter { compare, .. } => {
            compare.as_ref().is_some_and(|(_, v)| value_has_token(v))
        }
        PredicateAST::ManaValue { value, .. }
        | PredicateAST::Power { value, .. }
        | PredicateAST::Toughness { value, .. } => value_has_token(value),
        PredicateAST::Controls { selector, .. } => sel_has_token(selector),
        PredicateAST::Compare { lhs, rhs, .. } => value_has_token(lhs) || value_has_token(rhs),
        _ => false,
    }
}

fn sel_has_token(s: &ObjectSelectorAST) -> bool {
    match s {
        ObjectSelectorAST::Ref(_) => false,
        ObjectSelectorAST::Filter(f) => {
            f.predicate.as_deref().is_some_and(pred_has_token)
                || f.count.as_deref().is_some_and(value_has_token)
        }
    }
}

fn history_has_token(h: &HistoryQuery) -> bool {
    match h {
        HistoryQuery::SpellsCastThisTurn { condition, .. }
        | HistoryQuery::CreaturesDiedThisTurn { condition } => {
            condition.as_ref().is_some_and(pred_has_token)
        }
        HistoryQuery::ManaSpentToCast { color, .. } => color.as_ref().is_some_and(word_is_token),
        _ => false,
    }
}

fn value_has_token(v: &ValueExpressionAST) -> bool {
    match v {
        ValueExpressionAST::PlayerAttr {
            attr: PlayerAttr::Devotion(ws),
            ..
        } => ws.iter().any(word_is_token),
        ValueExpressionAST::Count(s) => sel_has_token(s),
        ValueExpressionAST::Distinct { selector, .. } => sel_has_token(selector),
        ValueExpressionAST::Math { lhs, rhs, .. } => value_has_token(lhs) || value_has_token(rhs),
        ValueExpressionAST::History(h) => history_has_token(h),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// 6. compile success
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 613.1c: COLOR_WORD, BASIC_LAND_TYPE, CREATURE_TYPE and NUMBER (value is a
/// string) compile to exactly `LayerModification::ReplaceTextWord` with the same category, from
/// and to, through both entry points (the parsed layer-3 CHAR effect and the bare modification).
#[test]
fn req_dsl_012_compile_supported_categories() {
    for (cat, wire) in SUPPORTED {
        let (from, to) = if cat == Number {
            ("3", "4")
        } else {
            ("Swamp", "Island")
        };
        let want = LayerModification::ReplaceTextWord {
            category: cat,
            from: from.into(),
            to: to.into(),
        };
        let m = Modification::replace_text_word(cat, from, to);
        assert_eq!(compile_text_modification(&m), Ok(want.clone()), "{wire}");
        let parsed: Modification = serde_json::from_value(rtw_json(wire, from, to)).expect("parse");
        assert_eq!(
            compile_text_modification(&parsed),
            Ok(want.clone()),
            "{wire}"
        );
        assert_eq!(
            compile_text_effect(&dsl_effect("3", &rtw_json(wire, from, to))),
            Ok(want.clone()),
            "{wire}"
        );
        assert_eq!(
            compile_text_effect(&dsl_effect_raw(Layer::L3, m)),
            Ok(want),
            "{wire}"
        );
    }
}

/// REQ-DSL-012 (DECISIONS "#37 FP-036 — Word replacement scope"): `from == to` compiles, and a
/// cross-category `to` value is not checked.
#[test]
fn req_dsl_012_compile_from_equals_to_and_no_category_check() {
    for (cat, wire) in SUPPORTED {
        assert_eq!(
            compiled(wire, "Red", "Red"),
            LayerModification::ReplaceTextWord {
                category: cat,
                from: "Red".into(),
                to: "Red".into(),
            },
            "{wire}"
        );
        assert_eq!(
            compiled(wire, "Swamp", "7"),
            LayerModification::ReplaceTextWord {
                category: cat,
                from: "Swamp".into(),
                to: "7".into(),
            },
            "{wire}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 7. rejections
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 612.2: a CARD_NAME replacement is rejected at compile with
/// `LayerError::Unsupported` whose message mentions CARD_NAME, through both entry points, for
/// any from/to (including from == to and otherwise-invalid words).
#[test]
fn req_dsl_012_compile_rejects_card_name() {
    for (from, to) in [
        ("test.name_a", "test.name_b"),
        ("test.name_a", "test.name_a"),
        ("Swamp", "Island"),
    ] {
        let m = Modification::replace_text_word(CardName, from, to);
        for r in [
            compile_text_modification(&m),
            compile_text_effect(&dsl_effect_raw(Layer::L3, m.clone())),
            compile_text_effect(&dsl_effect("3", &rtw_json("CARD_NAME", from, to))),
        ] {
            match r {
                Err(LayerError::Unsupported(msg)) => {
                    assert!(
                        msg.contains("CARD_NAME"),
                        "message {msg:?} mentions CARD_NAME"
                    );
                }
                other => panic!("CARD_NAME {from:?}->{to:?}: expected Unsupported, got {other:?}"),
            }
        }
    }
    // Rejected whatever the from/to validity (still Unsupported, CR 612.2).
    for (from, to) in [("", ""), ("\u{FFFF}", "x")] {
        let m = Modification::replace_text_word(CardName, from, to);
        assert!(is_unsupported(&compile_text_modification(&m)));
    }
}

/// REQ-DSL-012: an empty or Unicode-noncharacter `from` or `to` is `LayerError::Unsupported`
/// for every supported category, through both entry points.
#[test]
fn req_dsl_012_compile_rejects_invalid_words() {
    for (cat, wire) in SUPPORTED {
        for (from, to) in [
            ("", "Island"),
            ("Swamp", ""),
            ("", ""),
            ("\u{FFFF}", "Island"),
            ("Swamp", "\u{FFFF}"),
            ("Sw\u{FFFF}amp", "Island"),
            ("Swamp", "Isl\u{FFFF}and"),
            ("\u{FDD0}", "Island"),
            ("Swamp", "\u{FFFE}"),
        ] {
            let m = Modification::replace_text_word(cat, from, to);
            let a = compile_text_modification(&m);
            assert!(is_unsupported(&a), "{wire} {from:?}->{to:?}: {a:?}");
            let b = compile_text_effect(&dsl_effect_raw(Layer::L3, m));
            assert!(is_unsupported(&b), "{wire} {from:?}->{to:?}: {b:?}");
        }
    }
}

/// REQ-DSL-012 (R12): every malformed member set (missing, wrong JSON type, unknown category,
/// extra unknown param, JSON null) is `LayerError::Unsupported`, never a panic.
#[test]
fn req_dsl_012_compile_rejects_malformed_members() {
    let good = [
        ("category", json!("COLOR_WORD")),
        ("from", json!("Swamp")),
        ("to", json!("Island")),
    ];
    let mut cases: Vec<(String, Modification)> = Vec::new();
    for k in ["category", "from", "to"] {
        let without: Vec<(&str, Value)> = good.iter().filter(|(n, _)| *n != k).cloned().collect();
        cases.push((format!("missing {k}"), raw("REPLACE_TEXT_WORD", &without)));
        for bad in [
            json!(1),
            json!(true),
            json!(["x"]),
            json!({"name":"x"}),
            Value::Null,
        ] {
            let params: Vec<(&str, Value)> = good
                .iter()
                .map(|(n, v)| (*n, if *n == k { bad.clone() } else { v.clone() }))
                .collect();
            cases.push((format!("{k} = {bad}"), raw("REPLACE_TEXT_WORD", &params)));
        }
    }
    for cat in ["TEST_CATEGORY", "color_word", "", "CARD NAME"] {
        let params = [
            ("category", json!(cat)),
            ("from", json!("Swamp")),
            ("to", json!("Island")),
        ];
        cases.push((
            format!("category {cat:?}"),
            raw("REPLACE_TEXT_WORD", &params),
        ));
    }
    for extra in ["extra", "value", "layer"] {
        let mut params = good.to_vec();
        params.push((extra, json!("x")));
        cases.push((format!("extra {extra}"), raw("REPLACE_TEXT_WORD", &params)));
    }
    cases.push(("no params".into(), raw("REPLACE_TEXT_WORD", &[])));
    for (what, m) in cases {
        let a = compile_text_modification(&m);
        assert!(is_unsupported(&a), "{what}: {a:?}");
        let b = compile_text_effect(&dsl_effect_raw(Layer::L3, m));
        assert!(is_unsupported(&b), "{what}: {b:?}");
    }
    // The same through the parser (documents that parse but are malformed).
    for doc in [
        json!({"op":"REPLACE_TEXT_WORD","from":"Swamp","to":"Island"}),
        json!({"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","to":"Island"}),
        json!({"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp"}),
        json!({"op":"REPLACE_TEXT_WORD","category":7,"from":"Swamp","to":"Island"}),
        json!({"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":3,"to":"Island"}),
        json!({"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp","to":[1]}),
        json!({"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp","to":"Island",
            "extra":1}),
    ] {
        let r = compile_text_effect(&dsl_effect("3", &doc));
        assert!(is_unsupported(&r), "{doc}: {r:?}");
    }
}

/// REQ-DSL-012, REQ-DSL-013: any other op is `LayerError::Unsupported` (the compile step owns
/// only REPLACE_TEXT_WORD), including ops carrying category/from/to members.
#[test]
fn req_dsl_012_compile_rejects_other_ops() {
    let mut set_pt: Modification =
        serde_json::from_value(json!({"op":"SET_BASE_PT","power":1,"toughness":1})).expect("parse");
    let ms = vec![
        Modification::new("TEST_OP"),
        Modification::new("SET_TEXT"),
        Modification::new(""),
        raw(
            "TEST_REPLACE",
            &[
                ("category", json!("COLOR_WORD")),
                ("from", json!("Swamp")),
                ("to", json!("Island")),
            ],
        ),
        raw(
            "replace_text_word",
            &[
                ("category", json!("COLOR_WORD")),
                ("from", json!("Swamp")),
                ("to", json!("Island")),
            ],
        ),
        set_pt.clone(),
    ];
    set_pt.op = "MODIFY_PT".into();
    for m in ms.into_iter().chain([set_pt]) {
        let a = compile_text_modification(&m);
        assert!(is_unsupported(&a), "{}: {a:?}", m.op);
        for layer in [Layer::L3, Layer::L7b] {
            let b = compile_text_effect(&dsl_effect_raw(layer, m.clone()));
            assert!(is_unsupported(&b), "{} {layer:?}: {b:?}", m.op);
        }
    }
}

/// REQ-DSL-012; CR 613.1c: a well-formed REPLACE_TEXT_WORD in any layer other than 3 is
/// `LayerError::Unsupported` from `compile_text_effect` (parsed and built), and `Ok` in layer 3.
#[test]
fn req_dsl_012_compile_effect_requires_layer_3() {
    for layer in NOT_L3 {
        for (cat, _) in SUPPORTED {
            let m = Modification::replace_text_word(cat, "Swamp", "Island");
            let r = compile_text_effect(&dsl_effect_raw(layer, m.clone()));
            assert!(is_unsupported(&r), "{layer:?} {cat:?}: {r:?}");
            let ok = compile_text_effect(&dsl_effect_raw(Layer::L3, m));
            assert!(ok.is_ok(), "{cat:?} in layer 3: {ok:?}");
        }
    }
    for wire in ["1a", "1b", "2", "4", "5", "6", "7a", "7b", "7c", "7d"] {
        let r = compile_text_effect(&dsl_effect(wire, &rtw_json("COLOR_WORD", "Red", "Blue")));
        assert!(is_unsupported(&r), "layer {wire}: {r:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// 8. registration and layer 3 application
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012, REQ-LAYER-002; CR 613.1c: a compiled modification registers with
/// `ContinuousEffects::register` in layer 3, is applied in layer 3 only (every order-record step
/// of the effect is a layer-3 step), and outside layer 3 the registry still refuses it.
#[test]
fn req_dsl_012_compiled_registers_and_applies_in_layer_3() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            rules_text: enc(vec![
                lit_t("l0", "test.when"),
                word("t1", BasicLandType, "Swamp"),
            ]),
            ..creature("test.text_a", 2, 2)
        },
    );
    let m = compiled("BASIC_LAND_TYPE", "Swamp", "Island");
    let mut r = ContinuousEffects::new();
    assert_eq!(r.register(eid(7), l3_eff(5, &[a], m.clone())), Ok(()));
    let (e, rec) = evaluate_with_order(&r, &env, a).expect("evaluate");
    let steps: Vec<_> = rec
        .steps
        .iter()
        .filter(|s| s.item == OrderItem::Effect(eid(7)))
        .collect();
    assert!(!steps.is_empty(), "the compiled effect is applied");
    assert!(steps.iter().all(|s| s.layer == Layer::L3), "{steps:?}");
    assert!(steps.iter().all(|s| s.applied));
    assert_eq!(
        token_value(&dec(&e.characteristics.rules_text), "t1"),
        "Island"
    );
    // Power and toughness, types and the rest are untouched.
    assert_eq!(pt(&e), spt(2, 2));
    assert_eq!(e.characteristics.card_types, set(&["Creature"]));
    // With a selector too.
    let mut r2 = ContinuousEffects::new();
    assert_eq!(
        r2.register(eid(8), eff(Layer::L3, 5, by_sel(SEL_ALL), m.clone())),
        Ok(())
    );
    assert_eq!(token_value(&eval_text(&r2, &env, a), "t1"), "Island");
    // The registry keeps its own layer check (CR 613.1c).
    for layer in NOT_L3 {
        let mut r3 = ContinuousEffects::new();
        assert!(
            r3.register(eid(9), eff(layer, 5, objs(&[a]), m.clone()))
                .is_err(),
            "{layer:?}"
        );
        assert!(r3.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// 9. acceptance scenario
// ---------------------------------------------------------------------------------------------

const S_ID: u64 = 10;
const S2_ID: u64 = 13;
const X_ID: u64 = 11;
const Y_ID: u64 = 12;
const ANTHEM: u64 = 50;
const TEXT_CHANGE: u64 = 51;

/// The source's rules text: [Literal, Word{COLOR_WORD "Swamp"} t1, Literal, NameRef SELF].
fn source_tokens() -> Vec<TextToken> {
    vec![
        lit_t("l0", "test.creatures_with_subtype"),
        word("t1", ColorWord, "Swamp"),
        lit_t("l2", "test.get_bonus"),
        self_ref("t3"),
    ]
}

/// A second object with the same value in another category (BASIC_LAND_TYPE "Swamp").
fn other_category_tokens() -> Vec<TextToken> {
    vec![
        word("t1", BasicLandType, "Swamp"),
        lit_t("l1", "test.other"),
    ]
}

/// The dependent ability's predicate and selector: `{"pred":"HAS_SUBTYPE","value":
/// {"textToken":"t1"}}`.
fn dependent_pred() -> PredicateAST {
    PredicateAST::HasSubtype { value: tt("t1") }
}

fn dependent_selector() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(dependent_pred())),
    })
}

/// S (the source), S2 (another-category word), X (subtype Swamp), Y (subtype Island). The
/// dependent selector is resolved through `source_text` (the source's evaluated text; cross-object
/// evaluation is the caller's, gap G2 of #37).
fn scenario_env(source_text: &Stream) -> TestEnv {
    let mut env = TestEnv::new();
    env.put(oid(S_ID), with_text("test.source", source_tokens()));
    env.put(
        oid(S2_ID),
        with_text("test.source_2", other_category_tokens()),
    );
    env.put(
        oid(X_ID),
        Characteristics {
            subtypes: set(&["Swamp"]),
            ..creature("test.x", 2, 2)
        },
    );
    env.put(
        oid(Y_ID),
        Characteristics {
            subtypes: set(&["Island"]),
            ..creature("test.y", 2, 2)
        },
    );
    let sel = dependent_selector();
    let text = source_text.clone();
    env.add_selector(
        sel.clone(),
        Rc::new(move |ctx: &MatchContext<'_>| {
            match resolve_selector_words(&sel, &text).expect("selector resolves") {
                ObjectSelectorAST::Filter(f) => match f.predicate.as_deref() {
                    Some(PredicateAST::HasSubtype {
                        value: WordRef::Name(n),
                    }) => ctx.candidate_view.characteristics.subtypes.contains(n),
                    other => panic!("unexpected resolved predicate {other:?}"),
                },
                other => panic!("unexpected resolved selector {other:?}"),
            }
        }),
    );
    env
}

/// The text change compiled from the DSL document of the source's effect.
fn compiled_text_change() -> LayerModification {
    let doc = json!({"type":"CHAR","layer":"3","affected":{"ref":"TARGET","slot":0},
        "modification":{"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp",
        "to":"Island"}});
    let e: CharacteristicEffect = serde_json::from_value(doc).expect("parse");
    compile_text_effect(&e).expect("compiles")
}

fn scenario_registry(text_change: bool) -> ContinuousEffects {
    let mut v = vec![(
        ANTHEM,
        static_eff(
            oid(S_ID),
            None,
            Layer::L7c,
            3,
            Affected::Selector(dependent_selector()),
            modify_pt(1, 1),
        ),
    )];
    if text_change {
        v.push((
            TEXT_CHANGE,
            l3_eff(9, &[oid(S_ID), oid(S2_ID)], compiled_text_change()),
        ));
    }
    registry(&v)
}

type Pt = (Option<i64>, Option<i64>);

/// Stage 1 evaluates S; stage 2 evaluates X and Y with the selector resolved through S's
/// evaluated text. Returns S's evaluated text and X's and Y's power/toughness.
fn run_scenario(text_change: bool) -> (Stream, Pt, Pt) {
    let r = scenario_registry(text_change);
    let stage1 = scenario_env(&stream(source_tokens()));
    let s_text = eval_text(&r, &stage1, oid(S_ID));
    let stage2 = scenario_env(&s_text);
    let x = evaluate(&r, &stage2, oid(X_ID)).expect("evaluate X");
    let y = evaluate(&r, &stage2, oid(Y_ID)).expect("evaluate Y");
    (s_text, pt(&x), pt(&y))
}

/// REQ-DSL-012 (acceptance: "a synthetic card's text-changing effect, compiled from the DSL,
/// changes what a dependent ability applies to"); CR 613.1c: without the effect the dependent
/// predicate resolves to Name("Swamp") and the Swamp creature gets +1/+1; with the compiled
/// COLOR_WORD Swamp->Island effect it resolves to Name("Island") and the Island creature gets
/// +1/+1 instead.
#[test]
fn req_dsl_012_acceptance_compiled_effect_retargets_dependent_ability() {
    let (text, x, y) = run_scenario(false);
    assert_eq!(
        resolve_predicate_words(&dependent_pred(), &text),
        Ok(PredicateAST::HasSubtype { value: nm("Swamp") })
    );
    assert_eq!(x, spt(3, 3));
    assert_eq!(y, spt(2, 2));

    let (text, x, y) = run_scenario(true);
    assert_eq!(
        resolve_predicate_words(&dependent_pred(), &text),
        Ok(PredicateAST::HasSubtype {
            value: nm("Island")
        })
    );
    assert_eq!(x, spt(2, 2));
    assert_eq!(y, spt(3, 3));
}

/// REQ-DSL-012; CR 612.2, 613.1c: after the compiled effect the evaluated text of S has word t1
/// "Island" with ids, order and kinds kept and the literals and SELF untouched; the
/// BASIC_LAND_TYPE "Swamp" word of S2 (another category, same value) is not rewritten; the
/// registered dependent selector still holds the token id.
#[test]
fn req_dsl_012_acceptance_evaluated_text_and_category_scope() {
    let r = scenario_registry(true);
    let env = scenario_env(&stream(source_tokens()));
    let s = eval_text(&r, &env, oid(S_ID));
    let mut want = source_tokens();
    want[1] = word("t1", ColorWord, "Island");
    assert_eq!(s.tokens(), want.as_slice());
    let ids: Vec<&str> = s.tokens().iter().map(TextToken::id).collect();
    assert_eq!(ids, vec!["l0", "t1", "l2", "t3"]);
    let s2 = eval_text(&r, &env, oid(S2_ID));
    assert_eq!(s2, stream(other_category_tokens()));
    assert_eq!(
        r.get(eid(ANTHEM)).expect("registered").affected,
        Affected::Selector(dependent_selector())
    );
    // Without the effect the evaluated text equals the base.
    let r0 = scenario_registry(false);
    assert_eq!(eval_text(&r0, &env, oid(S_ID)), stream(source_tokens()));
}

// ---------------------------------------------------------------------------------------------
// 10. value-expression resolution
// ---------------------------------------------------------------------------------------------

fn battlefield(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: Some(vec![Zone::Battlefield]),
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

fn add(lhs: ValueExpressionAST, rhs: ValueExpressionAST) -> ValueExpressionAST {
    ValueExpressionAST::Math {
        op: MathOp::Add,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// A value expression reaching `w` through a devotion word list, a count over a selector whose
/// predicate holds it (nested in And/Not and in a Compare's devotion), a history query's mana
/// colour and a history query's condition; `fixed` is a Name that must stay unchanged.
fn value_with(w: &WordRef) -> ValueExpressionAST {
    let devotion = ValueExpressionAST::PlayerAttr {
        reference: ReferenceAST::Controller,
        attr: PlayerAttr::Devotion(vec![w.clone(), nm("Blue")]),
    };
    let count = ValueExpressionAST::Count(Box::new(battlefield(PredicateAST::And(vec![
        PredicateAST::HasSubtype { value: w.clone() },
        PredicateAST::Not(Box::new(PredicateAST::HasColor { value: nm("Green") })),
        PredicateAST::Compare {
            cmp: Cmp::Ge,
            lhs: Box::new(ValueExpressionAST::PlayerAttr {
                reference: ReferenceAST::Controller,
                attr: PlayerAttr::Devotion(vec![w.clone()]),
            }),
            rhs: Box::new(ValueExpressionAST::Literal(1)),
        },
    ]))));
    let mana = ValueExpressionAST::History(Box::new(HistoryQuery::ManaSpentToCast {
        reference: ReferenceAST::SelfRef,
        color: Some(w.clone()),
    }));
    let spells = ValueExpressionAST::History(Box::new(HistoryQuery::SpellsCastThisTurn {
        player: None,
        condition: Some(PredicateAST::HasColor { value: w.clone() }),
    }));
    add(devotion, add(count, add(mana, spells)))
}

fn sample_text() -> Stream {
    stream(source_tokens())
}

/// REQ-DSL-012 (SRS §14.5, gap G4 of #37): `resolve_value_words` replaces every reachable
/// `WordRef::TextToken` by `WordRef::Name(current value)`; before the compiled effect it yields
/// the old word, after it the new one; the result holds no TextToken; the input is unchanged.
#[test]
fn req_dsl_012_resolve_value_words_before_and_after() {
    let v = value_with(&tt("t1"));
    let copy = v.clone();
    let before = resolve_value_words(&v, &sample_text()).expect("resolves");
    assert_eq!(before, value_with(&nm("Swamp")));
    assert!(!value_has_token(&before));
    assert!(value_has_token(&v), "the walker sees the input's tokens");

    let mut env = TestEnv::new();
    let s = env.put(oid(S_ID), with_text("test.source", source_tokens()));
    let r = registry(&[(1, l3_eff(5, &[s], compiled_text_change()))]);
    let after_text = eval_text(&r, &env, s);
    let after = resolve_value_words(&v, &after_text).expect("resolves");
    assert_eq!(after, value_with(&nm("Island")));
    assert!(!value_has_token(&after));
    assert_eq!(v, copy);
}

/// REQ-DSL-012: each single reach path resolves on its own (devotion, count/selector predicate,
/// selector count expression, history mana colour, history condition).
#[test]
fn req_dsl_012_resolve_value_words_each_path() {
    let text = sample_text();
    let paths: Vec<fn(&WordRef) -> ValueExpressionAST> = vec![
        |w| ValueExpressionAST::PlayerAttr {
            reference: ReferenceAST::Controller,
            attr: PlayerAttr::Devotion(vec![w.clone()]),
        },
        |w| {
            ValueExpressionAST::Count(Box::new(battlefield(PredicateAST::Or(vec![
                PredicateAST::IsCreature,
                PredicateAST::HasColor { value: w.clone() },
            ]))))
        },
        |w| {
            ValueExpressionAST::Count(Box::new(ObjectSelectorAST::Filter(SelectorFilter {
                zones: None,
                count: Some(Box::new(ValueExpressionAST::PlayerAttr {
                    reference: ReferenceAST::Controller,
                    attr: PlayerAttr::Devotion(vec![w.clone()]),
                })),
                ordering: None,
                predicate: Some(Box::new(PredicateAST::IsCreature)),
            })))
        },
        |w| {
            ValueExpressionAST::History(Box::new(HistoryQuery::ManaSpentToCast {
                reference: ReferenceAST::SelfRef,
                color: Some(w.clone()),
            }))
        },
        |w| {
            ValueExpressionAST::History(Box::new(HistoryQuery::CreaturesDiedThisTurn {
                condition: Some(PredicateAST::HasSubtype { value: w.clone() }),
            }))
        },
    ];
    for (i, mk) in paths.iter().enumerate() {
        let out = resolve_value_words(&mk(&tt("t1")), &text);
        assert_eq!(out, Ok(mk(&nm("Swamp"))), "path {i}");
        assert!(!value_has_token(&out.expect("ok")), "path {i}");
        // A Name is returned unchanged.
        assert_eq!(
            resolve_value_words(&mk(&nm("test.fixed")), &text),
            Ok(mk(&nm("test.fixed"))),
            "path {i}"
        );
    }
}

/// REQ-DSL-012: Names, token-free expressions and a history query without a colour come back
/// equal.
#[test]
fn req_dsl_012_resolve_value_words_names_unchanged() {
    let text = sample_text();
    for v in [
        value_with(&nm("Swamp")),
        value_with(&nm("t1")),
        ValueExpressionAST::Literal(3),
        ValueExpressionAST::X,
        ValueExpressionAST::Memory("test.key".into()),
        ValueExpressionAST::History(Box::new(HistoryQuery::ManaSpentToCast {
            reference: ReferenceAST::SelfRef,
            color: None,
        })),
    ] {
        assert_eq!(resolve_value_words(&v, &text), Ok(v.clone()));
    }
}

/// REQ-DSL-012: a missing token id is `TextWordError::UnknownToken`; a literal or name-reference
/// token id is `TextWordError::NotAWord`, wherever the reference sits.
#[test]
fn req_dsl_012_resolve_value_words_errors() {
    let text = sample_text();
    assert_eq!(
        resolve_value_words(&value_with(&tt("t99")), &text),
        Err(TextWordError::UnknownToken("t99".into()))
    );
    for id in ["l0", "l2", "t3"] {
        assert_eq!(
            resolve_value_words(&value_with(&tt(id)), &text),
            Err(TextWordError::NotAWord(id.into())),
            "{id}"
        );
    }
    // Deep only: the bad reference only in the history colour.
    let deep = add(
        ValueExpressionAST::Literal(1),
        ValueExpressionAST::History(Box::new(HistoryQuery::ManaSpentToCast {
            reference: ReferenceAST::SelfRef,
            color: Some(tt("t42")),
        })),
    );
    assert_eq!(
        resolve_value_words(&deep, &text),
        Err(TextWordError::UnknownToken("t42".into()))
    );
    // Deep only: inside a count's selector predicate.
    let deep2 = ValueExpressionAST::Count(Box::new(battlefield(PredicateAST::Not(Box::new(
        PredicateAST::HasColor { value: tt("t3") },
    )))));
    assert_eq!(
        resolve_value_words(&deep2, &text),
        Err(TextWordError::NotAWord("t3".into()))
    );
}

// ---------------------------------------------------------------------------------------------
// 11. both timestamp orders (CR 613.8)
// ---------------------------------------------------------------------------------------------

const EFF_A: u64 = 1;
const EFF_B: u64 = 2;

fn swamp_island() -> Vec<TextToken> {
    vec![
        lit_t("l0", "test.text"),
        word("w1", BasicLandType, "Swamp"),
        word("w2", BasicLandType, "Island"),
    ]
}

/// The evaluated text and layer-3 order of `swamp_island()` under compiled A = Swamp->`a_to` at
/// `ta` and compiled B = `b_from`->`b_to` at `tb`; asserts cached == uncached.
fn run_mixed(ta: u64, tb: u64, a_to: &str, b_from: &str, b_to: &str) -> (Stream, Vec<OrderItem>) {
    let mut env = TestEnv::new();
    let x = env.put(oid(1), with_text("test.mixed", swamp_island()));
    let r = registry(&[
        (
            EFF_A,
            l3_eff(ta, &[x], compiled("BASIC_LAND_TYPE", "Swamp", a_to)),
        ),
        (
            EFF_B,
            l3_eff(tb, &[x], compiled("BASIC_LAND_TYPE", b_from, b_to)),
        ),
    ]);
    let (e, rec) = evaluate_with_order(&r, &env, x).expect("evaluate");
    assert_eq!(evaluate(&r, &env, x).expect("evaluate"), e);
    let mut cache = LayerCache::new();
    assert_eq!(cache.evaluate(&r, &env, x).expect("cached"), e);
    assert_eq!(cache.evaluate(&r, &env, x).expect("cached again"), e);
    (dec(&e.characteristics.rules_text), l3_steps(&rec))
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8: compiled A = Swamp->Island and B = Island->Forest on
/// text [Swamp, Island]: B depends on A, so in every timestamp order both words end as Forest and
/// A applies before B (mirrors the #37 assertions for the uncompiled form).
#[test]
fn req_layer_002_compiled_chain_both_timestamp_orders() {
    for (ta, tb) in [(5, 6), (6, 5), (20, 10), (10, 20)] {
        let (text, order) = run_mixed(ta, tb, "Island", "Island", "Forest");
        assert_eq!(token_value(&text, "w1"), "Forest", "ts(A)={ta} ts(B)={tb}");
        assert_eq!(token_value(&text, "w2"), "Forest", "ts(A)={ta} ts(B)={tb}");
        assert_eq!(text.find("l0"), Some(&lit_t("l0", "test.text")));
        assert_eq!(
            order,
            vec![OrderItem::Effect(eid(EFF_A)), OrderItem::Effect(eid(EFF_B))],
            "ts(A)={ta} ts(B)={tb}"
        );
    }
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8: compiled A = Swamp->Island and B = Island->Swamp on
/// text [Swamp, Island] form a dependency loop applied in timestamp order: A earlier gives
/// [Swamp, Swamp], B earlier gives [Island, Island] (mirrors #37).
#[test]
fn req_layer_002_compiled_loop_both_timestamp_orders() {
    for (ta, tb, want, first, second) in [
        (5, 6, "Swamp", EFF_A, EFF_B),
        (6, 5, "Island", EFF_B, EFF_A),
        (10, 20, "Swamp", EFF_A, EFF_B),
        (20, 10, "Island", EFF_B, EFF_A),
    ] {
        let (text, order) = run_mixed(ta, tb, "Island", "Island", "Swamp");
        assert_eq!(token_value(&text, "w1"), want, "ts(A)={ta} ts(B)={tb}");
        assert_eq!(token_value(&text, "w2"), want, "ts(A)={ta} ts(B)={tb}");
        assert_eq!(
            order,
            vec![
                OrderItem::Effect(eid(first)),
                OrderItem::Effect(eid(second))
            ],
            "ts(A)={ta} ts(B)={tb}"
        );
    }
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8 (DECISIONS "#37 FP-036 — Dependency between text
/// effects"): a whole-text SetText and a compiled replacement on the same object: the SetText
/// applies first in BOTH timestamp orders and the replacement rewrites the new text.
#[test]
fn req_layer_002_compiled_with_set_text_both_timestamp_orders() {
    const SET: u64 = 1;
    const REP: u64 = 2;
    let base = vec![lit_t("l0", "test.old"), word("t1", BasicLandType, "Swamp")];
    let replacement = vec![lit_t("r0", "test.new"), word("r1", BasicLandType, "Swamp")];
    let want = stream(vec![
        lit_t("r0", "test.new"),
        word("r1", BasicLandType, "Island"),
    ]);
    for (t_set, t_rep) in [(5, 6), (6, 5), (10, 30), (30, 10)] {
        let mut env = TestEnv::new();
        let a = env.put(oid(1), with_text("test.text_a", base.clone()));
        let r = registry(&[
            (
                SET,
                l3_eff(
                    t_set,
                    &[a],
                    LayerModification::SetText(enc(replacement.clone())),
                ),
            ),
            (
                REP,
                l3_eff(t_rep, &[a], compiled("BASIC_LAND_TYPE", "Swamp", "Island")),
            ),
        ]);
        let ctx = format!("SetText ts {t_set}, replace ts {t_rep}");
        let (e, rec) = evaluate_with_order(&r, &env, a).expect("evaluate");
        assert_eq!(dec(&e.characteristics.rules_text), want, "{ctx}");
        assert_eq!(
            l3_steps(&rec),
            vec![OrderItem::Effect(eid(SET)), OrderItem::Effect(eid(REP))],
            "{ctx}"
        );
        let mut cache = LayerCache::new();
        assert_eq!(cache.evaluate(&r, &env, a).expect("cached"), e, "{ctx}");
    }
}

// ---------------------------------------------------------------------------------------------
// 12. determinism
// ---------------------------------------------------------------------------------------------

/// REQ-CORE-001, REQ-DSL-012: compile is a pure function (same input, same output, repeatedly,
/// including for errors); evaluating twice, `evaluate_all`, and a `LayerCache` (twice) give
/// identical results and digests.
#[test]
fn req_core_001_compile_pure_and_evaluation_deterministic() {
    let inputs = vec![
        Modification::replace_text_word(ColorWord, "Swamp", "Island"),
        Modification::replace_text_word(CardName, "a", "b"),
        Modification::replace_text_word(Number, "", "4"),
        Modification::new("TEST_OP"),
        raw("REPLACE_TEXT_WORD", &[("category", json!(1))]),
    ];
    for m in &inputs {
        let first = compile_text_modification(m);
        for _ in 0..3 {
            assert_eq!(compile_text_modification(m), first, "{m:?}");
            assert_eq!(compile_text_modification(&m.clone()), first, "{m:?}");
        }
        for layer in [Layer::L3, Layer::L6] {
            let e = dsl_effect_raw(layer, m.clone());
            let f = compile_text_effect(&e);
            assert_eq!(compile_text_effect(&e), f, "{m:?} {layer:?}");
            assert_eq!(compile_text_effect(&e.clone()), f, "{m:?} {layer:?}");
        }
    }

    let mut env = TestEnv::new();
    let a = env.put(oid(1), with_text("test.a", source_tokens()));
    let b = env.put(oid(2), with_text("test.b", swamp_island()));
    let c = env.put(oid(3), with_text("test.c", other_category_tokens()));
    let r = registry(&[
        (
            1,
            l3_eff(5, &[a, c], compiled("COLOR_WORD", "Swamp", "Island")),
        ),
        (
            2,
            l3_eff(6, &[b, c], compiled("BASIC_LAND_TYPE", "Swamp", "Island")),
        ),
        (
            3,
            eff(
                Layer::L3,
                7,
                by_sel(SEL_ALL),
                compiled("BASIC_LAND_TYPE", "Island", "Forest"),
            ),
        ),
    ]);
    let all = evaluate_all(&r, &env).expect("evaluate_all");
    for id in [a, b, c] {
        let e1 = evaluate(&r, &env, id).expect("evaluate");
        let e2 = evaluate(&r, &env, id).expect("evaluate");
        assert_eq!(e1, e2);
        assert_eq!(all[&id], e1);
    }
    let mut cache = LayerCache::new();
    let cached = cache.evaluate_all(&r, &env).expect("cached");
    assert_eq!(cached, all);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&all));
    let again = cache.evaluate_all(&r, &env).expect("cached again");
    assert_eq!(again, all);
    for id in [a, b, c] {
        assert_eq!(cache.evaluate(&r, &env, id).expect("cached"), all[&id]);
    }
    // A fresh registry built from freshly compiled effects agrees.
    let r2 = registry(&[
        (
            1,
            l3_eff(5, &[a, c], compiled("COLOR_WORD", "Swamp", "Island")),
        ),
        (
            2,
            l3_eff(6, &[b, c], compiled("BASIC_LAND_TYPE", "Swamp", "Island")),
        ),
        (
            3,
            eff(
                Layer::L3,
                7,
                by_sel(SEL_ALL),
                compiled("BASIC_LAND_TYPE", "Island", "Forest"),
            ),
        ),
    ]);
    assert_eq!(evaluate_all(&r2, &env).expect("fresh"), all);
    // Expected values: a's t1 Island, b's words Forest (chain), c untouched by the COLOR_WORD
    // effect but its BASIC_LAND_TYPE Swamp chained to Forest.
    assert_eq!(
        token_value(&dec(&all[&a].characteristics.rules_text), "t1"),
        "Island"
    );
    let bt = dec(&all[&b].characteristics.rules_text);
    assert_eq!(token_value(&bt, "w1"), "Forest");
    assert_eq!(token_value(&bt, "w2"), "Forest");
    assert_eq!(
        token_value(&dec(&all[&c].characteristics.rules_text), "t1"),
        "Forest"
    );
}

// ---------------------------------------------------------------------------------------------
// 13. robustness
// ---------------------------------------------------------------------------------------------

const OPS: [&str; 6] = [
    "REPLACE_TEXT_WORD",
    "REPLACE_TEXT_WORD",
    "REPLACE_TEXT_WORD",
    "SET_TEXT",
    "TEST_OP",
    "",
];
const CATS: [&str; 8] = [
    "COLOR_WORD",
    "BASIC_LAND_TYPE",
    "CREATURE_TYPE",
    "NUMBER",
    "CARD_NAME",
    "TEST_CATEGORY",
    "",
    "color_word",
];
const WORDS: [&str; 10] = [
    "Swamp",
    "Island",
    "Red",
    "3",
    "",
    "\u{FFFF}",
    "a\u{FDD0}",
    "test.word",
    "Goblin",
    "Swamp",
];

fn pick<'a, T>(rng: &mut Lcg, xs: &'a [T]) -> &'a T {
    &xs[usize::try_from(rng.below(xs.len() as u64)).expect("fits")]
}

fn random_param(rng: &mut Lcg, pool: &[&str]) -> Option<Value> {
    match rng.below(10) {
        0 => None,
        1 => Some(Value::Null),
        2 => Some(json!(i64::try_from(rng.below(100)).expect("fits"))),
        3 => Some(json!([pick(rng, pool)])),
        4 => Some(json!({"name": pick(rng, pool)})),
        5 => Some(Value::Bool(true)),
        _ => Some(json!(pick(rng, pool))),
    }
}

fn random_modification(rng: &mut Lcg) -> Modification {
    let mut m = Modification::new(*pick(rng, &OPS));
    if let Some(v) = random_param(rng, &CATS) {
        m.params.insert("category".into(), v);
    }
    if let Some(v) = random_param(rng, &WORDS) {
        m.params.insert("from".into(), v);
    }
    if let Some(v) = random_param(rng, &WORDS) {
        m.params.insert("to".into(), v);
    }
    if rng.below(8) == 0 {
        let mut extra = Map::new();
        extra.insert("k".into(), json!(1));
        m.params.insert(
            (*pick(rng, &["extra", "", "layer"])).to_string(),
            Value::Object(extra),
        );
    }
    m
}

/// REQ-DSL-012, REQ-CORE-001 (R12): a bounded random run of arbitrary `Modification` values
/// through `compile_text_modification` and `compile_text_effect` (every layer) never panics;
/// an Ok is exactly the accessor's well-formed tuple with a supported category and valid words,
/// `compile_text_effect` is Ok only in layer 3 and then equals `compile_text_modification`, and
/// every Ok registers with `ContinuousEffects::register` in layer 3.
#[test]
fn req_dsl_012_random_modifications_never_panic_and_ok_registers() {
    let mut env = TestEnv::new();
    let a = env.put(oid(1), with_text("test.a", source_tokens()));
    let mut rng = Lcg::new(0x0195_0150);
    let mut oks = 0u32;
    let mut layers: Vec<Layer> = NOT_L3.to_vec();
    layers.push(Layer::L3);
    for i in 0..3000u64 {
        let m = random_modification(&mut rng);
        let r = compile_text_modification(&m);
        let layer = *pick(&mut rng, &layers);
        let re = compile_text_effect(&dsl_effect_raw(layer, m.clone()));
        if let Err(e) = &r {
            assert!(matches!(e, LayerError::Unsupported(_)), "{m:?}: {e:?}");
        }
        if let Err(e) = &re {
            assert!(matches!(e, LayerError::Unsupported(_)), "{m:?}: {e:?}");
        }
        if layer == Layer::L3 {
            assert_eq!(re, r, "{m:?}");
        } else {
            assert!(re.is_err(), "{m:?} in {layer:?}: {re:?}");
        }
        if let Ok(lm) = &r {
            oks += 1;
            let (cat, from, to) = m
                .as_replace_text_word()
                .expect("well-formed")
                .expect("is REPLACE_TEXT_WORD");
            assert_ne!(cat, CardName);
            assert!(!from.is_empty() && !to.is_empty(), "{m:?}");
            assert_eq!(
                lm,
                &LayerModification::ReplaceTextWord {
                    category: cat,
                    from,
                    to
                }
            );
            let mut reg = ContinuousEffects::new();
            assert_eq!(
                reg.register(eid(i + 1), l3_eff(5, &[a], lm.clone())),
                Ok(()),
                "{m:?}"
            );
            evaluate(&reg, &env, a).expect("evaluates");
        }
    }
    assert!(oks > 20, "the generator must reach the Ok path ({oks})");
}

// ---------------------------------------------------------------------------------------------
// 14. granted abilities
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 612.3: abilities granted by effects are not text: an object with a printed
/// static ability and a layer-6 granted ability, plus a compiled replacement that rewrites its
/// text, has `abilities` byte-identical to the evaluation without the replacement.
#[test]
fn req_dsl_012_compiled_replacement_leaves_granted_abilities() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            abilities: vec![static_ability("test.a.static")],
            rules_text: enc(source_tokens()),
            ..creature("test.a", 2, 2)
        },
    );
    let grant = eff(
        Layer::L6,
        3,
        objs(&[a]),
        LayerModification::AddAbility(kw_ability("test.granted", "flying")),
    );
    let without = registry(&[(1, grant.clone())]);
    let with = registry(&[(1, grant), (2, l3_eff(5, &[a], compiled_text_change()))]);
    let e0 = evaluate(&without, &env, a).expect("evaluate");
    let e1 = evaluate(&with, &env, a).expect("evaluate");
    assert_eq!(e0.characteristics.abilities.len(), 2);
    assert_eq!(e1.characteristics.abilities, e0.characteristics.abilities);
    assert_eq!(
        e1.characteristics.abilities[1],
        kw_ability("test.granted", "flying")
    );
    assert_eq!(
        token_value(&dec(&e1.characteristics.rules_text), "t1"),
        "Island"
    );
    assert_eq!(
        Characteristics {
            rules_text: e0.characteristics.rules_text.clone(),
            ..e1.characteristics.clone()
        },
        e0.characteristics
    );
}
