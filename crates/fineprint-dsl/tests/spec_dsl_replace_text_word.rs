// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the DSL `REPLACE_TEXT_WORD` modification (REQ-DSL-012, SRS §14.5; CR 612.2):
//! the `Modification::replace_text_word` builder, the wire shape
//! `{"op":"REPLACE_TEXT_WORD","category","from","to"}` (flat members, canonical bytes, round
//! trips inside a CHAR effect and a CREATE_CONTINUOUS_EFFECT instruction) and the
//! `Modification::as_replace_text_word` accessor (Ok(None) for other ops, Ok(Some) when
//! well-formed, Err and never a panic when malformed). `CARD_NAME` parses and is returned by the
//! accessor; rejecting it is the compile step's job (CR 612.2).
//!
//! Black-box: written from the #195 (FP-150) brief only. Word values are rules vocabulary or
//! neutral `test.*` strings (R5).

use std::collections::BTreeMap;

use fineprint_dsl::ability::{CharacteristicEffect, Layer, Modification};
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use fineprint_dsl::effect::Instruction;
use fineprint_dsl::text::RewritableCategory;
use serde_json::{Map, Value, json};

use RewritableCategory::{BasicLandType, CardName, ColorWord, CreatureType, Number};

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

/// The five categories with their wire names (SRS §14.5), written out independently.
const CATEGORIES: [(RewritableCategory, &str); 5] = [
    (ColorWord, "COLOR_WORD"),
    (BasicLandType, "BASIC_LAND_TYPE"),
    (CreatureType, "CREATURE_TYPE"),
    (Number, "NUMBER"),
    (CardName, "CARD_NAME"),
];

fn rtw_json(cat: &str, from: &str, to: &str) -> Value {
    json!({"op":"REPLACE_TEXT_WORD","category":cat,"from":from,"to":to})
}

fn parse_mod(v: &Value) -> Result<Modification, serde_json::Error> {
    serde_json::from_value::<Modification>(v.clone())
}

fn must_parse(v: &Value) -> Modification {
    let text = serde_json::to_string(v).expect("json");
    let a = match serde_json::from_str::<Modification>(&text) {
        Ok(m) => m,
        Err(e) => panic!("expected {text} to parse as a Modification: {e}"),
    };
    assert_eq!(
        serde_json::from_slice::<Modification>(text.as_bytes()).expect("from_slice"),
        a
    );
    assert_eq!(parse_mod(v).expect("from_value"), a);
    a
}

/// A modification with the given op and raw params, built without the parser (so values the
/// parser rejects, e.g. JSON null, can be fed to the accessor too).
fn raw(op: &str, params: &[(&str, Value)]) -> Modification {
    let mut m = Modification::new(op);
    for (k, v) in params {
        m.params.insert((*k).to_string(), v.clone());
    }
    m
}

fn char_effect(m: &Value) -> Value {
    json!({"type":"CHAR","layer":"3","affected":{"zones":["BATTLEFIELD"],"pred":"IS_PERMANENT"},
        "modification":m})
}

/// A deterministic 64-bit LCG (no `rand`, R3).
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[usize::try_from(self.below(xs.len() as u64)).expect("fits")]
    }
}

const WORD_POOL: [&str; 12] = [
    "Swamp",
    "Island",
    "Red",
    "Goblin",
    "3",
    "",
    "\u{FFFF}",
    "test.word",
    "COLOR_WORD",
    "CARD_NAME",
    "x\u{FDD0}y",
    "Elf",
];

const KEY_POOL: [&str; 9] = [
    "category",
    "from",
    "to",
    "op",
    "power",
    "toughness",
    "extra",
    "value",
    "",
];

