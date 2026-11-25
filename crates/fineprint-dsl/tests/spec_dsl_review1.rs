// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Black-box regression tests for CodeRabbit review round 1 on FP-010: non-finite numbers,
//! duplicate members from `Serialize`, Unicode noncharacters, strict enums, lossless decoding
//! (explicit nulls, positional structs), nesting depth, number normalisation, mana validation and
//! mutation robustness. Covers REQ-DSL-001, REQ-DSL-002, REQ-OBJ-001, REQ-NFR-006 and
//! REQ-CORE-006. Every definition here is SYNTHETIC (R5).
//! #159 FP-140: AST positions hold valid real ASTs (`seal` and `from_json` reject anything else);
//! limits formerly pushed through free-form placeholders are reached with valid real content
//! (deep `MATH` chains, LITERAL tokens) or with raw document text, as each test notes.

use std::collections::BTreeMap;

use fineprint_base::hash::sha256;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{
    CanonicalError, MAX_SAFE_INTEGER, canonicalize, parse_checked, to_canonical_vec,
};
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardDefinitionError, CardFaceDSL, ColorBitmask,
    CommanderEligibilityDSL, FaceKind, LayoutBehavior, Legality, Origin, PartnerAbilityDSL,
    ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaParseError, ManaSymbol, Variable};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

// ---------------------------------------------------------------- synthetic definitions

fn mana(s: &str) -> ManaCostDSL {
    ManaCostDSL::parse(s).expect("valid synthetic mana cost")
}

fn bitmask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits <= 31")
}

fn plain_face(kind: FaceKind, name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: vec![],
        card_types: vec![],
        subtypes: vec![],
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: TextTokenStream(vec![]),
        abilities: vec![],
    }
}

fn minimal() -> CardDefinitionDSL {
    let mut face = plain_face(FaceKind::Main, "Test Vanilla");
    face.mana_cost = Some(mana("{1}{G}"));
    face.card_types = vec!["Creature".to_string()];
    face.power = Some(ValueExpressionAST(json!({"LITERAL": 1})));
    face.toughness = Some(ValueExpressionAST(json!({"LITERAL": 1})));
    let mut legalities = BTreeMap::new();
    legalities.insert("testformat".to_string(), Legality::Legal);
    CardDefinitionDSL {
        card_def_id: "test.vanilla_1_1".to_string(),
        dsl_version: "1.0".to_string(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0000".to_string(),
            origin: Origin::Parser,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_string(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout: LayoutBehavior::Normal,
        faces: vec![face],
        meld_partner: None,
        color_identity: bitmask(16),
        commander_eligibility: None,
        legalities,
        digital_only: false,
        related_tokens: None,
    }
}

/// Every optional member present on the definition and on face 0, absent on face 1.
fn rich() -> CardDefinitionDSL {
    let mut front = plain_face(FaceKind::Main, "Test Modal Front");
    front.mana_cost = Some(mana("{2}{W}{U/P}"));
    front.color_indicator = Some(bitmask(3));
    front.supertypes = vec!["Legendary".to_string()];
    front.card_types = vec!["Creature".to_string(), "Artifact".to_string()];
    front.subtypes = vec!["Testkin".to_string()];
    // #159 FP-140: real content replaces the former free-form placeholders (a nested expression
    // with a variable; a nested and a maximal-integer ability payload, now as KEYWORD params).
    front.power = Some(ValueExpressionAST(
        json!({"MATH": {"op": "ADD", "lhs": {"LITERAL": 1}, "rhs": "X"}}),
    ));
    front.toughness = Some(ValueExpressionAST(json!({"LITERAL": 3})));
    front.loyalty = Some(4);
    front.defense = Some(5);
    front.hand_modifier = Some(-1);
    front.life_modifier = Some(7);
    front.text = TextTokenStream(vec![
        json!({"id": "t1", "kind": "LITERAL", "text": "Test"}),
        json!({"id": "t2", "kind": "WORD", "category": "COLOR_WORD", "value": "Hue"}),
    ]);
    front.abilities = vec![
        AbilityAST(
            json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
            "params": {"nested": {"a": [1, 2, {"b": false}]}}}),
        ),
        AbilityAST(
            json!({"abilityId": "a2", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
            "params": {"cost": "{T}", "n": MAX_SAFE_INTEGER}}),
        ),
    ];
    let back = plain_face(FaceKind::Back, "Test Modal Back");
    let mut legalities = BTreeMap::new();
    legalities.insert("alpha".to_string(), Legality::Legal);
    legalities.insert("beta".to_string(), Legality::Banned);
    CardDefinitionDSL {
        card_def_id: "test.modal_pair".to_string(),
        dsl_version: "1.0".to_string(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "ab".repeat(32),
            origin: Origin::Mixed,
            parser_version: Some("0.1.0".to_string()),
            override_source: Some("test-override".to_string()),
            verified_by: Some(vec!["test.suite_a".to_string()]),
            rules_baseline: "test-baseline".to_string(),
        },
        support_status: SupportStatus::Partial,
        known_gaps: Some(vec!["test gap one".to_string()]),
        layout: LayoutBehavior::DfcModal,
        faces: vec![front, back],
        meld_partner: Some("test.meld_other".to_string()),
        color_identity: bitmask(0b10011),
        commander_eligibility: Some(CommanderEligibilityDSL {
            eligible: true,
            partner_abilities: vec![PartnerAbilityDSL(
                json!({"abilityId": "p1", "kind": "KEYWORD", "keyword": "PARTNER"}),
            )],
        }),
        legalities,
        digital_only: true,
        related_tokens: Some(vec!["test.token_1_1".to_string()]),
    }
}

/// `minimal()` with a LITERAL and a WORD token, so text-level limits can be reached through
/// valid real content (#159 FP-140).
fn with_text() -> CardDefinitionDSL {
    let mut d = minimal();
    d.faces[0].text = TextTokenStream(vec![
        json!({"id": "t1", "kind": "LITERAL", "text": "Test Literal"}),
        json!({"id": "t2", "kind": "WORD", "category": "COLOR_WORD", "value": "Testhue"}),
    ]);
    d
}

fn sealed(d: CardDefinitionDSL) -> CardDefinitionDSL {
    d.seal().expect("seal")
}

fn canonical_bytes(d: &CardDefinitionDSL) -> Vec<u8> {
    d.to_canonical_json().expect("to_canonical_json")
}

fn to_value(d: &CardDefinitionDSL) -> Value {
    serde_json::to_value(d).expect("serialise")
}

fn load(bytes: &[u8]) -> Result<CardDefinitionDSL, CardDefinitionError> {
    CardDefinitionDSL::from_json(bytes)
}

fn load_value(v: &Value) -> Result<CardDefinitionDSL, CardDefinitionError> {
    load(&serde_json::to_vec(v).unwrap())
}

/// Recomputes `definitionHash` independently: SHA-256 over the RFC 8785 bytes of the document
/// without the member.
fn redigest(v: &mut Value) {
    let obj = v.as_object_mut().expect("object");
    obj.remove("definitionHash");
    let bytes = canonicalize(&serde_json::to_vec(&Value::Object(obj.clone())).unwrap())
        .expect("canonicalize patched document");
    obj.insert("definitionHash".to_string(), json!(sha256(&bytes).to_hex()));
}

fn expect_schema(r: Result<CardDefinitionDSL, CardDefinitionError>, what: &str) {
    match r {
        Err(CardDefinitionError::Schema(_)) => {}
        other => panic!("{what}: expected Schema error, got {other:?}"),
    }
}

