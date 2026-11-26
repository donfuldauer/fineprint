// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! #232 FP-164 (REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-NFR-006): the card AST positions hold
//! the real typed ASTs, so the former tuple wrappers are gone. These helpers build the typed
//! values from their wire JSON through plain serde, panicking on invalid content: every fixture
//! that uses them is valid, neutral and SYNTHETIC (R5).

#![allow(dead_code)]

use fineprint_dsl::card::{AbilityAST, PartnerAbilityDSL, TextTokenStream, ValueExpressionAST};
use serde_json::Value;

/// A value expression from its wire JSON (REQ-DSL-003).
pub fn vx(v: Value) -> ValueExpressionAST {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("value expression {v}: {e}"))
}

/// An ability from its wire JSON (REQ-DSL-005, REQ-DSL-010).
pub fn ability_of(v: Value) -> AbilityAST {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("ability {v}: {e}"))
}

/// A text token stream from its token objects (REQ-DSL-012).
pub fn tx(tokens: Vec<Value>) -> TextTokenStream {
    let v = Value::Array(tokens);
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("text {v}: {e}"))
}

/// A partner ability from its wire JSON (a KEYWORD ability).
pub fn pa(v: Value) -> PartnerAbilityDSL {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("partner ability {v}: {e}"))
}

/// The wire JSON of any typed value.
pub fn wire_of<T: serde::Serialize>(t: &T) -> Value {
    serde_json::to_value(t).expect("typed AST serialises")
}

/// The token objects of a text stream.
pub fn tokens_of(t: &TextTokenStream) -> Vec<Value> {
    match wire_of(t) {
        Value::Array(a) => a,
        other => panic!("text stream serialised as {other}"),
    }
}

/// An inert, valid ability identified by `name` (#232 FP-164): a KEYWORD ability with
/// `abilityId` `name` and the synthetic keyword `TEST_SYNTHETIC`. It replaces the former
/// free-form placeholder `{"synthetic": name}`, which is not a valid ability; two calls are equal
/// exactly when the names are.
pub fn synthetic_ability(name: &str) -> AbilityAST {
    ability_of(serde_json::json!({
        "abilityId": name,
        "kind": "KEYWORD",
        "keyword": "TEST_SYNTHETIC",
    }))
}

/// A KEYWORD ability with `abilityId` `id` and keyword `keyword` (`[A-Z][A-Z0-9_]*`).
pub fn keyword_ability(id: &str, keyword: &str) -> AbilityAST {
    ability_of(serde_json::json!({"abilityId": id, "kind": "KEYWORD", "keyword": keyword}))
}
