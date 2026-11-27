// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The scenario runner and the assertion evaluator (REQ-TEST-001).
//!
//! The runner reads nothing but the parsed scenario and the host (R12) and spends no clock, I/O
//! or environment (R3, R7). It never panics.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::model::{
    AbsentWhat, Assertion, CharacteristicsExpect, DesignationSpec, EventPattern, EventsMode,
    OptionsMode, OrderMode, Scenario, Setup, ZoneName,
};
use super::observe::{Observed, ObservedChoice, ObservedEvent};

/// The seam between the runner and a game (REQ-TEST-001). A host builds the exact starting
/// state, reports the pending choice, applies scripted answers and reports what it observes.
/// Every method must be a deterministic function of the setup and the answers (R3).
pub trait ScenarioHost {
    /// Build the exact starting state; Err(message) if the host cannot (reported as a script
    /// failure).
    ///
    /// # Errors
    /// A message naming what the host cannot build.
    fn build(&mut self, setup: &Setup) -> Result<(), String>;

    /// The pending choice, if any, as plain data.
    fn pending(&self) -> Option<ObservedChoice>;

    /// Apply one scripted answer; Err(message) if refused.
    ///
    /// # Errors
    /// A message naming why the answer was refused.
    fn answer(&mut self, chooser: &str, answer: &serde_json::Value) -> Result<(), String>;

    /// The full observable state plus every event emitted since `build`, in order.
    fn observe(&self) -> Observed;
}

/// The outcome of one assertion (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionOutcome {
    /// The assertion's 0-based index in the file.
    pub index: usize,
    /// The assertion `kind`.
    pub kind: String,
    /// The JSON pointer of the assertion (`/assertions/<index>`).
    pub path: String,
    /// Whether it held.
    pub passed: bool,
    /// What was expected, as compact JSON.
    pub expected: String,
    /// What was observed, as compact JSON.
    pub actual: String,
}

/// A script error: the run stopped at `step` (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFailure {
    /// The 0-based script step; 0 with a message starting [`BUILD_FAILURE_PREFIX`] when the
    /// host could not build the setup.
    pub step: usize,
    /// What went wrong.
    pub message: String,
}

/// The message prefix of a [`ScriptFailure`] raised by [`ScenarioHost::build`].
pub const BUILD_FAILURE_PREFIX: &str = "setup could not be built: ";

/// The result of one run (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioReport {
    /// True iff there is no script error and every assertion held.
    pub passed: bool,
    /// The scenario title.
    pub title: String,
    /// One outcome per assertion, in file order (empty when the setup could not be built).
    pub outcomes: Vec<AssertionOutcome>,
    /// The script error, if the run stopped early.
    pub script_error: Option<ScriptFailure>,
}

impl ScenarioReport {
    /// Every problem as (JSON pointer, message), in order: the script error first, then each
    /// failed assertion.
    #[must_use]
    pub fn problems(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(f) = &self.script_error {
            let pointer = if f.message.starts_with(BUILD_FAILURE_PREFIX) {
                String::new()
            } else {
                format!("/script/{}", f.step)
            };
            out.push((pointer, format!("script step {}: {}", f.step, f.message)));
        }
        for o in self.outcomes.iter().filter(|o| !o.passed) {
            out.push((
                o.path.clone(),
                format!(
                    "assertion {} ({}) at {} failed: expected {}, actual {}",
                    o.index, o.kind, o.path, o.expected, o.actual
                ),
            ));
        }
        out
    }

    /// The report as deterministic text: `PASS <title>` or `FAIL <title>`, then one line per
    /// problem (`<pointer>: <message>`).
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "{} {}\n",
            if self.passed { "PASS" } else { "FAIL" },
            self.title
        );
        for (pointer, message) in self.problems() {
            out.push_str(&format!("{pointer}: {message}\n"));
        }
        out
    }
}

