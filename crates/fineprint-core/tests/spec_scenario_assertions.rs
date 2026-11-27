// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::scenario::evaluate_assertions` (REQ-TEST-001 item 1): for
//! every assertion kind a passing case, a deliberately wrong expectation, and the boundaries
//! the binding contract states. Observations are built by hand (no engine).
//! Black-box: written from the work-item brief (FP-049) only.

#[path = "spec_scenario_support.rs"]
mod support;

use fineprint_core::scenario::*;
use serde_json::{Value, json};
use support::*;

fn obs_with(f: impl FnOnce(&mut Value)) -> Observed {
    let mut v = base_observed_json();
    f(&mut v);
    observed(v)
}

fn events_obs(events: Value) -> Observed {
    obs_with(|v| v["events"] = events)
}

/// A stream of three events with extra fields.
fn stream() -> Value {
    json!([
        {"type": "Draw", "fields": {"player": "p1", "seq": 1}},
        {"type": "LifeChange", "fields": {"player": "p2", "delta": -7351, "seq": 2}},
        {"type": "ZoneChange", "fields": {"object": "o3", "to": "graveyard", "seq": 3}}
    ])
}

// ---------------------------------------------------------------------------------------------
// characteristics

/// REQ-TEST-001.1: characteristics pass on equal values and fail on a wrong power, naming both.
#[test]
fn req_test_001_assert_characteristics_pass_and_fail() {
    let obs = obs_with(|v| {
        v["characteristics"]["o1"] = chars_json(37, 5, &["Creature"], "p1", 4);
    });
    let a = json!({"kind": "characteristics", "object": "o1",
        "expect": {"power": 37, "toughness": 5, "types": ["Creature"], "controller": "p1", "manaValue": 4}});
    assert_passes(&eval_one(a, &obs), 0, "characteristics");
    let s = with_assertions(vec![
        json!({"kind": "life", "player": "p1", "equals": 20}),
        json!({"kind": "characteristics", "object": "o1", "expect": {"power": 41}}),
    ]);
    let out = evaluate_assertions(&s, &obs);
    assert_eq!(out.len(), 2);
    assert_passes(&out[0], 0, "life");
    assert_fails(&out[1], 1, "characteristics", "41", "37");
}

/// REQ-TEST-001.1: characteristics compare only listed keys (a subset) and arrays as sets.
#[test]
fn req_test_001_assert_characteristics_subset_and_sets() {
    let obs = obs_with(|v| {
        v["characteristics"]["o1"] = json!({
            "power": 2, "toughness": 2, "types": ["Artifact", "Creature"],
            "subtypes": ["Golem", "Beast"], "supertypes": [], "colors": ["Green", "White"],
            "abilities": ["k2", "k1"], "controller": "p2", "manaValue": 3
        });
    });
    // Unlisted keys (toughness, controller, ...) are ignored.
    assert_passes(
        &eval_one(
            json!({"kind": "characteristics", "object": "o1", "expect": {"power": 2}}),
            &obs,
        ),
        0,
        "characteristics",
    );
    // An empty expectation passes.
    assert_passes(
        &eval_one(
            json!({"kind": "characteristics", "object": "o1", "expect": {}}),
            &obs,
        ),
        0,
        "characteristics",
    );
    // Arrays compare as sets: order does not matter.
    let a = json!({"kind": "characteristics", "object": "o1", "expect": {
        "types": ["Creature", "Artifact"], "subtypes": ["Beast", "Golem"],
        "colors": ["White", "Green"], "abilities": ["k1", "k2"], "supertypes": []}});
    assert_passes(&eval_one(a, &obs), 0, "characteristics");
    // A missing member fails (set equality, not order).
    let a =
        json!({"kind": "characteristics", "object": "o1", "expect": {"colors": ["Green", "Blue"]}});
    // Colour spelling is not fixed by the contract (a name or its letter).
    let o = eval_one(a, &obs);
    assert_fails(&o, 0, "characteristics", "", "");
    assert!(
        o.expected.contains("Blue") || o.expected.contains('U'),
        "{o:?}"
    );
    assert!(
        o.actual.contains("White") || o.actual.contains('W'),
        "{o:?}"
    );
    // A proper subset of an array is not set-equal.
    let a = json!({"kind": "characteristics", "object": "o1", "expect": {"types": ["Creature"]}});
    assert_fails(
        &eval_one(a, &obs),
        0,
        "characteristics",
        "Creature",
        "Artifact",
    );
    // Wrong controller and wrong mana value.
    let a = json!({"kind": "characteristics", "object": "o1", "expect": {"controller": "p1"}});
    assert_fails(&eval_one(a, &obs), 0, "characteristics", "p1", "p2");
    let a = json!({"kind": "characteristics", "object": "o1", "expect": {"manaValue": 9}});
    assert_fails(&eval_one(a, &obs), 0, "characteristics", "9", "3");
}

