// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the effect-instruction schema (REQ-DSL-010, REQ-DSL-011, REQ-DSL-015,
//! REQ-CORE-006, REQ-NFR-006, REQ-NFR-008). Every id, kind and word is synthetic (R5). The
//! seeded property tests are in the `props` submodule.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::*;
use crate::ability::{CostAST, EventSubject, Layer, RuleKind, StaticEffect, Trigger};
use crate::canonical::{MAX_DEPTH, to_canonical_vec};
use crate::choice::{ChoiceKind, ChoiceSpec};
use crate::expr::tests::{accept, check_wire_enum, reject};
use crate::expr::{
    ObjectSelectorAST, PlayerGroup, PlayerSelectorAST, PredicateAST, ReferenceAST,
    ValueExpressionAST, Zone,
};

mod props;

// ---------------------------------------------------------------------------------------------
// Fragments and helpers

/// PLR fragment.
fn plr() -> Value {
    json!({"ref":"CONTROLLER"})
}

/// OBJ fragment.
fn obj() -> Value {
    json!({"ref":"SELF"})
}

/// VAL fragment.
fn val() -> Value {
    json!({"LITERAL":3})
}

/// PRED fragment.
fn pred() -> Value {
    json!({"pred":"IS_CREATURE"})
}

/// CH fragment.
fn ch() -> Value {
    json!({"chooser":{"ref":"CONTROLLER"},"kind":"OBJECTS"})
}

/// DUR fragment.
fn dur() -> Value {
    json!({"kind":"UNTIL_END_OF_TURN"})
}

/// SUBJ fragment.
fn subj() -> Value {
    json!({"players":"ALL_PLAYERS"})
}

/// COSTS fragment.
fn costs() -> Value {
    json!([{"cost":"TAP_SOURCE"}])
}

/// INSTRS fragment.
fn instrs() -> Value {
    json!([{"op":"POPULATE"}])
}