fn random_value(rng: &mut Lcg, depth: u32) -> Value {
    let k = if depth == 0 {
        rng.below(6)
    } else {
        rng.below(8)
    };
    match k {
        0 => Value::Null,
        1 => Value::Bool(rng.below(2) == 0),
        2 => json!(i64::try_from(rng.below(2001)).expect("fits") - 1000),
        3 => json!(*rng.pick(&CATEGORIES.map(|(_, w)| w))),
        4 | 5 => json!(*rng.pick(&WORD_POOL)),
        6 => {
            let n = rng.below(3);
            Value::Array((0..n).map(|_| random_value(rng, depth - 1)).collect())
        }
        _ => {
            let mut m = Map::new();
            for _ in 0..rng.below(3) {
                m.insert(
                    (*rng.pick(&KEY_POOL)).to_string(),
                    random_value(rng, depth - 1),
                );
            }
            Value::Object(m)
        }
    }
}

fn random_object(rng: &mut Lcg) -> Value {
    let mut m = Map::new();
    let op = match rng.below(5) {
        0 | 1 => json!("REPLACE_TEXT_WORD"),
        2 => json!(*rng.pick(&["SET_BASE_PT", "MODIFY_PT", "TEST_OP", "replace_text_word"])),
        3 => random_value(rng, 1),
        _ => Value::Null,
    };
    if !op.is_null() || rng.below(2) == 0 {
        m.insert("op".into(), op);
    }
    // Half the time start from well-formed members, so the Ok(Some) path is reached too.
    if rng.below(2) == 0 {
        m.insert(
            "category".into(),
            json!(*rng.pick(&CATEGORIES.map(|(_, w)| w))),
        );
        m.insert("from".into(), json!(*rng.pick(&WORD_POOL)));
        m.insert("to".into(), json!(*rng.pick(&WORD_POOL)));
    }
    for _ in 0..rng.below(4) {
        m.insert((*rng.pick(&KEY_POOL)).to_string(), random_value(rng, 2));
    }
    Value::Object(m)
}

// ---------------------------------------------------------------------------------------------
// 1. builder and wire shape
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012 (SRS §14.5): the op name constant is the wire name.
#[test]
fn req_dsl_012_op_constant() {
    assert_eq!(Modification::REPLACE_TEXT_WORD, "REPLACE_TEXT_WORD");
}

/// REQ-DSL-012 (SRS §14.5): `replace_text_word` builds op REPLACE_TEXT_WORD with exactly the
/// three flat members `category` (wire name), `from` and `to`, for each of the five
/// categories, and serialises to exactly the canonical bytes of the wire form.
#[test]
fn req_dsl_012_builder_wire_shape_each_category() {
    for (cat, wire) in CATEGORIES {
        let m = Modification::replace_text_word(cat, "Swamp", "Island");
        assert_eq!(m.op, "REPLACE_TEXT_WORD", "{wire}");
        assert_eq!(m.power, None, "{wire}");
        assert_eq!(m.toughness, None, "{wire}");
        let mut want = BTreeMap::new();
        want.insert("category".to_string(), json!(wire));
        want.insert("from".to_string(), json!("Swamp"));
        want.insert("to".to_string(), json!("Island"));
        assert_eq!(m.params, want, "{wire}");

        let doc = rtw_json(wire, "Swamp", "Island");
        assert_eq!(serde_json::to_value(&m).expect("to_value"), doc, "{wire}");
        let literal = format!(
            r#"{{"op":"REPLACE_TEXT_WORD","category":"{wire}","from":"Swamp","to":"Island"}}"#
        );
        let canon = canonicalize(literal.as_bytes()).expect("canonical");
        assert_eq!(
            String::from_utf8(to_canonical_vec(&m).expect("canonical")).expect("utf8"),
            String::from_utf8(canon).expect("utf8"),
            "{wire}"
        );
        // The documented example, byte for byte in canonical member order.
        if cat == ColorWord {
            assert_eq!(
                to_canonical_vec(&m).expect("canonical"),
                br#"{"category":"COLOR_WORD","from":"Swamp","op":"REPLACE_TEXT_WORD","to":"Island"}"#
                    .to_vec()
            );
        }
        // Parse of the wire form gives the built value.
        assert_eq!(must_parse(&doc), m, "{wire}");
    }
}

