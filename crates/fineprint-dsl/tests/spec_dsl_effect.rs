// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::effect` (FP-013 wire contract), part 1: the instruction
//! catalogue (REQ-DSL-010, SRS §14.4.5). Per-op acceptance, canonical form and rejections for all
//! 72 ops, the `REPEAT_WHILE` `maxIter` acceptance criterion, seeded round-trip property tests
//! over generated instruction trees, and seeded random-bytes / mutation runs (R12).
//! Black-box: written from the work-item brief only. Fixed seeds, no wall clock; synthetic data
//! only (R5).

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use fineprint_dsl::effect::{DurationAST, Instruction, NativeEffectAST};
use fineprint_dsl::expr::{PlayerSelectorAST, PredicateAST, ValueExpressionAST};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------- catalogue

/// The 72 op names of brief section C, in catalogue order.
const NAMES: [&str; 72] = [
    "DRAW",
    "MILL",
    "DISCARD",
    "MOVE",
    "SEARCH_LIBRARY",
    "SHUFFLE",
    "REVEAL",
    "LOOK_AT",
    "PUT_ON_TOP_OR_BOTTOM",
    "EXILE_UNTIL",
    "PLAY_PERMISSION_GRANT",
    "DEAL_DAMAGE",
    "FIGHT",
    "BITE",
    "GAIN_LIFE",
    "LOSE_LIFE",
    "SET_LIFE",
    "PREVENT",
    "DESTROY",
    "SACRIFICE",
    "REGENERATE",
    "COUNTER_SPELL",
    "PUT_COUNTERS",
    "REMOVE_COUNTERS",
    "MOVE_COUNTERS",
    "PROLIFERATE",
    "CREATE_TOKEN",
    "CREATE_PREDEFINED_TOKEN",
    "CREATE_COPY_TOKEN",
    "POPULATE",
    "AMASS",
    "INCUBATE",
    "COPY_SPELL_OR_ABILITY",
    "CHANGE_TARGETS",
    "CAST_FROM",
    "TAP_OR_UNTAP",
    "TRANSFORM",
    "CONVERT",
    "TURN_FACE_UP",
    "TURN_FACE_DOWN",
    "PHASE_OUT",
    "ATTACH",
    "GAIN_CONTROL",
    "EXCHANGE_CONTROL",
    "GOAD",
    "SET_DESIGNATION",
    "VENTURE",
    "RING_TEMPTS",
    "INCREASE_SPEED",
    "EMPOWER_JACE",
    "MELD",
    "MUTATE_MERGE",
    "MANIFEST",
    "CLOAK",
    "EXPLORE",
    "CONNIVE",
    "DISCOVER",
    "BLIGHT",
    "WATERBEND",
    "CREATE_CONTINUOUS_EFFECT",
    "CREATE_DELAYED_TRIGGER",
    "REFLEXIVE_TRIGGER",
    "IF",
    "FOR_EACH",
    "REPEAT",
    "REPEAT_WHILE",
    "MAY",
    "UNLESS",
    "CHOOSE_MODE",
    "VOTE",
    "REMEMBER",
    "NATIVE_EFFECT",
];

/// The type of one member (brief A3-A8 and section C).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum K {
    Plr,
    Obj,
    Val,
    Ch,
    Dur,
    Subj,
    Costs,
    Zone,
    Pos,
    Bool,
    Str,
    Instrs,
    Pred,
    Trigger,
    Statics,
    Captures,
    /// Opaque object, may be empty.
    Opaque,
    /// Opaque object, non-empty.
    OpaqueNe,
    Var,
    TapAct,
    Store,
    Desig,
    MaxIter,
    MinMax,
    Modes,
    Version,
    Layers,
    RuleKinds,
}

