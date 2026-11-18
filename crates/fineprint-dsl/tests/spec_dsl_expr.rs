// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::expr` (FP-011 wire contract): references, player and object
//! selectors, predicates and value expressions. REQ-DSL-003 (SRS §14.4.0), REQ-DSL-012 (by-id
//! text-token references, SRS §14.5), REQ-CORE-006 (integer only) and REQ-NFR-006 (untrusted
//! input, canonical form). Black-box: written from the work-item brief only; everything is driven
//! through serde_json and `fineprint_dsl::canonical`.

use std::fmt::Debug;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{MAX_SAFE_INTEGER, canonicalize, to_canonical_vec};
use fineprint_dsl::choice::ChoiceSpec;
use fineprint_dsl::expr::{
    ObjectSelectorAST, PlayerSelectorAST, PredicateAST, ReferenceAST, ValueExpressionAST, Zone,
};
use fineprint_dsl::text::TextTokenStream;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

type Obj = Map<String, Value>;

// ---------------------------------------------------------------- vocabulary from the contract

const SIMPLE_REFS: [&str; 11] = [
    "SELF",
    "SOURCE_LKI",
    "CONTROLLER",
    "OWNER",
    "ACTIVE_PLAYER",
    "DEFENDING_PLAYER",
    "ENCHANTED",
    "EQUIPPED",
    "ATTACHED_TO",
    "THAT_PLAYER",
    "EACH_OPPONENT",
];

const ZONES: [&str; 9] = [
    "LIBRARY",
    "HAND",
    "BATTLEFIELD",
    "GRAVEYARD",
    "STACK",
    "EXILE",
    "COMMAND",
    "ANTE",
    "OUTSIDE_GAME",
];

const CMPS: [&str; 6] = ["EQ", "NE", "LT", "LE", "GT", "GE"];

const MATH_OPS: [&str; 7] = ["ADD", "SUB", "MUL", "DIV_FLOOR", "DIV_CEIL", "MIN", "MAX"];

const FLAG_PREDS: [&str; 16] = [
    "IS_CREATURE",
    "IS_LAND",
    "IS_PERMANENT",
    "IS_PLAYER",
    "IS_CARD_OR_TOKEN",
    "IS_TOKEN",
    "IS_COLORLESS",
    "IS_MULTICOLORED",
    "FACE_UP",
    "FACE_DOWN",
    "TAPPED",
    "UNTAPPED",
    "ATTACKING",
    "BLOCKING",
    "ENTERED_THIS_TURN",
    "WAS_CAST",
];

const WORD_PREDS: [&str; 4] = ["HAS_CARD_TYPE", "HAS_SUBTYPE", "HAS_SUPERTYPE", "HAS_COLOR"];

const NUMERIC_PREDS: [&str; 3] = ["MANA_VALUE", "POWER", "TOUGHNESS"];

const OBJECT_ATTRS: [&str; 6] = [
    "POWER",
    "TOUGHNESS",
    "MANA_VALUE",
    "LOYALTY",
    "DEFENSE",
    "COLOR_COUNT",
];

const PLAYER_ATTRS: [&str; 5] = [
    "LIFE",
    "SPEED",
    "HAND_SIZE",
    "LIBRARY_SIZE",
    "GRAVEYARD_SIZE",
];

const DISTINCT_ATTRS: [&str; 8] = [
    "CARD_TYPE",
    "SUBTYPE",
    "SUPERTYPE",
    "COLOR",
    "NAME",
    "MANA_VALUE",
    "POWER",
    "TOUGHNESS",
];

const ORDER_BY: [&str; 5] = [
    "TIMESTAMP",
    "MANA_VALUE",
    "POWER",
    "TOUGHNESS",
    "ZONE_POSITION",
];

/// Synthetic strings only (R5).
const WORDS: [&str; 11] = [
    "Hue",
    "Moor",
    "Wisp",
    "tester",
    "Test Token A",
    "k1",
    "key_2",
    "caf\u{e9}",
    "quote\"d",
    "tab\tx",
    "\u{1F600}",
];

// ---------------------------------------------------------------- helpers

/// Decodes `s` or fails the test with the serde error.
fn decode<T: DeserializeOwned>(s: &str) -> T {
    match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    }
}

/// Full acceptance check for one valid document `s`: it decodes, `to_value` equals the input
/// JSON value, `to_canonical_vec` equals `canonicalize(s)`, and both `from_value(to_value(t))`
/// and the canonical bytes decode back to an equal value.
fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = decode(s);
    let original: Value = serde_json::from_str(s).unwrap();
    let tv = serde_json::to_value(&t).unwrap();
    assert_eq!(tv, original, "to_value of {s}");
    assert_no_float(&tv);
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&t).unwrap()).unwrap(),
        String::from_utf8(canon.clone()).unwrap(),
        "canonical bytes of {s}"
    );
    let back: T = serde_json::from_value(tv).unwrap();
    assert_eq!(back, t, "from_value(to_value) of {s}");
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t, "decode of canonical bytes of {s}");
    t
}

/// The document `s` is rejected both from text and (when it is JSON at all) from a parsed
/// `serde_json::Value`. A panic fails the test.
fn reject<T>(s: &str)
where
    T: DeserializeOwned + Debug,
{
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    let r = serde_json::from_slice::<T>(s.as_bytes());
    assert!(r.is_err(), "expected rejection (slice) of {s}");
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        let r = serde_json::from_value::<T>(v);
        assert!(r.is_err(), "expected rejection (from_value) of {s}");
    }
}

/// No floating-point number appears anywhere in a serialised value (REQ-CORE-006).
fn assert_no_float(v: &Value) {
    match v {
        Value::Number(n) => assert!(n.is_i64() || n.is_u64(), "non-integer number {n}"),
        Value::Array(a) => a.iter().for_each(assert_no_float),
        Value::Object(m) => m.values().for_each(assert_no_float),
        _ => {}
    }
}

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

fn pick<'a>(rng: &mut Pcg64Dxsm, items: &[&'a str]) -> &'a str {
    items[below(rng, items.len() as u64) as usize]
}

/// Writes `v` as JSON text with every object's members in a random order and random whitespace.
fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    const WS: [&str; 4] = ["", " ", "\n", "\t "];
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(&String, &Value)> = m.iter().collect();
            rng.shuffle(&mut entries);
            out.push('{');
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(WS[below(rng, 4) as usize]);
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(WS[below(rng, 4) as usize]);
                out.push(':');
                write_shuffled(val, rng, out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, val) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(WS[below(rng, 4) as usize]);
                write_shuffled(val, rng, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

fn shuffled_text(v: &Value, rng: &mut Pcg64Dxsm) -> String {
    let mut s = String::new();
    write_shuffled(v, rng, &mut s);
    s
}

/// Property check for one generated valid document `v` (REQ-DSL-003, REQ-NFR-006).
fn check_property<T>(v: &Value, rng: &mut Pcg64Dxsm)
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let text = serde_json::to_string(v).unwrap();
    let t: T = decode(&text);
    let tv = serde_json::to_value(&t).unwrap();
    assert_eq!(&tv, v, "to_value of {text}");
    assert_no_float(&tv);
    let back: T = serde_json::from_value(tv).unwrap();
    assert_eq!(back, t, "from_value(to_value(T)) of {text}");
    let c0 = to_canonical_vec(&t).unwrap();
    for _ in 0..10 {
        assert_eq!(
            to_canonical_vec(&t).unwrap(),
            c0,
            "repeat canonical of {text}"
        );
    }
    assert_eq!(
        c0,
        canonicalize(&serde_json::to_vec(&t).unwrap()).unwrap(),
        "to_canonical_vec vs canonicalize(to_vec) of {text}"
    );
    assert_eq!(c0, canonicalize(text.as_bytes()).unwrap());
    let shuffled = shuffled_text(v, rng);
    let s: T = decode(&shuffled);
    assert_eq!(s, t, "shuffled spelling {shuffled}");
    assert_eq!(
        to_canonical_vec(&s).unwrap(),
        c0,
        "shuffled canonical {shuffled}"
    );
    let from_canon: T = serde_json::from_slice(&c0).unwrap();
    assert_eq!(from_canon, t);
}

// ---------------------------------------------------------------- generators (valid trees)

fn gen_str(rng: &mut Pcg64Dxsm) -> String {
    const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_- ";
    if below(rng, 3) == 0 {
        let len = 1 + below(rng, 8) as usize;
        (0..len)
            .map(|_| ALNUM[below(rng, ALNUM.len() as u64) as usize] as char)
            .collect()
    } else {
        pick(rng, &WORDS).to_string()
    }
}

fn gen_var(rng: &mut Pcg64Dxsm) -> String {
    const VAR: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
    let len = 1 + below(rng, 6) as usize;
    let mut s = String::from("$");
    for _ in 0..len {
        s.push(VAR[below(rng, VAR.len() as u64) as usize] as char);
    }
    s
}

/// An integer in the safe range, biased towards the edges.
fn gen_int(rng: &mut Pcg64Dxsm) -> i64 {
    match below(rng, 6) {
        0 => MAX_SAFE_INTEGER,
        1 => -MAX_SAFE_INTEGER,
        2 => 0,
        3 => below(rng, 21) as i64 - 10,
        _ => (below(rng, 2 * MAX_SAFE_INTEGER as u64 + 1) as i64) - MAX_SAFE_INTEGER,
    }
}