/// Runs `scenario` on `host` (REQ-TEST-001): builds the setup, plays every script step
/// (checking its `expect` against the pending choice first), then evaluates every assertion
/// against [`ScenarioHost::observe`]. A script error stops the script and fails the scenario;
/// the assertions are still evaluated against the state the run stopped in, unless the setup
/// could not be built.
pub fn run_scenario(scenario: &Scenario, host: &mut dyn ScenarioHost) -> ScenarioReport {
    let mut report = ScenarioReport {
        passed: false,
        title: scenario.title.clone(),
        outcomes: Vec::new(),
        script_error: None,
    };
    if let Err(message) = host.build(&scenario.setup) {
        report.script_error = Some(ScriptFailure {
            step: 0,
            message: format!("{BUILD_FAILURE_PREFIX}{message}"),
        });
        return report;
    }
    report.script_error = play(scenario, host).err();
    let observed = host.observe();
    report.outcomes = evaluate_assertions(scenario, &observed);
    report.passed = report.script_error.is_none() && report.outcomes.iter().all(|o| o.passed);
    report
}

/// Plays the script; the first failure stops it.
fn play(scenario: &Scenario, host: &mut dyn ScenarioHost) -> Result<(), ScriptFailure> {
    for (step, s) in scenario.script.iter().enumerate() {
        let fail = |message: String| ScriptFailure { step, message };
        let Some(pending) = host.pending() else {
            return Err(fail(
                "an answer is scripted but no choice is pending".to_owned(),
            ));
        };
        if let Some(expect) = &s.expect {
            if let Some(c) = &expect.chooser
                && *c != pending.chooser
            {
                return Err(fail(format!(
                    "expected a choice of {c}, but the pending choice is {}'s",
                    pending.chooser
                )));
            }
            if let Some(t) = &expect.choice_type
                && *t != pending.choice_type
            {
                return Err(fail(format!(
                    "expected a {t} choice, but the pending choice is {}",
                    pending.choice_type
                )));
            }
        }
        host.answer(&pending.chooser, &s.answer)
            .map_err(|m| fail(format!("the host refused the answer: {m}")))?;
    }
    Ok(())
}

/// Evaluates every assertion of `scenario` against `observed`, in file order (REQ-TEST-001).
/// Pure: the same logic [`run_scenario`] uses. Every assertion is evaluated, whatever the
/// earlier ones gave.
#[must_use]
pub fn evaluate_assertions(scenario: &Scenario, observed: &Observed) -> Vec<AssertionOutcome> {
    scenario
        .assertions
        .iter()
        .enumerate()
        .map(|(index, a)| {
            let (passed, expected, actual) = check(a, observed);
            AssertionOutcome {
                index,
                kind: a.kind().to_owned(),
                path: format!("/assertions/{index}"),
                passed,
                expected: compact(&expected),
                actual: compact(&actual),
            }
        })
        .collect()
}

fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// A color name in its one-letter form (`WHITE`, `white` and `W` all give `W`); anything else
/// unchanged.
fn color_letter(c: &str) -> String {
    match c.to_ascii_uppercase().as_str() {
        "W" | "WHITE" => "W".to_owned(),
        "U" | "BLUE" => "U".to_owned(),
        "B" | "BLACK" => "B".to_owned(),
        "R" | "RED" => "R".to_owned(),
        "G" | "GREEN" => "G".to_owned(),
        _ => c.to_owned(),
    }
}

fn as_set(list: &[String]) -> Vec<String> {
    let mut v = list.to_vec();
    v.sort();
    v.dedup();
    v
}

fn set_value(list: &[String]) -> Value {
    Value::from(as_set(list))
}