/// `(member, type, required)`.
type M = (&'static str, K, bool);

const R: bool = true;
const O: bool = false;

/// Section C. `CREATE_TOKEN` lists `tokenDef` as required (its exactly-one partner `inline` is
/// tested separately); `REMEMBER` lists `value` likewise (partner `objects`).
const CATALOGUE: [(&str, &[M]); 72] = [
    ("DRAW", &[("player", K::Plr, R), ("count", K::Val, R)]),
    ("MILL", &[("player", K::Plr, R), ("count", K::Val, R)]),
    (
        "DISCARD",
        &[
            ("player", K::Plr, R),
            ("count", K::Val, R),
            ("choice", K::Ch, O),
        ],
    ),
    (
        "MOVE",
        &[
            ("objects", K::Obj, R),
            ("toZone", K::Zone, R),
            ("position", K::Pos, O),
            ("faceDown", K::Bool, O),
            ("tapped", K::Bool, O),
            ("underControlOf", K::Plr, O),
            ("transformed", K::Bool, O),
            ("choice", K::Ch, O),
        ],
    ),
    (
        "SEARCH_LIBRARY",
        &[
            ("player", K::Plr, R),
            ("selector", K::Obj, R),
            ("count", K::Val, R),
            ("toZone", K::Zone, R),
            ("position", K::Pos, O),
            ("choice", K::Ch, O),
        ],
    ),
    ("SHUFFLE", &[("player", K::Plr, R)]),
    (
        "REVEAL",
        &[
            ("objects", K::Obj, R),
            ("to", K::Plr, O),
            ("choice", K::Ch, O),
        ],
    ),
    ("LOOK_AT", &[("objects", K::Obj, R), ("player", K::Plr, R)]),
    (
        "PUT_ON_TOP_OR_BOTTOM",
        &[
            ("objects", K::Obj, R),
            ("player", K::Plr, R),
            ("choice", K::Ch, O),
        ],
    ),
    (
        "EXILE_UNTIL",
        &[
            ("objects", K::Obj, R),
            ("duration", K::Dur, R),
            ("faceDown", K::Bool, O),
        ],
    ),
    (
        "PLAY_PERMISSION_GRANT",
        &[
            ("objects", K::Obj, R),
            ("duration", K::Dur, R),
            ("who", K::Plr, R),
            ("alsoAs", K::Str, O),
        ],
    ),
    (
        "DEAL_DAMAGE",
        &[
            ("source", K::Obj, R),
            ("recipients", K::Subj, R),
            ("amount", K::Val, R),
            ("divided", K::Bool, O),
            ("choice", K::Ch, O),
        ],
    ),
    ("FIGHT", &[("first", K::Obj, R), ("second", K::Obj, R)]),
    ("BITE", &[("source", K::Obj, R), ("target", K::Obj, R)]),
    ("GAIN_LIFE", &[("player", K::Plr, R), ("amount", K::Val, R)]),
    ("LOSE_LIFE", &[("player", K::Plr, R), ("amount", K::Val, R)]),
    ("SET_LIFE", &[("player", K::Plr, R), ("amount", K::Val, R)]),
    (
        "PREVENT",
        &[
            ("duration", K::Dur, R),
            ("source", K::Obj, O),
            ("recipients", K::Subj, O),
            ("amount", K::Val, O),
            ("combatOnly", K::Bool, O),
        ],
    ),
    (
        "DESTROY",
        &[("objects", K::Obj, R), ("noRegeneration", K::Bool, O)],
    ),
    (
        "SACRIFICE",
        &[
            ("player", K::Plr, R),
            ("selector", K::Obj, R),
            ("count", K::Val, R),
            ("choice", K::Ch, O),
        ],
    ),
    ("REGENERATE", &[("objects", K::Obj, R)]),
    (
        "COUNTER_SPELL",
        &[("objects", K::Obj, R), ("unless", K::Costs, O)],
    ),
    (
        "PUT_COUNTERS",
        &[
            ("objects", K::Obj, R),
            ("kind", K::Str, R),
            ("count", K::Val, R),
        ],
    ),
    (
        "REMOVE_COUNTERS",
        &[
            ("objects", K::Obj, R),
            ("kind", K::Str, R),
            ("count", K::Val, R),
        ],
    ),
    (
        "MOVE_COUNTERS",
        &[
            ("from", K::Obj, R),
            ("to", K::Obj, R),
            ("kind", K::Str, R),
            ("count", K::Val, R),
        ],
    ),
    ("PROLIFERATE", &[("choice", K::Ch, O)]),
    (
        "CREATE_TOKEN",
        &[
            ("tokenDef", K::Str, R),
            ("count", K::Val, R),
            ("modifiers", K::OpaqueNe, O),
            ("controller", K::Plr, O),
        ],
    ),
    (
        "CREATE_PREDEFINED_TOKEN",
        &[("kind", K::Str, R), ("count", K::Val, O)],
    ),
    (
        "CREATE_COPY_TOKEN",
        &[("of", K::Obj, R), ("except", K::OpaqueNe, O)],
    ),
    ("POPULATE", &[]),
    ("AMASS", &[("n", K::Val, R)]),
    ("INCUBATE", &[("n", K::Val, R)]),
    (
        "COPY_SPELL_OR_ABILITY",
        &[("objects", K::Obj, R), ("newTargets", K::Bool, O)],
    ),
    (
        "CHANGE_TARGETS",
        &[("objects", K::Obj, R), ("choice", K::Ch, O)],
    ),
    (
        "CAST_FROM",
        &[
            ("objects", K::Obj, R),
            ("withoutPaying", K::Bool, O),
            ("altCost", K::Costs, O),
        ],
    ),
    (
        "TAP_OR_UNTAP",
        &[("objects", K::Obj, R), ("action", K::TapAct, R)],
    ),
    ("TRANSFORM", &[("objects", K::Obj, R)]),
    ("CONVERT", &[("objects", K::Obj, R)]),
    (
        "TURN_FACE_UP",
        &[("objects", K::Obj, R), ("profile", K::Str, O)],
    ),
    (
        "TURN_FACE_DOWN",
        &[("objects", K::Obj, R), ("profile", K::Str, O)],
    ),
    ("PHASE_OUT", &[("objects", K::Obj, R)]),
    ("ATTACH", &[("objects", K::Obj, R), ("to", K::Obj, R)]),
    (
        "GAIN_CONTROL",
        &[
            ("objects", K::Obj, R),
            ("duration", K::Dur, R),
            ("player", K::Plr, O),
        ],
    ),
    (
        "EXCHANGE_CONTROL",
        &[("first", K::Obj, R), ("second", K::Obj, R)],
    ),
    ("GOAD", &[("objects", K::Obj, R)]),
    (
        "SET_DESIGNATION",
        &[
            ("kind", K::Str, R),
            ("value", K::Desig, R),
            ("to", K::Subj, O),
        ],
    ),
    ("VENTURE", &[]),
    ("RING_TEMPTS", &[]),
    (
        "INCREASE_SPEED",
        &[("player", K::Plr, R), ("amount", K::Val, O)],
    ),
    ("EMPOWER_JACE", &[("n", K::Val, R)]),
    ("MELD", &[("objects", K::Obj, R)]),
    (
        "MUTATE_MERGE",
        &[("objects", K::Obj, R), ("onto", K::Obj, R)],
    ),
    ("MANIFEST", &[("objects", K::Obj, R)]),
    ("CLOAK", &[("objects", K::Obj, R)]),
    ("EXPLORE", &[("objects", K::Obj, R)]),
    ("CONNIVE", &[("objects", K::Obj, R)]),
    ("DISCOVER", &[("n", K::Val, R)]),
    ("BLIGHT", &[("n", K::Val, R)]),
    ("WATERBEND", &[("cost", K::Costs, R)]),
    (
        "CREATE_CONTINUOUS_EFFECT",
        &[
            ("effects", K::Statics, R),
            ("duration", K::Dur, R),
            ("lockAffectedSet", K::Bool, O),
        ],
    ),
    (
        "CREATE_DELAYED_TRIGGER",
        &[
            ("trigger", K::Trigger, R),
            ("effect", K::Instrs, R),
            ("captures", K::Captures, O),
            ("oneShot", K::Bool, R),
        ],
    ),
    (
        "REFLEXIVE_TRIGGER",
        &[("ifActionTaken", K::Instrs, R), ("effect", K::Instrs, R)],
    ),
    (
        "IF",
        &[
            ("cond", K::Pred, R),
            ("then", K::Instrs, R),
            ("else", K::Instrs, O),
        ],
    ),
    (
        "FOR_EACH",
        &[
            ("iterator", K::Subj, R),
            ("bind", K::Var, O),
            ("body", K::Instrs, R),
        ],
    ),
    ("REPEAT", &[("times", K::Val, R), ("body", K::Instrs, R)]),
    (
        "REPEAT_WHILE",
        &[
            ("cond", K::Pred, R),
            ("maxIter", K::MaxIter, R),
            ("body", K::Instrs, R),
        ],
    ),
    ("MAY", &[("player", K::Plr, R), ("body", K::Instrs, R)]),
    (
        "UNLESS",
        &[
            ("payer", K::Plr, R),
            ("cost", K::Costs, R),
            ("body", K::Instrs, R),
        ],
    ),
    (
        "CHOOSE_MODE",
        &[
            ("chooser", K::Plr, R),
            ("min", K::MinMax, R),
            ("max", K::MinMax, R),
            ("modes", K::Modes, R),
        ],
    ),
    (
        "VOTE",
        &[
            ("voters", K::Plr, R),
            ("choice", K::Ch, R),
            ("store", K::Str, R),
        ],
    ),
    (
        "REMEMBER",
        &[
            ("key", K::Str, R),
            ("into", K::Store, R),
            ("value", K::Val, R),
        ],
    ),
    (
        "NATIVE_EFFECT",
        &[
            ("id", K::Str, R),
            ("version", K::Version, R),
            ("params", K::Opaque, R),
            ("layers", K::Layers, O),
            ("ruleKinds", K::RuleKinds, O),
        ],
    ),
];

fn members_of(op: &str) -> &'static [M] {
    CATALOGUE
        .iter()
        .find(|(n, _)| *n == op)
        .unwrap_or_else(|| panic!("no catalogue entry for {op}"))
        .1
}