/// REQ-TEST-001.1: an object with no observed characteristics fails instead of panicking.
#[test]
fn req_test_001_assert_characteristics_missing_object_fails() {
    let obs = obs_with(|v| v["characteristics"] = json!({}));
    let o = eval_one(
        json!({"kind": "characteristics", "object": "o1", "expect": {"power": 2}}),
        &obs,
    );
    assert!(!o.passed);
    assert!(path_under_assertion(&o.path, 0));
    assert!(!o.expected.is_empty() && !o.actual.is_empty());
}

// ---------------------------------------------------------------------------------------------
// zone

/// REQ-TEST-001.1: zone passes on exact order and fails on a swapped pair; `any` passes on the
/// swapped pair; `order` defaults to exact.
#[test]
fn req_test_001_assert_zone_order() {
    let obs = base_observed();
    let exact = json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o4", "o5"], "order": "exact"});
    assert_passes(&eval_one(exact, &obs), 0, "zone");
    let swapped = json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o5", "o4"], "order": "exact"});
    assert_fails(&eval_one(swapped, &obs), 0, "zone", "o5", "o4");
    let defaulted =
        json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o5", "o4"]});
    assert_fails(&eval_one(defaulted, &obs), 0, "zone", "o5", "o4");
    let any = json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o5", "o4"], "order": "any"});
    assert_passes(&eval_one(any, &obs), 0, "zone");
    let any_wrong = json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o5", "o6"], "order": "any"});
    assert_fails(&eval_one(any_wrong, &obs), 0, "zone", "o6", "o4");
    let any_short = json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o5"], "order": "any"});
    assert_fails(&eval_one(any_short, &obs), 0, "zone", "o5", "o4");
}

/// REQ-TEST-001.1: an empty zone expectation passes on an empty zone and fails on a non-empty
/// one; shared zones use their own key.
#[test]
fn req_test_001_assert_zone_empty_and_shared() {
    let obs = base_observed();
    assert_passes(
        &eval_one(
            json!({"kind": "zone", "owner": "p2", "zone": "hand", "contents": []}),
            &obs,
        ),
        0,
        "zone",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "zone", "owner": "p1", "zone": "hand", "contents": []}),
            &obs,
        ),
        0,
        "zone",
        "",
        "o3",
    );
    assert_passes(
        &eval_one(
            json!({"kind": "zone", "owner": "p1", "zone": "battlefield", "contents": ["o1", "o2"]}),
            &obs,
        ),
        0,
        "zone",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "zone", "owner": "p1", "zone": "graveyard", "contents": ["o3"]}),
            &obs,
        ),
        0,
        "zone",
        "o3",
        "o6",
    );
}

// ---------------------------------------------------------------------------------------------
// life

/// REQ-TEST-001.1: life passes on the observed total and fails on another, naming both.
#[test]
fn req_test_001_assert_life_pass_and_fail() {
    let obs = obs_with(|v| v["life"]["p2"] = json!(-4682));
    assert_passes(
        &eval_one(
            json!({"kind": "life", "player": "p2", "equals": -4682}),
            &obs,
        ),
        0,
        "life",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "life", "player": "p2", "equals": 7351}),
            &obs,
        ),
        0,
        "life",
        "7351",
        "-4682",
    );
}

// ---------------------------------------------------------------------------------------------
// counters