/// REQ-DSL-012: `from` and `to` accept `String` and `&str` alike; NUMBER keeps its value as a
/// string (SRS §14.5).
#[test]
fn req_dsl_012_builder_into_string_and_number_as_string() {
    let a = Modification::replace_text_word(Number, String::from("3"), "4");
    let b = Modification::replace_text_word(Number, "3", String::from("4"));
    assert_eq!(a, b);
    assert_eq!(a.params["from"], json!("3"));
    assert_eq!(a.params["to"], json!("4"));
    assert_eq!(
        serde_json::to_value(&a).expect("to_value"),
        rtw_json("NUMBER", "3", "4")
    );
}

// ---------------------------------------------------------------------------------------------
// 2. round trips
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012 (SRS §14.5): a layer-3 CHAR effect with the modification: parse -> serialize ->
/// parse is equal and the canonical bytes are stable, for each category.
#[test]
fn req_dsl_012_char_effect_round_trip() {
    for (cat, wire) in CATEGORIES {
        let doc = char_effect(&rtw_json(wire, "Swamp", "Island"));
        let text = serde_json::to_string(&doc).expect("json");
        let e1: CharacteristicEffect = serde_json::from_str(&text).expect("parse");
        assert_eq!(e1.layer, Layer::L3);
        assert_eq!(
            e1.modification,
            Modification::replace_text_word(cat, "Swamp", "Island")
        );
        let bytes1 = to_canonical_vec(&e1).expect("canonical");
        assert_eq!(
            bytes1,
            canonicalize(text.as_bytes()).expect("canonical"),
            "{wire}"
        );
        let e2: CharacteristicEffect = serde_json::from_slice(&bytes1).expect("re-parse");
        assert_eq!(e2, e1, "{wire}");
        let bytes2 = to_canonical_vec(&e2).expect("canonical");
        assert_eq!(bytes2, bytes1, "{wire}");
        assert_eq!(serde_json::to_value(&e2).expect("to_value"), doc, "{wire}");
    }
}

/// REQ-DSL-012 (SRS §14.5): a CREATE_CONTINUOUS_EFFECT instruction holding the layer-3
/// REPLACE_TEXT_WORD CHAR effect parses and re-serialises identically (canonical bytes), and the
/// re-parse is equal.
#[test]
fn req_dsl_012_create_continuous_effect_round_trip() {
    let doc = json!({
        "op":"CREATE_CONTINUOUS_EFFECT",
        "duration":{"kind":"UNTIL_END_OF_TURN"},
        "effects":[char_effect(&rtw_json("BASIC_LAND_TYPE","Swamp","Island"))]
    });
    let text = serde_json::to_string(&doc).expect("json");
    let i1: Instruction = serde_json::from_str(&text).expect("parse");
    let bytes1 = to_canonical_vec(&i1).expect("canonical");
    assert_eq!(bytes1, canonicalize(text.as_bytes()).expect("canonical"));
    let i2: Instruction = serde_json::from_slice(&bytes1).expect("re-parse");
    assert_eq!(i2, i1);
    assert_eq!(to_canonical_vec(&i2).expect("canonical"), bytes1);
    assert_eq!(serde_json::to_value(&i2).expect("to_value"), doc);
}

