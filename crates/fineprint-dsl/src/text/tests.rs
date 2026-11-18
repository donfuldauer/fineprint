// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit and property tests for text token streams (REQ-DSL-012, REQ-NFR-006, REQ-NFR-008).
//! Every token is synthetic (R5).

use fineprint_base::prng::Pcg64Dxsm;

use super::*;
use crate::expr::tests::{
    accept, assert_wire_props, check_wire_enum, decode_never_panics, gen_str, mutate, pick,
    random_doc, reject,
};

/// A literal token.
fn lit(id: &str, text: &str) -> TextToken {
    TextToken::Literal {
        id: id.into(),
        text: text.into(),
    }
}

/// A word token.
fn word(id: &str, category: RewritableCategory, value: &str) -> TextToken {
    TextToken::Word {
        id: id.into(),
        category,
        value: value.into(),
    }
}

/// A name reference token.
fn name_ref(id: &str, kind: NameRefKind, name: Option<&str>) -> TextToken {
    TextToken::NameRef {
        id: id.into(),
        kind,
        name: name.map(Into::into),
    }
}

/// A generated valid stream with unique ids.
fn gen_stream(rng: &mut Pcg64Dxsm) -> TextTokenStream {
    let n = pick(rng, 8);
    let tokens = (0..n)
        .map(|i| {
            let id = format!("t{i}");
            match pick(rng, 4) {
                0 => lit(&id, &gen_str(rng)),
                1 => {
                    let c = RewritableCategory::ALL[pick(rng, RewritableCategory::ALL.len())];
                    word(&id, c, &gen_str(rng))
                }
                2 => name_ref(&id, NameRefKind::SelfRef, None),
                _ => name_ref(&id, NameRefKind::Other, Some(&gen_str(rng))),
            }
        })
        .collect();
    TextTokenStream::new(tokens).expect("valid stream")
}

