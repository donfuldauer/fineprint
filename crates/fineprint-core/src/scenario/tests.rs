// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the `.pzl` loader: REQ-TEST-001 (item 1, the scenario format) and R12
// (untrusted input: located errors, never a panic), with REQ-NFR-008 (every parser branch has a
// test). Synthetic cards only (`test.*` ids, R5).

use std::collections::BTreeSet;

use super::*;

pub(super) const WHITE_1_1: &str =
    "168ce6ad9b4766080ac7d9c8f678a991278faabc2ee1e4927880e05c3637dece";
pub(super) const GREEN_2_2: &str =
    "e8744f6f3091b0523468ddb9b545c8a7213bdc9828ed949e50d7ea6450b2ffa4";
pub(super) const FLIER_2_2: &str =
    "fd2d5eeeeb17df2cfc75df291119a8c5aeeabc8fc565a711c8f8d7dbf1b1c24e";

/// A valid scenario exercising every setup member and every assertion kind. Line numbers in
/// the tests below refer to this text.
pub(super) fn sample() -> String {
    format!(
        r#"{{
  "pzlVersion": 1,
  "title": "test sample",
  "requirements": ["REQ-TEST-001"],
  "cr": [],
  "format": "testformat",
  "rulesBaseline": "test-baseline",
  "cardDatabase": {{"version": "test-1", "definitionHashes": {{
    "test.vanilla_white_1_1": "{WHITE_1_1}",
    "test.vanilla_green_2_2": "{GREEN_2_2}",
    "test.flier_2_2": "{FLIER_2_2}"}}}},
  "prngSeed": 7,
  "players": [
    {{"id": "p1", "life": 20, "zones": {{
      "library": ["test.vanilla_white_1_1", {{"id": "o2", "card": "test.flier_2_2"}}],
      "hand": [{{"id": "o3", "card": "test.vanilla_green_2_2"}}],
      "graveyard": ["test.vanilla_white_1_1", {{"id": "o4", "card": "test.flier_2_2"}}]}}}},
    {{"id": "p2", "life": 20, "counters": {{"poison": 2}}, "designations": {{"test.flag": true}}}}
  ],
  "battlefield": [
    {{"id": "o1", "card": "test.flier_2_2", "owner": "p1", "counters": {{"+1/+1": 1}}, "tapped": true}},
    {{"id": "o5", "card": "test.vanilla_green_2_2", "owner": "p2", "controller": "p1",
      "designations": {{"test.mark": 3}}, "timestamp": 1}}
  ],
  "stack": [],
  "turn": {{"number": 3, "activePlayer": "p1", "phase": "PRECOMBAT_MAIN", "step": "MAIN"}},
  "priority": "p1",
  "script": [
    {{"expect": {{"chooser": "p1", "choiceType": "PRIORITY_ACTION"}}, "answer": {{"type": "PASS_PRIORITY"}}}},
    {{"expect": {{"chooser": "p2"}}, "answer": {{"type": "PASS_PRIORITY"}}}}
  ],
  "assertions": [
    {{"kind": "characteristics", "object": "o1", "expect": {{"power": 3, "toughness": 3,
      "types": ["Creature"], "colors": ["W"], "abilities": ["FLYING"], "controller": "p1",
      "manaValue": 2, "subtypes": [], "supertypes": []}}}},
    {{"kind": "zone", "owner": "p1", "zone": "library", "contents": ["p1.library.0", "o2"]}},
    {{"kind": "zone", "owner": "p1", "zone": "graveyard", "contents": ["p1.graveyard.0", "o4"], "order": "exact"}},
    {{"kind": "zone", "zone": "battlefield", "contents": ["o5", "o1"], "order": "any"}},
    {{"kind": "life", "player": "p2", "equals": 20, "note": "unchanged"}},
    {{"kind": "counters", "target": "p2", "counter": "poison", "equals": 2}},
    {{"kind": "counters", "target": "o1", "counter": "+1/+1", "equals": 1}},
    {{"kind": "designation", "target": "p2", "key": "test.flag", "equals": true}},
    {{"kind": "designation", "target": "o5", "key": "test.mark", "equals": 3}},
    {{"kind": "designation", "target": "p1", "key": "test.flag", "equals": null}},
    {{"kind": "events", "mode": "subsequence", "events": [
      {{"type": "PRIORITY_PASSED", "fields": {{"player": "p1"}}}},
      {{"type": "STEP_ENDS"}}]}},
    {{"kind": "events", "mode": "exact", "events": [
      {{"type": "PRIORITY_PASSED", "fields": {{"player": "p1", "result": "PRIORITY_MOVED", "to": "p2"}}}},
      {{"type": "PRIORITY_PASSED", "fields": {{"player": "p2", "result": "STEP_ENDS"}}}},
      {{"type": "STEP_ENDS", "fields": {{"phase": "PRECOMBAT_MAIN"}}}}]}},
    {{"kind": "absent", "what": "event", "event": {{"type": "PLAYER_LOST"}}}},
    {{"kind": "absent", "what": "zoneMember", "owner": "p1", "zone": "graveyard", "object": "o3"}},
    {{"kind": "pendingChoice", "chooser": null}},
    {{"kind": "result", "status": "IN_PROGRESS", "winners": [], "losers": []}}
  ]
}}
"#
    )
}

