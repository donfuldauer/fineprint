// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::scenario` spec tests (REQ-TEST-001 item 1, R12).
//! Black-box: written from the work-item brief (FP-049) only.
//!
//! Each `spec_scenario_*.rs` file includes this file with `#[path]`. All card data is the
//! synthetic library (`test.*`, R5); no clock, no environment besides `CARGO_MANIFEST_DIR` (R3).

#![allow(dead_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use fineprint_core::scenario::*;
use serde_json::{Value, json};

/// Library hash of `test.vanilla_green_2_2` (copied from tests/fixtures/cards/).
pub const GREEN_2_2_HASH: &str = "e8744f6f3091b0523468ddb9b545c8a7213bdc9828ed949e50d7ea6450b2ffa4";
/// Library hash of `test.vanilla_white_1_1`.
pub const WHITE_1_1_HASH: &str = "168ce6ad9b4766080ac7d9c8f678a991278faabc2ee1e4927880e05c3637dece";
/// Library hash of `test.flier_2_2`.
pub const FLIER_2_2_HASH: &str = "fd2d5eeeeb17df2cfc75df291119a8c5aeeabc8fc565a711c8f8d7dbf1b1c24e";
/// Library hash of `test.mana_land_green`.
pub const LAND_GREEN_HASH: &str =
    "1b4c2b9dd878bdc3d252651c269dbdd1a49f09da231752f2189e3320cfd0d6da";
/// Library hash of `test.aura_plus_2_2`.
pub const AURA_2_2_HASH: &str = "e570d8b755bd5286bca5e525cd01fd67fd6bb276cffa57de86c648a4fbb39e46";
/// Library hash of `test.draw_two`.
pub const DRAW_TWO_HASH: &str = "2de8baeed4d632a5c76d70d79b199cd1a29d97f128e1ca463f1dbea8614e134a";

/// Deterministic xorshift64 (constant seeds only; no clock, no `rand`).
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Value in `0..n` (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// The workspace root (the ancestor of `CARGO_MANIFEST_DIR` holding `tests/scenarios` or
/// `tests/fixtures`).
pub fn workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut dir: Option<&Path> = Some(start.as_path());
    while let Some(d) = dir {
        if d.join("tests").join("fixtures").join("cards").is_dir() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }
    panic!("no workspace root above {}", start.display());
}

/// Directory of the passing sample scenarios.
pub fn samples_dir() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("scenarios")
        .join("format")
}

/// Directory of the malformed / deliberately failing scenarios.
pub fn negative_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("pzl_negative")
}

/// The 13 sample names (file stem under `tests/scenarios/format/`).
pub const SAMPLES: [&str; 13] = [
    "assert_characteristics",
    "assert_zone",
    "assert_life",
    "assert_counters",
    "assert_designation",
    "assert_events_subsequence",
    "assert_events_exact",
    "assert_pendingChoice",
    "assert_result",
    "assert_absent_event",
    "assert_absent_zoneMember",
    "minimal",
    "full_header",
];

