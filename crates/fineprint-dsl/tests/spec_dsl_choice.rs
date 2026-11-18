// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::choice` (FP-011 wire contract): `ChoiceSpec` of SRS §14.4.6,
//! REQ-DSL-011, with REQ-DSL-003 (embedded selectors and value expressions), REQ-CORE-006
//! (integer-only min/max) and REQ-NFR-006. Black-box: written from the work-item brief only.

use std::fmt::Debug;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{MAX_SAFE_INTEGER, canonicalize, to_canonical_vec};
use fineprint_dsl::choice::{ChoiceCandidates, ChoiceKind, ChoiceSpec};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

const KINDS: [&str; 11] = [
    "OBJECTS",
    "MODE",
    "NUMBER",
    "COLOR",
    "CREATURE_TYPE",
    "CARD_NAME",
    "PLAYER",
    "YES_NO",
    "ORDER",
    "DIVISION",
    "PILE",
];

const SIMPLE_REFS: [&str; 6] = [
    "SELF",
    "CONTROLLER",
    "OWNER",
    "ACTIVE_PLAYER",
    "THAT_PLAYER",
    "EACH_OPPONENT",
];

const ZONES: [&str; 4] = ["HAND", "BATTLEFIELD", "GRAVEYARD", "EXILE"];

const LABELS: [&str; 6] = ["Hue", "Moor", "Wisp", "tester", "mode one", "caf\u{e9}"];

// ---------------------------------------------------------------- helpers

fn decode<T: DeserializeOwned>(s: &str) -> T {
    match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    }
}

/// Decodes `s`; `to_value` equals the input value; canonical bytes equal `canonicalize(s)`;
/// round trips decode equal.
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