fn gen_slot(rng: &mut Pcg64Dxsm) -> u64 {
    match below(rng, 4) {
        0 => 0,
        1 => u64::from(u32::MAX),
        2 => below(rng, 8),
        _ => below(rng, u64::from(u32::MAX) + 1),
    }
}

fn gen_ref_obj(rng: &mut Pcg64Dxsm) -> Obj {
    let mut m = Obj::new();
    match below(rng, 10) {
        0 => {
            m.insert("ref".into(), json!("TARGET"));
            m.insert("slot".into(), json!(gen_slot(rng)));
        }
        1 => {
            m.insert("ref".into(), json!("CHOSEN"));
            m.insert("key".into(), json!(gen_str(rng)));
        }
        2 => {
            m.insert("ref".into(), json!("BOUND"));
            m.insert("var".into(), json!(gen_var(rng)));
        }
        3 => {
            m.insert("ref".into(), json!("LINKED"));
            m.insert("linkId".into(), json!(gen_str(rng)));
        }
        4 => {
            m.insert("ref".into(), json!("REMEMBERED"));
            m.insert("key".into(), json!(gen_str(rng)));
        }
        _ => {
            m.insert("ref".into(), json!(pick(rng, &SIMPLE_REFS)));
        }
    }
    m
}

fn gen_player(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"players": "ALL_PLAYERS"}),
        1 => json!({"players": "TEAMMATES"}),
        _ => Value::Object(gen_ref_obj(rng)),
    }
}

fn gen_word_ref(rng: &mut Pcg64Dxsm) -> Value {
    if below(rng, 2) == 0 {
        json!({"textToken": format!("t{}", below(rng, 100))})
    } else {
        json!({"name": gen_str(rng)})
    }
}

fn gen_pred(rng: &mut Pcg64Dxsm, depth: u32) -> Obj {
    let choice = if depth > 0 {
        below(rng, 20)
    } else {
        3 + below(rng, 17)
    };
    let d = depth.saturating_sub(1);
    let mut m = Obj::new();
    match choice {
        0 | 1 => {
            m.insert("op".into(), json!(if choice == 0 { "AND" } else { "OR" }));
            let n = 1 + below(rng, 3);
            let conds: Vec<Value> = (0..n).map(|_| Value::Object(gen_pred(rng, d))).collect();
            m.insert("conditions".into(), Value::Array(conds));
        }
        2 => {
            m.insert("op".into(), json!("NOT"));
            m.insert("condition".into(), Value::Object(gen_pred(rng, d)));
        }
        4 => {
            m.insert("pred".into(), json!(pick(rng, &WORD_PREDS)));
            m.insert("value".into(), gen_word_ref(rng));
        }
        5 => {
            m.insert("pred".into(), json!("HAS_KEYWORD"));
            m.insert("keyword".into(), json!(gen_str(rng)));
        }
        6 => {
            m.insert("pred".into(), json!("HAS_DESIGNATION"));
            m.insert("kind".into(), json!(gen_str(rng)));
        }
        7 => {
            m.insert("pred".into(), json!("HAS_COUNTER"));
            m.insert("kind".into(), json!(gen_str(rng)));
            if below(rng, 2) == 0 {
                m.insert("cmp".into(), json!(pick(rng, &CMPS)));
                m.insert("value".into(), gen_value(rng, d));
            }
        }
        8 => {
            let name = if below(rng, 2) == 0 {
                "CONTROLLED_BY"
            } else {
                "OWNED_BY"
            };
            m.insert("pred".into(), json!(name));
            m.insert("player".into(), gen_player(rng));
        }
        9 => {
            m.insert("pred".into(), json!(pick(rng, &NUMERIC_PREDS)));
            m.insert("cmp".into(), json!(pick(rng, &CMPS)));
            m.insert("value".into(), gen_value(rng, d));
        }
        10 => {
            m = gen_ref_obj(rng);
            m.insert("pred".into(), json!("SAME_AS"));
        }
        11 if depth > 0 => {
            m.insert("pred".into(), json!("CONTROLS"));
            m.insert("selector".into(), Value::Object(gen_selector(rng, d)));
            if below(rng, 2) == 0 {
                m.insert("player".into(), gen_player(rng));
            }
        }
        12 => {
            m.insert("pred".into(), json!("PLAYER_HAS_MAX_SPEED"));
            m.insert("player".into(), gen_player(rng));
        }
        13 => {
            m.insert("pred".into(), json!("COMPARE"));
            m.insert("cmp".into(), json!(pick(rng, &CMPS)));
            m.insert("lhs".into(), gen_value(rng, d));
            m.insert("rhs".into(), gen_value(rng, d));
        }
        14 => {
            m.insert("pred".into(), json!("TEXT_TOKEN_MATCH"));
            m.insert("textToken".into(), json!(format!("t{}", below(rng, 100))));
            m.insert("value".into(), json!(gen_str(rng)));
        }
        _ => {
            m.insert("pred".into(), json!(pick(rng, &FLAG_PREDS)));
        }
    }
    m
}

fn gen_object_attr(rng: &mut Pcg64Dxsm) -> Value {
    if below(rng, 5) == 0 {
        json!({"COUNTERS": gen_str(rng)})
    } else {
        json!(pick(rng, &OBJECT_ATTRS))
    }
}

fn gen_player_attr(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 7) {
        0 => json!({"COUNTERS": gen_str(rng)}),
        1 => {
            let n = 1 + below(rng, 3);
            let refs: Vec<Value> = (0..n).map(|_| gen_word_ref(rng)).collect();
            json!({ "DEVOTION": refs })
        }
        _ => json!(pick(rng, &PLAYER_ATTRS)),
    }
}

fn gen_history(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    let d = depth.saturating_sub(1);
    match below(rng, 6) {
        0 => {
            let mut inner = Obj::new();
            if below(rng, 2) == 0 {
                inner.insert("player".into(), gen_player(rng));
            }
            if depth > 0 && below(rng, 2) == 0 {
                inner.insert("where".into(), Value::Object(gen_pred(rng, d)));
            }
            json!({ "SPELLS_CAST_THIS_TURN": inner })
        }
        1 => json!({"LIFE_LOST_THIS_TURN": {"player": gen_player(rng)}}),
        2 => json!({"LIFE_GAINED_THIS_TURN": {"player": gen_player(rng)}}),
        3 => json!({"CARDS_DRAWN_THIS_TURN": {"player": gen_player(rng)}}),
        4 => {
            let mut inner = Obj::new();
            if depth > 0 && below(rng, 2) == 0 {
                inner.insert("where".into(), Value::Object(gen_pred(rng, d)));
            }
            json!({ "CREATURES_DIED_THIS_TURN": inner })
        }
        _ => {
            let mut inner = gen_ref_obj(rng);
            if below(rng, 2) == 0 {
                inner.insert("color".into(), gen_word_ref(rng));
            }
            json!({ "MANA_SPENT_TO_CAST": inner })
        }
    }
}

fn gen_value(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    let choice = if depth > 0 {
        below(rng, 11)
    } else {
        below(rng, 6)
    };
    let d = depth.saturating_sub(1);
    match choice {
        0 => json!({"LITERAL": gen_int(rng)}),
        1 => json!("X"),
        2 => json!({"MEMORY": gen_str(rng)}),
        3 => {
            let mut m = gen_ref_obj(rng);
            m.insert("attr".into(), gen_object_attr(rng));
            json!({ "ATTR": m })
        }
        4 => {
            let mut m = gen_ref_obj(rng);
            m.insert("attr".into(), gen_player_attr(rng));
            json!({ "PLAYER_ATTR": m })
        }
        5 | 9 => json!({ "HISTORY": gen_history(rng, if choice == 5 { 0 } else { d }) }),
        6 => json!({ "COUNT": gen_selector(rng, d) }),
        7 | 10 => json!({"MATH": {
            "op": pick(rng, &MATH_OPS),
            "lhs": gen_value(rng, d),
            "rhs": gen_value(rng, d),
        }}),
        _ => json!({"DISTINCT": {
            "attr": pick(rng, &DISTINCT_ATTRS),
            "selector": gen_selector(rng, d),
        }}),
    }
}

/// A random valid selector. The flat predicate at the top of a filter selector is never
/// SAME_AS (that shape is tested on its own in `req_dsl_003_selector_with_flat_same_as`).
fn gen_selector(rng: &mut Pcg64Dxsm, depth: u32) -> Obj {
    if below(rng, 4) == 0 {
        return gen_ref_obj(rng);
    }
    let has_zones = below(rng, 2) == 0;
    let has_pred = !has_zones || below(rng, 2) == 0;
    let mut m = Obj::new();
    if has_pred {
        let p = loop {
            let p = gen_pred(rng, depth);
            if p.get("pred") != Some(&json!("SAME_AS")) {
                break p;
            }
        };
        m.extend(p);
    }
    if has_zones {
        let mut zones: Vec<&str> = ZONES.to_vec();
        rng.shuffle(&mut zones);
        let n = 1 + below(rng, 9) as usize;
        zones.truncate(n);
        m.insert("zones".into(), json!(zones));
    }
    if below(rng, 3) == 0 {
        m.insert("count".into(), gen_value(rng, depth.saturating_sub(1)));
    }
    if below(rng, 3) == 0 {
        let dir = if below(rng, 2) == 0 { "ASC" } else { "DESC" };
        m.insert(
            "ordering".into(),
            json!({"by": pick(rng, &ORDER_BY), "direction": dir}),
        );
    }
    m
}

