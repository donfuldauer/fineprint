// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-SCHEMA` (REQ-ING-009 schema validity; REQ-DSL-003, REQ-DSL-010, REQ-DSL-012): every AST
//! position holds a valid value of its real DSL type.
//!
//! The AST positions of a [`CardDefinitionDSL`] are the real typed ASTs, so a decoded definition
//! never has an invalid position: invalid content is refused when it is decoded
//! ([`CardDefinitionDSL::from_json`] returns `Schema`). What remains is a hand-built value that
//! breaks an invariant of its type (its fields are public): such a position does not encode,
//! and every one is reported with the encoder's (clipped) message (docs/DECISIONS.md, "#232
//! FP-164 — Lint schema check after typed storage").

use super::walk::CardFacts;
use super::{CLIP_MESSAGE, Lint, LintId, LintLocation, Severity, clip, push};
use crate::canonical;
use crate::card::CardDefinitionDSL;

/// One Error per position whose typed value does not encode, with the encoder's (clipped)
/// message.
pub(super) fn check(def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    let mut fail = |location: LintLocation, message: &str| {
        push(
            out,
            LintId::SCHEMA,
            Severity::Error,
            location,
            clip(message, CLIP_MESSAGE),
        );
    };
    for (i, face) in def.faces.iter().enumerate() {
        if let Some(Err(e)) = face.power.as_ref().map(canonical::to_checked_value) {
            fail(LintLocation::face(i, ".power"), &e.to_string());
        }
        if let Some(Err(e)) = face.toughness.as_ref().map(canonical::to_checked_value) {
            fail(LintLocation::face(i, ".toughness"), &e.to_string());
        }
        if let Err(e) = canonical::to_checked_value(&face.text) {
            fail(LintLocation::face(i, ".text"), &e.to_string());
        }
    }
    for a in &facts.abilities {
        if let Some(e) = a.error {
            fail(a.location(), e);
        }
    }
    for p in &facts.partners {
        if let Some(e) = p.error {
            fail(p.location(), e);
        }
    }
}
