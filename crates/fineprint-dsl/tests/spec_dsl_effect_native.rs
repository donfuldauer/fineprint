// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::effect` (FP-013 wire contract), part 2: the NATIVE_EFFECT node
//! (REQ-DSL-015, SRS §14.8), `DurationAST` (REQ-DSL-010, SRS §14.4.5; FOR_AS_LONG_AS is 611.2b),
//! the small enums, and the Serialize re-checks of hand-built invalid values (brief A2).
//! Black-box: written from the work-item brief only. Synthetic data only (R5).

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use fineprint_dsl::ability::{Layer, RuleKind, StaticEffect, StepId};
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use fineprint_dsl::effect::{
    DurationAST, Instruction, InstructionMode, MemoryStore, NativeEffectAST, Position, TapAction,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerSelectorAST, PredicateAST, ValueExpressionAST};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------- helpers

fn accept<T>(doc: &Value) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(doc).unwrap();
    let t: T = match serde_json::from_str(&text) {
        Ok(t) => t,
        Err(e) => panic!("expected {text} to decode, got error: {e}"),
    };
    assert_eq!(serde_json::from_slice::<T>(text.as_bytes()).unwrap(), t);
    assert_eq!(serde_json::from_value::<T>(doc.clone()).unwrap(), t);
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&t).unwrap()).unwrap(),
        String::from_utf8(canon).unwrap(),
        "canonical bytes of {text}"
    );
    let enc = serde_json::to_string(&t).unwrap();
    assert_eq!(serde_json::from_str::<T>(&enc).unwrap(), t);
    t
}

fn reject<T: DeserializeOwned + std::fmt::Debug>(doc: &Value) {
    let text = serde_json::to_string(doc).unwrap();
    let r = serde_json::from_str::<T>(&text);
    assert!(r.is_err(), "expected rejection of {text}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(text.as_bytes()).is_err(),
        "{text}"
    );
    assert!(serde_json::from_value::<T>(doc.clone()).is_err(), "{text}");
}

fn reject_text<T: DeserializeOwned + std::fmt::Debug>(text: &str) {
    let r = serde_json::from_str::<T>(text);
    assert!(r.is_err(), "expected rejection of {text}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<T>(text.as_bytes()).is_err(),
        "{text}"
    );
}