/// REQ-TEST-001.1: counters on a player and on an object pass and fail, naming both values.
#[test]
fn req_test_001_assert_counters_pass_and_fail() {
    let obs = obs_with(|v| {
        v["counters"]["p1"] = json!({"poison": 57});
        v["counters"]["o1"] = json!({"charge": 93, "time": 1});
    });
    assert_passes(
        &eval_one(
            json!({"kind": "counters", "target": "p1", "counter": "poison", "equals": 57}),
            &obs,
        ),
        0,
        "counters",
    );
    assert_passes(
        &eval_one(
            json!({"kind": "counters", "target": "o1", "counter": "charge", "equals": 93}),
            &obs,
        ),
        0,
        "counters",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "counters", "target": "p1", "counter": "poison", "equals": 58}),
            &obs,
        ),
        0,
        "counters",
        "58",
        "57",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "counters", "target": "o1", "counter": "charge", "equals": 39}),
            &obs,
        ),
        0,
        "counters",
        "39",
        "93",
    );
}

// ---------------------------------------------------------------------------------------------
// designation

/// REQ-TEST-001.1: designations pass on equal values (bool, string, integer) and fail on a
/// different one; `null` means absent.
#[test]
fn req_test_001_assert_designation_values_and_null() {
    let obs = obs_with(|v| {
        v["designations"]["p1"] = json!({"marked": true, "label": "alpha", "tier": 4});
        v["designations"]["o1"] = json!({"label": "gamma"});
    });
    for (target, key, val) in [
        ("p1", "marked", json!(true)),
        ("p1", "label", json!("alpha")),
        ("p1", "tier", json!(4)),
        ("o1", "label", json!("gamma")),
    ] {
        let a = json!({"kind": "designation", "target": target, "key": key, "equals": val});
        assert_passes(&eval_one(a, &obs), 0, "designation");
    }
    let a = json!({"kind": "designation", "target": "p1", "key": "label", "equals": "beta"});
    assert_fails(&eval_one(a, &obs), 0, "designation", "beta", "alpha");
    let a = json!({"kind": "designation", "target": "o1", "key": "label", "equals": "delta"});
    assert_fails(&eval_one(a, &obs), 0, "designation", "delta", "gamma");
    // A bool is not equal to a string or an integer.
    let a = json!({"kind": "designation", "target": "p1", "key": "marked", "equals": "true"});
    assert!(!eval_one(a, &obs).passed);
    // null = absent: passes on a missing key and on a target with no designations.
    let a = json!({"kind": "designation", "target": "p1", "key": "unset", "equals": null});
    assert_passes(&eval_one(a, &obs), 0, "designation");
    let a = json!({"kind": "designation", "target": "p2", "key": "marked", "equals": null});
    assert_passes(&eval_one(a, &obs), 0, "designation");
    // null fails when the key is present, and a value fails when the key is absent.
    let a = json!({"kind": "designation", "target": "p1", "key": "label", "equals": null});
    assert_fails(&eval_one(a, &obs), 0, "designation", "", "alpha");
    let a = json!({"kind": "designation", "target": "p2", "key": "marked", "equals": true});
    assert_fails(&eval_one(a, &obs), 0, "designation", "true", "");
}

// ---------------------------------------------------------------------------------------------
// events

/// REQ-TEST-001.1: subsequence passes with gaps, fails out of order, ignores extra actual
/// fields, and fails on a field mismatch.
#[test]
fn req_test_001_assert_events_subsequence() {
    let obs = events_obs(stream());
    let gaps = json!({"kind": "events", "mode": "subsequence", "events": [
        {"type": "Draw", "fields": {"player": "p1"}},
        {"type": "ZoneChange", "fields": {"object": "o3"}}]});
    assert_passes(&eval_one(gaps, &obs), 0, "events");
    let no_fields =
        json!({"kind": "events", "mode": "subsequence", "events": [{"type": "LifeChange"}]});
    assert_passes(&eval_one(no_fields, &obs), 0, "events");
    let wrong_order = json!({"kind": "events", "mode": "subsequence", "events": [
        {"type": "ZoneChange", "fields": {}}, {"type": "Draw", "fields": {}}]});
    assert_fails(
        &eval_one(wrong_order, &obs),
        0,
        "events",
        "ZoneChange",
        "Draw",
    );
    let wrong_field = json!({"kind": "events", "mode": "subsequence", "events": [
        {"type": "LifeChange", "fields": {"delta": -1111}}]});
    assert_fails(&eval_one(wrong_field, &obs), 0, "events", "-1111", "-7351");
    let missing = json!({"kind": "events", "mode": "subsequence", "events": [{"type": "Damage", "fields": {}}]});
    assert_fails(&eval_one(missing, &obs), 0, "events", "Damage", "Draw");
}