/// Reads a sample's bytes.
pub fn sample_bytes(name: &str) -> Vec<u8> {
    let p = samples_dir().join(format!("{name}.pzl"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// Reads a negative fixture's bytes.
pub fn negative_bytes(name: &str) -> Vec<u8> {
    let p = negative_dir().join(format!("{name}.pzl"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// Parses bytes that must be a valid scenario.
pub fn parse_ok(bytes: &[u8]) -> Scenario {
    match parse_scenario(bytes) {
        Ok(s) => s,
        Err(e) => panic!("expected a valid scenario, got {e} ({:?})", e.kind),
    }
}

/// Parses a JSON value that must be a valid scenario.
pub fn parse_value(v: &Value) -> Scenario {
    parse_ok(&serde_json::to_vec(v).unwrap())
}

/// A small valid base scenario as JSON, for tests that set their own assertions or script.
///
/// Players p1, p2 (life 20). p1: library o4 (top), o5; hand o3; graveyard o6. Battlefield o1
/// (p1), o2 (p2). Priority p1 in the precombat main phase.
pub fn base_json() -> Value {
    json!({
        "pzlVersion": 1,
        "title": "Spec-test base scenario",
        "requirements": ["REQ-TEST-001"],
        "cr": [],
        "format": "testformat",
        "rulesBaseline": "test-baseline",
        "cardDatabase": {"version": "synthetic-library-1", "definitionHashes": {
            "test.vanilla_green_2_2": GREEN_2_2_HASH,
            "test.vanilla_white_1_1": WHITE_1_1_HASH,
            "test.flier_2_2": FLIER_2_2_HASH,
            "test.mana_land_green": LAND_GREEN_HASH
        }},
        "prngSeed": 42,
        "players": [
            {"id": "p1", "life": 20, "zones": {
                "library": [{"id": "o4", "card": "test.mana_land_green"},
                            {"id": "o5", "card": "test.mana_land_green"}],
                "hand": [{"id": "o3", "card": "test.flier_2_2"}],
                "graveyard": [{"id": "o6", "card": "test.vanilla_white_1_1"}]
            }},
            {"id": "p2", "life": 20}
        ],
        "battlefield": [
            {"id": "o1", "card": "test.vanilla_green_2_2", "owner": "p1", "controller": "p1"},
            {"id": "o2", "card": "test.vanilla_white_1_1", "owner": "p2", "controller": "p2"}
        ],
        "stack": [],
        "turn": {"number": 3, "activePlayer": "p1", "phase": "PRECOMBAT_MAIN", "step": "MAIN"},
        "priority": "p1",
        "script": [],
        "assertions": []
    })
}

/// The base scenario with the given assertions.
pub fn with_assertions(assertions: Vec<Value>) -> Scenario {
    let mut v = base_json();
    v["assertions"] = Value::Array(assertions);
    parse_value(&v)
}

/// The base scenario with the given script and assertions.
pub fn with_script(script: Vec<Value>, assertions: Vec<Value>) -> Scenario {
    let mut v = base_json();
    v["script"] = Value::Array(script);
    v["assertions"] = Value::Array(assertions);
    parse_value(&v)
}

/// A script step passing priority, optionally with an `expect`.
pub fn pass_step(expect: Option<(&str, &str)>) -> Value {
    match expect {
        Some((chooser, ty)) => json!({
            "expect": {"chooser": chooser, "choiceType": ty},
            "answer": {"type": "PASS_PRIORITY"}
        }),
        None => json!({"answer": {"type": "PASS_PRIORITY"}}),
    }
}

/// Full `ObservedCharacteristics` JSON (every field present).
pub fn chars_json(power: i64, toughness: i64, types: &[&str], controller: &str, mv: i64) -> Value {
    json!({
        "power": power, "toughness": toughness, "types": types, "subtypes": [],
        "supertypes": [], "colors": [], "abilities": [], "controller": controller,
        "manaValue": mv
    })
}

/// Deserialises an `Observed` from JSON (camelCase, missing fields default).
pub fn observed(v: Value) -> Observed {
    serde_json::from_value(v).expect("Observed deserialises from camelCase JSON")
}

/// An `Observed` matching [`base_json`]: no events, nothing pending, game in progress.
pub fn base_observed_json() -> Value {
    json!({
        "characteristics": {
            "o1": chars_json(2, 2, &["Creature"], "p1", 2),
            "o2": chars_json(1, 1, &["Creature"], "p2", 1)
        },
        "zones": {
            "p1/library": ["o4", "o5"], "p1/hand": ["o3"], "p1/graveyard": ["o6"],
            "p1/exile": [], "p1/command": [],
            "p2/library": [], "p2/hand": [], "p2/graveyard": [], "p2/exile": [], "p2/command": [],
            "battlefield": ["o1", "o2"], "stack": [], "sharedExile": []
        },
        "life": {"p1": 20, "p2": 20},
        "counters": {},
        "designations": {},
        "events": [],
        "pending": null,
        "result": {"status": "IN_PROGRESS", "winners": [], "losers": []}
    })
}

/// [`base_observed_json`] as an `Observed`.
pub fn base_observed() -> Observed {
    observed(base_observed_json())
}

/// An `ObservedChoice`.
pub fn choice(chooser: &str, ty: &str, options: &[&str]) -> ObservedChoice {
    ObservedChoice {
        chooser: chooser.to_string(),
        choice_type: ty.to_string(),
        options: options.iter().map(|s| s.to_string()).collect(),
    }
}

/// Evaluates one assertion against an observation and returns its outcome.
pub fn eval_one(assertion: Value, obs: &Observed) -> AssertionOutcome {
    let s = with_assertions(vec![assertion]);
    let out = evaluate_assertions(&s, obs);
    assert_eq!(out.len(), 1, "one assertion gives one outcome");
    out.into_iter().next().unwrap()
}

/// `path` is `/assertions/<index>` or below it.
pub fn path_under_assertion(path: &str, index: usize) -> bool {
    let p = format!("/assertions/{index}");
    path == p || path.starts_with(&format!("{p}/"))
}

/// Checks a failing outcome: not passed, right index and kind, path under the assertion,
/// non-empty `expected`/`actual` mentioning the differing values. An empty mention is used only
/// where the differing value is an absence (an empty zone or stream, a `null`), whose spelling
/// the contract leaves open.
pub fn assert_fails(
    o: &AssertionOutcome,
    index: usize,
    kind: &str,
    expected_mentions: &str,
    actual_mentions: &str,
) {
    assert!(!o.passed, "assertion {index} ({kind}) must fail: {o:?}");
    assert_eq!(o.index, index, "{o:?}");
    assert_eq!(o.kind, kind, "{o:?}");
    assert!(
        path_under_assertion(&o.path, index),
        "path {:?} not under /assertions/{index}",
        o.path
    );
    assert!(!o.expected.is_empty() && !o.actual.is_empty(), "{o:?}");
    assert!(
        o.expected.contains(expected_mentions),
        "expected {:?} should mention {expected_mentions:?}",
        o.expected
    );
    assert!(
        o.actual.contains(actual_mentions),
        "actual {:?} should mention {actual_mentions:?}",
        o.actual
    );
}

/// Checks a passing outcome.
pub fn assert_passes(o: &AssertionOutcome, index: usize, kind: &str) {
    assert!(o.passed, "assertion {index} ({kind}) must pass: {o:?}");
    assert_eq!(o.index, index, "{o:?}");
    assert_eq!(o.kind, kind, "{o:?}");
}

/// One recorded host call.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Build(Box<Setup>),
    Answer { chooser: String, answer: Value },
}

/// Mock [`ScenarioHost`]: returns a fixed `Observed`, serves a queue of pending choices (one is
/// consumed per accepted answer), refuses the answer with the given 0-based number on demand,
/// and records `build` / `answer` calls in order.
pub struct MockHost {
    pub observed: Observed,
    pub choices: Vec<ObservedChoice>,
    pub next: usize,
    pub refuse_answer: Option<usize>,
    pub fail_build: bool,
    pub answers_seen: usize,
    pub calls: RefCell<Vec<Call>>,
}

impl MockHost {
    pub fn new(observed: Observed, choices: Vec<ObservedChoice>) -> Self {
        MockHost {
            observed,
            choices,
            next: 0,
            refuse_answer: None,
            fail_build: false,
            answers_seen: 0,
            calls: RefCell::new(Vec::new()),
        }
    }
    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl ScenarioHost for MockHost {
    fn build(&mut self, setup: &Setup) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push(Call::Build(Box::new(setup.clone())));
        if self.fail_build {
            Err("mock build refused".to_string())
        } else {
            Ok(())
        }
    }
    fn pending(&self) -> Option<ObservedChoice> {
        self.choices.get(self.next).cloned()
    }
    fn answer(&mut self, chooser: &str, answer: &Value) -> Result<(), String> {
        self.calls.borrow_mut().push(Call::Answer {
            chooser: chooser.to_string(),
            answer: answer.clone(),
        });
        let n = self.answers_seen;
        self.answers_seen += 1;
        if self.refuse_answer == Some(n) {
            return Err("mock host refused the answer".to_string());
        }
        self.next += 1;
        Ok(())
    }
    fn observe(&self) -> Observed {
        let mut o = self.observed.clone();
        o.pending = self.pending();
        o
    }
}

/// A hand-built observation of each passing sample's state (after its script), derived from the
/// sample's setup by reading the file by hand (no engine).
pub fn sample_observed(sample: &str) -> Observed {
    let mut v = json!({
        "characteristics": {},
        "zones": {},
        "life": {"p1": 20, "p2": 20},
        "counters": {},
        "designations": {},
        "events": [],
        "pending": {"chooser": "p1", "choiceType": "PRIORITY_ACTION", "options": ["PASS_PRIORITY"]},
        "result": {"status": "IN_PROGRESS", "winners": [], "losers": []}
    });
    let empty_zones = |v: &mut Value| {
        for p in ["p1", "p2"] {
            for z in ["library", "hand", "graveyard", "exile", "command"] {
                v["zones"][format!("{p}/{z}")] = json!([]);
            }
        }
        for z in ["battlefield", "stack", "sharedExile"] {
            v["zones"][z] = json!([]);
        }
    };
    empty_zones(&mut v);
    match sample {
        "assert_characteristics" => {
            v["characteristics"]["o1"] = chars_json(2, 2, &["Creature"], "p1", 2);
            v["zones"]["battlefield"] = json!(["o1"]);
        }
        "assert_zone" => {
            v["zones"]["p1/library"] = json!(["o1", "o2"]);
            v["zones"]["p1/hand"] = json!(["o3"]);
        }
        "assert_life" => {
            v["life"]["p2"] = json!(17);
        }
        "assert_counters" => {
            v["counters"]["p1"] = json!({"poison": 2});
            v["counters"]["o1"] = json!({"charge": 3});
            v["zones"]["battlefield"] = json!(["o1"]);
            v["characteristics"]["o1"] = chars_json(2, 2, &["Creature"], "p1", 2);
        }
        "assert_designation" => {
            v["designations"]["p1"] = json!({"marked": true, "label": "alpha"});
            v["designations"]["o1"] = json!({"tier": 2});
            v["zones"]["battlefield"] = json!(["o1"]);
        }
        "assert_absent_event" => {
            v["zones"]["p1/hand"] = json!(["o1"]);
        }
        "assert_absent_zoneMember" => {
            v["zones"]["p1/hand"] = json!(["o1"]);
            v["zones"]["battlefield"] = json!(["o2"]);
        }
        "full_header" => {
            v["zones"]["p1/library"] = json!(["o2"]);
            v["zones"]["p1/hand"] = json!(["o3"]);
            v["zones"]["battlefield"] = json!(["o1"]);
            // After p1's scripted pass the next player holds priority; nothing else changes.
            v["pending"] = json!({"chooser": "p2", "choiceType": "PRIORITY_ACTION", "options": ["PASS_PRIORITY"]});
        }
        "assert_events_subsequence"
        | "assert_events_exact"
        | "assert_pendingChoice"
        | "assert_result"
        | "minimal" => {}
        other => panic!("no hand-built observation for {other}"),
    }
    observed(v)
}

/// The wrong twin (`tests/data/pzl_negative/wrong_<kind>.pzl`) of each sample with an assertion,
/// and the index of the one changed assertion. `minimal` has no assertion to change.
pub const WRONG_TWINS: [(&str, &str, usize); 12] = [
    ("assert_characteristics", "wrong_characteristics", 0),
    ("assert_zone", "wrong_zone", 0),
    ("assert_life", "wrong_life", 1),
    ("assert_counters", "wrong_counters", 1),
    ("assert_designation", "wrong_designation", 3),
    ("assert_events_subsequence", "wrong_events_subsequence", 0),
    ("assert_events_exact", "wrong_events_exact", 0),
    ("assert_pendingChoice", "wrong_pendingChoice", 0),
    ("assert_result", "wrong_result", 0),
    ("assert_absent_event", "wrong_absent_event", 1),
    ("assert_absent_zoneMember", "wrong_absent_zoneMember", 0),
    ("full_header", "wrong_full_header", 0),
];

/// Every `(pointer, leaf)` of a JSON value; empty objects/arrays count as leaves.
pub fn leaves(v: &Value, prefix: String, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, x) in m {
                let k = k.replace('~', "~0").replace('/', "~1");
                leaves(x, format!("{prefix}/{k}"), out);
            }
        }
        Value::Array(a) if !a.is_empty() => {
            for (i, x) in a.iter().enumerate() {
                leaves(x, format!("{prefix}/{i}"), out);
            }
        }
        _ => out.push((prefix, v.clone())),
    }
}

/// Whether the serialised scenario keeps the file's leaf `want` at `ptr`. A serialiser may omit
/// a `null`, `{}` or `[]` (semantically a default); an unnamed card reference may come back as
/// an object with that `card`. Anything else must be equal.
pub fn leaf_matches(ser: &Value, ptr: &str, want: &Value) -> bool {
    match (ser.pointer(ptr), want) {
        (Some(g), w) if g == w => true,
        (None, Value::Null) => true,
        (None, Value::Object(m)) if m.is_empty() => true,
        (None, Value::Array(a)) if a.is_empty() => true,
        (Some(Value::Object(m)), Value::String(card)) if ptr.contains("/zones/") => {
            m.get("card") == Some(&Value::String(card.clone()))
        }
        _ => false,
    }
}
