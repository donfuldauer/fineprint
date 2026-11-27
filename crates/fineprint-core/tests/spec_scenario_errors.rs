// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for located loader errors and defensive limits of
//! `fineprint_core::scenario::parse_scenario` (REQ-TEST-001 item 1, R11, R12).
//! Black-box: written from the work-item brief (FP-049) only.
//!
//! Each malformed input in `tests/data/pzl_negative/` (one file per case) must be rejected with
//! the expected `ScenarioErrorKind` and JSON-Pointer `path`; the too-large, too-deep and
//! too-many-elements inputs are generated here. Where the binding contract allows two kinds
//! ("accept either"), the case lists both.

#[path = "spec_scenario_support.rs"]
mod support;

use fineprint_core::scenario::*;
use serde_json::json;
use support::*;

use ScenarioErrorKind as K;

/// Edits a base scenario in place.
type Mutation = fn(&mut serde_json::Value);

/// How the expected path is matched.
#[derive(Clone, Copy, Debug)]
enum P {
    /// Exactly this path.
    Exact(&'static str),
    /// This path or a path below it (the contract says "the path of the second occurrence",
    /// which may name the element or its `id`).
    AtOrBelow(&'static str),
}

fn path_ok(p: P, got: &str) -> bool {
    match p {
        P::Exact(x) => got == x,
        P::AtOrBelow(x) => got == x || got.starts_with(&format!("{x}/")),
    }
}

/// Common checks on every loader error: `Display` contains the path and the message, the
/// message is non-empty and under 400 chars, the path is "" or a JSON Pointer, and no more than
/// 80 consecutive chars of the input are echoed.
fn check_common(input: &[u8], e: &ScenarioError) {
    let shown = e.to_string();
    assert!(
        shown.contains(&e.path),
        "Display {shown:?} lacks path {:?}",
        e.path
    );
    assert!(!e.message.is_empty(), "empty message: {e:?}");
    assert!(
        e.message.chars().count() < 400,
        "message too long: {} chars",
        e.message.len()
    );
    assert!(
        shown.contains(&e.message),
        "Display {shown:?} lacks message"
    );
    assert!(
        e.path.is_empty() || e.path.starts_with('/'),
        "path {:?}",
        e.path
    );
    if let (Some(l), Some(c)) = (e.line, e.column) {
        assert!(l >= 1 && c >= 1, "line/column are 1-based: {e:?}");
    }
    let _: &dyn std::error::Error = e;
    assert_no_echo(input, &shown);
    assert_no_echo(input, &e.message);
}

/// No window of 81 chars of `text` occurs in `input`.
fn assert_no_echo(input: &[u8], text: &str) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 80 {
        return;
    }
    for w in chars.windows(81) {
        let s: String = w.iter().collect();
        let needle = s.as_bytes();
        let found = input.windows(needle.len()).any(|x| x == needle);
        assert!(
            !found,
            "error echoes more than 80 chars of the input: {s:?}"
        );
    }
}

fn expect_err(input: &[u8]) -> ScenarioError {
    match parse_scenario(input) {
        Ok(_) => panic!("input must be rejected"),
        Err(e) => {
            check_common(input, &e);
            e
        }
    }
}

/// One negative fixture: (file stem, accepted kinds, expected path).
fn check_file(name: &str, kinds: &[ScenarioErrorKind], path: P) -> ScenarioError {
    let input = negative_bytes(name);
    let e = expect_err(&input);
    assert!(
        kinds.contains(&e.kind),
        "{name}: kind {:?}, expected one of {kinds:?} ({e})",
        e.kind
    );
    assert!(
        path_ok(path, &e.path),
        "{name}: path {:?}, expected {path:?} ({e})",
        e.path
    );
    e
}

fn assert_syntax_located(name: &str, e: &ScenarioError) {
    assert_eq!(
        e.path, "",
        "{name}: document-level syntax error has path \"\""
    );
    let (l, c) = (
        e.line.expect("syntax line"),
        e.column.expect("syntax column"),
    );
    assert!(l >= 1 && c >= 1, "{name}: 1-based line/column, got {l}:{c}");
}

/// REQ-TEST-001.1, R12: an empty file is a syntax error with a location.
#[test]
fn req_test_001_err_empty_file() {
    let e = check_file("empty", &[K::Syntax], P::Exact(""));
    assert_syntax_located("empty", &e);
}

/// REQ-TEST-001.1, R12: non-UTF-8 bytes are a syntax error with a location on line 3 (the
/// title line of the fixture).
#[test]
fn req_test_001_err_non_utf8() {
    let input = negative_bytes("non_utf8");
    assert!(std::str::from_utf8(&input).is_err());
    let e = check_file("non_utf8", &[K::Syntax], P::Exact(""));
    assert_syntax_located("non_utf8", &e);
    assert_eq!(e.line, Some(3), "the invalid bytes are on line 3: {e}");
}

/// REQ-TEST-001.1, R12: truncated JSON is a syntax error with a location.
#[test]
fn req_test_001_err_truncated() {
    // The input ends inside `/players/0`; the contract fixes path "" only for a
    // document-level syntax error, so the path may be "" or the unfinished value's pointer.
    let e = check_file("truncated", &[K::Syntax], P::AtOrBelow(""));
    let (l, c) = (
        e.line.expect("syntax line"),
        e.column.expect("syntax column"),
    );
    assert!(l >= 1 && c >= 1, "1-based line/column, got {l}:{c}");
}

/// REQ-TEST-001.1, R12: trailing garbage after the document is a syntax error located at the
/// garbage (line 46, column 1 of the fixture).
#[test]
fn req_test_001_err_trailing_garbage() {
    let e = check_file("trailing_garbage", &[K::Syntax], P::Exact(""));
    assert_syntax_located("trailing_garbage", &e);
    assert_eq!((e.line, e.column), (Some(46), Some(1)), "{e}");
}

/// REQ-TEST-001.1: a top-level array is rejected: the top level is one object.
#[test]
fn req_test_001_err_top_level_array() {
    check_file("top_level_array", &[K::WrongType], P::Exact(""));
}

/// REQ-TEST-001.1: a missing `pzlVersion` (and a missing `title`) is `MissingField` at the root.
#[test]
fn req_test_001_err_missing_fields() {
    check_file("missing_pzl_version", &[K::MissingField], P::Exact(""));
    let e = check_file("missing_title", &[K::MissingField], P::Exact(""));
    assert!(e.message.contains("title"), "{e}");
}

/// REQ-TEST-001.1: `pzlVersion` 2 is `Unsupported` at `/pzlVersion` (DEVELOPMENT.md: "a
/// `pzlVersion` other than 1 (`Unsupported`)").
#[test]
fn req_test_001_err_pzl_version_2() {
    check_file("pzl_version_2", &[K::Unsupported], P::Exact("/pzlVersion"));
}

/// REQ-TEST-001.1, R11: a top-level `supportStatus` is an unknown field: a scenario can never
/// set a support status.
#[test]
fn req_test_001_err_unknown_top_level_field_support_status() {
    let e = check_file("unknown_top_level_field", &[K::UnknownField], P::Exact(""));
    assert!(
        e.message.contains("supportStatus"),
        "message names the field: {e}"
    );
    for field in ["supportStatus", "verified", "status", "VERIFIED"] {
        let mut v = base_json();
        v[field] = json!("VERIFIED");
        let input = serde_json::to_vec(&v).unwrap();
        let e = expect_err(&input);
        assert_eq!(e.kind, K::UnknownField, "{field}: {e}");
        assert_eq!(e.path, "", "{field}");
        assert!(e.message.contains(field), "{field}: {e}");
    }
}

/// REQ-TEST-001.1: an unknown field in a nested object is `UnknownField` at that object.
#[test]
fn req_test_001_err_unknown_nested_field() {
    let e = check_file(
        "unknown_nested_field",
        &[K::UnknownField],
        P::Exact("/turn"),
    );
    assert!(e.message.contains("extraTurnField"), "{e}");
    // Deeper objects reject unknown fields too.
    let cases: [(&str, Mutation); 5] = [
        ("/players/0", |v| v["players"][0]["mana"] = json!(1)),
        ("/battlefield/1", |v| {
            v["battlefield"][1]["extraObjectField"] = json!(1)
        }),
        ("/cardDatabase", |v| {
            v["cardDatabase"]["extraDbField"] = json!(1)
        }),
        ("/players/0/zones", |v| {
            v["players"][0]["zones"]["deck"] = json!([])
        }),
        (
            "/assertions/0",
            |v| {
                v["assertions"] =
                    json!([{"kind": "life", "player": "p1", "equals": 20, "extraAssertField": 1}])
            },
        ),
    ];
    for (path, mutate) in cases {
        let mut v = base_json();
        mutate(&mut v);
        let input = serde_json::to_vec(&v).unwrap();
        let e = expect_err(&input);
        assert_eq!(e.kind, K::UnknownField, "{path}: {e}");
        assert_eq!(e.path, path, "{e}");
    }
}

/// REQ-TEST-001.1: a string for `life` is `WrongType` at `/players/0/life`.
#[test]
fn req_test_001_err_wrong_type_life() {
    check_file(
        "wrong_type_life",
        &[K::WrongType],
        P::Exact("/players/0/life"),
    );
}

/// REQ-TEST-001.1, R3: floats are `WrongType` anywhere (`life: 20.5`, `prngSeed: 1.0`).
#[test]
fn req_test_001_err_floats() {
    check_file("float_life", &[K::WrongType], P::Exact("/players/0/life"));
    check_file("float_prng_seed", &[K::WrongType], P::Exact("/prngSeed"));
    let mut v = base_json();
    v["assertions"] = json!([{"kind": "events", "mode": "subsequence",
        "events": [{"type": "LifeChange", "fields": {"delta": 1.5}}]}]);
    let input = serde_json::to_vec(&v).unwrap();
    let e = expect_err(&input);
    assert_eq!(e.kind, K::WrongType, "{e}");
    assert_eq!(e.path, "/assertions/0/events/0/fields/delta", "{e}");
}

/// REQ-TEST-001.1: an integer outside the i64 range is rejected at its path.
#[test]
fn req_test_001_err_integer_out_of_i64() {
    check_file(
        "life_out_of_i64",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/players/1/life"),
    );
}

/// REQ-TEST-001.1: a negative `prngSeed` is rejected (`prngSeed` is a u64).
#[test]
fn req_test_001_err_negative_prng_seed() {
    check_file(
        "negative_prng_seed",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/prngSeed"),
    );
}

/// REQ-TEST-001.1, R12: a duplicate JSON key is `DuplicateKey` at the object holding it.
#[test]
fn req_test_001_err_duplicate_key() {
    check_file("duplicate_key", &[K::DuplicateKey], P::Exact("/players/0"));
    let input = br#"{"pzlVersion": 1, "pzlVersion": 1}"#;
    let e = expect_err(input);
    assert_eq!(e.kind, K::DuplicateKey, "{e}");
    assert_eq!(e.path, "");
}

/// REQ-TEST-001.1: a duplicate player id is `Duplicate` at the second occurrence.
#[test]
fn req_test_001_err_duplicate_player_id() {
    check_file(
        "duplicate_player_id",
        &[K::Duplicate],
        P::AtOrBelow("/players/1"),
    );
}

/// REQ-TEST-001.1: a duplicate object id is `Duplicate` at the second occurrence.
#[test]
fn req_test_001_err_duplicate_object_id() {
    check_file(
        "duplicate_object_id",
        &[K::Duplicate],
        P::AtOrBelow("/battlefield/1"),
    );
    // Across zones: o3 is in p1's hand in the base scenario.
    let mut v = base_json();
    v["stack"] = json!([{"id": "o3", "card": "test.flier_2_2", "owner": "p1", "controller": "p1"}]);
    let input = serde_json::to_vec(&v).unwrap();
    let e = expect_err(&input);
    assert_eq!(e.kind, K::Duplicate, "{e}");
    assert!(
        path_ok(P::AtOrBelow("/stack/0"), &e.path)
            || path_ok(P::AtOrBelow("/players/0/zones/hand/0"), &e.path),
        "{e}"
    );
}

/// REQ-TEST-001.1: unresolved player, object and card references are `UnresolvedReference` at
/// the reference.
#[test]
fn req_test_001_err_unresolved_references() {
    check_file(
        "unknown_owner",
        &[K::UnresolvedReference],
        P::Exact("/battlefield/0/owner"),
    );
    check_file(
        "unknown_attached_to",
        &[K::UnresolvedReference],
        P::Exact("/battlefield/0/attachedTo"),
    );
    check_file(
        "unknown_priority_player",
        &[K::UnresolvedReference],
        P::Exact("/priority"),
    );
    check_file(
        "unknown_assertion_object",
        &[K::UnresolvedReference],
        P::Exact("/assertions/0/object"),
    );
    check_file(
        "unknown_script_chooser",
        &[K::UnresolvedReference],
        P::Exact("/script/0/expect/chooser"),
    );
    check_file(
        "unknown_card",
        &[K::UnresolvedReference],
        P::Exact("/battlefield/0/card"),
    );
    let more: [(&str, Mutation); 4] = [
        ("/assertions/0/player", |v| {
            v["assertions"] = json!([{"kind": "life", "player": "p9", "equals": 20}])
        }),
        (
            "/assertions/0/contents/1",
            |v| {
                v["assertions"] = json!([{"kind": "zone", "owner": "p1", "zone": "hand", "contents": ["o3", "o99"]}])
            },
        ),
        ("/turn/activePlayer", |v| {
            v["turn"]["activePlayer"] = json!("p5")
        }),
        ("/players/0/zones/hand/0/card", |v| {
            v["players"][0]["zones"]["hand"][0]["card"] = json!("test.draw_two")
        }),
    ];
    for (path, mutate) in more {
        let mut v = base_json();
        mutate(&mut v);
        let input = serde_json::to_vec(&v).unwrap();
        let e = expect_err(&input);
        assert_eq!(e.kind, K::UnresolvedReference, "{path}: {e}");
        assert_eq!(e.path, path, "{e}");
    }
}

/// REQ-TEST-001.1: a malformed definition hash (63 chars, uppercase, non-hex) is rejected at
/// the hash: `OutOfRange` (DEVELOPMENT.md: a `definitionHash` that is not 64 lowercase hex
/// characters is `OutOfRange`).
#[test]
fn req_test_001_err_malformed_hash() {
    for name in ["hash_63_chars", "hash_uppercase", "hash_non_hex"] {
        check_file(
            name,
            &[K::OutOfRange],
            P::Exact("/cardDatabase/definitionHashes/test.vanilla_green_2_2"),
        );
    }
}

/// REQ-TEST-001.1: unknown enum tags (assertion kind, phase, step, zone, events mode, answer
/// type) are `UnknownTag` at the tag.
#[test]
fn req_test_001_err_unknown_tags() {
    check_file(
        "unknown_assertion_kind",
        &[K::UnknownTag],
        P::Exact("/assertions/0/kind"),
    );
    check_file("unknown_phase", &[K::UnknownTag], P::Exact("/turn/phase"));
    check_file("unknown_step", &[K::UnknownTag], P::Exact("/turn/step"));
    check_file(
        "unknown_zone_tag",
        &[K::UnknownTag],
        P::Exact("/assertions/0/zone"),
    );
    check_file(
        "events_mode_unknown",
        &[K::UnknownTag],
        P::Exact("/assertions/0/mode"),
    );
    check_file(
        "unknown_answer_type",
        &[K::UnknownTag],
        P::Exact("/script/0/answer/type"),
    );
    let more: [(&str, serde_json::Value); 4] = [
        (
            "/assertions/0/order",
            json!({"kind": "zone", "owner": "p1", "zone": "hand", "contents": [], "order": "sorted"}),
        ),
        (
            "/assertions/0/optionsMode",
            json!({"kind": "pendingChoice", "chooser": "p1", "options": [], "optionsMode": "some"}),
        ),
        (
            "/assertions/0/what",
            json!({"kind": "absent", "what": "anything"}),
        ),
        (
            "/assertions/0/status",
            json!({"kind": "result", "status": "LOST"}),
        ),
    ];
    for (path, a) in more {
        let mut v = base_json();
        v["assertions"] = json!([a]);
        let input = serde_json::to_vec(&v).unwrap();
        let e = expect_err(&input);
        assert_eq!(e.kind, K::UnknownTag, "{path}: {e}");
        assert_eq!(e.path, path, "{e}");
    }
}

/// REQ-TEST-001.1: zero players and nine players are rejected at `/players` (1..=8).
#[test]
fn req_test_001_err_player_count() {
    check_file(
        "zero_players",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/players"),
    );
    check_file(
        "nine_players",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/players"),
    );
}

/// REQ-TEST-001.1: a 201-char title and an empty title are rejected at `/title`.
#[test]
fn req_test_001_err_title_length() {
    check_file(
        "title_201_chars",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/title"),
    );
    check_file(
        "title_empty",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/title"),
    );
}

/// REQ-TEST-001.1: turn number 0 is rejected (`number` is >= 1).
#[test]
fn req_test_001_err_turn_number_zero() {
    check_file(
        "turn_number_zero",
        &[K::OutOfRange, K::WrongType],
        P::Exact("/turn/number"),
    );
}

/// REQ-TEST-001.1: empty `requirements` without a `ruling` is rejected (the contract names no
/// kind; the path is `/requirements` or the root).
#[test]
fn req_test_001_err_requirements_empty_without_ruling() {
    let input = negative_bytes("requirements_empty_without_ruling");
    let e = expect_err(&input);
    if e.kind == K::MissingField {
        // A missing required field is reported at the parent object.
        assert_eq!(e.path, "", "{e} ({:?})", e.kind);
    } else {
        assert_eq!(e.path, "/requirements", "{e} ({:?})", e.kind);
    }
}

/// REQ-TEST-001.1, R12: nesting depth 65 is `TooDeep`; depth 64 is not; an extreme depth is
/// rejected without overflowing the stack.
#[test]
fn req_test_001_err_too_deep() {
    // `{"title": [[...]]}`: the root object plus `n - 1` arrays is `n` nested containers.
    let nest = |n: usize| -> Vec<u8> {
        let inner = n - 1;
        let mut s = String::from("{\"title\": ");
        s.push_str(&"[".repeat(inner));
        s.push_str(&"]".repeat(inner));
        s.push('}');
        s.into_bytes()
    };
    let e = expect_err(&nest(65));
    assert_eq!(e.kind, K::TooDeep, "{e}");
    let e = expect_err(&nest(64));
    assert_ne!(e.kind, K::TooDeep, "depth 64 is within the limit: {e}");
    let e = expect_err(&nest(200_000));
    assert_eq!(e.kind, K::TooDeep, "{e}");
    let mut deep_obj = String::new();
    for _ in 0..100_000 {
        deep_obj.push_str("{\"a\":");
    }
    let e = expect_err(deep_obj.as_bytes());
    assert!(matches!(e.kind, K::TooDeep | K::Syntax), "{e}");
}

/// REQ-TEST-001.1, R12: an input over `MAX_SCENARIO_BYTES` is `TooLarge`, even when it is
/// otherwise valid (padding) or garbage.
#[test]
fn req_test_001_err_too_large() {
    assert_eq!(MAX_SCENARIO_BYTES, 1_048_576);
    let mut bytes = serde_json::to_vec_pretty(&base_json()).unwrap();
    bytes.resize(MAX_SCENARIO_BYTES + 1, b' ');
    let e = expect_err(&bytes);
    assert_eq!(e.kind, K::TooLarge, "{e}");
    let garbage = vec![0xffu8; MAX_SCENARIO_BYTES + 1000];
    let e = expect_err(&garbage);
    assert_eq!(e.kind, K::TooLarge, "{e}");
}

/// REQ-TEST-001.1, R12: more than 10_000 array elements in total is `TooLarge`, whether in one
/// array or spread over several.
#[test]
fn req_test_001_err_too_many_elements() {
    let mut v = base_json();
    v["cr"] = json!(vec!["x"; 10_001]);
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert_eq!(e.kind, K::TooLarge, "{e}");
    let mut v = base_json();
    v["cr"] = json!(vec!["y"; 5_001]);
    v["requirements"] = json!(vec!["REQ-TEST-001"; 5_001]);
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert_eq!(e.kind, K::TooLarge, "{e}");
    // Elements of the assertions array count the same way.
    let mut v = base_json();
    v["assertions"] = json!(vec![
        json!({"kind": "life", "player": "p1", "equals": 20});
        10_001
    ]);
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert_eq!(e.kind, K::TooLarge, "{e}");
}

/// REQ-TEST-001.1, R12: an error never echoes a long run of the offending input, even when the
/// offending value is long (a 5000-char title, a long unknown key).
#[test]
fn req_test_001_err_no_long_echo() {
    let long: String = (0..5000).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let mut v = base_json();
    v["title"] = json!(long.clone());
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert!(matches!(e.kind, K::OutOfRange | K::WrongType), "{e}");
    let mut v = base_json();
    v[long.as_str()] = json!(1);
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert_eq!(e.kind, K::UnknownField, "{e}");
    let mut v = base_json();
    v["turn"]["phase"] = json!(long);
    let e = expect_err(&serde_json::to_vec(&v).unwrap());
    assert_eq!(e.kind, K::UnknownTag, "{e}");
}

/// REQ-TEST-001.1: every file in `tests/data/pzl_negative/` that is not a wrong twin is
/// rejected (no fixture is accidentally valid), and every wrong twin parses.
#[test]
fn req_test_001_err_every_negative_fixture_rejected() {
    let mut n = 0;
    let mut entries: Vec<_> = std::fs::read_dir(negative_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        let stem = p.file_stem().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&p).unwrap();
        if WRONG_TWINS.iter().any(|(_, w, _)| *w == stem) {
            parse_ok(&bytes);
        } else {
            expect_err(&bytes);
            n += 1;
        }
    }
    assert!(
        n >= 39,
        "expected at least 39 malformed fixtures, found {n}"
    );
}
