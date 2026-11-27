// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::scenario::parse_scenario` on valid input (REQ-TEST-001
//! item 1): a full-featured scenario using every optional field, a minimal one, round-trip
//! through `serde_json`, and parsed fields equal to the file's values.
//! Black-box: written from the work-item brief (FP-049) only.

// The full-featured scenario is one large `json!` literal.
#![recursion_limit = "512"]

#[path = "spec_scenario_support.rs"]
mod support;

use fineprint_core::scenario::*;
use serde_json::{Value, json};
use support::*;

/// A scenario using every optional field of the binding schema.
fn full_json() -> Value {
    json!({
        "pzlVersion": 1,
        "title": "Full-featured scenario using every optional field",
        "requirements": ["REQ-TEST-001", "REQ-NFR-004"],
        "cr": [],
        "ruling": {"date": "2026-01-15", "ref": "synthetic-ruling-0001"},
        "format": "testformat",
        "rulesBaseline": "test-baseline",
        "cardDatabase": {"version": "synthetic-library-1", "definitionHashes": {
            "test.vanilla_green_2_2": GREEN_2_2_HASH,
            "test.vanilla_white_1_1": WHITE_1_1_HASH,
            "test.flier_2_2": FLIER_2_2_HASH,
            "test.mana_land_green": LAND_GREEN_HASH,
            "test.aura_plus_2_2": AURA_2_2_HASH,
            "test.draw_two": DRAW_TWO_HASH
        }},
        "prngSeed": 18446744073709551615u64,
        "players": [
            {"id": "p1", "life": 20, "counters": {"poison": 1, "energy": 4},
             "designations": {"marked": true, "label": "alpha", "tier": -3},
             "zones": {
                "library": ["test.mana_land_green", {"id": "o10", "card": "test.mana_land_green"}],
                "hand": [{"id": "o11", "card": "test.flier_2_2"}, "test.draw_two"],
                "graveyard": [{"id": "o12", "card": "test.vanilla_white_1_1"}],
                "exile": [{"id": "o13", "card": "test.vanilla_white_1_1"}],
                "command": [{"id": "o14", "card": "test.vanilla_green_2_2"}]
             }},
            {"id": "p2", "life": -4},
            {"id": "p3", "life": 0, "zones": {"library": [], "hand": []}}
        ],
        "battlefield": [
            {"id": "o1", "card": "test.vanilla_green_2_2", "owner": "p1", "controller": "p2",
             "tapped": true, "flipped": false, "faceDown": false, "phasedOut": false,
             "counters": {"charge": 2}, "designations": {"tier": 1}, "timestamp": 5},
            {"id": "o2", "card": "test.aura_plus_2_2", "owner": "p1", "controller": "p1",
             "tapped": false, "flipped": true, "faceDown": true, "phasedOut": true,
             "counters": {}, "designations": {"label": "beta"}, "attachedTo": "o1",
             "timestamp": 9}
        ],
        "stack": [
            {"id": "o3", "card": "test.draw_two", "owner": "p2", "controller": "p2"}
        ],
        "sharedExile": [
            {"id": "o4", "card": "test.flier_2_2", "owner": "p3", "controller": "p3",
             "tapped": false, "flipped": false, "faceDown": true, "phasedOut": false,
             "counters": {"charge": 1}, "designations": {}, "timestamp": 2}
        ],
        "turn": {"number": 7, "activePlayer": "p2", "phase": "COMBAT", "step": "DECLARE_BLOCKERS"},
        "priority": "p2",
        "script": [
            {"expect": {"chooser": "p2", "choiceType": "PRIORITY_ACTION"},
             "answer": {"type": "PASS_PRIORITY"}},
            {"answer": {"type": "PASS_PRIORITY"}}
        ],
        "assertions": [
            {"kind": "characteristics", "object": "o1", "note": "every key",
             "expect": {"power": 4, "toughness": 4, "types": ["Creature"], "subtypes": [],
                        "supertypes": [], "colors": ["Green"], "abilities": ["k1"],
                        "controller": "p2", "manaValue": 2}},
            {"kind": "zone", "owner": "p1", "zone": "graveyard", "contents": ["o12"],
             "order": "any", "note": "zone"},
            {"kind": "zone", "owner": "p1", "zone": "battlefield", "contents": ["o1", "o2"]},
            {"kind": "zone", "owner": "p3", "zone": "sharedExile", "contents": ["o4"], "order": "exact"},
            {"kind": "zone", "owner": "p2", "zone": "stack", "contents": ["o3"]},
            {"kind": "life", "player": "p2", "equals": -4, "note": "negative life"},
            {"kind": "counters", "target": "p1", "counter": "poison", "equals": 1},
            {"kind": "counters", "target": "o1", "counter": "charge", "equals": 2},
            {"kind": "designation", "target": "p1", "key": "label", "equals": "alpha"},
            {"kind": "designation", "target": "o2", "key": "missing", "equals": null},
            {"kind": "designation", "target": "p1", "key": "tier", "equals": -3},
            {"kind": "designation", "target": "p1", "key": "marked", "equals": true},
            {"kind": "events", "mode": "subsequence",
             "events": [{"type": "LifeChange", "fields": {"player": "p1", "delta": -2}}]},
            {"kind": "events", "mode": "exact", "events": [{"type": "Draw", "fields": {}}]},
            {"kind": "absent", "what": "event", "event": {"type": "Damage", "fields": {"amount": 3}}},
            {"kind": "absent", "what": "zoneMember", "owner": "p1", "zone": "hand", "object": "o1"},
            {"kind": "pendingChoice", "chooser": "p1", "choiceType": "PRIORITY_ACTION",
             "options": ["PASS_PRIORITY"], "optionsMode": "exact"},
            {"kind": "pendingChoice", "chooser": null},
            {"kind": "result", "status": "WON", "winners": ["p1"], "losers": ["p2", "p3"]}
        ]
    })
}

