// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-SCHEMA` (REQ-ING-009 schema validity; REQ-DSL-003, REQ-DSL-010, REQ-DSL-012): every AST
//! position decodes as its real DSL type. Unlike [`CardDefinitionDSL::validate_asts`], which
//! stops at the first failure, every failing position is reported.

use super::walk::CardFacts;
use super::{CLIP_MESSAGE, Lint, LintId, LintLocation, Severity, clip, push};
use crate::card::CardDefinitionDSL;

/// One Error per position whose `to_real` fails, with the decoder's (clipped) message.
pub(super) fn check(def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    let mut fail = |location: LintLocation, message: String| {
        push(
            out,
            LintId::SCHEMA,
            Severity::Error,
            location,
            clip(&message, CLIP_MESSAGE),
        );
    };
    for (i, face) in def.faces.iter().enumerate() {
        if let Some(Err(e)) = face.power.as_ref().map(|p| p.to_real()) {
            fail(LintLocation::face(i, ".power"), e);
        }
        if let Some(Err(e)) = face.toughness.as_ref().map(|t| t.to_real()) {
            fail(LintLocation::face(i, ".toughness"), e);
        }
        if let Err(e) = face.text.to_real() {
            fail(LintLocation::face(i, ".text"), e);
        }
    }
    for a in &facts.abilities {
        if let Err(e) = &a.typed {
            fail(a.location(), e.clone());
        }
    }
    for p in &facts.partners {
        if let Some(e) = &p.error {
            fail(p.location(), e.clone());
        }
    }
}