/// `sample()` with `from` replaced by `to` (exactly once).
pub(super) fn edit(from: &str, to: &str) -> String {
    let s = sample();
    assert_eq!(
        s.matches(from).count(),
        1,
        "edit anchor {from:?} must be unique"
    );
    s.replacen(from, to, 1)
}

fn parse_err(text: &str) -> ScenarioError {
    match parse_scenario(text.as_bytes()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

/// The 1-based (line, column) of the first occurrence of `needle` in `text`.
fn locate(text: &str, needle: &str) -> (u32, u32) {
    let at = text.find(needle).expect("needle present");
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (u32::try_from(line).unwrap(), u32::try_from(column).unwrap())
}

fn assert_error(e: &ScenarioError, kind: ScenarioErrorKind, path: &str, pos: Option<(u32, u32)>) {
    assert_eq!(e.kind, kind, "{e}");
    assert_eq!(e.path, path, "{e}");
    if let Some((l, c)) = pos {
        assert_eq!((e.line, e.column), (Some(l), Some(c)), "{e}");
    }
    assert!(!e.message.is_empty());
}

#[test]
fn req_test_001_sample_parses() {
    let s = parse_scenario(sample().as_bytes()).unwrap();
    assert_eq!(s.pzl_version, 1);
    assert_eq!(s.title, "test sample");
    assert_eq!(s.requirements, vec!["REQ-TEST-001".to_owned()]);
    assert_eq!(s.setup.players.len(), 2);
    assert_eq!(s.setup.prng_seed, 7);
    assert_eq!(s.setup.priority.as_deref(), Some("p1"));
    assert_eq!(s.setup.turn.step, StepName::Main);
    assert_eq!(s.script.len(), 2);
    assert_eq!(s.assertions.len(), 16);
    let kinds: BTreeSet<&str> = s.assertions.iter().map(Assertion::kind).collect();
    assert_eq!(kinds.len(), Assertion::KINDS.len());
    let ids: Vec<String> = s.setup.objects().into_iter().map(|o| o.id).collect();
    assert_eq!(
        ids,
        [
            "p1.library.0",
            "o2",
            "o3",
            "p1.graveyard.0",
            "o4",
            "o1",
            "o5"
        ]
    );
}

#[test]
fn req_test_001_round_trip_serialise_parse() {
    // REQ-TEST-001: parse(serialize(s)) == s, and serialising is canonical (stable).
    let s = parse_scenario(sample().as_bytes()).unwrap();
    let text = s.to_json();
    let again = parse_scenario(text.as_bytes()).unwrap();
    assert_eq!(again, s);
    assert_eq!(again.to_json(), text);
    // serde Deserialize of the canonical text gives the same value too.
    let via_serde: Scenario = serde_json::from_str(&text).unwrap();
    assert_eq!(via_serde, s);
}

#[test]
fn req_test_001_round_trip_with_optional_members() {
    let text = edit(
        "\"cr\": [],",
        "\"cr\": [\"117.3\"], \"ruling\": {\"date\": \"2024-02-29\", \"ref\": \"test ref\"},",
    )
    .replace(
        "\"stack\": [],",
        "\"stack\": [{\"id\": \"s1\", \"card\": \"test.flier_2_2\", \"owner\": \"p2\"}],\n  \"sharedExile\": [{\"id\": \"x1\", \"card\": \"test.flier_2_2\", \"owner\": \"p2\", \"phasedOut\": false}],",
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    assert_eq!(s.setup.stack.len(), 1);
    assert_eq!(s.setup.shared_exile.len(), 1);
    assert_eq!(
        s.ruling.as_ref().map(|r| r.reference.as_str()),
        Some("test ref")
    );
    assert_eq!(parse_scenario(s.to_json().as_bytes()).unwrap(), s);
}

#[test]
fn req_test_001_minimal_scenario_defaults() {
    // Optional members may be left out: battlefield, stack, script, player zones, counters.
    let text = r#"{"pzlVersion":1,"title":"t","requirements":["REQ-TEST-001"],"cr":[],"format":"f",
"rulesBaseline":"b","cardDatabase":{"version":"v","definitionHashes":{}},"prngSeed":0,
"players":[{"id":"p1","life":20}],"turn":{"number":1,"activePlayer":"p1","phase":"BEGINNING",
"step":"UPKEEP"},"priority":null,"assertions":[]}"#;
    let s = parse_scenario(text.as_bytes()).unwrap();
    assert!(s.setup.battlefield.is_empty() && s.script.is_empty());
    assert_eq!(s.setup.players[0].zones, PlayerZones::default());
    assert_eq!(s.setup.priority, None);
}

// ---- Syntax ----------------------------------------------------------------------------

#[test]
fn req_test_001_syntax_truncated() {
    let text = "{\n  \"pzlVersion\": 1,\n  \"title\": \"x";
    let e = parse_err(text);
    assert_error(&e, ScenarioErrorKind::Syntax, "/title", Some((3, 14)));
}

#[test]
fn req_test_001_syntax_empty_and_whitespace() {
    for text in ["", "   \n\t  "] {
        let e = parse_err(text);
        assert_error(&e, ScenarioErrorKind::Syntax, "", Some((1, 1)));
    }
}

#[test]
fn req_test_001_syntax_not_utf8() {
    // R12: a non-UTF-8 byte is located.
    let mut bytes = b"{\n  \"title\": \"ab".to_vec();
    bytes.push(0xFF);
    let e = parse_scenario(&bytes).unwrap_err();
    assert_error(&e, ScenarioErrorKind::Syntax, "", Some((2, 15)));
}

#[test]
fn req_test_001_syntax_bad_tokens() {
    let cases: [(&str, (u32, u32)); 12] = [
        ("{\"a\" 1}", (1, 6)),
        ("{\"a\": tru}", (1, 10)),
        ("{\"a\": 01}", (1, 7)),
        ("{\"a\": -}", (1, 8)),
        ("{\"a\": \"\\x\"}", (1, 9)),
        ("{\"a\": \"\\u12\"}", (1, 12)),
        ("{\"a\": \"\\ud800\"}", (1, 14)),
        ("{\"a\": \"\\udc00\"}", (1, 14)),
        ("{\"a\": \"tab\there\"}", (1, 11)),
        ("{\"a\": [1 2]}", (1, 10)),
        ("{} {}", (1, 4)),
        ("{1: 2}", (1, 2)),
    ];
    for (text, pos) in cases {
        let e = parse_err(text);
        assert_eq!(e.kind, ScenarioErrorKind::Syntax, "{text}: {e}");
        assert_eq!(
            (e.line, e.column),
            (Some(pos.0), Some(pos.1)),
            "{text}: {e}"
        );
    }
}

#[test]
fn req_test_001_valid_escapes_and_bom() {
    let text = format!(
        "\u{feff}{}",
        edit(
            "\"title\": \"test sample\"",
            "\"title\": \"a\\\"\\\\\\/\\b\\f\\n\\r\\t\\u00e9\\ud83d\\ude00\""
        )
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    assert_eq!(s.title, "a\"\\/\u{8}\u{c}\n\r\t\u{e9}\u{1F600}");
}

// ---- Limits ------------------------------------------------------------------------------

#[test]
fn req_test_001_too_large_bytes() {
    let mut text = sample();
    text.push_str(&" ".repeat(MAX_SCENARIO_BYTES));
    let e = parse_err(&text);
    assert_error(&e, ScenarioErrorKind::TooLarge, "", None);
    assert_eq!((e.line, e.column), (None, None));
    // Exactly at the limit is not too large (a valid scenario plus whitespace).
    let mut ok = sample();
    ok.push_str(&" ".repeat(MAX_SCENARIO_BYTES - ok.len()));
    assert!(parse_scenario(ok.as_bytes()).is_ok());
}

#[test]
fn req_test_001_too_many_array_elements() {
    let at_limit = format!("[{}]", vec!["0"; MAX_ARRAY_ELEMENTS].join(","));
    // At the limit the JSON reads fine (and then fails as a non-object top level).
    assert_eq!(parse_err(&at_limit).kind, ScenarioErrorKind::WrongType);
    let over = format!("[{}]", vec!["0"; MAX_ARRAY_ELEMENTS + 1].join(","));
    let e = parse_err(&over);
    assert_error(&e, ScenarioErrorKind::TooLarge, "", Some((1, 20_002)));
}

#[test]
fn req_test_001_too_deep() {
    let at_limit = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert_eq!(parse_err(&at_limit).kind, ScenarioErrorKind::WrongType);
    let over = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    let e = parse_err(&over);
    assert_eq!(e.kind, ScenarioErrorKind::TooDeep);
    assert_eq!((e.line, e.column), (Some(1), Some(65)));
    assert!(e.path.starts_with("/0/0"));
    let objects = format!(
        "{}1{}",
        "{\"a\":".repeat(MAX_DEPTH + 1),
        "}".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(parse_err(&objects).kind, ScenarioErrorKind::TooDeep);
}

// ---- Structure ---------------------------------------------------------------------------

#[test]
fn req_test_001_unknown_field() {
    let text = edit("\"prngSeed\": 7,", "\"prngSeed\": 7, \"bogus\": 1,");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::UnknownField,
        "",
        Some(locate(&text, "\"bogus\"")),
    );
    let text = edit("\"tapped\": true", "\"tapped\": true, \"color\": 1");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::UnknownField,
        "/battlefield/0",
        Some(locate(&text, "\"color\"")),
    );
    // attachedTo is not a shared-exile member.
    let text = edit(
        "\"stack\": [],",
        "\"stack\": [], \"sharedExile\": [{\"id\": \"x\", \"card\": \"test.flier_2_2\", \"owner\": \"p1\", \"attachedTo\": \"o1\"}],",
    );
    assert_eq!(parse_err(&text).path, "/sharedExile/0");
}

#[test]
fn req_test_001_missing_field() {
    let text = edit("\"title\": \"test sample\",\n", "");
    let e = parse_err(&text);
    assert_error(&e, ScenarioErrorKind::MissingField, "", Some((1, 1)));
    assert!(e.message.contains("title"));
    let text = edit("{\"id\": \"p1\", \"life\": 20, ", "{\"id\": \"p1\", ");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::MissingField,
        "/players/0",
        Some(locate(&text, "{\"id\": \"p1\"")),
    );
    // No version at all.
    let e = parse_err("{}");
    assert_error(&e, ScenarioErrorKind::MissingField, "", Some((1, 1)));
    // Empty requirements need a ruling.
    let e = parse_err(&edit("[\"REQ-TEST-001\"]", "[]"));
    assert_error(&e, ScenarioErrorKind::MissingField, "", Some((1, 1)));
    // A per-player zone assertion needs an owner.
    let text = edit(
        "{\"kind\": \"zone\", \"owner\": \"p1\", \"zone\": \"library\"",
        "{\"kind\": \"zone\", \"zone\": \"library\"",
    );
    assert_eq!(parse_err(&text).path, "/assertions/1");
}

#[test]
fn req_test_001_wrong_type() {
    let text = edit("\"prngSeed\": 7", "\"prngSeed\": \"7\"");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::WrongType,
        "/prngSeed",
        Some(locate(&text, "\"7\"")),
    );
    let e = parse_err(&edit("\"tapped\": true", "\"tapped\": 1"));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::WrongType, "/battlefield/0/tapped")
    );
    let e = parse_err("[]");
    assert_error(&e, ScenarioErrorKind::WrongType, "", Some((1, 1)));
    let e = parse_err(&edit(
        "{\"chooser\": \"p2\"}, \"answer\": {\"type\": \"PASS_PRIORITY\"}",
        "{\"chooser\": \"p2\"}, \"answer\": \"PASS\"",
    ));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::WrongType, "/script/1/answer")
    );
}

