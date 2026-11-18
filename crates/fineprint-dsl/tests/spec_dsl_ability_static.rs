// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::ability` (FP-012 wire contract), part 3: static abilities
//! (REQ-DSL-004, REQ-DSL-008, SRS §14.4.3: CHAR / PLAYER / RULE effects, modifications and the
//! 613.6 lock-in through `sameAsLayer`) and replacement abilities (REQ-DSL-009, SRS §14.4.4:
//! match, ReplacementOp, tierHint 616.1b–d, shield). Black-box: written from the work-item brief
//! only. Synthetic data only (R5).

use std::fmt::Debug;

use fineprint_dsl::ability::{AbilityAST, ReplacementOp, StaticEffect};
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const LAYERS: [&str; 11] = ["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"];

const TIER_HINTS: [&str; 3] = ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"];

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

// ---------------------------------------------------------------- helpers

fn text(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    };
    let t2: T = serde_json::from_slice(s.as_bytes()).unwrap();
    assert_eq!(t2, t);
    let original: Value = serde_json::from_str(s).unwrap();
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        original,
        "to_value of {s}"
    );
    let canon = canonicalize(s.as_bytes()).unwrap();
    assert_eq!(
        to_canonical_vec(&t).unwrap(),
        canon,
        "canonical bytes of {s}"
    );
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t);
    let back: T = serde_json::from_slice(&serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!(back, t);
    t
}

fn accept_v<T>(v: &Value) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    accept::<T>(&text(v))
}

fn reject<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(s.as_bytes()).is_err(),
        "expected rejection (from_slice) of {s}"
    );
    if canonicalize(s.as_bytes()).is_ok() {
        let v: Value = serde_json::from_str(s).unwrap();
        assert!(
            serde_json::from_value::<T>(v).is_err(),
            "expected rejection (from_value) of {s}"
        );
    }
}

fn reject_v<T: DeserializeOwned + Debug>(v: &Value) {
    reject::<T>(&text(v))
}

fn with(base: &Value, k: &str, x: Value) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().insert(k.to_string(), x);
    v
}

fn without(base: &Value, k: &str) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove(k);
    v
}

fn stat(effects: Value) -> Value {
    json!({"abilityId":"s","kind":"STATIC","effects":effects})
}

fn char_effect(layer: &str, affected: Value, modification: Value) -> Value {
    json!({"type":"CHAR","layer":layer,"affected":affected,"modification":modification})
}

fn creatures() -> Value {
    json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"})
}

fn op(name: &str) -> Value {
    json!({ "op": name })
}

/// Accepts the effect standalone and inside a STATIC ability.
fn accept_effect(e: &Value) -> StaticEffect {
    accept_v::<AbilityAST>(&stat(json!([e])));
    accept_v(e)
}

/// Rejects the effect standalone and inside a STATIC ability.
fn reject_effect(e: &Value) {
    reject_v::<StaticEffect>(e);
    reject_v::<AbilityAST>(&stat(json!([e])));
}

fn repl(m: Value, ops: Value) -> Value {
    json!({"abilityId":"r","kind":"REPLACEMENT","match":m,"replaceWith":ops})
}

fn self_match() -> Value {
    json!({"event":"ENTERS","affected":{"ref":"SELF"}})
}

fn accept_op(o: &Value) -> ReplacementOp {
    accept_v::<AbilityAST>(&repl(self_match(), json!([o])));
    accept_v(o)
}

fn reject_op(o: &Value) {
    reject_v::<ReplacementOp>(o);
    reject_v::<AbilityAST>(&repl(self_match(), json!([o])));
}

// ---------------------------------------------------------------- 7. static abilities

