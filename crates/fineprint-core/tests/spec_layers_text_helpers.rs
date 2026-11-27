// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the pure text-token helpers of `fineprint_core::layers` (REQ-DSL-012; CR
//! 612.1, 612.2, 201.5): `rewrite_text_words`, `resolve_word_ref`, `text_token_matches`,
//! `resolve_predicate_words`, `resolve_selector_words` and `TextWordError`. #232 FP-164:
//! `decode_rules_text`/`encode_rules_text` were removed with the placeholder rules text (the
//! rules text of `Characteristics` is the typed stream itself); their tests now check the typed
//! stream's wire form and its refusal of invalid placeholders.
//!
//! Black-box: written from the FP-036 brief only. Every token value is rules vocabulary or a
//! neutral `test.*` string (R5).

#[path = "spec_layers_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::layers::{
    TextWordError, resolve_predicate_words, resolve_selector_words, resolve_word_ref,
    rewrite_text_words, text_token_matches,
};
use fineprint_dsl::card;
use fineprint_dsl::expr::{
    Cmp, ObjectSelectorAST, PlayerAttr, PredicateAST, ReferenceAST, SelectorFilter,
    ValueExpressionAST, WordRef, Zone,
};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream as Stream};
use support::via_msgpack;

use RewritableCategory::{BasicLandType, CardName, ColorWord, CreatureType, Number};

// ---------------------------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------------------------

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

fn other_ref(id: &str, name: &str) -> TextToken {
    TextToken::NameRef {
        id: id.into(),
        kind: NameRefKind::Other,
        name: Some(name.into()),
    }
}

fn stream(tokens: Vec<TextToken>) -> Stream {
    Stream::new(tokens).expect("valid stream")
}

/// A stream with one token of every kind and a word of every category.
fn every_kind() -> Stream {
    stream(vec![
        lit_t("l0", "Gain"),
        word("t1", BasicLandType, "Swamp"),
        word("t2", ColorWord, "Red"),
        word("t3", CreatureType, "Goblin"),
        word("t4", Number, "3"),
        word("t5", CardName, "test.card_name"),
        self_ref("t6"),
        other_ref("t7", "Swamp"),
    ])
}

/// The FP-011 wire form of one token as a string map, built independently of the typed
/// stream's serialiser.
fn wire_token(t: &TextToken) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    match t {
        TextToken::Literal { id, text } => {
            m.insert("id", id.clone());
            m.insert("kind", "LITERAL".into());
            m.insert("text", text.clone());
        }
        TextToken::Word {
            id,
            category,
            value,
        } => {
            let cat = match category {
                ColorWord => "COLOR_WORD",
                BasicLandType => "BASIC_LAND_TYPE",
                CreatureType => "CREATURE_TYPE",
                Number => "NUMBER",
                CardName => "CARD_NAME",
            };
            m.insert("id", id.clone());
            m.insert("kind", "WORD".into());
            m.insert("category", cat.into());
            m.insert("value", value.clone());
        }
        TextToken::NameRef { id, kind, name } => {
            m.insert("id", id.clone());
            m.insert("kind", "NAME_REF".into());
            match kind {
                NameRefKind::SelfRef => {
                    m.insert("ref", "SELF".into());
                }
                NameRefKind::Other => {
                    m.insert("ref", "OTHER".into());
                }
            }
            if let Some(n) = name {
                m.insert("name", n.clone());
            }
        }
    }
    m
}

/// The rules text in the wire form, through the MessagePack bridge, decoded as the typed
/// `card::TextTokenStream` (#232 FP-164).
fn wire(tokens: &[TextToken]) -> card::TextTokenStream {
    let maps: Vec<BTreeMap<&'static str, String>> = tokens.iter().map(wire_token).collect();
    via_msgpack(&maps)
}