/// A patched document must be a `Schema` error with the original digest and with a digest
/// recomputed over the patched document.
fn expect_schema_both(patched: &Value, what: &str) {
    expect_schema(load_value(patched), &format!("{what} (original digest)"));
    let mut re = patched.clone();
    redigest(&mut re);
    expect_schema(load_value(&re), &format!("{what} (recomputed digest)"));
}

fn replace(hay: &[u8], needle: &str, with: &str) -> Vec<u8> {
    String::from_utf8(hay.to_vec())
        .expect("utf-8")
        .replace(needle, with)
        .into_bytes()
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_u64() % n
}

fn nested(n: usize) -> Value {
    let mut v = Value::Array(vec![]);
    for _ in 1..n {
        v = Value::Array(vec![v]);
    }
    v
}

fn nested_text(n: usize) -> String {
    "[".repeat(n) + &"]".repeat(n)
}

// ---------------------------------------------------------------- T1 non-finite numbers

#[allow(clippy::disallowed_types)] // test-only: exercising the non-finite rejection
#[derive(Serialize)]
struct F64Field {
    x: f64,
}

#[allow(clippy::disallowed_types)] // test-only: exercising the non-finite rejection
#[derive(Serialize)]
struct F32Field {
    x: f32,
}

fn expect_serialize_err<T: Serialize>(v: &T, what: &str) {
    match to_canonical_vec(v) {
        Err(CanonicalError::Serialize(_)) => {}
        other => panic!("{what}: expected Serialize error, got {other:?}"),
    }
}

/// REQ-NFR-006, REQ-CORE-006: NaN and the infinities are rejected from every position.
#[allow(clippy::disallowed_types)] // test-only: exercising the non-finite rejection
#[test]
fn req_nfr_006_core_006_non_finite_numbers_rejected() {
    for (name, x) in [
        ("NAN", f64::NAN),
        ("INFINITY", f64::INFINITY),
        ("NEG_INFINITY", f64::NEG_INFINITY),
    ] {
        expect_serialize_err(&x, &format!("f64 {name} alone"));
        expect_serialize_err(&vec![x], &format!("f64 {name} in Vec"));
        expect_serialize_err(&vec![1.5f64, x], &format!("f64 {name} in Vec after finite"));
        expect_serialize_err(&F64Field { x }, &format!("f64 {name} struct field"));
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), 1.0f64);
        m.insert("b".to_string(), x);
        expect_serialize_err(&m, &format!("f64 {name} map value"));
    }
    let y = f32::NAN;
    expect_serialize_err(&y, "f32 NAN alone");
    expect_serialize_err(&vec![y], "f32 NAN in Vec");
    expect_serialize_err(&F32Field { x: y }, "f32 NAN struct field");
    let mut m = BTreeMap::new();
    m.insert("a".to_string(), y);
    expect_serialize_err(&m, "f32 NAN map value");

    assert_eq!(to_canonical_vec(&None::<i64>).unwrap(), b"null");
    assert_eq!(to_canonical_vec(&()).unwrap(), b"null");
    assert_eq!(to_canonical_vec(&1.5f64).unwrap(), b"1.5");
    assert_eq!(to_canonical_vec(&0i64).unwrap(), b"0");
    assert_eq!(
        to_canonical_vec(&F64Field { x: 1.5 }).unwrap(),
        b"{\"x\":1.5}"
    );
}

// ---------------------------------------------------------------- T2 duplicate members

struct DupMap;

impl Serialize for DupMap {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(2))?;
        m.serialize_entry("k", &1i64)?;
        m.serialize_entry("k", &2i64)?;
        m.end()
    }
}

struct DupStruct;

impl Serialize for DupStruct {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("DupStruct", 2)?;
        st.serialize_field("k", &1i64)?;
        st.serialize_field("k", &2i64)?;
        st.end()
    }
}

#[derive(Serialize)]
struct Plain {
    b: i64,
    a: i64,
}

fn expect_dup(r: Result<Vec<u8>, CanonicalError>, what: &str) {
    match r {
        Err(CanonicalError::DuplicateKey(k)) => assert_eq!(k, "k", "{what}"),
        other => panic!("{what}: expected DuplicateKey, got {other:?}"),
    }
}

/// REQ-NFR-006: duplicate member names produced by `Serialize` are rejected.
#[test]
fn req_nfr_006_serialize_duplicate_members_rejected() {
    expect_dup(to_canonical_vec(&DupMap), "SerializeMap");
    expect_dup(to_canonical_vec(&DupStruct), "SerializeStruct");
    expect_dup(to_canonical_vec(&vec![DupMap]), "map in array");
    expect_dup(to_canonical_vec(&vec![DupStruct]), "struct in array");
    expect_dup(
        to_canonical_vec(&vec![vec![DupMap]]),
        "map in array in array",
    );

    let mut m = BTreeMap::new();
    m.insert("b".to_string(), 2i64);
    m.insert("a".to_string(), 1i64);
    assert_eq!(to_canonical_vec(&m).unwrap(), b"{\"a\":1,\"b\":2}");
    assert_eq!(
        to_canonical_vec(&Plain { b: 2, a: 1 }).unwrap(),
        b"{\"a\":1,\"b\":2}"
    );
}

// ---------------------------------------------------------------- T3 noncharacters

/// First column: the JSON escape spelling (backslash-u text, surrogate pair above U+FFFF).
const NONCHARS: &[(&str, char)] = &[
    (r"\uFDD0", '\u{FDD0}'),
    (r"\uFDEF", '\u{FDEF}'),
    (r"\uFFFF", '\u{FFFF}'),
    (r"\uFFFE", '\u{FFFE}'),
    (r"\uD83F\uDFFE", '\u{1FFFE}'),
    (r"\uDBFF\uDFFF", '\u{10FFFF}'),
];

/// First column: the JSON escape spelling (backslash-u text, surrogate pair above U+FFFF).
const ALLOWED: &[(&str, char)] = &[
    (r"\u00E9", '\u{E9}'),
    (r"\uD83D\uDE02", '\u{1F602}'),
    (r"\uFDCF", '\u{FDCF}'),
    (r"\uFDF0", '\u{FDF0}'),
    (r"\uFFFD", '\u{FFFD}'),
    (r"\uE000", '\u{E000}'),
    (r"\uD800\uDC00", '\u{10000}'),
];

/// Documents placing `t` as a value (two member orders, and in an array) and as a member name
/// (both orders).
fn placements(t: &str) -> Vec<(String, &'static str)> {
    vec![
        (format!("{{\"a\":\"{t}\",\"b\":1}}"), "value a-first"),
        (format!("{{\"b\":1,\"a\":\"{t}\"}}"), "value b-first"),
        (format!("[\"{t}\"]"), "array element"),
        (format!("{{\"{t}\":1,\"b\":2}}"), "name first"),
        (format!("{{\"b\":2,\"{t}\":1}}"), "name last"),
    ]
}

fn expect_syntax<T: std::fmt::Debug>(r: Result<T, CanonicalError>, what: &str) {
    match r {
        Err(CanonicalError::Syntax(_)) => {}
        other => panic!("{what}: expected Syntax error, got {other:?}"),
    }
}

