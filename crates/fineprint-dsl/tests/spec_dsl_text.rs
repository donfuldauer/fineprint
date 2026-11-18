// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::text` (FP-011 wire contract): the `face.text` token stream of
//! SRS §14.5 (literal text, rewritable words, name references). REQ-DSL-012, REQ-NFR-006.
//! Black-box: written from the work-item brief only. Synthetic content only (R5).

use std::collections::BTreeSet;
use std::fmt::Debug;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use fineprint_dsl::text::{TextError, TextToken, TextTokenStream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

const CATEGORIES: [&str; 5] = [
    "COLOR_WORD",
    "BASIC_LAND_TYPE",
    "CREATURE_TYPE",
    "NUMBER",
    "CARD_NAME",
];

const WORDS: [&str; 8] = [
    "Hue",
    "Moor",
    "Wisp",
    "tester",
    "Test Token A",
    "two",
    "caf\u{e9}",
    "with \"quotes\"",
];

// ---------------------------------------------------------------- helpers

fn decode<T: DeserializeOwned>(s: &str) -> T {
    match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    }
}

/// Decodes `s`, checks `to_value` equals the input value, canonical bytes equal
/// `canonicalize(s)`, and the round trips decode equal.
fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = decode(s);
    let original: Value = serde_json::from_str(s).unwrap();
    let tv = serde_json::to_value(&t).unwrap();
    assert_eq!(tv, original, "to_value of {s}");
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&t).unwrap()).unwrap(),
        String::from_utf8(canon.clone()).unwrap(),
        "canonical bytes of {s}"
    );
    let back: T = serde_json::from_value(tv).unwrap();
    assert_eq!(back, t);
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t);
    t
}

fn reject<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        assert!(
            serde_json::from_value::<T>(v).is_err(),
            "expected rejection (from_value) of {s}"
        );
    }
}