/// REQ-DSL-008: `effects` is required and non-empty; the effect `type` is CHAR, PLAYER or RULE.
#[test]
fn req_dsl_008_static_effects_required() {
    let ok = stat(json!([{"type":"RULE","kind":"SKIP"}]));
    accept_v::<AbilityAST>(&ok);
    reject_v::<AbilityAST>(&stat(json!([])));
    reject_v::<AbilityAST>(&stat(json!(null)));
    reject_v::<AbilityAST>(&stat(json!({"type":"RULE","kind":"SKIP"})));
    reject_v::<AbilityAST>(&without(&ok, "effects"));
    reject_v::<AbilityAST>(&with(&ok, "effect", json!([])));
    accept_effect(&char_effect("6", creatures(), op("TEST_OP")));
    for t in ["OTHER", "char", "Rule", "", "LAYER"] {
        reject_effect(&json!({"type":t,"kind":"SKIP"}));
        reject_effect(&with(
            &char_effect("6", creatures(), op("TEST_OP")),
            "type",
            json!(t),
        ));
    }
    reject_effect(&json!({"kind":"SKIP"}));
    reject_effect(&json!({"type":null,"kind":"SKIP"}));
    // Order of effects is kept.
    let a: AbilityAST = accept_v(&stat(
        json!([{"type":"RULE","kind":"SKIP"},{"type":"RULE","kind":"EVASION"}]),
    ));
    let b: AbilityAST = accept_v(&stat(
        json!([{"type":"RULE","kind":"EVASION"},{"type":"RULE","kind":"SKIP"}]),
    ));
    assert_ne!(a, b);
}