/// REQ-NFR-006: noncharacters (escaped or literal, any plane) are rejected in values and names.
#[test]
fn req_nfr_006_noncharacters_rejected_by_canonicalize_and_parse_checked() {
    for (esc, ch) in NONCHARS {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            for (doc, place) in placements(token) {
                let what = format!("U+{:04X} {token:?} {place}", *ch as u32);
                expect_syntax(canonicalize(doc.as_bytes()), &what);
                expect_syntax(parse_checked(doc.as_bytes()), &what);
            }
        }
    }
}

/// REQ-NFR-006: allowed code points round-trip to their literal UTF-8 in canonical bytes.
#[test]
fn req_nfr_006_near_noncharacters_accepted_with_canonical_bytes() {
    for (esc, ch) in ALLOWED {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            let v_expected = format!("{{\"a\":\"{ch}\",\"b\":1}}");
            let k_expected = format!("{{\"b\":2,\"{ch}\":1}}");
            for (doc, place) in placements(token) {
                let what = format!("U+{:04X} {token:?} {place}", *ch as u32);
                let expected = if place.starts_with("value") {
                    v_expected.clone()
                } else if place == "array element" {
                    format!("[\"{ch}\"]")
                } else {
                    k_expected.clone()
                };
                assert_eq!(
                    canonicalize(doc.as_bytes()).unwrap_or_else(|e| panic!("{what}: {e:?}")),
                    expected.as_bytes(),
                    "{what}"
                );
                assert!(parse_checked(doc.as_bytes()).is_ok(), "{what}");
            }
        }
    }
}

#[derive(Serialize)]
struct Holder {
    s: String,
}

/// REQ-NFR-006: `to_canonical_vec` rejects noncharacters in strings, fields and map keys, and
/// accepts the neighbouring code points.
#[test]
fn req_nfr_006_noncharacters_in_to_canonical_vec() {
    for (_, ch) in NONCHARS {
        let s = format!("x{ch}y");
        let what = format!("U+{:04X}", *ch as u32);
        expect_serialize_err(&s, &format!("{what} string"));
        expect_serialize_err(&ch.to_string(), &format!("{what} bare char string"));
        expect_serialize_err(&Holder { s: s.clone() }, &format!("{what} struct field"));
        expect_serialize_err(&vec![s.clone()], &format!("{what} in Vec"));
        let mut value_first = BTreeMap::new();
        value_first.insert("a".to_string(), s.clone());
        expect_serialize_err(&value_first, &format!("{what} map value"));
        let mut key_map = BTreeMap::new();
        key_map.insert("b".to_string(), 2i64);
        key_map.insert(s.clone(), 1i64);
        expect_serialize_err(&key_map, &format!("{what} map key"));
        let mut key_only = BTreeMap::new();
        key_only.insert(s, 1i64);
        expect_serialize_err(&key_only, &format!("{what} sole map key"));
    }
    for (_, ch) in ALLOWED {
        let s = format!("x{ch}y");
        assert_eq!(
            to_canonical_vec(&s).unwrap(),
            format!("\"{s}\"").as_bytes(),
            "string U+{:04X}",
            *ch as u32
        );
        assert_eq!(
            to_canonical_vec(&Holder { s: s.clone() }).unwrap(),
            format!("{{\"s\":\"{s}\"}}").as_bytes()
        );
        let mut key_map = BTreeMap::new();
        key_map.insert(s.clone(), 1i64);
        assert_eq!(
            to_canonical_vec(&key_map).unwrap(),
            format!("{{\"{s}\":1}}").as_bytes()
        );
    }
}

/// REQ-NFR-006: `from_json` rejects a noncharacter in a face name or in a member name as a
/// canonicalisation `Syntax` error, before any schema or hash check.
#[test]
fn req_nfr_006_noncharacters_rejected_by_from_json() {
    let base = canonical_bytes(&sealed(minimal()));
    for (esc, ch) in NONCHARS {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            let what = format!("U+{:04X} {token:?}", *ch as u32);
            let in_name = replace(&base, "Test Vanilla", &format!("T{token}V"));
            match load(&in_name) {
                Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_))) => {}
                other => panic!("{what} in face name: got {other:?}"),
            }
            let in_key = replace(
                &base,
                "{\"cardDefId\"",
                &format!("{{\"{token}\":1,\"cardDefId\""),
            );
            match load(&in_key) {
                Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_))) => {}
                other => panic!("{what} in member name: got {other:?}"),
            }
        }
    }
    for (esc, ch) in ALLOWED {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            let in_name = replace(&base, "Test Vanilla", &format!("T{token}V"));
            // The text changed, so the old digest no longer matches; it must get past syntax.
            match load(&in_name) {
                Err(CardDefinitionError::HashMismatch { .. }) => {}
                other => panic!("U+{:04X} {token:?}: got {other:?}", *ch as u32),
            }
        }
    }
}

/// REQ-NFR-006, REQ-DSL-012 (#159 FP-140): a noncharacter inside a LITERAL token's `text` or a
/// WORD token's `value` is a canonicalisation `Syntax` error on load and a `Serialize` error on
/// seal; the neighbouring allowed code points get past syntax and, re-digested, load.
#[test]
fn req_nfr_006_noncharacters_in_text_tokens() {
    let base = canonical_bytes(&sealed(with_text()));
    let places = ["Test Literal", "Testhue"];
    for (esc, ch) in NONCHARS {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            for place in places {
                let what = format!("U+{:04X} {token:?} in {place}", *ch as u32);
                let doc = replace(&base, place, &format!("T{token}V"));
                match load(&doc) {
                    Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_))) => {}
                    other => panic!("{what}: got {other:?}"),
                }
            }
        }
        for i in 0..2 {
            let mut d = with_text();
            let member = if i == 0 { "text" } else { "value" };
            d.faces[0].text.0[i][member] = json!(format!("T{ch}V"));
            assert!(
                is_serialize_err(d.seal()),
                "U+{:04X} seal token {i}",
                *ch as u32
            );
        }
    }
    for (esc, ch) in ALLOWED {
        let literal = ch.to_string();
        for token in [*esc, literal.as_str()] {
            for place in places {
                let what = format!("U+{:04X} {token:?} in {place}", *ch as u32);
                let doc = replace(&base, place, &format!("T{token}V"));
                match load(&doc) {
                    Err(CardDefinitionError::HashMismatch { .. }) => {}
                    other => panic!("{what}: got {other:?}"),
                }
                let mut v: Value = serde_json::from_slice(&doc).unwrap();
                redigest(&mut v);
                let loaded = load_value(&v).unwrap_or_else(|e| panic!("{what}: {e:?}"));
                assert_eq!(
                    load(&canonical_bytes(&loaded)),
                    Ok(loaded.clone()),
                    "{what}"
                );
                assert_eq!(sealed(loaded.clone()), loaded, "{what}");
            }
        }
    }
}

// ---------------------------------------------------------------- T4 enums

macro_rules! strict_enum {
    ($ty:ty) => {{
        for v in <$ty>::ALL.iter() {
            let s = v.as_str();
            let a: $ty = serde_json::from_value(json!(s)).unwrap();
            assert_eq!(a, *v);
            let bytes = serde_json::to_vec(&json!(s)).unwrap();
            let b: $ty = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(b, *v);
            let mut obj_null = serde_json::Map::new();
            obj_null.insert(s.to_string(), Value::Null);
            let mut obj_empty = serde_json::Map::new();
            obj_empty.insert(s.to_string(), json!({}));
            let bad: Vec<Value> = vec![
                Value::Object(obj_null),
                Value::Object(obj_empty),
                json!([s]),
                json!(null),
                json!(1),
                json!(true),
                json!(s.to_lowercase()),
                json!(format!("{s} ")),
                json!(""),
            ];
            for b in bad {
                assert!(
                    serde_json::from_value::<$ty>(b.clone()).is_err(),
                    "{}: {b} from_value",
                    stringify!($ty)
                );
                assert!(
                    serde_json::from_slice::<$ty>(&serde_json::to_vec(&b).unwrap()).is_err(),
                    "{}: {b} from_slice",
                    stringify!($ty)
                );
            }
        }
    }};
}

