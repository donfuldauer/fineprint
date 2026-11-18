// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::effect` (FP-013 wire contract), part 3: control flow and
//! nesting (REQ-DSL-010; CREATE_DELAYED_TRIGGER is 603.7, REFLEXIVE_TRIGGER is 603.12), the
//! exactly-one and union rules, and strict decoding through `from_str`, `from_slice` and
//! `from_value` (REQ-NFR-006, REQ-CORE-006). Black-box: written from the work-item brief only.
//! Synthetic data only (R5).

use fineprint_dsl::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};
use fineprint_dsl::effect::{DesignationValue, Instruction, TokenSource};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------- helpers

fn accept(doc: &Value) -> Instruction {
    let text = serde_json::to_string(doc).unwrap();
    let i: Instruction = match serde_json::from_str(&text) {
        Ok(i) => i,
        Err(e) => panic!("expected {text} to decode, got error: {e}"),
    };
    assert_eq!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).unwrap(),
        i
    );
    assert_eq!(
        serde_json::from_value::<Instruction>(doc.clone()).unwrap(),
        i
    );
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&i).unwrap()).unwrap(),
        String::from_utf8(canon).unwrap(),
        "canonical bytes of {text}"
    );
    let enc = serde_json::to_string(&i).unwrap();
    assert_eq!(serde_json::from_str::<Instruction>(&enc).unwrap(), i);
    i
}

fn accept_text(text: &str) -> Instruction {
    let doc: Value = serde_json::from_str(text).unwrap();
    accept(&doc)
}

/// Rejected by `from_str`, `from_slice` and `from_value`.
fn reject(doc: &Value) {
    let text = serde_json::to_string(doc).unwrap();
    let r = serde_json::from_str::<Instruction>(&text);
    assert!(r.is_err(), "expected rejection of {text}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).is_err(),
        "from_slice {text}"
    );
    assert!(
        serde_json::from_value::<Instruction>(doc.clone()).is_err(),
        "from_value {text}"
    );
}

/// Rejected by `from_str` and `from_slice` (text that a `Value` cannot represent faithfully).
fn reject_text(text: &str) {
    let r = serde_json::from_str::<Instruction>(text);
    assert!(r.is_err(), "expected rejection of {text}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).is_err(),
        "from_slice {text}"
    );
}

/// Rejected by all three entry points when the text parses as a plain `Value`; by the two text
/// entry points otherwise.
fn reject_any(text: &str) {
    reject_text(text);
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        assert!(
            serde_json::from_value::<Instruction>(v).is_err(),
            "from_value {text}"
        );
    }
}

fn with(base: &Value, key: &str, v: Value) -> Value {
    let mut m = base.as_object().unwrap().clone();
    m.insert(key.into(), v);
    Value::Object(m)
}

fn without(base: &Value, key: &str) -> Value {
    let mut m = base.as_object().unwrap().clone();
    m.remove(key);
    Value::Object(m)
}

