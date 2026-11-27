// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-TEXT-COVERAGE` (REQ-ING-009, SRS §14.3 `textSpan`): every token of `face.text` is
//! covered by the inclusive `[start, end]` span of some ability of that face. Spans are read
//! from the raw JSON of every ability, decodable or not. An uncovered run is an Error for
//! `VERIFIED` and `AUTO_PARSED`, a Warning for `PARTIAL`; `UNSUPPORTED` definitions are not
//! checked (docs/DECISIONS.md, "#16 FP-015 — Text coverage").

use super::walk::{AbilityFacts, CardFacts, RawSpan};
use super::{Lint, LintId, LintLocation, Severity, push};
use crate::card::{CardDefinitionDSL, SupportStatus};

/// Checks span range and coverage on every face.
pub(super) fn check(def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    let gap_severity = match def.support_status {
        SupportStatus::Verified | SupportStatus::AutoParsed => Severity::Error,
        SupportStatus::Partial => Severity::Warning,
        SupportStatus::Unsupported => return,
    };
    // One pass groups the abilities by face (REQ-NFR-006), in their order in `facts`; an
    // ability whose face index names no face of `def` is not checked, and a face with no
    // ability keeps an empty bucket, so its whole text is a gap.
    let mut by_face: Vec<Vec<&AbilityFacts<'_>>> = vec![Vec::new(); def.faces.len()];
    for a in &facts.abilities {
        if let Some(bucket) = by_face.get_mut(a.face) {
            bucket.push(a);
        }
    }
    for (i, (face, abilities)) in def.faces.iter().zip(&by_face).enumerate() {
        let n = u64::try_from(face.text.len()).unwrap_or(u64::MAX);
        let mut covered: Vec<(u64, u64)> = Vec::new();
        for a in abilities {
            match a.span {
                RawSpan::Absent => {}
                RawSpan::Malformed => push(
                    out,
                    LintId::TEXT_COVERAGE,
                    Severity::Error,
                    a.location(),
                    "textSpan is not [start, end] with integers 0 <= start <= end".to_owned(),
                ),
                RawSpan::Valid(start, end) => {
                    if end >= n {
                        push(
                            out,
                            LintId::TEXT_COVERAGE,
                            Severity::Error,
                            a.location(),
                            format!(
                                "textSpan [{start},{end}] ends past the last text token (the \
                                 face has {n} tokens)"
                            ),
                        );
                    }
                    if start < n {
                        covered.push((start, end.min(n - 1)));
                    }
                }
            }
        }
        for (first, last) in gaps(n, covered) {
            let mut location = LintLocation::face(i, ".text");
            location.text_span = Some((
                u32::try_from(first).unwrap_or(u32::MAX),
                u32::try_from(last).unwrap_or(u32::MAX),
            ));
            push(
                out,
                LintId::TEXT_COVERAGE,
                gap_severity,
                location,
                format!("text tokens {first}..={last} are covered by no ability's textSpan"),
            );
        }
    }
}

/// The maximal runs of `0..n` covered by none of the inclusive ranges in `covered` (each range
/// already within `0..n`).
fn gaps(n: u64, mut covered: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    covered.sort_unstable();
    let mut out = Vec::new();
    let mut next = 0u64; // first index not yet known to be covered
    for (start, end) in covered {
        if start > next {
            out.push((next, start - 1));
        }
        next = next.max(end.saturating_add(1));
    }
    if next < n {
        out.push((next, n - 1));
    }
    out
}