fn token(s: &str) -> TextToken {
    decode(s)
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

fn lit(id: &str, text: &str) -> String {
    json!({"id": id, "kind": "LITERAL", "text": text}).to_string()
}

fn word(id: &str, cat: &str, value: &str) -> String {
    json!({"id": id, "kind": "WORD", "category": cat, "value": value}).to_string()
}

fn self_ref(id: &str) -> String {
    json!({"id": id, "kind": "NAME_REF", "ref": "SELF"}).to_string()
}

fn other_ref(id: &str, name: &str) -> String {
    json!({"id": id, "kind": "NAME_REF", "ref": "OTHER", "name": name}).to_string()
}

fn stream(tokens: &[String]) -> String {
    format!("[{}]", tokens.join(","))
}

// ---------------------------------------------------------------- valid streams

/// REQ-DSL-012: the empty stream is valid.
#[test]
fn req_dsl_012_empty_stream() {
    let s = accept::<TextTokenStream>("[]");
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert!(s.tokens().is_empty());
    assert!(s.find("t0").is_none());
}

/// REQ-DSL-012: a literal-only stream round-trips exactly.
#[test]
fn req_dsl_012_literal_only_stream() {
    let s = accept::<TextTokenStream>(&stream(&[lit("t0", "Draw a tester.")]));
    assert_eq!(s.len(), 1);
    assert!(!s.is_empty());
    assert!(!s.tokens()[0].is_rewritable());
}

/// REQ-DSL-012: all five rewritable-word categories, NAME_REF SELF and OTHER, mixed, keep their
/// order and round-trip exactly.
#[test]
fn req_dsl_012_all_kinds_and_categories() {
    let mut toks = vec![
        lit("t0", "When "),
        self_ref("t1"),
        lit("t2", " enters, choose a "),
    ];
    for (i, c) in CATEGORIES.iter().enumerate() {
        toks.push(word(&format!("w{i}"), c, WORDS[i]));
    }
    toks.push(other_ref("t9", "Test Token A"));
    let s = accept::<TextTokenStream>(&stream(&toks));
    assert_eq!(s.len(), toks.len());
    let ids: Vec<&str> = s.tokens().iter().map(TextToken::id).collect();
    assert_eq!(ids, ["t0", "t1", "t2", "w0", "w1", "w2", "w3", "w4", "t9"]);
}

/// REQ-DSL-012: order is preserved, not normalised: a reversed stream is a different value
/// that re-serialises in its own order.
#[test]
fn req_dsl_012_order_preserved() {
    let toks = vec![
        lit("a", "Hue "),
        word("b", "COLOR_WORD", "Hue"),
        self_ref("c"),
        other_ref("d", "Wisp"),
    ];
    let mut rev = toks.clone();
    rev.reverse();
    let fwd = accept::<TextTokenStream>(&stream(&toks));
    let bwd = accept::<TextTokenStream>(&stream(&rev));
    assert_ne!(fwd, bwd);
    let ids: Vec<&str> = bwd.tokens().iter().map(TextToken::id).collect();
    assert_eq!(ids, ["d", "c", "b", "a"]);
}

/// REQ-DSL-012: individual tokens round-trip; NUMBER words carry their value as a string.
#[test]
fn req_dsl_012_single_tokens() {
    for c in CATEGORIES {
        accept::<TextToken>(&word("w", c, "Moor"));
    }
    accept::<TextToken>(&word("n", "NUMBER", "2"));
    accept::<TextToken>(&lit("l", "tester"));
    accept::<TextToken>(&self_ref("s"));
    accept::<TextToken>(&other_ref("o", "Test Token A"));
    // Member order in the input does not matter.
    let a: TextToken = decode(r#"{"value":"Hue","category":"COLOR_WORD","kind":"WORD","id":"x"}"#);
    let b: TextToken = decode(&word("x", "COLOR_WORD", "Hue"));
    assert_eq!(a, b);
}

// ---------------------------------------------------------------- rejections

/// REQ-DSL-012: duplicate ids are rejected, including between different token kinds.
#[test]
fn req_dsl_012_duplicate_ids_rejected() {
    let cases = [
        vec![lit("t1", "a"), lit("t1", "b")],
        vec![word("t1", "COLOR_WORD", "Hue"), lit("t1", "b")],
        vec![lit("t1", "b"), word("t1", "COLOR_WORD", "Hue")],
        vec![self_ref("t1"), other_ref("t1", "Wisp")],
        vec![
            lit("a", "x"),
            word("b", "CREATURE_TYPE", "Wisp"),
            self_ref("c"),
            word("a", "NUMBER", "2"),
        ],
    ];
    for c in cases {
        reject::<TextTokenStream>(&stream(&c));
    }
}

/// REQ-DSL-012: malformed tokens and streams are rejected.
#[test]
fn req_dsl_012_token_rejections() {
    let bad_tokens = [
        r#"{"id":"","kind":"LITERAL","text":"a"}"#,
        r#"{"id":null,"kind":"LITERAL","text":"a"}"#,
        r#"{"kind":"LITERAL","text":"a"}"#,
        r#"{"id":"a","text":"a"}"#,
        r#"{"id":"a","kind":"TEXT","text":"a"}"#,
        r#"{"id":"a","kind":"literal","text":"a"}"#,
        r#"{"id":"a","kind":null,"text":"a"}"#,
        r#"{"id":"a","kind":"LITERAL"}"#,
        r#"{"id":"a","kind":"LITERAL","text":""}"#,
        r#"{"id":"a","kind":"LITERAL","text":null}"#,
        r#"{"id":"a","kind":"LITERAL","text":"a","extra":1}"#,
        r#"{"id":"a","kind":"LITERAL","text":"a","category":"COLOR_WORD"}"#,
        r#"{"id":"a","kind":"LITERAL","value":"a"}"#,
        r#"{"id":"a","kind":"WORD","value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":null,"value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR","value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"color_word","value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"Color_Word","value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"","value":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR_WORD"}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR_WORD","value":""}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR_WORD","value":null}"#,
        r#"{"id":"a","kind":"WORD","category":"NUMBER","value":2}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR_WORD","value":"Hue","text":"Hue"}"#,
        r#"{"id":"a","kind":"WORD","category":"COLOR_WORD","text":"Hue"}"#,
        r#"{"id":"a","kind":"word","category":"COLOR_WORD","value":"Hue"}"#,
        r#"{"id":"a","kind":"NAME_REF"}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"SELF","name":"Wisp"}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"SELF","name":null}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"OTHER"}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"OTHER","name":""}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"OTHER","name":null}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"self"}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"THIS"}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":null}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"SELF","extra":1}"#,
        r#"{"id":"a","kind":"NAME_REF","ref":"OTHER","name":"Wisp","category":"CARD_NAME"}"#,
        r#"{"id":"a","kind":"name_ref","ref":"SELF"}"#,
        r#"{}"#,
        r#""a""#,
        r#"null"#,
    ];
    for t in bad_tokens {
        reject::<TextToken>(t);
        reject::<TextTokenStream>(&format!("[{t}]"));
        reject::<TextTokenStream>(&format!("[{},{t}]", lit("ok", "fine")));
    }
}

/// REQ-DSL-012: a stream must be a JSON array.
#[test]
fn req_dsl_012_non_array_stream_rejected() {
    for s in [
        "{}",
        "null",
        "\"t\"",
        "1",
        r#"{"tokens":[]}"#,
        r#"{"id":"a","kind":"LITERAL","text":"a"}"#,
        "[null]",
        "[[]]",
    ] {
        reject::<TextTokenStream>(s);
    }
}

// ---------------------------------------------------------------- Rust API

/// REQ-DSL-012: `TextTokenStream::new` accepts unique ids and rejects duplicates exactly as
/// deserialisation does; `tokens()` keeps order; `find` and `id` agree.
#[test]
fn req_dsl_012_api_new_find_tokens() {
    let src = vec![
        lit("t0", "Choose "),
        word("t1", "CREATURE_TYPE", "Wisp"),
        self_ref("t2"),
        other_ref("t3", "Test Token A"),
        word("t4", "NUMBER", "3"),
    ];
    let toks: Vec<TextToken> = src.iter().map(|s| token(s)).collect();
    let r: Result<TextTokenStream, TextError> = TextTokenStream::new(toks.clone());
    let s = r.unwrap();
    assert_eq!(s.tokens(), &toks[..]);
    assert_eq!(s.len(), 5);
    assert!(!s.is_empty());
    let decoded: TextTokenStream = decode(&stream(&src));
    assert_eq!(decoded.tokens(), s.tokens());
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&s).unwrap()
    );
    for t in &toks {
        assert_eq!(s.find(t.id()), Some(t));
    }
    assert_eq!(toks[0].id(), "t0");
    assert_eq!(toks[3].id(), "t3");
    assert!(s.find("t5").is_none());
    assert!(s.find("").is_none());
    assert!(s.find("T0").is_none());

    // Reversed order is kept by `new` as well.
    let mut rev = toks.clone();
    rev.reverse();
    let sr = TextTokenStream::new(rev.clone()).unwrap();
    assert_eq!(sr.tokens(), &rev[..]);

    // Empty stream.
    let e = TextTokenStream::new(Vec::new()).unwrap();
    assert!(e.is_empty());
    assert_eq!(e.len(), 0);

    // Duplicates, including word vs literal.
    for dup in [
        vec![lit("d", "x"), word("d", "COLOR_WORD", "Hue")],
        vec![self_ref("d"), lit("e", "y"), other_ref("d", "Moor")],
    ] {
        let v: Vec<TextToken> = dup.iter().map(|s| token(s)).collect();
        let r: Result<TextTokenStream, TextError> = TextTokenStream::new(v);
        assert!(r.is_err(), "duplicate ids accepted by new");
        reject::<TextTokenStream>(&stream(&dup));
    }
}