/// REQ-DSL-001: enums decode only from their exact plain string, never from a singleton object.
#[test]
fn req_dsl_001_enums_strict_decoding() {
    strict_enum!(LayoutBehavior);
    strict_enum!(FaceKind);
    strict_enum!(SupportStatus);
    strict_enum!(Origin);
    strict_enum!(Legality);
}

/// REQ-DSL-001, REQ-NFR-006: a singleton-object enum in a sealed document is a `Schema` error,
/// with the original digest and with an independently recomputed digest.
#[test]
fn req_dsl_001_from_json_singleton_object_enum_is_schema_error() {
    let base = to_value(&sealed(minimal()));
    for (ptr, name) in [
        ("/layout", "NORMAL"),
        ("/supportStatus", "AUTO_PARSED"),
        ("/source/origin", "PARSER"),
        ("/legalities/testformat", "LEGAL"),
        ("/faces/0/faceKind", "MAIN"),
    ] {
        let mut v = base.clone();
        let mut obj = serde_json::Map::new();
        obj.insert(name.to_string(), Value::Null);
        *v.pointer_mut(ptr).unwrap() = Value::Object(obj);
        expect_schema_both(&v, ptr);
    }
}

// ---------------------------------------------------------------- T5 explicit nulls

fn insert_null(v: &mut Value, parent: &str, member: &str) {
    v.pointer_mut(parent)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert(member.to_string(), Value::Null);
}

/// REQ-OBJ-001, REQ-DSL-002: an explicit `null` for an optional member is a `Schema` error.
#[test]
fn req_obj_001_dsl_002_explicit_null_optionals_rejected() {
    let rich_v = to_value(&sealed(rich()));
    let min_v = to_value(&sealed(minimal()));
    for member in [
        "manaCost",
        "colorIndicator",
        "loyalty",
        "defense",
        "handModifier",
        "lifeModifier",
    ] {
        assert!(rich_v.pointer("/faces/1").unwrap().get(member).is_none());
        let mut v = rich_v.clone();
        insert_null(&mut v, "/faces/1", member);
        expect_schema_both(&v, &format!("absent face.{member} -> null"));
        // Present member replaced by null.
        let mut v = rich_v.clone();
        insert_null(&mut v, "/faces/0", member);
        expect_schema_both(&v, &format!("present face.{member} -> null"));
    }
    for member in [
        "knownGaps",
        "meldPartner",
        "commanderEligibility",
        "relatedTokens",
    ] {
        assert!(min_v.get(member).is_none());
        let mut v = min_v.clone();
        insert_null(&mut v, "", member);
        expect_schema_both(&v, &format!("absent {member} -> null"));
        let mut v = rich_v.clone();
        insert_null(&mut v, "", member);
        expect_schema_both(&v, &format!("present {member} -> null"));
    }
    for member in ["parserVersion", "overrideSource", "verifiedBy"] {
        assert!(min_v.pointer("/source").unwrap().get(member).is_none());
        let mut v = min_v.clone();
        insert_null(&mut v, "/source", member);
        expect_schema_both(&v, &format!("absent source.{member} -> null"));
        let mut v = rich_v.clone();
        insert_null(&mut v, "/source", member);
        expect_schema_both(&v, &format!("present source.{member} -> null"));
    }
}

/// REQ-OBJ-001: omitted members load; `{0}` is a present one-symbol cost distinct from omitted;
/// the empty string is rejected.
#[test]
fn req_obj_001_mana_cost_omitted_zero_and_empty() {
    for d in [sealed(rich()), sealed(minimal())] {
        assert_eq!(load(&canonical_bytes(&d)).unwrap(), d);
    }
    let base = to_value(&sealed(rich()));
    let omitted = load_value(&base).unwrap();
    assert_eq!(omitted.faces[1].mana_cost, None);

    let mut v = base.clone();
    v["faces"][1]["manaCost"] = json!("{0}");
    assert!(matches!(
        load_value(&v),
        Err(CardDefinitionError::HashMismatch { .. })
    ));
    redigest(&mut v);
    let zero = load_value(&v).expect("{0} loads");
    let cost = zero.faces[1].mana_cost.as_ref().expect("present");
    assert_eq!(cost.symbols().len(), 1);
    assert_eq!(*cost, mana("{0}"));
    assert_ne!(zero, omitted);
    assert_ne!(zero.definition_hash, omitted.definition_hash);

    let mut v = base;
    v["faces"][1]["manaCost"] = json!("");
    assert!(load_value(&v).is_err());
    redigest(&mut v);
    assert!(load_value(&v).is_err());
}

// ---------------------------------------------------------------- T6 positional structs

const FACE_ORDER: &[&str] = &[
    "faceKind",
    "name",
    "manaCost",
    "colorIndicator",
    "supertypes",
    "cardTypes",
    "subtypes",
    "power",
    "toughness",
    "loyalty",
    "defense",
    "handModifier",
    "lifeModifier",
    "text",
    "abilities",
];
const SOURCE_ORDER: &[&str] = &[
    "oracleTextHash",
    "origin",
    "parserVersion",
    "overrideSource",
    "verifiedBy",
    "rulesBaseline",
];

fn positional(obj: &Value, order: &[&str]) -> Value {
    Value::Array(order.iter().filter_map(|k| obj.get(*k).cloned()).collect())
}

/// REQ-DSL-002: a positional array where a nested struct is required is a `Schema` error.
#[test]
fn req_dsl_002_positional_arrays_for_structs_rejected() {
    for base_def in [sealed(rich()), sealed(minimal())] {
        let base = to_value(&base_def);
        let mut v = base.clone();
        let arr = positional(&base["source"], SOURCE_ORDER);
        v["source"] = arr;
        expect_schema_both(&v, "source as array");
        for i in 0..base["faces"].as_array().unwrap().len() {
            let mut v = base.clone();
            let arr = positional(&base["faces"][i], FACE_ORDER);
            v["faces"][i] = arr;
            expect_schema_both(&v, &format!("face {i} as array"));
        }
    }
    let base = to_value(&sealed(rich()));
    let mut v = base.clone();
    v["commanderEligibility"] = positional(
        &base["commanderEligibility"],
        &["eligible", "partnerAbilities"],
    );
    assert_eq!(
        v["commanderEligibility"][0],
        json!(true),
        "oracle built the array"
    );
    expect_schema_both(&v, "commanderEligibility as array");
    let mut v = base;
    v["commanderEligibility"] = json!([true, []]);
    expect_schema_both(&v, "commanderEligibility as [true, []]");
}