/// A fixed valid value of each member type.
fn sample(k: K) -> Value {
    match k {
        K::Plr => json!({"ref":"CONTROLLER"}),
        K::Obj => json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        K::Val => json!({"LITERAL":3}),
        K::Ch => json!({"chooser":{"ref":"CONTROLLER"},"kind":"YES_NO"}),
        K::Dur => json!({"kind":"UNTIL_END_OF_TURN"}),
        K::Subj => json!({"players":"ALL_PLAYERS"}),
        K::Costs => json!([{"cost":"TAP_SOURCE"}]),
        K::Zone => json!("GRAVEYARD"),
        K::Pos => json!("TOP"),
        K::Bool => json!(true),
        K::Str => json!("test_counter"),
        K::Instrs => json!([{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]),
        K::Pred => json!({"pred":"IS_CREATURE"}),
        K::Trigger => json!({"mode":"PHASE_STEP","step":"UPKEEP","whose":{"ref":"CONTROLLER"}}),
        K::Statics => json!([{"type":"RULE","kind":"SKIP"}]),
        K::Captures => json!({"$x":{"ref":"SELF"}}),
        K::Opaque => json!({}),
        K::OpaqueNe => json!({"k1":1}),
        K::Var => json!("$x"),
        K::TapAct => json!("TAP"),
        K::Store => json!("MEMORY"),
        K::Desig => json!(true),
        K::MaxIter => json!(3),
        K::MinMax => json!(1),
        K::Modes => json!([{"id":"m1","effect":[{"op":"SHUFFLE","player":{"ref":"CONTROLLER"}}]}]),
        K::Version => json!(1),
        K::Layers => json!(["6", "1a"]),
        K::RuleKinds => json!(["SKIP", "EVASION"]),
    }
}

/// The minimal valid document of `op` (required members only).
fn minimal(op: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("op".into(), json!(op));
    for (name, k, req) in members_of(op) {
        if *req {
            m.insert((*name).into(), sample(*k));
        }
    }
    m
}

/// The document of `op` using every member (required and optional).
fn full(op: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("op".into(), json!(op));
    for (name, k, _) in members_of(op) {
        m.insert((*name).into(), sample(*k));
    }
    m
}

/// Values of a different JSON type from the sample of `k` (never an arbitrary string, since
/// several string-typed members accept any string and VAL accepts "X").
fn wrong_types(k: K) -> Vec<Value> {
    let valid = &sample(k);
    let mut out = Vec::new();
    for w in [json!(7), json!(true), json!([7]), json!({"zzz":7})] {
        let same = matches!(
            (&w, valid),
            (Value::Number(_), Value::Number(_))
                | (Value::Bool(_), Value::Bool(_))
                | (Value::Array(_), Value::Array(_))
                | (Value::Object(_), Value::Object(_))
        );
        if !same {
            out.push(w);
        }
    }
    // A number or a bool written as a string (a string is a valid designation value).
    if (valid.is_number() || valid.is_boolean()) && k != K::Desig {
        out.push(Value::String(valid.to_string()));
    }
    out
}

// ---------------------------------------------------------------- helpers

/// Decodes `doc` through `from_str`, `from_slice` and `from_value`, checks they agree, that the
/// RFC 8785 form of the decoded value is the canonical form of the document, and that
/// decode(encode(x)) == x.
fn accept(doc: &Value) -> Instruction {
    let text = serde_json::to_string(doc).unwrap();
    let i: Instruction = match serde_json::from_str(&text) {
        Ok(i) => i,
        Err(e) => panic!("expected {text} to decode, got error: {e}"),
    };
    let i2: Instruction = serde_json::from_slice(text.as_bytes()).unwrap();
    let i3: Instruction = serde_json::from_value(doc.clone()).unwrap();
    assert_eq!(i2, i, "from_slice of {text}");
    assert_eq!(i3, i, "from_value of {text}");
    let canon = canonicalize(text.as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(to_canonical_vec(&i).unwrap()).unwrap(),
        String::from_utf8(canon.clone()).unwrap(),
        "canonical bytes of {text}"
    );
    let encoded = serde_json::to_string(&i).unwrap();
    let back: Instruction = serde_json::from_str(&encoded).unwrap();
    assert_eq!(back, i, "decode(encode(x)) of {text}");
    let back2: Instruction = serde_json::from_slice(&canon).unwrap();
    assert_eq!(back2, i);
    i
}

/// Rejected by `from_str`, `from_slice` and `from_value`.
fn reject(doc: &Value) {
    let text = serde_json::to_string(doc).unwrap();
    let r = serde_json::from_str::<Instruction>(&text);
    assert!(r.is_err(), "expected rejection of {text}, got {:?}", r.ok());
    assert!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).is_err(),
        "expected rejection (from_slice) of {text}"
    );
    assert!(
        serde_json::from_value::<Instruction>(doc.clone()).is_err(),
        "expected rejection (from_value) of {text}"
    );
}

fn obj(m: Map<String, Value>) -> Value {
    Value::Object(m)
}