fn leaf() -> Value {
    json!({"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}})
}

fn instrs() -> Value {
    json!([leaf()])
}

// ---------------------------------------------------------------- 6. control flow

/// REQ-DSL-010 (requirement 6): IF with and without `else`; empty `then`/`else` rejected; the
/// Rust `otherwise` field holds `else`.
#[test]
fn req_dsl_010_if_rules() {
    let base = json!({"op":"IF","cond":{"pred":"IS_CREATURE"},"then":instrs()});
    let no_else = accept(&base);
    let with_else = accept(&with(&base, "else", json!([{"op":"POPULATE"}])));
    assert_ne!(no_else, with_else);
    let Instruction::If {
        otherwise, then, ..
    } = &with_else
    else {
        panic!("expected If, got {with_else:?}");
    };
    assert_eq!(otherwise.as_deref(), Some(&[Instruction::Populate][..]));
    assert_eq!(then.len(), 1);
    let Instruction::If { otherwise, .. } = &no_else else {
        panic!("expected If");
    };
    assert_eq!(*otherwise, None);
    reject(&with(&base, "then", json!([])));
    reject(&with(&base, "else", json!([])));
    reject(&with(&base, "else", Value::Null));
    reject(&with(&base, "otherwise", instrs()));
    reject(&without(&base, "cond"));
    reject(&with(&base, "cond", json!({"pred":"NOT_A_PREDICATE"})));
    // Multi-instruction lists keep their order.
    let two = accept(&with(
        &base,
        "then",
        json!([{"op":"POPULATE"},{"op":"VENTURE"}]),
    ));
    let rev = accept(&with(
        &base,
        "then",
        json!([{"op":"VENTURE"},{"op":"POPULATE"}]),
    ));
    assert_ne!(two, rev);
}

/// REQ-DSL-010 (requirement 6, brief A3): FOR_EACH `bind` must be a VAR (`$` plus one or more of
/// `A-Za-z0-9_`); iterator accepts an object selector and a player group.
#[test]
fn req_dsl_010_for_each_bind_is_var() {
    let base = json!({"op":"FOR_EACH","iterator":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},
                      "body":instrs()});
    accept(&base);
    for good in ["$x", "$X", "$_", "$a1", "$A_b_9", "$0"] {
        accept(&with(&base, "bind", json!(good)));
    }
    for bad in [
        json!("x"),
        json!("$"),
        json!("$a-b"),
        json!(""),
        json!("$a b"),
        json!("$$a"),
        json!("a$b"),
        json!("$\u{e9}"),
        json!(7),
        Value::Null,
    ] {
        reject(&with(&base, "bind", bad));
    }
    accept(&with(&base, "iterator", json!({"players":"ALL_PLAYERS"})));
    accept(&with(&base, "iterator", json!({"ref":"SELF"})));
    reject(&with(&base, "body", json!([])));
    reject(&with(&base, "iterator", json!({"players":"NOBODY"})));
}

/// REQ-DSL-010 (requirement 6): REPEAT and REPEAT_WHILE bodies are non-empty INSTRS.
#[test]
fn req_dsl_010_repeat_rules() {
    let rep = json!({"op":"REPEAT","times":{"LITERAL":2},"body":instrs()});
    accept(&rep);
    accept(&with(&rep, "times", json!("X")));
    reject(&with(&rep, "body", json!([])));
    reject(&without(&rep, "times"));
    reject(&with(&rep, "maxIter", json!(3)));
    let rw = json!({"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"maxIter":2,"body":instrs()});
    accept(&rw);
    reject(&with(&rw, "body", json!([])));
    reject(&with(&rw, "times", json!({"LITERAL":2})));
}

/// REQ-DSL-010 (requirement 6): MAY and UNLESS bodies are non-empty; UNLESS cost is non-empty
/// COSTS.
#[test]
fn req_dsl_010_may_and_unless() {
    let may = json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":instrs()});
    accept(&may);
    reject(&with(&may, "body", json!([])));
    reject(&without(&may, "player"));
    let unless = json!({"op":"UNLESS","payer":{"ref":"EACH_OPPONENT"},
                        "cost":[{"cost":"PAY_LIFE","amount":{"LITERAL":1}}],"body":instrs()});
    accept(&unless);
    accept(&with(
        &unless,
        "cost",
        json!([{"cost":"TAP_SOURCE"},{"cost":"PAY_MANA","mana":"{2}"}]),
    ));
    reject(&with(&unless, "cost", json!([])));
    reject(&with(&unless, "cost", json!([{"cost":"NOT_A_COST"}])));
    reject(&with(&unless, "cost", json!({"cost":"TAP_SOURCE"})));
    reject(&with(&unless, "body", json!([])));
}

fn choose(min: u64, max: u64, modes: Value) -> Value {
    json!({"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":min,"max":max,"modes":modes})
}

fn mode(id: &str) -> Value {
    json!({"id":id,"effect":instrs()})
}

/// REQ-DSL-010 (requirement 6, brief C 69): CHOOSE_MODE ids distinct, min <= max <= mode
/// count, modes non-empty, mode effect non-empty, mode targets non-empty with distinct slots.
#[test]
fn req_dsl_010_choose_mode_rules() {
    accept(&choose(1, 1, json!([mode("m1")])));
    accept(&choose(0, 0, json!([mode("m1")])));
    accept(&choose(1, 3, json!([mode("m1"), mode("m2"), mode("m3")])));
    accept(&choose(2, 2, json!([mode("m1"), mode("m2")])));
    let with_targets = json!({"id":"m1","targets":[
        {"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1},
        {"slot":1,"selector":{"pred":"IS_CREATURE"},"min":0,"max":2}],"effect":instrs()});
    accept(&choose(1, 1, json!([with_targets])));

    reject(&choose(1, 1, json!([])));
    reject(&choose(1, 1, json!([mode("m1"), mode("m1")])));
    reject(&choose(2, 1, json!([mode("m1"), mode("m2")])));
    reject(&choose(1, 2, json!([mode("m1")])));
    reject(&choose(0, 4, json!([mode("m1"), mode("m2"), mode("m3")])));
    reject(&choose(1, 1, json!([mode("")])));
    reject(&choose(1, 1, json!([{"id":"m1","effect":[]}])));
    reject(&choose(1, 1, json!([{"id":"m1"}])));
    reject(&choose(1, 1, json!([{"effect":instrs()}])));
    reject(&choose(1, 1, json!([{"id":"m1","effect":instrs(),"zz":1}])));
    reject(&choose(
        1,
        1,
        json!([{"id":"m1","targets":[],"effect":instrs()}]),
    ));
    reject(&choose(
        1,
        1,
        json!([{"id":"m1","targets":null,"effect":instrs()}]),
    ));
    reject(&choose(
        1,
        1,
        json!([{"id":"m1","targets":[
            {"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1},
            {"slot":0,"selector":{"pred":"IS_CREATURE"},"min":1,"max":1}],"effect":instrs()}]),
    ));
    let base = choose(1, 1, json!([mode("m1")]));
    for bad in [json!(-1), json!(4_294_967_296_u64), json!("1"), Value::Null] {
        reject(&with(&base, "min", bad.clone()));
        reject(&with(&base, "max", bad));
    }
    // Mode order is kept.
    let ab = accept(&choose(1, 1, json!([mode("a"), mode("b")])));
    let ba = accept(&choose(1, 1, json!([mode("b"), mode("a")])));
    assert_ne!(ab, ba);
}

/// REQ-DSL-010 (requirement 6, brief C 70): VOTE with voters, choice and store; `store` is STR.
#[test]
fn req_dsl_010_vote() {
    let base = json!({"op":"VOTE","voters":{"players":"ALL_PLAYERS"},
                      "choice":{"chooser":{"ref":"CONTROLLER"},"kind":"MODE"},"store":"k1"});
    accept(&base);
    for m in ["voters", "choice", "store"] {
        reject(&without(&base, m));
    }
    reject(&with(&base, "store", json!("")));
    reject(&with(&base, "choice", json!({"kind":"MODE"})));
}

/// REQ-DSL-010 (requirement 6, brief C 71): REMEMBER needs exactly one of `value` and
/// `objects`; `into` is MEMORY or LINKED (case-sensitive); `key` is STR.
#[test]
fn req_dsl_010_remember_rules() {
    let base = json!({"op":"REMEMBER","key":"k1","into":"MEMORY"});
    let v = accept(&with(&base, "value", json!({"LITERAL":4})));
    let o = accept(&with(&base, "objects", json!({"ref":"SELF"})));
    assert_ne!(v, o);
    accept(&with(
        &with(&base, "into", json!("LINKED")),
        "value",
        json!("X"),
    ));
    let mem = accept(&with(&base, "objects", json!({"ref":"SELF"})));
    let linked = accept(&with(
        &with(&base, "into", json!("LINKED")),
        "objects",
        json!({"ref":"SELF"}),
    ));
    assert_ne!(mem, linked);
    reject(&base);
    reject(&with(
        &with(&base, "value", json!({"LITERAL":4})),
        "objects",
        json!({"ref":"SELF"}),
    ));
    reject(&with(&base, "value", Value::Null));
    reject(&with(&base, "objects", Value::Null));
    reject(&with(
        &with(&base, "value", json!({"LITERAL":4})),
        "objects",
        Value::Null,
    ));
    let full = with(&base, "value", json!({"LITERAL":4}));
    for bad in ["memory", "linked", "Memory", "STORE", ""] {
        reject(&with(&full, "into", json!(bad)));
    }
    reject(&without(&full, "into"));
    reject(&without(&full, "key"));
    reject(&with(&full, "key", json!("")));
}

/// REQ-DSL-010 (requirement 6, brief C 61; 603.7): CREATE_DELAYED_TRIGGER captures keys are
/// VARs and non-empty when present; `oneShot` is required (true and false distinct); effect
/// non-empty.
#[test]
fn req_dsl_010_create_delayed_trigger_rules() {
    let base = json!({"op":"CREATE_DELAYED_TRIGGER",
        "trigger":{"mode":"PHASE_STEP","step":"END","whose":{"ref":"CONTROLLER"}},
        "effect":instrs(),"oneShot":true});
    let t = accept(&base);
    let f = accept(&with(&base, "oneShot", json!(false)));
    assert_ne!(t, f);
    let c = accept(&with(
        &base,
        "captures",
        json!({"$x":{"ref":"SELF"},"$Y_2":{"ref":"TARGET","slot":0}}),
    ));
    assert_ne!(c, t);
    reject(&without(&base, "oneShot"));
    reject(&with(&base, "oneShot", Value::Null));
    reject(&with(&base, "effect", json!([])));
    reject(&without(&base, "effect"));
    reject(&without(&base, "trigger"));
    for bad in [
        json!({}),
        json!({"x":{"ref":"SELF"}}),
        json!({"$":{"ref":"SELF"}}),
        json!({"$a-b":{"ref":"SELF"}}),
        json!({"":{"ref":"SELF"}}),
        json!({"$x":null}),
        json!({"$x":7}),
        json!([{"ref":"SELF"}]),
        Value::Null,
    ] {
        reject(&with(&base, "captures", bad));
    }
}

/// REQ-DSL-010 (requirement 6, brief C 62; 603.12): REFLEXIVE_TRIGGER needs both lists, each
/// non-empty.
#[test]
fn req_dsl_010_reflexive_trigger_rules() {
    let base = json!({"op":"REFLEXIVE_TRIGGER","ifActionTaken":instrs(),
                      "effect":[{"op":"GAIN_LIFE","player":{"ref":"CONTROLLER"},"amount":{"LITERAL":2}}]});
    accept(&base);
    reject(&with(&base, "ifActionTaken", json!([])));
    reject(&with(&base, "effect", json!([])));
    reject(&without(&base, "ifActionTaken"));
    reject(&without(&base, "effect"));
    reject(&with(&base, "if_action_taken", instrs()));
}

/// Every INSTRS position as `(document, member, path-to-parent)`: the member at the end of the
/// path holds a non-empty list in the valid document.
fn instr_positions() -> Vec<(Value, Vec<&'static str>)> {
    vec![
        (
            json!({"op":"IF","cond":{"pred":"TAPPED"},"then":instrs()}),
            vec!["then"],
        ),
        (
            json!({"op":"IF","cond":{"pred":"TAPPED"},"then":instrs(),"else":instrs()}),
            vec!["else"],
        ),
        (
            json!({"op":"FOR_EACH","iterator":{"ref":"SELF"},"body":instrs()}),
            vec!["body"],
        ),
        (
            json!({"op":"REPEAT","times":{"LITERAL":2},"body":instrs()}),
            vec!["body"],
        ),
        (
            json!({"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"maxIter":2,"body":instrs()}),
            vec!["body"],
        ),
        (
            json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":instrs()}),
            vec!["body"],
        ),
        (
            json!({"op":"UNLESS","payer":{"ref":"CONTROLLER"},"cost":[{"cost":"TAP_SOURCE"}],
                   "body":instrs()}),
            vec!["body"],
        ),
        (
            json!({"op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},
                   "effect":instrs(),"oneShot":false}),
            vec!["effect"],
        ),
        (
            json!({"op":"REFLEXIVE_TRIGGER","ifActionTaken":instrs(),"effect":instrs()}),
            vec!["ifActionTaken"],
        ),
        (
            json!({"op":"REFLEXIVE_TRIGGER","ifActionTaken":instrs(),"effect":instrs()}),
            vec!["effect"],
        ),
    ]
}

/// REQ-DSL-010 (requirement 6, brief A4): an empty INSTRS is rejected at every position — at top
/// level and when the empty list sits deep inside a valid tree.
#[test]
fn req_dsl_010_empty_instrs_rejected_everywhere() {
    for (doc, path) in instr_positions() {
        accept(&doc);
        let member = path[0];
        let empty = with(&doc, member, json!([]));
        reject(&empty);
        // The same empty list nested two levels down inside MAY > IF.
        let nested = json!({"op":"MAY","player":{"ref":"CONTROLLER"},
            "body":[{"op":"IF","cond":{"pred":"TAPPED"},"then":[doc.clone()]}]});
        accept(&nested);
        let nested_bad = json!({"op":"MAY","player":{"ref":"CONTROLLER"},
            "body":[{"op":"IF","cond":{"pred":"TAPPED"},"then":[empty]}]});
        reject(&nested_bad);
    }
    // Mode effect.
    let ok = choose(1, 1, json!([mode("m1")]));
    accept(&ok);
    reject(&choose(1, 1, json!([{"id":"m1","effect":[]}])));
    let nested_bad = json!({"op":"REPEAT","times":{"LITERAL":1},
        "body":[choose(1, 1, json!([{"id":"m1","effect":[]}]))]});
    reject(&nested_bad);
}

/// One wrapper level around `inner`, cycling through every nesting op.
fn wrap(level: usize, inner: Value) -> Value {
    let list = json!([inner]);
    match level % 10 {
        0 => json!({"op":"IF","cond":{"pred":"TAPPED"},"then":list}),
        1 => json!({"op":"FOR_EACH","iterator":{"ref":"SELF"},"bind":"$x","body":list}),
        2 => json!({"op":"REPEAT","times":{"LITERAL":2},"body":list}),
        3 => json!({"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"maxIter":5,"body":list}),
        4 => json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":list}),
        5 => json!({"op":"UNLESS","payer":{"ref":"CONTROLLER"},"cost":[{"cost":"TAP_SOURCE"}],
                    "body":list}),
        6 => {
            json!({"op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},
                    "effect":list,"oneShot":true})
        }
        7 => json!({"op":"REFLEXIVE_TRIGGER","ifActionTaken":[{"op":"POPULATE"}],"effect":list}),
        8 => json!({"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":1,"max":1,
                    "modes":[{"id":"m","effect":list}]}),
        _ => json!({"op":"IF","cond":{"pred":"TAPPED"},"then":[{"op":"VENTURE"}],"else":list}),
    }
}

/// REQ-DSL-010 (requirement 6): trees nested 20 and 25 levels deep through every nesting op
/// decode and round-trip.
#[test]
fn req_dsl_010_deep_trees_round_trip() {
    for levels in [20_usize, 25] {
        let mut v = leaf();
        for l in 0..levels {
            v = wrap(l, v);
        }
        let i = accept(&v);
        assert_eq!(serde_json::to_value(&i).unwrap(), v);
    }
}

// ---------------------------------------------------------------- 7. exactly-one and unions

/// REQ-DSL-010 (requirement 7, brief C 27): CREATE_TOKEN needs exactly one of `tokenDef` and
/// `inline`; inline may be `{}`; modifiers non-empty when present; opaque contents exclude null,
/// floats and unsafe integers.
#[test]
fn req_dsl_010_create_token_rules() {
    let base = json!({"op":"CREATE_TOKEN","count":{"LITERAL":1}});
    let def = accept(&with(&base, "tokenDef", json!("test.token_1_1")));
    let Instruction::CreateToken { token, .. } = &def else {
        panic!("expected CreateToken, got {def:?}");
    };
    assert_eq!(*token, TokenSource::Def("test.token_1_1".into()));
    let empty_inline = accept(&with(&base, "inline", json!({})));
    let Instruction::CreateToken { token, .. } = &empty_inline else {
        panic!("expected CreateToken");
    };
    assert_eq!(*token, TokenSource::Inline(Default::default()));
    let rich = json!({"k1":1,"k2":"v","k3":[true,{"a":[]}],"":{}});
    let inl = accept(&with(&base, "inline", rich.clone()));
    let Instruction::CreateToken { token, .. } = &inl else {
        panic!("expected CreateToken");
    };
    assert_eq!(
        *token,
        TokenSource::Inline(serde_json::from_value(rich).unwrap())
    );
    // Neither, both.
    reject(&base);
    reject(&with(
        &with(&base, "tokenDef", json!("test.token_1_1")),
        "inline",
        json!({}),
    ));
    reject(&with(&base, "tokenDef", json!("")));
    reject(&with(&base, "tokenDef", Value::Null));
    reject(&with(&base, "inline", Value::Null));
    reject(&with(&base, "inline", json!([])));
    reject(&with(&base, "inline", json!("test.token_1_1")));
    let ok = with(&base, "tokenDef", json!("test.token_1_1"));
    accept(&with(&ok, "modifiers", json!({"k1":true})));
    accept(&with(&ok, "controller", json!({"ref":"EACH_OPPONENT"})));
    reject(&with(&ok, "modifiers", json!({})));
    reject(&with(&ok, "modifiers", Value::Null));
    reject(&with(&ok, "modifiers", json!([1])));
    let opaque_bads = [
        json!({"k1":null}),
        json!({"k1":[null]}),
        json!({"k1":{"k2":null}}),
        json!({"k1":1.5}),
        json!({"k1":[0.5]}),
        json!({"k1":9_007_199_254_740_992_i64}),
        json!({"k1":{"k2":-9_007_199_254_740_992_i64}}),
    ];
    for bad in &opaque_bads {
        reject(&with(&base, "inline", bad.clone()));
        reject(&with(&ok, "modifiers", bad.clone()));
    }
    for t in [
        r#"{"op":"CREATE_TOKEN","inline":{"k1":1.0},"count":{"LITERAL":1}}"#,
        r#"{"op":"CREATE_TOKEN","inline":{"k1":1e2},"count":{"LITERAL":1}}"#,
        r#"{"op":"CREATE_TOKEN","inline":{"k1":-0},"count":{"LITERAL":1}}"#,
        r#"{"op":"CREATE_TOKEN","tokenDef":"t","modifiers":{"k1":1.0},"count":{"LITERAL":1}}"#,
    ] {
        reject_text(t);
    }
}

/// REQ-DSL-010 (requirement 7, brief C 29): CREATE_COPY_TOKEN `except` is a non-empty opaque
/// object.
#[test]
fn req_dsl_010_create_copy_token_except() {
    let base = json!({"op":"CREATE_COPY_TOKEN","of":{"ref":"TARGET","slot":0}});
    accept(&base);
    accept(&with(&base, "except", json!({"k1":{"a":1},"k2":[1,2]})));
    reject(&with(&base, "except", json!({})));
    reject(&with(&base, "except", Value::Null));
    reject(&with(&base, "except", json!([])));
    for bad in [
        json!({"k1":null}),
        json!({"k1":2.5}),
        json!({"k1":9_007_199_254_740_992_i64}),
    ] {
        reject(&with(&base, "except", bad));
    }
}

/// REQ-DSL-010 (requirement 7, brief C 46): SET_DESIGNATION value is a JSON bool (Flag) or a
/// non-empty string (Name); number, null and empty string are rejected.
#[test]
fn req_dsl_010_set_designation_value() {
    let base = json!({"op":"SET_DESIGNATION","kind":"test_designation"});
    let mut seen = Vec::new();
    for (v, expect) in [
        (json!(true), DesignationValue::Flag(true)),
        (json!(false), DesignationValue::Flag(false)),
        (
            json!("test_name"),
            DesignationValue::Name("test_name".into()),
        ),
        (json!("true"), DesignationValue::Name("true".into())),
    ] {
        let i = accept(&with(&base, "value", v));
        let Instruction::SetDesignation { value, .. } = &i else {
            panic!("expected SetDesignation, got {i:?}");
        };
        assert_eq!(*value, expect);
        assert!(!seen.contains(&i));
        seen.push(i);
    }
    for bad in [
        json!(1),
        json!(0),
        Value::Null,
        json!(""),
        json!([]),
        json!({}),
    ] {
        reject(&with(&base, "value", bad));
    }
    reject(&base);
    let ok = with(&base, "value", json!(true));
    accept(&with(&ok, "to", json!({"players":"ALL_PLAYERS"})));
    accept(&with(&ok, "to", json!({"ref":"SELF"})));
    reject(&with(&ok, "kind", json!("")));
}

/// REQ-DSL-010 (requirement 7, brief C 60): CREATE_CONTINUOUS_EFFECT effects non-empty; mixed
/// CHAR/PLAYER/RULE accepted and order kept.
#[test]
fn req_dsl_010_create_continuous_effect_effects() {
    let base = json!({"op":"CREATE_CONTINUOUS_EFFECT","duration":{"kind":"UNTIL_END_OF_TURN"}});
    let mixed = json!([
        {"type":"CHAR","layer":"6","affected":{"pred":"IS_CREATURE"},"modification":{"op":"TEST_OP"}},
        {"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"TEST_OP"}},
        {"type":"RULE","kind":"SKIP"}
    ]);
    let a = accept(&with(&base, "effects", mixed.clone()));
    let mut rev = mixed.as_array().unwrap().clone();
    rev.reverse();
    let b = accept(&with(&base, "effects", Value::Array(rev)));
    assert_ne!(a, b);
    accept(&with(
        &with(&base, "effects", mixed),
        "lockAffectedSet",
        json!(true),
    ));
    reject(&with(&base, "effects", json!([])));
    reject(&base);
    reject(&with(
        &base,
        "effects",
        json!({"type":"RULE","kind":"SKIP"}),
    ));
    reject(&with(&base, "effects", json!([{"type":"BOGUS"}])));
    reject(&with(
        &base,
        "effects",
        json!([{"type":"RULE","kind":"NOT_A_RULE_KIND"}]),
    ));
}

/// REQ-DSL-010 (requirement 7, brief A7): SUBJ positions (DEAL_DAMAGE recipients, FOR_EACH
/// iterator, PREVENT recipients, SET_DESIGNATION to) accept an object selector and
/// `{"players":...}`, and the two decode to different values.
#[test]
fn req_dsl_010_subject_positions_accept_both_forms() {
    let hosts = [
        (
            json!({"op":"DEAL_DAMAGE","source":{"ref":"SELF"},"amount":{"LITERAL":2}}),
            "recipients",
        ),
        (
            json!({"op":"FOR_EACH","body":[{"op":"POPULATE"}]}),
            "iterator",
        ),
        (
            json!({"op":"PREVENT","duration":{"kind":"UNTIL_END_OF_TURN"}}),
            "recipients",
        ),
        (
            json!({"op":"SET_DESIGNATION","kind":"test_designation","value":true}),
            "to",
        ),
    ];
    for (h, m) in hosts {
        let p = accept(&with(&h, m, json!({"players":"ALL_PLAYERS"})));
        let t = accept(&with(&h, m, json!({"players":"TEAMMATES"})));
        let o = accept(&with(
            &h,
            m,
            json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        ));
        let r = accept(&with(&h, m, json!({"ref":"TARGET","slot":1})));
        assert_ne!(p, o);
        assert_ne!(p, t);
        assert_ne!(o, r);
        reject(&with(&h, m, json!({"players":"ALL_PLAYERS","ref":"SELF"})));
        reject(&with(&h, m, json!({"players":"all_players"})));
    }
}

// ---------------------------------------------------------------- 8. strictness

/// REQ-NFR-006, REQ-CORE-006 (requirement 8, brief A2): duplicate member names, at top level and
/// nested, are rejected by `from_str` and `from_slice`.
#[test]
fn req_nfr_006_duplicate_members_rejected() {
    let cases = [
        r#"{"op":"DRAW","op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#,
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}"#,
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER","ref":"CONTROLLER"},"count":{"LITERAL":1}}"#,
        r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k1":1,"k1":1}}"#,
        r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"version":1,"params":{}}"#,
        r#"{"op":"EXILE_UNTIL","objects":{"ref":"SELF"},"duration":{"kind":"PERMANENT","kind":"PERMANENT"}}"#,
        r#"{"op":"CREATE_TOKEN","inline":{"k1":{"a":1,"a":1}},"count":{"LITERAL":1}}"#,
        r#"{"op":"IF","cond":{"pred":"TAPPED"},"then":[{"op":"POPULATE","op":"POPULATE"}]}"#,
        r#"{"op":"REMEMBER","key":"k1","into":"MEMORY","value":{"LITERAL":1},"value":{"LITERAL":1}}"#,
        r#"{"op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"effect":[{"op":"POPULATE"}],"oneShot":true,"captures":{"$x":{"ref":"SELF"},"$x":{"ref":"SELF"}}}"#,
        r#"{"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":1,"max":1,"modes":[{"id":"m","id":"m","effect":[{"op":"POPULATE"}]}]}"#,
    ];
    for t in cases {
        reject_text(t);
        // The de-duplicated document is valid.
        let v: Value = serde_json::from_str(t).unwrap();
        accept(&v);
    }
}

/// Templates with a `<<V>>` number placeholder; `1` gives a valid document.
const NUMBER_TEMPLATES: [&str; 8] = [
    r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":<<V>>}}"#,
    r#"{"op":"REPEAT_WHILE","cond":{"pred":"TAPPED"},"maxIter":<<V>>,"body":[{"op":"POPULATE"}]}"#,
    r#"{"op":"NATIVE_EFFECT","id":"native.x","version":<<V>>,"params":{}}"#,
    r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k1":<<V>>}}"#,
    r#"{"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":<<V>>,"max":1,"modes":[{"id":"m","effect":[{"op":"POPULATE"}]}]}"#,
    r#"{"op":"CREATE_TOKEN","inline":{"k1":[<<V>>]},"count":{"LITERAL":1}}"#,
    r#"{"op":"CREATE_COPY_TOKEN","of":{"ref":"SELF"},"except":{"k1":{"a":<<V>>}}}"#,
    r#"{"op":"IF","cond":{"pred":"TAPPED"},"then":[{"op":"DISCARD","player":{"ref":"CONTROLLER"},"count":{"LITERAL":<<V>>}}]}"#,
];

/// REQ-NFR-006, REQ-CORE-006 (requirement 8, brief A2): `1.0`, `1e2`, `-0` and integers beyond
/// ±(2^53-1) are rejected in every number position; the same document with `1` decodes.
#[test]
fn req_core_006_floats_and_unsafe_integers_rejected() {
    for t in NUMBER_TEMPLATES {
        accept_text(&t.replace("<<V>>", "1"));
        for bad in [
            "1.0",
            "1e2",
            "1E2",
            "-0",
            "0.5",
            "9007199254740992",
            "-9007199254740992",
            "1e400",
        ] {
            reject_any(&t.replace("<<V>>", bad));
        }
    }
}

/// Templates with a `<<S>>` string placeholder; `s` gives a valid document.
const STRING_TEMPLATES: [&str; 9] = [
    r#"{"op":"PUT_COUNTERS","objects":{"ref":"SELF"},"kind":"<<S>>","count":{"LITERAL":1}}"#,
    r#"{"op":"VOTE","voters":{"players":"ALL_PLAYERS"},"choice":{"chooser":{"ref":"CONTROLLER"},"kind":"YES_NO"},"store":"<<S>>"}"#,
    r#"{"op":"REMEMBER","key":"<<S>>","into":"LINKED","objects":{"ref":"SELF"}}"#,
    r#"{"op":"CREATE_TOKEN","tokenDef":"<<S>>","count":{"LITERAL":1}}"#,
    r#"{"op":"PLAY_PERMISSION_GRANT","objects":{"ref":"SELF"},"duration":{"kind":"PERMANENT"},"who":{"ref":"CONTROLLER"},"alsoAs":"<<S>>"}"#,
    r#"{"op":"NATIVE_EFFECT","id":"<<S>>","version":1,"params":{}}"#,
    r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k1":"<<S>>"}}"#,
    r#"{"op":"SET_DESIGNATION","kind":"test_designation","value":"<<S>>"}"#,
    r#"{"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":1,"max":1,"modes":[{"id":"<<S>>","effect":[{"op":"POPULATE"}]}]}"#,
];

/// Templates with a `<<K>>` member-name placeholder; `k` gives a valid document.
const NAME_TEMPLATES: [&str; 4] = [
    r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"<<K>>":1}}"#,
    r#"{"op":"CREATE_TOKEN","inline":{"<<K>>":true},"count":{"LITERAL":1}}"#,
    r#"{"op":"CREATE_TOKEN","tokenDef":"t","modifiers":{"<<K>>":"v"},"count":{"LITERAL":1}}"#,
    r#"{"op":"CREATE_COPY_TOKEN","of":{"ref":"SELF"},"except":{"a":{"<<K>>":1}}}"#,
];

/// REQ-NFR-006 (requirement 8, brief A2/A3/A5): noncharacters (escaped and raw) are rejected in
/// STR values, opaque strings and opaque member names; U+FFFD and U+00E9 are fine.
#[test]
fn req_nfr_006_noncharacters_rejected() {
    let okays = ["k", "x\\uFFFDy", "\u{e9}", "\u{FFFD}"];
    let bads = [
        "x\\uFFFE",
        "x\\uFFFF",
        "x\\uFDD0",
        "\\uFDEF",
        "x\u{FFFE}",
        "x\u{FFFF}",
        "x\u{FDD0}",
        "\u{FDEF}",
        "\\uD83F\\uDFFE",
    ];
    for t in STRING_TEMPLATES {
        for o in okays {
            accept_text(&t.replace("<<S>>", o));
        }
        for b in bads {
            reject_any(&t.replace("<<S>>", b));
        }
    }
    for t in NAME_TEMPLATES {
        for o in okays {
            accept_text(&t.replace("<<K>>", o));
        }
        for b in bads {
            reject_any(&t.replace("<<K>>", b));
        }
    }
    // A noncharacter in an op name or member name of the instruction itself.
    reject_any(r#"{"op":"POPULATE￾"}"#);
    reject_any(r#"{"op":"POPULATE","￿":1}"#);
}

/// REQ-NFR-006 (requirement 8, brief A6): unknown, lower-case, mixed-case, missing, null and
/// non-string `op` are rejected by all three entry points.
#[test]
fn req_nfr_006_op_tag_rules() {
    let cases = [
        json!({"op":"NOT_AN_OP"}),
        json!({"op":"populate"}),
        json!({"op":"Populate"}),
        json!({"op":"POPULATE "}),
        json!({"op":""}),
        json!({"op":"TEST_OP"}),
        json!({}),
        json!({"player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}),
        json!({"op":null}),
        json!({"op":7}),
        json!({"op":true}),
        json!({"op":["POPULATE"]}),
        json!({"op":{"POPULATE":{}}}),
        json!({"OP":"POPULATE"}),
        json!({"op":"draw","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}),
        json!("POPULATE"),
        json!(["POPULATE"]),
        json!({"POPULATE":{}}),
        Value::Null,
    ];
    for c in cases {
        reject(&c);
    }
}

/// An IF chain with `levels` IF objects around a DRAW whose count is `count`.
fn if_chain(levels: usize, count: Value) -> Value {
    let mut v = json!({"op":"DRAW","player":{"ref":"CONTROLLER"},"count":count});
    for _ in 0..levels {
        let mut m = Map::new();
        m.insert("op".into(), json!("IF"));
        m.insert("cond".into(), json!({"pred":"TAPPED"}));
        m.insert("then".into(), Value::Array(vec![v]));
        v = Value::Object(m);
    }
    v
}

/// REQ-NFR-006 (requirement 8, brief A2): nesting deeper than `canonical::MAX_DEPTH` (127) is
/// rejected by `from_str`, `from_slice` and `from_value`.
#[test]
fn req_nfr_006_nesting_beyond_budget_rejected() {
    assert_eq!(MAX_DEPTH, 127);
    // 2*63 + DRAW(1) + count(1) = 128.
    let over = if_chain(63, json!({"LITERAL":1}));
    reject(&over);
    reject(&if_chain(62, json!({"COUNT":{"zones":["BATTLEFIELD"]}})));
    let far = if_chain(300, json!({"LITERAL":1}));
    reject(&far);
    // Deep opaque contents count too.
    let mut deep = json!(1);
    for _ in 0..200 {
        deep = json!([deep]);
    }
    reject(&json!({"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k1":deep}}));
    let text = format!(
        r#"{{"op":"CREATE_TOKEN","inline":{{"k1":{}1{}}},"count":{{"LITERAL":1}}}}"#,
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    reject_text(&text);
}