fn emit_reversed(v: &Value, out: &mut String) {
    match v {
        Value::Array(items) => {
            out.push_str("[ ");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(" ,\n");
                }
                emit_reversed(item, out);
            }
            out.push_str(" ]");
        }
        Value::Object(map) => {
            out.push_str("{\n");
            for (i, (k, val)) in map.iter().rev().enumerate() {
                if i > 0 {
                    out.push_str(",\n\t");
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(" :\t");
                emit_reversed(val, out);
            }
            out.push_str("\r\n}");
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

/// REQ-DSL-002, REQ-NFR-006: whitespace and member order do not change the loaded value or hash.
#[test]
fn req_dsl_002_whitespace_and_member_order_do_not_matter() {
    for d in [sealed(rich()), sealed(minimal())] {
        let mut text = String::new();
        emit_reversed(&to_value(&d), &mut text);
        for form in [
            text.into_bytes(),
            serde_json::to_vec_pretty(&d).unwrap(),
            canonical_bytes(&d),
        ] {
            let loaded = load(&form).expect("loads");
            assert_eq!(loaded, d);
            assert_eq!(loaded.definition_hash, d.definition_hash);
            assert_eq!(loaded.compute_hash().unwrap(), d.definition_hash);
        }
    }
}

// ---------------------------------------------------------------- T7 depth

fn n_doc() -> usize {
    let mut best = 0;
    for n in 1..=400 {
        if canonicalize(nested_text(n).as_bytes()).is_ok() {
            best = n;
        }
    }
    best
}

/// REQ-NFR-006, REQ-DSL-001: the serialiser side rejects exactly what the parser side rejects.
#[test]
fn req_nfr_006_to_canonical_vec_depth_matches_canonicalize() {
    let n = n_doc();
    assert!((100..=400).contains(&n), "N_doc = {n}");
    assert_eq!(
        to_canonical_vec(&nested(n)).unwrap(),
        nested_text(n).as_bytes()
    );
    expect_serialize_err(&nested(n + 1), "one past the limit");
    expect_serialize_err(&nested(n + 20), "well past the limit");
}

/// A valid real value expression with exactly `k >= 1` nested containers, built iteratively:
/// `k / 2` MATH levels (two containers each) around a `{"LITERAL":1}` leaf (one container) when
/// `k` is odd, or an `"X"` leaf (none) when `k` is even.
fn math_chain(k: usize) -> Value {
    let mut v = if k % 2 == 1 {
        json!({"LITERAL": 1})
    } else {
        json!("X")
    };
    for _ in 0..k / 2 {
        v = json!({"MATH": {"op": "ADD", "lhs": v, "rhs": "X"}});
    }
    v
}

/// #159 FP-140: formerly `k` nested arrays as a free-form ability placeholder; now a valid MATH
/// chain of `k` containers in `power`, which reaches the same depth limit with real content.
fn depth_def(k: usize) -> CardDefinitionDSL {
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(math_chain(k)));
    d
}

/// REQ-DSL-001, REQ-NFR-006: `seal` is Ok exactly when it can be loaded back; no seal-Ok /
/// from_json-Err gap at any nesting depth, and the accepted depths form a prefix.
#[test]
fn req_dsl_001_nfr_006_seal_and_from_json_agree_on_depth() {
    let mut last_ok = 0usize;
    let mut first_err = None;
    for k in 1..=140usize {
        let s = depth_def(k).seal();
        let j = depth_def(k).to_canonical_json();
        let h = depth_def(k).compute_hash();
        assert_eq!(s.is_ok(), j.is_ok(), "k={k} seal vs to_canonical_json");
        assert_eq!(s.is_ok(), h.is_ok(), "k={k} seal vs compute_hash");
        match s {
            Ok(sealed_def) => {
                assert!(first_err.is_none(), "k={k} accepted after a rejection");
                last_ok = k;
                let bytes = sealed_def.to_canonical_json().expect("export");
                let back = load(&bytes).unwrap_or_else(|e| panic!("k={k}: seal Ok, load {e:?}"));
                assert_eq!(back, sealed_def, "k={k}");
            }
            Err(e) => {
                first_err.get_or_insert(k);
                assert!(matches!(e, CanonicalError::Serialize(_)), "k={k}: {e:?}");
            }
        }
    }
    assert!(last_ok >= 5, "k=5 must be accepted");
    assert!((100..=139).contains(&last_ok), "threshold k0 = {last_ok}");
    assert_eq!(first_err, Some(last_ok + 1));
    assert!(depth_def(5).seal().is_ok());
    assert!(matches!(
        depth_def(140).seal(),
        Err(CanonicalError::Serialize(_))
    ));
}

/// REQ-DSL-001, REQ-NFR-006: a document nested beyond the limit is a `Syntax` error.
#[test]
fn req_dsl_001_nfr_006_from_json_rejects_over_deep_documents() {
    let n = n_doc();
    let base = canonical_bytes(&sealed(minimal()));
    // Raw document text (#159 FP-140): the document-level depth check runs before the AST
    // check, so arrays in an AST position still report Canonical(Syntax).
    for depth in [n + 10, n + 1, 5000] {
        let doc = replace(
            &base,
            "\"power\":{\"LITERAL\":1}",
            &format!("\"power\":{}", nested_text(depth)),
        );
        match load(&doc) {
            Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_))) => {}
            other => panic!("power depth {depth}: got {other:?}"),
        }
    }
    for depth in [n + 10, n + 1, 5000] {
        let doc = replace(
            &base,
            "\"abilities\":[]",
            &format!("\"abilities\":[{}]", nested_text(depth)),
        );
        match load(&doc) {
            Err(CardDefinitionError::Canonical(CanonicalError::Syntax(_))) => {}
            other => panic!("depth {depth}: got {other:?}"),
        }
    }
    let unterminated = "[".repeat(100_000);
    expect_syntax(
        canonicalize(unterminated.as_bytes()),
        "100000 open brackets",
    );
    assert!(load(unterminated.as_bytes()).is_err());
}

// ---------------------------------------------------------------- T8 number normalisation

fn insert_hash(body_without_hash: &str) -> Vec<u8> {
    let canon = canonicalize(body_without_hash.as_bytes()).expect("canonicalize body");
    let h = sha256(&canon).to_hex();
    body_without_hash
        .replacen('{', &format!("{{\"definitionHash\":\"{h}\","), 1)
        .into_bytes()
}

/// REQ-DSL-002, REQ-NFR-006: numbers in AST positions normalise so that sealed equals loaded.
/// #159 FP-140: the numbers sit in a real LITERAL and in KEYWORD params (formerly bare
/// placeholders); `seal` normalises `-0.0`/`1.0` to `0`/`1` before the AST check, like the
/// loader's canonical step.
#[test]
fn req_dsl_002_nfr_006_number_normalisation_in_placeholders() {
    for (text, int) in [("-0.0", 0), ("1.0", 1)] {
        let n: Value = serde_json::from_str(text).unwrap();
        let mut d = minimal();
        d.faces[0].power = Some(ValueExpressionAST(json!({ "LITERAL": n.clone() })));
        let s = sealed(d);
        assert_eq!(s.verify_hash(), Ok(()), "{text}");
        assert_eq!(
            s.faces[0].power,
            Some(ValueExpressionAST(json!({ "LITERAL": int }))),
            "{text}"
        );
        let back = load(&canonical_bytes(&s)).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        assert_eq!(back, s, "{text}");
        assert_eq!(back.verify_hash(), Ok(()), "{text}");
        // Independent oracle for the sealed digest.
        let mut v = to_value(&s);
        v.as_object_mut().unwrap().remove("definitionHash");
        let oracle = sha256(&canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap()).to_hex();
        assert_eq!(s.definition_hash, oracle, "{text}");
        // Nested in a larger placeholder too.
        let mut d = minimal();
        d.faces[0].abilities = vec![AbilityAST(
            json!({"abilityId": "a1", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
                "params": {"k": [n.clone()]}}),
        )];
        let s = sealed(d);
        assert_eq!(load(&canonical_bytes(&s)).unwrap(), s, "{text} nested");
    }
}

