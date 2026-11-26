// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-LAYER-KIND` (REQ-ING-009, REQ-DSL-008): every effect of a static ability declares its
//! layer (`CHAR`) or its RuleSet kind (`RULE`); `PLAYER` effects need neither. Read from the raw
//! JSON, because the typed decoder cannot represent a `CHAR` effect without a layer.

use serde_json::Value;

use super::walk::CardFacts;
use super::{CLIP, Lint, LintId, Severity, clip, push};
use crate::card::CardDefinitionDSL;

/// The reason `effect` does not declare its layer or kind, if it does not.
fn problem(effect: &Value) -> Option<String> {
    let Value::Object(map) = effect else {
        return Some("is not an object".to_owned());
    };
    match map.get("type") {
        None => Some("has no type".to_owned()),
        Some(Value::String(t)) => match t.as_str() {
            "CHAR" if !map.contains_key("layer") => {
                Some("is a CHAR effect with no layer".to_owned())
            }
            "RULE" if !map.contains_key("kind") => Some("is a RULE effect with no kind".to_owned()),
            "CHAR" | "RULE" | "PLAYER" => None,
            other => Some(format!("has unknown type {:?}", clip(other, CLIP))),
        },
        Some(_) => Some("has a type that is not a string".to_owned()),
    }
}

/// Checks every ability whose raw `kind` is `"STATIC"`, decodable or not.
pub(super) fn check(_def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    for a in &facts.abilities {
        if a.raw.get("kind").and_then(Value::as_str) != Some("STATIC") {
            continue;
        }
        let mut fail = |message: String| {
            push(
                out,
                LintId::LAYER_KIND,
                Severity::Error,
                a.location(),
                message,
            );
        };
        match a.raw.get("effects") {
            Some(Value::Array(effects)) => {
                for (k, effect) in effects.iter().enumerate() {
                    if let Some(why) = problem(effect) {
                        fail(format!("static effect effects[{k}] {why}"));
                    }
                }
            }
            Some(_) => fail("static ability member effects is not an array".to_owned()),
            None => fail("static ability has no effects array".to_owned()),
        }
    }
}
