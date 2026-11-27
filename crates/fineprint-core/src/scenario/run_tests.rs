// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the runner and the assertion evaluator: REQ-TEST-001 (every assertion kind
// passes and fails, subsequence versus exact event matching, absence, script errors stop the
// run, every assertion is evaluated after a failure, deterministic report text). A scripted
// fake host; no card data (R5).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::tests::sample;
use super::*;

/// A fake host: a fixed observation and a queue of pending choices.
#[derive(Default)]
struct Fake {
    observed: Observed,
    choices: Vec<ObservedChoice>,
    refuse: bool,
    fail_build: bool,
    answers: Vec<(String, Value)>,
}

impl ScenarioHost for Fake {
    fn build(&mut self, _setup: &Setup) -> Result<(), String> {
        if self.fail_build {
            Err("test.no_build".to_owned())
        } else {
            Ok(())
        }
    }

    fn pending(&self) -> Option<ObservedChoice> {
        self.choices.first().cloned()
    }

    fn answer(&mut self, chooser: &str, answer: &Value) -> Result<(), String> {
        if self.refuse {
            return Err("test.refused".to_owned());
        }
        self.answers.push((chooser.to_owned(), answer.clone()));
        self.choices.remove(0);
        Ok(())
    }

    fn observe(&self) -> Observed {
        let mut o = self.observed.clone();
        o.pending = self.pending();
        o
    }
}

fn priority(chooser: &str) -> ObservedChoice {
    ObservedChoice {
        chooser: chooser.to_owned(),
        choice_type: "PRIORITY_ACTION".to_owned(),
        options: vec!["PASS_PRIORITY".to_owned()],
    }
}