#[test]
fn req_test_001_floats_rejected_everywhere() {
    // R3: no floating point, wherever it appears.
    for (from, to, path) in [
        ("\"prngSeed\": 7", "\"prngSeed\": 7.0", "/prngSeed"),
        (
            "\"life\": 20, \"zones\"",
            "\"life\": 2e1, \"zones\"",
            "/players/0/life",
        ),
        (
            "{\"player\": \"p1\"}",
            "{\"player\": \"p1\", \"x\": -0.5}",
            "/assertions/10/events/0/fields/x",
        ),
    ] {
        let text = edit(from, to);
        let e = parse_err(&text);
        assert_eq!(e.kind, ScenarioErrorKind::WrongType, "{e}");
        assert_eq!(e.path, path);
        assert!(e.line.is_some() && e.column.is_some());
    }
}

#[test]
fn req_test_001_out_of_range() {
    let text = edit("\"prngSeed\": 7", "\"prngSeed\": -1");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::OutOfRange,
        "/prngSeed",
        Some({
            let (l, c) = locate(&text, "\"prngSeed\": -1");
            (l, c + 12)
        }),
    );
    let e = parse_err(&edit(
        "\"prngSeed\": 7",
        "\"prngSeed\": 18446744073709551616",
    ));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::OutOfRange, "/prngSeed")
    );
    let e = parse_err(&edit(
        "\"prngSeed\": 7",
        "\"prngSeed\": 123456789012345678901234",
    ));
    assert_eq!(e.kind, ScenarioErrorKind::OutOfRange);
    // u64::MAX is a valid seed; i64 fields refuse values above i64::MAX.
    assert!(
        parse_scenario(edit("\"prngSeed\": 7", "\"prngSeed\": 18446744073709551615").as_bytes())
            .is_ok()
    );
    let e = parse_err(&edit("\"equals\": 20", "\"equals\": 9223372036854775808"));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::OutOfRange, "/assertions/4/equals")
    );
    let e = parse_err(&edit("\"poison\": 2", "\"poison\": 0"));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::OutOfRange, "/players/1/counters/poison")
    );
    let e = parse_err(&edit("\"number\": 3", "\"number\": 0"));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::OutOfRange, "/turn/number")
    );
    let e = parse_err(&edit(
        "\"title\": \"test sample\"",
        &format!("\"title\": \"{}\"", "x".repeat(201)),
    ));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::OutOfRange, "/title")
    );
    let e = parse_err(&edit("\"title\": \"test sample\"", "\"title\": \"\""));
    assert_eq!(e.kind, ScenarioErrorKind::OutOfRange);
    // A hash of the wrong length or not lowercase hex.
    for bad in [FLIER_2_2[1..].to_owned(), FLIER_2_2.to_uppercase()] {
        let e = parse_err(&edit(FLIER_2_2, &bad));
        assert_eq!(
            (e.kind, e.path.as_str()),
            (
                ScenarioErrorKind::OutOfRange,
                "/cardDatabase/definitionHashes/test.flier_2_2"
            )
        );
    }
}