/// REQ-DSL-012: only WORD tokens are rewritable; literals and both NAME_REF forms are not
/// (self-references are never rewritten).
#[test]
fn req_dsl_012_is_rewritable() {
    for c in CATEGORIES {
        assert!(token(&word("w", c, "Hue")).is_rewritable(), "{c}");
    }
    assert!(!token(&lit("l", "Hue")).is_rewritable());
    assert!(!token(&self_ref("s")).is_rewritable());
    assert!(!token(&other_ref("o", "Hue")).is_rewritable());
}

// ---------------------------------------------------------------- properties and fuzzing

fn gen_text(rng: &mut Pcg64Dxsm) -> String {
    const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,~";
    if below(rng, 2) == 0 {
        WORDS[below(rng, WORDS.len() as u64) as usize].to_string()
    } else {
        let n = 1 + below(rng, 10) as usize;
        (0..n)
            .map(|_| ALNUM[below(rng, ALNUM.len() as u64) as usize] as char)
            .collect()
    }
}

/// A random valid stream as a JSON value, with unique ids.
fn gen_stream(rng: &mut Pcg64Dxsm) -> Value {
    let n = below(rng, 12);
    let mut used = BTreeSet::new();
    let mut out = Vec::new();
    for _ in 0..n {
        let id = loop {
            let id = if below(rng, 2) == 0 {
                format!("t{}", below(rng, 40))
            } else {
                gen_text(rng)
            };
            if used.insert(id.clone()) {
                break id;
            }
        };
        let mut m = Map::new();
        m.insert("id".into(), json!(id));
        match below(rng, 4) {
            0 => {
                m.insert("kind".into(), json!("LITERAL"));
                m.insert("text".into(), json!(gen_text(rng)));
            }
            1 => {
                m.insert("kind".into(), json!("WORD"));
                m.insert("category".into(), json!(CATEGORIES[below(rng, 5) as usize]));
                m.insert("value".into(), json!(gen_text(rng)));
            }
            2 => {
                m.insert("kind".into(), json!("NAME_REF"));
                m.insert("ref".into(), json!("SELF"));
            }
            _ => {
                m.insert("kind".into(), json!("NAME_REF"));
                m.insert("ref".into(), json!("OTHER"));
                m.insert("name".into(), json!(gen_text(rng)));
            }
        }
        out.push(Value::Object(m));
    }
    Value::Array(out)
}

fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(&String, &Value)> = m.iter().collect();
            rng.shuffle(&mut entries);
            out.push('{');
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(" : ");
                write_shuffled(val, rng, out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push_str("[ ");
            for (i, val) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                write_shuffled(val, rng, out);
            }
            out.push_str(" ]");
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

/// REQ-DSL-012, REQ-NFR-006: 600 random valid streams (seed 0x0F11_0000_0000_0030) round-trip,
/// have one canonical spelling (repeat-stable, equal to `canonicalize`), and decode equal from
/// a member-shuffled spelling; `find` locates every token.
#[test]
fn req_dsl_012_property_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0030);
    for _ in 0..600 {
        let v = gen_stream(&mut rng);
        let text = serde_json::to_string(&v).unwrap();
        let s: TextTokenStream = decode(&text);
        let sv = serde_json::to_value(&s).unwrap();
        assert_eq!(sv, v);
        let back: TextTokenStream = serde_json::from_value(sv).unwrap();
        assert_eq!(back, s);
        let c0 = to_canonical_vec(&s).unwrap();
        for _ in 0..10 {
            assert_eq!(to_canonical_vec(&s).unwrap(), c0);
        }
        assert_eq!(c0, canonicalize(&serde_json::to_vec(&s).unwrap()).unwrap());
        assert_eq!(c0, canonicalize(text.as_bytes()).unwrap());
        let mut sh = String::new();
        write_shuffled(&v, &mut rng, &mut sh);
        let s2: TextTokenStream = decode(&sh);
        assert_eq!(s2, s, "{sh}");
        assert_eq!(to_canonical_vec(&s2).unwrap(), c0);
        assert_eq!(s.len(), v.as_array().unwrap().len());
        for t in s.tokens() {
            assert_eq!(s.find(t.id()).map(TextToken::id), Some(t.id()));
        }
        let rebuilt = TextTokenStream::new(s.tokens().to_vec()).unwrap();
        assert_eq!(rebuilt.tokens(), s.tokens());
    }
}