// ---------------------------------------------------------------- references

/// REQ-DSL-003: every parameterless reference name is accepted and round-trips exactly.
#[test]
fn req_dsl_003_parameterless_references() {
    for name in SIMPLE_REFS {
        let s = format!("{{\"ref\":\"{name}\"}}");
        accept::<ReferenceAST>(&s);
        accept::<PlayerSelectorAST>(&s);
        accept::<ObjectSelectorAST>(&s);
    }
}

/// REQ-DSL-003: every parameterised reference form is accepted and round-trips exactly.
#[test]
fn req_dsl_003_parameterised_references() {
    let docs = [
        r#"{"ref":"TARGET","slot":0}"#,
        r#"{"ref":"TARGET","slot":1}"#,
        r#"{"ref":"TARGET","slot":4294967295}"#,
        r#"{"ref":"CHOSEN","key":"Hue"}"#,
        r#"{"ref":"BOUND","var":"$x"}"#,
        r#"{"ref":"BOUND","var":"$_"}"#,
        r#"{"ref":"BOUND","var":"$0"}"#,
        r#"{"ref":"BOUND","var":"$Abc_09z"}"#,
        r#"{"ref":"LINKED","linkId":"link-1"}"#,
        r#"{"ref":"REMEMBERED","key":"Moor"}"#,
        r#"{"key":"Wisp","ref":"REMEMBERED"}"#,
    ];
    for d in docs {
        accept::<ReferenceAST>(d);
        accept::<PlayerSelectorAST>(d);
        accept::<ObjectSelectorAST>(d);
    }
}

/// REQ-DSL-003: reference rejection rules (unknown/lowercase names, extra or missing members,
/// nulls, empty strings, bad `$var` spellings, wrong member spellings).
#[test]
fn req_dsl_003_reference_rejections() {
    let docs = [
        r#"{"ref":"NOPE"}"#,
        r#"{"ref":"self"}"#,
        r#"{"ref":"Self"}"#,
        r#"{"ref":"target","slot":0}"#,
        r#"{"ref":""}"#,
        r#"{"ref":null}"#,
        r#"{"ref":"TARGET(0)"}"#,
        r#"{}"#,
        r#""SELF""#,
        r#"["SELF"]"#,
        r#"null"#,
        r#"{"ref":"SELF","slot":0}"#,
        r#"{"ref":"SELF","key":"a"}"#,
        r#"{"ref":"SELF","extra":true}"#,
        r#"{"ref":"SELF","ref2":"SELF"}"#,
        r#"{"ref":"TARGET"}"#,
        r#"{"ref":"TARGET","slot":null}"#,
        r#"{"ref":"TARGET","slot":"0"}"#,
        r#"{"ref":"TARGET","slot":0,"key":"a"}"#,
        r#"{"ref":"CHOSEN"}"#,
        r#"{"ref":"CHOSEN","key":""}"#,
        r#"{"ref":"CHOSEN","key":null}"#,
        r#"{"ref":"CHOSEN","key":1}"#,
        r#"{"ref":"CHOSEN","linkId":"a"}"#,
        r#"{"ref":"BOUND"}"#,
        r#"{"ref":"BOUND","var":""}"#,
        r#"{"ref":"BOUND","var":"x"}"#,
        r#"{"ref":"BOUND","var":"$"}"#,
        r#"{"ref":"BOUND","var":"$a-b"}"#,
        r#"{"ref":"BOUND","var":"$a b"}"#,
        r#"{"ref":"BOUND","var":"$café"}"#,
        r#"{"ref":"BOUND","var":"$$a"}"#,
        r#"{"ref":"BOUND","var":"a$b"}"#,
        r#"{"ref":"BOUND","var":" $a"}"#,
        r#"{"ref":"BOUND","var":"$a\n"}"#,
        r#"{"ref":"BOUND","var":null}"#,
        r#"{"ref":"LINKED"}"#,
        r#"{"ref":"LINKED","linkId":""}"#,
        r#"{"ref":"LINKED","link_id":"a"}"#,
        r#"{"ref":"LINKED","linkid":"a"}"#,
        r#"{"ref":"LINKED","LinkId":"a"}"#,
        r#"{"ref":"REMEMBERED"}"#,
        r#"{"ref":"REMEMBERED","key":""}"#,
        r#"{"ref":"REMEMBERED","key":null}"#,
        r#"{"ref":{"ref":"SELF"}}"#,
    ];
    for d in docs {
        reject::<ReferenceAST>(d);
    }
}

// ---------------------------------------------------------------- player selectors