/// REQ-DSL-004, REQ-DSL-008: CHAR needs layer, affected and modification; every Layer string is
/// accepted and others are not.
#[test]
fn req_dsl_008_char_effect_members_and_layers() {
    let mut seen: Vec<StaticEffect> = Vec::new();
    for l in LAYERS {
        let e = accept_effect(&char_effect(l, creatures(), op("TEST_OP")));
        assert!(!seen.contains(&e), "{l}");
        seen.push(e);
    }
    let ok = char_effect("4", creatures(), op("TEST_OP"));
    for m in ["layer", "affected", "modification"] {
        reject_effect(&without(&ok, m));
        reject_effect(&with(&ok, m, json!(null)));
    }
    for l in [
        json!("1"),
        json!("7"),
        json!("7e"),
        json!("1A"),
        json!("7B"),
        json!("8"),
        json!("0"),
        json!(""),
        json!("6 "),
        json!(6),
    ] {
        reject_effect(&with(&ok, "layer", l));
    }
    for a in [
        json!({"ref":"SELF"}),
        json!({"ref":"TARGET","slot":0}),
        json!({"pred":"IS_CREATURE"}),
        json!({"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"TAPPED"}]}),
        json!({"zones":["GRAVEYARD"]}),
    ] {
        accept_effect(&with(&ok, "affected", a));
    }
    for a in [
        json!({}),
        json!("SELF"),
        json!({"ref":"NOBODY"}),
        json!({"players":"NOBODY"}),
    ] {
        reject_effect(&with(&ok, "affected", a));
    }
    for (k, v) in [
        ("zzUnknown", json!(1)),
        ("kind", json!("SKIP")),
        ("op", json!("TEST_OP")),
        ("Layer", json!("4")),
    ] {
        reject_effect(&with(&ok, k, v));
    }
}

/// REQ-DSL-008: isCDA true and false are both kept, distinct from absent; null rejected.
#[test]
fn req_dsl_008_char_is_cda() {
    let ok = char_effect("7a", json!({"ref":"SELF"}), op("TEST_OP"));
    let a = accept_effect(&ok);
    let t = accept_effect(&with(&ok, "isCDA", json!(true)));
    let f = accept_effect(&with(&ok, "isCDA", json!(false)));
    assert_ne!(a, t);
    assert_ne!(a, f);
    assert_ne!(t, f);
    assert_eq!(serde_json::to_value(&f).unwrap()["isCDA"], json!(false));
    assert!(serde_json::to_value(&a).unwrap().get("isCDA").is_none());
    for v in [json!(null), json!("true"), json!(1)] {
        reject_effect(&with(&ok, "isCDA", v));
    }
    reject_effect(&with(&ok, "iscda", json!(true)));
    reject_effect(&with(&ok, "isCda", json!(true)));
}

/// REQ-DSL-008: Modification op pattern; flat opaque params; nulls rejected.
#[test]
fn req_dsl_008_modification_op_and_params() {
    let ok = char_effect("6", creatures(), op("REMOVE_ALL_ABILITIES"));
    accept_effect(&ok);
    for o in ["A", "A1", "TEST_OP_2", "Z_"] {
        accept_effect(&with(&ok, "modification", op(o)));
    }
    for o in [
        json!("lower"),
        json!("Test"),
        json!("1A"),
        json!("_A"),
        json!("A-B"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        reject_effect(&with(&ok, "modification", json!({ "op": o })));
    }
    reject_effect(&with(&ok, "modification", json!({})));
    reject_effect(&with(&ok, "modification", json!({"ability":"TEST"})));
    reject_effect(&with(&ok, "modification", json!("TEST_OP")));
    let params = json!({"op":"ADD_ABILITY","ability":{"keyword":"TEST_KW"},"n":2,"list":[1,"x",false],
        "any":"thing","power":"X"});
    accept_effect(&with(&ok, "modification", params));
    for bad in [
        json!({"op":"TEST_OP","n":null}),
        json!({"op":"TEST_OP","n":[null]}),
        json!({"op":"TEST_OP","n":{"m":null}}),
        json!({"op":"TEST_OP","":1}),
        json!({"op":"TEST_OP","n":9007199254740992u64}),
    ] {
        reject_effect(&with(&ok, "modification", bad));
    }
    reject::<StaticEffect>(
        r#"{"type":"CHAR","layer":"6","affected":{"pred":"IS_CREATURE"},"modification":{"op":"TEST_OP","n":1.5}}"#,
    );
    reject::<StaticEffect>(
        r#"{"type":"CHAR","layer":"6","affected":{"pred":"IS_CREATURE"},"modification":{"op":"TEST_OP","op":"OTHER_OP"}}"#,
    );
}

/// REQ-DSL-008: SET_BASE_PT and MODIFY_PT need power and toughness, each a safe integer or a
/// value-expression object.
#[test]
fn req_dsl_008_pt_modifications() {
    for o in ["SET_BASE_PT", "MODIFY_PT"] {
        let good = [
            (json!(1), json!(1)),
            (json!(0), json!(-3)),
            (json!(-9007199254740991i64), json!(9007199254740991i64)),
            (json!({"LITERAL":2}), json!({"LITERAL":-1})),
            (json!({"COUNT":{"pred":"IS_CREATURE"}}), json!(1)),
            (
                json!(1),
                json!({"MATH":{"op":"ADD","lhs":"X","rhs":{"LITERAL":1}}}),
            ),
        ];
        for (p, t) in &good {
            let m = json!({"op":o,"power":p,"toughness":t});
            accept_effect(&char_effect("7b", creatures(), m));
        }
        let m = json!({"op":o,"power":1,"toughness":1});
        accept_effect(&char_effect(
            "7c",
            creatures(),
            with(&m, "extra", json!("x")),
        ));
        reject_effect(&char_effect("7b", creatures(), without(&m, "power")));
        reject_effect(&char_effect("7b", creatures(), without(&m, "toughness")));
        reject_effect(&char_effect("7b", creatures(), json!({ "op": o })));
        for bad in [
            json!("1"),
            json!("X"),
            json!(null),
            json!(true),
            json!([1]),
            json!({}),
            json!({"NOPE":1}),
            json!(9007199254740992u64),
        ] {
            reject_effect(&char_effect(
                "7b",
                creatures(),
                with(&m, "power", bad.clone()),
            ));
            reject_effect(&char_effect("7b", creatures(), with(&m, "toughness", bad)));
        }
        reject::<StaticEffect>(&format!(
            r#"{{"type":"CHAR","layer":"7b","affected":{{"pred":"IS_CREATURE"}},"modification":{{"op":"{o}","power":1.5,"toughness":1}}}}"#
        ));
        reject::<StaticEffect>(&format!(
            r#"{{"type":"CHAR","layer":"7b","affected":{{"pred":"IS_CREATURE"}},"modification":{{"op":"{o}","power":1,"toughness":2.0}}}}"#
        ));
    }
    // Any other op has no member checks: power "X" or a string is fine there.
    accept_effect(&char_effect(
        "7b",
        creatures(),
        json!({"op":"TEST_PT","power":"X","toughness":"1"}),
    ));
    accept_effect(&char_effect(
        "7b",
        creatures(),
        json!({"op":"SET_BASE_PT_LIKE","power":"X"}),
    ));
}

/// REQ-DSL-004, REQ-DSL-008: PLAYER needs affected (a player selector) and modification.
#[test]
fn req_dsl_008_player_effect() {
    let ok =
        json!({"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"TEST_OP"}});
    accept_effect(&ok);
    for a in [
        json!({"players":"ALL_PLAYERS"}),
        json!({"players":"TEAMMATES"}),
        json!({"ref":"EACH_OPPONENT"}),
        json!({"ref":"ACTIVE_PLAYER"}),
    ] {
        accept_effect(&with(&ok, "affected", a));
    }
    accept_effect(&with(
        &ok,
        "modification",
        json!({"op":"MAX_HAND_SIZE_SET","value":7}),
    ));
    reject_effect(&without(&ok, "affected"));
    reject_effect(&without(&ok, "modification"));
    for a in [
        json!({"pred":"IS_CREATURE"}),
        json!({"zones":["BATTLEFIELD"]}),
        json!({"players":"NOBODY"}),
        json!({}),
        json!(null),
    ] {
        reject_effect(&with(&ok, "affected", a));
    }
    for (k, v) in [
        ("layer", json!("6")),
        ("isCDA", json!(false)),
        ("kind", json!("SKIP")),
        ("zzUnknown", json!(1)),
        ("modification", json!({"op":"lower"})),
        ("modification", json!(null)),
    ] {
        reject_effect(&with(&ok, k, v));
    }
}

/// REQ-DSL-004, REQ-DSL-008: RULE params: opaque, no null, non-empty keys, no duplicate
/// `kind`/`type` (W2), `cost` a non-empty CostAST array.
#[test]
fn req_dsl_008_rule_effect_params() {
    let ok = json!({"type":"RULE","kind":"COST_INCREASE","affected":{"pred":"IS_CREATURE"},
        "amount":{"LITERAL":1},"note":"x","n":-4,"flags":[true,false],"deep":{"a":{"b":[]}}});
    accept_effect(&ok);
    reject_effect(&without(&ok, "kind"));
    reject_effect(&with(&ok, "kind", json!(null)));
    reject_effect(&with(&ok, "kind", json!("cost_increase")));
    for c in [
        json!([{"cost":"TAP_SOURCE"}]),
        json!([{"cost":"PAY_MANA","mana":"{2}"},{"cost":"PAY_LIFE","amount":{"LITERAL":2}}]),
        json!([{"cost":"COMPOSITE","costs":[{"cost":"FORAGE"}]}]),
    ] {
        accept_effect(&with(&ok, "cost", c));
    }
    for c in [
        json!([]),
        json!([{"cost":"NOPE"}]),
        json!([{"cost":"TAP_SOURCE","zzUnknown":1}]),
        json!({"cost":"TAP_SOURCE"}),
        json!("{2}"),
        json!(2),
        json!(null),
        json!([null]),
    ] {
        reject_effect(&with(&ok, "cost", c));
    }
    for bad in [
        ("n", json!(null)),
        ("n", json!([1, null])),
        ("n", json!({"m":{"k":null}})),
        ("", json!(1)),
        ("n", json!(9007199254740992u64)),
    ] {
        reject_effect(&with(&ok, bad.0, bad.1));
    }
    // Duplicate `kind` / `type` members (W2), with the single-member form as control.
    accept::<StaticEffect>(r#"{"type":"RULE","kind":"SKIP"}"#);
    reject::<StaticEffect>(r#"{"type":"RULE","kind":"SKIP","kind":"EVASION"}"#);
    reject::<StaticEffect>(r#"{"type":"RULE","kind":"SKIP","kind":"SKIP"}"#);
    reject::<StaticEffect>(r#"{"type":"RULE","type":"RULE","kind":"SKIP"}"#);
    reject::<StaticEffect>(r#"{"type":"RULE","kind":"SKIP","n":1,"n":2}"#);
    reject::<AbilityAST>(
        r#"{"abilityId":"s","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP","kind":"EVASION"}]}"#,
    );
    reject::<StaticEffect>(r#"{"type":"RULE","kind":"SKIP","n":1.5}"#);
}

/// REQ-DSL-008 (613.6 lock-in): `{"sameAsLayer":L}` needs another CHAR effect in the same ability
/// with layer L and a selector `affected`.
#[test]
fn req_dsl_008_same_as_layer_lock_in() {
    let six = char_effect("6", creatures(), op("REMOVE_ALL_ABILITIES"));
    let lock =
        |from: &str, to: &str| char_effect(from, json!({ "sameAsLayer": to }), op("TEST_OP"));
    // Accepted: referenced effect before or after; several references to one selector effect.
    accept_v::<AbilityAST>(&stat(json!([six, lock("7b", "6")])));
    accept_v::<AbilityAST>(&stat(json!([lock("7b", "6"), six])));
    accept_v::<AbilityAST>(&stat(json!([six, lock("7b", "6"), lock("7c", "6")])));
    for l in LAYERS {
        let target = char_effect(l, creatures(), op("TEST_OP"));
        let other = if l == "7d" { "1a" } else { "7d" };
        accept_v::<AbilityAST>(&stat(json!([target, lock(other, l)])));
    }
    // Dangling: no effect at that layer, or only a lock-in at that layer.
    reject_v::<AbilityAST>(&stat(json!([lock("7b", "6")])));
    reject_v::<AbilityAST>(&stat(json!([
        char_effect("4", creatures(), op("TEST_OP")),
        lock("7b", "6")
    ])));
    reject_v::<AbilityAST>(&stat(
        json!([{"type":"RULE","kind":"SKIP"}, lock("7b", "6")]),
    ));
    reject_v::<AbilityAST>(&stat(json!([
        {"type":"PLAYER","affected":{"ref":"CONTROLLER"},"modification":{"op":"TEST_OP"}},
        lock("7b", "6")
    ])));
    // Self-reference (same layer, nothing else at that layer).
    reject_v::<AbilityAST>(&stat(json!([lock("6", "6")])));
    reject_v::<AbilityAST>(&stat(json!([
        char_effect("4", creatures(), op("TEST_OP")),
        lock("6", "6")
    ])));
    // Chained: the referenced layer-6 effect is itself a lock-in.
    let four = char_effect("4", creatures(), op("TEST_OP"));
    accept_v::<AbilityAST>(&stat(json!([four, lock("6", "4"), lock("7b", "4")])));
    reject_v::<AbilityAST>(&stat(json!([four, lock("6", "4"), lock("7b", "6")])));
    reject_v::<AbilityAST>(&stat(json!([lock("6", "7b"), lock("7b", "6")])));
    // Invalid layer in sameAsLayer.
    for l in [
        json!("8"),
        json!("7e"),
        json!("1A"),
        json!(""),
        json!(6),
        json!(null),
    ] {
        let e = char_effect("7b", json!({ "sameAsLayer": l }), op("TEST_OP"));
        reject_v::<AbilityAST>(&stat(json!([six, e])));
    }
    // sameAsLayer with other members.
    for extra in [
        json!({"sameAsLayer":"6","pred":"IS_CREATURE"}),
        json!({"sameAsLayer":"6","zones":["BATTLEFIELD"]}),
        json!({"sameAsLayer":"6","ref":"SELF"}),
        json!({"sameAsLayer":"6","zzUnknown":1}),
        json!({"same_as_layer":"6"}),
    ] {
        let e = char_effect("7b", extra, op("TEST_OP"));
        reject_v::<AbilityAST>(&stat(json!([six, e])));
    }
    // sameAsLayer is not a player selector.
    reject_v::<AbilityAST>(&stat(json!([six,
        {"type":"PLAYER","affected":{"sameAsLayer":"6"},"modification":{"op":"TEST_OP"}}])));
}

// ---------------------------------------------------------------- 8. replacement abilities

/// REQ-DSL-009: match and replaceWith are required; replaceWith non-empty.
#[test]
fn req_dsl_009_replacement_required_members() {
    let ok = repl(self_match(), json!([{"op":"SKIP"}]));
    accept_v::<AbilityAST>(&ok);
    reject_v::<AbilityAST>(&without(&ok, "match"));
    reject_v::<AbilityAST>(&without(&ok, "replaceWith"));
    reject_v::<AbilityAST>(&with(&ok, "replaceWith", json!([])));
    reject_v::<AbilityAST>(&with(&ok, "replaceWith", json!(null)));
    reject_v::<AbilityAST>(&with(&ok, "replaceWith", json!({"op":"SKIP"})));
    reject_v::<AbilityAST>(&with(&ok, "match", json!(null)));
    reject_v::<AbilityAST>(&with(&ok, "effect", json!([])));
    let two: AbilityAST = accept_v(&with(
        &ok,
        "replaceWith",
        json!([{"op":"SKIP"},{"op":"PREVENT"}]),
    ));
    let owt: AbilityAST = accept_v(&with(
        &ok,
        "replaceWith",
        json!([{"op":"PREVENT"},{"op":"SKIP"}]),
    ));
    assert_ne!(two, owt);
}

/// REQ-DSL-009: match members: event (EventKind), filters (as D), affected (subject forms).
#[test]
fn req_dsl_009_match_members() {
    let ops = json!([{"op":"SKIP"}]);
    for a in [
        json!({"ref":"SELF"}),
        json!({"ref":"TARGET","slot":0}),
        json!({"pred":"IS_CREATURE"}),
        json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        json!({"op":"AND","conditions":[{"pred":"IS_CREATURE"}]}),
        json!({"players":"ALL_PLAYERS"}),
        json!({"players":"TEAMMATES"}),
    ] {
        accept_v::<AbilityAST>(&repl(
            json!({"event":"DAMAGE_DEALT","affected":a}),
            ops.clone(),
        ));
    }
    let mut filters = vec![
        json!({}),
        json!({"to":"GRAVEYARD","from":"ANY"}),
        json!({"combat":true,"amount":3,"name":"TestType","who":{"pred":"IS_PLAYER"}}),
    ];
    for z in ZONES {
        filters.push(json!({ "to": z }));
    }
    for f in filters {
        accept_v::<AbilityAST>(&repl(
            json!({"event":"ZONE_CHANGE","filters":f,"affected":{"ref":"SELF"}}),
            ops.clone(),
        ));
    }
    let m = json!({"event":"ZONE_CHANGE","filters":{"to":"EXILE"},"affected":{"ref":"SELF"}});
    accept_v::<AbilityAST>(&repl(m.clone(), ops.clone()));
    for bad in [
        without(&m, "event"),
        without(&m, "affected"),
        with(&m, "filters", json!(null)),
        with(&m, "filters", json!({"to":null})),
        with(&m, "filters", json!({"to":[]})),
        with(&m, "filters", json!({"to":""})),
        with(&m, "filters", json!({"":"EXILE"})),
        with(&m, "affected", json!({"players":"NOBODY"})),
        with(&m, "affected", json!({})),
        with(&m, "affected", json!(null)),
        with(&m, "event", json!("zone_change")),
        with(&m, "event", json!(null)),
        with(&m, "kind", json!("ZONE_CHANGE")),
        with(&m, "subject", json!({"ref":"SELF"})),
        with(&m, "bind", json!({"x":"$x"})),
        with(&m, "batch", json!("EACH")),
        with(&m, "zzUnknown", json!(1)),
    ] {
        reject_v::<AbilityAST>(&repl(bad, ops.clone()));
    }
}

/// REQ-DSL-009: each ReplacementOp form and each rejection.
#[test]
fn req_dsl_009_replacement_ops() {
    let good = [
        json!({"op":"MODIFY_EVENT","set":{"to":"EXILE"}}),
        json!({"op":"MODIFY_EVENT","set":{"to":"ANY","n":2,"b":false,"o":{"pred":"TAPPED"},"name":"TestType"}}),
        json!({"op":"INSTEAD","instructions":[{"op":"TEST_OP"}]}),
        json!({"op":"INSTEAD","instructions":[{"op":"TEST_OP","n":1},{"op":"OTHER_OP","s":"x"}]}),
        json!({"op":"SKIP"}),
        json!({"op":"PREVENT"}),
        json!({"op":"PREVENT","amount":{"LITERAL":2}}),
        json!({"op":"PREVENT","amount":"X"}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"},"store":"chosenType"}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":"k"}),
    ];
    let mut seen: Vec<ReplacementOp> = Vec::new();
    for g in &good {
        let o = accept_op(g);
        assert!(!seen.contains(&o), "{g}");
        seen.push(o);
    }
    let bad = [
        json!({"op":"REPLACE"}),
        json!({"op":"skip"}),
        json!({"op":""}),
        json!({"op":null}),
        json!({}),
        json!({"op":"MODIFY_EVENT"}),
        json!({"op":"MODIFY_EVENT","set":{}}),
        json!({"op":"MODIFY_EVENT","set":{"to":null}}),
        json!({"op":"MODIFY_EVENT","set":{"to":[]}}),
        json!({"op":"MODIFY_EVENT","set":{"to":""}}),
        json!({"op":"MODIFY_EVENT","set":{"":"EXILE"}}),
        json!({"op":"MODIFY_EVENT","set":[]}),
        json!({"op":"MODIFY_EVENT","set":{"to":"EXILE"},"zzUnknown":1}),
        json!({"op":"INSTEAD"}),
        json!({"op":"INSTEAD","instructions":[]}),
        json!({"op":"INSTEAD","instructions":[{"op":"bad"}]}),
        json!({"op":"INSTEAD","instructions":[{"op":"TEST_OP","n":null}]}),
        json!({"op":"INSTEAD","instructions":{"op":"TEST_OP"}}),
        json!({"op":"SKIP","amount":{"LITERAL":1}}),
        json!({"op":"SKIP","set":{"to":"EXILE"}}),
        json!({"op":"SKIP","zzUnknown":1}),
        json!({"op":"PREVENT","amount":2}),
        json!({"op":"PREVENT","amount":null}),
        json!({"op":"PREVENT","zzUnknown":1}),
        json!({"op":"CHOOSE","choice":"CREATURE_TYPE","store":"chosenType"}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"},"store":""}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"}}),
        json!({"op":"CHOOSE","store":"chosenType"}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"NOPE"},"store":"k"}),
        json!({"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"COLOR"},"store":"k","zzUnknown":1}),
        json!("SKIP"),
        json!(null),
    ];
    for b in &bad {
        reject_op(b);
    }
}

/// REQ-DSL-009 (616.1b–d): tierHint takes exactly the three listed values.
#[test]
fn req_dsl_009_tier_hint() {
    let base = repl(self_match(), json!([{"op":"SKIP"}]));
    let plain: AbilityAST = accept_v(&base);
    let mut seen = vec![plain];
    for t in TIER_HINTS {
        let a: AbilityAST = accept_v(&with(&base, "tierHint", json!(t)));
        assert!(!seen.contains(&a));
        seen.push(a);
    }
    for t in [
        json!("OTHER"),
        json!("copy_etb"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        reject_v::<AbilityAST>(&with(&base, "tierHint", t));
    }
}

/// REQ-DSL-009: isSelfReplacement (614.15), optional and overrides614_5 round-trip; false is
/// kept distinct from absent.
#[test]
fn req_dsl_009_replacement_flags() {
    let base = repl(self_match(), json!([{"op":"SKIP"}]));
    let plain: AbilityAST = accept_v(&base);
    for k in ["isSelfReplacement", "optional", "overrides614_5"] {
        let t: AbilityAST = accept_v(&with(&base, k, json!(true)));
        let f: AbilityAST = accept_v(&with(&base, k, json!(false)));
        assert_ne!(t, f, "{k}");
        assert_ne!(f, plain, "{k}");
        assert_ne!(t, plain, "{k}");
        assert_eq!(serde_json::to_value(&f).unwrap()[k], json!(false));
        for v in [json!(null), json!("true"), json!(0)] {
            reject_v::<AbilityAST>(&with(&base, k, v));
        }
    }
    reject_v::<AbilityAST>(&with(&base, "overrides6145", json!(true)));
    reject_v::<AbilityAST>(&with(&base, "overrides614_5a", json!(true)));
    reject_v::<AbilityAST>(&with(&base, "selfReplacement", json!(true)));
    let all = with(
        &with(
            &with(
                &with(&base, "isSelfReplacement", json!(true)),
                "optional",
                json!(false),
            ),
            "overrides614_5",
            json!(true),
        ),
        "tierHint",
        json!("COPY_ETB"),
    );
    accept_v::<AbilityAST>(&all);
}

/// REQ-DSL-009: shield uses (integer >= 1 or COUNTER_BACKED) and counterKind rules.
#[test]
fn req_dsl_009_shield() {
    let base = repl(self_match(), json!([{"op":"PREVENT"}]));
    let plain: AbilityAST = accept_v(&base);
    let mut seen = vec![plain];
    for s in [
        json!({"uses":1}),
        json!({"uses":5}),
        json!({"uses":"COUNTER_BACKED"}),
        json!({"uses":"COUNTER_BACKED","counterKind":"test_counter"}),
    ] {
        let a: AbilityAST = accept_v(&with(&base, "shield", s));
        assert!(!seen.contains(&a));
        seen.push(a);
    }
    for s in [
        json!({"uses":0}),
        json!({"uses":-1}),
        json!({"uses":"OTHER"}),
        json!({"uses":"counter_backed"}),
        json!({"uses":"1"}),
        json!({"uses":null}),
        json!({"uses":1,"counterKind":"test_counter"}),
        json!({"uses":5,"counterKind":"test_counter"}),
        json!({"uses":"COUNTER_BACKED","counterKind":""}),
        json!({"uses":"COUNTER_BACKED","counterKind":null}),
        json!({"uses":"COUNTER_BACKED","counterKind":1}),
        json!({"counterKind":"test_counter"}),
        json!({}),
        json!({"uses":1,"zzUnknown":1}),
        json!({"uses":9007199254740992u64}),
        json!(1),
        json!(null),
    ] {
        reject_v::<AbilityAST>(&with(&base, "shield", s));
    }
    reject::<AbilityAST>(
        r#"{"abilityId":"r","kind":"REPLACEMENT","match":{"event":"ENTERS","affected":{"ref":"SELF"}},"replaceWith":[{"op":"PREVENT"}],"shield":{"uses":1.0}}"#,
    );
}