/// REQ-DSL-012: the WORD category set is exactly the five SRS categories; NameRefKind is
/// SELF/OTHER.
#[test]
fn req_dsl_012_category_set_exact() {
    check_wire_enum(
        RewritableCategory::ALL,
        RewritableCategory::as_str,
        RewritableCategory::from_wire,
    );
    let names: Vec<_> = RewritableCategory::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "COLOR_WORD",
            "BASIC_LAND_TYPE",
            "CREATURE_TYPE",
            "NUMBER",
            "CARD_NAME"
        ]
    );
    check_wire_enum(
        NameRefKind::ALL,
        NameRefKind::as_str,
        NameRefKind::from_wire,
    );
    reject::<RewritableCategory>(r#""SUBTYPE""#);
    reject::<TextTokenStream>(r#"[{"id":"t1","kind":"WORD","category":"SUBTYPE","value":"v"}]"#);
}

/// REQ-DSL-012: a stream with every token kind round-trips with order preserved; `tokens`,
/// `find`, `len`, `is_empty` and `id` behave.
#[test]
fn req_dsl_012_stream_round_trip_and_api() {
    let doc = r#"[{"id":"t1","kind":"NAME_REF","ref":"SELF"},{"id":"t2","kind":"LITERAL","text":" gets more for each "},{"id":"t3","kind":"WORD","category":"CREATURE_TYPE","value":"test_type"},{"id":"t4","kind":"NAME_REF","ref":"OTHER","name":"test.other_card"},{"id":"t5","kind":"WORD","category":"NUMBER","value":"2"}]"#;
    let s: TextTokenStream = accept(doc);
    assert_eq!(s.len(), 5);
    assert!(!s.is_empty());
    let ids: Vec<_> = s.tokens().iter().map(TextToken::id).collect();
    assert_eq!(ids, ["t1", "t2", "t3", "t4", "t5"]);
    assert_eq!(
        s.find("t3"),
        Some(&word("t3", RewritableCategory::CreatureType, "test_type"))
    );
    assert_eq!(
        s.find("t5"),
        Some(&word("t5", RewritableCategory::Number, "2"))
    );
    assert_eq!(
        s.find("t4"),
        Some(&name_ref("t4", NameRefKind::Other, Some("test.other_card")))
    );
    assert_eq!(s.find("t9"), None);
    assert_eq!(s.find(""), None);
    // Order is significant.
    let mut reversed = s.tokens().to_vec();
    reversed.reverse();
    let r = TextTokenStream::new(reversed).expect("valid");
    assert_ne!(r, s);
    assert_eq!(r.tokens()[0].id(), "t5");
    // A NUMBER value is a string, never a JSON number.
    reject::<TextTokenStream>(r#"[{"id":"t1","kind":"WORD","category":"NUMBER","value":2}]"#);
}

/// REQ-DSL-012: the empty stream is valid.
#[test]
fn req_dsl_012_empty_stream() {
    let s: TextTokenStream = accept("[]");
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s, TextTokenStream::default());
    assert_eq!(
        TextTokenStream::new(Vec::new()),
        Ok(TextTokenStream::default())
    );
}

/// REQ-DSL-012, SRS §14.5 (CR 612, CR 201.5): only WORD tokens are rewritable; literal text and
/// name references, including SELF, never are.
#[test]
fn req_dsl_012_is_rewritable_only_for_words() {
    for c in RewritableCategory::ALL {
        assert!(word("w", *c, "v").is_rewritable());
    }
    assert!(!lit("l", "text").is_rewritable());
    assert!(!name_ref("s", NameRefKind::SelfRef, None).is_rewritable());
    assert!(!name_ref("o", NameRefKind::Other, Some("n")).is_rewritable());
}

/// REQ-DSL-012: duplicate ids are rejected by `new` and by deserialisation.
#[test]
fn req_dsl_012_duplicate_id_rejected() {
    assert_eq!(
        TextTokenStream::new(vec![lit("t1", "a"), lit("t1", "b")]),
        Err(TextError::DuplicateId { id: "t1".into() })
    );
    reject::<TextTokenStream>(
        r#"[{"id":"t1","kind":"LITERAL","text":"a"},{"id":"t1","kind":"NAME_REF","ref":"SELF"}]"#,
    );
}

/// REQ-DSL-012: NAME_REF SELF with a name, and OTHER without one, are rejected by `new` and by
/// deserialisation.
#[test]
fn req_dsl_012_name_ref_rules() {
    assert_eq!(
        TextTokenStream::new(vec![name_ref("s", NameRefKind::SelfRef, Some("n"))]),
        Err(TextError::SelfWithName { id: "s".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![name_ref("o", NameRefKind::Other, None)]),
        Err(TextError::OtherWithoutName { id: "o".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![name_ref("o", NameRefKind::Other, Some(""))]),
        Err(TextError::OtherWithoutName { id: "o".into() })
    );
    reject::<TextTokenStream>(r#"[{"id":"s","kind":"NAME_REF","ref":"SELF","name":"n"}]"#);
    reject::<TextTokenStream>(r#"[{"id":"o","kind":"NAME_REF","ref":"OTHER"}]"#);
    reject::<TextTokenStream>(r#"[{"id":"o","kind":"NAME_REF","ref":"OTHER","name":""}]"#);
    reject::<TextTokenStream>(r#"[{"id":"o","kind":"NAME_REF","ref":"OTHER","name":null}]"#);
    reject::<TextTokenStream>(r#"[{"id":"o","kind":"NAME_REF","ref":"self"}]"#);
    reject::<TextTokenStream>(r#"[{"id":"o","kind":"NAME_REF"}]"#);
}

/// REQ-DSL-012: every other validation rule of `new`: empty id, empty text, empty value,
/// noncharacters; `TextError` displays a message.
#[test]
fn req_dsl_012_new_validation() {
    assert_eq!(
        TextTokenStream::new(vec![lit("a", "x"), lit("", "x")]),
        Err(TextError::EmptyId { index: 1 })
    );
    assert_eq!(
        TextTokenStream::new(vec![lit("a", "")]),
        Err(TextError::EmptyText { id: "a".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![word("a", RewritableCategory::ColorWord, "")]),
        Err(TextError::EmptyValue { id: "a".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![lit("\u{fdd0}", "x")]),
        Err(TextError::Noncharacter { id: "0".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![lit("a", "\u{fffe}")]),
        Err(TextError::Noncharacter { id: "a".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![word("a", RewritableCategory::CardName, "\u{ffff}")]),
        Err(TextError::Noncharacter { id: "a".into() })
    );
    assert_eq!(
        TextTokenStream::new(vec![name_ref("a", NameRefKind::Other, Some("\u{fdd1}"))]),
        Err(TextError::Noncharacter { id: "a".into() })
    );
    let errors = [
        TextError::EmptyId { index: 0 },
        TextError::DuplicateId { id: "a".into() },
        TextError::EmptyText { id: "a".into() },
        TextError::EmptyValue { id: "a".into() },
        TextError::SelfWithName { id: "a".into() },
        TextError::OtherWithoutName { id: "a".into() },
        TextError::Noncharacter { id: "a".into() },
    ];
    for e in errors {
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
    // A single invalid token refuses to serialise.
    assert!(serde_json::to_string(&lit("", "x")).is_err());
    assert!(serde_json::to_string(&name_ref("s", NameRefKind::SelfRef, Some("n"))).is_err());
}

/// REQ-DSL-012: token JSON rejections: unknown kind, missing/extra members, nulls, wrong types,
/// a stream that is not an array.
#[test]
fn req_dsl_012_token_rejections() {
    for doc in [
        r#"{"id":"t","kind":"LITERAL"}"#,
        r#"{"id":"t","kind":"LITERAL","text":""}"#,
        r#"{"id":"t","kind":"LITERAL","text":"x","value":"v"}"#,
        r#"{"id":"","kind":"LITERAL","text":"x"}"#,
        r#"{"kind":"LITERAL","text":"x"}"#,
        r#"{"id":1,"kind":"LITERAL","text":"x"}"#,
        r#"{"id":"t","kind":"literal","text":"x"}"#,
        r#"{"id":"t","kind":"KEYWORD","text":"x"}"#,
        r#"{"id":"t","kind":"WORD","value":"v"}"#,
        r#"{"id":"t","kind":"WORD","category":"COLOR_WORD"}"#,
        r#"{"id":"t","kind":"WORD","category":"COLOR_WORD","value":""}"#,
        r#"{"id":"t","kind":"WORD","category":"color_word","value":"v"}"#,
        r#"{"id":"t","kind":"WORD","category":"COLOR_WORD","value":"v","text":"x"}"#,
        r#"{"id":"t","kind":"NAME_REF","ref":"OTHER","name":"n","value":"v"}"#,
        r#"{"id":"t","kind":"LITERAL","text":"x","id":"u"}"#,
        r#"{"id":"t","kind":null,"text":"x"}"#,
        r#"[]"#,
        r#""t""#,
    ] {
        reject::<TextToken>(doc);
    }
    for doc in [
        r#"{}"#,
        r#"null"#,
        r#"[null]"#,
        r#"[{"id":"t","kind":"LITERAL","text":"x"},1]"#,
        r#""""#,
    ] {
        reject::<TextTokenStream>(doc);
    }
    let t: TextToken = accept(r#"{"id":"t","kind":"LITERAL","text":"x"}"#);
    assert_eq!(t, lit("t", "x"));
}

/// REQ-DSL-012, REQ-NFR-006: generated streams satisfy every wire invariant.
#[test]
fn req_dsl_012_property_streams() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0012_0001);
    for _ in 0..400 {
        let s = gen_stream(&mut rng);
        assert_wire_props(&s, &mut rng);
        for t in s.tokens() {
            assert_wire_props(t, &mut rng);
            assert_eq!(s.find(t.id()), Some(t));
        }
    }
}

/// REQ-DSL-012, R12: random bytes and mutated streams never panic the decoders; deep input is
/// an error.
#[test]
fn req_dsl_012_random_and_mutated_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f11_0012_0002);
    for _ in 0..2000 {
        let doc = random_doc(&mut rng);
        decode_never_panics::<TextTokenStream>(&doc);
        decode_never_panics::<TextToken>(&doc);
        decode_never_panics::<RewritableCategory>(&doc);
        decode_never_panics::<NameRefKind>(&doc);
    }
    for _ in 0..500 {
        let doc = serde_json::to_vec(&gen_stream(&mut rng)).expect("encode");
        for _ in 0..4 {
            let m = mutate(&doc, &mut rng);
            decode_never_panics::<TextTokenStream>(&m);
        }
    }
    reject::<TextTokenStream>(&"[".repeat(100_000));
    reject::<TextToken>(&r#"{"a":"#.repeat(100_000));
}