/// REQ-TEST-001.1: the full-featured scenario parses and its header fields equal the file's.
#[test]
fn req_test_001_parse_full_featured() {
    let v = full_json();
    let s = parse_value(&v);
    assert_eq!(s.title, "Full-featured scenario using every optional field");
    assert_eq!(
        s.requirements,
        vec!["REQ-TEST-001".to_string(), "REQ-NFR-004".to_string()]
    );
    assert!(s.cr.is_empty());
    assert_eq!(s.script.len(), 2);
    assert_eq!(s.assertions.len(), 19);
}

/// REQ-TEST-001.1: every value written in the full-featured file is present, unchanged, in the
/// scenario's serialised form (which must itself be a valid `.pzl`), per `leaf_matches`.
#[test]
fn req_test_001_parse_full_featured_fields_equal_file() {
    let v = full_json();
    let s = parse_value(&v);
    let ser = serde_json::to_value(&s).unwrap();
    let mut ls = Vec::new();
    leaves(&v, String::new(), &mut ls);
    for (ptr, want) in ls {
        assert!(
            leaf_matches(&ser, &ptr, &want),
            "field {ptr}: file has {want}, serialised scenario has {:?}",
            ser.pointer(&ptr)
        );
    }
}

/// REQ-TEST-001.1: `parse(to_vec(scenario)) == scenario` for the full-featured scenario.
#[test]
fn req_test_001_parse_full_featured_round_trip() {
    let s = parse_value(&full_json());
    let bytes = serde_json::to_vec(&s).unwrap();
    let again = parse_ok(&bytes);
    assert_eq!(again, s);
    // A second round trip is stable too.
    assert_eq!(parse_ok(&serde_json::to_vec(&again).unwrap()), s);
}

/// REQ-TEST-001.1: the minimal sample parses, round-trips, and its fields equal the file's.
#[test]
fn req_test_001_parse_minimal() {
    let bytes = sample_bytes("minimal");
    let file: Value = serde_json::from_slice(&bytes).unwrap();
    let s = parse_ok(&bytes);
    assert_eq!(s.title, file["title"].as_str().unwrap());
    assert_eq!(s.requirements, vec!["REQ-TEST-001".to_string()]);
    assert!(s.cr.is_empty());
    assert!(s.script.is_empty());
    assert!(s.assertions.is_empty());
    let again = parse_ok(&serde_json::to_vec(&s).unwrap());
    assert_eq!(again, s);
    let ser = serde_json::to_value(&s).unwrap();
    for key in [
        "pzlVersion",
        "prngSeed",
        "turn",
        "format",
        "rulesBaseline",
        "priority",
    ] {
        assert_eq!(ser.get(key), file.get(key), "top-level {key}");
    }
    assert_eq!(ser["players"][0]["id"], json!("p1"));
    assert_eq!(ser["players"][1]["life"], json!(20));
    assert_eq!(
        ser["cardDatabase"]["version"],
        file["cardDatabase"]["version"]
    );
}

/// REQ-TEST-001.1: every sample scenario round-trips and keeps every value written in its file.
#[test]
fn req_test_001_parse_samples_round_trip_and_fields() {
    for name in SAMPLES {
        let bytes = sample_bytes(name);
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        let s = parse_ok(&bytes);
        assert_eq!(
            parse_ok(&serde_json::to_vec(&s).unwrap()),
            s,
            "{name} round trip"
        );
        let ser = serde_json::to_value(&s).unwrap();
        let mut ls = Vec::new();
        leaves(&file, String::new(), &mut ls);
        for (ptr, want) in ls {
            assert!(
                leaf_matches(&ser, &ptr, &want),
                "{name}: field {ptr}: file has {want}, serialised has {:?}",
                ser.pointer(&ptr)
            );
        }
    }
}

