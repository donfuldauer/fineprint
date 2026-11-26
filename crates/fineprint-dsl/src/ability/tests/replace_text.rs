// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of the typed `REPLACE_TEXT_WORD` view of [`Modification`] (REQ-DSL-012, SRS
//! §14.5; CR 612.1, 612.2, 613.1c): the constructor, the accessor, the unchanged wire shape
//! (additive only: no new rejection, canonical bytes unchanged) and bounded random inputs that
//! never panic (R12). Synthetic words only (R5): rules vocabulary and `test.*` strings.

use std::collections::BTreeMap;

use fineprint_base::prng::Pcg64Dxsm;
use serde_json::{Value, json};

use super::super::*;
use crate::canonical::to_canonical_vec;
use crate::expr::tests::{accept, gen_str, pick, random_doc};
use crate::text::RewritableCategory;

/// `m` with `params` replaced by the members of the JSON object `members`.
fn with_params(op: &str, members: &Value) -> Modification {
    let mut m = Modification::new(op);
    m.params = members
        .as_object()
        .expect("object")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<BTreeMap<_, _>>();
    m
}

// REQ-DSL-012: the constructor writes op REPLACE_TEXT_WORD with the flat params category (its
// wire name), from and to, and the accessor reads them back, for every category (CARD_NAME is
// returned as parsed; CR 612.2 rejection belongs to the compile step).
#[test]
fn req_dsl_012_constructor_and_accessor_round_trip() {
    assert_eq!(Modification::REPLACE_TEXT_WORD, "REPLACE_TEXT_WORD");
    for &category in RewritableCategory::ALL {
        let m = Modification::replace_text_word(category, "Swamp", "Island");
        assert_eq!(m.op, "REPLACE_TEXT_WORD");
        assert_eq!(m.power, None);
        assert_eq!(m.toughness, None);
        assert_eq!(m.params.len(), 3);
        assert_eq!(m.params["category"], json!(category.as_str()));
        assert_eq!(m.params["from"], json!("Swamp"));
        assert_eq!(m.params["to"], json!("Island"));
        assert_eq!(
            m.as_replace_text_word(),
            Ok(Some((category, "Swamp".to_owned(), "Island".to_owned())))
        );
    }
    // Words are free strings: empty, equal and non-ASCII words are returned as given.
    let m = Modification::replace_text_word(RewritableCategory::Number, String::new(), "");
    assert_eq!(
        m.as_replace_text_word(),
        Ok(Some((
            RewritableCategory::Number,
            String::new(),
            String::new()
        )))
    );
}

// REQ-DSL-012, REQ-CORE-006: a built modification serializes to the documented wire shape,
// parses back equal, and its canonical bytes are stable (parse -> serialize -> parse).
#[test]
fn req_dsl_012_built_modification_is_canonically_stable() {
    for &category in RewritableCategory::ALL {
        let m = Modification::replace_text_word(category, "test.from \u{e9}", "test.to");
        let bytes = to_canonical_vec(&m).expect("canonical");
        let expected = format!(
            r#"{{"category":"{}","from":"test.from é","op":"REPLACE_TEXT_WORD","to":"test.to"}}"#,
            category.as_str()
        );
        assert_eq!(String::from_utf8(bytes.clone()).expect("utf8"), expected);
        let back: Modification = serde_json::from_slice(&bytes).expect("parse");
        assert_eq!(back, m);
        assert_eq!(to_canonical_vec(&back).expect("canonical"), bytes);
        assert_eq!(back.as_replace_text_word(), m.as_replace_text_word());
    }
}

// REQ-DSL-012 (SRS §14.5): the documented example parses through the existing decoder and the
// accessor gives the typed view; inside a layer 3 CHAR effect too.
#[test]
fn req_dsl_012_parsed_document_has_typed_view() {
    let m: Modification = accept(
        r#"{"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":"Swamp","to":"Island"}"#,
    );
    assert_eq!(
        m.as_replace_text_word(),
        Ok(Some((
            RewritableCategory::ColorWord,
            "Swamp".to_owned(),
            "Island".to_owned()
        )))
    );
    let c: CharacteristicEffect = accept(
        r#"{"type":"CHAR","layer":"3","affected":{"ref":"SELF"},"modification":{"op":"REPLACE_TEXT_WORD","category":"BASIC_LAND_TYPE","from":"Swamp","to":"Island"}}"#,
    );
    assert_eq!(c.layer, Layer::L3);
    assert_eq!(
        c.modification,
        Modification::replace_text_word(RewritableCategory::BasicLandType, "Swamp", "Island")
    );
}