/// REQ-DSL-003: the two `players` forms are accepted; references were covered above.
#[test]
fn req_dsl_003_player_selector_forms() {
    accept::<PlayerSelectorAST>(r#"{"players":"ALL_PLAYERS"}"#);
    accept::<PlayerSelectorAST>(r#"{"players":"TEAMMATES"}"#);
    let a: PlayerSelectorAST = decode(r#"{"players":"ALL_PLAYERS"}"#);
    let b: PlayerSelectorAST = decode(r#"{"players":"TEAMMATES"}"#);
    let c: PlayerSelectorAST = decode(r#"{"ref":"CONTROLLER"}"#);
    assert_ne!(a, b);
    assert_ne!(a, c);
}

/// REQ-DSL-003: player selector rejection rules.
#[test]
fn req_dsl_003_player_selector_rejections() {
    let docs = [
        r#"{"players":"all_players"}"#,
        r#"{"players":"OPPONENTS"}"#,
        r#"{"players":""}"#,
        r#"{"players":null}"#,
        r#"{"players":"ALL_PLAYERS","ref":"SELF"}"#,
        r#"{"players":"TEAMMATES","extra":1}"#,
        r#"{"Players":"TEAMMATES"}"#,
        r#"{}"#,
        r#""ALL_PLAYERS""#,
        r#"{"ref":"NOPE"}"#,
        r#"{"ref":"SELF","slot":0}"#,
    ];
    for d in docs {
        reject::<PlayerSelectorAST>(d);
    }
}

// ---------------------------------------------------------------- zones / object selectors

/// REQ-DSL-003: every Zone string is accepted alone and inside a selector; others rejected.
#[test]
fn req_dsl_003_zones() {
    for z in ZONES {
        accept::<Zone>(&format!("\"{z}\""));
        accept::<ObjectSelectorAST>(&format!("{{\"zones\":[\"{z}\"]}}"));
    }
    let all = serde_json::to_string(&ZONES).unwrap();
    accept::<ObjectSelectorAST>(&format!("{{\"zones\":{all}}}"));
    for bad in [
        "\"battlefield\"",
        "\"Battlefield\"",
        "\"BATTLEFIELD \"",
        "\"SIDEBOARD\"",
        "\"\"",
        "null",
        "0",
    ] {
        reject::<Zone>(bad);
        reject::<ObjectSelectorAST>(&format!("{{\"zones\":[{bad}]}}"));
    }
}

/// REQ-DSL-003: the zones array keeps the order given (it is not normalised): two orders are
/// different values and each re-serialises in its own order.
#[test]
fn req_dsl_003_zone_order_preserved() {
    let a = accept::<ObjectSelectorAST>(r#"{"zones":["HAND","BATTLEFIELD"]}"#);
    let b = accept::<ObjectSelectorAST>(r#"{"zones":["BATTLEFIELD","HAND"]}"#);
    assert_ne!(a, b);
    let mut rev: Vec<&str> = ZONES.to_vec();
    rev.reverse();
    let fwd = accept::<ObjectSelectorAST>(&format!(
        "{{\"zones\":{}}}",
        serde_json::to_string(&ZONES).unwrap()
    ));
    let bwd = accept::<ObjectSelectorAST>(&format!(
        "{{\"zones\":{}}}",
        serde_json::to_string(&rev).unwrap()
    ));
    assert_ne!(fwd, bwd);
}

/// REQ-DSL-003: object selector filter forms, including the SRS §14.4.0/§14.5 examples.
#[test]
fn req_dsl_003_object_selector_filter_forms() {
    let docs = [
        r#"{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}"#,
        r#"{"pred":"IS_CREATURE"}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}"#,
        r#"{"op":"AND","conditions":[{"pred":"IS_LAND"},{"op":"NOT","condition":{"pred":"TAPPED"}}]}"#,
        r#"{"zones":["HAND","GRAVEYARD"],"count":{"LITERAL":2},"ordering":{"by":"TIMESTAMP","direction":"DESC"},"op":"AND","conditions":[{"pred":"IS_LAND"},{"op":"NOT","condition":{"pred":"TAPPED"}}]}"#,
        r#"{"count":{"LITERAL":1},"pred":"IS_TOKEN"}"#,
        r#"{"ordering":{"by":"POWER","direction":"ASC"},"pred":"IS_TOKEN"}"#,
        r#"{"zones":["LIBRARY"],"count":"X","ordering":{"by":"ZONE_POSITION","direction":"ASC"}}"#,
        r#"{"zones":["EXILE"],"count":{"COUNT":{"zones":["HAND"]}}}"#,
        r#"{"zones":["BATTLEFIELD"],"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}}"#,
        r#"{"zones":["BATTLEFIELD"],"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":{"LITERAL":3}}"#,
    ];
    for d in docs {
        accept::<ObjectSelectorAST>(d);
    }
    for by in ORDER_BY {
        for dir in ["ASC", "DESC"] {
            accept::<ObjectSelectorAST>(&format!(
                "{{\"zones\":[\"HAND\"],\"ordering\":{{\"by\":\"{by}\",\"direction\":\"{dir}\"}}}}"
            ));
        }
    }
}

/// REQ-DSL-003: a SAME_AS predicate written flat in a filter selector carries `ref` members
/// next to `pred`; it is the filter form (it has a predicate) and round-trips exactly.
#[test]
fn req_dsl_003_selector_with_flat_same_as() {
    accept::<ObjectSelectorAST>(r#"{"pred":"SAME_AS","ref":"TARGET","slot":0}"#);
    accept::<ObjectSelectorAST>(r#"{"zones":["BATTLEFIELD"],"pred":"SAME_AS","ref":"SELF"}"#);
}

/// REQ-DSL-003: object selector rejection rules.
#[test]
fn req_dsl_003_object_selector_rejections() {
    let docs = [
        r#"{}"#,
        r#"{"count":{"LITERAL":1}}"#,
        r#"{"ordering":{"by":"TIMESTAMP","direction":"ASC"}}"#,
        r#"{"count":{"LITERAL":1},"ordering":{"by":"TIMESTAMP","direction":"ASC"}}"#,
        r#"{"ref":"SELF","zones":["HAND"]}"#,
        r#"{"ref":"SELF","count":{"LITERAL":1}}"#,
        r#"{"ref":"SELF","ordering":{"by":"TIMESTAMP","direction":"ASC"}}"#,
        r#"{"ref":"TARGET","slot":0,"zones":["HAND"]}"#,
        r#"{"zones":["HAND","HAND"]}"#,
        r#"{"zones":["HAND","BATTLEFIELD","HAND"]}"#,
        r#"{"zones":[]}"#,
        r#"{"zones":[],"pred":"IS_CREATURE"}"#,
        r#"{"zones":"HAND"}"#,
        r#"{"zones":null,"pred":"IS_CREATURE"}"#,
        r#"{"zones":["HAND"],"count":null}"#,
        r#"{"zones":["HAND"],"ordering":null}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"TIMESTAMP"}}"#,
        r#"{"zones":["HAND"],"ordering":{"direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"NAME","direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"timestamp","direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"TIMESTAMP","direction":"asc"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"TIMESTAMP","direction":"ASCENDING"}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":"TIMESTAMP","direction":"ASC","extra":1}}"#,
        r#"{"zones":["HAND"],"ordering":{"by":null,"direction":"ASC"}}"#,
        r#"{"zones":["HAND"],"extra":1}"#,
        r#"{"zones":["HAND"],"pred":"NOPE"}"#,
        r#"{"zones":["HAND"],"pred":"HAS_SUBTYPE"}"#,
        r#"{"zones":["HAND"],"pred":null}"#,
        r#"{"zones":["HAND"],"count":1}"#,
        r#"{"zones":["HAND"],"selector":{"zones":["HAND"]}}"#,
        r#"{"Zones":["HAND"]}"#,
        r#"[]"#,
        r#""SELF""#,
    ];
    for d in docs {
        reject::<ObjectSelectorAST>(d);
    }
}

// ---------------------------------------------------------------- predicates

/// REQ-DSL-003: every parameterless predicate name is accepted.
#[test]
fn req_dsl_003_parameterless_predicates() {
    for name in FLAG_PREDS {
        let s = format!("{{\"pred\":\"{name}\"}}");
        accept::<PredicateAST>(&s);
        accept::<ObjectSelectorAST>(&s);
    }
}

/// REQ-DSL-003, REQ-DSL-012: every predicate with members, in every member shape.
#[test]
fn req_dsl_003_predicates_with_members() {
    let mut docs: Vec<String> = Vec::new();
    for p in WORD_PREDS {
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"value\":{{\"textToken\":\"t14\"}}}}"
        ));
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"value\":{{\"name\":\"Moor\"}}}}"
        ));
    }
    for p in NUMERIC_PREDS {
        for c in CMPS {
            docs.push(format!(
                "{{\"pred\":\"{p}\",\"cmp\":\"{c}\",\"value\":{{\"LITERAL\":2}}}}"
            ));
        }
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"cmp\":\"LE\",\"value\":\"X\"}}"
        ));
    }
    for c in CMPS {
        docs.push(format!(
            "{{\"pred\":\"COMPARE\",\"cmp\":\"{c}\",\"lhs\":{{\"LITERAL\":1}},\"rhs\":\"X\"}}"
        ));
    }
    for p in ["CONTROLLED_BY", "OWNED_BY", "PLAYER_HAS_MAX_SPEED"] {
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"player\":{{\"players\":\"TEAMMATES\"}}}}"
        ));
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"player\":{{\"ref\":\"CONTROLLER\"}}}}"
        ));
        docs.push(format!(
            "{{\"pred\":\"{p}\",\"player\":{{\"ref\":\"TARGET\",\"slot\":1}}}}"
        ));
    }
    let fixed = [
        r#"{"pred":"HAS_KEYWORD","keyword":"tester"}"#,
        r#"{"pred":"HAS_DESIGNATION","kind":"Wisp"}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge"}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":{"LITERAL":2}}"#,
        r#"{"pred":"SAME_AS","ref":"TARGET","slot":0}"#,
        r#"{"pred":"SAME_AS","ref":"SELF"}"#,
        r#"{"pred":"SAME_AS","ref":"BOUND","var":"$it"}"#,
        r#"{"pred":"SAME_AS","ref":"LINKED","linkId":"l1"}"#,
        r#"{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}}"#,
        r#"{"pred":"CONTROLS","selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"player":{"ref":"ACTIVE_PLAYER"}}"#,
        r#"{"pred":"CONTROLS","selector":{"ref":"SELF"}}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t3","value":"Moor"}"#,
        r#"{"op":"AND","conditions":[{"pred":"IS_CREATURE"}]}"#,
        r#"{"op":"OR","conditions":[{"pred":"IS_CREATURE"},{"pred":"IS_LAND"},{"pred":"IS_TOKEN"}]}"#,
        r#"{"op":"NOT","condition":{"op":"OR","conditions":[{"pred":"TAPPED"}]}}"#,
    ];
    docs.extend(fixed.iter().map(|s| s.to_string()));
    for d in &docs {
        accept::<PredicateAST>(d);
    }
}

/// REQ-DSL-003: the SRS §14.4.0 / §14.5 example `{"pred":"CONTROLS","selector":{"pred":
/// "HAS_SUBTYPE","value":{"textToken":"t14"}}}` decodes as a PredicateAST and is byte-exact
/// canonical.
#[test]
fn req_dsl_003_srs_examples() {
    let s = r#"{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}}"#;
    let p = accept::<PredicateAST>(s);
    assert_eq!(to_canonical_vec(&p).unwrap(), s.as_bytes());
    let s = r#"{"pred":"IS_CREATURE","zones":["BATTLEFIELD"]}"#;
    let sel = accept::<ObjectSelectorAST>(s);
    assert_eq!(to_canonical_vec(&sel).unwrap(), s.as_bytes());
    // The flat serialisation keeps predicate members next to `zones`.
    let v = serde_json::to_value(&sel).unwrap();
    assert_eq!(v, json!({"zones": ["BATTLEFIELD"], "pred": "IS_CREATURE"}));
}