/// The full per-op check of brief requirements 1 and 2.
fn check_op(op: &str) {
    let ms = members_of(op);
    // 1. Minimal document: decodes, name() is the op, canonical form matches.
    let min = minimal(op);
    let i = accept(&obj(min.clone()));
    assert_eq!(i.name(), op);

    // 2a. Every required member dropped.
    for (name, _, req) in ms {
        if *req {
            let mut d = min.clone();
            d.remove(*name);
            reject(&obj(d));
        }
    }
    // 2b. An unknown member added (also a member name from another op).
    for (u, v) in [
        ("zzUnknown", json!(1)),
        ("zzUnknown", json!({})),
        ("Op", json!(op)),
        ("maxIterations", json!(3)),
    ] {
        let mut d = min.clone();
        d.insert(u.into(), v);
        reject(&obj(d));
    }
    // 2c. Each optional member set to null; each member (any) set to null.
    for (name, _, _) in ms {
        let mut d = min.clone();
        d.insert((*name).into(), Value::Null);
        reject(&obj(d));
    }
    // 2d. Wrong member types, required and optional.
    for (name, k, _) in ms {
        for w in wrong_types(*k) {
            let mut d = min.clone();
            d.insert((*name).into(), w);
            reject(&obj(d));
        }
    }
    // 2e. Every optional member present: round-trips; each optional alone too.
    let all = accept(&obj(full(op)));
    assert_eq!(all.name(), op);
    for (name, k, req) in ms {
        if !*req {
            let mut d = min.clone();
            d.insert((*name).into(), sample(*k));
            let one = accept(&obj(d));
            assert_ne!(
                one, i,
                "{op}: optional {name} present must differ from absent"
            );
        }
    }
    // 2f. Explicit false bool flags survive and differ from absent and from true.
    for (name, k, req) in ms {
        if *k == K::Bool {
            let mut f = min.clone();
            f.insert((*name).into(), json!(false));
            let with_false = accept(&obj(f));
            let encoded = serde_json::to_value(&with_false).unwrap();
            assert_eq!(encoded.get(*name), Some(&json!(false)), "{op}.{name}");
            let mut t = min.clone();
            t.insert((*name).into(), json!(true));
            let with_true = accept(&obj(t));
            assert_ne!(with_false, with_true, "{op}.{name}");
            if !*req {
                assert_ne!(with_false, i, "{op}.{name}: false differs from absent");
            }
        }
    }
    // The op tag is case-sensitive.
    let mut d = min.clone();
    d.insert("op".into(), json!(op.to_lowercase()));
    reject(&obj(d));
}

macro_rules! per_op_tests {
    ($($test:ident => $op:literal,)*) => {
        $(
            /// REQ-DSL-010 (brief requirements 1 and 2): minimal document decodes with
            /// `name()` == op and canonical bytes == canonicalize(doc); dropped required,
            /// unknown, null and wrong-typed members are rejected; the every-member document
            /// round-trips; explicit `false` flags survive.
            #[test]
            fn $test() {
                check_op($op);
            }
        )*
        /// REQ-DSL-010: the per-op test list covers exactly the 72 catalogue names.
        #[test]
        fn req_dsl_010_per_op_tests_cover_catalogue() {
            let tested: Vec<&str> = vec![$($op),*];
            assert_eq!(tested, NAMES.to_vec());
        }
    };
}

per_op_tests! {
    req_dsl_010_op_draw => "DRAW",
    req_dsl_010_op_mill => "MILL",
    req_dsl_010_op_discard => "DISCARD",
    req_dsl_010_op_move => "MOVE",
    req_dsl_010_op_search_library => "SEARCH_LIBRARY",
    req_dsl_010_op_shuffle => "SHUFFLE",
    req_dsl_010_op_reveal => "REVEAL",
    req_dsl_010_op_look_at => "LOOK_AT",
    req_dsl_010_op_put_on_top_or_bottom => "PUT_ON_TOP_OR_BOTTOM",
    req_dsl_010_op_exile_until => "EXILE_UNTIL",
    req_dsl_010_op_play_permission_grant => "PLAY_PERMISSION_GRANT",
    req_dsl_010_op_deal_damage => "DEAL_DAMAGE",
    req_dsl_010_op_fight => "FIGHT",
    req_dsl_010_op_bite => "BITE",
    req_dsl_010_op_gain_life => "GAIN_LIFE",
    req_dsl_010_op_lose_life => "LOSE_LIFE",
    req_dsl_010_op_set_life => "SET_LIFE",
    req_dsl_010_op_prevent => "PREVENT",
    req_dsl_010_op_destroy => "DESTROY",
    req_dsl_010_op_sacrifice => "SACRIFICE",
    req_dsl_010_op_regenerate => "REGENERATE",
    req_dsl_010_op_counter_spell => "COUNTER_SPELL",
    req_dsl_010_op_put_counters => "PUT_COUNTERS",
    req_dsl_010_op_remove_counters => "REMOVE_COUNTERS",
    req_dsl_010_op_move_counters => "MOVE_COUNTERS",
    req_dsl_010_op_proliferate => "PROLIFERATE",
    req_dsl_010_op_create_token => "CREATE_TOKEN",
    req_dsl_010_op_create_predefined_token => "CREATE_PREDEFINED_TOKEN",
    req_dsl_010_op_create_copy_token => "CREATE_COPY_TOKEN",
    req_dsl_010_op_populate => "POPULATE",
    req_dsl_010_op_amass => "AMASS",
    req_dsl_010_op_incubate => "INCUBATE",
    req_dsl_010_op_copy_spell_or_ability => "COPY_SPELL_OR_ABILITY",
    req_dsl_010_op_change_targets => "CHANGE_TARGETS",
    req_dsl_010_op_cast_from => "CAST_FROM",
    req_dsl_010_op_tap_or_untap => "TAP_OR_UNTAP",
    req_dsl_010_op_transform => "TRANSFORM",
    req_dsl_010_op_convert => "CONVERT",
    req_dsl_010_op_turn_face_up => "TURN_FACE_UP",
    req_dsl_010_op_turn_face_down => "TURN_FACE_DOWN",
    req_dsl_010_op_phase_out => "PHASE_OUT",
    req_dsl_010_op_attach => "ATTACH",
    req_dsl_010_op_gain_control => "GAIN_CONTROL",
    req_dsl_010_op_exchange_control => "EXCHANGE_CONTROL",
    req_dsl_010_op_goad => "GOAD",
    req_dsl_010_op_set_designation => "SET_DESIGNATION",
    req_dsl_010_op_venture => "VENTURE",
    req_dsl_010_op_ring_tempts => "RING_TEMPTS",
    req_dsl_010_op_increase_speed => "INCREASE_SPEED",
    req_dsl_010_op_empower_jace => "EMPOWER_JACE",
    req_dsl_010_op_meld => "MELD",
    req_dsl_010_op_mutate_merge => "MUTATE_MERGE",
    req_dsl_010_op_manifest => "MANIFEST",
    req_dsl_010_op_cloak => "CLOAK",
    req_dsl_010_op_explore => "EXPLORE",
    req_dsl_010_op_connive => "CONNIVE",
    req_dsl_010_op_discover => "DISCOVER",
    req_dsl_010_op_blight => "BLIGHT",
    req_dsl_010_op_waterbend => "WATERBEND",
    req_dsl_010_op_create_continuous_effect => "CREATE_CONTINUOUS_EFFECT",
    req_dsl_010_op_create_delayed_trigger => "CREATE_DELAYED_TRIGGER",
    req_dsl_010_op_reflexive_trigger => "REFLEXIVE_TRIGGER",
    req_dsl_010_op_if => "IF",
    req_dsl_010_op_for_each => "FOR_EACH",
    req_dsl_010_op_repeat => "REPEAT",
    req_dsl_010_op_repeat_while => "REPEAT_WHILE",
    req_dsl_010_op_may => "MAY",
    req_dsl_010_op_unless => "UNLESS",
    req_dsl_010_op_choose_mode => "CHOOSE_MODE",
    req_dsl_010_op_vote => "VOTE",
    req_dsl_010_op_remember => "REMEMBER",
    req_dsl_015_op_native_effect => "NATIVE_EFFECT",
}

