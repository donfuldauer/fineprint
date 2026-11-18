// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::ability` (FP-012 wire contract), part 1: the SRS §14.4.1–14.4.4
//! examples (REQ-DSL-004..009, REQ-NFR-006), the closed wire sets, the ability envelope
//! (REQ-DSL-005), triggers and event matchers with the look-back ban (REQ-DSL-006), and SPELL /
//! KEYWORD abilities, targets, modes and effect instructions (REQ-DSL-005, REQ-DSL-010
//! placeholder). Black-box: written from the work-item brief only; everything is driven through
//! serde_json and `fineprint_dsl::canonical`. Synthetic data only (R5).

use std::collections::BTreeSet;
use std::fmt::Debug;

use fineprint_dsl::ability::{
    AbilityAST, ActivationTiming, EffectInstructionAST, EventBatch, EventKind, EventMatcher, Layer,
    ModeSetAST, RuleKind, StepId, TargetSlotAST, TierHint,
};
use fineprint_dsl::canonical::{canonicalize, to_canonical_vec};
use fineprint_dsl::mana::ManaCostDSL;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

// ---------------------------------------------------------------- vocabulary from the contract

const EVENT_KINDS: [&str; 57] = [
    "ZONE_CHANGE",
    "ENTERS",
    "DIES",
    "LEAVES_BATTLEFIELD",
    "PUT_INTO_GRAVEYARD",
    "LEAVES_GRAVEYARD",
    "EXILED",
    "RETURNED_TO_HAND",
    "DRAWN",
    "DISCARDED",
    "MILLED",
    "CYCLED",
    "SACRIFICED",
    "DESTROYED",
    "CAST",
    "COPIED",
    "ACTIVATED",
    "TRIGGERED",
    "RESOLVED",
    "COUNTERED",
    "BECAME_TARGET",
    "ATTACKS",
    "ATTACKS_ALONE",
    "BLOCKS",
    "BECOMES_BLOCKED",
    "ATTACKER_UNBLOCKED",
    "DAMAGE_DEALT",
    "DAMAGE_DEALT_BATCH",
    "LIFE_GAINED",
    "LIFE_LOST",
    "COUNTERS_PUT",
    "COUNTERS_REMOVED",
    "TAPPED",
    "UNTAPPED",
    "TRANSFORMED",
    "CONVERTED",
    "TURNED_FACE_UP",
    "PHASED_OUT",
    "PHASED_IN",
    "ATTACHED",
    "UNATTACHED",
    "CONTROL_CHANGED",
    "TOKEN_CREATED",
    "SEARCHED_LIBRARY",
    "SHUFFLED",
    "SCRY",
    "SURVEIL",
    "EXPLORE",
    "VENTURE",
    "BLIGHT",
    "WATERBEND",
    "DESIGNATION_GAINED",
    "ROLLED_DIE",
    "FLIPPED_COIN",
    "PLAYER_LOST",
    "PLANESWALKED",
    "SPECIAL_ACTION_TAKEN",
];

const RULE_KINDS: [&str; 34] = [
    "COST_INCREASE",
    "COST_REDUCTION",
    "COST_MINIMUM",
    "COST_SET",
    "ALT_COST_GRANT",
    "ADDITIONAL_COST_REQUIRE",
    "PAYMENT_METHOD",
    "CAST_PERMISSION",
    "PLAY_PERMISSION",
    "TIMING_PERMISSION",
    "PROHIBITION",
    "ADDITIONAL_LAND_PLAY",
    "MAX_HAND_SIZE",
    "ATTACK_RESTRICTION",
    "ATTACK_REQUIREMENT",
    "ATTACK_COST",
    "BLOCK_RESTRICTION",
    "BLOCK_REQUIREMENT",
    "BLOCK_COST",
    "BLOCK_CAPACITY",
    "EVASION",
    "HASTE_AS_THOUGH",
    "ATTACK_AS_THOUGH",
    "DAMAGE_ASSIGNMENT",
    "TARGETING_RULE",
    "SKIP",
    "UNTAP_RULE",
    "MANA_RULE",
    "LIFE_RULE",
    "DRAW_RULE",
    "ZONE_RULE",
    "WIN_LOSE_RULE",
    "SBA_RULE",
    "STATION_SUBSTITUTE",
];

const LAYERS: [&str; 11] = ["1a", "1b", "2", "3", "4", "5", "6", "7a", "7b", "7c", "7d"];

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

const TIMINGS: [&str; 5] = [
    "INSTANT",
    "SORCERY",
    "YOUR_TURN",
    "OPPONENT_TURN",
    "COMBAT_ONLY",
];

const TIER_HINTS: [&str; 3] = ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"];

const BATCHES: [&str; 2] = ["EACH", "ONE_OR_MORE"];

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

const KINDS: [&str; 6] = [
    "SPELL",
    "ACTIVATED",
    "TRIGGERED",
    "STATIC",
    "REPLACEMENT",
    "KEYWORD",
];

// ---------------------------------------------------------------- SRS examples (verbatim)

const EX1: &str = r#"{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],"trigger":{"mode":"EVENT","event":{"kind":"DAMAGE_DEALT","subject":{"ref":"SELF"},"filters":{"combat":true,"recipient":{"pred":"IS_PLAYER"}},"bind":{"recipient":"$P"}}},"effect":[{"op":"DISCARD","player":"$P","count":{"LITERAL":1},"chooser":"$P"}]}"#;
const EX2: &str = r#"{"abilityId":"a1","kind":"TRIGGERED","functionsIn":["BATTLEFIELD"],"trigger":{"mode":"EVENT","event":{"kind":"ATTACKS","subject":{"ref":"SELF"}}},"effect":[{"op":"DRAW","player":{"ref":"CONTROLLER"},"count":{"LITERAL":1}}]}"#;
const EX3: &str = r#"{"abilityId":"a2","kind":"ACTIVATED","functionsIn":["BATTLEFIELD"],"cost":[{"cost":"PAY_MANA","mana":"{2}"},{"cost":"TAP_SOURCE"}],"timing":"SORCERY","limits":[{"perTurn":1}],"activateOnlyIf":{"pred":"CONTROLS","selector":{"pred":"HAS_SUBTYPE","value":{"name":"TestType"}}},"isLoyalty":false,"isManaAbility":false,"targets":[{"slot":0,"selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"min":1,"max":1}],"effect":[{"op":"TAP_OR_UNTAP","mode":"TAP","objects":{"ref":"TARGET","slot":0}}]}"#;
const EX4: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"RULE","kind":"ATTACK_COST","attackers":{"pred":"IS_CREATURE"},"attackTarget":{"player":{"ref":"CONTROLLER"}},"cost":[{"cost":"PAY_MANA","mana":"{2}"}],"per":"EACH_ATTACKER"}]}"#;
const EX5: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"RULE","kind":"BLOCK_RESTRICTION","blockers":{"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":"OPPONENT_OF_CONTROLLER"}]},"rule":"CANT_BLOCK"}]}"#;
const EX6: &str = r#"{"abilityId":"s1","kind":"STATIC","effects":[{"type":"RULE","kind":"BLOCK_CAPACITY","blockers":{"ref":"SELF"},"max":"UNBOUNDED"}]}"#;
const EX7: &str = r#"{"abilityId":"s1","kind":"STATIC","functionsIn":["BATTLEFIELD"],"effects":[{"type":"CHAR","layer":"6","affected":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"modification":{"op":"REMOVE_ALL_ABILITIES"}},{"type":"CHAR","layer":"7b","affected":{"sameAsLayer":"6"},"modification":{"op":"SET_BASE_PT","power":1,"toughness":1}}]}"#;
const EX8: &str = r#"{"abilityId":"s2","kind":"STATIC","condition":{"pred":"PLAYER_HAS_MAX_SPEED","player":{"ref":"CONTROLLER"}},"effects":[{"type":"CHAR","layer":"7c","affected":{"zones":["BATTLEFIELD"],"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"CONTROLLED_BY","player":{"ref":"CONTROLLER"}},{"op":"NOT","condition":{"pred":"SAME_AS","ref":"SELF"}}]},"modification":{"op":"MODIFY_PT","power":2,"toughness":1}}]}"#;
const EX9: &str = r#"{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["BATTLEFIELD"],"match":{"event":"ZONE_CHANGE","filters":{"to":"GRAVEYARD","from":"ANY"},"affected":{"pred":"IS_CARD_OR_TOKEN"}},"replaceWith":[{"op":"MODIFY_EVENT","set":{"to":"EXILE"}}]}"#;
const EX10: &str = r#"{"abilityId":"r1","kind":"REPLACEMENT","functionsIn":["STACK","BATTLEFIELD"],"match":{"event":"ENTERS","affected":{"ref":"SELF"}},"replaceWith":[{"op":"CHOOSE","choice":{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"},"store":"chosenType"}]}"#;