#[test]
fn req_test_001_player_count_range() {
    let nine: Vec<String> = (1..=9)
        .map(|i| format!("{{\"id\": \"q{i}\", \"life\": 1}}"))
        .collect();
    let text = sample();
    let start = text.find("\"players\": [").unwrap();
    let end = text.find("\"battlefield\"").unwrap();
    let bad = format!(
        "{}\"players\": [{}],\n  {}",
        &text[..start],
        nine.join(","),
        &text[end..]
    );
    let e = parse_err(&bad);
    assert_error(
        &e,
        ScenarioErrorKind::OutOfRange,
        "/players",
        Some(locate(&bad, "[{\"id\": \"q1\"")),
    );
    let none = format!("{}\"players\": [],\n  {}", &text[..start], &text[end..]);
    assert_eq!(parse_err(&none).kind, ScenarioErrorKind::OutOfRange);
}

#[test]
fn req_test_001_unknown_tag() {
    for (from, to, path) in [
        (
            "\"p1\", \"phase\": \"PRECOMBAT_MAIN\"",
            "\"p1\", \"phase\": \"LUNCH\"",
            "/turn/phase",
        ),
        ("\"step\": \"MAIN\"", "\"step\": \"NAP\"", "/turn/step"),
        (
            "{\"kind\": \"life\"",
            "{\"kind\": \"lifetotal\"",
            "/assertions/4/kind",
        ),
        (
            "\"order\": \"any\"",
            "\"order\": \"random\"",
            "/assertions/3/order",
        ),
        (
            "\"mode\": \"exact\"",
            "\"mode\": \"fuzzy\"",
            "/assertions/11/mode",
        ),
        (
            "\"status\": \"IN_PROGRESS\"",
            "\"status\": \"PAUSED\"",
            "/assertions/15/status",
        ),
        (
            "\"what\": \"event\"",
            "\"what\": \"thing\"",
            "/assertions/12/what",
        ),
        (
            "\"zone\": \"battlefield\"",
            "\"zone\": \"moon\"",
            "/assertions/3/zone",
        ),
        (
            "\"choiceType\": \"PRIORITY_ACTION\"",
            "\"choiceType\": \"PRIORITY\"",
            "/script/0/expect/choiceType",
        ),
        (
            "{\"chooser\": \"p2\"}, \"answer\": {\"type\": \"PASS_PRIORITY\"}",
            "{\"chooser\": \"p2\"}, \"answer\": {\"type\": \"CONCEDE\"}",
            "/script/1/answer/type",
        ),
        (
            "{\"kind\": \"pendingChoice\", \"chooser\": null}",
            "{\"kind\": \"pendingChoice\", \"chooser\": \"p1\", \"optionsMode\": \"most\"}",
            "/assertions/14/optionsMode",
        ),
    ] {
        let text = edit(from, to);
        let e = parse_err(&text);
        assert_eq!(e.kind, ScenarioErrorKind::UnknownTag, "{to}: {e}");
        assert_eq!(e.path, path);
        assert!(e.line.is_some());
    }
}