/// REQ-DSL-010 (requirement 1): `Instruction::NAMES` is exactly the 72 catalogue names, in
/// order, all distinct; the catalogue table of this file matches too.
#[test]
fn req_dsl_010_names_are_the_catalogue_in_order() {
    assert_eq!(Instruction::NAMES.len(), 72);
    assert_eq!(Instruction::NAMES.to_vec(), NAMES.to_vec());
    let set: BTreeSet<&str> = Instruction::NAMES.iter().copied().collect();
    assert_eq!(set.len(), 72);
    let table: Vec<&str> = CATALOGUE.iter().map(|(n, _)| *n).collect();
    assert_eq!(table, NAMES.to_vec());
}

/// REQ-DSL-010 (A6): a unit-variant op (no members) is written `{"op":NAME}` only, and
/// `Instruction::Populate` encodes so.
#[test]
fn req_dsl_010_unit_ops_encode_as_op_only() {
    assert_eq!(
        serde_json::to_value(&Instruction::Populate).unwrap(),
        json!({"op":"POPULATE"})
    );
    for op in ["POPULATE", "VENTURE", "RING_TEMPTS"] {
        let i = accept(&json!({ "op": op }));
        assert_eq!(serde_json::to_value(&i).unwrap(), json!({ "op": op }));
    }
}

// ---------------------------------------------------------------- 3. REPEAT_WHILE maxIter

fn repeat_while_doc(max_iter: Option<Value>) -> Value {
    let mut m = minimal("REPEAT_WHILE");
    m.remove("maxIter");
    if let Some(v) = max_iter {
        m.insert("maxIter".into(), v);
    }
    Value::Object(m)
}

/// REQ-DSL-010 (requirement 3, issue acceptance criterion; SRS §14.4.5 "Loop safety"):
/// `REPEAT_WHILE` without a valid `maxIter` cannot be deserialized; 1 and 4294967295 are the
/// accepted bounds.
#[test]
fn req_dsl_010_repeat_while_requires_max_iter() {
    reject(&repeat_while_doc(None));
    for bad in [
        json!(0),
        json!(-1),
        json!(4_294_967_296_u64),
        json!("5"),
        Value::Null,
        json!(true),
        json!(1.0),
        json!(100.0),
    ] {
        reject(&repeat_while_doc(Some(bad)));
    }
    // Float spellings that only exist as text.
    let base = r#"{"op":"REPEAT_WHILE","cond":{"pred":"IS_CREATURE"},"maxIter":<<V>>,"body":[{"op":"SHUFFLE","player":{"ref":"CONTROLLER"}}]}"#;
    for bad in ["1.0", "1e2", "1E2", "-0", "5.5"] {
        let t = base.replace("<<V>>", bad);
        assert!(serde_json::from_str::<Instruction>(&t).is_err(), "{t}");
        assert!(
            serde_json::from_slice::<Instruction>(t.as_bytes()).is_err(),
            "{t}"
        );
    }
    for good in [1_u64, 4_294_967_295] {
        let i = accept(&repeat_while_doc(Some(json!(good))));
        let Instruction::RepeatWhile { max_iter, .. } = &i else {
            panic!("expected RepeatWhile, got {i:?}");
        };
        assert_eq!(u64::from(max_iter.get()), good);
    }
}

fn cond() -> PredicateAST {
    serde_json::from_value(json!({"pred":"IS_CREATURE"})).unwrap()
}

fn draw() -> Instruction {
    let player: PlayerSelectorAST = serde_json::from_value(json!({"ref":"CONTROLLER"})).unwrap();
    let count: ValueExpressionAST = serde_json::from_value(json!({"LITERAL":1})).unwrap();
    Instruction::Draw { player, count }
}

/// REQ-DSL-010 (requirement 3): a `RepeatWhile` value cannot be constructed without a maximum.
/// The missing-field case is guaranteed by the type: `max_iter` is a `NonZeroU32`, so neither
/// "absent" nor 0 is representable. A hand-built value encodes with `"maxIter":3`.
#[test]
fn req_dsl_010_repeat_while_constructed_with_non_zero_max_iter() {
    let rw = Instruction::RepeatWhile {
        cond: cond(),
        max_iter: NonZeroU32::new(3).unwrap(),
        body: vec![draw()],
    };
    let v = serde_json::to_value(&rw).unwrap();
    assert_eq!(
        v,
        json!({"op":"REPEAT_WHILE","cond":{"pred":"IS_CREATURE"},"maxIter":3,
               "body":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]})
    );
    let back: Instruction = serde_json::from_value(v).unwrap();
    assert_eq!(back, rw);
    assert_eq!(rw.name(), "REPEAT_WHILE");
}

/// REQ-DSL-010 (requirement 3): the encoded form always carries `maxIter`, equal to the field,
/// across the whole NonZeroU32 range.
#[test]
fn req_dsl_010_repeat_while_encoding_always_carries_max_iter() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F13_0000_0000_0003);
    let mut values: Vec<u32> = vec![1, 2, 3, 1000, u32::MAX - 1, u32::MAX];
    for _ in 0..64 {
        values.push(1 + below(&mut rng, u64::from(u32::MAX)) as u32);
    }
    for n in values {
        let rw = Instruction::RepeatWhile {
            cond: cond(),
            max_iter: NonZeroU32::new(n).unwrap(),
            body: vec![Instruction::Populate],
        };
        let v = serde_json::to_value(&rw).unwrap();
        assert_eq!(v.get("maxIter"), Some(&json!(n)), "maxIter {n}");
        let back: Instruction = serde_json::from_value(v).unwrap();
        assert_eq!(back, rw);
    }
}

