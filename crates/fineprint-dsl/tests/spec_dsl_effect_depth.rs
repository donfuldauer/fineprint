// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_dsl::effect` (FP-013 wire contract), part 4: instruction trees at
//! the nesting budget (REQ-NFR-006, REQ-DSL-010, brief A2). Kept in its own test binary so a
//! stack overflow here cannot hide the results of the other spec tests. Black-box: written from
//! the work-item brief only. Synthetic data only (R5).

use fineprint_dsl::canonical::{MAX_DEPTH, canonicalize, to_canonical_vec};
use fineprint_dsl::effect::Instruction;
use serde_json::{Map, Value, json};

/// An IF chain with `levels` IF objects around a DRAW whose count is `count`.
fn if_chain(levels: usize, count: Value) -> Value {
    let mut v = json!({"op":"DRAW","player":{"ref":"CONTROLLER"},"count":count});
    for _ in 0..levels {
        let mut m = Map::new();
        m.insert("op".into(), json!("IF"));
        m.insert("cond".into(), json!({"pred":"TAPPED"}));
        m.insert("then".into(), Value::Array(vec![v]));
        v = Value::Object(m);
    }
    v
}

fn accept(doc: &Value) -> Instruction {
    let text = serde_json::to_string(doc).unwrap();
    let i: Instruction = match serde_json::from_str(&text) {
        Ok(i) => i,
        Err(e) => panic!("expected the document to decode, got error: {e}"),
    };
    assert_eq!(
        serde_json::from_slice::<Instruction>(text.as_bytes()).unwrap(),
        i
    );
    assert_eq!(
        serde_json::from_value::<Instruction>(doc.clone()).unwrap(),
        i
    );
    assert_eq!(
        to_canonical_vec(&i).unwrap(),
        canonicalize(text.as_bytes()).unwrap()
    );
    let enc = serde_json::to_string(&i).unwrap();
    assert_eq!(serde_json::from_str::<Instruction>(&enc).unwrap(), i);
    i
}

/// REQ-NFR-006, REQ-DSL-010 (requirement 8, brief A2): nesting is rejected only beyond
/// `canonical::MAX_DEPTH`, so a document exactly at the budget (127 open containers) decodes
/// through all three entry points and round-trips, on the default test-thread stack. Depth:
/// 62 IFs (object + array each) = 124, then the DRAW object (125), its `count` object (126) and
/// the `{"COUNT":{"ref":..}}` selector object (127).
#[test]
fn req_nfr_006_nesting_at_budget_accepted() {
    assert_eq!(MAX_DEPTH, 127);
    for levels in [30, 42, 50, 62] {
        accept(&if_chain(levels, json!({"LITERAL":1})));
    }
    accept(&if_chain(62, json!({"COUNT":{"ref":"SELF"}})));
}