/// REQ-DSL-003: HAS_COUNTER's optional `cmp`+`value` pair: absent and present forms are
/// different values; member order (given and reversed) does not matter.
#[test]
fn req_dsl_003_has_counter_pair_order() {
    let a = accept::<PredicateAST>(
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":{"LITERAL":2}}"#,
    );
    let b: PredicateAST =
        decode(r#"{"value":{"LITERAL":2},"cmp":"GE","kind":"charge","pred":"HAS_COUNTER"}"#);
    assert_eq!(a, b);
    let c = accept::<PredicateAST>(r#"{"pred":"HAS_COUNTER","kind":"charge"}"#);
    let d: PredicateAST = decode(r#"{"kind":"charge","pred":"HAS_COUNTER"}"#);
    assert_eq!(c, d);
    assert_ne!(a, c);
}

/// REQ-DSL-003: AND/OR keep their condition order.
#[test]
fn req_dsl_003_condition_order_preserved() {
    let a = accept::<PredicateAST>(
        r#"{"op":"AND","conditions":[{"pred":"TAPPED"},{"pred":"IS_LAND"}]}"#,
    );
    let b = accept::<PredicateAST>(
        r#"{"op":"AND","conditions":[{"pred":"IS_LAND"},{"pred":"TAPPED"}]}"#,
    );
    assert_ne!(a, b);
    let c = accept::<PredicateAST>(
        r#"{"op":"OR","conditions":[{"pred":"TAPPED"},{"pred":"IS_LAND"}]}"#,
    );
    assert_ne!(a, c);
}

/// REQ-DSL-003, REQ-DSL-012: predicate rejection rules.
#[test]
fn req_dsl_003_predicate_rejections() {
    let docs = [
        r#"{}"#,
        r#"{"pred":"NOPE"}"#,
        r#"{"pred":"is_creature"}"#,
        r#"{"pred":"Is_Creature"}"#,
        r#"{"pred":""}"#,
        r#"{"pred":null}"#,
        r#""IS_CREATURE""#,
        r#"{"pred":"IS_CREATURE","extra":1}"#,
        r#"{"pred":"IS_CREATURE","value":{"textToken":"t1"}}"#,
        r#"{"pred":"IS_CREATURE","op":"NOT","condition":{"pred":"TAPPED"}}"#,
        r#"{"op":"and","conditions":[{"pred":"IS_CREATURE"}]}"#,
        r#"{"op":"XOR","conditions":[{"pred":"IS_CREATURE"}]}"#,
        r#"{"op":"AND","conditions":[]}"#,
        r#"{"op":"OR","conditions":[]}"#,
        r#"{"op":"AND"}"#,
        r#"{"op":"AND","conditions":null}"#,
        r#"{"op":"AND","condition":{"pred":"IS_CREATURE"}}"#,
        r#"{"op":"AND","conditions":{"pred":"IS_CREATURE"}}"#,
        r#"{"op":"NOT"}"#,
        r#"{"op":"NOT","condition":null}"#,
        r#"{"op":"NOT","conditions":[{"pred":"IS_CREATURE"}]}"#,
        r#"{"op":"NOT","condition":{"pred":"IS_CREATURE"},"extra":0}"#,
        r#"{"op":"AND","conditions":[{"pred":"NOPE"}]}"#,
        r#"{"pred":"HAS_SUBTYPE"}"#,
        r#"{"pred":"HAS_SUBTYPE","value":null}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":"Moor"}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":""}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"name":""}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":null}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"text_token":"t1"}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t1","name":"Moor"}}"#,
        r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t1"},"extra":1}"#,
        r#"{"pred":"HAS_COLOR","value":{"name":"Hue","extra":1}}"#,
        r#"{"pred":"HAS_KEYWORD"}"#,
        r#"{"pred":"HAS_KEYWORD","keyword":""}"#,
        r#"{"pred":"HAS_KEYWORD","keyword":null}"#,
        r#"{"pred":"HAS_DESIGNATION"}"#,
        r#"{"pred":"HAS_DESIGNATION","kind":""}"#,
        r#"{"pred":"HAS_COUNTER"}"#,
        r#"{"pred":"HAS_COUNTER","kind":""}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE"}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","value":{"LITERAL":2}}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":null,"value":{"LITERAL":2}}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":null}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"eq","value":{"LITERAL":2}}"#,
        r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":2}"#,
        r#"{"pred":"CONTROLLED_BY"}"#,
        r#"{"pred":"CONTROLLED_BY","player":null}"#,
        r#"{"pred":"OWNED_BY","player":{"players":"NOBODY"}}"#,
        r#"{"pred":"OWNED_BY","player":{"ref":"CONTROLLER"},"extra":1}"#,
        r#"{"pred":"MANA_VALUE","value":{"LITERAL":2}}"#,
        r#"{"pred":"MANA_VALUE","cmp":"EQ"}"#,
        r#"{"pred":"POWER","cmp":"eq","value":{"LITERAL":2}}"#,
        r#"{"pred":"POWER","cmp":"==","value":{"LITERAL":2}}"#,
        r#"{"pred":"POWER","cmp":"NEQ","value":{"LITERAL":2}}"#,
        r#"{"pred":"POWER","cmp":"","value":{"LITERAL":2}}"#,
        r#"{"pred":"TOUGHNESS","cmp":"LT","value":2}"#,
        r#"{"pred":"TOUGHNESS","cmp":"LT","value":"x"}"#,
        r#"{"pred":"SAME_AS"}"#,
        r#"{"pred":"SAME_AS","ref":{"ref":"SELF"}}"#,
        r#"{"pred":"SAME_AS","ref":"TARGET"}"#,
        r#"{"pred":"SAME_AS","ref":"SELF","slot":0}"#,
        r#"{"pred":"SAME_AS","ref":"NOPE"}"#,
        r#"{"pred":"CONTROLS"}"#,
        r#"{"pred":"CONTROLS","selector":{}}"#,
        r#"{"pred":"CONTROLS","selector":null}"#,
        r#"{"pred":"CONTROLS","selector":{"ref":"SELF"},"player":null}"#,
        r#"{"pred":"CONTROLS","selector":{"ref":"SELF"},"extra":1}"#,
        r#"{"pred":"PLAYER_HAS_MAX_SPEED"}"#,
        r#"{"pred":"PLAYER_HAS_MAX_SPEED","player":null}"#,
        r#"{"pred":"COMPARE","cmp":"EQ","lhs":{"LITERAL":1}}"#,
        r#"{"pred":"COMPARE","cmp":"EQ","rhs":{"LITERAL":1}}"#,
        r#"{"pred":"COMPARE","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}"#,
        r#"{"pred":"COMPARE","cmp":"EQ","lhs":{"LITERAL":1},"rhs":{"LITERAL":1},"extra":1}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t3"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","value":"Moor"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"","value":"Moor"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t3","value":""}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"t3","value":{"name":"Moor"}}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":null,"value":"Moor"}"#,
        r#"{"pred":"TEXT_TOKEN_MATCH","text_token":"t3","value":"Moor"}"#,
        r#"{"pred":"IS_CREATURE","zones":["HAND"]}"#,
    ];
    for d in docs {
        reject::<PredicateAST>(d);
    }
}

/// REQ-DSL-003: every Cmp string is accepted and nothing else.
#[test]
fn req_dsl_003_cmp_strings() {
    for c in CMPS {
        accept::<PredicateAST>(&format!(
            "{{\"pred\":\"POWER\",\"cmp\":\"{c}\",\"value\":{{\"LITERAL\":0}}}}"
        ));
    }
    for bad in ["eq", "Eq", "==", "!=", "<", "LTE", "GTE", "NEQ", "", "EQ "] {
        reject::<PredicateAST>(&format!(
            "{{\"pred\":\"POWER\",\"cmp\":\"{bad}\",\"value\":{{\"LITERAL\":0}}}}"
        ));
    }
}

// ---------------------------------------------------------------- by-id text-token references

/// REQ-DSL-012: a HAS_SUBTYPE word reference and a TEXT_TOKEN_MATCH carry the token id
/// verbatim (no stream is consulted; any non-empty id decodes).
#[test]
fn req_dsl_012_by_id_references() {
    let p = accept::<PredicateAST>(r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}"#);
    assert_eq!(
        serde_json::to_value(&p).unwrap()["value"]["textToken"],
        json!("t14")
    );
    let q = accept::<PredicateAST>(r#"{"pred":"HAS_SUBTYPE","value":{"textToken":"t15"}}"#);
    assert_ne!(p, q);
    let by_name = accept::<PredicateAST>(r#"{"pred":"HAS_SUBTYPE","value":{"name":"t14"}}"#);
    assert_ne!(p, by_name);
    let m = accept::<PredicateAST>(
        r#"{"pred":"TEXT_TOKEN_MATCH","textToken":"no-such-id","value":"Wisp"}"#,
    );
    assert_eq!(
        serde_json::to_value(&m).unwrap()["textToken"],
        json!("no-such-id")
    );
    accept::<ValueExpressionAST>(
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":{"DEVOTION":[{"textToken":"t2"},{"name":"Hue"}]}}}"#,
    );
}

/// REQ-DSL-012: a WordRef with both members, or neither, is rejected wherever it appears.
#[test]
fn req_dsl_012_word_ref_exactly_one_member() {
    for wr in [
        r#"{"textToken":"t14","name":"Moor"}"#,
        r#"{"name":"Moor","textToken":"t14"}"#,
        r#"{}"#,
    ] {
        for p in WORD_PREDS {
            reject::<PredicateAST>(&format!("{{\"pred\":\"{p}\",\"value\":{wr}}}"));
        }
        reject::<ValueExpressionAST>(&format!(
            "{{\"PLAYER_ATTR\":{{\"ref\":\"CONTROLLER\",\"attr\":{{\"DEVOTION\":[{wr}]}}}}}}"
        ));
        reject::<ValueExpressionAST>(&format!(
            "{{\"HISTORY\":{{\"MANA_SPENT_TO_CAST\":{{\"ref\":\"SELF\",\"color\":{wr}}}}}}}"
        ));
    }
}

// ---------------------------------------------------------------- value expressions

/// REQ-DSL-003: every ValueExpressionAST variant, attribute and MathOp is accepted.
#[test]
fn req_dsl_003_value_expression_variants() {
    let mut docs: Vec<String> = vec![
        r#"{"LITERAL":0}"#.into(),
        r#"{"LITERAL":-7}"#.into(),
        r#""X""#.into(),
        r#"{"COUNT":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}}"#.into(),
        r#"{"COUNT":{"ref":"SELF"}}"#.into(),
        r#"{"ATTR":{"ref":"TARGET","slot":0,"attr":"POWER"}}"#.into(),
        r#"{"ATTR":{"ref":"SELF","attr":{"COUNTERS":"charge"}}}"#.into(),
        r#"{"ATTR":{"ref":"BOUND","var":"$it","attr":"MANA_VALUE"}}"#.into(),
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"LIFE"}}"#.into(),
        r#"{"PLAYER_ATTR":{"ref":"CHOSEN","key":"p","attr":{"COUNTERS":"tester"}}}"#.into(),
        r#"{"PLAYER_ATTR":{"ref":"ACTIVE_PLAYER","attr":{"DEVOTION":[{"name":"Hue"}]}}}"#.into(),
        r#"{"DISTINCT":{"attr":"NAME","selector":{"zones":["GRAVEYARD"]}}}"#.into(),
        r#"{"MEMORY":"k1"}"#.into(),
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{}}}"#.into(),
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"player":{"ref":"CONTROLLER"}}}}"#.into(),
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"where":{"pred":"IS_CREATURE"}}}}"#.into(),
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"player":{"players":"ALL_PLAYERS"},"where":{"op":"NOT","condition":{"pred":"IS_LAND"}}}}}"#.into(),
        r#"{"HISTORY":{"LIFE_LOST_THIS_TURN":{"player":{"ref":"THAT_PLAYER"}}}}"#.into(),
        r#"{"HISTORY":{"LIFE_GAINED_THIS_TURN":{"player":{"ref":"CONTROLLER"}}}}"#.into(),
        r#"{"HISTORY":{"CARDS_DRAWN_THIS_TURN":{"player":{"players":"TEAMMATES"}}}}"#.into(),
        r#"{"HISTORY":{"CREATURES_DIED_THIS_TURN":{}}}"#.into(),
        r#"{"HISTORY":{"CREATURES_DIED_THIS_TURN":{"where":{"pred":"IS_TOKEN"}}}}"#.into(),
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":"SELF"}}}"#.into(),
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":"TARGET","slot":3,"color":{"name":"Hue"}}}}"#.into(),
    ];
    for a in OBJECT_ATTRS {
        docs.push(format!(
            "{{\"ATTR\":{{\"ref\":\"SELF\",\"attr\":\"{a}\"}}}}"
        ));
    }
    for a in PLAYER_ATTRS {
        docs.push(format!(
            "{{\"PLAYER_ATTR\":{{\"ref\":\"CONTROLLER\",\"attr\":\"{a}\"}}}}"
        ));
    }
    for a in DISTINCT_ATTRS {
        docs.push(format!(
            "{{\"DISTINCT\":{{\"attr\":\"{a}\",\"selector\":{{\"pred\":\"IS_CREATURE\"}}}}}}"
        ));
    }
    for op in MATH_OPS {
        docs.push(format!(
            "{{\"MATH\":{{\"op\":\"{op}\",\"lhs\":{{\"LITERAL\":1}},\"rhs\":\"X\"}}}}"
        ));
    }
    for d in &docs {
        accept::<ValueExpressionAST>(d);
    }
    // The ATTR reference is written flat: `ref` and `slot` sit next to `attr`.
    let v: ValueExpressionAST = decode(r#"{"ATTR":{"ref":"TARGET","slot":0,"attr":"POWER"}}"#);
    assert_eq!(
        serde_json::to_value(&v).unwrap(),
        json!({"ATTR": {"ref": "TARGET", "slot": 0, "attr": "POWER"}})
    );
}