// ---------------------------------------------------------------- 9. generator

fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

fn coin(rng: &mut Pcg64Dxsm) -> bool {
    below(rng, 2) == 0
}

fn pick<'a>(rng: &mut Pcg64Dxsm, items: &'a [&'a str]) -> &'a str {
    items[below(rng, items.len() as u64) as usize]
}

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

const STEPS: [&str; 13] = [
    "UNTAP",
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
];

const LAYERS: [&str; 11] = ["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"];

const RULE_KINDS: [&str; 10] = [
    "COST_INCREASE",
    "CAST_PERMISSION",
    "PROHIBITION",
    "MAX_HAND_SIZE",
    "EVASION",
    "SKIP",
    "UNTAP_RULE",
    "DRAW_RULE",
    "ZONE_RULE",
    "SBA_RULE",
];

const DURATION_KINDS: [&str; 9] = [
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

/// Deepest instruction nesting the generator produces (brief requirement 9: <= 12).
const MAX_GEN_DEPTH: u32 = 12;

fn gen_plr(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 5) {
        0 => json!({"ref":"CONTROLLER"}),
        1 => json!({"players":"ALL_PLAYERS"}),
        2 => json!({"players":"TEAMMATES"}),
        3 => json!({"ref":"EACH_OPPONENT"}),
        _ => json!({"ref":"ACTIVE_PLAYER"}),
    }
}

fn gen_obj(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 5) {
        0 => json!({"ref":"SELF"}),
        1 => json!({"ref":"TARGET","slot":below(rng, 4)}),
        2 => json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        3 => json!({"pred":"IS_CREATURE"}),
        _ => json!({"zones":[pick(rng, &ZONES)]}),
    }
}

fn gen_val(rng: &mut Pcg64Dxsm) -> Value {
    if below(rng, 4) == 0 {
        json!("X")
    } else {
        json!({"LITERAL": below(rng, 30) as i64 - 5})
    }
}

fn gen_pred(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 4) {
        0 => json!({"pred":"IS_CREATURE"}),
        1 => json!({"pred":"TAPPED"}),
        2 => json!({"op":"NOT","condition":{"pred":"TAPPED"}}),
        _ => json!({"op":"AND","conditions":[{"pred":"IS_CREATURE"},
                {"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}}]}),
    }
}

fn gen_ch(rng: &mut Pcg64Dxsm) -> Value {
    let kind = pick(rng, &["YES_NO", "COLOR", "NUMBER", "MODE", "PLAYER"]);
    json!({"chooser":gen_plr(rng),"kind":kind})
}

fn gen_dur(rng: &mut Pcg64Dxsm) -> Value {
    match pick(rng, &DURATION_KINDS) {
        "UNTIL" => json!({"kind":"UNTIL","step":pick(rng, &STEPS),"whose":gen_plr(rng)}),
        "FOR_AS_LONG_AS" => json!({"kind":"FOR_AS_LONG_AS","predicate":gen_pred(rng)}),
        "WHILE_CONTROLLED_BY" => json!({"kind":"WHILE_CONTROLLED_BY","player":gen_plr(rng)}),
        k => json!({ "kind": k }),
    }
}

fn gen_subj(rng: &mut Pcg64Dxsm) -> Value {
    if coin(rng) {
        json!({"players": pick(rng, &["ALL_PLAYERS", "TEAMMATES"])})
    } else {
        gen_obj(rng)
    }
}

fn gen_costs(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(match below(rng, 4) {
            0 => json!({"cost":"TAP_SOURCE"}),
            1 => json!({"cost":"PAY_LIFE","amount":gen_val(rng)}),
            2 => json!({"cost":"PAY_MANA","mana":"{2}"}),
            _ => json!({"cost":"FORAGE"}),
        });
    }
    Value::Array(out)
}

fn gen_str(rng: &mut Pcg64Dxsm) -> Value {
    json!(pick(rng, &["test_counter", "k1", "k2", "test_s", "x"]))
}

fn gen_scalar(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 5) {
        0 => json!(below(rng, 2000) as i64 - 1000),
        1 => json!(coin(rng)),
        2 => json!("v"),
        3 => json!(9_007_199_254_740_991_i64),
        _ => json!(""),
    }
}

fn gen_opaque(rng: &mut Pcg64Dxsm, min: u64) -> Value {
    let n = min + below(rng, 4);
    let mut m = Map::new();
    for i in 0..n {
        let v = match below(rng, 4) {
            0 => json!([gen_scalar(rng), gen_scalar(rng)]),
            1 => json!({"inner": gen_scalar(rng), "": []}),
            _ => gen_scalar(rng),
        };
        m.insert(format!("k{i}"), v);
    }
    Value::Object(m)
}

fn gen_statics(rng: &mut Pcg64Dxsm) -> Value {
    let n = 1 + below(rng, 3);
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(match below(rng, 3) {
            0 => json!({"type":"CHAR","layer":pick(rng, &LAYERS),"affected":gen_obj(rng),
                        "modification":{"op":"TEST_OP"}}),
            1 => json!({"type":"PLAYER","affected":gen_plr(rng),"modification":{"op":"TEST_OP"}}),
            _ => json!({"type":"RULE","kind":pick(rng, &RULE_KINDS)}),
        });
    }
    Value::Array(out)
}

fn gen_trigger(rng: &mut Pcg64Dxsm) -> Value {
    match below(rng, 3) {
        0 => json!({"mode":"PHASE_STEP","step":pick(rng, &STEPS),"whose":gen_plr(rng)}),
        1 => json!({"mode":"EVENT","event":{"kind":"DIES"}}),
        _ => json!({"mode":"STATE","state":gen_pred(rng)}),
    }
}

fn gen_distinct(rng: &mut Pcg64Dxsm, items: &[&str]) -> Value {
    let mut v: Vec<&str> = items.to_vec();
    rng.shuffle(&mut v);
    v.truncate(1 + below(rng, 4) as usize);
    json!(v)
}

struct Gen<'a> {
    rng: &'a mut Pcg64Dxsm,
    seen: BTreeSet<&'static str>,
    max_depth_seen: u32,
}