// REQ-DSL-012: any other op gives Ok(None), whatever its params, including typed P/T ops.
#[test]
fn req_dsl_012_other_ops_are_none() {
    let mut pt = Modification::new("MODIFY_PT");
    pt.power = Some(PtValue::Int(1));
    pt.toughness = Some(PtValue::Int(1));
    for m in [
        Modification::new("SET_TEXT"),
        Modification::new("REPLACE_TEXT_WORDS"),
        Modification::new("replace_text_word"),
        with_params(
            "TEST_OP",
            &json!({"category": "COLOR_WORD", "from": "a", "to": "b"}),
        ),
        pt,
    ] {
        assert_eq!(m.as_replace_text_word(), Ok(None), "{m:?}");
    }
}

// REQ-DSL-012, R12: malformed REPLACE_TEXT_WORD members are an Err (never a panic): missing,
// non-string or unknown category (wire names are case-sensitive), missing or non-string words,
// extra members, typed power or toughness.
#[test]
fn req_dsl_012_malformed_members_are_errors() {
    let cases = [
        json!({"from": "a", "to": "b"}),
        json!({"category": 1, "from": "a", "to": "b"}),
        json!({"category": null, "from": "a", "to": "b"}),
        json!({"category": "color_word", "from": "a", "to": "b"}),
        json!({"category": "TEST_CATEGORY", "from": "a", "to": "b"}),
        json!({"category": "COLOR_WORD", "to": "b"}),
        json!({"category": "COLOR_WORD", "from": "a"}),
        json!({"category": "COLOR_WORD", "from": ["a"], "to": "b"}),
        json!({"category": "COLOR_WORD", "from": "a", "to": {"LITERAL": 1}}),
        json!({"category": "COLOR_WORD", "from": "a", "to": "b", "extra": 1}),
        json!({"category": "COLOR_WORD", "from": "a", "to": "b", "power": 1}),
        json!({}),
    ];
    for members in &cases {
        let m = with_params(Modification::REPLACE_TEXT_WORD, members);
        let err = m.as_replace_text_word().expect_err("malformed");
        assert!(err.contains("REPLACE_TEXT_WORD"), "{err}");
    }
    let mut typed = Modification::replace_text_word(RewritableCategory::ColorWord, "a", "b");
    typed.power = Some(PtValue::Int(1));
    assert!(typed.as_replace_text_word().is_err());
    let mut typed = Modification::replace_text_word(RewritableCategory::ColorWord, "a", "b");
    typed.toughness = Some(PtValue::Int(1));
    assert!(typed.as_replace_text_word().is_err());
}

// REQ-DSL-012, REQ-CORE-006: additive only. A malformed REPLACE_TEXT_WORD document is still
// accepted by the decoder exactly as before (it is an opaque op on the wire) with unchanged
// canonical bytes; only the typed view reports it.
#[test]
fn req_dsl_012_no_new_wire_rejection() {
    for doc in [
        r#"{"op":"REPLACE_TEXT_WORD"}"#,
        r#"{"op":"REPLACE_TEXT_WORD","category":"TEST_CATEGORY","from":"a","to":"b"}"#,
        r#"{"op":"REPLACE_TEXT_WORD","category":"COLOR_WORD","from":1,"to":"b","x":[1]}"#,
        r#"{"op":"REPLACE_TEXT_WORD","category":"CARD_NAME","from":"","to":""}"#,
    ] {
        let m: Modification = accept(doc);
        let _ = m.as_replace_text_word();
    }
}

/// A random JSON value from a small synthetic vocabulary, biased toward valid members.
fn gen_member(rng: &mut Pcg64Dxsm) -> Value {
    match pick(rng, 7) {
        0 => Value::Null,
        1 => json!(pick(rng, 10)),
        2 => json!([gen_str(rng)]),
        3 => json!({"LITERAL": 1}),
        4 => json!(RewritableCategory::ALL[pick(rng, RewritableCategory::ALL.len())].as_str()),
        _ => json!(gen_str(rng)),
    }
}

// REQ-DSL-012, R12: bounded random params and random bytes never panic the accessor; a
// well-formed result always rebuilds an equal modification.
#[test]
fn req_dsl_012_random_inputs_never_panic() {
    const KEYS: [&str; 6] = ["category", "from", "to", "power", "extra", "category2"];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0f15_0195_0001);
    for _ in 0..3000 {
        let mut m = Modification::new(if pick(&mut rng, 8) == 0 {
            "TEST_OP"
        } else {
            Modification::REPLACE_TEXT_WORD
        });
        for _ in 0..pick(&mut rng, 5) {
            let key = KEYS[pick(&mut rng, KEYS.len())];
            m.params.insert(key.to_owned(), gen_member(&mut rng));
        }
        if let Ok(Some((category, from, to))) = m.as_replace_text_word() {
            assert_eq!(Modification::replace_text_word(category, from, to), m);
        }
        if let Ok(decoded) = serde_json::from_slice::<Modification>(&random_doc(&mut rng)) {
            let _ = decoded.as_replace_text_word();
        }
    }
}
