// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the reference host `StateHost`: REQ-TEST-001 (the runner over the real engine
// components), with REQ-TURN-001 (the priority heartbeat: a pass moves priority, all passes
// end the step) and REQ-SBA-001 (the standard registry's life and toughness entries) for the
// seams it drives, and R3 (same scenario, same
// report text and state hash). Synthetic library cards only (R5).

use super::tests::{FLIER_2_2, edit, sample};
use super::*;

const WHITE_1_1_JSON: &[u8] =
    include_bytes!("../../../../tests/fixtures/cards/creatures/vanilla_white_1_1.json");
const GREEN_2_2_JSON: &[u8] =
    include_bytes!("../../../../tests/fixtures/cards/creatures/vanilla_green_2_2.json");
const FLIER_2_2_JSON: &[u8] =
    include_bytes!("../../../../tests/fixtures/cards/keywords/flier_2_2.json");
const AURA_JSON: &[u8] =
    include_bytes!("../../../../tests/fixtures/cards/auras/aura_plus_2_2.json");
const LAND_JSON: &[u8] =
    include_bytes!("../../../../tests/fixtures/cards/lands/mana_land_white.json");

const AURA_HASH: &str = "e570d8b755bd5286bca5e525cd01fd67fd6bb276cffa57de86c648a4fbb39e46";
const LAND_HASH: &str = "ba91bfe33986103e538bb863ae55626b4d25193d84b939b679ca398be3b6cfd0";

/// A host with the five library cards these tests use.
pub(super) fn host() -> StateHost {
    let mut h = StateHost::new();
    for bytes in [
        WHITE_1_1_JSON,
        GREEN_2_2_JSON,
        FLIER_2_2_JSON,
        AURA_JSON,
        LAND_JSON,
    ] {
        h.add_card_json(bytes).unwrap();
    }
    h
}

fn run_text(text: &str) -> ScenarioReport {
    let s = parse_scenario(text.as_bytes()).unwrap();
    run_scenario(&s, &mut host())
}

/// `text` with `id` -> `hash` added to the definition hashes.
fn with_hash(text: &str, id: &str, hash: &str) -> String {
    text.replacen(
        "\"test.flier_2_2\": ",
        &format!("\"{id}\": \"{hash}\",\n    \"test.flier_2_2\": "),
        1,
    )
}

/// `text` with one more battlefield object after `o5`.
fn with_permanent(text: &str, object: &str) -> String {
    text.replacen(
        "\"timestamp\": 1}\n  ],",
        &format!("\"timestamp\": 1}},\n    {object}\n  ],"),
        1,
    )
}

#[test]
fn req_test_001_sample_passes_on_the_reference_host() {
    let report = run_text(&sample());
    assert!(report.passed, "{}", report.to_text());
    assert_eq!(report.outcomes.len(), 16);
    assert!(report.script_error.is_none());
    assert_eq!(report.to_text(), "PASS test sample\n");
}

#[test]
fn req_test_001_r3_same_scenario_same_text_and_hash() {
    let s = parse_scenario(sample().as_bytes()).unwrap();
    let mut a = host();
    let mut b = host();
    let ra = run_scenario(&s, &mut a);
    let rb = run_scenario(&s, &mut b);
    assert_eq!(ra.to_text(), rb.to_text());
    assert_eq!(ra, rb);
    assert!(a.state_hash().is_some());
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.observe(), b.observe());
    // A different seed is a different state.
    let other = parse_scenario(edit("\"prngSeed\": 7", "\"prngSeed\": 8").as_bytes()).unwrap();
    let mut c = host();
    run_scenario(&other, &mut c);
    assert_ne!(a.state_hash(), c.state_hash());
}