#[test]
fn req_test_001_duplicate_key() {
    let text = edit("\"prngSeed\": 7,", "\"prngSeed\": 7, \"prngSeed\": 8,");
    let e = parse_err(&text);
    let first = locate(&text, "\"prngSeed\"");
    assert_error(
        &e,
        ScenarioErrorKind::DuplicateKey,
        "",
        Some((first.0, first.1 + 15)),
    );
    let e = parse_err(&edit("\"poison\": 2", "\"poison\": 2, \"poison\": 3"));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::DuplicateKey, "/players/1/counters")
    );
}

#[test]
fn req_test_001_unresolved_reference() {
    for (from, to, path) in [
        (
            "\"player\": \"p2\", \"equals\": 20",
            "\"player\": \"p9\", \"equals\": 20",
            "/assertions/4/player",
        ),
        (
            "\"object\": \"o1\"",
            "\"object\": \"o99\"",
            "/assertions/0/object",
        ),
        (
            "\"activePlayer\": \"p1\"",
            "\"activePlayer\": \"p3\"",
            "/turn/activePlayer",
        ),
        (
            "\"priority\": \"p1\"",
            "\"priority\": \"nobody\"",
            "/priority",
        ),
        (
            "{\"chooser\": \"p2\"}",
            "{\"chooser\": \"p7\"}",
            "/script/1/expect/chooser",
        ),
        (
            "\"owner\": \"p2\", \"controller\": \"p1\"",
            "\"owner\": \"p2\", \"controller\": \"px\"",
            "/battlefield/1/controller",
        ),
        (
            "\"tapped\": true",
            "\"tapped\": true, \"attachedTo\": \"o3\"",
            "/battlefield/0/attachedTo",
        ),
        (
            "\"contents\": [\"o5\", \"o1\"]",
            "\"contents\": [\"o5\", \"o6\"]",
            "/assertions/3/contents/1",
        ),
        (
            "\"target\": \"o1\"",
            "\"target\": \"zz\"",
            "/assertions/6/target",
        ),
        (
            "{\"id\": \"o3\", \"card\": \"test.vanilla_green_2_2\"}",
            "{\"id\": \"o3\", \"card\": \"test.unknown\"}",
            "/players/0/zones/hand/0/card",
        ),
        (
            "\"library\": [\"test.vanilla_white_1_1\"",
            "\"library\": [\"test.missing\"",
            "/players/0/zones/library/0",
        ),
        (
            "\"status\": \"IN_PROGRESS\", \"winners\": []",
            "\"status\": \"IN_PROGRESS\", \"winners\": [\"p5\"]",
            "/assertions/15/winners/0",
        ),
    ] {
        let text = edit(from, to);
        let e = parse_err(&text);
        assert_eq!(e.kind, ScenarioErrorKind::UnresolvedReference, "{to}: {e}");
        assert_eq!(e.path, path);
        assert!(e.line.is_some(), "{e}");
    }
    // A script answer naming an undeclared object.
    let text = edit(
        "{\"chooser\": \"p2\"}, \"answer\": {\"type\": \"PASS_PRIORITY\"}",
        "{\"chooser\": \"p2\"}, \"answer\": {\"type\": \"PLAY_LAND\", \"object\": \"o42\"}",
    );
    assert_eq!(parse_err(&text).path, "/script/1/answer/object");
}

