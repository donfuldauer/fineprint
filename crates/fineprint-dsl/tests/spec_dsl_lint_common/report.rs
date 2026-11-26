// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Report helpers for the FP-015 lint spec suites (REQ-ING-009): an independent comparator for
//! the S-0 total order, the location path grammar, and finding lookups. Written from the brief
//! only; it never looks at the implementation.

#![allow(dead_code)]

use std::cmp::Ordering;

use fineprint_dsl::card::CardDefinitionDSL;
use fineprint_dsl::lint::{Lint, LintContext, LintId, LintReport, Severity, lint_definition};

/// Default context: no source characteristics, no token catalogue.
pub fn ctx0() -> LintContext {
    LintContext::default()
}

/// Lints `d` with `ctx`, checking the S-0 invariants on every call: sorted, de-duplicated,
/// paths well formed, and a second call gives an equal report (purity).
pub fn run(d: &CardDefinitionDSL, ctx: &LintContext) -> LintReport {
    let r = lint_definition(d, ctx);
    assert_invariants(d, &r);
    let again = lint_definition(d, ctx);
    assert_eq!(r, again, "lint_definition is not pure");
    r
}

/// Lints with the default context.
pub fn run0(d: &CardDefinitionDSL) -> LintReport {
    run(d, &ctx0())
}

/// The S-0 key comparison, written independently from the brief: face_index (None first),
/// ability_index (None first), id string, severity (Error first), text_span (None first), path,
/// message.
pub fn cmp_key(a: &Lint, b: &Lint) -> Ordering {
    fn opt<T: Ord>(x: &Option<T>, y: &Option<T>) -> Ordering {
        match (x, y) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(p), Some(q)) => p.cmp(q),
        }
    }
    let sev = |s: Severity| -> u8 {
        match s {
            Severity::Error => 0,
            Severity::Warning => 1,
        }
    };
    opt(&a.location.face_index, &b.location.face_index)
        .then_with(|| opt(&a.location.ability_index, &b.location.ability_index))
        .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        .then_with(|| sev(a.severity).cmp(&sev(b.severity)))
        .then_with(|| opt(&a.location.text_span, &b.location.text_span))
        .then_with(|| a.location.path.cmp(&b.location.path))
        .then_with(|| a.message.cmp(&b.message))
}

/// `lints` sorted by [`cmp_key`] and de-duplicated.
pub fn sorted_dedup(lints: &[Lint]) -> Vec<Lint> {
    let mut v = lints.to_vec();
    v.sort_by(cmp_key);
    v.dedup();
    v
}

/// Parses `[n]` at the start of `s`, returning the index and the rest.
fn bracket(s: &str) -> Option<(usize, &str)> {
    let s = s.strip_prefix('[')?;
    let end = s.find(']')?;
    let digits = &s[..end];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    Some((digits.parse().ok()?, &s[end + 1..]))
}

/// The location implied by a path in the brief's grammar: `(face_index, ability_index)`, or
/// `None` if the path is not in the grammar. Index bounds are checked against `d`.
pub fn parse_path(d: &CardDefinitionDSL, path: &str) -> Option<(Option<usize>, Option<usize>)> {
    if path == "$" {
        return Some((None, None));
    }
    if let Some(rest) = path.strip_prefix("relatedTokens") {
        let (k, tail) = bracket(rest)?;
        let n = d.related_tokens.as_ref().map_or(0, Vec::len);
        return (tail.is_empty() && k < n).then_some((None, None));
    }
    if let Some(rest) = path.strip_prefix("commanderEligibility.partnerAbilities") {
        let (k, tail) = bracket(rest)?;
        let n = d
            .commander_eligibility
            .as_ref()
            .map_or(0, |c| c.partner_abilities.len());
        return (tail.is_empty() && k < n).then_some((None, None));
    }
    let rest = path.strip_prefix("faces")?;
    let (i, tail) = bracket(rest)?;
    let f = d.faces.get(i)?;
    match tail {
        "" | ".power" | ".toughness" | ".text" => Some((Some(i), None)),
        _ => {
            let rest = tail.strip_prefix(".abilities")?;
            let (j, tail) = bracket(rest)?;
            (tail.is_empty() && j < f.abilities.len()).then_some((Some(i), Some(j)))
        }
    }
}

/// S-0 invariants: sorted by the total key, no duplicates, every path in the grammar with
/// indices consistent with the path text, spans only on `faces[i].text` with start <= end.
pub fn assert_invariants(d: &CardDefinitionDSL, r: &LintReport) {
    for w in r.lints.windows(2) {
        assert_eq!(
            cmp_key(&w[0], &w[1]),
            Ordering::Less,
            "report not strictly sorted / has duplicates:\n{}\n{}",
            w[0],
            w[1]
        );
    }
    assert_eq!(r.lints, sorted_dedup(&r.lints));
    for l in &r.lints {
        let parsed = parse_path(d, &l.location.path)
            .unwrap_or_else(|| panic!("path outside the grammar: {l}"));
        assert_eq!(
            parsed,
            (l.location.face_index, l.location.ability_index),
            "indices disagree with path: {l}"
        );
        if let Some((s, e)) = l.location.text_span {
            assert!(s <= e, "span start > end: {l}");
            // The brief gives spans to coverage findings (`faces[i].text` runs; an ability
            // whose own span is out of range may carry it). No other path has a span.
            assert!(
                l.location.path.ends_with(".text") || l.location.ability_index.is_some(),
                "span on a path that is neither text nor an ability: {l}"
            );
        }
        assert!(!l.message.is_empty(), "empty message: {l}");
    }
}

/// Findings of `id`.
pub fn of(r: &LintReport, id: LintId) -> Vec<Lint> {
    r.with_id(id).cloned().collect()
}

/// Paths of findings of `id` with `sev`.
pub fn paths(r: &LintReport, id: LintId, sev: Severity) -> Vec<String> {
    r.lints
        .iter()
        .filter(|l| l.id == id && l.severity == sev)
        .map(|l| l.location.path.clone())
        .collect()
}

/// Asserts no finding of `id`.
pub fn assert_none(r: &LintReport, id: LintId) {
    let found = of(r, id);
    assert!(
        found.is_empty(),
        "unexpected {id} findings:\n{}",
        found
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Asserts at least one finding of `id` with `sev` at exactly `path`, and returns it.
pub fn assert_at(r: &LintReport, id: LintId, sev: Severity, path: &str) -> Lint {
    r.lints
        .iter()
        .find(|l| l.id == id && l.severity == sev && l.location.path == path)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected {} {id} at {path}; report:\n{}",
                sev.as_str(),
                dump(r)
            )
        })
}

/// Asserts the findings of `id` are exactly `expected` as (severity, path) in report order.
pub fn assert_exactly(r: &LintReport, id: LintId, expected: &[(Severity, String)]) {
    let got: Vec<(Severity, String)> = r
        .lints
        .iter()
        .filter(|l| l.id == id)
        .map(|l| (l.severity, l.location.path.clone()))
        .collect();
    let want: Vec<(Severity, String)> = expected.iter().map(|(s, p)| (*s, p.clone())).collect();
    assert_eq!(got, want, "{id} findings differ; report:\n{}", dump(r));
}

/// The report in Display form, one finding per line.
pub fn dump(r: &LintReport) -> String {
    r.lints
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The ability path `faces[i].abilities[j]`.
pub fn ab(i: usize, j: usize) -> String {
    format!("faces[{i}].abilities[{j}]")
}