#[test]
fn req_test_001_observed_state_matches_the_setup() {
    let s = parse_scenario(sample().as_bytes()).unwrap();
    let mut h = host();
    h.build(&s.setup).unwrap();
    // REQ-TURN-001: the active player is asked first.
    let p = h.pending().unwrap();
    assert_eq!(p.chooser, "p1");
    assert_eq!(p.choice_type, "PRIORITY_ACTION");
    assert_eq!(p.options, vec!["PASS_PRIORITY".to_owned()]);
    let o = h.observe();
    assert_eq!(o.zones["p1/library"], ["p1.library.0", "o2"]);
    assert_eq!(o.zones["p1/hand"], ["o3"]);
    assert_eq!(o.zones["p1/graveyard"], ["p1.graveyard.0", "o4"]);
    assert_eq!(o.zones["battlefield"], ["o1", "o5"]);
    assert!(!o.zones.contains_key("p1/battlefield"));
    assert!(o.zones["stack"].is_empty());
    assert_eq!(o.life["p1"], 20);
    assert_eq!(o.counters["p2"]["poison"], 2);
    assert_eq!(o.counters["o1"]["+1/+1"], 1);
    assert_eq!(o.designations["p2"]["test.flag"], serde_json::json!(true));
    assert_eq!(o.designations["o5"]["test.mark"], serde_json::json!(3));
    let o5 = &o.characteristics["o5"];
    assert_eq!((o5.power, o5.toughness), (Some(2), Some(2)));
    assert_eq!(o5.controller.as_deref(), Some("p1"));
    assert_eq!(o5.colors, ["G"]);
    // Off the battlefield a card has no controller.
    assert_eq!(o.characteristics["o3"].controller, None);
    assert!(o.events.is_empty());
    assert_eq!(o.result.status, "IN_PROGRESS");
    // After one pass priority moved on; the live state shows the pass.
    h.answer("p1", &serde_json::json!({"type": "PASS_PRIORITY"}))
        .unwrap();
    assert_eq!(h.pending().unwrap().chooser, "p2");
    let o = h.observe();
    assert_eq!(o.events.len(), 1);
    assert_eq!(o.events[0].event_type, "PRIORITY_PASSED");
    // Observed round-trips through serde and defaults missing members.
    let text = serde_json::to_string(&o).unwrap();
    assert_eq!(serde_json::from_str::<Observed>(&text).unwrap(), o);
    let empty: Observed = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, Observed::default());
    assert_eq!(empty.result.status, "IN_PROGRESS");
}