impl Gen<'_> {
    fn instrs(&mut self, depth: u32) -> Value {
        let n = 1 + below(self.rng, 3);
        Value::Array((0..n).map(|_| self.instr(depth)).collect())
    }

    fn modes(&mut self, depth: u32) -> (Value, u64) {
        let n = 1 + below(self.rng, 3);
        let mut out = Vec::new();
        for i in 0..n {
            let mut m = Map::new();
            m.insert("id".into(), json!(format!("m{i}")));
            if coin(self.rng) {
                let mut slots: Vec<u64> = (0..6).collect();
                self.rng.shuffle(&mut slots);
                let k = 1 + below(self.rng, 3) as usize;
                let ts: Vec<Value> = slots[..k]
                    .iter()
                    .map(|s| json!({"slot":s,"selector":{"ref":"SELF"},"min":0,"max":1}))
                    .collect();
                m.insert("targets".into(), Value::Array(ts));
            }
            m.insert("effect".into(), self.instrs(depth + 1));
            out.push(Value::Object(m));
        }
        (Value::Array(out), n)
    }

    fn member(&mut self, k: K, depth: u32) -> Value {
        let rng = &mut *self.rng;
        match k {
            K::Plr => gen_plr(rng),
            K::Obj => gen_obj(rng),
            K::Val => gen_val(rng),
            K::Ch => gen_ch(rng),
            K::Dur => gen_dur(rng),
            K::Subj => gen_subj(rng),
            K::Costs => gen_costs(rng),
            K::Zone => json!(pick(rng, &ZONES)),
            K::Pos => json!(pick(rng, &["TOP", "BOTTOM"])),
            K::Bool | K::Desig => json!(coin(rng)),
            K::Str => gen_str(rng),
            K::Instrs => self.instrs(depth + 1),
            K::Pred => gen_pred(rng),
            K::Trigger => gen_trigger(rng),
            K::Statics => gen_statics(rng),
            K::Captures => {
                let n = 1 + below(rng, 3);
                let mut m = Map::new();
                for i in 0..n {
                    m.insert(format!("$c_{i}"), gen_obj(rng));
                }
                Value::Object(m)
            }
            K::Opaque => gen_opaque(rng, 0),
            K::OpaqueNe => gen_opaque(rng, 1),
            K::Var => json!(pick(rng, &["$x", "$Y_1", "$_", "$abc9"])),
            K::TapAct => json!(pick(rng, &["TAP", "UNTAP"])),
            K::Store => json!(pick(rng, &["MEMORY", "LINKED"])),
            K::MaxIter => match below(rng, 3) {
                0 => json!(u32::MAX),
                _ => json!(1 + below(rng, 1000)),
            },
            K::MinMax => json!(0),
            K::Modes => unreachable!("modes are generated with min/max"),
            K::Version => json!(1 + below(rng, u64::from(u32::MAX))),
            K::Layers => gen_distinct(rng, &LAYERS),
            K::RuleKinds => gen_distinct(rng, &RULE_KINDS),
        }
    }

    /// A random valid instruction at instruction-nesting `depth` (1 = top level).
    fn instr(&mut self, depth: u32) -> Value {
        self.max_depth_seen = self.max_depth_seen.max(depth);
        let mut op = NAMES[below(self.rng, 72) as usize];
        let nests = members_of(op)
            .iter()
            .any(|(_, k, _)| matches!(k, K::Instrs | K::Modes));
        if nests && depth >= MAX_GEN_DEPTH {
            op = pick(
                self.rng,
                &["DRAW", "POPULATE", "SET_DESIGNATION", "NATIVE_EFFECT"],
            );
        }
        let op: &'static str = NAMES.iter().copied().find(|n| *n == op).unwrap();
        self.seen.insert(op);
        let mut m = Map::new();
        m.insert("op".into(), json!(op));
        for (name, k, req) in members_of(op) {
            if *k == K::MinMax || *k == K::Modes {
                continue;
            }
            if *req || coin(self.rng) {
                let v = self.member(*k, depth);
                m.insert((*name).into(), v);
            }
        }
        match op {
            "CHOOSE_MODE" => {
                let (modes, n) = self.modes(depth);
                let max = below(self.rng, n + 1);
                let min = below(self.rng, max + 1);
                m.insert("min".into(), json!(min));
                m.insert("max".into(), json!(max));
                m.insert("modes".into(), modes);
            }
            "CREATE_TOKEN" if coin(self.rng) => {
                m.remove("tokenDef");
                let inline = gen_opaque(self.rng, 0);
                m.insert("inline".into(), inline);
            }
            "REMEMBER" if coin(self.rng) => {
                m.remove("value");
                let o = gen_obj(self.rng);
                m.insert("objects".into(), o);
            }
            "SET_DESIGNATION" if coin(self.rng) => {
                m.insert("value".into(), json!("test_designation_name"));
            }
            _ => {}
        }
        Value::Object(m)
    }
}

/// Writes `v` as JSON text with object members in a random order.
fn write_shuffled(v: &Value, rng: &mut Pcg64Dxsm, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            rng.shuffle(&mut keys);
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_shuffled(&m[k.as_str()], rng, out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_shuffled(x, rng, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap()),
    }
}

const SEEDS: [u64; 4] = [
    0x0F13_0000_0000_0001,
    0x0F13_0000_0000_0002,
    0x0F13_0000_0000_0003,
    0x0F13_DEAD_BEEF_0004,
];

const TREES_PER_SEED: usize = 500;

/// REQ-DSL-010 (requirement 9, issue verification "round-trip property tests over generated
/// instruction trees"): for 500 generated trees per seed over four fixed seeds, using all 72 ops
/// with instruction nesting <= 12: decode succeeds through all three entry points;
/// decode(encode(x)) == x; `to_canonical_vec` equals the canonical form of the source and is
/// stable; member order does not matter; encoder output re-decodes.
#[test]
fn req_dsl_010_generated_trees_round_trip() {
    let mut seen_all: BTreeSet<&'static str> = BTreeSet::new();
    let mut deepest = 0;
    for seed in SEEDS {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut shuffle_rng = Pcg64Dxsm::from_u64_seed(seed ^ 0x5555);
        let mut g = Gen {
            rng: &mut rng,
            seen: BTreeSet::new(),
            max_depth_seen: 0,
        };
        for _ in 0..TREES_PER_SEED {
            let doc = g.instr(1);
            let text = serde_json::to_string(&doc).unwrap();
            let x: Instruction = match serde_json::from_str(&text) {
                Ok(x) => x,
                Err(e) => panic!("seed {seed:#x}: {text} should decode: {e}"),
            };
            let xs: Instruction = serde_json::from_slice(text.as_bytes()).unwrap();
            let xv: Instruction = serde_json::from_value(doc.clone()).unwrap();
            assert_eq!(xs, x);
            assert_eq!(xv, x);
            let canon = canonicalize(text.as_bytes()).unwrap();
            let c1 = to_canonical_vec(&x).unwrap();
            assert_eq!(c1, canon, "canonical of {text}");
            assert_eq!(to_canonical_vec(&x.clone()).unwrap(), c1, "stable");
            let enc = serde_json::to_string(&x).unwrap();
            let y: Instruction = serde_json::from_str(&enc).unwrap();
            assert_eq!(y, x, "decode(encode(x))");
            assert_eq!(to_canonical_vec(&y).unwrap(), c1);
            let mut shuffled = String::new();
            write_shuffled(&doc, &mut shuffle_rng, &mut shuffled);
            let z: Instruction = serde_json::from_str(&shuffled).unwrap();
            assert_eq!(z, x, "member order of {shuffled}");
        }
        assert!(g.max_depth_seen <= MAX_GEN_DEPTH);
        deepest = deepest.max(g.max_depth_seen);
        seen_all.extend(g.seen.iter().copied());
        // Every op appears for each seed.
        assert_eq!(g.seen.len(), 72, "seed {seed:#x} covered {:?}", g.seen);
    }
    assert_eq!(seen_all.len(), 72);
    assert!(deepest >= 4, "generator should nest, deepest {deepest}");
}