// ---------------------------------------------------------------- helpers

fn text(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

/// Decodes `s` through `from_str` and `from_slice` (equal results); `to_value` equals the input;
/// canonical bytes equal `canonicalize(s)` (W4); re-decoding the canonical bytes and the
/// `serde_json::to_vec` output gives the same value.
fn accept<T>(s: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    let t: T = match serde_json::from_str::<T>(s) {
        Ok(t) => t,
        Err(e) => panic!("expected {s} to decode, got error: {e}"),
    };
    let t2: T = serde_json::from_slice(s.as_bytes()).unwrap();
    assert_eq!(t2, t, "from_slice differs for {s}");
    let original: Value = serde_json::from_str(s).unwrap();
    assert_eq!(
        serde_json::to_value(&t).unwrap(),
        original,
        "to_value of {s}"
    );
    let canon = canonicalize(s.as_bytes()).unwrap();
    let mine = to_canonical_vec(&t).unwrap();
    assert_eq!(
        String::from_utf8(mine).unwrap(),
        String::from_utf8(canon.clone()).unwrap(),
        "canonical bytes of {s}"
    );
    let from_canon: T = serde_json::from_slice(&canon).unwrap();
    assert_eq!(from_canon, t);
    let enc = serde_json::to_vec(&t).unwrap();
    let back: T = serde_json::from_slice(&enc).unwrap();
    assert_eq!(back, t);
    t
}

fn accept_v<T>(v: &Value) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + Debug,
{
    accept::<T>(&text(v))
}

/// Rejected by `from_str` and `from_slice`; when the text is a valid I-JSON document, also by
/// `from_value`.
fn reject<T: DeserializeOwned + Debug>(s: &str) {
    let r = serde_json::from_str::<T>(s);
    assert!(r.is_err(), "expected rejection of {s}, got {:?}", r.ok());
    let r = serde_json::from_slice::<T>(s.as_bytes());
    assert!(r.is_err(), "expected rejection (from_slice) of {s}");
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

/// `base` with member `k` set to `x` (base must be an object).
fn with(base: &Value, k: &str, x: Value) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().insert(k.to_string(), x);
    v
}

/// `base` without member `k`.
fn without(base: &Value, k: &str) -> Value {
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove(k);
    v
}

/// The smallest valid ability of each kind.
fn minimal(kind: &str) -> Value {
    match kind {
        "SPELL" => json!({"abilityId":"a","kind":"SPELL","effect":[]}),
        "ACTIVATED" => {
            json!({"abilityId":"a","kind":"ACTIVATED","cost":[{"cost":"TAP_SOURCE"}],"effect":[]})
        }
        "TRIGGERED" => json!({"abilityId":"a","kind":"TRIGGERED",
            "trigger":{"mode":"EVENT","event":{"kind":"DIES"}},"effect":[]}),
        "STATIC" => {
            json!({"abilityId":"a","kind":"STATIC","effects":[{"type":"RULE","kind":"SKIP"}]})
        }
        "REPLACEMENT" => json!({"abilityId":"a","kind":"REPLACEMENT",
            "match":{"event":"DIES","affected":{"ref":"SELF"}},"replaceWith":[{"op":"SKIP"}]}),
        "KEYWORD" => json!({"abilityId":"a","kind":"KEYWORD","keyword":"TEST_KEYWORD"}),
        _ => unreachable!(),
    }
}

fn trig(trigger: Value) -> Value {
    with(&minimal("TRIGGERED"), "trigger", trigger)
}

fn event_trig(matcher: Value) -> Value {
    trig(json!({"mode":"EVENT","event":matcher}))
}

/// The lower-cased key with every `_` and `-` removed (rule E5).
fn normalized(k: &str) -> String {
    k.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

fn assert_no_look_back_key(v: &Value) {
    // Linear walk with an explicit stack.
    let mut stack = vec![v];
    while let Some(x) = stack.pop() {
        match x {
            Value::Object(m) => {
                for (k, val) in m {
                    assert!(
                        !normalized(k).starts_with("lookback"),
                        "serialised look-back key {k}"
                    );
                    stack.push(val);
                }
            }
            Value::Array(a) => stack.extend(a.iter()),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- 1. SRS examples

fn check_example(ex: &str) {
    let a: AbilityAST = accept(ex);
    // decode(to_canonical_vec(&x)) == x
    let c = to_canonical_vec(&a).unwrap();
    let a2: AbilityAST = serde_json::from_slice(&c).unwrap();
    assert_eq!(a2, a);
    assert_eq!(to_canonical_vec(&a2).unwrap(), c);
}

/// REQ-DSL-006, REQ-NFR-006: EX1 (triggered, DAMAGE_DEALT with filters and bind).
#[test]
fn req_dsl_006_example_ex1() {
    check_example(EX1);
}

/// REQ-DSL-006, REQ-NFR-006: EX2 (triggered, ATTACKS).
#[test]
fn req_dsl_006_example_ex2() {
    check_example(EX2);
}

/// REQ-DSL-007, REQ-NFR-006: EX3 (activated ability with every activated member).
#[test]
fn req_dsl_007_example_ex3() {
    assert!(ManaCostDSL::parse("{2}").is_ok());
    check_example(EX3);
}

/// REQ-DSL-004, REQ-DSL-008, REQ-NFR-006: EX4 (RULE ATTACK_COST with a `cost` param).
#[test]
fn req_dsl_008_example_ex4() {
    check_example(EX4);
}

/// REQ-DSL-004, REQ-DSL-008, REQ-NFR-006: EX5 (RULE BLOCK_RESTRICTION, opaque params).
#[test]
fn req_dsl_008_example_ex5() {
    check_example(EX5);
}

/// REQ-DSL-004, REQ-DSL-008, REQ-NFR-006: EX6 (RULE BLOCK_CAPACITY).
#[test]
fn req_dsl_008_example_ex6() {
    check_example(EX6);
}

/// REQ-DSL-004, REQ-DSL-008, REQ-NFR-006: EX7 (CHAR layer 6 + sameAsLayer lock-in, 613.6).
#[test]
fn req_dsl_008_example_ex7() {
    check_example(EX7);
}

/// REQ-DSL-004, REQ-DSL-008, REQ-NFR-006: EX8 (CHAR layer 7c with condition).
#[test]
fn req_dsl_008_example_ex8() {
    check_example(EX8);
}

/// REQ-DSL-009, REQ-NFR-006: EX9 (replacement MODIFY_EVENT).
#[test]
fn req_dsl_009_example_ex9() {
    check_example(EX9);
}

/// REQ-DSL-009, REQ-NFR-006: EX10 (replacement CHOOSE).
#[test]
fn req_dsl_009_example_ex10() {
    check_example(EX10);
}

/// REQ-DSL-004..009: the ten examples are pairwise different values.
#[test]
fn req_dsl_005_examples_distinct() {
    let all = [EX1, EX2, EX3, EX4, EX5, EX6, EX7, EX8, EX9, EX10];
    let decoded: Vec<AbilityAST> = all.iter().map(|e| accept::<AbilityAST>(e)).collect();
    for i in 0..decoded.len() {
        for j in 0..i {
            assert_ne!(decoded[i], decoded[j], "EX{} == EX{}", i + 1, j + 1);
        }
    }
}

// ---------------------------------------------------------------- 2. closed sets

fn check_closed_set<T>(
    all: &[T],
    names: &[&str],
    as_str: fn(T) -> &'static str,
    from_wire: fn(&str) -> Option<T>,
) where
    T: Clone + Debug + PartialEq + Serialize + DeserializeOwned,
{
    assert_eq!(all.len(), names.len(), "ALL length");
    let wire: BTreeSet<&str> = all.iter().map(|t| as_str(t.clone())).collect();
    let want: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(wire, want, "wire names");
    assert_eq!(wire.len(), all.len(), "as_str not injective");
    for (i, t) in all.iter().enumerate() {
        for u in &all[..i] {
            assert_ne!(t, u, "duplicate in ALL");
        }
        let s = as_str(t.clone());
        assert_eq!(from_wire(s).as_ref(), Some(t), "from_wire({s})");
        let q = format!("\"{s}\"");
        let d: T = accept(&q);
        assert_eq!(&d, t);
    }
    for n in names {
        let t = from_wire(n).unwrap_or_else(|| panic!("from_wire({n}) is None"));
        assert_eq!(as_str(t), *n);
        let lower = n.to_lowercase();
        let mut mixed = String::new();
        for (i, c) in n.chars().enumerate() {
            if i == 0 {
                mixed.extend(c.to_uppercase());
            } else {
                mixed.extend(c.to_lowercase());
            }
        }
        let upper = n.to_uppercase();
        for bad in [
            lower,
            mixed,
            upper,
            format!(" {n}"),
            format!("{n} "),
            format!("{n}X"),
        ] {
            if bad == *n {
                continue;
            }
            assert!(from_wire(&bad).is_none(), "from_wire({bad}) accepted");
            reject::<T>(&format!("\"{bad}\""));
        }
    }
    for bad in ["", "UNKNOWN_WIRE_NAME", "null"] {
        assert!(from_wire(bad).is_none(), "from_wire({bad:?})");
        reject::<T>(&format!("\"{bad}\""));
    }
    reject::<T>("null");
    reject::<T>("0");
    reject::<T>("[]");
}

/// REQ-DSL-006: EventKind is exactly the 57 listed names.
#[test]
fn req_dsl_006_event_kind_closed_set() {
    check_closed_set(
        EventKind::ALL,
        &EVENT_KINDS,
        EventKind::as_str,
        EventKind::from_wire,
    );
}

/// REQ-DSL-006: EventBatch is exactly EACH and ONE_OR_MORE.
#[test]
fn req_dsl_006_event_batch_closed_set() {
    check_closed_set(
        EventBatch::ALL,
        &BATCHES,
        EventBatch::as_str,
        EventBatch::from_wire,
    );
}

/// REQ-DSL-006: StepId is exactly the 13 listed steps.
#[test]
fn req_dsl_006_step_id_closed_set() {
    check_closed_set(StepId::ALL, &STEPS, StepId::as_str, StepId::from_wire);
}

/// REQ-DSL-007: ActivationTiming is exactly the 5 listed values.
#[test]
fn req_dsl_007_activation_timing_closed_set() {
    check_closed_set(
        ActivationTiming::ALL,
        &TIMINGS,
        ActivationTiming::as_str,
        ActivationTiming::from_wire,
    );
}

/// REQ-DSL-008: Layer is exactly the 11 listed strings ("1A", "7", "7e" are not layers).
#[test]
fn req_dsl_008_layer_closed_set() {
    check_closed_set(Layer::ALL, &LAYERS, Layer::as_str, Layer::from_wire);
    for bad in ["1", "7", "7e", "1A", "7B", "8", "0", "1c", "6 "] {
        assert!(Layer::from_wire(bad).is_none(), "{bad}");
        reject::<Layer>(&format!("\"{bad}\""));
    }
    reject::<Layer>("6");
}

/// REQ-DSL-008: RuleKind is exactly the 34 listed names.
#[test]
fn req_dsl_008_rule_kind_closed_set() {
    check_closed_set(
        RuleKind::ALL,
        &RULE_KINDS,
        RuleKind::as_str,
        RuleKind::from_wire,
    );
}

/// REQ-DSL-009: TierHint is exactly the 3 listed values.
#[test]
fn req_dsl_009_tier_hint_closed_set() {
    check_closed_set(
        TierHint::ALL,
        &TIER_HINTS,
        TierHint::as_str,
        TierHint::from_wire,
    );
}

/// REQ-DSL-006: each of the 57 EventKind names works in an EventMatcher, in a trigger and as
/// `match.event` of a replacement; the values are pairwise distinct.
#[test]
fn req_dsl_006_every_event_kind_in_matcher_trigger_and_match() {
    let mut seen: Vec<EventMatcher> = Vec::new();
    for k in EVENT_KINDS {
        let m: EventMatcher = accept_v(&json!({ "kind": k }));
        assert!(!seen.contains(&m), "{k}");
        seen.push(m);
        accept_v::<EventMatcher>(&json!({"kind":k,"subject":{"ref":"SELF"},"batch":"EACH"}));
        accept_v::<AbilityAST>(&event_trig(json!({ "kind": k })));
        let r = with(
            &minimal("REPLACEMENT"),
            "match",
            json!({"event":k,"affected":{"ref":"SELF"}}),
        );
        accept_v::<AbilityAST>(&r);
    }
    for bad in ["dies", "Dies", "DIE", "", "LOOK_BACK"] {
        reject_v::<EventMatcher>(&json!({ "kind": bad }));
        reject_v::<AbilityAST>(&event_trig(json!({ "kind": bad })));
        let r = with(
            &minimal("REPLACEMENT"),
            "match",
            json!({"event":bad,"affected":{"ref":"SELF"}}),
        );
        reject_v::<AbilityAST>(&r);
    }
}

/// REQ-DSL-008: each of the 34 RuleKind names works in a RULE static effect.
#[test]
fn req_dsl_008_every_rule_kind_in_rule_effect() {
    for k in RULE_KINDS {
        let a = json!({"abilityId":"s","kind":"STATIC","effects":[{"type":"RULE","kind":k}]});
        accept_v::<AbilityAST>(&a);
        let b = json!({"abilityId":"s","kind":"STATIC",
            "effects":[{"type":"RULE","kind":k,"rule":"TEST_RULE","amount":{"LITERAL":1}}]});
        accept_v::<AbilityAST>(&b);
    }
    for bad in ["skip", "Skip", "SKIPS", "", "TEST_RULE"] {
        let a = json!({"abilityId":"s","kind":"STATIC","effects":[{"type":"RULE","kind":bad}]});
        reject_v::<AbilityAST>(&a);
    }
}

// ---------------------------------------------------------------- 3. no look-back flag

const LOOK_BACK_NAMES: [&str; 5] = [
    "lookBack",
    "lookback",
    "look_back",
    "LOOK_BACK",
    "lookBackFlag",
];

/// REQ-DSL-006 (SRS §14.4.1, 603.10a–g derived by the engine): a look-back member on an
/// EventMatcher, on the trigger object or on the ability object is rejected.
#[test]
fn req_dsl_006_look_back_member_rejected_on_matcher_trigger_ability() {
    let matcher = json!({"kind":"DIES","subject":{"ref":"SELF"}});
    accept_v::<EventMatcher>(&matcher);
    accept_v::<AbilityAST>(&event_trig(matcher.clone()));
    for name in LOOK_BACK_NAMES {
        for val in [json!(true), json!(false), json!(1), json!("YES")] {
            let m = with(&matcher, name, val.clone());
            reject_v::<EventMatcher>(&m);
            reject_v::<AbilityAST>(&event_trig(m));
            let t = json!({"mode":"EVENT","event":matcher.clone(), name: val.clone()});
            reject_v::<AbilityAST>(&trig(t));
            reject_v::<AbilityAST>(&with(&event_trig(matcher.clone()), name, val.clone()));
            for k in KINDS {
                reject_v::<AbilityAST>(&with(&minimal(k), name, val.clone()));
            }
        }
    }
}

/// REQ-DSL-006: an effect instruction is opaque, so a look-back-named member there is allowed and
/// round-trips unchanged.
#[test]
fn req_dsl_006_look_back_name_inside_effect_instruction_is_opaque() {
    for name in LOOK_BACK_NAMES {
        let instr = json!({"op":"TEST_OP", name: true});
        accept_v::<EffectInstructionAST>(&instr);
        accept_v::<AbilityAST>(&with(
            &event_trig(json!({"kind":"DIES"})),
            "effect",
            json!([instr]),
        ));
    }
}

const LOOK_BACK_KEYS: [&str; 14] = [
    "lookBack",
    "lookback",
    "look_back",
    "LOOK_BACK",
    "lookBackFlag",
    "look-back",
    "Look-Back",
    "LoOk_BaCk",
    "_look_back",
    "-lookback",
    "l_o_o_k-b_a-c_k",
    "LOOKBACK",
    "lookback_x",
    "look__back__time",
];

const NOT_LOOK_BACK_KEYS: [&str; 7] = [
    "lookBac",
    "backLook",
    "look",
    "xlookback",
    "look_bac_x",
    "lookAhead",
    "l0okback",
];

/// REQ-DSL-006 (rule E5): a `filters` key whose normalised form starts with "lookback" is
/// rejected, through every path; near-miss keys are accepted.
#[test]
fn req_dsl_006_look_back_rejected_in_filters() {
    for k in NOT_LOOK_BACK_KEYS {
        let m = json!({"kind":"DIES","filters":{ k: true }});
        accept_v::<EventMatcher>(&m);
        accept_v::<AbilityAST>(&event_trig(m));
    }
    for k in LOOK_BACK_KEYS {
        assert!(normalized(k).starts_with("lookback"), "{k}");
        for val in [
            json!(true),
            json!(3),
            json!("ANY"),
            json!({"pred":"TAPPED"}),
        ] {
            let good = json!({"kind":"DIES","filters":{"combat": val.clone()}});
            accept_v::<EventMatcher>(&good);
            let m = json!({"kind":"DIES","filters":{"combat":true, k: val.clone()}});
            reject_v::<EventMatcher>(&m);
            reject_v::<AbilityAST>(&event_trig(m));
        }
    }
}

/// REQ-DSL-006 (rule E5): a `bind` key whose normalised form starts with "lookback" is rejected.
#[test]
fn req_dsl_006_look_back_rejected_in_bind() {
    for k in NOT_LOOK_BACK_KEYS {
        let m = json!({"kind":"DIES","bind":{ k: "$v" }});
        accept_v::<EventMatcher>(&m);
        accept_v::<AbilityAST>(&event_trig(m));
    }
    for k in LOOK_BACK_KEYS {
        let m = json!({"kind":"DIES","bind":{"recipient":"$P", k: "$v"}});
        reject_v::<EventMatcher>(&m);
        reject_v::<AbilityAST>(&event_trig(m));
    }
}

/// REQ-DSL-006: the encode side never writes a look-back key: an EventMatcher obtained by
/// decoding a legal document re-serialises without one, and re-decodes equal.
#[test]
fn req_dsl_006_look_back_never_serialised() {
    let docs = [
        json!({"kind":"DIES"}),
        json!({"kind":"LEAVES_BATTLEFIELD","subject":{"pred":"IS_CREATURE"},
            "filters":{"lookBac":true,"backLook":1,"from":"BATTLEFIELD"},"bind":{"look":"$v"},
            "batch":"ONE_OR_MORE"}),
        serde_json::from_str::<Value>(EX1).unwrap()["trigger"]["event"].clone(),
    ];
    for d in &docs {
        let m: EventMatcher = accept_v(d);
        let v = serde_json::to_value(&m).unwrap();
        assert_no_look_back_key(&v);
        let a: AbilityAST = accept_v(&event_trig(d.clone()));
        let mut av = serde_json::to_value(&a).unwrap();
        av.as_object_mut().unwrap().remove("effect");
        assert_no_look_back_key(&av);
    }
}

// ---------------------------------------------------------------- 4. ability envelope

fn optional_base_members() -> Vec<(&'static str, Value)> {
    vec![
        ("functionsIn", json!(["BATTLEFIELD"])),
        ("functionsIn", json!(["HAND", "GRAVEYARD", "EXILE"])),
        ("functionsIn", json!(ZONES)),
        ("textSpan", json!([0, 12])),
        ("textSpan", json!([7, 7])),
        ("textSpan", json!([0, 4294967295u64])),
        ("linkIds", json!(["link1"])),
        ("linkIds", json!(["link1", "link2"])),
        ("abilityWord", json!("testword")),
        ("condition", json!({"pred":"TAPPED"})),
        (
            "condition",
            json!({"op":"NOT","condition":{"pred":"IS_CREATURE"}}),
        ),
        ("isManaAbility", json!(true)),
        ("isManaAbility", json!(false)),
    ]
}

/// REQ-DSL-005: every optional base member round-trips on every kind, alone and all together;
/// explicit `false` for isManaAbility is distinct from absence.
#[test]
fn req_dsl_005_base_optional_members_round_trip() {
    for kind in KINDS {
        let base = minimal(kind);
        let plain: AbilityAST = accept_v(&base);
        for (k, v) in optional_base_members() {
            let a: AbilityAST = accept_v(&with(&base, k, v));
            assert_ne!(a, plain, "{kind} {k}");
        }
        let mut all = base.clone();
        for (k, v) in [
            ("functionsIn", json!(["STACK", "BATTLEFIELD"])),
            ("textSpan", json!([3, 9])),
            ("linkIds", json!(["l1", "l2"])),
            ("abilityWord", json!("testword")),
            ("condition", json!({"pred":"TAPPED"})),
            ("isManaAbility", json!(false)),
        ] {
            all = with(&all, k, v);
        }
        accept_v::<AbilityAST>(&all);
        let f: AbilityAST = accept_v(&with(&base, "isManaAbility", json!(false)));
        let t: AbilityAST = accept_v(&with(&base, "isManaAbility", json!(true)));
        assert_ne!(f, t);
        assert_eq!(
            serde_json::to_value(&f).unwrap()["isManaAbility"],
            json!(false)
        );
        assert!(
            serde_json::to_value(&plain)
                .unwrap()
                .get("isManaAbility")
                .is_none()
        );
        // Order of functionsIn is kept (no sorting): different orders are different values.
        let x: AbilityAST = accept_v(&with(&base, "functionsIn", json!(["HAND", "EXILE"])));
        let y: AbilityAST = accept_v(&with(&base, "functionsIn", json!(["EXILE", "HAND"])));
        assert_ne!(x, y);
    }
}

/// REQ-DSL-005: base-member rejections on every kind (each with the valid base as control).
#[test]
fn req_dsl_005_base_rejections() {
    for kind in KINDS {
        let base = minimal(kind);
        accept_v::<AbilityAST>(&base);
        reject_v::<AbilityAST>(&without(&base, "abilityId"));
        reject_v::<AbilityAST>(&without(&base, "kind"));
        let bad: Vec<(&str, Value)> = vec![
            ("abilityId", json!("")),
            ("abilityId", json!(1)),
            ("abilityId", json!(null)),
            ("kind", json!(kind.to_lowercase())),
            ("kind", json!("INSTANT")),
            ("kind", json!("")),
            ("kind", json!(null)),
            ("functionsIn", json!([])),
            ("functionsIn", json!(["HAND", "HAND"])),
            ("functionsIn", json!(["BATTLEFIELD", "HAND", "BATTLEFIELD"])),
            ("functionsIn", json!(["NOWHERE"])),
            ("functionsIn", json!(["battlefield"])),
            ("functionsIn", json!("BATTLEFIELD")),
            ("functionsIn", json!(null)),
            ("textSpan", json!([5, 3])),
            ("textSpan", json!([1, 2, 3])),
            ("textSpan", json!([1])),
            ("textSpan", json!([])),
            ("textSpan", json!([-1, 2])),
            ("textSpan", json!([0, 4294967296u64])),
            ("textSpan", json!(["0", "1"])),
            ("textSpan", json!({"start":0,"end":1})),
            ("textSpan", json!(null)),
            ("linkIds", json!([])),
            ("linkIds", json!(["l", "l"])),
            ("linkIds", json!(["l", "m", "l"])),
            ("linkIds", json!([""])),
            ("linkIds", json!([1])),
            ("linkIds", json!(null)),
            ("abilityWord", json!("")),
            ("abilityWord", json!(null)),
            ("abilityWord", json!(["w"])),
            ("condition", json!({})),
            ("condition", json!(null)),
            ("condition", json!({"pred":"NOT_A_PREDICATE"})),
            ("isManaAbility", json!("true")),
            ("isManaAbility", json!(1)),
            ("isManaAbility", json!(null)),
            ("zzUnknown", json!(1)),
            ("AbilityId", json!("a")),
            ("ability_id", json!("a")),
        ];
        for (k, v) in bad {
            reject_v::<AbilityAST>(&with(&base, k, v));
        }
    }
    reject::<AbilityAST>("{}");
    reject::<AbilityAST>("[]");
    reject::<AbilityAST>("null");
    reject::<AbilityAST>("\"SPELL\"");
}

/// REQ-DSL-005: a member belonging to another kind is unknown for this kind.
#[test]
fn req_dsl_005_members_of_other_kinds_rejected() {
    let foreign: Vec<(&str, Value)> = vec![
        ("effect", json!([])),
        ("cost", json!([{"cost":"TAP_SOURCE"}])),
        ("trigger", json!({"mode":"EVENT","event":{"kind":"DIES"}})),
        ("effects", json!([{"type":"RULE","kind":"SKIP"}])),
        ("match", json!({"event":"DIES","affected":{"ref":"SELF"}})),
        ("replaceWith", json!([{"op":"SKIP"}])),
        ("keyword", json!("TEST_KEYWORD")),
        ("timing", json!("INSTANT")),
        ("limits", json!([{"perTurn":1}])),
        ("isLoyalty", json!(true)),
        ("interveningIf", json!({"pred":"TAPPED"})),
        ("limit", json!({"perTurn":1})),
        ("tierHint", json!("COPY_ETB")),
        ("shield", json!({"uses":1})),
        ("params", json!({"n":1})),
        ("isSelfReplacement", json!(true)),
        ("overrides614_5", json!(true)),
        (
            "targets",
            json!([{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}]),
        ),
        (
            "modes",
            json!({"min":1,"max":1,"modes":[{"id":"m","effect":[]}]}),
        ),
    ];
    let allowed = |kind: &str, k: &str| -> bool {
        match kind {
            "SPELL" => ["effect", "targets", "modes"].contains(&k),
            "ACTIVATED" => [
                "effect",
                "cost",
                "timing",
                "limits",
                "isLoyalty",
                "targets",
                "modes",
            ]
            .contains(&k),
            "TRIGGERED" => [
                "effect",
                "trigger",
                "interveningIf",
                "limit",
                "targets",
                "modes",
            ]
            .contains(&k),
            "STATIC" => k == "effects",
            "REPLACEMENT" => [
                "match",
                "replaceWith",
                "tierHint",
                "shield",
                "isSelfReplacement",
                "overrides614_5",
            ]
            .contains(&k),
            "KEYWORD" => ["keyword", "params"].contains(&k),
            _ => unreachable!(),
        }
    };
    for kind in KINDS {
        let base = minimal(kind);
        for (k, v) in &foreign {
            let doc = with(&base, k, v.clone());
            if allowed(kind, k) {
                accept_v::<AbilityAST>(&doc);
            } else {
                reject_v::<AbilityAST>(&doc);
            }
        }
    }
}

// ---------------------------------------------------------------- 5. triggers

/// REQ-DSL-006: EVENT trigger: needs `event`, no extra members.
#[test]
fn req_dsl_006_event_trigger() {
    let ok = json!({"mode":"EVENT","event":{"kind":"ENTERS","subject":{"ref":"SELF"}}});
    accept_v::<AbilityAST>(&trig(ok.clone()));
    reject_v::<AbilityAST>(&trig(without(&ok, "event")));
    reject_v::<AbilityAST>(&trig(without(&ok, "mode")));
    for (k, v) in [
        ("step", json!("UPKEEP")),
        ("whose", json!({"ref":"CONTROLLER"})),
        ("state", json!({"pred":"TAPPED"})),
        ("zzUnknown", json!(1)),
        ("kind", json!("ENTERS")),
    ] {
        reject_v::<AbilityAST>(&trig(with(&ok, k, v)));
    }
    for mode in ["event", "Event", "EVENTS", "", "OTHER"] {
        reject_v::<AbilityAST>(&trig(with(&ok, "mode", json!(mode))));
    }
    reject_v::<AbilityAST>(&trig(with(&ok, "event", json!(null))));
    reject_v::<AbilityAST>(&trig(with(&ok, "event", json!({}))));
    reject_v::<AbilityAST>(&trig(with(&ok, "event", json!("ENTERS"))));
    reject_v::<AbilityAST>(&trig(json!("EVENT")));
    reject_v::<AbilityAST>(&trig(json!(null)));
    reject_v::<AbilityAST>(&without(&minimal("TRIGGERED"), "trigger"));
}

/// REQ-DSL-006: PHASE_STEP trigger for every StepId and player-selector form; missing or extra
/// members rejected.
#[test]
fn req_dsl_006_phase_step_trigger() {
    let whoses = [
        json!({"ref":"CONTROLLER"}),
        json!({"ref":"ACTIVE_PLAYER"}),
        json!({"ref":"EACH_OPPONENT"}),
        json!({"players":"ALL_PLAYERS"}),
        json!({"players":"TEAMMATES"}),
    ];
    let mut seen = Vec::new();
    for step in STEPS {
        for w in &whoses {
            let t = json!({"mode":"PHASE_STEP","step":step,"whose":w});
            let a: AbilityAST = accept_v(&trig(t));
            assert!(!seen.contains(&a));
            seen.push(a);
        }
    }
    let ok = json!({"mode":"PHASE_STEP","step":"UPKEEP","whose":{"ref":"CONTROLLER"}});
    reject_v::<AbilityAST>(&trig(without(&ok, "step")));
    reject_v::<AbilityAST>(&trig(without(&ok, "whose")));
    for (k, v) in [
        ("step", json!("upkeep")),
        ("step", json!("MAIN")),
        ("step", json!("")),
        ("step", json!(null)),
        ("whose", json!({"players":"NOBODY"})),
        ("whose", json!(null)),
        ("whose", json!("CONTROLLER")),
        ("whose", json!({})),
        ("event", json!({"kind":"DIES"})),
        ("state", json!({"pred":"TAPPED"})),
        ("zzUnknown", json!(1)),
    ] {
        reject_v::<AbilityAST>(&trig(with(&ok, k, v)));
    }
}

/// REQ-DSL-006: STATE trigger needs `state` (a predicate); no extra members.
#[test]
fn req_dsl_006_state_trigger() {
    let ok = json!({"mode":"STATE","state":{"pred":"TAPPED"}});
    accept_v::<AbilityAST>(&trig(ok.clone()));
    accept_v::<AbilityAST>(&trig(json!({"mode":"STATE",
        "state":{"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"op":"NOT","condition":{"pred":"TAPPED"}}]}})));
    reject_v::<AbilityAST>(&trig(without(&ok, "state")));
    for (k, v) in [
        ("state", json!({})),
        ("state", json!(null)),
        ("state", json!("TAPPED")),
        ("event", json!({"kind":"DIES"})),
        ("step", json!("UPKEEP")),
        ("zzUnknown", json!(1)),
    ] {
        reject_v::<AbilityAST>(&trig(with(&ok, k, v)));
    }
}

/// REQ-DSL-006: TRIGGERED optional members interveningIf, optional, limit; limit rules.
#[test]
fn req_dsl_006_triggered_optional_members() {
    let base = minimal("TRIGGERED");
    let plain: AbilityAST = accept_v(&base);
    for (k, v) in [
        ("interveningIf", json!({"pred":"TAPPED"})),
        ("optional", json!(true)),
        ("optional", json!(false)),
        ("limit", json!({"perTurn":1})),
        ("limit", json!({"perGame":2})),
        ("limit", json!({"perTurn":1,"perGame":3})),
        ("limit", json!({"perTurn":4294967295u64})),
    ] {
        let a: AbilityAST = accept_v(&with(&base, k, v));
        assert_ne!(a, plain, "{k}");
    }
    let f: AbilityAST = accept_v(&with(&base, "optional", json!(false)));
    let t: AbilityAST = accept_v(&with(&base, "optional", json!(true)));
    assert_ne!(f, t);
    for (k, v) in [
        ("limit", json!({})),
        ("limit", json!({"perTurn":0})),
        ("limit", json!({"perGame":0})),
        ("limit", json!({"perTurn":1,"perGame":0})),
        ("limit", json!({"perTurn":-1})),
        ("limit", json!({"perTurn":4294967296u64})),
        ("limit", json!({"perTurn":"1"})),
        ("limit", json!({"perTurn":null})),
        ("limit", json!({"perTurn":1,"zzUnknown":1})),
        ("limit", json!({"per_turn":1})),
        ("limit", json!([{"perTurn":1}])),
        ("limit", json!(null)),
        ("interveningIf", json!({})),
        ("interveningIf", json!(null)),
        ("optional", json!("yes")),
        ("optional", json!(null)),
        ("limits", json!([{"perTurn":1}])),
    ] {
        reject_v::<AbilityAST>(&with(&base, k, v));
    }
}

/// REQ-DSL-006: EventMatcher subject forms (selectors, references, player groups).
#[test]
fn req_dsl_006_event_matcher_subject() {
    let ok = [
        json!({"ref":"SELF"}),
        json!({"ref":"TARGET","slot":0}),
        json!({"ref":"CONTROLLER"}),
        json!({"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}),
        json!({"pred":"IS_CREATURE"}),
        json!({"op":"AND","conditions":[{"pred":"IS_CREATURE"},{"pred":"TAPPED"}]}),
        json!({"players":"ALL_PLAYERS"}),
        json!({"players":"TEAMMATES"}),
    ];
    let plain: EventMatcher = accept_v(&json!({"kind":"CAST"}));
    for s in &ok {
        let m: EventMatcher = accept_v(&json!({"kind":"CAST","subject":s}));
        assert_ne!(m, plain);
        accept_v::<AbilityAST>(&event_trig(json!({"kind":"CAST","subject":s})));
    }
    for bad in [
        json!({"players":"NOBODY"}),
        json!({}),
        json!("SELF"),
        json!(null),
        json!({"ref":"NOBODY"}),
        json!([{"ref":"SELF"}]),
    ] {
        reject_v::<EventMatcher>(&json!({"kind":"CAST","subject":bad}));
        reject_v::<AbilityAST>(&event_trig(json!({"kind":"CAST","subject":bad})));
    }
}

/// REQ-DSL-006: every FilterValue form; arrays, null, empty strings and empty keys rejected.
#[test]
fn req_dsl_006_event_matcher_filters() {
    let mut ok = vec![
        json!(true),
        json!(false),
        json!(0),
        json!(3),
        json!(-2),
        json!(9007199254740991i64),
        json!(-9007199254740991i64),
        json!("ANY"),
        json!("TestType"),
        json!("Graveyard"),
        json!("any"),
        json!({"pred":"IS_PLAYER"}),
        json!({"op":"NOT","condition":{"pred":"TAPPED"}}),
    ];
    for z in ZONES {
        ok.push(json!(z));
    }
    accept_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","filters":{}}));
    let mut seen: Vec<EventMatcher> = Vec::new();
    for v in &ok {
        let m: EventMatcher = accept_v(&json!({"kind":"DAMAGE_DEALT","filters":{"f": v}}));
        assert!(!seen.contains(&m), "{v}");
        seen.push(m);
    }
    let multi = json!({"kind":"ZONE_CHANGE","filters":{"to":"GRAVEYARD","from":"ANY",
        "combat":true,"amount":2,"name":"TestType","who":{"pred":"IS_PLAYER"}}});
    accept_v::<EventMatcher>(&multi);
    accept_v::<AbilityAST>(&event_trig(multi));
    for bad in [
        json!({"f":[]}),
        json!({"f":[1]}),
        json!({"f":null}),
        json!({"f":""}),
        json!({"":true}),
        json!({"f":9007199254740992u64}),
        json!({"f":-9007199254740992i64}),
        json!({"f":{}}),
        json!([]),
        json!("ANY"),
        json!(null),
    ] {
        reject_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","filters":bad}));
        reject_v::<AbilityAST>(&event_trig(json!({"kind":"DAMAGE_DEALT","filters":bad})));
    }
    reject::<EventMatcher>(r#"{"kind":"DAMAGE_DEALT","filters":{"f":1.5}}"#);
    reject::<EventMatcher>(r#"{"kind":"DAMAGE_DEALT","filters":{"f":1.0}}"#);
}

/// REQ-DSL-006: bind values must be `$` plus one or more of A-Z a-z 0-9 _; keys non-empty.
#[test]
fn req_dsl_006_event_matcher_bind() {
    accept_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","bind":{}}));
    for v in ["$P", "$a_1", "$ABCxyz09_", "$_", "$0"] {
        let m = json!({"kind":"DAMAGE_DEALT","bind":{"recipient": v}});
        accept_v::<EventMatcher>(&m);
        accept_v::<AbilityAST>(&event_trig(m));
    }
    accept_v::<EventMatcher>(
        &json!({"kind":"DAMAGE_DEALT","bind":{"recipient":"$P","source":"$S"}}),
    );
    for v in [
        json!("P"),
        json!("$"),
        json!("$a-b"),
        json!("$ a"),
        json!("$a "),
        json!(""),
        json!("$\u{e9}"),
        json!("$$a"),
        json!("a$b"),
        json!(1),
        json!(null),
        json!({"ref":"BOUND","var":"$P"}),
    ] {
        let m = json!({"kind":"DAMAGE_DEALT","bind":{"recipient": v}});
        reject_v::<EventMatcher>(&m);
        reject_v::<AbilityAST>(&event_trig(m));
    }
    reject_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","bind":{"":"$P"}}));
    reject_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","bind":[]}));
    reject_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT","bind":null}));
}

/// REQ-DSL-006: batch values; unknown members; kind required.
#[test]
fn req_dsl_006_event_matcher_batch_and_strictness() {
    let e: EventMatcher = accept_v(&json!({"kind":"DAMAGE_DEALT_BATCH","batch":"EACH"}));
    let o: EventMatcher = accept_v(&json!({"kind":"DAMAGE_DEALT_BATCH","batch":"ONE_OR_MORE"}));
    let n: EventMatcher = accept_v(&json!({"kind":"DAMAGE_DEALT_BATCH"}));
    assert_ne!(e, o);
    assert_ne!(e, n);
    assert_ne!(o, n);
    for b in [
        json!("each"),
        json!("ALL"),
        json!(""),
        json!(null),
        json!(1),
    ] {
        reject_v::<EventMatcher>(&json!({"kind":"DAMAGE_DEALT_BATCH","batch":b}));
    }
    reject_v::<EventMatcher>(&json!({"subject":{"ref":"SELF"}}));
    reject_v::<EventMatcher>(&json!({}));
    reject_v::<EventMatcher>(&json!({"kind":null}));
    for k in ["zzUnknown", "event", "Kind", "filter", "binds"] {
        reject_v::<EventMatcher>(&json!({"kind":"DIES", k: {}}));
    }
}

// ---------------------------------------------------------------- 9. SPELL, KEYWORD, targets, modes

fn slot(n: u64, min: u64, max: u64) -> Value {
    json!({"slot":n,"selector":{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"},"min":min,"max":max})
}

/// REQ-DSL-005: TargetSlotAST rules, standalone and on SPELL, ACTIVATED and TRIGGERED.
#[test]
fn req_dsl_005_targets() {
    let good = [
        json!([slot(0, 1, 1)]),
        json!([slot(0, 0, 0)]),
        json!([slot(0, 0, 4294967295)]),
        json!([slot(0, 1, 1), slot(1, 0, 2)]),
        json!([slot(3, 1, 1), slot(0, 1, 1)]),
        json!([{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}]),
        json!([{"slot":0,"selector":{"op":"AND","conditions":[{"pred":"IS_CREATURE"}]},"min":1,"max":1}]),
    ];
    let bad = [
        json!([]),
        json!([slot(0, 1, 1), slot(0, 0, 1)]),
        json!([slot(1, 1, 1), slot(2, 1, 1), slot(1, 1, 1)]),
        json!([slot(0, 2, 1)]),
        json!([slot(0, 1, 4294967296)]),
        json!([slot(4294967296, 1, 1)]),
        json!([{"slot":-1,"selector":{"ref":"SELF"},"min":1,"max":1}]),
        json!([{"slot":0,"selector":{},"min":1,"max":1}]),
        json!([{"slot":0,"selector":"SELF","min":1,"max":1}]),
        json!([{"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1,"zzUnknown":1}]),
        json!([{"slot":0,"selector":{"ref":"SELF"},"min":{"LITERAL":1},"max":1}]),
        json!([null]),
        json!({"slot":0,"selector":{"ref":"SELF"},"min":1,"max":1}),
    ];
    for g in &good {
        for t in g.as_array().unwrap() {
            accept_v::<TargetSlotAST>(t);
        }
    }
    for member in ["slot", "selector", "min", "max"] {
        reject_v::<TargetSlotAST>(&without(&slot(0, 1, 1), member));
        reject_v::<TargetSlotAST>(&with(&slot(0, 1, 1), member, json!(null)));
    }
    reject_v::<TargetSlotAST>(&slot(0, 2, 1));
    for kind in ["SPELL", "ACTIVATED", "TRIGGERED"] {
        let base = minimal(kind);
        for g in &good {
            accept_v::<AbilityAST>(&with(&base, "targets", g.clone()));
        }
        for b in &bad {
            reject_v::<AbilityAST>(&with(&base, "targets", b.clone()));
        }
        for member in ["slot", "selector", "min", "max"] {
            let t = json!([without(&slot(0, 1, 1), member)]);
            reject_v::<AbilityAST>(&with(&base, "targets", t));
        }
        reject_v::<AbilityAST>(&with(&base, "targets", json!(null)));
    }
}

fn mode(id: &str) -> Value {
    json!({"id":id,"effect":[{"op":"TEST_OP"}]})
}

/// REQ-DSL-005: ModeSetAST rules, standalone and on SPELL, ACTIVATED and TRIGGERED.
#[test]
fn req_dsl_005_modes() {
    let good = [
        json!({"min":1,"max":1,"modes":[mode("m1")]}),
        json!({"min":0,"max":0,"modes":[mode("m1")]}),
        json!({"min":1,"max":2,"modes":[mode("m1"),mode("m2")]}),
        json!({"min":1,"max":3,"modes":[mode("m1"),mode("m2"),mode("m3")]}),
        json!({"min":2,"max":2,"modes":[{"id":"m1","effect":[]},
            {"id":"m2","targets":[slot(0,1,1)],"effect":[{"op":"TEST_OP","n":1}]}]}),
    ];
    let bad = [
        json!({"min":1,"max":2,"modes":[mode("m1")]}),
        json!({"min":2,"max":1,"modes":[mode("m1"),mode("m2")]}),
        json!({"min":1,"max":1,"modes":[]}),
        json!({"min":1,"max":1,"modes":[mode("m1"),mode("m1")]}),
        json!({"min":1,"max":1,"modes":[mode("")]}),
        json!({"max":1,"modes":[mode("m1")]}),
        json!({"min":1,"modes":[mode("m1")]}),
        json!({"min":1,"max":1}),
        json!({"min":1,"max":1,"modes":[{"id":"m1"}]}),
        json!({"min":1,"max":1,"modes":[{"effect":[]}]}),
        json!({"min":1,"max":1,"modes":[{"id":"m1","targets":[],"effect":[]}]}),
        json!({"min":1,"max":1,"modes":[{"id":"m1","targets":null,"effect":[]}]}),
        json!({"min":1,"max":1,"modes":[{"id":"m1","effect":[],"zzUnknown":1}]}),
        json!({"min":1,"max":1,"modes":[{"id":"m1","effect":[{"op":"bad"}]}]}),
        json!({"min":1,"max":1,"modes":[mode("m1")],"zzUnknown":1}),
        json!({"min":-1,"max":1,"modes":[mode("m1")]}),
        json!({"min":1,"max":1,"modes":[{"id":"m1","targets":[slot(0,1,1),slot(0,1,1)],"effect":[]}]}),
        json!([mode("m1")]),
    ];
    for g in &good {
        accept_v::<ModeSetAST>(g);
    }
    for b in &bad {
        reject_v::<ModeSetAST>(b);
    }
    for kind in ["SPELL", "ACTIVATED", "TRIGGERED"] {
        let base = minimal(kind);
        for g in &good {
            accept_v::<AbilityAST>(&with(&base, "modes", g.clone()));
        }
        for b in &bad {
            reject_v::<AbilityAST>(&with(&base, "modes", b.clone()));
        }
        reject_v::<AbilityAST>(&with(&base, "modes", json!(null)));
        let both = with(
            &with(&base, "modes", good[0].clone()),
            "targets",
            json!([slot(0, 1, 1)]),
        );
        accept_v::<AbilityAST>(&both);
    }
}

/// REQ-DSL-005, REQ-DSL-010 (placeholder): EffectInstructionAST needs an `op` matching
/// [A-Z][A-Z0-9_]*; other members are opaque non-null values and round-trip unchanged.
#[test]
fn req_dsl_010_effect_instruction_placeholder() {
    let good = [
        json!({"op":"DRAW"}),
        json!({"op":"A"}),
        json!({"op":"A1_B2_"}),
        json!({"op":"TEST_OP","n":3,"s":"text","b":false,"o":{"a":[1,"b",true,{"c":[]}]}}),
        json!({"op":"TEST_OP","neg":-9007199254740991i64,"empty":{},"list":[]}),
        json!({"op":"TEST_OP","player":"$P","count":{"LITERAL":1}}),
    ];
    for g in &good {
        accept_v::<EffectInstructionAST>(g);
        let a = with(&minimal("SPELL"), "effect", json!([g]));
        accept_v::<AbilityAST>(&a);
    }
    // Order of instructions is kept.
    let ab: AbilityAST = accept_v(&with(
        &minimal("SPELL"),
        "effect",
        json!([good[0], good[1]]),
    ));
    let ba: AbilityAST = accept_v(&with(
        &minimal("SPELL"),
        "effect",
        json!([good[1], good[0]]),
    ));
    assert_ne!(ab, ba);
    let bad = [
        json!({"op":"draw"}),
        json!({"op":"Draw"}),
        json!({"op":"1X"}),
        json!({"op":"_X"}),
        json!({"op":"A-B"}),
        json!({"op":"A B"}),
        json!({"op":""}),
        json!({"op":1}),
        json!({"op":null}),
        json!({}),
        json!({"n":1}),
        json!({"op":"TEST_OP","n":null}),
        json!({"op":"TEST_OP","n":[1,null]}),
        json!({"op":"TEST_OP","n":{"a":{"b":null}}}),
        json!({"op":"TEST_OP","":1}),
        json!({"op":"TEST_OP","n":9007199254740992u64}),
        json!("DRAW"),
        json!([]),
        json!(null),
    ];
    for b in &bad {
        reject_v::<EffectInstructionAST>(b);
        reject_v::<AbilityAST>(&with(&minimal("SPELL"), "effect", json!([b])));
    }
    reject::<EffectInstructionAST>(r#"{"op":"TEST_OP","n":1.5}"#);
    reject::<EffectInstructionAST>(r#"{"op":"TEST_OP","n":1e2}"#);
}

/// REQ-DSL-005: `effect` is required on SPELL, ACTIVATED and TRIGGERED; `[]` is allowed.
#[test]
fn req_dsl_005_effect_required() {
    for kind in ["SPELL", "ACTIVATED", "TRIGGERED"] {
        let base = minimal(kind);
        let empty: AbilityAST = accept_v(&base);
        let one: AbilityAST = accept_v(&with(&base, "effect", json!([{"op":"TEST_OP"}])));
        assert_ne!(empty, one);
        reject_v::<AbilityAST>(&without(&base, "effect"));
        for v in [
            json!(null),
            json!({}),
            json!({"op":"TEST_OP"}),
            json!([null]),
        ] {
            reject_v::<AbilityAST>(&with(&base, "effect", v));
        }
    }
}

/// REQ-DSL-005: SPELL ability (a fully populated one round-trips).
#[test]
fn req_dsl_005_spell_ability() {
    let full = json!({"abilityId":"sp","kind":"SPELL","functionsIn":["STACK"],"textSpan":[0,40],
        "linkIds":["l1"],"abilityWord":"testword","condition":{"pred":"TAPPED"},
        "isManaAbility":false,"targets":[slot(0,1,1)],
        "modes":{"min":1,"max":1,"modes":[mode("m1"),mode("m2")]},
        "effect":[{"op":"TEST_OP","target":{"ref":"TARGET","slot":0}}]});
    accept_v::<AbilityAST>(&full);
}

/// REQ-DSL-005: KEYWORD ability: keyword pattern and opaque params.
#[test]
fn req_dsl_005_keyword_ability() {
    let base = minimal("KEYWORD");
    let plain: AbilityAST = accept_v(&base);
    for kw in ["A", "A1", "TEST_KEYWORD", "TEST_2_", "Z9"] {
        accept_v::<AbilityAST>(&with(&base, "keyword", json!(kw)));
    }
    for kw in [
        json!(""),
        json!("test"),
        json!("Test"),
        json!("1A"),
        json!("_A"),
        json!("A-B"),
        json!("A B"),
        json!("A\u{e9}"),
        json!(null),
        json!(1),
    ] {
        reject_v::<AbilityAST>(&with(&base, "keyword", kw));
    }
    reject_v::<AbilityAST>(&without(&base, "keyword"));
    let p: AbilityAST = accept_v(&with(
        &base,
        "params",
        json!({"n":3,"s":"x","o":{"a":[1,"b",{"c":true}]},"b":false}),
    ));
    assert_ne!(p, plain);
    accept_v::<AbilityAST>(&with(&base, "params", json!({"cost":"{2}"})));
    for bad in [
        json!({"n":null}),
        json!({"n":[null]}),
        json!({"n":{"m":null}}),
        json!({"":1}),
        json!([]),
        json!("x"),
        json!(null),
        json!(1),
    ] {
        reject_v::<AbilityAST>(&with(&base, "params", bad));
    }
    for k in ["effect", "zzUnknown", "keywords", "param"] {
        reject_v::<AbilityAST>(&with(&base, k, json!({})));
    }
}