#[test]
fn req_test_001_req_sba_001_zero_life_player_loses_at_the_first_settle() {
    // REQ-SBA-001 through the heartbeat's settle: p2 at 0 life loses before p1 gets priority;
    // one pass by the last player in the game ends the step (REQ-TURN-001).
    let text = edit(
        "{\"id\": \"p2\", \"life\": 20,",
        "{\"id\": \"p2\", \"life\": 0,",
    )
    .replacen(
        ",\n    {\"expect\": {\"chooser\": \"p2\"}, \"answer\": {\"type\": \"PASS_PRIORITY\"}}",
        "",
        1,
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    assert_eq!(s.script.len(), 1);
    let mut h = host();
    let report = run_scenario(&s, &mut h);
    assert!(report.script_error.is_none(), "{}", report.to_text());
    let o = h.observe();
    assert_eq!(o.result.status, "WON");
    assert_eq!(o.result.winners, ["p1"]);
    assert_eq!(o.result.losers, ["p2"]);
    assert_eq!(o.events[0].event_type, "PLAYER_LOST");
    assert_eq!(o.events[0].fields["player"], serde_json::json!("p2"));
    // The sample's life 20, exact events, absent PLAYER_LOST and IN_PROGRESS assertions now
    // fail; every assertion was still evaluated.
    assert_eq!(report.outcomes.len(), 16);
    let failed: Vec<&str> = report
        .outcomes
        .iter()
        .filter(|o| !o.passed)
        .map(|o| o.kind.as_str())
        .collect();
    assert_eq!(failed, ["life", "events", "absent", "result"]);
}

#[test]
fn req_test_001_req_sba_001_toughness_zero_creature_goes_to_the_graveyard() {
    // REQ-SBA-001: a creature with toughness 0 or less goes to its owner's graveyard; the event
    // stream shows the ZONE_CHANGE and the zones follow the card.
    let text = edit(
        "\"counters\": {\"+1/+1\": 1}",
        "\"counters\": {\"-1/-1\": 2}",
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    let mut h = host();
    run_scenario(&s, &mut h);
    let o = h.observe();
    assert_eq!(o.zones["battlefield"], ["o5"]);
    assert_eq!(o.zones["p1/graveyard"], ["p1.graveyard.0", "o4", "o1"]);
    let zc = o
        .events
        .iter()
        .find(|e| e.event_type == "ZONE_CHANGE")
        .unwrap();
    assert_eq!(zc.fields["object"], serde_json::json!("o1"));
    assert_eq!(zc.fields["from"], serde_json::json!("BATTLEFIELD"));
    assert_eq!(zc.fields["to"], serde_json::json!("GRAVEYARD"));
}

#[test]
fn req_test_001_build_refuses_missing_and_mismatched_definitions() {
    // R11: a scenario never runs on a missing or different definition.
    let s = parse_scenario(sample().as_bytes()).unwrap();
    let mut empty = StateHost::new();
    let report = run_scenario(&s, &mut empty);
    assert!(!report.passed);
    let f = report.script_error.unwrap();
    assert_eq!(f.step, 0);
    assert!(f.message.starts_with(BUILD_FAILURE_PREFIX), "{}", f.message);
    assert!(f.message.contains("not loaded"), "{}", f.message);
    assert!(report.outcomes.is_empty());
    let wrong = "0".repeat(64);
    let s = parse_scenario(edit(FLIER_2_2, &wrong).as_bytes()).unwrap();
    let report = run_scenario(&s, &mut host());
    let message = report.script_error.unwrap().message;
    assert!(message.contains("differs from the required"), "{message}");
    // Loading twice and loading a tampered definition are refused.
    let mut h = host();
    assert!(h.add_card_json(FLIER_2_2_JSON).is_err());
    let tampered = String::from_utf8(FLIER_2_2_JSON.to_vec())
        .unwrap()
        .replace("\"LITERAL\": 2", "\"LITERAL\": 3");
    assert!(StateHost::new().add_card_json(tampered.as_bytes()).is_err());
    assert_eq!(h.card_ids().len(), 5);
    assert!(format!("{h:?}").contains("StateHost"));
}

#[test]
fn req_test_001_unsupported_items_fail_loudly() {
    let unsupported = |text: &str| {
        let s = parse_scenario(text.as_bytes()).unwrap();
        let report = run_scenario(&s, &mut host());
        assert!(!report.passed);
        let f = report.script_error.expect("a script failure");
        assert!(f.message.contains(UNSUPPORTED), "{}", f.message);
        f
    };
    // A face-down permanent.
    unsupported(&edit(
        "\"tapped\": true",
        "\"tapped\": true, \"faceDown\": true",
    ));
    // Priority held by a non-active player at setup.
    unsupported(&edit("\"priority\": \"p1\"", "\"priority\": \"p2\""));
    // A battlefield object with a static ability (its effect is not wired yet).
    let aura = with_permanent(
        &with_hash(&sample(), "test.aura_plus_2_2", AURA_HASH),
        "{\"id\": \"a1\", \"card\": \"test.aura_plus_2_2\", \"owner\": \"p1\", \"attachedTo\": \"o1\"}",
    );
    let f = unsupported(&aura);
    assert!(f.message.contains("STATIC"), "{}", f.message);
    // An answer type the host does not take yet.
    let f = unsupported(&edit(
        "\"answer\": {\"type\": \"PASS_PRIORITY\"}},\n",
        "\"answer\": {\"type\": \"PLAY_LAND\", \"object\": \"o3\"}},\n",
    ));
    assert_eq!(f.step, 0);
    // Resolving a stack object.
    let stack = edit(
        "\"stack\": [],",
        "\"stack\": [{\"id\": \"s1\", \"card\": \"test.flier_2_2\", \"owner\": \"p2\"}],",
    );
    let f = unsupported(&stack);
    assert_eq!(f.step, 1);
}

#[test]
fn req_test_001_answers_are_checked() {
    let s = parse_scenario(sample().as_bytes()).unwrap();
    let mut h = host();
    let pass = serde_json::json!({"type": "PASS_PRIORITY"});
    assert!(h.answer("p1", &pass).is_err());
    assert_eq!(h.pending(), None);
    assert_eq!(h.observe(), Observed::default());
    assert_eq!(h.state_hash(), None);
    h.build(&s.setup).unwrap();
    // Wrong chooser, not an object, extra field, a field-less answer the request refuses.
    assert!(h.answer("p2", &pass).is_err());
    assert!(h.answer("p1", &serde_json::json!("PASS_PRIORITY")).is_err());
    assert!(
        h.answer("p1", &serde_json::json!({"type": "PASS_PRIORITY", "x": 1}))
            .is_err()
    );
    let before = h.observe();
    assert!(h.answer("p1", &serde_json::json!({"type": "YES"})).is_err());
    // A refused answer changes nothing.
    assert_eq!(h.observe(), before);
    h.answer("p1", &pass).unwrap();
    h.answer("p2", &pass).unwrap();
    // REQ-TURN-001: the step ended; no choice is pending.
    assert_eq!(h.pending(), None);
    assert!(h.answer("p1", &pass).is_err());
}

#[test]
fn req_test_001_static_state_without_priority() {
    // With no priority holder nothing is asked: the scenario only checks the built state.
    let text = edit("\"priority\": \"p1\"", "\"priority\": null");
    let s = parse_scenario(text.as_bytes()).unwrap();
    let mut h = host();
    let report = run_scenario(&s, &mut h);
    let f = report.script_error.unwrap();
    assert_eq!(f.step, 0);
    assert!(f.message.contains("no choice is pending"));
    assert_eq!(h.pending(), None);
    assert!(h.observe().events.is_empty());
    assert!(
        h.answer("p1", &serde_json::json!({"type": "PASS_PRIORITY"}))
            .is_err()
    );
}

#[test]
fn req_test_001_shared_exile_command_stack_and_attachments() {
    let text = with_hash(&sample(), "test.mana_land_white", LAND_HASH)
        .replacen(
            "\"hand\": [{\"id\": \"o3\", \"card\": \"test.vanilla_green_2_2\"}],",
            "\"hand\": [{\"id\": \"o3\", \"card\": \"test.vanilla_green_2_2\"}], \"exile\": [\"test.flier_2_2\"], \"command\": [{\"id\": \"c1\", \"card\": \"test.flier_2_2\"}],",
            1,
        )
        .replacen(
            "\"stack\": [],",
            "\"stack\": [{\"id\": \"s1\", \"card\": \"test.flier_2_2\", \"owner\": \"p2\"}, {\"id\": \"s2\", \"card\": \"test.vanilla_white_1_1\", \"owner\": \"p1\", \"controller\": \"p2\"}],\n  \"sharedExile\": [{\"id\": \"x1\", \"card\": \"test.flier_2_2\", \"owner\": \"p2\"}],",
            1,
        );
    let text = with_permanent(
        &text,
        "{\"id\": \"l1\", \"card\": \"test.mana_land_white\", \"owner\": \"p1\", \"attachedTo\": \"p2\", \"flipped\": false, \"phasedOut\": true, \"controlledSinceTurn\": 3}",
    );
    let s = parse_scenario(text.as_bytes()).unwrap();
    let mut h = host();
    h.build(&s.setup).unwrap();
    let o = h.observe();
    assert_eq!(o.zones["p1/exile"], ["p1.exile.0"]);
    assert_eq!(o.zones["p1/command"], ["c1"]);
    assert_eq!(o.zones["command"], ["c1"]);
    assert_eq!(o.zones["sharedExile"], ["x1"]);
    assert!(o.zones["p2/exile"].is_empty());
    // The stack is bottom first.
    assert_eq!(o.zones["stack"], ["s1", "s2"]);
    assert_eq!(o.characteristics["s2"].controller.as_deref(), Some("p2"));
    assert_eq!(o.characteristics["l1"].abilities, ["ACTIVATED"]);
    assert_eq!(o.characteristics["l1"].power, None);
}