/// Serialising fails (both `serde_json` and the canonical encoder).
fn fails_to_serialise<T: Serialize + std::fmt::Debug>(x: &T) {
    assert!(
        serde_json::to_string(x).is_err(),
        "expected {x:?} to fail to serialise"
    );
    assert!(to_canonical_vec(x).is_err(), "canonical of {x:?}");
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

fn native_min() -> Value {
    json!({"op":"NATIVE_EFFECT","id":"native.test_effect","version":1,"params":{}})
}

fn from<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

// ---------------------------------------------------------------- 4. NATIVE_EFFECT

/// REQ-DSL-015 (requirement 4, SRS §14.8): the SRS example parses verbatim (as `Instruction` and
/// as `NativeEffectAST`), round-trips, and its registry key is `id@version`.
#[test]
fn req_dsl_015_srs_example_parses_and_round_trips() {
    let text = r#"{"op":"NATIVE_EFFECT","id":"native.subgame.start","version":2,"params":{}}"#;
    let doc: Value = serde_json::from_str(text).unwrap();
    let i: Instruction = accept(&doc);
    assert_eq!(i.name(), "NATIVE_EFFECT");
    let n: NativeEffectAST = accept(&doc);
    assert_eq!(n.id, "native.subgame.start");
    assert_eq!(n.version, 2);
    assert!(n.params.is_empty());
    assert_eq!(n.layers, None);
    assert_eq!(n.rule_kinds, None);
    assert_eq!(n.registry_key(), "native.subgame.start@2");
    assert_eq!(i, Instruction::NativeEffect(n.clone()));
    // The canonical bytes are the RFC 8785 form of the example itself.
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(to_canonical_vec(&i).unwrap(), canon);
    assert_eq!(to_canonical_vec(&n).unwrap(), canon);
    assert_eq!(serde_json::to_value(&i).unwrap(), doc);
}

/// REQ-DSL-015 (requirement 4): `NativeEffectAST::new` gives empty params and no declarations;
/// standalone JSON is the whole object including `"op":"NATIVE_EFFECT"`.
#[test]
fn req_dsl_015_new_and_registry_key() {
    let n = NativeEffectAST::new("native.x", 3);
    assert_eq!(n.id, "native.x");
    assert_eq!(n.version, 3);
    assert!(n.params.is_empty());
    assert_eq!(n.layers, None);
    assert_eq!(n.rule_kinds, None);
    assert_eq!(n.registry_key(), "native.x@3");
    let expect = json!({"op":"NATIVE_EFFECT","id":"native.x","version":3,"params":{}});
    assert_eq!(serde_json::to_value(&n).unwrap(), expect);
    assert_eq!(
        serde_json::to_value(Instruction::NativeEffect(n.clone())).unwrap(),
        expect
    );
    let back: NativeEffectAST = accept(&expect);
    assert_eq!(back, n);
    let n2 = NativeEffectAST::new(String::from("native.y"), u32::MAX);
    assert_eq!(n2.registry_key(), "native.y@4294967295");
}

/// REQ-DSL-015 (requirement 4, brief E): `id` is a non-empty string without `@`; required.
#[test]
fn req_dsl_015_id_rules() {
    let base = native_min();
    for good in ["n", "native.a.b_c-1", "x\u{e9}"] {
        accept::<Instruction>(&with(&base, "id", json!(good)));
    }
    for bad in [
        json!(""),
        json!("a@b"),
        json!("@"),
        json!("native.x@2"),
        json!(7),
        Value::Null,
    ] {
        reject::<Instruction>(&with(&base, "id", bad.clone()));
        reject::<NativeEffectAST>(&with(&base, "id", bad));
    }
    reject::<Instruction>(&without(&base, "id"));
    reject::<NativeEffectAST>(&without(&base, "id"));
}

/// REQ-DSL-015 (requirement 4, brief E): `version` is an integer 1..=4294967295; required.
#[test]
fn req_dsl_015_version_rules() {
    let base = native_min();
    for good in [1_u64, 2, 4_294_967_295] {
        let n: NativeEffectAST = accept(&with(&base, "version", json!(good)));
        assert_eq!(u64::from(n.version), good);
    }
    for bad in [
        json!(0),
        json!(-1),
        json!(4_294_967_296_u64),
        json!(1.5),
        json!(1.0),
        json!("1"),
        json!(true),
        Value::Null,
    ] {
        reject::<Instruction>(&with(&base, "version", bad.clone()));
        reject::<NativeEffectAST>(&with(&base, "version", bad));
    }
    reject::<Instruction>(&without(&base, "version"));
    reject::<NativeEffectAST>(&without(&base, "version"));
    let t = r#"{"op":"NATIVE_EFFECT","id":"native.x","version":<<V>>,"params":{}}"#;
    for bad in ["1.0", "1e0", "1E2", "-0", "1.5"] {
        reject_text::<Instruction>(&t.replace("<<V>>", bad));
        reject_text::<NativeEffectAST>(&t.replace("<<V>>", bad));
    }
}

/// REQ-DSL-015 (requirement 4, brief A5/E): `params` is a required opaque object (may be empty);
/// non-objects and objects holding null, floats or unsafe integers anywhere are rejected.
#[test]
fn req_dsl_015_params_rules() {
    let base = native_min();
    reject::<Instruction>(&without(&base, "params"));
    reject::<NativeEffectAST>(&without(&base, "params"));
    let goods = [
        json!({}),
        json!({"k1":1,"k2":"v","k3":true,"k4":[1,[2,{}]],"k5":{"a":{"b":false}},"":[]}),
        json!({"k1":9_007_199_254_740_991_i64,"k2":-9_007_199_254_740_991_i64}),
        json!({"k1":""}),
    ];
    for p in goods {
        let n: NativeEffectAST = accept(&with(&base, "params", p.clone()));
        let expect: BTreeMap<String, Value> = from(p);
        assert_eq!(n.params, expect);
    }
    let bads = [
        json!([]),
        json!(7),
        json!("x"),
        json!(true),
        Value::Null,
        json!({"k1":null}),
        json!({"k1":[1,null]}),
        json!({"k1":{"a":null}}),
        json!({"k1":1.5}),
        json!({"k1":[2.0]}),
        json!({"k1":9_007_199_254_740_992_i64}),
        json!({"k1":-9_007_199_254_740_992_i64}),
        json!({"k1":{"a":[18_446_744_073_709_551_615_u64]}}),
    ];
    for p in bads {
        reject::<Instruction>(&with(&base, "params", p.clone()));
        reject::<NativeEffectAST>(&with(&base, "params", p));
    }
    let t = r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k1":<<V>>}}"#;
    for bad in ["1.0", "1e2", "-0", "9007199254740992"] {
        reject_text::<Instruction>(&t.replace("<<V>>", bad));
    }
    // Noncharacters in params member names and strings.
    for bad in [
        r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k￾":1}}"#,
        r#"{"op":"NATIVE_EFFECT","id":"native.x","version":1,"params":{"k":"﷐"}}"#,
        r#"{"op":"NATIVE_EFFECT","id":"native.x￿","version":1,"params":{}}"#,
    ] {
        reject_text::<Instruction>(bad);
        reject_text::<NativeEffectAST>(bad);
    }
}

fn layers_json(n: &NativeEffectAST) -> Option<Value> {
    n.layers.as_ref().map(|l| serde_json::to_value(l).unwrap())
}

fn rule_kinds_json(n: &NativeEffectAST) -> Option<Value> {
    n.rule_kinds
        .as_ref()
        .map(|l| serde_json::to_value(l).unwrap())
}

/// REQ-DSL-015 (requirement 4, brief E): `layers` and `ruleKinds` are optional non-empty lists
/// without duplicates; order is kept; empty, null, duplicate and unknown entries are rejected;
/// absent and present are different values.
#[test]
fn req_dsl_015_layers_and_rule_kinds() {
    let base = native_min();
    let absent: NativeEffectAST = accept(&base);
    for good in [
        json!(["6"]),
        json!(["7d", "1a", "4"]),
        json!(["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"]),
    ] {
        let n: NativeEffectAST = accept(&with(&base, "layers", good.clone()));
        assert_eq!(layers_json(&n), Some(good));
        assert_eq!(n.rule_kinds, None);
        assert_ne!(n, absent);
    }
    for good in [
        json!(["SKIP"]),
        json!(["EVASION", "COST_INCREASE", "SBA_RULE"]),
    ] {
        let n: NativeEffectAST = accept(&with(&base, "ruleKinds", good.clone()));
        assert_eq!(rule_kinds_json(&n), Some(good));
        assert_eq!(n.layers, None);
        assert_ne!(n, absent);
    }
    let both: NativeEffectAST = accept(&with(
        &with(&base, "layers", json!(["2", "1b"])),
        "ruleKinds",
        json!(["ZONE_RULE", "DRAW_RULE"]),
    ));
    assert_eq!(layers_json(&both), Some(json!(["2", "1b"])));
    assert_eq!(
        rule_kinds_json(&both),
        Some(json!(["ZONE_RULE", "DRAW_RULE"]))
    );
    let layers: Vec<Layer> = from(json!(["2", "1b"]));
    let kinds: Vec<RuleKind> = from(json!(["ZONE_RULE", "DRAW_RULE"]));
    assert_eq!(both.layers, Some(layers));
    assert_eq!(both.rule_kinds, Some(kinds));

    for bad in [
        json!([]),
        Value::Null,
        json!(["6", "6"]),
        json!(["1a", "2", "1a"]),
        json!(["8"]),
        json!(["7"]),
        json!(["7A"]),
        json!([6]),
        json!("6"),
    ] {
        reject::<Instruction>(&with(&base, "layers", bad.clone()));
        reject::<NativeEffectAST>(&with(&base, "layers", bad));
    }
    for bad in [
        json!([]),
        Value::Null,
        json!(["SKIP", "SKIP"]),
        json!(["skip"]),
        json!(["NOT_A_RULE_KIND"]),
        json!("SKIP"),
    ] {
        reject::<Instruction>(&with(&base, "ruleKinds", bad.clone()));
        reject::<NativeEffectAST>(&with(&base, "ruleKinds", bad));
    }
    // Unknown members are rejected.
    reject::<Instruction>(&with(&base, "zzUnknown", json!(1)));
    reject::<NativeEffectAST>(&with(&base, "rule_kinds", json!(["SKIP"])));
}

/// REQ-DSL-015 (requirements 4 and 11, brief A2): hand-built invalid `NativeEffectAST` values
/// fail to serialise, standalone and inside an `Instruction`.
#[test]
fn req_dsl_015_invalid_values_fail_to_serialise() {
    let ok = NativeEffectAST::new("native.x", 3);
    let mut bads = Vec::new();
    let mut v = ok.clone();
    v.version = 0;
    bads.push(v);
    let mut v = ok.clone();
    v.id = "native@x".into();
    bads.push(v);
    let mut v = ok.clone();
    v.id = String::new();
    bads.push(v);
    let mut v = ok.clone();
    v.layers = Some(vec![]);
    bads.push(v);
    let mut v = ok.clone();
    v.rule_kinds = Some(vec![]);
    bads.push(v);
    let mut v = ok.clone();
    v.layers = Some(from(json!(["6", "1a"])));
    v.layers.as_mut().unwrap().push(from(json!("6")));
    bads.push(v);
    let mut v = ok.clone();
    v.rule_kinds = Some(from(json!(["SKIP"])));
    v.rule_kinds.as_mut().unwrap().push(from(json!("SKIP")));
    bads.push(v);
    let mut v = ok.clone();
    v.params.insert("k1".into(), Value::Null);
    bads.push(v);
    let mut v = ok.clone();
    v.params.insert("k1".into(), json!(1.5));
    bads.push(v);
    for b in &bads {
        fails_to_serialise(b);
        fails_to_serialise(&Instruction::NativeEffect(b.clone()));
    }
    // A valid hand-built value with declarations serialises and decodes back.
    let mut good = ok;
    good.layers = Some(from(json!(["7b"])));
    good.rule_kinds = Some(from(json!(["EVASION"])));
    good.params.insert("k1".into(), json!({"a":[1,"b"]}));
    let v = serde_json::to_value(&good).unwrap();
    assert_eq!(
        v,
        json!({"op":"NATIVE_EFFECT","id":"native.x","version":3,"params":{"k1":{"a":[1,"b"]}},
               "layers":["7b"],"ruleKinds":["EVASION"]})
    );
    assert_eq!(from::<NativeEffectAST>(v), good);
}

// ---------------------------------------------------------------- 5. DurationAST

fn duration_docs() -> Vec<Value> {
    vec![
        json!({"kind":"UNTIL_END_OF_TURN"}),
        json!({"kind":"UNTIL_YOUR_NEXT_TURN"}),
        json!({"kind":"UNTIL_END_OF_COMBAT"}),
        json!({"kind":"UNTIL","step":"END","whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"FOR_AS_LONG_AS","predicate":{"pred":"IS_CREATURE"}}),
        json!({"kind":"WHILE_SOURCE_ON_BATTLEFIELD"}),
        json!({"kind":"WHILE_CONTROLLED_BY","player":{"ref":"CONTROLLER"}}),
        json!({"kind":"PERMANENT"}),
        json!({"kind":"PERPETUAL"}),
    ]
}

/// REQ-DSL-010 (requirement 5, brief D): `DurationAST::KINDS` is the nine kinds in order;
/// each standalone document decodes, round-trips, and `kind()` names it.
#[test]
fn req_dsl_010_duration_kinds_standalone() {
    let kinds = [
        "UNTIL_END_OF_TURN",
        "UNTIL_YOUR_NEXT_TURN",
        "UNTIL_END_OF_COMBAT",
        "UNTIL",
        "FOR_AS_LONG_AS",
        "WHILE_SOURCE_ON_BATTLEFIELD",
        "WHILE_CONTROLLED_BY",
        "PERMANENT",
        "PERPETUAL",
    ];
    assert_eq!(DurationAST::KINDS, kinds);
    for (doc, k) in duration_docs().iter().zip(kinds) {
        let d: DurationAST = accept(doc);
        assert_eq!(d.kind(), k);
    }
}

/// REQ-DSL-010 (requirement 5, brief D): Rust shapes of DurationAST encode to the wire forms.
/// FOR_AS_LONG_AS is the 611.2b duration.
#[test]
fn req_dsl_010_duration_rust_shapes() {
    let whose: PlayerSelectorAST = from(json!({"ref":"CONTROLLER"}));
    let step: StepId = from(json!("END"));
    let predicate: PredicateAST = from(json!({"pred":"IS_CREATURE"}));
    let values = vec![
        DurationAST::UntilEndOfTurn,
        DurationAST::UntilYourNextTurn,
        DurationAST::UntilEndOfCombat,
        DurationAST::Until {
            step,
            whose: whose.clone(),
        },
        DurationAST::ForAsLongAs { predicate },
        DurationAST::WhileSourceOnBattlefield,
        DurationAST::WhileControlledBy { player: whose },
        DurationAST::Permanent,
        DurationAST::Perpetual,
    ];
    for (v, doc) in values.iter().zip(duration_docs()) {
        assert_eq!(serde_json::to_value(v).unwrap(), doc);
        assert_eq!(from::<DurationAST>(doc), *v);
    }
}

/// REQ-DSL-010 (requirement 5): every duration kind embedded in EXILE_UNTIL,
/// PLAY_PERMISSION_GRANT, PREVENT, GAIN_CONTROL and CREATE_CONTINUOUS_EFFECT decodes and
/// round-trips; an invalid duration there is rejected.
#[test]
fn req_dsl_010_duration_embedded_in_ops() {
    let hosts = [
        json!({"op":"EXILE_UNTIL","objects":{"ref":"SELF"}}),
        json!({"op":"PLAY_PERMISSION_GRANT","objects":{"ref":"SELF"},"who":{"ref":"CONTROLLER"}}),
        json!({"op":"PREVENT"}),
        json!({"op":"GAIN_CONTROL","objects":{"ref":"SELF"}}),
        json!({"op":"CREATE_CONTINUOUS_EFFECT","effects":[{"type":"RULE","kind":"SKIP"}]}),
    ];
    for h in &hosts {
        let mut seen = Vec::new();
        for d in duration_docs() {
            let i: Instruction = accept(&with(h, "duration", d));
            assert!(
                !seen.contains(&i),
                "distinct durations give distinct values"
            );
            seen.push(i);
        }
        reject::<Instruction>(h);
        for bad in [
            json!({"kind":"until_end_of_turn"}),
            json!({"kind":"FOREVER"}),
            json!({"kind":"UNTIL","step":"END"}),
            json!({}),
            json!("UNTIL_END_OF_TURN"),
        ] {
            reject::<Instruction>(&with(h, "duration", bad));
        }
    }
}

/// REQ-DSL-010 (requirement 5, brief D): missing and extra members, unknown or lower-case kind,
/// unknown step, and UNTIL without step or whose are rejected.
#[test]
fn req_dsl_010_duration_rejections() {
    let bads = [
        json!({}),
        json!({"kind":null}),
        json!({"kind":7}),
        json!({"kind":"FOREVER"}),
        json!({"kind":"until_end_of_turn"}),
        json!({"kind":"Permanent"}),
        json!({"kind":"UNTIL"}),
        json!({"kind":"UNTIL","step":"END"}),
        json!({"kind":"UNTIL","whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"UNTIL","step":"SOMETIME","whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"UNTIL","step":"end","whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"UNTIL","step":"END","whose":{"ref":"CONTROLLER"},"player":{"ref":"CONTROLLER"}}),
        json!({"kind":"UNTIL","step":null,"whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"FOR_AS_LONG_AS"}),
        json!({"kind":"FOR_AS_LONG_AS","predicate":null}),
        json!({"kind":"FOR_AS_LONG_AS","predicate":{"pred":"IS_CREATURE"},"step":"END"}),
        json!({"kind":"WHILE_CONTROLLED_BY"}),
        json!({"kind":"WHILE_CONTROLLED_BY","player":7}),
        json!({"kind":"UNTIL_END_OF_TURN","player":{"ref":"CONTROLLER"}}),
        json!({"kind":"UNTIL_END_OF_TURN","zzUnknown":1}),
        json!({"kind":"PERMANENT","predicate":{"pred":"IS_CREATURE"}}),
        json!({"kind":"PERPETUAL","step":"END"}),
        json!({"kind":"WHILE_SOURCE_ON_BATTLEFIELD","whose":{"ref":"CONTROLLER"}}),
        json!("UNTIL_END_OF_TURN"),
        json!(["UNTIL_END_OF_TURN"]),
        Value::Null,
    ];
    for b in bads {
        reject::<DurationAST>(&b);
    }
    reject_text::<DurationAST>(r#"{"kind":"PERMANENT","kind":"PERMANENT"}"#);
    // Every step is accepted in UNTIL.
    for step in [
        "UNTIL",
        "UPKEEP",
        "DRAW",
        "PRECOMBAT_MAIN",
        "BEGINNING_OF_COMBAT",
        "DECLARE_ATTACKERS",
        "DECLARE_BLOCKERS",
        "FIRST_STRIKE_DAMAGE",
        "COMBAT_DAMAGE",
        "END_OF_COMBAT",
        "POSTCOMBAT_MAIN",
        "END",
        "CLEANUP",
    ] {
        let doc = json!({"kind":"UNTIL","step":step,"whose":{"players":"ALL_PLAYERS"}});
        if step == "UNTIL" {
            reject::<DurationAST>(&doc);
        } else {
            accept::<DurationAST>(&doc);
        }
    }
}

// ---------------------------------------------------------------- small enums (A8)

/// REQ-DSL-010 (brief A8): Position, TapAction and MemoryStore list their wire names in order
/// and serialise as those strings; other spellings are rejected.
#[test]
fn req_dsl_010_small_enums() {
    let pos: Vec<&str> = Position::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(pos, ["TOP", "BOTTOM"]);
    for p in Position::ALL.iter().cloned() {
        assert_eq!(serde_json::to_value(p).unwrap(), json!(p.as_str()));
        assert_eq!(from::<Position>(json!(p.as_str())), p);
    }
    let tap: Vec<&str> = TapAction::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(tap, ["TAP", "UNTAP"]);
    for p in TapAction::ALL.iter().cloned() {
        assert_eq!(serde_json::to_value(p).unwrap(), json!(p.as_str()));
        assert_eq!(from::<TapAction>(json!(p.as_str())), p);
    }
    let mem: Vec<&str> = MemoryStore::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(mem, ["MEMORY", "LINKED"]);
    for p in MemoryStore::ALL.iter().cloned() {
        assert_eq!(serde_json::to_value(p).unwrap(), json!(p.as_str()));
        assert_eq!(from::<MemoryStore>(json!(p.as_str())), p);
    }
    for bad in ["top", "Top", "MIDDLE", ""] {
        reject::<Position>(&json!(bad));
    }
    for bad in ["tap", "FLIP", ""] {
        reject::<TapAction>(&json!(bad));
    }
    for bad in ["memory", "Linked", "STORE", ""] {
        reject::<MemoryStore>(&json!(bad));
    }
    let base = json!({"op":"MOVE","objects":{"ref":"SELF"},"toZone":"LIBRARY"});
    for p in ["TOP", "BOTTOM"] {
        accept::<Instruction>(&with(&base, "position", json!(p)));
    }
    for p in ["top", "MIDDLE"] {
        reject::<Instruction>(&with(&base, "position", json!(p)));
    }
    let base = json!({"op":"TAP_OR_UNTAP","objects":{"ref":"SELF"}});
    for a in ["TAP", "UNTAP"] {
        accept::<Instruction>(&with(&base, "action", json!(a)));
    }
    for a in ["untap", "TOGGLE"] {
        reject::<Instruction>(&with(&base, "action", json!(a)));
    }
}

// ---------------------------------------------------------------- 11. Serialize re-checks

fn objs() -> ObjectSelectorAST {
    from(json!({"ref":"SELF"}))
}

fn plr() -> PlayerSelectorAST {
    from(json!({"ref":"CONTROLLER"}))
}

fn val(n: i64) -> ValueExpressionAST {
    from(json!({ "LITERAL": n }))
}

fn pred() -> PredicateAST {
    from(json!({"pred":"IS_CREATURE"}))
}

fn draw() -> Instruction {
    Instruction::Draw {
        player: plr(),
        count: val(1),
    }
}

fn mode(id: &str) -> InstructionMode {
    InstructionMode {
        id: id.into(),
        targets: None,
        effect: vec![draw()],
    }
}

/// REQ-DSL-010 (requirement 11, brief A2): hand-built values that would not decode fail to
/// serialise instead of writing bad JSON.
#[test]
fn req_dsl_010_invalid_values_fail_to_serialise() {
    let rule: StaticEffect = from(json!({"type":"RULE","kind":"SKIP"}));
    let bads = vec![
        Instruction::PutCounters {
            objects: objs(),
            kind: String::new(),
            count: val(1),
        },
        Instruction::If {
            cond: pred(),
            then: vec![draw()],
            otherwise: Some(vec![]),
        },
        Instruction::If {
            cond: pred(),
            then: vec![],
            otherwise: None,
        },
        Instruction::May {
            player: plr(),
            body: vec![],
        },
        Instruction::RepeatWhile {
            cond: pred(),
            max_iter: NonZeroU32::new(5).unwrap(),
            body: vec![],
        },
        Instruction::CreateContinuousEffect {
            effects: vec![],
            duration: DurationAST::UntilEndOfTurn,
            lock_affected_set: None,
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 2,
            max: 1,
            modes: vec![mode("m1"), mode("m2")],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 0,
            max: 2,
            modes: vec![mode("m1")],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 1,
            max: 1,
            modes: vec![],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 1,
            max: 1,
            modes: vec![mode("m1"), mode("m1")],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 1,
            max: 1,
            modes: vec![mode("")],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 1,
            max: 1,
            modes: vec![InstructionMode {
                id: "m1".into(),
                targets: None,
                effect: vec![],
            }],
        },
        Instruction::ChooseMode {
            chooser: plr(),
            min: 1,
            max: 1,
            modes: vec![InstructionMode {
                id: "m1".into(),
                targets: Some(vec![]),
                effect: vec![draw()],
            }],
        },
        // Nested invalid value deep inside a valid one.
        Instruction::May {
            player: plr(),
            body: vec![Instruction::If {
                cond: pred(),
                then: vec![Instruction::PutCounters {
                    objects: objs(),
                    kind: String::new(),
                    count: val(1),
                }],
                otherwise: None,
            }],
        },
    ];
    for b in &bads {
        fails_to_serialise(b);
    }
    // The valid counterparts serialise to the wire forms.
    let goods: Vec<(Instruction, Value)> = vec![
        (
            Instruction::PutCounters {
                objects: objs(),
                kind: "test_counter".into(),
                count: val(2),
            },
            json!({"op":"PUT_COUNTERS","objects":{"ref":"SELF"},"kind":"test_counter","count":{"LITERAL":2}}),
        ),
        (
            Instruction::If {
                cond: pred(),
                then: vec![draw()],
                otherwise: None,
            },
            json!({"op":"IF","cond":{"pred":"IS_CREATURE"},
                   "then":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}),
        ),
        (
            Instruction::If {
                cond: pred(),
                then: vec![draw()],
                otherwise: Some(vec![Instruction::Populate]),
            },
            json!({"op":"IF","cond":{"pred":"IS_CREATURE"},
                   "then":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}],
                   "else":[{"op":"POPULATE"}]}),
        ),
        (
            Instruction::May {
                player: plr(),
                body: vec![Instruction::Populate],
            },
            json!({"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"POPULATE"}]}),
        ),
        (
            Instruction::CreateContinuousEffect {
                effects: vec![rule.clone()],
                duration: DurationAST::UntilEndOfTurn,
                lock_affected_set: Some(false),
            },
            json!({"op":"CREATE_CONTINUOUS_EFFECT","effects":[{"type":"RULE","kind":"SKIP"}],
                   "duration":{"kind":"UNTIL_END_OF_TURN"},"lockAffectedSet":false}),
        ),
        (
            Instruction::ChooseMode {
                chooser: plr(),
                min: 1,
                max: 2,
                modes: vec![mode("m1"), mode("m2")],
            },
            json!({"op":"CHOOSE_MODE","chooser":{"ref":"CONTROLLER"},"min":1,"max":2,"modes":[
                {"id":"m1","effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]},
                {"id":"m2","effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}]}),
        ),
    ];
    for (g, expect) in goods {
        assert_eq!(serde_json::to_value(&g).unwrap(), expect);
        assert_eq!(from::<Instruction>(expect), g);
    }
}

/// REQ-DSL-010 (requirement 7, brief C 60; 611.2c): `lockAffectedSet` absent, true and false are
/// three distinct round-tripping values, `None`, `Some(true)` and `Some(false)`.
#[test]
fn req_dsl_010_lock_affected_set_three_values() {
    let base = json!({"op":"CREATE_CONTINUOUS_EFFECT","effects":[{"type":"RULE","kind":"SKIP"}],
                      "duration":{"kind":"PERMANENT"}});
    let mut got = Vec::new();
    for (lock, expect) in [
        (None, None),
        (Some(true), Some(true)),
        (Some(false), Some(false)),
    ] {
        let doc = match lock {
            None => base.clone(),
            Some(b) => with(&base, "lockAffectedSet", json!(b)),
        };
        let i: Instruction = accept(&doc);
        let Instruction::CreateContinuousEffect {
            lock_affected_set, ..
        } = &i
        else {
            panic!("expected CreateContinuousEffect, got {i:?}");
        };
        assert_eq!(*lock_affected_set, expect);
        assert_eq!(serde_json::to_value(&i).unwrap(), doc);
        got.push(i);
    }
    assert_ne!(got[0], got[1]);
    assert_ne!(got[0], got[2]);
    assert_ne!(got[1], got[2]);
    reject::<Instruction>(&with(&base, "lockAffectedSet", Value::Null));
    reject::<Instruction>(&with(&base, "lockAffectedSet", json!("true")));
}

/// REQ-DSL-010 (brief A1): optional members are omitted when absent — the encoder never writes
/// `null` or an empty optional list.
#[test]
fn req_dsl_010_absent_optionals_are_omitted() {
    let i = Instruction::If {
        cond: pred(),
        then: vec![draw()],
        otherwise: None,
    };
    let v = serde_json::to_value(&i).unwrap();
    let m: &Map<String, Value> = v.as_object().unwrap();
    assert!(!m.contains_key("else"));
    assert!(!m.contains_key("otherwise"));
    let n = serde_json::to_value(NativeEffectAST::new("native.x", 1)).unwrap();
    assert!(!n.as_object().unwrap().contains_key("layers"));
    assert!(!n.as_object().unwrap().contains_key("ruleKinds"));
}
