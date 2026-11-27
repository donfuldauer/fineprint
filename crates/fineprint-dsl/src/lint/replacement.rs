// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-TIER-HINT` (REQ-ING-009, REQ-DSL-009): a replacement's `tierHint` must be consistent
//! with CR 616.1b–d. All three tiers are about how an object enters the battlefield, so a hint
//! needs an `ENTERS` matcher and at least one operation that modifies the entry. An absent hint
//! is never a finding (the effect then sits in tier 616.1e), and `isSelfReplacement` (616.1a) is
//! independent of the hint (docs/DECISIONS.md, "#16 FP-015 — Tier-hint check baseline").

use super::walk::CardFacts;
use super::{Lint, LintId, Severity, push};
use crate::ability::{AbilityAST, EventKind, ReplacementOp};
use crate::card::CardDefinitionDSL;

/// Checks every typed REPLACEMENT ability with a `tierHint`.
pub(super) fn check(_def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    for a in &facts.abilities {
        let AbilityAST::Replacement(r) = a.typed else {
            continue;
        };
        let Some(hint) = r.tier_hint else { continue };
        // CR 616.1b, CR 616.1c, CR 616.1d: each tier modifies how an object enters the
        // battlefield.
        if r.matcher.event != EventKind::Enters {
            push(
                out,
                LintId::TIER_HINT,
                Severity::Error,
                a.location(),
                format!(
                    "tierHint {hint} needs a replacement of the ENTERS event, found {} \
                     (CR 616.1b-d)",
                    r.matcher.event
                ),
            );
        }
        if r.replace_with
            .iter()
            .all(|op| matches!(op, ReplacementOp::Skip | ReplacementOp::Prevent { .. }))
        {
            push(
                out,
                LintId::TIER_HINT,
                Severity::Error,
                a.location(),
                format!(
                    "tierHint {hint} needs an operation that modifies the entry, but every \
                     replaceWith operation is SKIP or PREVENT (CR 616.1b-d)"
                ),
            );
        }
    }
}