/// The JSON of independently built token maps, decoded as the typed `card::TextTokenStream`.
fn try_json_stream(maps: &[BTreeMap<&'static str, String>]) -> Option<card::TextTokenStream> {
    serde_json::from_value(serde_json::to_value(maps).unwrap()).ok()
}

fn tt(id: &str) -> WordRef {
    WordRef::TextToken(id.into())
}

fn nm(n: &str) -> WordRef {
    WordRef::Name(n.into())
}

/// Whether any `WordRef::TextToken` is left anywhere in the predicate (walks every node this
/// suite builds).
fn pred_has_token(p: &PredicateAST) -> bool {
    match p {
        PredicateAST::And(v) | PredicateAST::Or(v) => v.iter().any(pred_has_token),
        PredicateAST::Not(b) => pred_has_token(b),
        PredicateAST::HasCardType { value }
        | PredicateAST::HasSubtype { value }
        | PredicateAST::HasSupertype { value }
        | PredicateAST::HasColor { value } => matches!(value, WordRef::TextToken(_)),
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

fn value_has_token(v: &ValueExpressionAST) -> bool {
    match v {
        ValueExpressionAST::PlayerAttr {
            attr: PlayerAttr::Devotion(ws),
            ..
        } => ws.iter().any(|w| matches!(w, WordRef::TextToken(_))),
        ValueExpressionAST::Count(s) => sel_has_token(s),
        ValueExpressionAST::Math { lhs, rhs, .. } => value_has_token(lhs) || value_has_token(rhs),
        _ => false,
    }
}

fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: Some(vec![Zone::Battlefield]),
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

// ---------------------------------------------------------------------------------------------
// rewrite_text_words
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 612.1, 612.2: a matching word of the category is rewritten; every other
/// token (literal, SELF, OTHER, card-name word, other categories) is unchanged.
#[test]
fn rewrite_changes_only_matching_words_of_the_category() {
    let s = every_kind();
    let out = rewrite_text_words(&s, BasicLandType, "Swamp", "Island").expect("ok");
    let mut want = s.tokens().to_vec();
    want[1] = word("t1", BasicLandType, "Island");
    assert_eq!(out.tokens(), want.as_slice());
}

/// REQ-DSL-012; CR 612.2: every supported category rewrites only its own words.
#[test]
fn rewrite_each_supported_category() {
    let s = every_kind();
    for (cat, from, to, idx) in [
        (ColorWord, "Red", "Blue", 2usize),
        (CreatureType, "Goblin", "Elf", 3),
        (Number, "3", "4", 4),
        (BasicLandType, "Swamp", "Forest", 1),
    ] {
        let out = rewrite_text_words(&s, cat, from, to).expect("ok");
        for (i, (a, b)) in s.tokens().iter().zip(out.tokens()).enumerate() {
            if i == idx {
                assert_eq!(b, &word(a.id(), cat, to), "{cat:?}");
            } else {
                assert_eq!(a, b, "{cat:?} token {i}");
            }
        }
    }
}

/// REQ-DSL-012; CR 612.2: CARD_NAME is rejected first, whatever `from` and `to` are.
#[test]
fn rewrite_card_name_is_unsupported_first() {
    let s = every_kind();
    for (from, to) in [
        ("test.card_name", "test.other"),
        ("", ""),
        ("\u{FFFF}", ""),
        ("", "\u{FFFF}"),
    ] {
        assert_eq!(
            rewrite_text_words(&s, CardName, from, to),
            Err(TextWordError::UnsupportedCategory)
        );
    }
}

/// REQ-DSL-012: an empty or noncharacter `from` or `to` is `InvalidWord`.
#[test]
fn rewrite_invalid_words() {
    let s = every_kind();
    for cat in [ColorWord, BasicLandType, CreatureType, Number] {
        for (from, to) in [
            ("", "Island"),
            ("Swamp", ""),
            ("", ""),
            ("Sw\u{FFFF}amp", "Island"),
            ("Swamp", "Isl\u{FFFF}and"),
            ("\u{FFFF}", "\u{FFFF}"),
            ("\u{FDD0}", "Island"),
        ] {
            assert_eq!(
                rewrite_text_words(&s, cat, from, to),
                Err(TextWordError::InvalidWord),
                "{cat:?} {from:?} {to:?}"
            );
        }
    }
}

/// REQ-DSL-012; CR 612.2: matching is exact and case-sensitive.
#[test]
fn rewrite_is_exact_and_case_sensitive() {
    let s = stream(vec![
        word("a", BasicLandType, "swamp"),
        word("b", BasicLandType, "Swamps"),
        word("c", BasicLandType, "Swamp"),
        word("d", BasicLandType, "SWAMP"),
    ]);
    let out = rewrite_text_words(&s, BasicLandType, "Swamp", "Island").expect("ok");
    let want = stream(vec![
        word("a", BasicLandType, "swamp"),
        word("b", BasicLandType, "Swamps"),
        word("c", BasicLandType, "Island"),
        word("d", BasicLandType, "SWAMP"),
    ]);
    assert_eq!(out, want);
}

/// REQ-DSL-012: from == to changes nothing; an absent word changes nothing; the empty stream
/// stays empty; the input is not mutated.
#[test]
fn rewrite_no_op_cases() {
    let s = every_kind();
    let copy = s.clone();
    assert_eq!(
        rewrite_text_words(&s, BasicLandType, "Swamp", "Swamp"),
        Ok(s.clone())
    );
    assert_eq!(
        rewrite_text_words(&s, BasicLandType, "Plains", "Island"),
        Ok(s.clone())
    );
    // CR 201.5, 612.2: `from` equal to the OTHER name or a literal text rewrites nothing.
    assert_eq!(
        rewrite_text_words(&s, CreatureType, "Swamp", "Elf"),
        Ok(s.clone())
    );
    assert_eq!(
        rewrite_text_words(&s, ColorWord, "Gain", "Blue"),
        Ok(s.clone())
    );
    let empty = Stream::default();
    assert_eq!(
        rewrite_text_words(&empty, ColorWord, "Red", "Blue"),
        Ok(Stream::default())
    );
    assert_eq!(s, copy);
}

/// REQ-DSL-012: several matching words are all rewritten, in place.
#[test]
fn rewrite_every_instance() {
    let s = stream(vec![
        word("a", ColorWord, "Red"),
        lit_t("b", "and"),
        word("c", ColorWord, "Red"),
        word("d", CardName, "Red"),
    ]);
    let out = rewrite_text_words(&s, ColorWord, "Red", "Blue").expect("ok");
    assert_eq!(
        out,
        stream(vec![
            word("a", ColorWord, "Blue"),
            lit_t("b", "and"),
            word("c", ColorWord, "Blue"),
            word("d", CardName, "Red"),
        ])
    );
}

// ---------------------------------------------------------------------------------------------
// resolve_word_ref, text_token_matches
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 201.5, 612.2: a Name is returned as given, a Word of each category (card
/// names included) reads its value, unknown ids and non-words are errors.
#[test]
fn resolve_word_ref_variants() {
    let s = every_kind();
    assert_eq!(resolve_word_ref(&s, &nm("Elf")), Ok("Elf".into()));
    assert_eq!(resolve_word_ref(&s, &nm("t1")), Ok("t1".into()));
    for (id, v) in [
        ("t1", "Swamp"),
        ("t2", "Red"),
        ("t3", "Goblin"),
        ("t4", "3"),
        ("t5", "test.card_name"),
    ] {
        assert_eq!(resolve_word_ref(&s, &tt(id)), Ok(v.into()), "{id}");
    }
    assert_eq!(
        resolve_word_ref(&s, &tt("t99")),
        Err(TextWordError::UnknownToken("t99".into()))
    );
    for id in ["l0", "t6", "t7"] {
        assert_eq!(
            resolve_word_ref(&s, &tt(id)),
            Err(TextWordError::NotAWord(id.into())),
            "{id}"
        );
    }
}

/// REQ-DSL-012: resolve_word_ref reads the rewritten value after a rewrite.
#[test]
fn resolve_word_ref_reads_rewritten_value() {
    let s = every_kind();
    let out = rewrite_text_words(&s, BasicLandType, "Swamp", "Island").expect("ok");
    assert_eq!(resolve_word_ref(&out, &tt("t1")), Ok("Island".into()));
    assert_eq!(resolve_word_ref(&s, &tt("t1")), Ok("Swamp".into()));
}

/// REQ-DSL-012: TEXT_TOKEN_MATCH semantics: true, false, UnknownToken, NotAWord.
#[test]
fn text_token_matches_variants() {
    let s = every_kind();
    assert_eq!(text_token_matches(&s, "t1", "Swamp"), Ok(true));
    assert_eq!(text_token_matches(&s, "t1", "swamp"), Ok(false));
    assert_eq!(text_token_matches(&s, "t1", "Island"), Ok(false));
    assert_eq!(text_token_matches(&s, "t5", "test.card_name"), Ok(true));
    assert_eq!(
        text_token_matches(&s, "nope", "Swamp"),
        Err(TextWordError::UnknownToken("nope".into()))
    );
    for id in ["l0", "t6", "t7"] {
        assert_eq!(
            text_token_matches(&s, id, "Swamp"),
            Err(TextWordError::NotAWord(id.into()))
        );
    }
    let out = rewrite_text_words(&s, BasicLandType, "Swamp", "Island").expect("ok");
    assert_eq!(text_token_matches(&out, "t1", "Island"), Ok(true));
    assert_eq!(text_token_matches(&out, "t1", "Swamp"), Ok(false));
}

// ---------------------------------------------------------------------------------------------
// resolve_predicate_words, resolve_selector_words
// ---------------------------------------------------------------------------------------------

fn nested_pred() -> PredicateAST {
    PredicateAST::And(vec![
        PredicateAST::Or(vec![
            PredicateAST::HasSubtype { value: tt("t3") },
            PredicateAST::Not(Box::new(PredicateAST::HasColor { value: tt("t2") })),
        ]),
        PredicateAST::HasCardType {
            value: nm("Creature"),
        },
        PredicateAST::HasSupertype { value: tt("t1") },
        PredicateAST::TextTokenMatch {
            text_token: "t1".into(),
            value: "Swamp".into(),
        },
        PredicateAST::Controls {
            selector: Box::new(filter(PredicateAST::HasSubtype { value: tt("t1") })),
            player: None,
        },
        PredicateAST::IsCreature,
    ])
}

fn nested_pred_resolved(land: &str) -> PredicateAST {
    PredicateAST::And(vec![
        PredicateAST::Or(vec![
            PredicateAST::HasSubtype {
                value: nm("Goblin"),
            },
            PredicateAST::Not(Box::new(PredicateAST::HasColor { value: nm("Red") })),
        ]),
        PredicateAST::HasCardType {
            value: nm("Creature"),
        },
        PredicateAST::HasSupertype { value: nm(land) },
        PredicateAST::TextTokenMatch {
            text_token: "t1".into(),
            value: "Swamp".into(),
        },
        PredicateAST::Controls {
            selector: Box::new(filter(PredicateAST::HasSubtype { value: nm(land) })),
            player: None,
        },
        PredicateAST::IsCreature,
    ])
}

/// REQ-DSL-012 (SRS §14.5): nested And/Or/Not, the four HAS_* leaves, Controls{selector}: every
/// TextToken becomes Name(current value); Name refs and TextTokenMatch are unchanged; the
/// input is not mutated.
#[test]
fn resolve_predicate_nested() {
    let s = every_kind();
    let p = nested_pred();
    let copy = p.clone();
    let out = resolve_predicate_words(&p, &s).expect("ok");
    assert_eq!(out, nested_pred_resolved("Swamp"));
    assert!(!pred_has_token(&out));
    assert_eq!(p, copy);

    // After a rewrite the same AST reads the new value through the token id.
    let s2 = rewrite_text_words(&s, BasicLandType, "Swamp", "Island").expect("ok");
    let out2 = resolve_predicate_words(&p, &s2).expect("ok");
    assert_eq!(out2, nested_pred_resolved("Island"));
    assert!(!pred_has_token(&out2));
}

/// REQ-DSL-012: a word ref in a value expression reached from a predicate (devotion colours in
/// a Compare) is resolved too.
#[test]
fn resolve_predicate_devotion_in_compare() {
    let s = every_kind();
    let p = PredicateAST::Compare {
        cmp: Cmp::Eq,
        lhs: Box::new(ValueExpressionAST::PlayerAttr {
            reference: ReferenceAST::Controller,
            attr: PlayerAttr::Devotion(vec![tt("t2"), nm("Blue")]),
        }),
        rhs: Box::new(ValueExpressionAST::Literal(1)),
    };
    let out = resolve_predicate_words(&p, &s).expect("ok");
    assert_eq!(
        out,
        PredicateAST::Compare {
            cmp: Cmp::Eq,
            lhs: Box::new(ValueExpressionAST::PlayerAttr {
                reference: ReferenceAST::Controller,
                attr: PlayerAttr::Devotion(vec![nm("Red"), nm("Blue")]),
            }),
            rhs: Box::new(ValueExpressionAST::Literal(1)),
        }
    );
    assert!(!pred_has_token(&out));
}

/// REQ-DSL-012: Name-only and TextTokenMatch-only predicates come back equal.
#[test]
fn resolve_predicate_leaves_names_and_text_token_match() {
    let s = every_kind();
    for p in [
        PredicateAST::HasSubtype {
            value: nm("Goblin"),
        },
        PredicateAST::TextTokenMatch {
            text_token: "t3".into(),
            value: "Goblin".into(),
        },
        PredicateAST::TextTokenMatch {
            text_token: "t99".into(),
            value: "Goblin".into(),
        },
        PredicateAST::IsLand,
    ] {
        assert_eq!(resolve_predicate_words(&p, &s), Ok(p.clone()));
    }
}

/// REQ-DSL-012: an unknown token deep inside a Not inside an And is UnknownToken; a non-word
/// token is NotAWord; the input is not mutated.
#[test]
fn resolve_predicate_errors() {
    let s = every_kind();
    let p = PredicateAST::And(vec![
        PredicateAST::HasSubtype { value: nm("Elf") },
        PredicateAST::Not(Box::new(PredicateAST::HasColor { value: tt("t42") })),
    ]);
    let copy = p.clone();
    assert_eq!(
        resolve_predicate_words(&p, &s),
        Err(TextWordError::UnknownToken("t42".into()))
    );
    assert_eq!(p, copy);
    let q = PredicateAST::Or(vec![
        PredicateAST::HasColor { value: tt("t2") },
        PredicateAST::Not(Box::new(PredicateAST::HasSubtype { value: tt("t6") })),
    ]);
    assert_eq!(
        resolve_predicate_words(&q, &s),
        Err(TextWordError::NotAWord("t6".into()))
    );
}

/// REQ-DSL-012; CR 201.5: a selector filter whose predicate holds the ref is resolved; a
/// `{"ref":"SELF"}` reference selector is unchanged; an unknown token is an error.
#[test]
fn resolve_selector_variants() {
    let s = every_kind();
    let sel = filter(PredicateAST::And(vec![
        PredicateAST::HasSubtype { value: tt("t3") },
        PredicateAST::Not(Box::new(PredicateAST::HasColor { value: tt("t2") })),
    ]));
    let copy = sel.clone();
    let out = resolve_selector_words(&sel, &s).expect("ok");
    assert_eq!(
        out,
        filter(PredicateAST::And(vec![
            PredicateAST::HasSubtype {
                value: nm("Goblin")
            },
            PredicateAST::Not(Box::new(PredicateAST::HasColor { value: nm("Red") })),
        ]))
    );
    assert!(!sel_has_token(&out));
    assert_eq!(sel, copy);

    let self_sel = ObjectSelectorAST::Ref(ReferenceAST::SelfRef);
    assert_eq!(resolve_selector_words(&self_sel, &s), Ok(self_sel.clone()));
    let target = ObjectSelectorAST::Ref(ReferenceAST::Target { slot: 0 });
    assert_eq!(resolve_selector_words(&target, &s), Ok(target.clone()));

    let bad = filter(PredicateAST::Not(Box::new(PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::HasSubtype { value: tt("t77") },
    ]))));
    assert_eq!(
        resolve_selector_words(&bad, &s),
        Err(TextWordError::UnknownToken("t77".into()))
    );
}

/// REQ-DSL-012: a selector's count expression holding a word ref is resolved too.
#[test]
fn resolve_selector_count_expression() {
    let s = every_kind();
    let mk = |w: WordRef| {
        ObjectSelectorAST::Filter(SelectorFilter {
            zones: None,
            count: Some(Box::new(ValueExpressionAST::PlayerAttr {
                reference: ReferenceAST::Controller,
                attr: PlayerAttr::Devotion(vec![w]),
            })),
            ordering: None,
            predicate: Some(Box::new(PredicateAST::IsCreature)),
        })
    };
    let out = resolve_selector_words(&mk(tt("t2")), &s).expect("ok");
    assert_eq!(out, mk(nm("Red")));
}

// ---------------------------------------------------------------------------------------------
// decode / encode
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012 (#232 FP-164, replaces the `decode_rules_text`/`encode_rules_text` round trip):
/// the rules-text position is the typed stream itself (`card::TextTokenStream` is
/// `text::TextTokenStream`), its serde round trip is exact for a stream with every token kind and
/// every category, and the empty stream is the default.
#[test]
fn encode_decode_round_trip() {
    let s: card::TextTokenStream = every_kind();
    let back: card::TextTokenStream =
        serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    let empty: Stream = card::TextTokenStream::default();
    assert_eq!(empty, Stream::default());
    assert_eq!(serde_json::to_value(&empty).unwrap(), serde_json::json!([]));
}

/// REQ-DSL-012: the typed stream reads the FP-011 wire form built independently through the
/// MessagePack bridge, and serialises to exactly that wire form.
#[test]
fn decode_reads_the_wire_form() {
    let s = every_kind();
    let w = wire(s.tokens());
    assert_eq!(w, s.clone());
    let maps: Vec<BTreeMap<&'static str, String>> = s.tokens().iter().map(wire_token).collect();
    assert_eq!(
        serde_json::to_value(&s).unwrap(),
        serde_json::to_value(&maps).unwrap()
    );
    assert_eq!(try_json_stream(&maps), Some(s));
}

/// REQ-DSL-012 (#232 FP-164, replaces "placeholders that are not a valid stream are
/// InvalidRulesText"): the same invalid placeholders cannot be decoded as the typed stream, so
/// they can no longer reach a text effect.
#[test]
fn decode_invalid_placeholders() {
    assert!(
        serde_json::from_value::<card::TextTokenStream>(serde_json::json!(["test.word"])).is_err(),
        "a JSON string"
    );
    let word_map = |value: &str, category: &str| {
        let mut m = BTreeMap::new();
        m.insert("id", "a".to_string());
        m.insert("kind", "WORD".to_string());
        m.insert("category", category.to_string());
        m.insert("value", value.to_string());
        m
    };
    let self_with_name = {
        let mut m = BTreeMap::new();
        m.insert("id", "a".to_string());
        m.insert("kind", "NAME_REF".to_string());
        m.insert("ref", "SELF".to_string());
        m.insert("name", "test.x".to_string());
        m
    };
    let a = wire_token(&lit_t("a", "x"));
    let cases: Vec<(&str, Vec<BTreeMap<&'static str, String>>)> = vec![
        ("duplicate ids", vec![a.clone(), a]),
        ("empty value", vec![word_map("", "COLOR_WORD")]),
        ("unknown category", vec![word_map("Red", "test.category")]),
        ("SELF with a name", vec![self_with_name]),
    ];
    for (what, maps) in &cases {
        assert_eq!(try_json_stream(maps), None, "{what}: JSON decode accepted");
        let bytes = rmp_serde::to_vec_named(maps).unwrap();
        assert!(
            rmp_serde::from_slice::<card::TextTokenStream>(&bytes).is_err(),
            "{what}: MessagePack decode accepted"
        );
    }
}

/// REQ-DSL-012: every TextWordError has a non-empty Display and is a std error.
#[test]
fn error_display_non_empty() {
    let all = [
        TextWordError::UnknownToken("t1".into()),
        TextWordError::NotAWord("t1".into()),
        TextWordError::InvalidWord,
        TextWordError::UnsupportedCategory,
    ];
    for e in &all {
        assert!(!e.to_string().is_empty(), "{e:?}");
        let _: &dyn std::error::Error = e;
        assert_eq!(e.clone(), *e);
    }
}