/// REQ-TEST-001.1: a `ruling` lets `requirements` be empty.
#[test]
fn req_test_001_parse_ruling_allows_empty_requirements() {
    let mut v = base_json();
    v["requirements"] = json!([]);
    v["ruling"] = json!({"date": "2025-12-31", "ref": "synthetic-ruling-0002"});
    let s = parse_value(&v);
    assert!(s.requirements.is_empty());
    assert_eq!(parse_ok(&serde_json::to_vec(&s).unwrap()), s);
}

/// REQ-TEST-001.1: limits at their boundary are accepted: a 200-char title, eight players,
/// `prngSeed` 0 and `u64::MAX`, life at `i64::MIN` and `i64::MAX`, turn number 1, priority null,
/// and an input of exactly `MAX_SCENARIO_BYTES` bytes.
#[test]
fn req_test_001_parse_boundaries_accepted() {
    let mut v = base_json();
    v["title"] = json!("t".repeat(200));
    parse_value(&v);

    let mut v = base_json();
    let players: Vec<Value> = (1..=8)
        .map(|i| json!({"id": format!("p{i}"), "life": 20}))
        .collect();
    v["players"] = Value::Array(players.clone());
    v["players"][0] = base_json()["players"][0].clone();
    let s = parse_value(&v);
    assert_eq!(
        serde_json::to_value(&s).unwrap()["players"]
            .as_array()
            .unwrap()
            .len(),
        8
    );

    for seed in [json!(0), json!(u64::MAX)] {
        let mut v = base_json();
        v["prngSeed"] = seed.clone();
        let s = parse_value(&v);
        assert_eq!(serde_json::to_value(&s).unwrap()["prngSeed"], seed);
    }

    let mut v = base_json();
    v["players"][0]["life"] = json!(i64::MIN);
    v["players"][1]["life"] = json!(i64::MAX);
    parse_value(&v);

    let mut v = base_json();
    v["turn"]["number"] = json!(1);
    v["priority"] = Value::Null;
    parse_value(&v);

    let mut bytes = serde_json::to_vec_pretty(&base_json()).unwrap();
    assert!(bytes.len() < MAX_SCENARIO_BYTES);
    bytes.resize(MAX_SCENARIO_BYTES, b' ');
    assert_eq!(bytes.len(), MAX_SCENARIO_BYTES);
    parse_ok(&bytes);
}

/// REQ-TEST-001.1: every phase and step tag in the schema is accepted, and every listed zone tag
/// in a zone assertion.
#[test]
fn req_test_001_parse_every_tag() {
    for phase in [
        "BEGINNING",
        "PRECOMBAT_MAIN",
        "COMBAT",
        "POSTCOMBAT_MAIN",
        "ENDING",
    ] {
        let mut v = base_json();
        v["turn"]["phase"] = json!(phase);
        let step = match phase {
            "BEGINNING" => "UPKEEP",
            "COMBAT" => "BEGINNING_OF_COMBAT",
            "ENDING" => "END",
            _ => "MAIN",
        };
        v["turn"]["step"] = json!(step);
        parse_value(&v);
    }
    let pairs = [
        ("BEGINNING", "UNTAP"),
        ("BEGINNING", "UPKEEP"),
        ("BEGINNING", "DRAW"),
        ("PRECOMBAT_MAIN", "MAIN"),
        ("COMBAT", "BEGINNING_OF_COMBAT"),
        ("COMBAT", "DECLARE_ATTACKERS"),
        ("COMBAT", "DECLARE_BLOCKERS"),
        ("COMBAT", "FIRST_STRIKE_DAMAGE"),
        ("COMBAT", "COMBAT_DAMAGE"),
        ("COMBAT", "END_OF_COMBAT"),
        ("ENDING", "END"),
        ("ENDING", "CLEANUP"),
    ];
    for (phase, step) in pairs {
        let mut v = base_json();
        v["turn"]["phase"] = json!(phase);
        v["turn"]["step"] = json!(step);
        let s = parse_value(&v);
        let ser = serde_json::to_value(&s).unwrap();
        assert_eq!(ser["turn"]["step"], json!(step));
    }
    for zone in [
        "library",
        "hand",
        "graveyard",
        "exile",
        "command",
        "battlefield",
        "stack",
        "sharedExile",
    ] {
        with_assertions(vec![
            json!({"kind": "zone", "owner": "p1", "zone": zone, "contents": []}),
        ]);
    }
    for status in ["IN_PROGRESS", "WON", "DRAWN"] {
        with_assertions(vec![json!({"kind": "result", "status": status})]);
    }
}