/// REQ-DSL-002, REQ-NFR-006: `1.0` and `1` spell the same document; `-0.0` loads as `0`.
#[test]
fn req_dsl_002_nfr_006_one_point_zero_equals_one() {
    let s = sealed(minimal());
    let base = String::from_utf8(canonical_bytes(&s)).unwrap();
    let hash_member = format!("\"definitionHash\":\"{}\",", s.definition_hash);
    assert!(base.contains(&hash_member));
    let doc_int = base.clone();
    let doc_float = base.replace("\"power\":{\"LITERAL\":1},", "\"power\":{\"LITERAL\":1.0},");
    assert_ne!(doc_int, doc_float);
    for (doc, what) in [(&doc_int, "int"), (&doc_float, "float")] {
        let stripped = doc.replace(&hash_member, "");
        let oracle = sha256(&canonicalize(stripped.as_bytes()).unwrap()).to_hex();
        assert_eq!(oracle, s.definition_hash, "{what} oracle digest");
    }
    let a = load(doc_int.as_bytes()).expect("1 loads");
    let b = load(doc_float.as_bytes()).expect("1.0 loads");
    assert_eq!(a, b);
    assert_eq!(a, s);
    assert_eq!(canonical_bytes(&a), canonical_bytes(&b));
    assert_eq!(
        canonicalize(doc_int.as_bytes()).unwrap(),
        canonicalize(doc_float.as_bytes()).unwrap()
    );

    let body_neg = base.replace(&hash_member, "").replace(
        "\"power\":{\"LITERAL\":1},",
        "\"power\":{\"LITERAL\":-0.0},",
    );
    let doc_neg = insert_hash(&body_neg);
    let c = load(&doc_neg).expect("-0.0 loads");
    assert_eq!(
        c.faces[0].power,
        Some(ValueExpressionAST(json!({"LITERAL": 0})))
    );
    assert_eq!(c.verify_hash(), Ok(()));
    assert_ne!(c.definition_hash, s.definition_hash);
}

// ---------------------------------------------------------------- T9 mana validation

fn is_unknown(r: Result<ManaCostDSL, ManaParseError>) -> bool {
    matches!(r, Err(ManaParseError::UnknownSymbol { .. }))
}

/// REQ-OBJ-001: `ManaCostDSL::new` enforces the same symbol validity as the parser.
#[test]
fn req_obj_001_mana_cost_new_validates() {
    use ManaSymbol::*;
    assert!(is_unknown(ManaCostDSL::new(vec![GenericHybrid(
        0,
        Color::White
    )])));
    assert!(is_unknown(ManaCostDSL::new(vec![Generic(1_000_001)])));
    assert!(is_unknown(ManaCostDSL::new(vec![GenericHybrid(
        1_000_001,
        Color::Blue
    )])));
    assert!(is_unknown(ManaCostDSL::new(vec![Hybrid(
        Color::Red,
        Color::Red
    )])));
    assert!(is_unknown(ManaCostDSL::new(vec![HybridPhyrexian(
        Color::Green,
        Color::Green
    )])));
    match ManaCostDSL::new(vec![Generic(2), GenericHybrid(0, Color::White)]) {
        Err(ManaParseError::UnknownSymbol { offset, symbol }) => {
            assert_eq!(offset, 3);
            assert_eq!(symbol.to_string(), "{0/W}");
        }
        other => panic!("expected UnknownSymbol at offset 3, got {other:?}"),
    }
    assert!(matches!(
        ManaCostDSL::new(vec![]),
        Err(ManaParseError::Empty)
    ));

    let valid: Vec<ManaSymbol> = vec![
        Generic(0),
        GenericHybrid(1, Color::White),
        Generic(1_000_000),
        Hybrid(Color::White, Color::Blue),
        HybridPhyrexian(Color::White, Color::Blue),
        Phyrexian(Color::Red),
    ];
    for s in valid {
        let cost = ManaCostDSL::new(vec![s]).unwrap_or_else(|e| panic!("{s}: {e:?}"));
        let back = ManaCostDSL::parse(&cost.to_string()).unwrap_or_else(|e| panic!("{s}: {e:?}"));
        assert_eq!(back, cost, "{s}");
        let mut d = minimal();
        d.faces[0].mana_cost = Some(cost);
        let sd = sealed(d);
        assert_eq!(load(&canonical_bytes(&sd)).unwrap(), sd, "{s}");
    }
}

const COLORS: [Color; 5] = [
    Color::White,
    Color::Blue,
    Color::Black,
    Color::Red,
    Color::Green,
];

fn random_symbol(rng: &mut Pcg64Dxsm) -> ManaSymbol {
    const NUMS: [u32; 6] = [0, 1, 2, 999_999, 1_000_000, 1_000_001];
    let num = |rng: &mut Pcg64Dxsm| NUMS[below(rng, 6) as usize];
    let col = |rng: &mut Pcg64Dxsm| COLORS[below(rng, 5) as usize];
    match below(rng, 9) {
        0 => ManaSymbol::Colored(col(rng)),
        1 => ManaSymbol::Colorless,
        2 => ManaSymbol::Generic(num(rng)),
        3 => ManaSymbol::GenericHybrid(num(rng), col(rng)),
        4 => ManaSymbol::Hybrid(col(rng), col(rng)),
        5 => ManaSymbol::HybridPhyrexian(col(rng), col(rng)),
        6 => ManaSymbol::Phyrexian(col(rng)),
        7 => ManaSymbol::Snow,
        _ => ManaSymbol::Variable([Variable::X, Variable::Y, Variable::Z][below(rng, 3) as usize]),
    }
}

/// REQ-OBJ-001: a symbol is accepted by `new` exactly when its printed form parses.
#[test]
fn req_obj_001_new_and_parse_agree_property() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x00C0_FFEE_0000_0901);
    for _ in 0..500 {
        let s = random_symbol(&mut rng);
        assert_eq!(
            ManaSymbol::parse(&s.to_string()).is_ok(),
            ManaCostDSL::new(vec![s]).is_ok(),
            "{s:?} printed {s}"
        );
    }
}

// ---------------------------------------------------------------- T10 robustness