/// REQ-DSL-003: the X expression has exactly one spelling, the bare string "X".
#[test]
fn req_dsl_003_x_single_spelling() {
    let x = accept::<ValueExpressionAST>(r#""X""#);
    assert_eq!(serde_json::to_string(&x).unwrap(), r#""X""#);
    for bad in [
        r#"{"X":true}"#,
        r#"{"X":null}"#,
        r#"{"X":{}}"#,
        r#"{"X":[]}"#,
        r#"{"X":0}"#,
        r#""x""#,
        r#"" X""#,
        r#""XX""#,
        r#"["X"]"#,
        r#""LITERAL""#,
        r#""""#,
    ] {
        reject::<ValueExpressionAST>(bad);
    }
}

/// REQ-DSL-003: value expression rejection rules.
#[test]
fn req_dsl_003_value_expression_rejections() {
    let docs = [
        r#"{}"#,
        r#"null"#,
        r#"1"#,
        r#""Y""#,
        r#"{"LITERALS":1}"#,
        r#"{"literal":1}"#,
        r#"{"Literal":1}"#,
        r#"{"LITERAL":1,"MEMORY":"a"}"#,
        r#"{"LITERAL":null}"#,
        r#"{"MEMORY":""}"#,
        r#"{"MEMORY":null}"#,
        r#"{"MEMORY":1}"#,
        r#"{"memory":"k"}"#,
        r#"{"COUNT":{}}"#,
        r#"{"COUNT":null}"#,
        r#"{"COUNT":{"count":{"LITERAL":1}}}"#,
        r#"{"ATTR":{"ref":"SELF"}}"#,
        r#"{"ATTR":{"attr":"POWER"}}"#,
        r#"{"ATTR":{"ref":{"ref":"SELF"},"attr":"POWER"}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":"LIFE"}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":"power"}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":{"COUNTERS":""}}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":{"COUNTERS":null}}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":{"DEVOTION":[{"name":"Hue"}]}}}"#,
        r#"{"ATTR":{"ref":"SELF","attr":"POWER","extra":1}}"#,
        r#"{"ATTR":{"ref":"SELF","slot":0,"attr":"POWER"}}"#,
        r#"{"ATTR":{"ref":"TARGET","attr":"POWER"}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"POWER"}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":"life"}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":{"DEVOTION":[]}}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":{"DEVOTION":null}}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":{"DEVOTION":[{"name":""}]}}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER","attr":{"COUNTERS":""}}}"#,
        r#"{"PLAYER_ATTR":{"ref":"CONTROLLER"}}"#,
        r#"{"PLAYER_ATTR":{"players":"ALL_PLAYERS","attr":"LIFE"}}"#,
        r#"{"MATH":{"op":"ADD","lhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"ADD","rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"ADD","lhs":{"LITERAL":1},"rhs":{"LITERAL":1},"extra":0}}"#,
        r#"{"MATH":{"op":"add","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"DIV","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"DIVFLOOR","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"MOD","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"","lhs":{"LITERAL":1},"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"ADD","lhs":1,"rhs":{"LITERAL":1}}}"#,
        r#"{"MATH":{"op":"ADD","lhs":null,"rhs":{"LITERAL":1}}}"#,
        r#"{"DISTINCT":{"attr":"LOYALTY","selector":{"pred":"IS_CREATURE"}}}"#,
        r#"{"DISTINCT":{"attr":"name","selector":{"pred":"IS_CREATURE"}}}"#,
        r#"{"DISTINCT":{"attr":"NAME"}}"#,
        r#"{"DISTINCT":{"selector":{"pred":"IS_CREATURE"}}}"#,
        r#"{"DISTINCT":{"attr":"NAME","selector":{}}}"#,
        r#"{"HISTORY":{}}"#,
        r#"{"HISTORY":null}"#,
        r#"{"HISTORY":"SPELLS_CAST_THIS_TURN"}"#,
        r#"{"HISTORY":{"SPELLS_CAST":{}}}"#,
        r#"{"HISTORY":{"spells_cast_this_turn":{}}}"#,
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":null}}"#,
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{},"CREATURES_DIED_THIS_TURN":{}}}"#,
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"player":null}}}"#,
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"where":null}}}"#,
        r#"{"HISTORY":{"SPELLS_CAST_THIS_TURN":{"extra":1}}}"#,
        r#"{"HISTORY":{"LIFE_LOST_THIS_TURN":{}}}"#,
        r#"{"HISTORY":{"LIFE_LOST_THIS_TURN":{"player":null}}}"#,
        r#"{"HISTORY":{"LIFE_LOST_THIS_TURN":{"player":{"ref":"SELF"},"where":{"pred":"IS_LAND"}}}}"#,
        r#"{"HISTORY":{"LIFE_GAINED_THIS_TURN":{}}}"#,
        r#"{"HISTORY":{"CARDS_DRAWN_THIS_TURN":{}}}"#,
        r#"{"HISTORY":{"CREATURES_DIED_THIS_TURN":{"where":null}}}"#,
        r#"{"HISTORY":{"CREATURES_DIED_THIS_TURN":{"player":{"ref":"SELF"}}}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{}}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"color":{"name":"Hue"}}}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":{"ref":"SELF"}}}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":"SELF","color":null}}}"#,
        r#"{"HISTORY":{"MANA_SPENT_TO_CAST":{"ref":"SELF","color":"Hue"}}}"#,
    ];
    for d in docs {
        reject::<ValueExpressionAST>(d);
    }
}

// ---------------------------------------------------------------- integer-only (REQ-CORE-006)

const GOOD_I64: [&str; 5] = ["0", "1", "-1", "9007199254740991", "-9007199254740991"];