fn roundtrip_if_ok(bytes: &[u8]) {
    if let Ok(s) = serde_json::from_slice::<TextTokenStream>(bytes) {
        let enc = serde_json::to_vec(&s).unwrap();
        let s2: TextTokenStream = serde_json::from_slice(&enc).unwrap();
        assert_eq!(s2, s);
        assert!(TextTokenStream::new(s.tokens().to_vec()).is_ok());
    }
    if let Ok(t) = serde_json::from_slice::<TextToken>(bytes) {
        let enc = serde_json::to_vec(&t).unwrap();
        let t2: TextToken = serde_json::from_slice(&enc).unwrap();
        assert_eq!(t2, t);
    }
}

/// REQ-NFR-006 (R12): 3000 random byte strings (seed 0x0F11_0000_0000_0031) never panic.
#[test]
fn req_nfr_006_text_random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"{}[]\":,0123456789- idkindLITERALWORDtextNAME_REFSELFOTHER\\";
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0031);
    for _ in 0..3000 {
        let len = below(&mut rng, 80) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        roundtrip_if_ok(&bytes);
    }
}

/// REQ-NFR-006 (R12): 3000 valid streams with one byte flipped, inserted or deleted (seed
/// 0x0F11_0000_0000_0032) never panic; accepted mutants round-trip.
#[test]
fn req_nfr_006_text_mutations_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0032);
    for _ in 0..3000 {
        let mut bytes = serde_json::to_vec(&gen_stream(&mut rng)).unwrap();
        match below(&mut rng, 3) {
            0 if !bytes.is_empty() => {
                let i = below(&mut rng, bytes.len() as u64) as usize;
                bytes[i] ^= 1 << below(&mut rng, 8);
            }
            2 if !bytes.is_empty() => {
                let i = below(&mut rng, bytes.len() as u64) as usize;
                bytes.remove(i);
            }
            _ => {
                let i = below(&mut rng, bytes.len() as u64 + 1) as usize;
                bytes.insert(i, rng.next_u64() as u8);
            }
        }
        roundtrip_if_ok(&bytes);
    }
}

/// REQ-NFR-006: raw-input robustness check. Deeply nested or unclosed bracket text is an error
/// (no panic, no stack overflow). It does not by itself prove the DSL depth guard: these inputs
/// are also rejected on shape and by the text parser's own nesting limit.
#[test]
fn req_nfr_006_text_deep_raw_input_is_error() {
    for depth in [128usize, 200, 100_000] {
        let s = "[".repeat(depth) + &"]".repeat(depth);
        assert!(serde_json::from_str::<TextTokenStream>(&s).is_err());
        assert!(serde_json::from_str::<TextTokenStream>(&"[".repeat(depth)).is_err());
    }
}

/// REQ-NFR-006: deeply nested arrays handed over as a borrowed `serde_json::Value` are an error
/// and never panic or overflow. None of them is a valid stream (a stream is an array of token
/// objects, at most 2 containers deep), so the discriminating 127/128 depth assertions live in
/// spec_dsl_expr.rs (`req_nfr_006_borrowed_value_depth_guard`).
#[test]
fn req_nfr_006_text_deep_borrowed_value_is_error() {
    for depth in [127usize, 128, 129, 1000] {
        let mut value = json!([]);
        for _ in 1..depth {
            value = Value::Array(vec![value]);
        }
        let r = <TextTokenStream as serde::Deserialize>::deserialize(&value);
        assert!(r.is_err(), "depth {depth} was accepted");
    }
}