#[test]
fn req_test_001_duplicate_id() {
    let text = edit(
        "{\"id\": \"p2\", \"life\": 20,",
        "{\"id\": \"p1\", \"life\": 20,",
    );
    let e = parse_err(&text);
    assert_error(&e, ScenarioErrorKind::Duplicate, "/players/1/id", None);
    assert!(e.line.is_some());
    let e = parse_err(&edit("{\"id\": \"o5\"", "{\"id\": \"o1\""));
    assert_eq!(
        (e.kind, e.path.as_str()),
        (ScenarioErrorKind::Duplicate, "/battlefield/1/id")
    );
    // An object id equal to a player id.
    let e = parse_err(&edit("{\"id\": \"o5\"", "{\"id\": \"p2\""));
    assert_eq!(e.kind, ScenarioErrorKind::Duplicate);
}

#[test]
fn req_test_001_unsupported_version() {
    let text = edit("\"pzlVersion\": 1", "\"pzlVersion\": 2");
    let e = parse_err(&text);
    assert_error(
        &e,
        ScenarioErrorKind::Unsupported,
        "/pzlVersion",
        Some((2, 17)),
    );
    // The version is checked before anything else.
    let e = parse_err("{\"pzlVersion\": 3, \"future\": true}");
    assert_eq!(e.kind, ScenarioErrorKind::Unsupported);
    let e = parse_err("{\"pzlVersion\": \"1\"}");
    assert_eq!(e.kind, ScenarioErrorKind::WrongType);
}