/// Spellings that are never an accepted integer in any numeric position.
const BAD_NUMBERS: [&str; 15] = [
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

/// Every numeric position of the expression grammar, as a (type, template) pair: `@N@` in the
/// template is replaced with the number spelling.
fn numeric_positions() -> Vec<(&'static str, String)> {
    vec![
        ("value", r#"{"LITERAL":@N@}"#.to_string()),
        (
            "pred",
            r#"{"pred":"HAS_COUNTER","kind":"charge","cmp":"GE","value":{"LITERAL":@N@}}"#
                .to_string(),
        ),
        (
            "value",
            r#"{"MATH":{"op":"ADD","lhs":{"LITERAL":@N@},"rhs":"X"}}"#.to_string(),
        ),
        (
            "value",
            r#"{"MATH":{"op":"SUB","lhs":"X","rhs":{"LITERAL":@N@}}}"#.to_string(),
        ),
        (
            "pred",
            r#"{"pred":"COMPARE","cmp":"EQ","lhs":{"LITERAL":@N@},"rhs":"X"}"#.to_string(),
        ),
        (
            "pred",
            r#"{"pred":"COMPARE","cmp":"EQ","lhs":"X","rhs":{"LITERAL":@N@}}"#.to_string(),
        ),
        (
            "pred",
            r#"{"pred":"POWER","cmp":"GT","value":{"LITERAL":@N@}}"#.to_string(),
        ),
        (
            "selector",
            r#"{"zones":["HAND"],"count":{"LITERAL":@N@}}"#.to_string(),
        ),
    ]
}

fn accept_kind(kind: &str, s: &str) {
    match kind {
        "value" => {
            accept::<ValueExpressionAST>(s);
        }
        "pred" => {
            accept::<PredicateAST>(s);
        }
        "selector" => {
            accept::<ObjectSelectorAST>(s);
        }
        _ => {
            accept::<ReferenceAST>(s);
        }
    }
}

fn reject_kind(kind: &str, s: &str) {
    match kind {
        "value" => reject::<ValueExpressionAST>(s),
        "pred" => reject::<PredicateAST>(s),
        "selector" => reject::<ObjectSelectorAST>(s),
        _ => reject::<ReferenceAST>(s),
    }
}

/// REQ-DSL-003, REQ-CORE-006: in every integer position (LITERAL, HAS_COUNTER value, MATH
/// operands, COMPARE sides, numeric predicates, selector count) the safe-range integers are
/// accepted and every float, exponent, out-of-range or non-number spelling is rejected.
#[test]
fn req_core_006_integer_positions() {
    for (kind, tpl) in numeric_positions() {
        for n in GOOD_I64 {
            accept_kind(kind, &tpl.replace("@N@", n));
        }
        for n in BAD_NUMBERS {
            reject_kind(kind, &tpl.replace("@N@", n));
        }
    }
}

/// REQ-DSL-003, REQ-CORE-006: a value position never accepts a bare number (the only number
/// spelling is `{"LITERAL":n}`).
#[test]
fn req_core_006_bare_numbers_are_not_values() {
    for n in ["0", "1", "-1", "1.5"] {
        reject::<ValueExpressionAST>(n);
        reject::<PredicateAST>(&format!(
            "{{\"pred\":\"POWER\",\"cmp\":\"EQ\",\"value\":{n}}}"
        ));
        reject::<ObjectSelectorAST>(&format!("{{\"zones\":[\"HAND\"],\"count\":{n}}}"));
    }
}

/// REQ-DSL-003, REQ-CORE-006: `slot` is an integer in 0..=4294967295.
#[test]
fn req_core_006_slot_range() {
    for n in ["0", "1", "4294967295"] {
        accept::<ReferenceAST>(&format!("{{\"ref\":\"TARGET\",\"slot\":{n}}}"));
        accept::<ValueExpressionAST>(&format!(
            "{{\"ATTR\":{{\"ref\":\"TARGET\",\"slot\":{n},\"attr\":\"POWER\"}}}}"
        ));
    }
    let mut bad: Vec<&str> = BAD_NUMBERS.to_vec();
    bad.extend(["-1", "4294967296", "9007199254740991", "-9007199254740991"]);
    for n in bad {
        reject_kind("ref", &format!("{{\"ref\":\"TARGET\",\"slot\":{n}}}"));
        reject::<ValueExpressionAST>(&format!(
            "{{\"ATTR\":{{\"ref\":\"TARGET\",\"slot\":{n},\"attr\":\"POWER\"}}}}"
        ));
        reject::<PredicateAST>(&format!(
            "{{\"pred\":\"SAME_AS\",\"ref\":\"TARGET\",\"slot\":{n}}}"
        ));
    }
}

/// REQ-DSL-003, REQ-CORE-006: a sweep over random in-range i64 values (fixed seed): every one is
/// accepted and round-trips exactly. No f32/f64 representation can be produced: the serialised
/// text of every accepted LITERAL is exactly the decimal integer (no `.`, `e` or `E`).
#[test]
fn req_core_006_literal_sweep_integer_text_only() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0003);
    for _ in 0..3000 {
        let n = gen_int(&mut rng);
        let s = format!("{{\"LITERAL\":{n}}}");
        let v = accept::<ValueExpressionAST>(&s);
        let out = serde_json::to_string(&v).unwrap();
        let num = out
            .strip_prefix("{\"LITERAL\":")
            .and_then(|r| r.strip_suffix('}'))
            .unwrap_or_else(|| panic!("unexpected LITERAL spelling {out}"));
        assert!(
            !num.contains('.') && !num.contains('e') && !num.contains('E'),
            "{num}"
        );
        assert_eq!(num, n.to_string());
        let canon = to_canonical_vec(&v).unwrap();
        assert_eq!(canon, s.as_bytes());
    }
}

// ---------------------------------------------------------------- property tests

/// REQ-DSL-003, REQ-NFR-006: random valid references and player selectors (seed
/// 0x0F11_0000_0000_0010) round-trip and have one canonical spelling.
#[test]
fn req_dsl_003_property_references_and_players() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0010);
    for _ in 0..600 {
        let r = Value::Object(gen_ref_obj(&mut rng));
        check_property::<ReferenceAST>(&r, &mut rng);
        let p = gen_player(&mut rng);
        check_property::<PlayerSelectorAST>(&p, &mut rng);
    }
}

/// REQ-DSL-003, REQ-NFR-006: random predicate trees with AND/OR/NOT to depth 6 (seed
/// 0x0F11_0000_0000_0011).
#[test]
fn req_dsl_003_property_predicates() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0011);
    for _ in 0..600 {
        let depth = below(&mut rng, 7) as u32;
        let p = Value::Object(gen_pred(&mut rng, depth));
        check_property::<PredicateAST>(&p, &mut rng);
    }
}

/// REQ-DSL-003, REQ-NFR-006, REQ-CORE-006: random value expressions with MATH, COUNT, DISTINCT
/// and HISTORY nesting (seed 0x0F11_0000_0000_0012).
#[test]
fn req_dsl_003_property_values() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0012);
    for _ in 0..600 {
        let depth = below(&mut rng, 6) as u32;
        let v = gen_value(&mut rng, depth);
        check_property::<ValueExpressionAST>(&v, &mut rng);
    }
}

/// REQ-DSL-003, REQ-NFR-006: random object selectors (seed 0x0F11_0000_0000_0013).
#[test]
fn req_dsl_003_property_selectors() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0013);
    for _ in 0..600 {
        let depth = below(&mut rng, 5) as u32;
        let s = Value::Object(gen_selector(&mut rng, depth));
        check_property::<ObjectSelectorAST>(&s, &mut rng);
    }
}

/// REQ-DSL-003, REQ-NFR-006: mixed families at the deepest budget (seed 0x0F11_0000_0000_0014):
/// a value-expression tree is wrapped in a COMPARE predicate inside a COUNT selector.
#[test]
fn req_dsl_003_property_mixed() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0014);
    for _ in 0..500 {
        let lhs = gen_value(&mut rng, 4);
        let pred = Value::Object(gen_pred(&mut rng, 4));
        let mut sel = Obj::new();
        sel.insert("pred".into(), json!("COMPARE"));
        sel.insert("cmp".into(), json!(pick(&mut rng, &CMPS)));
        sel.insert("lhs".into(), lhs);
        sel.insert(
            "rhs".into(),
            json!({"COUNT": {"zones": ["BATTLEFIELD"], "op": "NOT", "condition": pred}}),
        );
        let sel = Value::Object(sel);
        check_property::<ObjectSelectorAST>(&sel, &mut rng);
        let v = json!({"COUNT": sel});
        check_property::<ValueExpressionAST>(&v, &mut rng);
    }
}

/// REQ-NFR-006: the generators are deterministic: the same seed yields the same documents.
#[test]
fn req_nfr_006_generators_deterministic() {
    let mut a = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0015);
    let mut b = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0015);
    for _ in 0..50 {
        assert_eq!(gen_value(&mut a, 4), gen_value(&mut b, 4));
        assert_eq!(gen_pred(&mut a, 4), gen_pred(&mut b, 4));
    }
}

// ---------------------------------------------------------------- untrusted input (R12)

/// Decodes `bytes` as `T`; when that succeeds the value re-encodes and re-decodes equal.
fn roundtrip_if_ok<T>(bytes: &[u8]) -> bool
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    match serde_json::from_slice::<T>(bytes) {
        Ok(t) => {
            let enc = serde_json::to_vec(&t).unwrap();
            let t2: T = serde_json::from_slice(&enc).unwrap();
            assert_eq!(t2, t, "re-decode of {}", String::from_utf8_lossy(bytes));
            true
        }
        Err(_) => false,
    }
}

fn try_all_types(bytes: &[u8]) -> usize {
    [
        roundtrip_if_ok::<ValueExpressionAST>(bytes),
        roundtrip_if_ok::<PredicateAST>(bytes),
        roundtrip_if_ok::<ObjectSelectorAST>(bytes),
        roundtrip_if_ok::<PlayerSelectorAST>(bytes),
        roundtrip_if_ok::<ReferenceAST>(bytes),
        roundtrip_if_ok::<TextTokenStream>(bytes),
        roundtrip_if_ok::<ChoiceSpec>(bytes),
    ]
    .iter()
    .filter(|b| **b)
    .count()
}

