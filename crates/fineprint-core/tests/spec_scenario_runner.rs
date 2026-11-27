// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::scenario::run_scenario` with a mock `ScenarioHost`
//! (REQ-TEST-001 item 1): build first and once, answers in script order with the right chooser,
//! script errors stop the run and fail the scenario, every assertion is evaluated, reports are
//! deterministic.
//! Black-box: written from the work-item brief (FP-049) only.

#[path = "spec_scenario_support.rs"]
mod support;

use fineprint_core::scenario::*;
use serde_json::json;
use support::*;

fn prio(p: &str) -> ObservedChoice {
    choice(p, "PRIORITY_ACTION", &["PASS_PRIORITY"])
}

fn answers(calls: &[Call]) -> Vec<(String, String)> {
    calls
        .iter()
        .filter_map(|c| match c {
            Call::Answer { chooser, answer } => Some((
                chooser.clone(),
                answer["type"].as_str().unwrap_or("").to_string(),
            )),
            Call::Build(_) => None,
        })
        .collect()
}

/// REQ-TEST-001.1: `build` is called exactly once, first, with the parsed scenario's `Setup`;
/// answers follow in script order with the pending chooser.
#[test]
fn req_test_001_runner_build_once_then_answers_in_order() {
    let s = with_script(
        vec![
            pass_step(Some(("p1", "PRIORITY_ACTION"))),
            pass_step(None),
            pass_step(Some(("p1", "PRIORITY_ACTION"))),
        ],
        vec![json!({"kind": "life", "player": "p1", "equals": 20})],
    );
    let mut host = MockHost::new(base_observed(), vec![prio("p1"), prio("p2"), prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(report.script_error.is_none(), "{report:?}");
    assert!(report.passed, "{report:?}");
    assert_eq!(report.title, s.title);
    let calls = host.calls();
    assert_eq!(
        calls.first(),
        Some(&Call::Build(Box::new(s.setup.clone()))),
        "build first, with the setup"
    );
    assert_eq!(
        calls.iter().filter(|c| matches!(c, Call::Build(_))).count(),
        1
    );
    assert_eq!(
        answers(&calls),
        vec![
            ("p1".to_string(), "PASS_PRIORITY".to_string()),
            ("p2".to_string(), "PASS_PRIORITY".to_string()),
            ("p1".to_string(), "PASS_PRIORITY".to_string()),
        ]
    );
    assert_eq!(calls.len(), 4);
    match &calls[1] {
        Call::Answer { answer, .. } => assert_eq!(answer, &json!({"type": "PASS_PRIORITY"})),
        other => panic!("second call should be an answer: {other:?}"),
    }
    assert_eq!(report.outcomes.len(), 1);
    assert!(report.outcomes[0].passed);
}

/// REQ-TEST-001.1: an `expect` naming the wrong chooser stops the run at that step and fails
/// the scenario.
#[test]
fn req_test_001_runner_expect_wrong_chooser() {
    let s = with_script(
        vec![
            pass_step(Some(("p1", "PRIORITY_ACTION"))),
            pass_step(Some(("p1", "PRIORITY_ACTION"))),
            pass_step(None),
        ],
        vec![json!({"kind": "life", "player": "p1", "equals": 20})],
    );
    let mut host = MockHost::new(base_observed(), vec![prio("p1"), prio("p2"), prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    let err = report.script_error.as_ref().expect("script error");
    assert_eq!(err.step, 1, "{report:?}");
    assert!(!err.message.is_empty());
    // The run stopped: only step 0 was answered.
    assert_eq!(
        answers(&host.calls()),
        vec![("p1".to_string(), "PASS_PRIORITY".to_string())]
    );
}

/// REQ-TEST-001.1: an `expect` naming the wrong choice type stops the run at that step.
#[test]
fn req_test_001_runner_expect_wrong_choice_type() {
    let s = with_script(
        vec![pass_step(Some(("p1", "DECLARE_ATTACKERS")))],
        vec![json!({"kind": "life", "player": "p1", "equals": 20})],
    );
    let mut host = MockHost::new(base_observed(), vec![prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    assert_eq!(report.script_error.as_ref().expect("script error").step, 0);
    assert!(
        answers(&host.calls()).is_empty(),
        "no answer after a failed expect"
    );
}

/// REQ-TEST-001.1: a host that refuses an answer stops the run at that step.
#[test]
fn req_test_001_runner_host_refuses_answer() {
    let s = with_script(
        vec![pass_step(None), pass_step(None), pass_step(None)],
        vec![json!({"kind": "life", "player": "p1", "equals": 20})],
    );
    let mut host = MockHost::new(base_observed(), vec![prio("p1"), prio("p2"), prio("p1")]);
    host.refuse_answer = Some(1);
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    let err = report.script_error.as_ref().expect("script error");
    assert_eq!(err.step, 1);
    assert!(!err.message.is_empty());
    assert_eq!(answers(&host.calls()).len(), 2, "step 2 is never submitted");
}

/// REQ-TEST-001.1: a script step when nothing is pending is a script error at that step.
#[test]
fn req_test_001_runner_answer_without_pending_choice() {
    let s = with_script(vec![pass_step(None), pass_step(None)], vec![]);
    let mut host = MockHost::new(base_observed(), vec![prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    assert_eq!(report.script_error.as_ref().expect("script error").step, 1);
    assert_eq!(answers(&host.calls()).len(), 1);

    let s = with_script(vec![pass_step(Some(("p1", "PRIORITY_ACTION")))], vec![]);
    let mut host = MockHost::new(base_observed(), vec![]);
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    assert_eq!(report.script_error.as_ref().expect("script error").step, 0);
    assert!(answers(&host.calls()).is_empty());
}

/// REQ-TEST-001.1: a host whose `build` fails cannot give a passing report.
#[test]
fn req_test_001_runner_build_failure_fails() {
    let s = with_script(vec![], vec![]);
    let mut host = MockHost::new(base_observed(), vec![]);
    host.fail_build = true;
    let report = run_scenario(&s, &mut host);
    assert!(!report.passed, "{report:?}");
    assert!(answers(&host.calls()).is_empty());
}

/// REQ-TEST-001.1: every assertion is evaluated even after an earlier one fails; `passed` is
/// false when any outcome fails.
#[test]
fn req_test_001_runner_all_assertions_evaluated() {
    let s = with_script(
        vec![pass_step(None)],
        vec![
            json!({"kind": "life", "player": "p1", "equals": 3}),
            json!({"kind": "life", "player": "p2", "equals": 20}),
            json!({"kind": "zone", "owner": "p1", "zone": "hand", "contents": ["o1"]}),
            json!({"kind": "result", "status": "IN_PROGRESS"}),
        ],
    );
    let mut host = MockHost::new(base_observed(), vec![prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(report.script_error.is_none(), "{report:?}");
    assert!(!report.passed);
    let flags: Vec<bool> = report.outcomes.iter().map(|o| o.passed).collect();
    assert_eq!(flags, vec![false, true, false, true]);
    for (i, o) in report.outcomes.iter().enumerate() {
        assert_eq!(o.index, i);
    }
}

/// REQ-TEST-001.1: the report is identical across two runs; `passed` is true only with no
/// script error and every outcome passed.
#[test]
fn req_test_001_runner_report_deterministic() {
    let s = with_script(
        vec![pass_step(Some(("p1", "PRIORITY_ACTION")))],
        vec![
            json!({"kind": "life", "player": "p1", "equals": 20}),
            json!({"kind": "zone", "owner": "p1", "zone": "library", "contents": ["o4", "o5"]}),
            json!({"kind": "pendingChoice", "chooser": "p2", "choiceType": "PRIORITY_ACTION"}),
        ],
    );
    let run = || {
        let mut host = MockHost::new(base_observed(), vec![prio("p1"), prio("p2")]);
        run_scenario(&s, &mut host)
    };
    let a = run();
    let b = run();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert!(a.passed, "{a:?}");
    assert!(a.script_error.is_none());
    assert!(a.outcomes.iter().all(|o| o.passed));
    assert_eq!(
        a.passed,
        a.script_error.is_none() && a.outcomes.iter().all(|o| o.passed)
    );
}

/// REQ-TEST-001.1: a scenario with zero script steps and zero assertions passes; `build` is
/// still called once.
#[test]
fn req_test_001_runner_empty_scenario_passes() {
    let s = with_script(vec![], vec![]);
    let mut host = MockHost::new(base_observed(), vec![prio("p1")]);
    let report = run_scenario(&s, &mut host);
    assert!(report.passed, "{report:?}");
    assert!(report.script_error.is_none());
    assert!(report.outcomes.is_empty());
    assert_eq!(host.calls(), vec![Call::Build(Box::new(s.setup.clone()))]);
}

/// REQ-TEST-001.1: every passing sample passes through the mock host fed its hand-built
/// observation (the host serves p1's priority choice, then p2's after a scripted pass), with the
/// same outcomes as `evaluate_assertions`; every wrong twin fails.
#[test]
fn req_test_001_runner_samples_through_mock_host() {
    let queue = || vec![prio("p1"), prio("p2")];
    for (sample, wrong, index) in WRONG_TWINS {
        let s = parse_ok(&sample_bytes(sample));
        let obs = sample_observed(sample);
        let mut host = MockHost::new(obs.clone(), queue());
        let report = run_scenario(&s, &mut host);
        assert!(report.passed, "{sample}: {report:?}");
        assert_eq!(
            format!("{:?}", report.outcomes),
            format!("{:?}", evaluate_assertions(&s, &obs)),
            "{sample}"
        );
        let w = parse_ok(&negative_bytes(wrong));
        let mut host = MockHost::new(obs, queue());
        let report = run_scenario(&w, &mut host);
        assert!(!report.passed, "{wrong}: {report:?}");
        assert!(report.script_error.is_none(), "{wrong}: {report:?}");
        let failed: Vec<usize> = report
            .outcomes
            .iter()
            .filter(|o| !o.passed)
            .map(|o| o.index)
            .collect();
        assert_eq!(failed, vec![index], "{wrong}");
    }
    let s = parse_ok(&sample_bytes("minimal"));
    let mut host = MockHost::new(sample_observed("minimal"), queue());
    assert!(run_scenario(&s, &mut host).passed);
}