/// REQ-DSL-012, REQ-CORE-001: a bounded random run over well-formed (category, from, to):
/// the accessor returns exactly what the builder was given, and parse(serialize) is equal and
/// byte-stable.
#[test]
fn req_dsl_012_builder_accessor_round_trip_property() {
    let alphabet: Vec<char> = "abcXYZ019 ._-\u{e9}\u{3b1}\u{4e2d}".chars().collect();
    let mut rng = Lcg::new(0x0195);
    for _ in 0..500 {
        let (cat, _) = *rng.pick(&CATEGORIES);
        let word = |rng: &mut Lcg| -> String {
            let n = 1 + rng.below(8);
            (0..n).map(|_| *rng.pick(&alphabet)).collect()
        };
        let from = word(&mut rng);
        let to = word(&mut rng);
        let m = Modification::replace_text_word(cat, from.clone(), to.clone());
        assert_eq!(
            m.as_replace_text_word(),
            Ok(Some((cat, from.clone(), to.clone())))
        );
        let bytes = to_canonical_vec(&m).expect("canonical");
        let back: Modification = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(back, m);
        assert_eq!(to_canonical_vec(&back).expect("canonical"), bytes);
        assert_eq!(back.as_replace_text_word(), Ok(Some((cat, from, to))));
    }
}

// ---------------------------------------------------------------------------------------------
// 3, 4. as_replace_text_word
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012 (SRS §14.5): Ok(Some) with the right tuple for each category, from the builder and
/// from parsed JSON; CARD_NAME is returned as parsed (rejection is the compile step's, CR 612.2).
#[test]
fn req_dsl_012_accessor_well_formed_each_category() {
    for (cat, wire) in CATEGORIES {
        let want = Ok(Some((cat, "Swamp".to_string(), "Island".to_string())));
        assert_eq!(
            Modification::replace_text_word(cat, "Swamp", "Island").as_replace_text_word(),
            want,
            "{wire}"
        );
        assert_eq!(
            must_parse(&rtw_json(wire, "Swamp", "Island")).as_replace_text_word(),
            want,
            "{wire}"
        );
    }
    // from == to is well-formed at the DSL level.
    assert_eq!(
        Modification::replace_text_word(ColorWord, "Red", "Red").as_replace_text_word(),
        Ok(Some((ColorWord, "Red".to_string(), "Red".to_string())))
    );
}

/// REQ-DSL-012; CR 612.2: a CARD_NAME replacement parses, re-serialises and is returned by the
/// accessor.
#[test]
fn req_dsl_012_card_name_parses_and_is_returned() {
    let doc = rtw_json("CARD_NAME", "test.name_a", "test.name_b");
    let m = must_parse(&doc);
    assert_eq!(
        m.as_replace_text_word(),
        Ok(Some((
            CardName,
            "test.name_a".to_string(),
            "test.name_b".to_string()
        )))
    );
    assert_eq!(serde_json::to_value(&m).expect("to_value"), doc);
    let e: CharacteristicEffect =
        serde_json::from_value(char_effect(&doc)).expect("CHAR effect with CARD_NAME parses");
    assert_eq!(e.modification, m);
}

/// REQ-DSL-012: Ok(None) for every other op, whatever its params (SET_BASE_PT, a made-up op, an
/// op that merely carries category/from/to members, a near-miss spelling).
#[test]
fn req_dsl_012_accessor_other_ops_none() {
    let pt = must_parse(&json!({"op":"SET_BASE_PT","power":1,"toughness":1}));
    assert_eq!(pt.as_replace_text_word(), Ok(None));
    let modify = must_parse(&json!({"op":"MODIFY_PT","power":1,"toughness":-1}));
    assert_eq!(modify.as_replace_text_word(), Ok(None));
    let made_up = must_parse(&json!({"op":"TEST_OP"}));
    assert_eq!(made_up.as_replace_text_word(), Ok(None));
    let lookalike = must_parse(&json!({"op":"TEST_REPLACE","category":"COLOR_WORD",
        "from":"Swamp","to":"Island"}));
    assert_eq!(lookalike.as_replace_text_word(), Ok(None));
    let near = must_parse(&json!({"op":"REPLACE_TEXT_WORDS","category":"COLOR_WORD",
        "from":"Swamp","to":"Island"}));
    assert_eq!(near.as_replace_text_word(), Ok(None));
    // Unparseable shapes built directly: still Ok(None) for a foreign op.
    assert_eq!(
        raw("TEST_OP", &[("category", Value::Null)]).as_replace_text_word(),
        Ok(None)
    );
    assert_eq!(Modification::new("").as_replace_text_word(), Ok(None));
}