fn mutate(v: &mut Value, rng: &mut Pcg64Dxsm, wrap: bool) {
    let apply = |slot: &mut Value| {
        if wrap {
            let inner = std::mem::take(slot);
            *slot = Value::Array(vec![inner]);
        } else {
            *slot = Value::Null;
        }
    };
    match v {
        Value::Object(map) if !map.is_empty() => {
            let keys: Vec<String> = map.keys().cloned().collect();
            let k = &keys[below(rng, keys.len() as u64) as usize];
            let child = map.get_mut(k).unwrap();
            if below(rng, 3) == 0 || !(child.is_object() || child.is_array()) {
                apply(child);
            } else {
                mutate(child, rng, wrap);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            let i = below(rng, items.len() as u64) as usize;
            let child = &mut items[i];
            if below(rng, 3) == 0 || !(child.is_object() || child.is_array()) {
                apply(child);
            } else {
                mutate(child, rng, wrap);
            }
        }
        other => apply(other),
    }
}

fn check_loaded(bytes: &[u8]) {
    let _ = canonicalize(bytes);
    let _ = parse_checked(bytes);
    if let Ok(d) = load(bytes) {
        assert_eq!(d.verify_hash(), Ok(()));
        let again = load(&d.to_canonical_json().unwrap()).expect("re-load");
        assert_eq!(again, d);
    }
}

/// REQ-DSL-001, R12: bounded mutation of valid sealed definitions never panics, and anything
/// accepted is hash-valid and round-trips.
#[test]
fn req_dsl_001_mutated_definitions_never_panic_and_accepted_ones_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xF1E0_0010_BAD0_0111);
    let seeds = [to_value(&sealed(rich())), to_value(&sealed(minimal()))];
    let seed_bytes: Vec<Vec<u8>> = seeds
        .iter()
        .map(|v| serde_json::to_vec(v).unwrap())
        .collect();
    for i in 0..2000usize {
        let which = i % seeds.len();
        match i % 4 {
            0 => {
                let mut bytes = seed_bytes[which].clone();
                let j = below(&mut rng, bytes.len() as u64) as usize;
                bytes[j] ^= 1 << below(&mut rng, 8);
                check_loaded(&bytes);
            }
            1 => {
                let mut bytes = seed_bytes[which].clone();
                let j = below(&mut rng, bytes.len() as u64 + 1) as usize;
                let ins: &[u8] = match below(&mut rng, 3) {
                    0 => "\u{FFFF}".as_bytes(),
                    1 => "\u{FDD0}".as_bytes(),
                    _ => "\u{10FFFE}".as_bytes(),
                };
                for (n, b) in ins.iter().enumerate() {
                    bytes.insert(j + n, *b);
                }
                check_loaded(&bytes);
            }
            op => {
                let mut v = seeds[which].clone();
                for _ in 0..1 + below(&mut rng, 2) {
                    mutate(&mut v, &mut rng, op == 3);
                }
                if v.is_object() && below(&mut rng, 2) == 0 {
                    // Make the digest agree with the mutated content so schema checks decide.
                    redigest(&mut v);
                }
                check_loaded(&serde_json::to_vec(&v).unwrap());
            }
        }
    }
    // Bounded random bytes over the three parsers.
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE trufalsn\\u";
    for _ in 0..2000 {
        let len = below(&mut rng, 96) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        if let Ok(c) = canonicalize(&bytes) {
            assert_eq!(canonicalize(&c).unwrap(), c, "idempotent");
        }
        check_loaded(&bytes);
    }
}

// ---------------------------------------------------------------- round 2

/// REQ-NFR-006: the escaped column really is an escape (differs from the literal), and it decodes
/// to exactly the character in the second column.
#[test]
fn req_nfr_006_table_escapes_are_real_escapes() {
    for (esc, ch) in NONCHARS.iter().chain(ALLOWED.iter()) {
        let literal = ch.to_string();
        assert_ne!(*esc, literal.as_str(), "U+{:04X}", *ch as u32);
        assert!(esc.starts_with("\\u"), "U+{:04X}: {esc}", *ch as u32);
        let parsed: String = serde_json::from_str(&format!("\"{esc}\""))
            .unwrap_or_else(|e| panic!("U+{:04X}: {e}", *ch as u32));
        assert_eq!(parsed, literal, "U+{:04X}", *ch as u32);
    }
}

/// REQ-NFR-006: for allowed characters the escaped and literal spellings canonicalise to the same
/// bytes (RFC 8785 writes them unescaped), in every placement.
#[test]
fn req_nfr_006_escaped_and_literal_spellings_canonicalise_identically() {
    for (esc, ch) in ALLOWED {
        let literal = ch.to_string();
        let lit_places = placements(&literal);
        let esc_places = placements(esc);
        for ((ld, place), (ed, _)) in lit_places.iter().zip(esc_places.iter()) {
            let what = format!("U+{:04X} {place}", *ch as u32);
            assert_ne!(ld, ed, "{what}: documents must differ");
            let a = canonicalize(ld.as_bytes()).unwrap_or_else(|e| panic!("{what}: {e:?}"));
            let b = canonicalize(ed.as_bytes()).unwrap_or_else(|e| panic!("{what}: {e:?}"));
            assert_eq!(a, b, "{what}");
        }
    }
}

fn is_serialize_err<T: std::fmt::Debug>(r: Result<T, CanonicalError>) -> bool {
    matches!(r, Err(CanonicalError::Serialize(_)))
}

/// REQ-DSL-001, REQ-DSL-002: `seal` rejects a `dslVersion` that `from_json` would reject.
#[test]
fn req_dsl_001_002_seal_rejects_unsupported_versions() {
    for v in ["2.0", "0.9", "abc", ""] {
        let mut d = minimal();
        d.dsl_version = v.to_string();
        match d.seal() {
            Err(CanonicalError::Serialize(msg)) => {
                if v == "2.0" {
                    assert!(msg.contains('2'), "message: {msg}");
                }
            }
            other => panic!("version {v:?}: expected Serialize error, got {other:?}"),
        }
    }
    for v in ["1.0", "1.7.3", "1.999"] {
        let mut d = minimal();
        d.dsl_version = v.to_string();
        let s = d.seal().unwrap_or_else(|e| panic!("version {v:?}: {e:?}"));
        assert_eq!(s.dsl_version, v);
        assert_eq!(load(&canonical_bytes(&s)), Ok(s.clone()), "version {v:?}");
    }
}

/// REQ-DSL-001: a 2.0 document with a correct digest is a `Version` error on load.
#[test]
fn req_dsl_001_from_json_rejects_major_2_with_valid_digest() {
    let mut v = to_value(&sealed(minimal()));
    v["dslVersion"] = json!("2.0");
    redigest(&mut v);
    match load_value(&v) {
        Err(CardDefinitionError::Version(_)) => {}
        other => panic!("expected Version error, got {other:?}"),
    }
}

/// REQ-DSL-001, REQ-DSL-002: `seal` succeeds iff the same definition, correctly digested, is not
/// a `Version` error on load.
#[test]
fn req_dsl_001_002_seal_ok_iff_from_json_not_version_error() {
    for text in [
        "1.0", "1.7.3", "1.999", "1", "2.0", "2.1", "0.9", "10.0", "abc", "", "1.", ".1", "1.x",
        " 1.0",
    ] {
        for base in [minimal(), rich()] {
            let which = if base.faces.len() == minimal().faces.len() && base == minimal() {
                "minimal"
            } else {
                "rich"
            };
            let mut d = base;
            d.dsl_version = text.to_string();
            let sealed_result = d.clone().seal();
            let mut v = to_value(&d);
            redigest(&mut v);
            let loaded = load_value(&v);
            match sealed_result {
                Ok(s) => {
                    let exported = s.to_canonical_json().unwrap_or_else(|e| {
                        panic!("version {text:?} base {which}: export failed {e:?}")
                    });
                    assert_eq!(
                        CardDefinitionDSL::from_json(&exported),
                        Ok(s.clone()),
                        "version {text:?} base {which}: from_json(to_canonical_json(sealed)) must equal sealed"
                    );
                    assert_eq!(
                        loaded,
                        Ok(s),
                        "version {text:?} base {which}: seal Ok, load must return the sealed definition"
                    );
                }
                Err(e) => assert!(
                    matches!(loaded, Err(CardDefinitionError::Version(_))),
                    "version {text:?} base {which}: seal Err {e:?}, load must be Version, got {loaded:?}"
                ),
            }
        }
    }
}