fn check(a: &Assertion, o: &Observed) -> (bool, Value, Value) {
    match a {
        Assertion::Characteristics { object, expect, .. } => characteristics(object, expect, o),
        Assertion::Zone {
            owner,
            zone,
            contents,
            order,
            ..
        } => {
            let actual = zone_contents(o, *zone, owner.as_deref());
            let passed = match order {
                OrderMode::Exact => actual == *contents,
                OrderMode::Any => {
                    let mut x = actual.clone();
                    let mut y = contents.clone();
                    x.sort();
                    y.sort();
                    x == y
                }
            };
            (passed, Value::from(contents.clone()), Value::from(actual))
        }
        Assertion::Life { player, equals, .. } => match o.life.get(player) {
            Some(l) => (l == equals, json!(equals), json!(l)),
            None => (false, json!(equals), Value::Null),
        },
        Assertion::Counters {
            target,
            counter,
            equals,
            ..
        } => {
            let actual = o
                .counters
                .get(target)
                .and_then(|m| m.get(counter))
                .copied()
                .unwrap_or(0);
            (actual == *equals, json!(equals), json!(actual))
        }
        Assertion::Designation {
            target,
            key,
            equals,
            ..
        } => {
            let actual = o
                .designations
                .get(target)
                .and_then(|m| m.get(key))
                .cloned()
                .filter(|v| *v != Value::Bool(false));
            let expected = equals.as_ref().and_then(DesignationSpec::to_observed);
            (
                actual == expected,
                expected.unwrap_or(Value::Null),
                actual.unwrap_or(Value::Null),
            )
        }
        Assertion::Events { mode, events, .. } => {
            let passed = match mode {
                EventsMode::Subsequence => is_subsequence(events, &o.events),
                EventsMode::Exact => {
                    events.len() == o.events.len()
                        && events.iter().zip(&o.events).all(|(p, e)| matches(p, e))
                }
            };
            (
                passed,
                json!({"mode": mode.as_str(), "events": events.iter().map(pattern_value).collect::<Vec<_>>()}),
                Value::from(o.events.iter().map(event_value).collect::<Vec<_>>()),
            )
        }
        Assertion::Absent {
            what,
            event,
            owner,
            zone,
            object,
            ..
        } => match what {
            AbsentWhat::Event => {
                let Some(p) = event else {
                    return (false, Value::Null, Value::Null);
                };
                let found: Vec<Value> = o
                    .events
                    .iter()
                    .filter(|e| matches(p, e))
                    .map(event_value)
                    .collect();
                (
                    found.is_empty(),
                    json!({"absent": pattern_value(p)}),
                    Value::from(found),
                )
            }
            AbsentWhat::ZoneMember => {
                let (Some(zone), Some(object)) = (zone, object) else {
                    return (false, Value::Null, Value::Null);
                };
                let actual = zone_contents(o, *zone, owner.as_deref());
                (
                    !actual.contains(object),
                    json!({"notIn": zone.key(owner.as_deref()), "object": object}),
                    Value::from(actual),
                )
            }
        },
        Assertion::PendingChoice {
            chooser,
            choice_type,
            options,
            options_mode,
            ..
        } => pending_choice(
            chooser.as_deref(),
            choice_type.as_deref(),
            options.as_deref(),
            options_mode.unwrap_or_default(),
            o.pending.as_ref(),
        ),
        Assertion::Result {
            status,
            winners,
            losers,
            ..
        } => {
            let r = &o.result;
            let mut passed = r.status == status.as_str();
            let mut expected = json!({"status": status.as_str()});
            let mut actual = json!({"status": r.status});
            for (key, want, have) in [
                ("winners", winners, &r.winners),
                ("losers", losers, &r.losers),
            ] {
                if let Some(want) = want {
                    passed &= as_set(want) == as_set(have);
                    expected[key] = set_value(want);
                    actual[key] = set_value(have);
                }
            }
            (passed, expected, actual)
        }
    }
}

fn zone_contents(o: &Observed, zone: ZoneName, owner: Option<&str>) -> Vec<String> {
    o.zones.get(&zone.key(owner)).cloned().unwrap_or_default()
}