/// One catalogue entry: the op, its required members (with a valid value) and its optional
/// members (with a valid value).
struct Case {
    /// The `op`.
    op: &'static str,
    /// Required members.
    req: Vec<(&'static str, Value)>,
    /// Optional members.
    opt: Vec<(&'static str, Value)>,
}

impl Case {
    /// The minimal document: `op` plus every required member.
    fn min(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("op".to_owned(), json!(self.op));
        for (k, v) in &self.req {
            m.insert((*k).to_owned(), v.clone());
        }
        m
    }

    /// The full document: the minimal one plus every optional member.
    fn full(&self) -> Map<String, Value> {
        let mut m = self.min();
        for (k, v) in &self.opt {
            m.insert((*k).to_owned(), v.clone());
        }
        m
    }
}

/// Shorthand for building a `Case`.
fn case(
    op: &'static str,
    req: Vec<(&'static str, Value)>,
    opt: Vec<(&'static str, Value)>,
) -> Case {
    Case { op, req, opt }
}

/// The whole catalogue, in `Instruction::NAMES` order (section C).
#[allow(clippy::too_many_lines)]
fn catalogue() -> Vec<Case> {
    vec![
        case("DRAW", vec![("player", plr()), ("count", val())], vec![]),
        case("MILL", vec![("player", plr()), ("count", val())], vec![]),
        case(
            "DISCARD",
            vec![("player", plr()), ("count", val())],
            vec![("choice", ch())],
        ),
        case(
            "MOVE",
            vec![("objects", obj()), ("toZone", json!("GRAVEYARD"))],
            vec![
                ("position", json!("BOTTOM")),
                ("faceDown", json!(true)),
                ("tapped", json!(true)),
                ("underControlOf", plr()),
                ("transformed", json!(true)),
                ("choice", ch()),
            ],
        ),
        case(
            "SEARCH_LIBRARY",
            vec![
                ("player", plr()),
                ("selector", json!({"zones":["LIBRARY"],"pred":"IS_LAND"})),
                ("count", val()),
                ("toZone", json!("HAND")),
            ],
            vec![("position", json!("TOP")), ("choice", ch())],
        ),
        case("SHUFFLE", vec![("player", plr())], vec![]),
        case(
            "REVEAL",
            vec![("objects", obj())],
            vec![("to", plr()), ("choice", ch())],
        ),
        case(
            "LOOK_AT",
            vec![("objects", obj()), ("player", plr())],
            vec![],
        ),
        case(
            "PUT_ON_TOP_OR_BOTTOM",
            vec![("objects", obj()), ("player", plr())],
            vec![("choice", ch())],
        ),
        case(
            "EXILE_UNTIL",
            vec![("objects", obj()), ("duration", dur())],
            vec![("faceDown", json!(true))],
        ),
        case(
            "PLAY_PERMISSION_GRANT",
            vec![("objects", obj()), ("duration", dur()), ("who", plr())],
            vec![("alsoAs", json!("test.as"))],
        ),
        case(
            "DEAL_DAMAGE",
            vec![("source", obj()), ("recipients", subj()), ("amount", val())],
            vec![("divided", json!(true)), ("choice", ch())],
        ),
        case("FIGHT", vec![("first", obj()), ("second", obj())], vec![]),
        case("BITE", vec![("source", obj()), ("target", obj())], vec![]),
        case(
            "GAIN_LIFE",
            vec![("player", plr()), ("amount", val())],
            vec![],
        ),
        case(
            "LOSE_LIFE",
            vec![("player", plr()), ("amount", val())],
            vec![],
        ),
        case(
            "SET_LIFE",
            vec![("player", plr()), ("amount", val())],
            vec![],
        ),
        case(
            "PREVENT",
            vec![("duration", dur())],
            vec![
                ("source", obj()),
                ("recipients", obj()),
                ("amount", val()),
                ("combatOnly", json!(true)),
            ],
        ),
        case(
            "DESTROY",
            vec![("objects", obj())],
            vec![("noRegeneration", json!(true))],
        ),
        case(
            "SACRIFICE",
            vec![("player", plr()), ("selector", obj()), ("count", val())],
            vec![("choice", ch())],
        ),
        case("REGENERATE", vec![("objects", obj())], vec![]),
        case(
            "COUNTER_SPELL",
            vec![("objects", obj())],
            vec![("unless", costs())],
        ),
        case(
            "PUT_COUNTERS",
            vec![
                ("objects", obj()),
                ("kind", json!("test.counter")),
                ("count", val()),
            ],
            vec![],
        ),
        case(
            "REMOVE_COUNTERS",
            vec![
                ("objects", obj()),
                ("kind", json!("test.counter")),
                ("count", val()),
            ],
            vec![],
        ),
        case(
            "MOVE_COUNTERS",
            vec![
                ("from", obj()),
                ("to", json!({"ref":"TARGET","slot":0})),
                ("kind", json!("test.counter")),
                ("count", val()),
            ],
            vec![],
        ),
        case("PROLIFERATE", vec![], vec![("choice", ch())]),
        case(
            "CREATE_TOKEN",
            vec![("tokenDef", json!("test.token_1_1")), ("count", val())],
            vec![("modifiers", json!({"m":[1,"x"]})), ("controller", plr())],
        ),
        case(
            "CREATE_PREDEFINED_TOKEN",
            vec![("kind", json!("test.predefined"))],
            vec![("count", val())],
        ),
        case(
            "CREATE_COPY_TOKEN",
            vec![("of", obj())],
            vec![("except", json!({"": true}))],
        ),
        case("POPULATE", vec![], vec![]),
        case("AMASS", vec![("n", val())], vec![]),
        case("INCUBATE", vec![("n", val())], vec![]),
        case(
            "COPY_SPELL_OR_ABILITY",
            vec![("objects", obj())],
            vec![("newTargets", json!(true))],
        ),
        case(
            "CHANGE_TARGETS",
            vec![("objects", obj())],
            vec![("choice", ch())],
        ),
        case(
            "CAST_FROM",
            vec![("objects", obj())],
            vec![("withoutPaying", json!(true)), ("altCost", costs())],
        ),
        case(
            "TAP_OR_UNTAP",
            vec![("objects", obj()), ("action", json!("UNTAP"))],
            vec![],
        ),
        case("TRANSFORM", vec![("objects", obj())], vec![]),
        case("CONVERT", vec![("objects", obj())], vec![]),
        case(
            "TURN_FACE_UP",
            vec![("objects", obj())],
            vec![("profile", json!("test.profile"))],
        ),
        case(
            "TURN_FACE_DOWN",
            vec![("objects", obj())],
            vec![("profile", json!("test.profile"))],
        ),
        case("PHASE_OUT", vec![("objects", obj())], vec![]),
        case("ATTACH", vec![("objects", obj()), ("to", obj())], vec![]),
        case(
            "GAIN_CONTROL",
            vec![("objects", obj()), ("duration", dur())],
            vec![("player", plr())],
        ),
        case(
            "EXCHANGE_CONTROL",
            vec![("first", obj()), ("second", obj())],
            vec![],
        ),
        case("GOAD", vec![("objects", obj())], vec![]),
        case(
            "SET_DESIGNATION",
            vec![("kind", json!("test.designation")), ("value", json!(true))],
            vec![("to", subj())],
        ),
        case("VENTURE", vec![], vec![]),
        case("RING_TEMPTS", vec![], vec![]),
        case(
            "INCREASE_SPEED",
            vec![("player", plr())],
            vec![("amount", val())],
        ),
        case("EMPOWER_JACE", vec![("n", val())], vec![]),
        case("MELD", vec![("objects", obj())], vec![]),
        case(
            "MUTATE_MERGE",
            vec![("objects", obj()), ("onto", obj())],
            vec![],
        ),
        case("MANIFEST", vec![("objects", obj())], vec![]),
        case("CLOAK", vec![("objects", obj())], vec![]),
        case("EXPLORE", vec![("objects", obj())], vec![]),
        case("CONNIVE", vec![("objects", obj())], vec![]),
        case("DISCOVER", vec![("n", val())], vec![]),
        case("BLIGHT", vec![("n", val())], vec![]),
        case("WATERBEND", vec![("cost", costs())], vec![]),
        case(
            "CREATE_CONTINUOUS_EFFECT",
            vec![
                ("effects", json!([{"type":"RULE","kind":"SKIP"}])),
                ("duration", dur()),
            ],
            vec![("lockAffectedSet", json!(true))],
        ),
        case(
            "CREATE_DELAYED_TRIGGER",
            vec![
                (
                    "trigger",
                    json!({"mode":"PHASE_STEP","step":"END","whose":{"ref":"CONTROLLER"}}),
                ),
                ("effect", instrs()),
                ("oneShot", json!(true)),
            ],
            vec![("captures", json!({"$it": {"ref":"SELF"}}))],
        ),
        case(
            "REFLEXIVE_TRIGGER",
            vec![("ifActionTaken", instrs()), ("effect", instrs())],
            vec![],
        ),
        case(
            "IF",
            vec![("cond", pred()), ("then", instrs())],
            vec![("else", instrs())],
        ),
        case(
            "FOR_EACH",
            vec![("iterator", obj()), ("body", instrs())],
            vec![("bind", json!("$each_1"))],
        ),
        case("REPEAT", vec![("times", val()), ("body", instrs())], vec![]),
        case(
            "REPEAT_WHILE",
            vec![("cond", pred()), ("maxIter", json!(10)), ("body", instrs())],
            vec![],
        ),
        case("MAY", vec![("player", plr()), ("body", instrs())], vec![]),
        case(
            "UNLESS",
            vec![("payer", plr()), ("cost", costs()), ("body", instrs())],
            vec![],
        ),
        case(
            "CHOOSE_MODE",
            vec![
                ("chooser", plr()),
                ("min", json!(1)),
                ("max", json!(1)),
                ("modes", json!([{"id":"m1","effect":[{"op":"POPULATE"}]}])),
            ],
            vec![],
        ),
        case(
            "VOTE",
            vec![
                ("voters", json!({"players":"ALL_PLAYERS"})),
                ("choice", ch()),
                ("store", json!("test.votes")),
            ],
            vec![],
        ),
        case(
            "REMEMBER",
            vec![
                ("key", json!("test.key")),
                ("into", json!("MEMORY")),
                ("value", val()),
            ],
            vec![],
        ),
        case(
            "NATIVE_EFFECT",
            vec![
                ("id", json!("test.native")),
                ("version", json!(1)),
                ("params", json!({})),
            ],
            vec![
                ("layers", json!(["7a", "6"])),
                ("ruleKinds", json!(["SKIP"])),
            ],
        ),
    ]
}

/// The JSON text of a map.
fn text(m: &Map<String, Value>) -> String {
    Value::Object(m.clone()).to_string()
}

/// Decodes `doc` as an `Instruction`, asserting canonical round trip.
fn instr(doc: &str) -> Instruction {
    accept::<Instruction>(doc)
}

/// `{"op":op, …members}` as text.
fn op_doc(op: &str, members: &Value) -> String {
    let mut m = members.as_object().cloned().unwrap_or_default();
    m.insert("op".to_owned(), json!(op));
    Value::Object(m).to_string()
}

/// Asserts the hand-built value fails to serialize.
fn no_encode<T: serde::Serialize + std::fmt::Debug>(v: &T) {
    assert!(serde_json::to_vec(v).is_err(), "{v:?} was written");
}

/// A self reference selector.
fn self_sel() -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::SelfRef)
}

/// A controller player selector.
fn ctrl() -> PlayerSelectorAST {
    PlayerSelectorAST::Ref(ReferenceAST::Controller)
}

// ---------------------------------------------------------------------------------------------
// Catalogue

/// REQ-DSL-010: `NAMES` lists 72 distinct ops in catalogue order, and the test catalogue covers
/// exactly those ops.
#[test]
fn req_dsl_010_names_catalogue_order() {
    assert_eq!(Instruction::NAMES.len(), 72);
    for (i, n) in Instruction::NAMES.iter().enumerate() {
        assert!(!Instruction::NAMES[..i].contains(n), "{n} twice");
    }
    let ops: Vec<&str> = catalogue().iter().map(|c| c.op).collect();
    assert_eq!(ops, Instruction::NAMES.to_vec());
}

/// REQ-DSL-010, REQ-DSL-015: every op's minimal document decodes, names itself and re-encodes
/// canonically; the full document (every optional member) round-trips too.
#[test]
fn req_dsl_010_every_op_minimal_and_full() {
    for c in catalogue() {
        let min = instr(&text(&c.min()));
        assert_eq!(min.name(), c.op);
        let full = instr(&text(&c.full()));
        assert_eq!(full.name(), c.op);
        if !c.opt.is_empty() {
            assert_ne!(min, full, "{}", c.op);
        }
    }
}

/// REQ-DSL-010: dropping any required member (or `op`) is rejected.
#[test]
fn req_dsl_010_every_required_member_dropped_rejected() {
    for c in catalogue() {
        for key in c.req.iter().map(|(k, _)| *k).chain(["op"]) {
            let mut m = c.min();
            m.remove(key);
            reject::<Instruction>(&text(&m));
        }
    }
}

/// REQ-DSL-010: an unknown member is rejected, on the minimal and the full document.
#[test]
fn req_dsl_010_unknown_member_rejected() {
    for c in catalogue() {
        for mut m in [c.min(), c.full()] {
            m.insert("zzUnknown".to_owned(), json!(1));
            reject::<Instruction>(&text(&m));
        }
    }
}

/// REQ-DSL-010: a nulled optional member is rejected; a nulled required member too.
#[test]
fn req_dsl_010_null_members_rejected() {
    for c in catalogue() {
        for (k, _) in c.opt.iter().chain(c.req.iter()) {
            let mut m = c.full();
            m.insert((*k).to_owned(), Value::Null);
            reject::<Instruction>(&text(&m));
        }
    }
}

/// REQ-DSL-010: an explicit `false` flag is kept and is distinct from an absent one.
#[test]
fn req_dsl_010_false_flags_distinct_from_absent() {
    let mut seen = 0;
    for c in catalogue() {
        for (k, v) in &c.opt {
            if !v.is_boolean() {
                continue;
            }
            seen += 1;
            let absent = instr(&text(&c.min()));
            let mut m = c.min();
            m.insert((*k).to_owned(), json!(false));
            let with_false = instr(&text(&m));
            m.insert((*k).to_owned(), json!(true));
            let with_true = instr(&text(&m));
            assert_ne!(absent, with_false, "{} {k}", c.op);
            assert_ne!(with_true, with_false, "{} {k}", c.op);
            assert_ne!(absent, with_true, "{} {k}", c.op);
        }
    }
    assert_eq!(seen, 10);
}

/// REQ-DSL-010 (rule A6): `op` must be one of the 72 names exactly.
#[test]
fn req_dsl_010_op_rules() {
    reject::<Instruction>(r#"{"op":"populate"}"#);
    reject::<Instruction>(r#"{"op":"Populate"}"#);
    reject::<Instruction>(r#"{"op":"NOT_AN_OP"}"#);
    reject::<Instruction>(r#"{"op":""}"#);
    reject::<Instruction>(r#"{"op":1}"#);
    reject::<Instruction>(r#"{"op":null}"#);
    reject::<Instruction>("{}");
    reject::<Instruction>(r#"["POPULATE"]"#);
    reject::<Instruction>(r#""POPULATE""#);
    assert_eq!(instr(r#"{"op":"POPULATE"}"#), Instruction::Populate);
    assert_eq!(instr(r#"{"op":"VENTURE"}"#), Instruction::Venture);
    assert_eq!(instr(r#"{"op":"RING_TEMPTS"}"#), Instruction::RingTempts);
}

/// REQ-DSL-010: the struct-variant field names and typed values of a few ops.
#[test]
fn req_dsl_010_typed_fields() {
    let m = instr(
        r#"{"op":"MOVE","objects":{"ref":"SELF"},"toZone":"LIBRARY","position":"TOP","faceDown":false,"underControlOf":{"ref":"OWNER"}}"#,
    );
    assert_eq!(
        m,
        Instruction::Move {
            objects: self_sel(),
            to_zone: Zone::Library,
            position: Some(Position::Top),
            face_down: Some(false),
            tapped: None,
            under_control_of: Some(PlayerSelectorAST::Ref(ReferenceAST::Owner)),
            transformed: None,
            choice: None,
        }
    );
    let d = instr(
        r#"{"op":"DEAL_DAMAGE","source":{"ref":"SELF"},"recipients":{"ref":"TARGET","slot":0},"amount":{"LITERAL":2},"choice":{"chooser":{"ref":"CONTROLLER"},"kind":"DIVISION"}}"#,
    );
    assert_eq!(
        d,
        Instruction::DealDamage {
            source: self_sel(),
            recipients: EventSubject::Object(ObjectSelectorAST::Ref(ReferenceAST::Target {
                slot: 0
            })),
            amount: ValueExpressionAST::Literal(2),
            divided: None,
            choice: Some(ChoiceSpec::new(ctrl(), ChoiceKind::Division)),
        }
    );
    let t = instr(r#"{"op":"TAP_OR_UNTAP","objects":{"ref":"SELF"},"action":"TAP"}"#);
    assert_eq!(
        t,
        Instruction::TapOrUntap {
            objects: self_sel(),
            action: TapAction::Tap
        }
    );
    reject::<Instruction>(r#"{"op":"TAP_OR_UNTAP","objects":{"ref":"SELF"},"action":"tap"}"#);
    reject::<Instruction>(
        r#"{"op":"MOVE","objects":{"ref":"SELF"},"toZone":"LIBRARY","position":"MIDDLE"}"#,
    );
    reject::<Instruction>(r#"{"op":"MOVE","objects":{"ref":"SELF"},"toZone":"NOWHERE"}"#);
    reject::<Instruction>(
        r#"{"op":"MOVE","objects":{"ref":"SELF"},"toZone":"HAND","tapped":"yes"}"#,
    );
}

/// REQ-DSL-010 (rule A8): the three small string enums.
#[test]
fn req_dsl_010_small_enums() {
    check_wire_enum(Position::ALL, Position::as_str, Position::from_wire);
    check_wire_enum(TapAction::ALL, TapAction::as_str, TapAction::from_wire);
    check_wire_enum(
        MemoryStore::ALL,
        MemoryStore::as_str,
        MemoryStore::from_wire,
    );
    assert_eq!(Position::ALL.len(), 2);
    assert_eq!(TapAction::ALL.len(), 2);
    assert_eq!(MemoryStore::ALL.len(), 2);
}

/// REQ-DSL-010 (rule A3): STR members must be non-empty strings.
#[test]
fn req_dsl_010_str_members() {
    for (op, key) in [
        ("PLAY_PERMISSION_GRANT", "alsoAs"),
        ("PUT_COUNTERS", "kind"),
        ("REMOVE_COUNTERS", "kind"),
        ("MOVE_COUNTERS", "kind"),
        ("CREATE_TOKEN", "tokenDef"),
        ("CREATE_PREDEFINED_TOKEN", "kind"),
        ("TURN_FACE_UP", "profile"),
        ("TURN_FACE_DOWN", "profile"),
        ("SET_DESIGNATION", "kind"),
        ("VOTE", "store"),
        ("REMEMBER", "key"),
        ("NATIVE_EFFECT", "id"),
    ] {
        let c = catalogue().into_iter().find(|c| c.op == op).expect("op");
        for bad in [json!(""), json!(1), json!(["x"])] {
            let mut m = c.full();
            m.insert(key.to_owned(), bad);
            reject::<Instruction>(&text(&m));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// REPEAT_WHILE

/// REQ-DSL-010 (SRS §14.4.5 loop safety): `maxIter` is required and 1..=4294967295.
#[test]
fn req_dsl_010_repeat_while_max_iter() {
    let doc = |m: &str| {
        format!(
            r#"{{"op":"REPEAT_WHILE","cond":{{"pred":"IS_CREATURE"}},"body":[{{"op":"POPULATE"}}]{m}}}"#
        )
    };
    reject::<Instruction>(&doc(""));
    for bad in [
        "0",
        "-1",
        "4294967296",
        "1.0",
        "1e2",
        "-0",
        "\"5\"",
        "null",
        "true",
        "[1]",
        "9007199254740992",
    ] {
        reject::<Instruction>(&doc(&format!(r#","maxIter":{bad}"#)));
    }
    for (good, n) in [("1", 1_u32), ("4294967295", u32::MAX), ("10", 10)] {
        match instr(&doc(&format!(r#","maxIter":{good}"#))) {
            Instruction::RepeatWhile { max_iter, .. } => assert_eq!(max_iter.get(), n),
            other => panic!("{other:?}"),
        }
    }
    let hand = Instruction::RepeatWhile {
        cond: PredicateAST::IsCreature,
        max_iter: NonZeroU32::new(7).expect("non-zero"),
        body: vec![Instruction::Populate],
    };
    let v: Value = serde_json::to_value(&hand).expect("encode");
    assert_eq!(v["maxIter"], json!(7));
    assert_eq!(
        String::from_utf8(to_canonical_vec(&hand).expect("encode")).expect("utf8"),
        r#"{"body":[{"op":"POPULATE"}],"cond":{"pred":"IS_CREATURE"},"maxIter":7,"op":"REPEAT_WHILE"}"#
    );
}

// ---------------------------------------------------------------------------------------------
// NATIVE_EFFECT

/// REQ-DSL-015: the SRS §14.8 example parses verbatim, round-trips and keys the registry.
#[test]
fn req_dsl_015_srs_example() {
    let doc = r#"{"op":"NATIVE_EFFECT","id":"native.subgame.start","version":2,"params":{}}"#;
    let i = instr(doc);
    let n = accept::<NativeEffectAST>(doc);
    assert_eq!(i, Instruction::NativeEffect(n.clone()));
    assert_eq!(n, NativeEffectAST::new("native.subgame.start", 2));
    assert_eq!(n.registry_key(), "native.subgame.start@2");
    assert_eq!(i.name(), "NATIVE_EFFECT");
    assert_eq!(
        serde_json::to_value(&n).expect("encode"),
        serde_json::to_value(&i).expect("encode")
    );
}

/// REQ-DSL-015: `id`, `version`, `params`, `layers` and `ruleKinds` rules.
#[test]
fn req_dsl_015_native_member_rules() {
    let doc = |m: &str| format!(r#"{{"op":"NATIVE_EFFECT",{m}}}"#);
    let base = r#""id":"test.n","version":1,"params":{}"#;
    let n = accept::<NativeEffectAST>(&doc(base));
    assert_eq!(n.layers, None);
    assert_eq!(n.rule_kinds, None);
    // id
    for bad in [r#""""#, r#""a@b""#, r#""@""#, "1", "null"] {
        reject::<NativeEffectAST>(&doc(&format!(r#""id":{bad},"version":1,"params":{{}}"#)));
    }
    reject::<NativeEffectAST>(&doc(r#""version":1,"params":{}"#));
    // version
    for bad in ["0", "-1", "4294967296", "1.0", "\"2\"", "null", "true"] {
        reject::<Instruction>(&doc(&format!(
            r#""id":"test.n","version":{bad},"params":{{}}"#
        )));
    }
    reject::<NativeEffectAST>(&doc(r#""id":"test.n","params":{}"#));
    let max = accept::<NativeEffectAST>(&doc(r#""id":"test.n","version":4294967295,"params":{}"#));
    assert_eq!(max.version, u32::MAX);
    assert_eq!(max.registry_key(), "test.n@4294967295");
    // params
    reject::<NativeEffectAST>(&doc(r#""id":"test.n","version":1"#));
    for bad in [
        "[]",
        "1",
        "\"x\"",
        "null",
        r#"{"a":null}"#,
        r#"{"a":[null]}"#,
    ] {
        reject::<NativeEffectAST>(&doc(&format!(
            r#""id":"test.n","version":1,"params":{bad}"#
        )));
    }
    let p = accept::<NativeEffectAST>(&doc(
        r#""id":"test.n","version":1,"params":{"":1,"k":{"x":[true,"s",-9007199254740991]}}"#,
    ));
    assert_eq!(p.params.len(), 2);
    // layers and ruleKinds
    let l = accept::<NativeEffectAST>(&doc(&format!(
        r#"{base},"layers":["7c","1a","4"],"ruleKinds":["SKIP","PROHIBITION"]"#
    )));
    assert_eq!(l.layers, Some(vec![Layer::L7c, Layer::L1a, Layer::L4]));
    assert_eq!(
        l.rule_kinds,
        Some(vec![RuleKind::Skip, RuleKind::Prohibition])
    );
    for bad in [
        r#""layers":[]"#,
        r#""layers":["6","6"]"#,
        r#""layers":["8"]"#,
        r#""layers":"6""#,
        r#""layers":null"#,
        r#""ruleKinds":[]"#,
        r#""ruleKinds":["SKIP","SKIP"]"#,
        r#""ruleKinds":["skip"]"#,
        r#""ruleKinds":null"#,
        r#""rule_kinds":["SKIP"]"#,
    ] {
        reject::<NativeEffectAST>(&doc(&format!("{base},{bad}")));
        reject::<Instruction>(&doc(&format!("{base},{bad}")));
    }
    // op of the standalone form
    reject::<NativeEffectAST>(&format!("{{{base}}}"));
    reject::<NativeEffectAST>(&format!(r#"{{"op":"POPULATE",{base}}}"#));
    reject::<NativeEffectAST>(&format!(r#"{{"op":1,{base}}}"#));
}

/// REQ-DSL-015: invalid hand-built native effects fail to serialise.
#[test]
fn req_dsl_015_native_invalid_not_written() {
    let ok = NativeEffectAST::new("test.n", 1);
    assert!(serde_json::to_vec(&ok).is_ok());
    let bad = |f: &dyn Fn(&mut NativeEffectAST)| {
        let mut n = ok.clone();
        f(&mut n);
        no_encode(&n);
        no_encode(&Instruction::NativeEffect(n));
    };
    bad(&|n| n.id = String::new());
    bad(&|n| n.id = "a@1".to_owned());
    bad(&|n| n.id = "\u{fdd0}".to_owned());
    bad(&|n| n.version = 0);
    bad(&|n| n.layers = Some(vec![]));
    bad(&|n| n.layers = Some(vec![Layer::L6, Layer::L6]));
    bad(&|n| n.rule_kinds = Some(vec![]));
    bad(&|n| n.rule_kinds = Some(vec![RuleKind::Skip, RuleKind::Skip]));
    bad(&|n| {
        n.params.insert("k".to_owned(), Value::Null);
    });
    bad(&|n| {
        n.params
            .insert("k".to_owned(), json!(9_007_199_254_740_992_i64));
    });
    bad(&|n| {
        n.params.insert("\u{ffff}".to_owned(), json!(1));
    });
    bad(&|n| {
        n.params.insert("k".to_owned(), json!({"\u{fdd0}": 1}));
    });
}

// ---------------------------------------------------------------------------------------------
// DurationAST

/// REQ-DSL-010: all nine duration kinds, standalone and embedded; `KINDS` and `kind()` agree.
#[test]
fn req_dsl_010_duration_kinds() {
    let docs = [
        json!({"kind":"UNTIL_END_OF_TURN"}),
        json!({"kind":"UNTIL_YOUR_NEXT_TURN"}),
        json!({"kind":"UNTIL_END_OF_COMBAT"}),
        json!({"kind":"UNTIL","step":"END","whose":{"ref":"CONTROLLER"}}),
        json!({"kind":"FOR_AS_LONG_AS","predicate":{"pred":"IS_CREATURE"}}),
        json!({"kind":"WHILE_SOURCE_ON_BATTLEFIELD"}),
        json!({"kind":"WHILE_CONTROLLED_BY","player":{"ref":"CONTROLLER"}}),
        json!({"kind":"PERMANENT"}),
        json!({"kind":"PERPETUAL"}),
    ];
    assert_eq!(DurationAST::KINDS.len(), 9);
    for (i, d) in docs.iter().enumerate() {
        let dur = accept::<DurationAST>(&d.to_string());
        assert_eq!(dur.kind(), DurationAST::KINDS[i]);
        assert!(!DurationAST::KINDS[..i].contains(&dur.kind()));
        let e = instr(&op_doc(
            "EXILE_UNTIL",
            &json!({"objects":{"ref":"SELF"},"duration":d}),
        ));
        match e {
            Instruction::ExileUntil { duration, .. } => assert_eq!(duration, dur),
            other => panic!("{other:?}"),
        }
        let g = instr(&op_doc(
            "GAIN_CONTROL",
            &json!({"objects":{"ref":"SELF"},"duration":d}),
        ));
        assert_eq!(g.name(), "GAIN_CONTROL");
    }
    assert_eq!(
        accept::<DurationAST>(&docs[3].to_string()),
        DurationAST::Until {
            step: crate::ability::StepId::End,
            whose: ctrl()
        }
    );
}

/// REQ-DSL-010: wrong, extra or missing duration members are rejected.
#[test]
fn req_dsl_010_duration_rejections() {
    for bad in [
        r#"{}"#,
        r#"{"kind":"until_end_of_turn"}"#,
        r#"{"kind":"FOREVER"}"#,
        r#"{"kind":1}"#,
        r#"{"kind":null}"#,
        r#"{"kind":"UNTIL_END_OF_TURN","step":"END"}"#,
        r#"{"kind":"PERMANENT","player":{"ref":"CONTROLLER"}}"#,
        r#"{"kind":"UNTIL","step":"END"}"#,
        r#"{"kind":"UNTIL","whose":{"ref":"CONTROLLER"}}"#,
        r#"{"kind":"UNTIL","step":"end","whose":{"ref":"CONTROLLER"}}"#,
        r#"{"kind":"UNTIL","step":"END","whose":{"ref":"CONTROLLER"},"predicate":{"pred":"IS_LAND"}}"#,
        r#"{"kind":"FOR_AS_LONG_AS"}"#,
        r#"{"kind":"FOR_AS_LONG_AS","predicate":null}"#,
        r#"{"kind":"WHILE_CONTROLLED_BY"}"#,
        r#"{"kind":"WHILE_CONTROLLED_BY","whose":{"ref":"CONTROLLER"}}"#,
        r#""UNTIL_END_OF_TURN""#,
    ] {
        reject::<DurationAST>(bad);
        reject::<Instruction>(&format!(
            r#"{{"op":"EXILE_UNTIL","objects":{{"ref":"SELF"}},"duration":{bad}}}"#
        ));
    }
}

// ---------------------------------------------------------------------------------------------
// Exactly-one members

/// REQ-DSL-010: CREATE_TOKEN needs exactly one of `tokenDef` and `inline`; `modifiers` is a
/// non-empty opaque object.
#[test]
fn req_dsl_010_create_token_exactly_one() {
    let doc = |m: &str| format!(r#"{{"op":"CREATE_TOKEN","count":{{"LITERAL":1}}{m}}}"#);
    let d = instr(&doc(r#","tokenDef":"test.t""#));
    let i = instr(&doc(r#","inline":{}"#));
    let i2 = instr(&doc(r#","inline":{"power":1,"types":["X"]}"#));
    match (&d, &i, &i2) {
        (
            Instruction::CreateToken {
                token: TokenSource::Def(def),
                ..
            },
            Instruction::CreateToken {
                token: TokenSource::Inline(empty),
                ..
            },
            Instruction::CreateToken {
                token: TokenSource::Inline(two),
                ..
            },
        ) => {
            assert_eq!(def, "test.t");
            assert!(empty.is_empty());
            assert_eq!(two.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    for bad in [
        "",
        r#","tokenDef":"test.t","inline":{}"#,
        r#","tokenDef":"""#,
        r#","tokenDef":null"#,
        r#","inline":null"#,
        r#","inline":[]"#,
        r#","inline":{"a":null}"#,
        r#","tokenDef":"t","modifiers":{}"#,
        r#","tokenDef":"t","modifiers":[]"#,
        r#","tokenDef":"t","modifiers":{"a":[null]}"#,
    ] {
        reject::<Instruction>(&doc(bad));
    }
    // standalone TokenSource
    assert_eq!(
        accept::<TokenSource>(r#"{"tokenDef":"test.t"}"#),
        TokenSource::Def("test.t".to_owned())
    );
    assert_eq!(
        accept::<TokenSource>(r#"{"inline":{}}"#),
        TokenSource::Inline(BTreeMap::new())
    );
    for bad in [
        "{}",
        r#"{"tokenDef":"t","inline":{}}"#,
        r#"{"tokenDef":"t","x":1}"#,
        r#"{"tokenDef":""}"#,
        r#""t""#,
    ] {
        reject::<TokenSource>(bad);
    }
    no_encode(&TokenSource::Def(String::new()));
    let mut inline = BTreeMap::new();
    inline.insert("k".to_owned(), Value::Null);
    no_encode(&TokenSource::Inline(inline.clone()));
    no_encode(&Instruction::CreateToken {
        token: TokenSource::Inline(inline),
        count: ValueExpressionAST::Literal(1),
        modifiers: None,
        controller: None,
    });
    no_encode(&Instruction::CreateToken {
        token: TokenSource::Def("t".to_owned()),
        count: ValueExpressionAST::Literal(1),
        modifiers: Some(BTreeMap::new()),
        controller: None,
    });
}

/// REQ-DSL-010: REMEMBER needs exactly one of `value` and `objects`.
#[test]
fn req_dsl_010_remember_exactly_one() {
    let doc = |m: &str| format!(r#"{{"op":"REMEMBER","key":"test.k","into":"LINKED"{m}}}"#);
    assert_eq!(
        instr(&doc(r#","objects":{"ref":"SELF"}"#)),
        Instruction::Remember {
            key: "test.k".to_owned(),
            into: MemoryStore::Linked,
            source: RememberSource::Objects(self_sel()),
        }
    );
    assert_eq!(
        instr(&doc(r#","value":"X""#)),
        Instruction::Remember {
            key: "test.k".to_owned(),
            into: MemoryStore::Linked,
            source: RememberSource::Value(ValueExpressionAST::X),
        }
    );
    for bad in [
        "",
        r#","value":{"LITERAL":1},"objects":{"ref":"SELF"}"#,
        r#","value":null"#,
        r#","objects":null"#,
        r#","value":1"#,
    ] {
        reject::<Instruction>(&doc(bad));
    }
    reject::<Instruction>(r#"{"op":"REMEMBER","key":"k","into":"memory","value":"X"}"#);
    // standalone RememberSource
    assert_eq!(
        accept::<RememberSource>(r#"{"value":{"LITERAL":4}}"#),
        RememberSource::Value(ValueExpressionAST::Literal(4))
    );
    assert_eq!(
        accept::<RememberSource>(r#"{"objects":{"ref":"SELF"}}"#),
        RememberSource::Objects(self_sel())
    );
    for bad in [
        "{}",
        r#"{"value":"X","objects":{"ref":"SELF"}}"#,
        r#"{"value":"X","y":1}"#,
    ] {
        reject::<RememberSource>(bad);
    }
    no_encode(&Instruction::Remember {
        key: String::new(),
        into: MemoryStore::Memory,
        source: RememberSource::Value(ValueExpressionAST::X),
    });
}

/// REQ-DSL-010: SET_DESIGNATION `value` is a boolean or a non-empty string.
#[test]
fn req_dsl_010_set_designation_value() {
    let doc = |v: &str| format!(r#"{{"op":"SET_DESIGNATION","kind":"test.d","value":{v}}}"#);
    for (v, want) in [
        ("true", DesignationValue::Flag(true)),
        ("false", DesignationValue::Flag(false)),
        (r#""day""#, DesignationValue::Name("day".to_owned())),
    ] {
        match instr(&doc(v)) {
            Instruction::SetDesignation { value, .. } => assert_eq!(value, want),
            other => panic!("{other:?}"),
        }
        assert_eq!(accept::<DesignationValue>(v), want);
    }
    for bad in [r#""""#, "1", "null", "{}", "[true]"] {
        reject::<Instruction>(&doc(bad));
        reject::<DesignationValue>(bad);
    }
    no_encode(&DesignationValue::Name(String::new()));
    no_encode(&Instruction::SetDesignation {
        kind: "k".to_owned(),
        value: DesignationValue::Name(String::new()),
        to: None,
    });
    no_encode(&Instruction::SetDesignation {
        kind: String::new(),
        value: DesignationValue::Flag(true),
        to: Some(EventSubject::Players(PlayerGroup::AllPlayers)),
    });
}

// ---------------------------------------------------------------------------------------------
// CHOOSE_MODE

/// REQ-DSL-010: CHOOSE_MODE rules (distinct ids, `min <= max <= modes`, non-empty modes and
/// mode effects, distinct target slots).
#[test]
fn req_dsl_010_choose_mode_rules() {
    let mode = |id: &str| json!({"id": id, "effect": [{"op":"POPULATE"}]});
    let doc = |min: u32, max: u32, modes: Value| {
        op_doc(
            "CHOOSE_MODE",
            &json!({"chooser":{"ref":"CONTROLLER"},"min":min,"max":max,"modes":modes}),
        )
    };
    let ok = instr(&doc(0, 2, json!([mode("a"), mode("b")])));
    match &ok {
        Instruction::ChooseMode {
            min, max, modes, ..
        } => {
            assert_eq!((*min, *max, modes.len()), (0, 2, 2));
        }
        other => panic!("{other:?}"),
    }
    instr(&doc(0, 0, json!([mode("a")])));
    instr(&doc(
        1,
        1,
        json!([{"id":"t","targets":[{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1},{"slot":1,"selector":{"ref":"SELF"},"min":0,"max":1}],"effect":[{"op":"POPULATE"}]}]),
    ));
    reject::<Instruction>(&doc(2, 1, json!([mode("a"), mode("b")])));
    reject::<Instruction>(&doc(1, 3, json!([mode("a"), mode("b")])));
    reject::<Instruction>(&doc(1, 1, json!([mode("a"), mode("a")])));
    reject::<Instruction>(&doc(0, 0, json!([])));
    reject::<Instruction>(&doc(1, 1, json!([{"id":"a","effect":[]}])));
    reject::<Instruction>(&doc(1, 1, json!([{"id":"","effect":[{"op":"POPULATE"}]}])));
    reject::<Instruction>(&doc(
        1,
        1,
        json!([{"id":"a","targets":[],"effect":[{"op":"POPULATE"}]}]),
    ));
    reject::<Instruction>(&doc(
        1,
        1,
        json!([{"id":"a","targets":[{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1},{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}],"effect":[{"op":"POPULATE"}]}]),
    ));
    reject::<Instruction>(&doc(
        1,
        1,
        json!([{"id":"a","effect":[{"op":"POPULATE"}],"x":1}]),
    ));
    reject::<Instruction>(&op_doc(
        "CHOOSE_MODE",
        &json!({"chooser":{"ref":"CONTROLLER"},"min":-1,"max":1,"modes":[mode("a")]}),
    ));
    // standalone InstructionMode
    let m = accept::<InstructionMode>(r#"{"id":"m","effect":[{"op":"VENTURE"}]}"#);
    assert_eq!(
        m,
        InstructionMode {
            id: "m".to_owned(),
            targets: None,
            effect: vec![Instruction::Venture],
        }
    );
    reject::<InstructionMode>(r#"{"id":"m","effect":[]}"#);
    reject::<InstructionMode>(r#"{"id":"m"}"#);
    reject::<InstructionMode>(r#"{"effect":[{"op":"VENTURE"}]}"#);
    // hand-built
    no_encode(&InstructionMode {
        id: "m".to_owned(),
        targets: None,
        effect: vec![],
    });
    no_encode(&InstructionMode {
        id: String::new(),
        targets: None,
        effect: vec![Instruction::Venture],
    });
    no_encode(&InstructionMode {
        id: "m".to_owned(),
        targets: Some(vec![]),
        effect: vec![Instruction::Venture],
    });
    let mk = |min, max, ids: &[&str]| Instruction::ChooseMode {
        chooser: ctrl(),
        min,
        max,
        modes: ids
            .iter()
            .map(|id| InstructionMode {
                id: (*id).to_owned(),
                targets: None,
                effect: vec![Instruction::Populate],
            })
            .collect(),
    };
    assert!(serde_json::to_vec(&mk(1, 2, &["a", "b"])).is_ok());
    no_encode(&mk(2, 1, &["a", "b"]));
    no_encode(&mk(1, 3, &["a", "b"]));
    no_encode(&mk(1, 1, &["a", "a"]));
    no_encode(&mk(0, 0, &[]));
}

/// REQ-DSL-010: CHOOSE_MODE with many distinct ids is accepted, and a duplicate that is not
/// adjacent (first and last) is rejected with the "listed twice" diagnostic on decode and encode.
#[test]
fn req_dsl_010_choose_mode_many_ids_and_duplicates() {
    let ids: Vec<String> = (0..200).map(|n| format!("m{n}")).collect();
    let mk = |ids: &[String]| Instruction::ChooseMode {
        chooser: ctrl(),
        min: 0,
        max: 1,
        modes: ids
            .iter()
            .map(|id| InstructionMode {
                id: id.clone(),
                targets: None,
                effect: vec![Instruction::Populate],
            })
            .collect(),
    };
    let wire = |ids: &[String]| {
        let modes: Vec<Value> = ids
            .iter()
            .map(|id| json!({"id": id, "effect": [{"op":"POPULATE"}]}))
            .collect();
        op_doc(
            "CHOOSE_MODE",
            &json!({"chooser":{"ref":"CONTROLLER"},"min":0,"max":1,"modes":modes}),
        )
    };
    let ok = mk(&ids);
    let bytes = serde_json::to_vec(&ok).expect("distinct ids encode");
    assert_eq!(instr(&wire(&ids)), ok);
    assert_eq!(
        serde_json::from_slice::<Instruction>(&bytes).expect("decode"),
        ok
    );

    let mut dup = ids.clone();
    dup.push(ids[0].clone());
    let enc = serde_json::to_vec(&mk(&dup)).expect_err("duplicate must not encode");
    assert!(enc.to_string().contains("listed twice"), "{enc}");
    let dec =
        serde_json::from_str::<Instruction>(&wire(&dup)).expect_err("duplicate must not decode");
    assert!(dec.to_string().contains("listed twice"), "{dec}");
}

// ---------------------------------------------------------------------------------------------
// Lists, triggers, control flow

/// REQ-DSL-010 (rule A4): an empty instruction list is rejected wherever one appears, and an
/// empty cost or effect list too.
#[test]
fn req_dsl_010_empty_lists_rejected() {
    for c in catalogue() {
        for (k, v) in c.req.iter().chain(c.opt.iter()) {
            if v.is_array() && *k != "layers" && *k != "ruleKinds" {
                let mut m = c.full();
                m.insert((*k).to_owned(), json!([]));
                reject::<Instruction>(&text(&m));
            }
        }
    }
    for (op, key) in [
        ("CREATE_DELAYED_TRIGGER", "effect"),
        ("REFLEXIVE_TRIGGER", "ifActionTaken"),
        ("REFLEXIVE_TRIGGER", "effect"),
        ("IF", "then"),
        ("IF", "else"),
        ("FOR_EACH", "body"),
        ("REPEAT", "body"),
        ("REPEAT_WHILE", "body"),
        ("MAY", "body"),
        ("UNLESS", "body"),
        ("UNLESS", "cost"),
        ("WATERBEND", "cost"),
        ("COUNTER_SPELL", "unless"),
        ("CAST_FROM", "altCost"),
        ("CREATE_CONTINUOUS_EFFECT", "effects"),
    ] {
        let c = catalogue().into_iter().find(|c| c.op == op).expect("op");
        let mut m = c.full();
        m.insert(key.to_owned(), json!([]));
        reject::<Instruction>(&text(&m));
        m.insert(key.to_owned(), json!({}));
        reject::<Instruction>(&text(&m));
    }
    // an invalid element in a list is rejected
    reject::<Instruction>(r#"{"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"NOPE"}]}"#);
    reject::<Instruction>(r#"{"op":"WATERBEND","cost":[{"cost":"TAP_SOURCE"},{"cost":"NOPE"}]}"#);
}

/// REQ-DSL-010: hand-built empty lists and invalid strings fail to serialise.
#[test]
fn req_dsl_010_hand_built_invalid_not_written() {
    let body = vec![Instruction::Populate];
    let cost = vec![CostAST::TapSource];
    let bad = [
        Instruction::Waterbend { cost: vec![] },
        Instruction::CounterSpell {
            objects: self_sel(),
            unless: Some(vec![]),
        },
        Instruction::CastFrom {
            objects: self_sel(),
            without_paying: None,
            alt_cost: Some(vec![]),
        },
        Instruction::Unless {
            payer: ctrl(),
            cost: vec![],
            body: body.clone(),
        },
        Instruction::Unless {
            payer: ctrl(),
            cost: cost.clone(),
            body: vec![],
        },
        Instruction::May {
            player: ctrl(),
            body: vec![],
        },
        Instruction::Repeat {
            times: ValueExpressionAST::X,
            body: vec![],
        },
        Instruction::RepeatWhile {
            cond: PredicateAST::IsLand,
            max_iter: NonZeroU32::MIN,
            body: vec![],
        },
        Instruction::If {
            cond: PredicateAST::IsLand,
            then: vec![],
            otherwise: None,
        },
        Instruction::If {
            cond: PredicateAST::IsLand,
            then: body.clone(),
            otherwise: Some(vec![]),
        },
        Instruction::ForEach {
            iterator: EventSubject::Players(PlayerGroup::AllPlayers),
            bind: None,
            body: vec![],
        },
        Instruction::ForEach {
            iterator: EventSubject::Players(PlayerGroup::AllPlayers),
            bind: Some("x".to_owned()),
            body: body.clone(),
        },
        Instruction::ReflexiveTrigger {
            if_action_taken: vec![],
            effect: body.clone(),
        },
        Instruction::ReflexiveTrigger {
            if_action_taken: body.clone(),
            effect: vec![],
        },
        Instruction::CreateDelayedTrigger {
            trigger: Trigger::State(PredicateAST::IsLand),
            effect: vec![],
            captures: None,
            one_shot: true,
        },
        Instruction::CreateDelayedTrigger {
            trigger: Trigger::State(PredicateAST::IsLand),
            effect: body.clone(),
            captures: Some(BTreeMap::new()),
            one_shot: true,
        },
        Instruction::CreateDelayedTrigger {
            trigger: Trigger::State(PredicateAST::IsLand),
            effect: body.clone(),
            captures: Some([("nodollar".to_owned(), self_sel())].into_iter().collect()),
            one_shot: false,
        },
        Instruction::CreateContinuousEffect {
            effects: vec![],
            duration: DurationAST::Permanent,
            lock_affected_set: None,
        },
        Instruction::PutCounters {
            objects: self_sel(),
            kind: String::new(),
            count: ValueExpressionAST::X,
        },
        Instruction::RemoveCounters {
            objects: self_sel(),
            kind: String::new(),
            count: ValueExpressionAST::X,
        },
        Instruction::MoveCounters {
            from: self_sel(),
            to: self_sel(),
            kind: String::new(),
            count: ValueExpressionAST::X,
        },
        Instruction::CreatePredefinedToken {
            kind: String::new(),
            count: None,
        },
        Instruction::CreateCopyToken {
            of: self_sel(),
            except: Some(BTreeMap::new()),
        },
        Instruction::PlayPermissionGrant {
            objects: self_sel(),
            duration: DurationAST::Permanent,
            who: ctrl(),
            also_as: Some(String::new()),
        },
        Instruction::TurnFaceUp {
            objects: self_sel(),
            profile: Some(String::new()),
        },
        Instruction::TurnFaceDown {
            objects: self_sel(),
            profile: Some("\u{fdd0}".to_owned()),
        },
        Instruction::Vote {
            voters: ctrl(),
            choice: ChoiceSpec::new(ctrl(), ChoiceKind::Player),
            store: String::new(),
        },
        Instruction::Discard {
            player: ctrl(),
            count: ValueExpressionAST::X,
            choice: Some(ChoiceSpec {
                up_to: Some(true),
                ..ChoiceSpec::new(ctrl(), ChoiceKind::Objects)
            }),
        },
    ];
    for b in &bad {
        no_encode(b);
        // and nested inside a valid parent
        no_encode(&Instruction::May {
            player: ctrl(),
            body: vec![b.clone()],
        });
    }
}

/// REQ-DSL-010 (CR 603.7, CR 603.12): delayed and reflexive triggers.
#[test]
fn req_dsl_010_delayed_and_reflexive_triggers() {
    let d = instr(
        r#"{"op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}],"captures":{"$a":{"ref":"SELF"},"$B_2":{"ref":"TARGET","slot":1}},"oneShot":false}"#,
    );
    match &d {
        Instruction::CreateDelayedTrigger {
            captures: Some(c),
            one_shot,
            ..
        } => {
            assert_eq!(c.len(), 2);
            assert!(!*one_shot);
        }
        other => panic!("{other:?}"),
    }
    let base = r#""op":"CREATE_DELAYED_TRIGGER","trigger":{"mode":"STATE","state":{"pred":"IS_LAND"}},"effect":[{"op":"POPULATE"}]"#;
    for bad in [
        r#","oneShot":true,"captures":{}"#,
        r#","oneShot":true,"captures":{"a":{"ref":"SELF"}}"#,
        r#","oneShot":true,"captures":{"$":{"ref":"SELF"}}"#,
        r#","oneShot":true,"captures":{"$a-b":{"ref":"SELF"}}"#,
        r#","oneShot":true,"captures":{"$a":null}"#,
        r#","oneShot":true,"captures":[]"#,
        r#","oneShot":"true""#,
        r#","oneShot":1"#,
        "",
    ] {
        reject::<Instruction>(&format!("{{{base}{bad}}}"));
    }
    let r = instr(
        r#"{"op":"REFLEXIVE_TRIGGER","ifActionTaken":[{"op":"SACRIFICE","player":{"ref":"CONTROLLER"},"selector":{"ref":"SELF"},"count":{"LITERAL":1}}],"effect":[{"op":"GAIN_LIFE","player":{"ref":"CONTROLLER"},"amount":{"LITERAL":2}}]}"#,
    );
    match r {
        Instruction::ReflexiveTrigger {
            if_action_taken,
            effect,
        } => {
            assert_eq!(if_action_taken[0].name(), "SACRIFICE");
            assert_eq!(effect[0].name(), "GAIN_LIFE");
        }
        other => panic!("{other:?}"),
    }
    reject::<Instruction>(
        r#"{"op":"REFLEXIVE_TRIGGER","ifActionTaken":{"op":"POPULATE"},"effect":[{"op":"POPULATE"}]}"#,
    );
}

/// REQ-DSL-010: control flow (IF/else, FOR_EACH bind, REPEAT, MAY, UNLESS per CR 733.6).
#[test]
fn req_dsl_010_control_flow() {
    let i = instr(
        r#"{"op":"IF","cond":{"pred":"IS_LAND"},"then":[{"op":"POPULATE"}],"else":[{"op":"VENTURE"},{"op":"RING_TEMPTS"}]}"#,
    );
    assert_eq!(
        i,
        Instruction::If {
            cond: PredicateAST::IsLand,
            then: vec![Instruction::Populate],
            otherwise: Some(vec![Instruction::Venture, Instruction::RingTempts]),
        }
    );
    reject::<Instruction>(
        r#"{"op":"IF","cond":{"pred":"IS_LAND"},"then":[{"op":"POPULATE"}],"otherwise":[{"op":"POPULATE"}]}"#,
    );
    let f = instr(
        r#"{"op":"FOR_EACH","iterator":{"players":"ALL_PLAYERS"},"bind":"$p","body":[{"op":"DRAW","player":{"ref":"BOUND","var":"$p"},"count":{"LITERAL":1}}]}"#,
    );
    match f {
        Instruction::ForEach { iterator, bind, .. } => {
            assert_eq!(iterator, EventSubject::Players(PlayerGroup::AllPlayers));
            assert_eq!(bind.as_deref(), Some("$p"));
        }
        other => panic!("{other:?}"),
    }
    for bad in [r#""p""#, r#""$""#, r#""$a b""#, "1"] {
        reject::<Instruction>(&format!(
            r#"{{"op":"FOR_EACH","iterator":{{"ref":"SELF"}},"bind":{bad},"body":[{{"op":"POPULATE"}}]}}"#
        ));
    }
    let u = instr(
        r#"{"op":"UNLESS","payer":{"ref":"THAT_PLAYER"},"cost":[{"cost":"PAY_LIFE","amount":{"LITERAL":2}}],"body":[{"op":"COUNTER_SPELL","objects":{"ref":"TARGET","slot":0}}]}"#,
    );
    assert_eq!(u.name(), "UNLESS");
    let nested = instr(
        r#"{"op":"REPEAT","times":"X","body":[{"op":"MAY","player":{"ref":"CONTROLLER"},"body":[{"op":"IF","cond":{"pred":"IS_CREATURE"},"then":[{"op":"POPULATE"}]}]}]}"#,
    );
    assert_eq!(nested.name(), "REPEAT");
}

/// REQ-DSL-010 (CR 611.2c): CREATE_CONTINUOUS_EFFECT with CHAR, PLAYER and RULE effects, and
/// the three-way `lockAffectedSet` (absent means true).
#[test]
fn req_dsl_010_continuous_effect() {
    let doc = |extra: &str| {
        format!(
            r#"{{"op":"CREATE_CONTINUOUS_EFFECT","effects":[{{"type":"CHAR","layer":"7c","affected":{{"ref":"TARGET","slot":0}},"modification":{{"op":"MODIFY_PT","power":2,"toughness":2}}}},{{"type":"PLAYER","affected":{{"ref":"CONTROLLER"}},"modification":{{"op":"TEST_PLAYER_OP"}}}},{{"type":"RULE","kind":"PROHIBITION","what":"test"}}],"duration":{{"kind":"UNTIL_END_OF_TURN"}}{extra}}}"#
        )
    };
    let absent = instr(&doc(""));
    let yes = instr(&doc(r#","lockAffectedSet":true"#));
    let no = instr(&doc(r#","lockAffectedSet":false"#));
    assert!(absent.locks_affected_set());
    assert!(yes.locks_affected_set());
    assert!(!no.locks_affected_set());
    assert_ne!(absent, yes);
    assert_ne!(absent, no);
    assert_ne!(yes, no);
    match &absent {
        Instruction::CreateContinuousEffect {
            effects,
            lock_affected_set,
            ..
        } => {
            assert_eq!(*lock_affected_set, None);
            assert!(matches!(effects[0], StaticEffect::Char(_)));
            assert!(matches!(effects[1], StaticEffect::Player(_)));
            assert!(matches!(effects[2], StaticEffect::Rule(_)));
        }
        other => panic!("{other:?}"),
    }
    reject::<Instruction>(&doc(r#","lockAffectedSet":null"#));
    reject::<Instruction>(&doc(r#","lockAffectedSet":"true""#));
    // No sameAsLayer cross-check inside this node (docs/DECISIONS.md): a dangling sameAsLayer is accepted here.
    instr(
        r#"{"op":"CREATE_CONTINUOUS_EFFECT","effects":[{"type":"CHAR","layer":"6","affected":{"sameAsLayer":"4"},"modification":{"op":"TEST_OP"}}],"duration":{"kind":"PERMANENT"}}"#,
    );
    // Instructions other than CREATE_CONTINUOUS_EFFECT lock nothing.
    assert!(!Instruction::Populate.locks_affected_set());
    assert!(!Instruction::NativeEffect(NativeEffectAST::new("test.n", 1)).locks_affected_set());
}

/// REQ-DSL-011: every instruction with a decision accepts a `choice` and rejects an invalid one.
#[test]
fn req_dsl_011_choice_members() {
    let with_choice: Vec<&str> = catalogue()
        .iter()
        .filter(|c| {
            c.opt
                .iter()
                .chain(c.req.iter())
                .any(|(k, _)| *k == "choice")
        })
        .map(|c| c.op)
        .collect();
    assert_eq!(
        with_choice,
        [
            "DISCARD",
            "MOVE",
            "SEARCH_LIBRARY",
            "REVEAL",
            "PUT_ON_TOP_OR_BOTTOM",
            "DEAL_DAMAGE",
            "SACRIFICE",
            "PROLIFERATE",
            "CHANGE_TARGETS",
            "VOTE"
        ]
    );
    for c in catalogue() {
        if !with_choice.contains(&c.op) {
            continue;
        }
        let mut m = c.full();
        m.insert(
            "choice".to_owned(),
            json!({"chooser":{"ref":"CONTROLLER"},"kind":"objects"}),
        );
        reject::<Instruction>(&text(&m));
        m.insert(
            "choice".to_owned(),
            json!({"chooser":{"ref":"CONTROLLER"},"kind":"NUMBER","upTo":true}),
        );
        reject::<Instruction>(&text(&m));
        m.insert(
            "choice".to_owned(),
            json!({"chooser":{"ref":"CONTROLLER"},"kind":"NUMBER","upTo":true,"max":{"LITERAL":2}}),
        );
        instr(&text(&m));
    }
}

// ---------------------------------------------------------------------------------------------
// Strictness (rule A2)

/// Decodes `doc` through `from_str`, `from_slice` and `from_value`/borrowed `&Value`
/// (when it parses as a `Value`), returning whether each accepted it.
fn all_paths(doc: &str) -> [bool; 3] {
    let s = serde_json::from_str::<Instruction>(doc).is_ok();
    let b = serde_json::from_slice::<Instruction>(doc.as_bytes()).is_ok();
    let v = match serde_json::from_str::<Value>(doc) {
        Ok(v) => Instruction::deserialize(&v).is_ok(),
        Err(_) => false,
    };
    [s, b, v]
}

/// REQ-NFR-006, REQ-CORE-006: floats, unsafe integers, duplicate members and noncharacters are
/// rejected on every decode path.
#[test]
fn req_nfr_006_strictness() {
    let draw = |count: &str| {
        format!(r#"{{"op":"DRAW","player":{{"ref":"CONTROLLER"}},"count":{{"LITERAL":{count}}}}}"#)
    };
    assert_eq!(all_paths(&draw("3")), [true; 3]);
    for bad in ["1.0", "1e2", "-0", "9007199254740992", "-9007199254740992"] {
        assert_eq!(all_paths(&draw(bad)), [false; 3], "{bad}");
    }
    assert_eq!(all_paths(&draw("9007199254740991")), [true; 3]);
    // Duplicate member names (visible to the str and slice paths only).
    let dup = r#"{"op":"POPULATE","op":"POPULATE"}"#;
    assert!(serde_json::from_str::<Instruction>(dup).is_err());
    assert!(serde_json::from_slice::<Instruction>(dup.as_bytes()).is_err());
    reject::<Instruction>(
        r#"{"op":"DRAW","player":{"ref":"CONTROLLER"},"player":{"ref":"OWNER"},"count":"X"}"#,
    );
    reject::<Instruction>(r#"{"op":"NATIVE_EFFECT","id":"n","version":1,"params":{"a":1,"a":2}}"#);
    // Noncharacters in strings and member names.
    for bad in [
        "{\"op\":\"PUT_COUNTERS\",\"objects\":{\"ref\":\"SELF\"},\"kind\":\"a\u{fdd0}\",\"count\":\"X\"}",
        "{\"op\":\"NATIVE_EFFECT\",\"id\":\"n\",\"version\":1,\"params\":{\"\u{ffff}\":1}}",
        "{\"op\":\"NATIVE_EFFECT\",\"id\":\"n\u{1fffe}\",\"version\":1,\"params\":{}}",
    ] {
        assert_eq!(all_paths(bad), [false; 3], "{bad}");
    }
    // Floats inside opaque values.
    for bad in [
        r#"{"op":"NATIVE_EFFECT","id":"n","version":1,"params":{"a":1.5}}"#,
        r#"{"op":"CREATE_COPY_TOKEN","of":{"ref":"SELF"},"except":{"a":[2e3]}}"#,
    ] {
        assert_eq!(all_paths(bad), [false; 3], "{bad}");
    }
}

/// A NATIVE_EFFECT document whose deepest container is at depth `depth` (params object at 2,
/// then nested arrays).
fn deep_native(depth: usize) -> String {
    let arrays = depth - 2;
    format!(
        r#"{{"op":"NATIVE_EFFECT","id":"n","version":1,"params":{{"k":{}1{}}}}}"#,
        "[".repeat(arrays),
        "]".repeat(arrays)
    )
}

/// REQ-NFR-006: nesting at `MAX_DEPTH` is accepted and beyond it rejected, through `from_str`,
/// `from_slice` and `from_value`; a deep instruction tree too.
#[test]
fn req_nfr_006_depth_limit() {
    let ok = deep_native(MAX_DEPTH);
    let too_deep = deep_native(MAX_DEPTH + 1);
    assert!(serde_json::from_str::<Instruction>(&ok).is_ok());
    assert!(serde_json::from_slice::<Instruction>(ok.as_bytes()).is_ok());
    assert!(serde_json::from_str::<Instruction>(&too_deep).is_err());
    assert!(serde_json::from_slice::<Instruction>(too_deep.as_bytes()).is_err());
    assert!(serde_json::from_str::<NativeEffectAST>(&too_deep).is_err());
    // from_value on values built in memory.
    let mut inner = json!(1);
    for _ in 0..MAX_DEPTH - 2 {
        inner = Value::Array(vec![inner]);
    }
    let at = json!({"op":"NATIVE_EFFECT","id":"n","version":1,"params":{"k":inner.clone()}});
    assert!(Instruction::deserialize(&at).is_ok());
    assert!(serde_json::from_value::<Instruction>(at).is_ok());
    let beyond = json!({"op":"NATIVE_EFFECT","id":"n","version":1,"params":{"k":[inner]}});
    assert!(Instruction::deserialize(&beyond).is_err());
    assert!(serde_json::from_value::<Instruction>(beyond).is_err());
    // A deep MAY chain: each level adds an object and an array.
    let chain = |levels: usize| {
        let mut s = String::from(r#"{"op":"POPULATE"}"#);
        for _ in 0..levels {
            s = format!(r#"{{"op":"MAY","player":{{"players":"ALL_PLAYERS"}},"body":[{s}]}}"#);
        }
        s
    };
    // depth = 2 * levels + 1 (the leaf object).
    let deepest: Instruction = serde_json::from_str(&chain(63)).expect("depth 127");
    assert!(serde_json::from_str::<Instruction>(&chain(64)).is_err());
    // The deepest valid tree encodes and reloads (each level costs a small stack frame).
    let bytes = to_canonical_vec(&deepest).expect("encode");
    assert_eq!(
        bytes,
        crate::canonical::canonicalize(chain(63).as_bytes()).expect("jcs")
    );
    let again: Instruction = serde_json::from_slice(&bytes).expect("reload");
    assert_eq!(again, deepest);
    let v = serde_json::to_value(&deepest).expect("value");
    assert_eq!(Instruction::deserialize(&v).expect("from &Value"), deepest);
    // Hand-built opaque values nested too deep fail to serialise.
    let mut deep = json!(1);
    for _ in 0..MAX_DEPTH - 1 {
        deep = Value::Array(vec![deep]);
    }
    let mut n = NativeEffectAST::new("n", 1);
    n.params.insert("k".to_owned(), deep.clone());
    assert!(serde_json::to_vec(&n).is_ok());
    n.params.insert("k".to_owned(), Value::Array(vec![deep]));
    no_encode(&n);
}

// ---------------------------------------------------------------------------------------------
// Construction helpers

/// REQ-DSL-015: `NativeEffectAST::new` and `registry_key`.
#[test]
fn req_dsl_015_new_and_registry_key() {
    let mut n = NativeEffectAST::new(String::from("test.native.x"), 3);
    assert!(n.params.is_empty());
    assert_eq!(n.layers, None);
    assert_eq!(n.rule_kinds, None);
    assert_eq!(n.registry_key(), "test.native.x@3");
    n.layers = Some(vec![Layer::L1a]);
    n.rule_kinds = Some(vec![RuleKind::MaxHandSize]);
    n.params.insert("depth".to_owned(), json!({"a": [1, 2]}));
    let bytes = to_canonical_vec(&n).expect("encode");
    assert_eq!(
        String::from_utf8(bytes.clone()).expect("utf8"),
        r#"{"id":"test.native.x","layers":["1a"],"op":"NATIVE_EFFECT","params":{"depth":{"a":[1,2]}},"ruleKinds":["MAX_HAND_SIZE"],"version":3}"#
    );
    let back: Instruction = serde_json::from_slice(&bytes).expect("decode");
    assert_eq!(back, Instruction::NativeEffect(n));
}

/// REQ-DSL-010: hand-built values of every family encode to documents that decode back.
#[test]
fn req_dsl_010_hand_built_round_trip() {
    let values = [
        Instruction::Draw {
            player: ctrl(),
            count: ValueExpressionAST::Literal(1),
        },
        Instruction::DealDamage {
            source: self_sel(),
            recipients: EventSubject::Players(PlayerGroup::AllPlayers),
            amount: ValueExpressionAST::X,
            divided: Some(false),
            choice: None,
        },
        Instruction::CreateToken {
            token: TokenSource::Def("test.t".to_owned()),
            count: ValueExpressionAST::Literal(2),
            modifiers: Some([("k".to_owned(), json!(true))].into_iter().collect()),
            controller: Some(ctrl()),
        },
        Instruction::Remember {
            key: "k".to_owned(),
            into: MemoryStore::Memory,
            source: RememberSource::Value(ValueExpressionAST::Literal(1)),
        },
        Instruction::Vote {
            voters: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
            choice: ChoiceSpec::new(ctrl(), ChoiceKind::Player),
            store: "test.v".to_owned(),
        },
        Instruction::ExileUntil {
            objects: self_sel(),
            duration: DurationAST::WhileSourceOnBattlefield,
            face_down: Some(true),
        },
    ];
    for v in values {
        let bytes = to_canonical_vec(&v).expect("encode");
        let back: Instruction = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(back, v);
    }
}