/// REQ-TEST-001.1: an empty subsequence passes against any stream, including an empty one.
#[test]
fn req_test_001_assert_events_subsequence_empty() {
    let a = json!({"kind": "events", "mode": "subsequence", "events": []});
    assert_passes(&eval_one(a.clone(), &events_obs(stream())), 0, "events");
    assert_passes(&eval_one(a, &events_obs(json!([]))), 0, "events");
}

/// REQ-TEST-001.1: exact passes on the whole stream (extra actual fields ignored), fails with an
/// extra or a missing event, and an empty list passes only against an empty stream.
#[test]
fn req_test_001_assert_events_exact() {
    let obs = events_obs(stream());
    let all = json!({"kind": "events", "mode": "exact", "events": [
        {"type": "Draw", "fields": {"player": "p1"}},
        {"type": "LifeChange", "fields": {"delta": -7351}},
        {"type": "ZoneChange", "fields": {}}]});
    assert_passes(&eval_one(all, &obs), 0, "events");
    let missing_one = json!({"kind": "events", "mode": "exact", "events": [
        {"type": "Draw", "fields": {}}, {"type": "ZoneChange", "fields": {}}]});
    assert_fails(
        &eval_one(missing_one, &obs),
        0,
        "events",
        "ZoneChange",
        "LifeChange",
    );
    let extra = json!({"kind": "events", "mode": "exact", "events": [
        {"type": "Draw", "fields": {}}, {"type": "LifeChange", "fields": {}},
        {"type": "ZoneChange", "fields": {}}, {"type": "Damage", "fields": {}}]});
    assert_fails(&eval_one(extra, &obs), 0, "events", "Damage", "ZoneChange");
    let reordered = json!({"kind": "events", "mode": "exact", "events": [
        {"type": "LifeChange", "fields": {}}, {"type": "Draw", "fields": {}},
        {"type": "ZoneChange", "fields": {}}]});
    assert!(!eval_one(reordered, &obs).passed);
    let empty = json!({"kind": "events", "mode": "exact", "events": []});
    assert_passes(
        &eval_one(empty.clone(), &events_obs(json!([]))),
        0,
        "events",
    );
    assert_fails(&eval_one(empty, &obs), 0, "events", "", "Draw");
}

// ---------------------------------------------------------------------------------------------
// absent

/// REQ-TEST-001.1: absent/event passes when no event matches (type plus listed fields) and fails
/// when one does.
#[test]
fn req_test_001_assert_absent_event() {
    let obs = events_obs(stream());
    let absent =
        json!({"kind": "absent", "what": "event", "event": {"type": "Damage", "fields": {}}});
    assert_passes(&eval_one(absent, &obs), 0, "absent");
    let other_field = json!({"kind": "absent", "what": "event",
        "event": {"type": "LifeChange", "fields": {"player": "p1"}}});
    assert_passes(&eval_one(other_field, &obs), 0, "absent");
    let present = json!({"kind": "absent", "what": "event",
        "event": {"type": "LifeChange", "fields": {"player": "p2"}}});
    assert_fails(
        &eval_one(present, &obs),
        0,
        "absent",
        "LifeChange",
        "LifeChange",
    );
    let empty_stream =
        json!({"kind": "absent", "what": "event", "event": {"type": "Draw", "fields": {}}});
    assert_passes(&eval_one(empty_stream, &events_obs(json!([]))), 0, "absent");
}

/// REQ-TEST-001.1: absent/zoneMember passes when the object is not in the zone and fails when it
/// is.
#[test]
fn req_test_001_assert_absent_zone_member() {
    let obs = base_observed();
    let a = json!({"kind": "absent", "what": "zoneMember", "owner": "p1", "zone": "graveyard", "object": "o3"});
    assert_passes(&eval_one(a, &obs), 0, "absent");
    let a = json!({"kind": "absent", "what": "zoneMember", "owner": "p1", "zone": "library", "object": "o5"});
    assert_fails(&eval_one(a, &obs), 0, "absent", "o5", "o5");
    let a = json!({"kind": "absent", "what": "zoneMember", "owner": "p2", "zone": "hand", "object": "o3"});
    assert_passes(&eval_one(a, &obs), 0, "absent");
}

