// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the sample scenarios under `tests/scenarios/format/` and their deliberately
//! wrong twins under `tests/data/pzl_negative/` (REQ-TEST-001 item 1, REQ-NFR-004).
//! Black-box: written from the work-item brief (FP-049) only.
//!
//! Every sample parses and has a complete header; every wrong twin parses; against a hand-built
//! observation of the sample's setup the sample passes and its twin fails on exactly the one
//! changed assertion (`evaluate_assertions`, no engine).

#[path = "spec_scenario_support.rs"]
mod support;

use fineprint_core::scenario::*;
use serde_json::Value;
use support::*;

/// (sample, assertion kind, optional discriminating field and value).
type KindSample = (
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
);

/// REQ-TEST-001.1: exactly the 13 expected sample files exist and each parses.
#[test]
fn req_test_001_samples_exist_and_parse() {
    let mut found: Vec<String> = std::fs::read_dir(samples_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "pzl"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    found.sort();
    let mut want: Vec<String> = SAMPLES.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(found, want);
    for name in SAMPLES {
        parse_ok(&sample_bytes(name));
    }
}

/// REQ-TEST-001.1, REQ-NFR-004: every sample has a non-empty title, lists `REQ-TEST-001`, sets
/// `cr` (empty: the samples prove format mechanics), a fixed `prngSeed`, explicit turn and
/// priority, and a definition hash for every card it uses.
#[test]
fn req_test_001_samples_header_complete() {
    for name in SAMPLES {
        let bytes = sample_bytes(name);
        let s = parse_ok(&bytes);
        assert!(!s.title.is_empty(), "{name}: title");
        assert!(
            s.requirements.iter().any(|r| r == "REQ-TEST-001"),
            "{name}: requirements"
        );
        assert!(s.cr.is_empty(), "{name}: cr");
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        for key in [
            "pzlVersion",
            "format",
            "rulesBaseline",
            "cardDatabase",
            "prngSeed",
            "turn",
            "priority",
            "script",
            "assertions",
        ] {
            assert!(file.get(key).is_some(), "{name}: header field {key}");
        }
        assert!(file["prngSeed"].is_u64(), "{name}: prngSeed");
    }
    let fh: Value = serde_json::from_slice(&sample_bytes("full_header")).unwrap();
    assert!(fh["ruling"]["date"].is_string() && fh["ruling"]["ref"].is_string());
    assert!(
        !fh["cardDatabase"]["definitionHashes"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

/// REQ-TEST-001.1: there is one sample per assertion kind (and per events mode and absent
/// form), and each per-kind sample's assertions are all of its kind.
#[test]
fn req_test_001_samples_one_per_kind() {
    let per_kind: [KindSample; 11] = [
        ("assert_characteristics", "characteristics", None),
        ("assert_zone", "zone", None),
        ("assert_life", "life", None),
        ("assert_counters", "counters", None),
        ("assert_designation", "designation", None),
        (
            "assert_events_subsequence",
            "events",
            Some(("mode", "subsequence")),
        ),
        ("assert_events_exact", "events", Some(("mode", "exact"))),
        ("assert_pendingChoice", "pendingChoice", None),
        ("assert_result", "result", None),
        ("assert_absent_event", "absent", Some(("what", "event"))),
        (
            "assert_absent_zoneMember",
            "absent",
            Some(("what", "zoneMember")),
        ),
    ];
    for (name, kind, sub) in per_kind {
        let file: Value = serde_json::from_slice(&sample_bytes(name)).unwrap();
        let first = &file["assertions"][0];
        assert_eq!(first["kind"], kind, "{name}");
        if let Some((k, v)) = sub {
            assert_eq!(first[k], v, "{name}");
        }
    }
}

/// REQ-TEST-001.1: each sample passes against a hand-built observation of its setup, and its
/// wrong twin parses and fails on exactly the changed assertion index.
#[test]
fn req_test_001_samples_pass_and_wrong_twins_fail() {
    for (sample, wrong, index) in WRONG_TWINS {
        let s = parse_ok(&sample_bytes(sample));
        let obs = sample_observed(sample);
        let out = evaluate_assertions(&s, &obs);
        assert_eq!(out.len(), s.assertions.len(), "{sample}");
        assert!(!out.is_empty(), "{sample} has assertions");
        for o in &out {
            assert!(o.passed, "{sample}: {o:?}");
        }
        let w = parse_ok(&negative_bytes(wrong));
        assert_eq!(w.assertions.len(), s.assertions.len(), "{wrong}");
        assert_eq!(w.setup, s.setup, "{wrong}: same setup as its twin");
        assert_eq!(w.script, s.script, "{wrong}: same script as its twin");
        let out = evaluate_assertions(&w, &obs);
        let failed: Vec<&AssertionOutcome> = out.iter().filter(|o| !o.passed).collect();
        assert_eq!(failed.len(), 1, "{wrong}: exactly one failure: {out:?}");
        let f = failed[0];
        assert_eq!(f.index, index, "{wrong}");
        assert!(path_under_assertion(&f.path, index), "{wrong}: {f:?}");
        assert!(
            !f.expected.is_empty() && !f.actual.is_empty(),
            "{wrong}: {f:?}"
        );
        // Only the one assertion differs between the twins.
        for i in 0..s.assertions.len() {
            if i != index {
                assert_eq!(w.assertions[i], s.assertions[i], "{wrong}: assertion {i}");
            }
        }
        assert_ne!(w.assertions[index], s.assertions[index], "{wrong}");
    }
}

/// REQ-TEST-001.1: `minimal` has no assertions and passes trivially.
#[test]
fn req_test_001_samples_minimal_has_no_assertions() {
    let s = parse_ok(&sample_bytes("minimal"));
    assert!(s.assertions.is_empty() && s.script.is_empty());
    assert!(evaluate_assertions(&s, &sample_observed("minimal")).is_empty());
}