fn characteristics(object: &str, e: &CharacteristicsExpect, o: &Observed) -> (bool, Value, Value) {
    let mut expected = serde_json::Map::new();
    let mut actual = serde_json::Map::new();
    let Some(c) = o.characteristics.get(object) else {
        let expected = serde_json::to_value(e).unwrap_or(Value::Null);
        return (
            false,
            expected,
            json!({"object": object, "observed": false}),
        );
    };
    let mut passed = true;
    let mut int = |key: &str, want: Option<i64>, have: Option<i64>| {
        if let Some(w) = want {
            passed &= have == Some(w);
            expected.insert(key.to_owned(), json!(w));
            actual.insert(key.to_owned(), json!(have));
        }
    };
    int("power", e.power, c.power);
    int("toughness", e.toughness, c.toughness);
    int("manaValue", e.mana_value, c.mana_value);
    let mut set = |key: &str, want: &Option<Vec<String>>, have: &[String], colors: bool| {
        if let Some(w) = want {
            let norm = |l: &[String]| -> Vec<String> {
                if colors {
                    as_set(&l.iter().map(|c| color_letter(c)).collect::<Vec<_>>())
                } else {
                    as_set(l)
                }
            };
            let (w, h) = (norm(w), norm(have));
            passed &= w == h;
            expected.insert(key.to_owned(), Value::from(w));
            actual.insert(key.to_owned(), Value::from(h));
        }
    };
    set("types", &e.types, &c.types, false);
    set("subtypes", &e.subtypes, &c.subtypes, false);
    set("supertypes", &e.supertypes, &c.supertypes, false);
    set("colors", &e.colors, &c.colors, true);
    set("abilities", &e.abilities, &c.abilities, false);
    if let Some(w) = &e.controller {
        passed &= c.controller.as_ref() == Some(w);
        expected.insert("controller".to_owned(), json!(w));
        actual.insert("controller".to_owned(), json!(c.controller));
    }
    (passed, Value::Object(expected), Value::Object(actual))
}

/// Whether `e` has `p`'s type and every field `p` lists, with equal values.
fn matches(p: &EventPattern, e: &ObservedEvent) -> bool {
    p.event_type == e.event_type && p.fields.iter().all(|(k, v)| e.fields.get(k) == Some(v))
}

/// The patterns occur in order in `events`, not necessarily adjacent (a greedy scan is exact
/// for subsequence matching).
fn is_subsequence(patterns: &[EventPattern], events: &[ObservedEvent]) -> bool {
    let mut it = events.iter();
    patterns.iter().all(|p| it.any(|e| matches(p, e)))
}

fn pattern_value(p: &EventPattern) -> Value {
    json!({"type": p.event_type, "fields": p.fields})
}

fn event_value(e: &ObservedEvent) -> Value {
    json!({"type": e.event_type, "fields": e.fields})
}

fn pending_choice(
    chooser: Option<&str>,
    choice_type: Option<&str>,
    options: Option<&[String]>,
    mode: OptionsMode,
    pending: Option<&ObservedChoice>,
) -> (bool, Value, Value) {
    let actual = pending.map_or(
        Value::Null,
        |p| json!({"chooser": p.chooser, "choiceType": p.choice_type, "options": p.options}),
    );
    let Some(chooser) = chooser else {
        return (pending.is_none(), Value::Null, actual);
    };
    let mut expected: BTreeMap<&str, Value> = BTreeMap::new();
    expected.insert("chooser", json!(chooser));
    let Some(p) = pending else {
        if let Some(t) = choice_type {
            expected.insert("choiceType", json!(t));
        }
        return (false, json!(expected), actual);
    };
    let mut passed = p.chooser == chooser;
    if let Some(t) = choice_type {
        passed &= p.choice_type == t;
        expected.insert("choiceType", json!(t));
    }
    if let Some(opts) = options {
        let have = as_set(&p.options);
        let want = as_set(opts);
        passed &= match mode {
            OptionsMode::Exact => have == want,
            OptionsMode::Contains => want.iter().all(|w| have.contains(w)),
        };
        expected.insert("options", Value::from(want));
        expected.insert("optionsMode", json!(mode.as_str()));
    }
    (passed, json!(expected), actual)
}