#[test]
fn req_test_001_invalid_values() {
    for (from, to, path) in [
        ("\"step\": \"MAIN\"", "\"step\": \"UPKEEP\"", "/turn/step"),
        ("[\"REQ-TEST-001\"]", "[\"TEST-001\"]", "/requirements/0"),
        ("\"format\": \"testformat\"", "\"format\": \"\"", "/format"),
        (
            "\"cr\": [],",
            "\"cr\": [], \"ruling\": {\"date\": \"2024-13-01\", \"ref\": \"r\"},",
            "/ruling/date",
        ),
        ("\"poison\": 2", "\"\": 2", "/players/1/counters/"),
    ] {
        let text = edit(from, to);
        let e = parse_err(&text);
        assert_eq!(e.kind, ScenarioErrorKind::Invalid, "{to}: {e}");
        assert_eq!(e.path, path);
    }
}

#[test]
fn req_test_001_ruling_allows_empty_requirements() {
    let text = edit(
        "\"requirements\": [\"REQ-TEST-001\"],",
        "\"requirements\": [], \"ruling\": {\"date\": \"2020-01-31\", \"ref\": \"test ruling\"},",
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    assert!(s.requirements.is_empty());
}

#[test]
fn req_test_001_error_display_and_messages_are_bounded() {
    // R12: messages quote at most 80 characters of input; path segments are cut and sanitised.
    let long = "k".repeat(500);
    let text = edit(
        "\"prngSeed\": 7,",
        &format!("\"prngSeed\": 7, \"{long}\\n\": 1,"),
    );
    let e = parse_err(&text);
    assert_eq!(e.kind, ScenarioErrorKind::UnknownField);
    assert!(e.message.matches('k').count() <= 80, "{}", e.message);
    assert!(e.message.len() <= 120, "{}", e.message);
    assert!(!e.path.contains('\n'));
    let shown = e.to_string();
    assert_eq!(
        shown,
        format!(
            "{}:{}:{}: {}",
            e.path,
            e.line.unwrap(),
            e.column.unwrap(),
            e.message
        )
    );
    let big = ScenarioError {
        path: String::new(),
        line: None,
        column: None,
        kind: ScenarioErrorKind::TooLarge,
        message: "m".to_owned(),
    };
    assert_eq!(big.to_string(), ":?:?: m");
    let source: &dyn std::error::Error = &big;
    assert!(source.source().is_none());
    assert_eq!(ScenarioErrorKind::TooLarge.to_string(), "too-large");
    let value = "v".repeat(300);
    let e = parse_err(&edit(
        "\"p1\", \"phase\": \"PRECOMBAT_MAIN\"",
        &format!("\"p1\", \"phase\": \"{value}\""),
    ));
    assert!(e.message.matches('v').count() <= 80, "{}", e.message);
}

#[test]
fn req_test_001_every_kind_has_a_name() {
    let kinds = [
        ScenarioErrorKind::Syntax,
        ScenarioErrorKind::TooLarge,
        ScenarioErrorKind::TooDeep,
        ScenarioErrorKind::UnknownField,
        ScenarioErrorKind::MissingField,
        ScenarioErrorKind::WrongType,
        ScenarioErrorKind::OutOfRange,
        ScenarioErrorKind::UnknownTag,
        ScenarioErrorKind::DuplicateKey,
        ScenarioErrorKind::UnresolvedReference,
        ScenarioErrorKind::Duplicate,
        ScenarioErrorKind::Unsupported,
        ScenarioErrorKind::Invalid,
    ];
    let names: BTreeSet<&str> = kinds.iter().map(|k| k.as_str()).collect();
    assert_eq!(names.len(), kinds.len());
}

#[test]
fn req_test_001_pointer_position() {
    let text = sample();
    assert_eq!(pointer_position(text.as_bytes(), ""), Some((1, 1)));
    assert_eq!(
        pointer_position(text.as_bytes(), "/assertions/4"),
        Some(locate(&text, "{\"kind\": \"life\""))
    );
    assert_eq!(
        pointer_position(
            text.as_bytes(),
            "/cardDatabase/definitionHashes/test.flier_2_2"
        ),
        Some(locate(&text, &format!("\"{FLIER_2_2}\"")))
    );
    assert_eq!(pointer_position(text.as_bytes(), "/nope"), None);
    assert_eq!(pointer_position(text.as_bytes(), "/title/0"), None);
    assert_eq!(pointer_position(text.as_bytes(), "x"), None);
    assert_eq!(pointer_position(b"{", ""), None);
    assert_eq!(pointer_position(b"{\"a/b~\": 1}", "/a~1b~0"), Some((1, 10)));
}

// ---- Fuzz ---------------------------------------------------------------------------------

/// A seeded xorshift64* generator (deterministic, no crate).
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            usize::try_from(self.next() % (n as u64)).unwrap_or(0)
        }
    }
}