/// The malformed REPLACE_TEXT_WORD member sets of requirement 3, as JSON documents that the
/// opaque-params parser accepted before #195.
fn malformed_docs() -> Vec<(&'static str, Value)> {
    let ok = rtw_json("COLOR_WORD", "Swamp", "Island");
    let drop = |k: &str| {
        let mut v = ok.clone();
        v.as_object_mut().expect("object").remove(k);
        v
    };
    let set = |k: &str, x: Value| {
        let mut v = ok.clone();
        v.as_object_mut().expect("object").insert(k.to_string(), x);
        v
    };
    vec![
        ("missing category", drop("category")),
        ("missing from", drop("from")),
        ("missing to", drop("to")),
        ("missing all", json!({"op":"REPLACE_TEXT_WORD"})),
        ("category number", set("category", json!(1))),
        ("category bool", set("category", json!(true))),
        ("category array", set("category", json!(["COLOR_WORD"]))),
        (
            "category object",
            set("category", json!({"name":"COLOR_WORD"})),
        ),
        ("category unknown", set("category", json!("TEST_CATEGORY"))),
        ("category lower case", set("category", json!("color_word"))),
        ("category empty", set("category", json!(""))),
        ("from number", set("from", json!(3))),
        ("from array", set("from", json!(["Swamp"]))),
        ("from object", set("from", json!({"textToken":"t1"}))),
        ("from bool", set("from", json!(false))),
        ("to number", set("to", json!(4))),
        ("to array", set("to", json!(["Island"]))),
        ("to object", set("to", json!({"name":"Island"}))),
        ("to bool", set("to", json!(true))),
        ("extra param", set("extra", json!("x"))),
        ("extra param value", set("value", json!("Swamp"))),
        ("extra layer param", set("layer", json!("3"))),
    ]
}

/// REQ-DSL-012 (R12): each malformed REPLACE_TEXT_WORD still parses (documents that parsed before
/// #195 still parse) and the accessor returns Err without panicking.
#[test]
fn req_dsl_012_accessor_malformed_err() {
    for (what, doc) in malformed_docs() {
        let m = must_parse(&doc);
        let r = m.as_replace_text_word();
        assert!(r.is_err(), "{what}: expected Err, got {r:?}");
        // Re-serialisation is unchanged for these previously valid documents.
        assert_eq!(
            serde_json::to_value(&m).expect("to_value"),
            doc,
            "{what}: re-serialise"
        );
        // Inside a CHAR effect too.
        let e: CharacteristicEffect = serde_json::from_value(char_effect(&doc))
            .unwrap_or_else(|err| panic!("{what}: CHAR effect parses: {err}"));
        assert!(e.modification.as_replace_text_word().is_err(), "{what}");
    }
}

/// REQ-DSL-012 (R12): values the parser never accepts (JSON null members, power/toughness on a
/// REPLACE_TEXT_WORD) are Err from the accessor, never a panic, when the value is built directly.
#[test]
fn req_dsl_012_accessor_directly_built_malformed_err() {
    let good = [
        ("category", json!("COLOR_WORD")),
        ("from", json!("Swamp")),
        ("to", json!("Island")),
    ];
    for k in ["category", "from", "to"] {
        let params: Vec<(&str, Value)> = good
            .iter()
            .map(|(n, v)| (*n, if *n == k { Value::Null } else { v.clone() }))
            .collect();
        let r = raw("REPLACE_TEXT_WORD", &params).as_replace_text_word();
        assert!(r.is_err(), "{k} null: {r:?}");
    }
    let mut with_empty_key: Vec<(&str, Value)> = good.to_vec();
    with_empty_key.push(("", json!(1)));
    assert!(
        raw("REPLACE_TEXT_WORD", &with_empty_key)
            .as_replace_text_word()
            .is_err()
    );
    let r = raw("REPLACE_TEXT_WORD", &[]).as_replace_text_word();
    assert!(r.is_err(), "no params: {r:?}");
}