// ---------------------------------------------------------------- 10. never panic (R12)

fn try_all(bytes: &[u8]) {
    let _ = serde_json::from_slice::<Instruction>(bytes);
    let _ = serde_json::from_slice::<DurationAST>(bytes);
    let _ = serde_json::from_slice::<NativeEffectAST>(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        let _ = serde_json::from_str::<Instruction>(s);
        let _ = serde_json::from_str::<DurationAST>(s);
        let _ = serde_json::from_str::<NativeEffectAST>(s);
    }
    if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
        let _ = serde_json::from_value::<Instruction>(v.clone());
        let _ = serde_json::from_value::<DurationAST>(v.clone());
        let _ = serde_json::from_value::<NativeEffectAST>(v);
    }
}

/// Every successful decode of `T` re-encodes and decodes back equal; decode errors are allowed.
fn roundtrip_ok<T>(r: Result<T, serde_json::Error>)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    if let Ok(x) = r {
        let enc = serde_json::to_string(&x).expect("a decoded value must re-encode");
        assert_eq!(serde_json::from_str::<T>(&enc).unwrap(), x);
    }
}

fn roundtrip_type<T>(bytes: &[u8])
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    roundtrip_ok(serde_json::from_slice::<T>(bytes));
    if let Ok(s) = std::str::from_utf8(bytes) {
        roundtrip_ok(serde_json::from_str::<T>(s));
    }
}

/// Like `try_all`, and every successful decode of each supported type re-encodes and decodes
/// back equal.
fn try_roundtrip(bytes: &[u8]) {
    try_all(bytes);
    roundtrip_type::<Instruction>(bytes);
    roundtrip_type::<DurationAST>(bytes);
    roundtrip_type::<NativeEffectAST>(bytes);
}

/// R12, REQ-DSL-010, REQ-DSL-015 (requirement 10): bounded random bytes never panic the
/// `Instruction`, `DurationAST` or `NativeEffectAST` decoders.
#[test]
fn req_dsl_010_random_bytes_never_panic() {
    const ALPHABET: &[u8] = b"{}[]\":,0123456789-.eE truefalsnul\\uOPDRAWIFKNX$_";
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F13_0000_0000_0010);
    for _ in 0..3000 {
        let len = below(&mut rng, 200) as usize;
        let bytes: Vec<u8> = if coin(&mut rng) {
            (0..len).map(|_| below(&mut rng, 256) as u8).collect()
        } else {
            (0..len)
                .map(|_| ALPHABET[below(&mut rng, ALPHABET.len() as u64) as usize])
                .collect()
        };
        try_all(&bytes);
    }
}

fn mutate_value(v: &mut Value, rng: &mut Pcg64Dxsm) {
    match v {
        Value::Object(m) if !m.is_empty() && below(rng, 3) != 0 => {
            let keys: Vec<String> = m.keys().cloned().collect();
            let k = keys[below(rng, keys.len() as u64) as usize].clone();
            match below(rng, 6) {
                0 => {
                    m.remove(&k);
                }
                1 => {
                    m.insert(k, Value::Null);
                }
                2 => {
                    m.insert("zz".into(), json!(1));
                }
                3 => {
                    let r = gen_scalar(rng);
                    m.insert(k, r);
                }
                _ => mutate_value(m.get_mut(&k).unwrap(), rng),
            }
        }
        Value::Array(a) if !a.is_empty() && below(rng, 3) != 0 => {
            let i = below(rng, a.len() as u64) as usize;
            match below(rng, 4) {
                0 => {
                    a.remove(i);
                }
                1 => {
                    let c = a[i].clone();
                    a.push(c);
                }
                _ => mutate_value(&mut a[i], rng),
            }
        }
        _ => *v = gen_scalar(rng),
    }
}

/// R12, REQ-DSL-010, REQ-DSL-015 (requirement 10): mutated valid documents (structural edits
/// and byte-level edits) never panic the decoders; whatever decodes re-encodes.
#[test]
fn req_dsl_010_mutated_documents_never_panic() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0F13_0000_0000_0011);
    let mut gen_rng = Pcg64Dxsm::from_u64_seed(0x0F13_0000_0000_0012);
    let mut g = Gen {
        rng: &mut gen_rng,
        seen: BTreeSet::new(),
        max_depth_seen: 0,
    };
    for round in 0..1500 {
        let mut doc = match round % 3 {
            0 => g.instr(1),
            1 => gen_dur(g.rng),
            _ => {
                let mut m = minimal("NATIVE_EFFECT");
                m.insert("layers".into(), gen_distinct(g.rng, &LAYERS));
                Value::Object(m)
            }
        };
        let original = serde_json::to_string(&doc).unwrap();
        let edits = 1 + below(&mut rng, 3);
        for _ in 0..edits {
            mutate_value(&mut doc, &mut rng);
        }
        let text = serde_json::to_string(&doc).unwrap();
        try_roundtrip(text.as_bytes());
        // Byte-level edits of the valid text.
        let mut bytes = original.into_bytes();
        for _ in 0..1 + below(&mut rng, 4) {
            if bytes.is_empty() {
                break;
            }
            let i = below(&mut rng, bytes.len() as u64) as usize;
            match below(&mut rng, 3) {
                0 => bytes[i] = below(&mut rng, 256) as u8,
                1 => {
                    bytes.remove(i);
                }
                _ => bytes.truncate(i),
            }
        }
        try_roundtrip(&bytes);
    }
}