/// REQ-NFR-006 (R12): 3000 arbitrary byte strings (seed 0x0F11_0000_0000_0020), fed to every
/// FP-011 type, never panic; any accepted input round-trips.
#[test]
fn req_nfr_006_random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eEX $_trufalsn\\ LITERAL ref SELF pred zones";
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0020);
    for _ in 0..3000 {
        let len = below(&mut rng, 64) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                if below(&mut rng, 4) == 0 {
                    rng.next_u64() as u8
                } else {
                    ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize]
                }
            })
            .collect();
        let _ = try_all_types(&bytes);
    }
}

/// Applies exactly one random edit: flip one bit of a byte, insert one byte, or delete one.
fn mutate_once(rng: &mut Pcg64Dxsm, bytes: &mut Vec<u8>) {
    const INSERTS: &[u8] = b"{}[]\",:9e.-\\ X$0";
    match below(rng, 3) {
        0 if !bytes.is_empty() => {
            let i = below(rng, bytes.len() as u64) as usize;
            bytes[i] ^= 1 << below(rng, 8);
        }
        1 | 0 => {
            let i = below(rng, bytes.len() as u64 + 1) as usize;
            let b = if below(rng, 2) == 0 {
                INSERTS[below(rng, INSERTS.len() as u64) as usize]
            } else {
                rng.next_u64() as u8
            };
            bytes.insert(i, b);
        }
        _ if !bytes.is_empty() => {
            let i = below(rng, bytes.len() as u64) as usize;
            bytes.remove(i);
        }
        _ => {}
    }
}

/// REQ-NFR-006 (R12): 2000 one-byte mutations of valid documents of each family (seed
/// 0x0F11_0000_0000_0021) never panic; any accepted mutant round-trips.
#[test]
fn req_nfr_006_mutated_documents_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F11_0000_0000_0021);
    let mut accepted = 0usize;
    for i in 0..10_000u32 {
        let doc = match i % 5 {
            0 => gen_value(&mut rng, 3),
            1 => Value::Object(gen_pred(&mut rng, 3)),
            2 => Value::Object(gen_selector(&mut rng, 3)),
            3 => gen_player(&mut rng),
            _ => Value::Object(gen_ref_obj(&mut rng)),
        };
        let mut bytes = serde_json::to_vec(&doc).unwrap();
        mutate_once(&mut rng, &mut bytes);
        accepted += try_all_types(&bytes);
    }
    let _ = accepted;
}

// ---------------------------------------------------------------- depth (G6)

fn nested_and(levels: usize) -> String {
    let mut s = String::new();
    for _ in 0..levels {
        s.push_str(r#"{"op":"AND","conditions":["#);
    }
    s.push_str(r#"{"pred":"IS_CREATURE"}"#);
    for _ in 0..levels {
        s.push_str("]}");
    }
    s
}

fn nested_not(levels: usize) -> String {
    let mut s = String::new();
    for _ in 0..levels {
        s.push_str(r#"{"op":"NOT","condition":"#);
    }
    s.push_str(r#"{"pred":"IS_CREATURE"}"#);
    for _ in 0..levels {
        s.push('}');
    }
    s
}

fn nested_math(levels: usize) -> String {
    let mut s = String::new();
    for _ in 0..levels {
        s.push_str(r#"{"MATH":{"op":"ADD","lhs":"#);
    }
    s.push_str(r#"{"LITERAL":1}"#);
    for _ in 0..levels {
        s.push_str(r#","rhs":{"LITERAL":2}}}"#);
    }
    s
}

fn nested_count_selector(levels: usize) -> String {
    let mut s = String::new();
    for _ in 0..levels {
        s.push_str(r#"{"zones":["HAND"],"count":{"COUNT":"#);
    }
    s.push_str(r#"{"zones":["HAND"]}"#);
    for _ in 0..levels {
        s.push_str("}}");
    }
    s
}

/// Maximum number of simultaneously open containers in `s` (string contents ignored: the
/// builders above put no brackets inside strings).
fn container_depth(s: &str) -> usize {
    let (mut d, mut max) = (0usize, 0usize);
    for c in s.bytes() {
        match c {
            b'{' | b'[' => {
                d += 1;
                max = max.max(d);
            }
            b'}' | b']' => d -= 1,
            _ => {}
        }
    }
    max
}

/// REQ-DSL-003, REQ-NFR-006: ten nesting levels of AND, NOT, MATH and COUNT-in-selector are
/// accepted and round-trip exactly.
#[test]
fn req_nfr_006_ten_levels_accepted() {
    accept::<PredicateAST>(&nested_and(10));
    accept::<PredicateAST>(&nested_not(10));
    accept::<ValueExpressionAST>(&nested_math(10));
    accept::<ObjectSelectorAST>(&nested_count_selector(10));
}

/// Builds a document nested the given number of levels.
type Builder = fn(usize) -> String;

/// Smallest nesting level of `build` whose document opens more than 127 containers.
fn first_level_over_127(build: Builder) -> usize {
    (1..200)
        .find(|n| container_depth(&build(*n)) > 127)
        .unwrap()
}

/// REQ-NFR-006: documents deeper than 127 open containers are rejected without panicking or
/// overflowing the stack, at the first level past the limit, a little past it and at 100000
/// levels (also unterminated).
#[test]
fn req_nfr_006_too_deep_is_error() {
    let builders: [(Builder, &str); 4] = [
        (nested_and, "pred"),
        (nested_not, "pred"),
        (nested_math, "value"),
        (nested_count_selector, "selector"),
    ];
    for (build, kind) in builders {
        let first = first_level_over_127(build);
        for levels in [first, first + 1, first + 50, 100_000] {
            let s = build(levels);
            assert!(container_depth(&s) > 127);
            reject_text(kind, &s);
            // Unterminated: only the opening half.
            let open = &s[..s.len() / 2];
            reject_text(kind, open);
        }
    }
}

/// Rejects text without building a `serde_json::Value` (the Value path would itself recurse).
fn reject_text(kind: &str, s: &str) {
    let failed = match kind {
        "value" => serde_json::from_str::<ValueExpressionAST>(s).is_err(),
        "pred" => serde_json::from_str::<PredicateAST>(s).is_err(),
        _ => serde_json::from_str::<ObjectSelectorAST>(s).is_err(),
    };
    assert!(failed, "deep document of {} bytes was accepted", s.len());
}

/// Container depth of a `Value` (recursive; only used at <= 1000 levels).
fn value_depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(value_depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(value_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn value_not(levels: usize) -> Value {
    let mut v = json!({"pred": "TAPPED"});
    for _ in 0..levels {
        v = json!({"op": "NOT", "condition": v});
    }
    v
}

fn value_math(levels: usize) -> Value {
    let mut v = json!({"LITERAL": 1});
    for _ in 0..levels {
        v = json!({"MATH": {"op": "ADD", "lhs": v, "rhs": {"LITERAL": 1}}});
    }
    v
}

fn value_count(levels: usize) -> Value {
    let mut v = json!({"zones": ["HAND"]});
    for _ in 0..levels {
        v = json!({"zones": ["HAND"], "count": {"COUNT": v}});
    }
    v
}

/// Largest level count with depth <= 127, and smallest level counts reaching depth >= 128,
/// >= 200 and >= 1000.
fn depth_levels(build: fn(usize) -> Value) -> (usize, [usize; 3]) {
    // Depth grows by a constant step per level; measure it with the helper.
    let base = value_depth(&build(0));
    let step = value_depth(&build(1)) - base;
    assert!(step > 0);
    assert_eq!(value_depth(&build(7)), base + 7 * step);
    let first_at = |target: usize| target.saturating_sub(base).div_ceil(step);
    let over = first_at(128);
    (over - 1, [over, first_at(200), first_at(1000)])
}

fn check_borrowed_guard<T>(name: &str, build: fn(usize) -> Value)
where
    T: for<'de> serde::Deserialize<'de> + Serialize + PartialEq + Debug,
{
    let (ok_levels, over) = depth_levels(build);
    let ok_value = build(ok_levels);
    let ok_depth = value_depth(&ok_value);
    assert!((126..=127).contains(&ok_depth), "{name}: depth {ok_depth}");
    let t = T::deserialize(&ok_value)
        .unwrap_or_else(|e| panic!("{name}: depth {ok_depth} rejected: {e}"));
    let bytes = to_canonical_vec(&t).unwrap();
    let back: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back, t, "{name}: canonical round trip");
    let again: T = T::deserialize(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap();
    assert_eq!(again, t);
    drop(ok_value);

    for levels in [over[0], over[0] + 1, over[1], over[2]] {
        let v = build(levels);
        let d = value_depth(&v);
        assert!(
            (128..=1002).contains(&d),
            "{name}: levels {levels} depth {d}"
        );
        assert!(
            T::deserialize(&v).is_err(),
            "{name}: levels {levels} (depth {d}) was accepted"
        );
    }
}

/// REQ-NFR-006, REQ-DSL-003: the DSL depth guard (G6), exercised through a borrowed `Value`
/// (the only path that discriminates: `serde_json::from_str` stops at the same limit first).
/// Documents of at most 127 open containers are accepted and round-trip canonically; 128 or
/// more is an error, for NOT predicates, MATH values and COUNT-in-selector selectors.
#[test]
fn req_nfr_006_borrowed_value_depth_guard() {
    check_borrowed_guard::<PredicateAST>("not", value_not);
    check_borrowed_guard::<ValueExpressionAST>("math", value_math);
    check_borrowed_guard::<ObjectSelectorAST>("count", value_count);
}