// ---------------------------------------------------------------------------------------------
// pendingChoice

fn pending_obs(p: Option<ObservedChoice>) -> Observed {
    let mut o = base_observed();
    o.pending = p;
    o
}

/// REQ-TEST-001.1: pendingChoice passes on the right chooser and type, fails on a wrong chooser
/// or type, naming both.
#[test]
fn req_test_001_assert_pending_choice_pass_and_fail() {
    let obs = pending_obs(Some(choice("p1", "PRIORITY_ACTION", &["PASS_PRIORITY"])));
    let a = json!({"kind": "pendingChoice", "chooser": "p1", "choiceType": "PRIORITY_ACTION"});
    assert_passes(&eval_one(a, &obs), 0, "pendingChoice");
    let a = json!({"kind": "pendingChoice", "chooser": "p2", "choiceType": "PRIORITY_ACTION"});
    assert_fails(&eval_one(a, &obs), 0, "pendingChoice", "p2", "p1");
    let a = json!({"kind": "pendingChoice", "chooser": "p1", "choiceType": "DECLARE_ATTACKERS"});
    assert_fails(
        &eval_one(a, &obs),
        0,
        "pendingChoice",
        "DECLARE_ATTACKERS",
        "PRIORITY_ACTION",
    );
}

/// REQ-TEST-001.1: `chooser: null` passes only when nothing is pending.
#[test]
fn req_test_001_assert_pending_choice_null_chooser() {
    let a = json!({"kind": "pendingChoice", "chooser": null});
    assert_passes(&eval_one(a.clone(), &pending_obs(None)), 0, "pendingChoice");
    let obs = pending_obs(Some(choice("p2", "PRIORITY_ACTION", &[])));
    assert_fails(&eval_one(a, &obs), 0, "pendingChoice", "", "p2");
    // A chooser expected while nothing is pending fails.
    let a = json!({"kind": "pendingChoice", "chooser": "p1", "choiceType": "PRIORITY_ACTION"});
    assert_fails(
        &eval_one(a, &pending_obs(None)),
        0,
        "pendingChoice",
        "p1",
        "",
    );
}

/// REQ-TEST-001.1: `optionsMode` exact requires the same option set; contains requires only the
/// listed options.
#[test]
fn req_test_001_assert_pending_choice_options_mode() {
    let obs = pending_obs(Some(choice(
        "p1",
        "PRIORITY_ACTION",
        &["optA", "optB", "optC"],
    )));
    let ok = |opts: Value, mode: &str| {
        json!({"kind": "pendingChoice", "chooser": "p1", "choiceType": "PRIORITY_ACTION",
               "options": opts, "optionsMode": mode})
    };
    assert_passes(
        &eval_one(ok(json!(["optA", "optB", "optC"]), "exact"), &obs),
        0,
        "pendingChoice",
    );
    assert_fails(
        &eval_one(ok(json!(["optA", "optB"]), "exact"), &obs),
        0,
        "pendingChoice",
        "optB",
        "optC",
    );
    assert_fails(
        &eval_one(ok(json!(["optA", "optB", "optC", "optD"]), "exact"), &obs),
        0,
        "pendingChoice",
        "optD",
        "optC",
    );
    assert_passes(
        &eval_one(ok(json!(["optA", "optB"]), "contains"), &obs),
        0,
        "pendingChoice",
    );
    assert_passes(
        &eval_one(ok(json!(["optC"]), "contains"), &obs),
        0,
        "pendingChoice",
    );
    assert_passes(
        &eval_one(ok(json!([]), "contains"), &obs),
        0,
        "pendingChoice",
    );
    assert_fails(
        &eval_one(ok(json!(["optA", "optZ"]), "contains"), &obs),
        0,
        "pendingChoice",
        "optZ",
        "optA",
    );
}

// ---------------------------------------------------------------------------------------------
// result

