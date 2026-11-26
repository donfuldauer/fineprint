// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Fixture self-checks for the FP-015 lint spec suites (REQ-ING-009): every fixture the lint
//! suites use as a VALID building block decodes through the card wrappers' `to_real()`, and
//! every fixture meant to be undecodable does not. A fixture mistake therefore fails here, not
//! as a false lint finding. Does not use `fineprint_dsl::lint`. Synthetic data only (R5).

mod spec_dsl_lint_common;

use serde_json::json;
use spec_dsl_lint_common::*;

/// REQ-ING-009: the shared clean definition decodes at every position.
#[test]
fn req_ing_009_clean_def_decodes_everywhere() {
    let d = clean_def();
    assert_eq!(undecodable_positions(&d), Vec::<(String, String)>::new());
}

/// REQ-ING-009: the valid ability fixtures used by the pass cases decode.
#[test]
fn req_ing_009_valid_ability_fixtures_decode() {
    let mut valid = vec![
        good_activated_mana("a"),
        good_triggered_mana("a", "ACTIVATED"),
        good_triggered_mana("a", "RESOLVED"),
        with(
            &good_triggered_mana("a", "RESOLVED"),
            "effect",
            json!([search_library()]),
        ),
        with(&good_activated_mana("a"), "isManaAbility", json!(false)),
        without(&good_activated_mana("a"), "isManaAbility"),
        with(
            &with(&good_activated_mana("a"), "isManaAbility", json!(false)),
            "targets",
            one_target(),
        ),
        with(&keyword("a"), "isManaAbility", json!(true)),
        activated(
            "a",
            tap_cost(),
            json!([draw(), mill(), search_library(), shuffle()]),
        ),
        replacement("a", "ENTERS", json!([instead()])),
        replacement("a", "ENTERS", json!([skip_op(), modify_event()])),
        replacement("a", "DIES", json!([skip_op()])),
        replacement("a", "ZONE_CHANGE", json!([prevent_op()])),
        static_ab("a", json!([char_effect(Some("7b"))])),
        static_ab("a", json!([rule_effect(Some("EVASION"))])),
        static_ab("a", json!([player_effect()])),
        spell(
            "a",
            json!([native_with(json!({"lookup": true, "back": {"x": 1}}))]),
        ),
        spell("a", json!([repeat_while(Some(json!(1)), json!([draw()]))])),
        spell(
            "a",
            json!([repeat_while(Some(json!(4_294_967_295u64)), json!([draw()]))]),
        ),
        spell(
            "a",
            json!([if_then(json!([for_each(json!([may(json!([
                repeat_while(Some(json!(3)), json!([draw()]))
            ]))]))]))]),
        ),
        spell(
            "a",
            json!([create_token("test.token_a"), create_token_inline()]),
        ),
        linked(&spell("a", json!([tap_linked("l1")])), &["l1"]),
        linked(&keyword("a"), &["l1", "l2"]),
        activated(
            "a",
            json!([{"cost":"PAY_MANA","mana":"{3}{R}"}]),
            json!([native()]),
        ),
        activated(
            "a",
            json!([{"cost":"COMPOSITE","costs":[{"cost":"TAP_SOURCE"},{"cost":"PAY_MANA","mana":"{G}"}]}]),
            json!([native()]),
        ),
        with(&keyword("a"), "abilityWord", json!(MARKER)),
        triggered("a", step_trigger(), json!([native()])),
        triggered("a", event_trigger("ENTERS"), json!([native()])),
    ];
    for t in ["CONTROL_ETB", "COPY_ETB", "BACK_FACE_ETB"] {
        valid.push(with(
            &replacement("a", "ENTERS", json!([modify_event()])),
            "tierHint",
            json!(t),
        ));
        for b in [true, false] {
            valid.push(with(
                &with(
                    &replacement("a", "ENTERS", json!([modify_event()])),
                    "tierHint",
                    json!(t),
                ),
                "isSelfReplacement",
                json!(b),
            ));
        }
    }
    for v in &valid {
        assert_decodes(v);
    }
}

/// REQ-ING-009: fixtures meant to be undecodable are.
#[test]
fn req_ing_009_invalid_ability_fixtures_do_not_decode() {
    let invalid = [
        json!({"kind": "FLYING"}),
        spell("a", json!([repeat_while(None, json!([draw()]))])),
        spell("a", json!([repeat_while(Some(json!(0)), json!([draw()]))])),
        triggered(
            "a",
            json!({"mode":"EVENT","event":{"kind":"DIES","filters":{"lookBack":true}}}),
            json!([]),
        ),
        with(
            &activated(
                "a",
                json!([{"cost":"PAY_MANA","mana":"{R}"}]),
                json!([native()]),
            ),
            "zzzUnknown",
            json!(1),
        ),
    ];
    for v in &invalid {
        assert_undecodable(v);
    }
}