fn event(t: &str, fields: Value) -> ObservedEvent {
    let fields = match fields {
        Value::Object(m) => m.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    ObservedEvent {
        event_type: t.to_owned(),
        fields,
    }
}

/// The observation the sample's assertions expect after its script.
fn matching() -> Observed {
    let mut o = Observed::default();
    o.characteristics.insert(
        "o1".to_owned(),
        ObservedCharacteristics {
            power: Some(3),
            toughness: Some(3),
            types: vec!["Creature".to_owned()],
            colors: vec!["W".to_owned()],
            abilities: vec!["FLYING".to_owned()],
            controller: Some("p1".to_owned()),
            mana_value: Some(2),
            ..ObservedCharacteristics::default()
        },
    );
    let zone = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    o.zones
        .insert("p1/library".to_owned(), zone(&["p1.library.0", "o2"]));
    o.zones
        .insert("p1/graveyard".to_owned(), zone(&["p1.graveyard.0", "o4"]));
    o.zones
        .insert("battlefield".to_owned(), zone(&["o1", "o5"]));
    o.life.insert("p2".to_owned(), 20);
    o.counters
        .insert("p2".to_owned(), BTreeMap::from([("poison".to_owned(), 2)]));
    o.counters
        .insert("o1".to_owned(), BTreeMap::from([("+1/+1".to_owned(), 1)]));
    o.designations.insert(
        "p2".to_owned(),
        BTreeMap::from([("test.flag".to_owned(), json!(true))]),
    );
    o.designations.insert(
        "o5".to_owned(),
        BTreeMap::from([("test.mark".to_owned(), json!(3))]),
    );
    o.events = vec![
        event(
            "PRIORITY_PASSED",
            json!({"player": "p1", "result": "PRIORITY_MOVED", "to": "p2"}),
        ),
        event(
            "PRIORITY_PASSED",
            json!({"player": "p2", "result": "STEP_ENDS"}),
        ),
        event("STEP_ENDS", json!({"phase": "PRECOMBAT_MAIN"})),
    ];
    o
}

fn scenario() -> Scenario {
    parse_scenario(sample().as_bytes()).unwrap()
}

fn fake() -> Fake {
    Fake {
        observed: matching(),
        choices: vec![priority("p1"), priority("p2")],
        ..Fake::default()
    }
}

#[test]
fn req_test_001_runner_pass() {
    let s = scenario();
    let mut h = fake();
    let report = run_scenario(&s, &mut h);
    assert!(report.passed, "{}", report.to_text());
    assert_eq!(report.title, "test sample");
    assert_eq!(report.outcomes.len(), s.assertions.len());
    assert!(report.outcomes.iter().all(|o| o.passed));
    for (i, o) in report.outcomes.iter().enumerate() {
        assert_eq!(o.index, i);
        assert_eq!(o.path, format!("/assertions/{i}"));
        assert_eq!(o.kind, s.assertions[i].kind());
    }
    // Both answers went to the pending chooser, as written.
    assert_eq!(h.answers.len(), 2);
    assert_eq!(h.answers[1].0, "p2");
    assert_eq!(h.answers[0].1, json!({"type": "PASS_PRIORITY"}));
    assert!(report.problems().is_empty());
}

#[test]
fn req_test_001_every_assertion_kind_fails_on_a_wrong_observation() {
    // A deliberately wrong observation for each assertion: each one fails, every one is
    // evaluated, in file order.
    let s = scenario();
    let mut o = matching();
    o.characteristics.get_mut("o1").unwrap().power = Some(2);
    o.zones.insert(
        "p1/library".to_owned(),
        vec!["o2".to_owned(), "p1.library.0".to_owned()],
    );
    o.zones.insert(
        "p1/graveyard".to_owned(),
        vec![
            "p1.graveyard.0".to_owned(),
            "o4".to_owned(),
            "o3".to_owned(),
        ],
    );
    o.zones
        .insert("battlefield".to_owned(), vec!["o1".to_owned()]);
    o.life.insert("p2".to_owned(), 19);
    o.counters.clear();
    o.designations.clear();
    o.designations.insert(
        "p1".to_owned(),
        BTreeMap::from([("test.flag".to_owned(), json!("x"))]),
    );
    o.events = vec![
        event("STEP_ENDS", json!({})),
        event("PLAYER_LOST", json!({"player": "p2"})),
    ];
    o.pending = Some(priority("p1"));
    o.result.status = "WON".to_owned();
    let outcomes = evaluate_assertions(&s, &o);
    assert_eq!(outcomes.len(), 16);
    let failed: Vec<usize> = outcomes
        .iter()
        .filter(|x| !x.passed)
        .map(|x| x.index)
        .collect();
    assert_eq!(failed, (0..16).collect::<Vec<_>>());
    // The text names index, kind, path, expected and actual.
    let life = &outcomes[4];
    assert_eq!((life.expected.as_str(), life.actual.as_str()), ("20", "19"));
    let report = ScenarioReport {
        passed: false,
        title: "t".to_owned(),
        outcomes: outcomes.clone(),
        script_error: None,
    };
    let text = report.to_text();
    assert!(text.starts_with("FAIL t\n"));
    assert!(
        text.contains(
            "/assertions/4: assertion 4 (life) at /assertions/4 failed: expected 20, actual 19"
        ),
        "{text}"
    );
    assert_eq!(text.lines().count(), 17);
    // Evaluation is pure: the same inputs give the same outcomes.
    assert_eq!(evaluate_assertions(&s, &o), outcomes);
}

#[test]
fn req_test_001_events_subsequence_versus_exact() {
    let pattern = |t: &str, f: Value| EventPattern {
        event_type: t.to_owned(),
        fields: match f {
            Value::Object(m) => m.into_iter().collect(),
            _ => BTreeMap::new(),
        },
    };
    let events = |mode: EventsMode, list: Vec<EventPattern>| Scenario {
        assertions: vec![Assertion::Events {
            note: None,
            mode,
            events: list,
        }],
        ..scenario()
    };
    let o = matching();
    let check = |s: &Scenario| evaluate_assertions(s, &o)[0].passed;
    let a = pattern("PRIORITY_PASSED", json!({"player": "p1"}));
    let b = pattern("STEP_ENDS", json!({}));
    // Subsequence: in order, gaps allowed, extra fields ignored.
    assert!(check(&events(
        EventsMode::Subsequence,
        vec![a.clone(), b.clone()]
    )));
    assert!(check(&events(EventsMode::Subsequence, vec![])));
    // Out of order fails.
    assert!(!check(&events(
        EventsMode::Subsequence,
        vec![b.clone(), a.clone()]
    )));
    // A listed field must match.
    let wrong = pattern("PRIORITY_PASSED", json!({"player": "p3"}));
    assert!(!check(&events(EventsMode::Subsequence, vec![wrong])));
    let missing_field = pattern("STEP_ENDS", json!({"step": "MAIN"}));
    assert!(!check(&events(
        EventsMode::Subsequence,
        vec![missing_field]
    )));
    // Exact: the whole stream, extra fields still ignored.
    let all = vec![a.clone(), pattern("PRIORITY_PASSED", json!({})), b.clone()];
    assert!(check(&events(EventsMode::Exact, all)));
    assert!(!check(&events(
        EventsMode::Exact,
        vec![a.clone(), b.clone()]
    )));
    assert!(!check(&events(EventsMode::Exact, vec![])));
    // The same pattern twice needs two events.
    let p1 = pattern("PRIORITY_PASSED", json!({}));
    assert!(check(&events(
        EventsMode::Subsequence,
        vec![p1.clone(), p1.clone()]
    )));
    assert!(!check(&events(
        EventsMode::Subsequence,
        vec![p1.clone(), p1.clone(), p1]
    )));
}

#[test]
fn req_test_001_absent_event_and_zone_member() {
    let mut o = matching();
    let s = scenario();
    let absent: Vec<_> = evaluate_assertions(&s, &o)
        .into_iter()
        .filter(|x| x.kind == "absent")
        .collect();
    assert!(absent.iter().all(|x| x.passed));
    o.events.push(event("PLAYER_LOST", json!({"player": "p2"})));
    o.zones
        .get_mut("p1/graveyard")
        .unwrap()
        .push("o3".to_owned());
    let outcomes = evaluate_assertions(&s, &o);
    assert!(!outcomes[12].passed);
    assert!(outcomes[12].actual.contains("PLAYER_LOST"));
    assert!(!outcomes[13].passed);
    assert!(outcomes[13].expected.contains("p1/graveyard"));
}

#[test]
fn req_test_001_pending_choice_forms() {
    let base = scenario();
    let pc = |chooser: Option<&str>,
              choice_type: Option<&str>,
              options: Option<Vec<&str>>,
              mode: Option<OptionsMode>| Scenario {
        assertions: vec![Assertion::PendingChoice {
            note: None,
            chooser: chooser.map(str::to_owned),
            choice_type: choice_type.map(str::to_owned),
            options: options.map(|v| v.into_iter().map(str::to_owned).collect()),
            options_mode: mode,
        }],
        ..base.clone()
    };
    let mut o = matching();
    let check = |s: &Scenario, o: &Observed| evaluate_assertions(s, o)[0].passed;
    // No choice pending.
    assert!(check(&pc(None, None, None, None), &o));
    assert!(!check(
        &pc(Some("p1"), Some("PRIORITY_ACTION"), None, None),
        &o
    ));
    o.pending = Some(ObservedChoice {
        options: vec!["PASS_PRIORITY".to_owned(), "PLAY_LAND".to_owned()],
        ..priority("p1")
    });
    assert!(!check(&pc(None, None, None, None), &o));
    assert!(check(
        &pc(Some("p1"), Some("PRIORITY_ACTION"), None, None),
        &o
    ));
    assert!(!check(&pc(Some("p2"), None, None, None), &o));
    assert!(!check(
        &pc(Some("p1"), Some("CHOOSE_TARGETS"), None, None),
        &o
    ));
    // Options: exact (default, any order) and contains.
    let both = Some(vec!["PLAY_LAND", "PASS_PRIORITY"]);
    assert!(check(&pc(Some("p1"), None, both.clone(), None), &o));
    assert!(!check(
        &pc(Some("p1"), None, Some(vec!["PASS_PRIORITY"]), None),
        &o
    ));
    assert!(check(
        &pc(
            Some("p1"),
            None,
            Some(vec!["PASS_PRIORITY"]),
            Some(OptionsMode::Contains)
        ),
        &o
    ));
    assert!(!check(
        &pc(
            Some("p1"),
            None,
            Some(vec!["ACTIVATE"]),
            Some(OptionsMode::Contains)
        ),
        &o
    ));
    assert!(check(
        &pc(Some("p1"), None, both, Some(OptionsMode::Exact)),
        &o
    ));
}

#[test]
fn req_test_001_result_and_designation_forms() {
    let base = scenario();
    let result =
        |status: ResultStatus, winners: Option<Vec<&str>>, losers: Option<Vec<&str>>| Scenario {
            assertions: vec![Assertion::Result {
                note: None,
                status,
                winners: winners.map(|v| v.into_iter().map(str::to_owned).collect()),
                losers: losers.map(|v| v.into_iter().map(str::to_owned).collect()),
            }],
            ..base.clone()
        };
    let mut o = matching();
    o.result = ObservedResult {
        status: "WON".to_owned(),
        winners: vec!["p1".to_owned()],
        losers: vec!["p2".to_owned()],
    };
    let check = |s: &Scenario| evaluate_assertions(s, &o)[0].passed;
    assert!(check(&result(ResultStatus::Won, None, None)));
    assert!(check(&result(
        ResultStatus::Won,
        Some(vec!["p1"]),
        Some(vec!["p2"])
    )));
    assert!(!check(&result(ResultStatus::Won, Some(vec!["p2"]), None)));
    assert!(!check(&result(ResultStatus::Drawn, None, None)));
    // Designations: false and null both mean absent; values compare exactly.
    let designation = |target: &str, equals: Option<DesignationSpec>| Scenario {
        assertions: vec![Assertion::Designation {
            note: None,
            target: target.to_owned(),
            key: "test.mark".to_owned(),
            equals,
        }],
        ..base.clone()
    };
    assert!(check(&designation("o5", Some(DesignationSpec::Int(3)))));
    assert!(!check(&designation("o5", Some(DesignationSpec::Int(4)))));
    assert!(!check(&designation(
        "o5",
        Some(DesignationSpec::Text("3".to_owned()))
    )));
    assert!(check(&designation(
        "o1",
        Some(DesignationSpec::Bool(false))
    )));
    assert!(check(&designation("o1", None)));
    assert!(!check(&designation("o5", None)));
}

#[test]
fn req_test_001_characteristics_forms() {
    let base = scenario();
    let ch = |object: &str, expect: CharacteristicsExpect| Scenario {
        assertions: vec![Assertion::Characteristics {
            note: None,
            object: object.to_owned(),
            expect,
        }],
        ..base.clone()
    };
    let o = matching();
    let check = |s: &Scenario| evaluate_assertions(s, &o)[0].passed;
    // An empty expectation holds for an observed object; an unobserved object fails.
    assert!(check(&ch("o1", CharacteristicsExpect::default())));
    assert!(!check(&ch("o9", CharacteristicsExpect::default())));
    // Color names normalise; arrays compare as sets.
    let colors = |v: &[&str]| CharacteristicsExpect {
        colors: Some(v.iter().map(|s| (*s).to_owned()).collect()),
        ..CharacteristicsExpect::default()
    };
    assert!(check(&ch("o1", colors(&["WHITE"]))));
    assert!(check(&ch("o1", colors(&["white", "W"]))));
    assert!(!check(&ch("o1", colors(&["U"]))));
    for e in [
        CharacteristicsExpect {
            toughness: Some(4),
            ..CharacteristicsExpect::default()
        },
        CharacteristicsExpect {
            mana_value: Some(1),
            ..CharacteristicsExpect::default()
        },
        CharacteristicsExpect {
            controller: Some("p2".to_owned()),
            ..CharacteristicsExpect::default()
        },
        CharacteristicsExpect {
            abilities: Some(vec![]),
            ..CharacteristicsExpect::default()
        },
        CharacteristicsExpect {
            subtypes: Some(vec!["test".to_owned()]),
            ..CharacteristicsExpect::default()
        },
    ] {
        assert!(!check(&ch("o1", e)));
    }
}

#[test]
fn req_test_001_zone_forms() {
    let base = scenario();
    let zone = |owner: Option<&str>, z: ZoneName, contents: &[&str], order: OrderMode| Scenario {
        assertions: vec![Assertion::Zone {
            note: None,
            owner: owner.map(str::to_owned),
            zone: z,
            contents: contents.iter().map(|s| (*s).to_owned()).collect(),
            order,
        }],
        ..base.clone()
    };
    let o = matching();
    let check = |s: &Scenario| evaluate_assertions(s, &o)[0].passed;
    assert!(check(&zone(
        None,
        ZoneName::Battlefield,
        &["o1", "o5"],
        OrderMode::Exact
    )));
    assert!(!check(&zone(
        None,
        ZoneName::Battlefield,
        &["o5", "o1"],
        OrderMode::Exact
    )));
    assert!(check(&zone(
        None,
        ZoneName::Battlefield,
        &["o5", "o1"],
        OrderMode::Any
    )));
    assert!(!check(&zone(
        None,
        ZoneName::Battlefield,
        &["o5"],
        OrderMode::Any
    )));
    // A zone the host does not report is empty.
    assert!(check(&zone(
        Some("p2"),
        ZoneName::Hand,
        &[],
        OrderMode::Exact
    )));
    assert!(!check(&zone(
        Some("p2"),
        ZoneName::Hand,
        &["o3"],
        OrderMode::Exact
    )));
    // Life of an unreported player fails.
    let life = Scenario {
        assertions: vec![Assertion::Life {
            note: None,
            player: "p1".to_owned(),
            equals: 20,
        }],
        ..base.clone()
    };
    assert!(!evaluate_assertions(&life, &o)[0].passed);
}

#[test]
fn req_test_001_script_errors_stop_the_run() {
    let s = scenario();
    // The host refuses the first answer.
    let mut h = Fake {
        refuse: true,
        ..fake()
    };
    let report = run_scenario(&s, &mut h);
    assert!(!report.passed);
    let f = report.script_error.clone().unwrap();
    assert_eq!(f.step, 0);
    assert!(f.message.contains("test.refused"));
    // The assertions are still evaluated against the state the run stopped in.
    assert_eq!(report.outcomes.len(), 16);
    assert_eq!(
        report.problems()[0],
        (
            "/script/0".to_owned(),
            format!("script step 0: {}", f.message)
        )
    );
    // No choice pending for the second step.
    let mut h = Fake {
        choices: vec![priority("p1")],
        ..fake()
    };
    let report = run_scenario(&s, &mut h);
    let f = report.script_error.unwrap();
    assert_eq!(f.step, 1);
    assert!(f.message.contains("no choice is pending"));
    assert_eq!(h.answers.len(), 1);
    // The step expects another chooser.
    let mut h = Fake {
        choices: vec![priority("p2"), priority("p2")],
        ..fake()
    };
    let report = run_scenario(&s, &mut h);
    let f = report.script_error.unwrap();
    assert_eq!(f.step, 0);
    assert!(f.message.contains("expected a choice of \"p1\""));
    assert!(h.answers.is_empty());
    // The step expects another choice type.
    let mut h = Fake {
        choices: vec![
            ObservedChoice {
                choice_type: "CHOOSE_TARGETS".to_owned(),
                ..priority("p1")
            },
            priority("p2"),
        ],
        ..fake()
    };
    let f = run_scenario(&s, &mut h).script_error.unwrap();
    assert!(f.message.contains("expected a PRIORITY_ACTION choice"));
    // The build fails: no assertion is evaluated.
    let mut h = Fake {
        fail_build: true,
        ..fake()
    };
    let report = run_scenario(&s, &mut h);
    assert!(!report.passed && report.outcomes.is_empty());
    assert_eq!(report.problems()[0].0, "");
    assert!(report.to_text().contains(BUILD_FAILURE_PREFIX));
}

#[test]
fn req_test_001_r3_report_text_is_deterministic() {
    let s = scenario();
    let texts: Vec<String> = (0..3)
        .map(|_| {
            let mut h = Fake {
                refuse: true,
                ..fake()
            };
            let r = run_scenario(&s, &mut h);
            serde_json::to_string(&r).unwrap() + &r.to_text()
        })
        .collect();
    assert_eq!(texts[0], texts[1]);
    assert_eq!(texts[1], texts[2]);
    // The report round-trips through serde.
    let r = run_scenario(&s, &mut fake());
    let back: ScenarioReport = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(back, r);
}

#[test]
fn req_test_001_model_helpers() {
    assert_eq!(PhaseName::from_name("COMBAT"), Some(PhaseName::Combat));
    assert_eq!(StepName::from_name("main"), None);
    assert!(StepName::Main.fits(PhaseName::PostcombatMain));
    assert!(!StepName::Cleanup.fits(PhaseName::Combat));
    assert!(StepName::CombatDamage.fits(PhaseName::Combat));
    assert!(ZoneName::Hand.is_per_player() && !ZoneName::Stack.is_per_player());
    assert_eq!(ZoneName::Hand.key(Some("p1")), "p1/hand");
    assert_eq!(ZoneName::SharedExile.key(None), "sharedExile");
    assert_eq!(OrderMode::default(), OrderMode::Exact);
    assert_eq!(EventsMode::default(), EventsMode::Subsequence);
    assert_eq!(OptionsMode::default(), OptionsMode::Exact);
    assert!(ResultStatus::names().contains("DRAWN"));
    assert_eq!(AbsentWhat::ZoneMember.as_str(), "zoneMember");
    assert_eq!(CardRef::Card("test.x".to_owned()).id(), None);
    assert_eq!(DesignationSpec::Bool(false).to_observed(), None);
    assert_eq!(ANSWER_TYPES.len(), 29);
}

// ---- Gate 06-turn findings F38, F78, F79 (REQ-TEST-001, R12) ----

/// REQ-TEST-001 (F78): the report header is one line whatever the title holds; `title` is the
/// original.
#[test]
fn req_test_001_report_title_is_one_line() {
    let s = super::tests::edit(
        "\"title\": \"test sample\"",
        "\"title\": \"a\\nb\\r\\tc\\u0001\"",
    );
    let s = parse_scenario(s.as_bytes()).unwrap();
    let report = run_scenario(&s, &mut fake());
    assert_eq!(report.title, "a\nb\r\tc\u{1}");
    let text = report.to_text();
    assert_eq!(text.lines().next().unwrap(), "PASS a\\nb\\r\\tc\\u{1}");
    assert_eq!(text.lines().count(), 1, "{text:?}");
}

/// REQ-TEST-001 (F79): chooser names in a script mismatch are escaped (one line), while the
/// comparison uses the originals.
#[test]
fn req_test_001_chooser_names_are_escaped_in_script_diagnostics() {
    let mut s = scenario();
    s.script[0].expect.as_mut().unwrap().chooser = Some("p1\nx".to_owned());
    let mut h = fake();
    let report = run_scenario(&s, &mut h);
    let f = report.script_error.as_ref().unwrap();
    assert!(!f.message.contains('\n'), "{:?}", f.message);
    assert!(f.message.contains("expected a choice of"), "{}", f.message);
    assert!(h.answers.is_empty());
    assert_eq!(
        report.to_text().lines().count(),
        1 + report.problems().len()
    );
    // The pending chooser is escaped too.
    let mut h = Fake {
        choices: vec![priority("p2\nz"), priority("p2")],
        ..fake()
    };
    let s = scenario();
    let report = run_scenario(&s, &mut h);
    let f = report.script_error.unwrap();
    assert!(!f.message.contains('\n'), "{:?}", f.message);
}

/// REQ-TEST-001, R12 (F38): previews are bounded (256 characters, `...` when cut) and the
/// report keeps at most 256 KiB of them; pass/fail is exact.
#[test]
fn req_test_001_assertion_previews_are_bounded() {
    let ids: Vec<String> = (0..2_000).map(|i| format!("o{i}")).collect();
    let mut o = Observed::default();
    o.zones.insert("p1/library".to_owned(), ids.clone());
    let zone = |contents: Vec<String>| Assertion::Zone {
        note: None,
        owner: Some("p1".to_owned()),
        zone: ZoneName::Library,
        contents,
        order: OrderMode::Exact,
    };
    let mut wrong = ids.clone();
    wrong.reverse();
    let mut s = scenario();
    s.assertions = (0..1_000)
        .map(|i| {
            zone(if i % 2 == 0 {
                ids.clone()
            } else {
                wrong.clone()
            })
        })
        .collect();
    let outcomes = evaluate_assertions(&s, &o);
    assert_eq!(outcomes.len(), 1_000);
    let mut kept = 0usize;
    let mut omitted = 0usize;
    for (i, out) in outcomes.iter().enumerate() {
        assert_eq!(out.passed, i % 2 == 0, "{i}");
        for text in [&out.expected, &out.actual] {
            if text == "(omitted: report budget exhausted)" {
                omitted += 1;
            } else {
                assert!(text.chars().count() <= 256 + 3, "{}", text.len());
                assert!(text.ends_with("..."), "{text}");
                kept += text.len();
            }
        }
    }
    assert!(kept <= 256 * 1024, "{kept}");
    assert!(omitted > 0, "the aggregate budget was reached");
    assert!(outcomes[0].expected.starts_with("[\"o0\",\"o1\""));
    assert!(!outcomes[1].expected.contains("omitted"));
    // A short value is untouched.
    let mut o = Observed::default();
    o.life.insert("p2".to_owned(), 20);
    let mut s = scenario();
    s.assertions = vec![Assertion::Life {
        note: None,
        player: "p2".to_owned(),
        equals: 20,
    }];
    let out = evaluate_assertions(&s, &o);
    assert_eq!(
        (out[0].expected.as_str(), out[0].actual.as_str()),
        ("20", "20")
    );
    // Multi-byte text is cut on a character boundary.
    let mut o = Observed::default();
    o.zones
        .insert("p1/library".to_owned(), vec!["\u{20ac}".repeat(400)]);
    let mut s = scenario();
    s.assertions = vec![zone(vec!["x".to_owned()])];
    let out = evaluate_assertions(&s, &o);
    assert!(!out[0].passed);
    assert!(out[0].actual.ends_with("..."));
    assert_eq!(out[0].actual.chars().count(), 256 + 3);
}