/// REQ-TEST-001.1: result with only `status` passes regardless of winners/losers; listed winners
/// or losers must match; a wrong status fails naming both.
#[test]
fn req_test_001_assert_result() {
    let obs =
        obs_with(|v| v["result"] = json!({"status": "WON", "winners": ["p1"], "losers": ["p2"]}));
    assert_passes(
        &eval_one(json!({"kind": "result", "status": "WON"}), &obs),
        0,
        "result",
    );
    assert_passes(
        &eval_one(
            json!({"kind": "result", "status": "WON", "winners": ["p1"], "losers": ["p2"]}),
            &obs,
        ),
        0,
        "result",
    );
    assert_fails(
        &eval_one(json!({"kind": "result", "status": "DRAWN"}), &obs),
        0,
        "result",
        "DRAWN",
        "WON",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "result", "status": "WON", "winners": ["p2"]}),
            &obs,
        ),
        0,
        "result",
        "p2",
        "p1",
    );
    assert_fails(
        &eval_one(
            json!({"kind": "result", "status": "WON", "losers": ["p1"]}),
            &obs,
        ),
        0,
        "result",
        "p1",
        "p2",
    );
    let in_progress = base_observed();
    assert_passes(
        &eval_one(
            json!({"kind": "result", "status": "IN_PROGRESS"}),
            &in_progress,
        ),
        0,
        "result",
    );
    assert_fails(
        &eval_one(json!({"kind": "result", "status": "WON"}), &in_progress),
        0,
        "result",
        "WON",
        "IN_PROGRESS",
    );
}

// ---------------------------------------------------------------------------------------------
// across kinds

/// REQ-TEST-001.1: one outcome per assertion, in order, with its index and kind; every
/// assertion is evaluated after an earlier failure; evaluation is deterministic.
#[test]
fn req_test_001_assert_outcomes_in_order_all_evaluated() {
    let obs = obs_with(|v| {
        v["events"] = stream();
        v["counters"]["p1"] = json!({"poison": 1});
        v["designations"]["p1"] = json!({"marked": true});
        v["characteristics"]["o1"] = chars_json(2, 2, &["Creature"], "p1", 2);
        v["pending"] = json!({"chooser": "p1", "choiceType": "PRIORITY_ACTION", "options": []});
    });
    let assertions = vec![
        json!({"kind": "life", "player": "p1", "equals": 19}),
        json!({"kind": "characteristics", "object": "o1", "expect": {"power": 2}}),
        json!({"kind": "zone", "owner": "p1", "zone": "hand", "contents": ["o3"]}),
        json!({"kind": "counters", "target": "p1", "counter": "poison", "equals": 2}),
        json!({"kind": "designation", "target": "p1", "key": "marked", "equals": true}),
        json!({"kind": "events", "mode": "subsequence", "events": [{"type": "Draw", "fields": {}}]}),
        json!({"kind": "absent", "what": "event", "event": {"type": "Draw", "fields": {}}}),
        json!({"kind": "pendingChoice", "chooser": "p1", "choiceType": "PRIORITY_ACTION"}),
        json!({"kind": "result", "status": "IN_PROGRESS"}),
        json!({"kind": "absent", "what": "zoneMember", "owner": "p1", "zone": "hand", "object": "o1"}),
        json!({"kind": "events", "mode": "exact", "events": []}),
    ];
    let s = with_assertions(assertions);
    let out = evaluate_assertions(&s, &obs);
    let kinds = [
        "life",
        "characteristics",
        "zone",
        "counters",
        "designation",
        "events",
        "absent",
        "pendingChoice",
        "result",
        "absent",
        "events",
    ];
    let passed = [
        false, true, true, false, true, true, false, true, true, true, false,
    ];
    assert_eq!(out.len(), kinds.len());
    for (i, o) in out.iter().enumerate() {
        assert_eq!(o.index, i);
        assert_eq!(o.kind, kinds[i]);
        assert_eq!(o.passed, passed[i], "assertion {i}: {o:?}");
        assert!(path_under_assertion(&o.path, i), "{o:?}");
        if !o.passed {
            assert!(!o.expected.is_empty() && !o.actual.is_empty(), "{o:?}");
        }
    }
    assert_eq!(
        format!("{out:?}"),
        format!("{:?}", evaluate_assertions(&s, &obs))
    );
}

/// REQ-TEST-001.1: no assertions give no outcomes.
#[test]
fn req_test_001_assert_no_assertions() {
    let s = with_assertions(vec![]);
    assert!(evaluate_assertions(&s, &base_observed()).is_empty());
}