fn check_no_panic(bytes: &[u8]) {
    match parse_scenario(bytes) {
        Ok(s) => {
            // A mutation that still parses must round-trip too.
            assert_eq!(parse_scenario(s.to_json().as_bytes()).as_ref(), Ok(&s));
        }
        Err(e) => {
            assert!(!e.message.is_empty(), "empty message for {bytes:?}");
            assert!(e.path.is_empty() || e.path.starts_with('/'), "{}", e.path);
            assert!(e.line.is_none_or(|l| l >= 1));
            assert!(e.column.is_none_or(|c| c >= 1));
        }
    }
}

#[test]
fn req_test_001_r12_fuzz_random_and_mutated_inputs() {
    // R12: bounded random bytes plus truncations, byte flips, deleted and duplicated chunks of a
    // valid scenario: never a panic; every error has a message and a pointer path.
    let valid = sample().into_bytes();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases = 0usize;
    let alphabet = b"{}[]\":,0123456789-.eE truefalsn\\u";
    for _ in 0..1_000 {
        let len = rng.below(300);
        let bytes: Vec<u8> = (0..len).map(|_| (rng.next() & 0xFF) as u8).collect();
        check_no_panic(&bytes);
        // Bytes from the JSON alphabet get further into the reader.
        let bytes: Vec<u8> = (0..len)
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        check_no_panic(&bytes);
        cases += 2;
    }
    for i in 0..1_500 {
        let cut = rng.below(valid.len());
        check_no_panic(&valid[..cut]);
        let mut flipped = valid.clone();
        let at = rng.below(flipped.len());
        flipped[at] ^= 1u8 << (i % 8);
        check_no_panic(&flipped);
        cases += 2;
    }
    for _ in 0..1_500 {
        let a = rng.below(valid.len());
        let b = (a + rng.below(64)).min(valid.len());
        let mut deleted = valid.clone();
        deleted.drain(a..b);
        check_no_panic(&deleted);
        let mut duplicated = valid.clone();
        let chunk: Vec<u8> = valid[a..b].to_vec();
        let at = rng.below(duplicated.len());
        duplicated.splice(at..at, chunk);
        check_no_panic(&duplicated);
        cases += 2;
    }
    assert!(cases >= 5_000, "{cases}");
}