/// A ChoiceSpec with the given extra members (a JSON object body without braces, may be empty)
/// next to `"chooser":{"ref":"CONTROLLER"},"kind":"<kind>"`.
fn spec(kind: &str, extra: &str) -> String {
    let sep = if extra.is_empty() { "" } else { "," };
    format!("{{\"chooser\":{{\"ref\":\"CONTROLLER\"}},\"kind\":\"{kind}\"{sep}{extra}}}")
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

// ---------------------------------------------------------------- acceptance

/// REQ-DSL-011: every ChoiceKind is accepted (as a member and on its own).
#[test]
fn req_dsl_011_every_kind() {
    let mut seen = Vec::new();
    for k in KINDS {
        let c = accept::<ChoiceSpec>(&spec(k, ""));
        accept::<ChoiceKind>(&format!("\"{k}\""));
        assert!(!seen.contains(&c));
        seen.push(c);
    }
    for bad in ["objects", "Objects", "OBJECT", "", "COLOUR", "YESNO"] {
        reject::<ChoiceKind>(&format!("\"{bad}\""));
        reject::<ChoiceSpec>(&spec(bad, ""));
    }
}

/// REQ-DSL-011: every candidates form (SELECTOR, PLAYERS, ENUM) round-trips.
#[test]
fn req_dsl_011_candidates_forms() {
    let forms = [
        r#"{"SELECTOR":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}}"#,
        r#"{"SELECTOR":{"ref":"TARGET","slot":0}}"#,
        r#"{"SELECTOR":{"zones":["HAND"],"count":{"LITERAL":2}}}"#,
        r#"{"PLAYERS":{"players":"ALL_PLAYERS"}}"#,
        r#"{"PLAYERS":{"ref":"EACH_OPPONENT"}}"#,
        r#"{"ENUM":["Hue"]}"#,
        r#"{"ENUM":["mode one","mode two","mode three"]}"#,
    ];
    for f in forms {
        accept::<ChoiceCandidates>(f);
        accept::<ChoiceSpec>(&spec("OBJECTS", &format!("\"candidates\":{f}")));
    }
}

/// REQ-DSL-011: ENUM order is preserved (not sorted): reversed lists are different values.
#[test]
fn req_dsl_011_enum_order_preserved() {
    let a = accept::<ChoiceSpec>(&spec(
        "MODE",
        r#""candidates":{"ENUM":["Moor","Hue","Wisp"]}"#,
    ));
    let b = accept::<ChoiceSpec>(&spec(
        "MODE",
        r#""candidates":{"ENUM":["Wisp","Hue","Moor"]}"#,
    ));
    assert_ne!(a, b);
}

/// REQ-DSL-011: candidates rejection rules.
#[test]
fn req_dsl_011_candidates_rejections() {
    let bad = [
        r#"{"ENUM":[]}"#,
        r#"{"ENUM":["Hue","Hue"]}"#,
        r#"{"ENUM":["Hue","Moor","Hue"]}"#,
        r#"{"ENUM":[""]}"#,
        r#"{"ENUM":["Hue",""]}"#,
        r#"{"ENUM":[null]}"#,
        r#"{"ENUM":[1]}"#,
        r#"{"ENUM":null}"#,
        r#"{"ENUM":"Hue"}"#,
        r#"{"enum":["Hue"]}"#,
        r#"{"SELECTOR":{}}"#,
        r#"{"SELECTOR":null}"#,
        r#"{"SELECTOR":{"count":{"LITERAL":1}}}"#,
        r#"{"PLAYERS":{"players":"NOBODY"}}"#,
        r#"{"PLAYERS":null}"#,
        r#"{"PLAYERS":{"players":"ALL_PLAYERS"},"ENUM":["Hue"]}"#,
        r#"{"OBJECTS":{"zones":["HAND"]}}"#,
        r#"{}"#,
        r#""ENUM""#,
        r#"null"#,
    ];
    for b in bad {
        reject::<ChoiceCandidates>(b);
        reject::<ChoiceSpec>(&spec("OBJECTS", &format!("\"candidates\":{b}")));
    }
}

/// REQ-DSL-011: `revealTo`, `hidden` and `random` round-trip; a fully populated spec too.
#[test]
fn req_dsl_011_optional_members_round_trip() {
    let docs = [
        spec("COLOR", r#""revealTo":{"players":"ALL_PLAYERS"}"#),
        spec("COLOR", r#""revealTo":{"ref":"TARGET","slot":1}"#),
        spec("CARD_NAME", r#""hidden":true"#),
        spec("PILE", r#""random":true"#),
        spec(
            "OBJECTS",
            r#""candidates":{"SELECTOR":{"zones":["HAND"]}},"min":{"LITERAL":0},"max":{"LITERAL":2},"hidden":true,"random":false,"revealTo":{"ref":"THAT_PLAYER"}"#,
        ),
        spec(
            "OBJECTS",
            r#""candidates":{"SELECTOR":{"zones":["HAND"]}},"max":{"LITERAL":2},"upTo":true,"hidden":false,"random":true,"revealTo":{"players":"TEAMMATES"}"#,
        ),
        r#"{"chooser":{"players":"TEAMMATES"},"kind":"YES_NO"}"#.to_string(),
        r#"{"chooser":{"ref":"BOUND","var":"$p"},"kind":"PLAYER","candidates":{"PLAYERS":{"ref":"EACH_OPPONENT"}}}"#.to_string(),
    ];
    for d in &docs {
        accept::<ChoiceSpec>(d);
    }
    // Member order in the input does not matter.
    let a: ChoiceSpec = decode(&docs[4]);
    let b: ChoiceSpec = decode(
        r#"{"revealTo":{"ref":"THAT_PLAYER"},"random":false,"hidden":true,"max":{"LITERAL":2},"min":{"LITERAL":0},"candidates":{"SELECTOR":{"zones":["HAND"]}},"kind":"OBJECTS","chooser":{"ref":"CONTROLLER"}}"#,
    );
    assert_eq!(a, b);
}

/// REQ-DSL-011: explicit `false` for `upTo`, `hidden` and `random` is a distinct value: it
/// re-serialises with the explicit `false` and is not equal to the omitted form.
#[test]
fn req_dsl_011_explicit_false_is_distinct() {
    let base = accept::<ChoiceSpec>(&spec("NUMBER", r#""max":{"LITERAL":3}"#));
    for member in ["upTo", "hidden", "random"] {
        let s = spec(
            "NUMBER",
            &format!("\"max\":{{\"LITERAL\":3}},\"{member}\":false"),
        );
        let f = accept::<ChoiceSpec>(&s);
        assert_ne!(f, base, "{member}:false equals omission");
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v[member], json!(false), "{member}");
        let t = accept::<ChoiceSpec>(&spec(
            "NUMBER",
            &format!("\"max\":{{\"LITERAL\":3}},\"{member}\":true"),
        ));
        assert_ne!(t, f);
        assert_ne!(t, base);
        let omitted = serde_json::to_value(&base).unwrap();
        assert!(
            omitted.get(member).is_none(),
            "{member} written when absent"
        );
    }
    // upTo:false is compatible with a min (nothing forbids it).
    accept::<ChoiceSpec>(&spec(
        "NUMBER",
        r#""min":{"LITERAL":1},"max":{"LITERAL":3},"upTo":false"#,
    ));
    accept::<ChoiceSpec>(&spec("NUMBER", r#""upTo":false"#));
}

// ---------------------------------------------------------------- validation matrix

/// REQ-DSL-011: min/max/upTo validation for literal bounds.
#[test]
fn req_dsl_011_min_max_up_to_matrix() {
    let ok = [
        r#""min":{"LITERAL":0},"max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":1},"max":{"LITERAL":3}"#,
        r#""min":{"LITERAL":3},"max":{"LITERAL":3}"#,
        r#""min":{"LITERAL":0}"#,
        r#""min":{"LITERAL":9007199254740991}"#,
        r#""max":{"LITERAL":9007199254740991}"#,
        r#""min":{"LITERAL":9007199254740991},"max":{"LITERAL":9007199254740991}"#,
        r#""max":{"LITERAL":0}"#,
        r#""upTo":true,"max":{"LITERAL":2}"#,
        r#""upTo":true,"max":{"LITERAL":0}"#,
        r#""upTo":true,"max":"X""#,
    ];
    for o in ok {
        accept::<ChoiceSpec>(&spec("OBJECTS", o));
    }
    let bad = [
        r#""min":{"LITERAL":3},"max":{"LITERAL":2}"#,
        r#""min":{"LITERAL":1},"max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":9007199254740991},"max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":-1}"#,
        r#""max":{"LITERAL":-1}"#,
        r#""min":{"LITERAL":-1},"max":{"LITERAL":2}"#,
        r#""min":{"LITERAL":-3},"max":{"LITERAL":-1}"#,
        r#""min":{"LITERAL":-9007199254740991}"#,
        r#""min":{"LITERAL":-1},"max":"X""#,
        r#""min":"X","max":{"LITERAL":-1}"#,
        r#""upTo":true"#,
        r#""upTo":true,"min":{"LITERAL":0}"#,
        r#""upTo":true,"min":{"LITERAL":0},"max":{"LITERAL":2}"#,
        r#""upTo":true,"min":"X","max":"X""#,
        r#""upTo":true,"max":{"LITERAL":-1}"#,
    ];
    for b in bad {
        reject::<ChoiceSpec>(&spec("OBJECTS", b));
    }
}

/// REQ-DSL-011: non-literal min/max (COUNT, X, MATH, ...) are accepted with no ordering check.
#[test]
fn req_dsl_011_non_literal_bounds_unchecked() {
    let ok = [
        r#""min":"X","max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":5},"max":"X""#,
        r#""min":{"LITERAL":5},"max":{"COUNT":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}}"#,
        r#""min":{"COUNT":{"zones":["HAND"]}},"max":{"LITERAL":0}"#,
        r#""min":{"LITERAL":1},"max":{"MATH":{"op":"SUB","lhs":{"LITERAL":0},"rhs":{"LITERAL":5}}}"#,
        r#""min":{"MATH":{"op":"SUB","lhs":{"LITERAL":0},"rhs":{"LITERAL":5}}}"#,
        r#""max":{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"HAND_SIZE"}}"#,
        r#""upTo":true,"max":{"COUNT":{"ref":"SELF"}}"#,
        r#""min":"X","max":"X""#,
    ];
    for o in ok {
        accept::<ChoiceSpec>(&spec("NUMBER", o));
    }
}

/// REQ-DSL-011, REQ-CORE-006: min/max literals accept only safe integers in JSON integer form.
#[test]
fn req_core_006_choice_bounds_integer_only() {
    for n in ["0", "1", "9007199254740991"] {
        accept::<ChoiceSpec>(&spec("NUMBER", &format!("\"min\":{{\"LITERAL\":{n}}}")));
        accept::<ChoiceSpec>(&spec("NUMBER", &format!("\"max\":{{\"LITERAL\":{n}}}")));
    }
    let bad = [
        "9007199254740992",
        "-9007199254740992",
        "1.0",
        "1.5",
        "1e2",
        "1E2",
        "-0.0",
        "0.0",
        "1e400",
        "\"1\"",
        "true",
        "null",
        "[1]",
        "{\"v\":1}",
        "18446744073709551615",
    ];
    for n in bad {
        reject::<ChoiceSpec>(&spec("NUMBER", &format!("\"min\":{{\"LITERAL\":{n}}}")));
        reject::<ChoiceSpec>(&spec("NUMBER", &format!("\"max\":{{\"LITERAL\":{n}}}")));
        reject::<ChoiceSpec>(&spec("NUMBER", &format!("\"min\":{n}")));
        reject::<ChoiceSpec>(&spec("NUMBER", &format!("\"max\":{n}")));
    }
    // A bare number is not a value expression.
    reject::<ChoiceSpec>(&spec("NUMBER", r#""min":1"#));
    reject::<ChoiceSpec>(&spec("NUMBER", r#""max":2"#));
}

/// REQ-DSL-011: required members, unknown members and nulls.
#[test]
fn req_dsl_011_spec_rejections() {
    let bad = [
        r#"{"kind":"OBJECTS"}"#.to_string(),
        r#"{"chooser":{"ref":"CONTROLLER"}}"#.to_string(),
        r#"{}"#.to_string(),
        r#"{"chooser":null,"kind":"OBJECTS"}"#.to_string(),
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":null}"#.to_string(),
        r#"{"chooser":{"players":"NOBODY"},"kind":"OBJECTS"}"#.to_string(),
        r#"{"chooser":{},"kind":"OBJECTS"}"#.to_string(),
        r#"{"chooser":{"ref":"CONTROLLER"},"kind":"NOPE"}"#.to_string(),
        spec("OBJECTS", r#""extra":1"#),
        spec("OBJECTS", r#""up_to":true,"max":{"LITERAL":1}"#),
        spec("OBJECTS", r#""UpTo":true,"max":{"LITERAL":1}"#),
        spec("OBJECTS", r#""reveal_to":{"ref":"SELF"}"#),
        spec("OBJECTS", r#""candidates":null"#),
        spec("OBJECTS", r#""min":null"#),
        spec("OBJECTS", r#""max":null"#),
        spec("OBJECTS", r#""upTo":null"#),
        spec("OBJECTS", r#""hidden":null"#),
        spec("OBJECTS", r#""random":null"#),
        spec("OBJECTS", r#""revealTo":null"#),
        spec("OBJECTS", r#""hidden":"true""#),
        spec("OBJECTS", r#""random":1"#),
        spec("OBJECTS", r#""revealTo":{"players":"NOBODY"}"#),
        spec("OBJECTS", r#""min":"x""#),
        spec("OBJECTS", r#""min":{"X":true}"#),
        r#"[]"#.to_string(),
        r#"null"#.to_string(),
    ];
    for b in &bad {
        reject::<ChoiceSpec>(b);
    }
}

// ---------------------------------------------------------------- properties and fuzzing

fn gen_ref(rng: &mut Pcg64Dxsm) -> Map<String, Value> {
    let mut m = Map::new();
    match below(rng, 5) {
        0 => {
            m.insert("ref".into(), json!("TARGET"));
            m.insert("slot".into(), json!(below(rng, u64::from(u32::MAX) + 1)));
        }
        1 => {
            m.insert("ref".into(), json!("CHOSEN"));
            m.insert("key".into(), json!(LABELS[below(rng, 6) as usize]));
        }
        2 => {
            m.insert("ref".into(), json!("BOUND"));
            m.insert("var".into(), json!(format!("$v{}", below(rng, 100))));
        }
        _ => {
            m.insert("ref".into(), json!(SIMPLE_REFS[below(rng, 6) as usize]));
        }
    }
    m
}

fn gen_player(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"players": "ALL_PLAYERS"}),
        1 => json!({"players": "TEAMMATES"}),
        _ => Value::Object(gen_ref(rng)),
    }
}

fn gen_selector(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => Value::Object(gen_ref(rng)),
        1 => json!({"zones": [ZONES[below(rng, 4) as usize]]}),
        2 => {
            json!({"zones": ["BATTLEFIELD", "GRAVEYARD"], "op": "NOT", "condition": {"pred": "TAPPED"}})
        }
        _ => json!({"pred": "CONTROLLED_BY", "player": gen_player(rng)}),
    }
}

fn gen_nonneg(rng: &mut Pcg64Dxsm) -> i64 {
    match below(rng, 4) {
        0 => 0,
        1 => MAX_SAFE_INTEGER,
        2 => below(rng, 10) as i64,
        _ => below(rng, MAX_SAFE_INTEGER as u64 + 1) as i64,
    }
}

fn gen_nonliteral(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!("X"),
        1 => json!({"COUNT": gen_selector(rng)}),
        2 => json!({"MEMORY": LABELS[below(rng, 6) as usize]}),
        _ => {
            json!({"MATH": {"op": "SUB", "lhs": "X", "rhs": {"LITERAL": -(below(rng, 1000) as i64)}}})
        }
    }
}

/// A random valid ChoiceSpec as JSON, honouring the validation rules.
fn gen_spec(rng: &mut Pcg64Dxsm) -> Value {
    let mut m = Map::new();
    m.insert("chooser".into(), gen_player(rng));
    m.insert("kind".into(), json!(KINDS[below(rng, 11) as usize]));
    match below(rng, 4) {
        0 => {
            m.insert("candidates".into(), json!({"SELECTOR": gen_selector(rng)}));
        }
        1 => {
            m.insert("candidates".into(), json!({"PLAYERS": gen_player(rng)}));
        }
        2 => {
            let mut labels: Vec<&str> = LABELS.to_vec();
            rng.shuffle(&mut labels);
            labels.truncate(1 + below(rng, 6) as usize);
            m.insert("candidates".into(), json!({ "ENUM": labels }));
        }
        _ => {}
    }
    let up_to = below(rng, 3);
    // up_to: 0 = absent, 1 = true, 2 = false.
    let want_max = up_to == 1 || below(rng, 2) == 0;
    let want_min = up_to != 1 && below(rng, 2) == 0;
    let max_lit = if below(rng, 3) == 0 {
        None
    } else {
        Some(gen_nonneg(rng))
    };
    if want_max {
        let v = match max_lit {
            Some(n) => json!({ "LITERAL": n }),
            None => gen_nonliteral(rng),
        };
        m.insert("max".into(), v);
    }
    if want_min {
        let v = if below(rng, 3) == 0 {
            gen_nonliteral(rng)
        } else {
            let bound = match (want_max, max_lit) {
                (true, Some(n)) => n,
                _ => MAX_SAFE_INTEGER,
            };
            let n = if below(rng, 3) == 0 {
                bound
            } else {
                below(rng, bound as u64 + 1) as i64
            };
            json!({ "LITERAL": n })
        };
        m.insert("min".into(), v);
    }
    match up_to {
        1 => {
            m.insert("upTo".into(), json!(true));
        }
        2 => {
            m.insert("upTo".into(), json!(false));
        }
        _ => {}
    }
    for b in ["hidden", "random"] {
        match below(rng, 3) {
            0 => {
                m.insert(b.into(), json!(true));
            }
            1 => {
                m.insert(b.into(), json!(false));
            }
            _ => {}
        }
    }
    if below(rng, 2) == 0 {
        m.insert("revealTo".into(), gen_player(rng));
    }
    Value::Object(m)
}

fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(&String, &Value)> = m.iter().collect();
            rng.shuffle(&mut entries);
            out.push_str("{ ");
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(" ,");
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_shuffled(val, rng, out);
            }
            out.push_str("\n}");
        }
        Value::Array(a) => {
            out.push('[');
            for (i, val) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_shuffled(val, rng, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

/// REQ-DSL-011, REQ-NFR-006: 600 random valid specs (seed 0x0F11_0000_0000_0040) round-trip,
/// have one canonical spelling and decode equal from a member-shuffled spelling.
#[test]
fn req_dsl_011_property_round_trip() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0040);
    for _ in 0..600 {
        let v = gen_spec(&mut rng);
        let text = serde_json::to_string(&v).unwrap();
        let c: ChoiceSpec = decode(&text);
        let cv = serde_json::to_value(&c).unwrap();
        assert_eq!(cv, v, "{text}");
        let back: ChoiceSpec = serde_json::from_value(cv).unwrap();
        assert_eq!(back, c);
        let c0 = to_canonical_vec(&c).unwrap();
        for _ in 0..10 {
            assert_eq!(to_canonical_vec(&c).unwrap(), c0);
        }
        assert_eq!(c0, canonicalize(&serde_json::to_vec(&c).unwrap()).unwrap());
        assert_eq!(c0, canonicalize(text.as_bytes()).unwrap());
        let mut sh = String::new();
        write_shuffled(&v, &mut rng, &mut sh);
        let c2: ChoiceSpec = decode(&sh);
        assert_eq!(c2, c, "{sh}");
        assert_eq!(to_canonical_vec(&c2).unwrap(), c0);
    }
}

fn roundtrip_if_ok(bytes: &[u8]) {
    if let Ok(c) = serde_json::from_slice::<ChoiceSpec>(bytes) {
        let enc = serde_json::to_vec(&c).unwrap();
        let c2: ChoiceSpec = serde_json::from_slice(&enc).unwrap();
        assert_eq!(c2, c);
    }
    if let Ok(c) = serde_json::from_slice::<ChoiceCandidates>(bytes) {
        let enc = serde_json::to_vec(&c).unwrap();
        let c2: ChoiceCandidates = serde_json::from_slice(&enc).unwrap();
        assert_eq!(c2, c);
    }
}

/// REQ-NFR-006 (R12): 3000 random byte strings (seed 0x0F11_0000_0000_0041) never panic.
#[test]
fn req_nfr_006_choice_random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.e chooserkindOBJECTSminmaxupTotruefalseLITERAL";
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0041);
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

/// REQ-NFR-006 (R12): 3000 valid specs with one byte flipped, inserted or deleted (seed
/// 0x0F11_0000_0000_0042) never panic; accepted mutants round-trip.
#[test]
fn req_nfr_006_choice_mutations_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0042);
    for _ in 0..3000 {
        let mut bytes = serde_json::to_vec(&gen_spec(&mut rng)).unwrap();
        match below(&mut rng, 3) {
            0 => {
                let i = below(&mut rng, bytes.len() as u64) as usize;
                bytes[i] ^= 1 << below(&mut rng, 8);
            }
            1 => {
                let i = below(&mut rng, bytes.len() as u64) as usize;
                bytes.remove(i);
            }
            _ => {
                let i = below(&mut rng, bytes.len() as u64 + 1) as usize;
                bytes.insert(i, b"{}[]\":,-.e1 tf"[below(&mut rng, 14) as usize]);
            }
        }
        roundtrip_if_ok(&bytes);
    }
}

/// REQ-NFR-006: a spec nested deeper than 127 containers is an error, not a stack overflow.
#[test]
fn req_nfr_006_choice_deep_nesting_is_error() {
    for levels in [64usize, 100, 100_000] {
        let mut s = String::from(r#"{"chooser":{"ref":"CONTROLLER"},"kind":"NUMBER","max":"#);
        for _ in 0..levels {
            s.push_str(r#"{"MATH":{"op":"ADD","lhs":"X","rhs":"#);
        }
        s.push_str(r#"{"LITERAL":1}"#);
        for _ in 0..levels {
            s.push_str("}}");
        }
        s.push('}');
        assert!(serde_json::from_str::<ChoiceSpec>(&s).is_err());
    }
}