// ---------------------------------------------------------------------------------------------
// 5. robustness
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012 (R12): a bounded run of arbitrary JSON objects through `Modification` parsing and
/// `as_replace_text_word` never panics; a REPLACE_TEXT_WORD that is Ok(Some) has exactly the
/// three members with a known category, and every other op is Ok(None).
#[test]
fn req_dsl_012_random_objects_never_panic() {
    let mut rng = Lcg::new(0xFB15_0001);
    let mut parsed = 0u32;
    let mut some = 0u32;
    for _ in 0..4000 {
        let doc = random_object(&mut rng);
        let m = match parse_mod(&doc) {
            Ok(m) => m,
            Err(_) => continue,
        };
        parsed += 1;
        let r = m.as_replace_text_word();
        if m.op != "REPLACE_TEXT_WORD" {
            assert_eq!(r, Ok(None), "{doc}");
            continue;
        }
        if let Ok(Some((cat, from, to))) = r {
            some += 1;
            assert_eq!(m.params.len(), 3, "{doc}");
            assert_eq!(m.params["category"], json!(cat.as_str()), "{doc}");
            assert_eq!(m.params["from"], json!(from), "{doc}");
            assert_eq!(m.params["to"], json!(to), "{doc}");
        } else {
            assert!(r.is_err(), "REPLACE_TEXT_WORD is never Ok(None): {doc}");
        }
        // Whatever parsed re-serialises and re-parses equal.
        let back: Modification =
            serde_json::from_slice(&to_canonical_vec(&m).expect("canonical")).expect("re-parse");
        assert_eq!(back, m, "{doc}");
    }
    assert!(
        parsed > 100,
        "the generator must reach the accessor ({parsed})"
    );
    assert!(some > 0, "the generator must hit the well-formed case");
}

/// REQ-DSL-012 (R12): bounded random bytes (and mutated wire documents) through the
/// `Modification` and `CharacteristicEffect` parsers never panic, and the accessor never panics
/// on whatever parses.
#[test]
fn req_dsl_012_random_bytes_never_panic() {
    let mut rng = Lcg::new(0xB17E5);
    let seed_doc =
        serde_json::to_vec(&char_effect(&rtw_json("COLOR_WORD", "Swamp", "Island"))).expect("json");
    let seed_mod = serde_json::to_vec(&rtw_json("COLOR_WORD", "Swamp", "Island")).expect("json");
    for i in 0..3000u32 {
        let bytes: Vec<u8> = match i % 3 {
            0 => {
                let n = rng.below(64);
                (0..n)
                    .map(|_| u8::try_from(rng.below(256)).expect("byte"))
                    .collect()
            }
            1 => {
                let mut b = seed_mod.clone();
                for _ in 0..1 + rng.below(4) {
                    let at = usize::try_from(rng.below(b.len() as u64)).expect("fits");
                    b[at] = u8::try_from(rng.below(256)).expect("byte");
                }
                b
            }
            _ => {
                let mut b = seed_doc.clone();
                let cut = usize::try_from(rng.below(b.len() as u64 + 1)).expect("fits");
                b.truncate(cut);
                if rng.below(2) == 0 {
                    let at = usize::try_from(rng.below(b.len().max(1) as u64)).expect("fits");
                    if at < b.len() {
                        b[at] = *rng.pick(b"\"{}[]:,0x\\");
                    }
                }
                b
            }
        };
        if let Ok(m) = serde_json::from_slice::<Modification>(&bytes) {
            let _ = m.as_replace_text_word();
        }
        if let Ok(e) = serde_json::from_slice::<CharacteristicEffect>(&bytes) {
            let _ = e.modification.as_replace_text_word();
        }
    }
}