/// A value nested `depth` deep, built iteratively (no recursion).
fn deep_value(depth: usize) -> Value {
    let mut v = json!(1);
    for _ in 0..depth {
        v = Value::Array(vec![v]);
    }
    v
}

/// REQ-NFR-006, REQ-DSL-001: a 100_000-deep placeholder makes `compute_hash` and
/// `to_canonical_json` return `Serialize` without recursing or cloning. Runs on the default
/// test stack; the definition is forgotten because `Value`'s `Drop` is recursive.
/// #159 FP-140: this stays on free-form placeholder content on purpose: `compute_hash` and
/// `to_canonical_json` do not validate ASTs, and the subject is their depth limit.
#[test]
fn req_nfr_006_deep_value_compute_hash_errors_without_overflow() {
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(deep_value(100_000)));
    let h = d.compute_hash();
    let j = d.to_canonical_json();
    let h_ok = matches!(h, Err(CanonicalError::Serialize(_)));
    let j_ok = matches!(j, Err(CanonicalError::Serialize(_)));
    std::mem::forget(d);
    assert!(h_ok, "compute_hash must be Serialize error");
    assert!(j_ok, "to_canonical_json must be Serialize error");
}

/// REQ-NFR-006, REQ-DSL-002: nesting past MAX_DEPTH is a `Serialize` error for `seal` and
/// `compute_hash`; a shallow value still hashes. #159 FP-140: `seal` is checked with a valid
/// over-deep MATH chain as well as the placeholder; the `compute_hash` cases stay on placeholder
/// content because `compute_hash` does not validate ASTs.
#[test]
fn req_nfr_006_seal_over_max_depth_is_serialize_error() {
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(deep_value(1_000)));
    assert!(matches!(d.seal(), Err(CanonicalError::Serialize(_))));
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(math_chain(1_000)));
    assert!(matches!(
        d.clone().seal(),
        Err(CanonicalError::Serialize(_))
    ));
    assert!(matches!(
        d.compute_hash(),
        Err(CanonicalError::Serialize(_))
    ));
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(math_chain(100)));
    assert!(d.compute_hash().is_ok());
    assert!(d.seal().is_ok());
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(deep_value(128)));
    assert!(matches!(
        d.compute_hash(),
        Err(CanonicalError::Serialize(_))
    ));
    let mut d = minimal();
    d.faces[0].power = Some(ValueExpressionAST(deep_value(100)));
    assert!(d.compute_hash().is_ok());
}

fn with_stale_hash(base: &CardDefinitionDSL, stale: &str) -> CardDefinitionDSL {
    let mut b = base.clone();
    b.definition_hash = stale.to_string();
    b
}

/// REQ-DSL-002, REQ-NFR-006: a malformed stale `definitionHash` cannot block hashing or sealing,
/// and is never a `Canonical` error from `verify_hash`.
#[test]
fn req_dsl_002_nfr_006_stale_hash_is_ignored_by_hashing_and_sealing() {
    let xs = "X".repeat(64);
    let stale_values: [(&str, &str, bool); 4] = [
        ("(i)", "\u{FDD0}", false),
        ("(ii)", "\u{FFFF}", false),
        ("(iii)", "", true),
        ("(iv)", xs.as_str(), true),
    ];
    for (name, base) in [("minimal", minimal()), ("rich", rich())] {
        let d = sealed(base);
        let mut v = to_value(&d);
        v.as_object_mut().unwrap().remove("definitionHash");
        let oracle = sha256(&canonicalize(&serde_json::to_vec(&v).unwrap()).unwrap()).to_hex();
        assert_eq!(d.definition_hash, oracle, "{name} sealed digest");
        for (label, stale, json_ok) in stale_values {
            if name == "rich" && label != "(i)" {
                continue;
            }
            let what = format!("{name} {label}");
            let bad = with_stale_hash(&d, stale);
            assert_eq!(
                bad.compute_hash(),
                Ok(oracle.clone()),
                "{what} compute_hash"
            );
            assert_eq!(bad.compute_hash(), d.compute_hash(), "{what} vs sealed");
            assert_eq!(bad.clone().seal(), Ok(d.clone()), "{what} seal");
            match bad.verify_hash() {
                Err(CardDefinitionError::HashMismatch { .. }) => {}
                other => panic!("{what}: expected HashMismatch, got {other:?}"),
            }
            let json = bad.to_canonical_json();
            if json_ok {
                let bytes = json.unwrap_or_else(|e| panic!("{what}: {e:?}"));
                let text = String::from_utf8(bytes).expect("utf-8");
                let member = format!("\"definitionHash\":\"{stale}\"");
                assert!(text.contains(&member), "{what}: {text}");
            } else {
                assert!(
                    is_serialize_err(json),
                    "{what}: to_canonical_json must fail"
                );
            }
        }
    }
}

fn sym(n: u32) -> ManaSymbol {
    ManaSymbol::Generic(n)
}

/// REQ-OBJ-001: `ManaCostDSL::new` reports the byte offset and text of the first invalid symbol.
#[test]
fn req_obj_001_mana_new_error_offsets() {
    use ManaSymbol::*;
    match ManaCostDSL::new(vec![
        Generic(10),
        Colored(Color::White),
        Hybrid(Color::Red, Color::Red),
    ]) {
        Err(ManaParseError::UnknownSymbol { offset, symbol }) => {
            assert_eq!(offset, 7);
            assert_eq!(symbol.to_string(), "{R/R}");
        }
        other => panic!("expected UnknownSymbol at 7, got {other:?}"),
    }
    // Two invalid symbols: the first is reported.
    match ManaCostDSL::new(vec![
        Generic(1),
        Generic(1_000_001),
        GenericHybrid(0, Color::White),
    ]) {
        Err(ManaParseError::UnknownSymbol { offset, symbol }) => {
            assert_eq!(offset, 3);
            assert_eq!(symbol.to_string(), "{1000001}");
        }
        other => panic!("expected first invalid symbol, got {other:?}"),
    }
    // Invalid symbol in position 0.
    match ManaCostDSL::new(vec![Hybrid(Color::Blue, Color::Blue), Generic(2)]) {
        Err(ManaParseError::UnknownSymbol { offset, symbol }) => {
            assert_eq!(offset, 0);
            assert_eq!(symbol.to_string(), "{U/U}");
        }
        other => panic!("expected UnknownSymbol at 0, got {other:?}"),
    }
    assert!(matches!(
        ManaCostDSL::new(vec![]),
        Err(ManaParseError::Empty)
    ));
}

/// REQ-OBJ-001: a long valid cost is accepted and round-trips through its text.
#[test]
fn req_obj_001_mana_long_cost_round_trips() {
    let symbols: Vec<ManaSymbol> = (0..200u32)
        .map(|i| match i % 4 {
            0 => sym(i),
            1 => ManaSymbol::Colored(COLORS[(i % 5) as usize]),
            2 => ManaSymbol::Snow,
            _ => ManaSymbol::Phyrexian(COLORS[(i % 5) as usize]),
        })
        .collect();
    let cost = ManaCostDSL::new(symbols.clone()).expect("long valid cost");
    assert_eq!(cost.symbols().len(), 200);
    assert_eq!(cost.symbols(), symbols.as_slice());
    assert_eq!(ManaCostDSL::parse(&cost.to_string()), Ok(cost));
}
