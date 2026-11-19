// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the `fineprint_core::choice` catalogue: the SRS 15.3 ChoiceType taxonomy and
//! its wire names, and the pinned `ChoiceResponse` variants (REQ-NET-003). Black-box: written
//! from the work-item brief only.

#[path = "spec_choice_support.rs"]
mod support;

use fineprint_core::choice::*;
use support::*;

/// The SRS 15.3 table's ChoiceType wire names, in table order.
const NAMES: [&str; 20] = [
    "PRIORITY_ACTION",
    "CHOOSE_MODES",
    "CHOOSE_COSTS_AND_X",
    "CHOOSE_TARGETS",
    "DIVIDE_VALUE",
    "PAY_MANA_AND_COSTS",
    "DECLARE_ATTACKERS",
    "DECLARE_BLOCKERS",
    "ORDER_SIMULTANEOUS_TRIGGERS",
    "CHOOSE_OPTIONAL_TRIGGER",
    "CHOOSE_REPLACEMENT_EFFECT",
    "SELECT_OBJECTS_FROM_ZONE",
    "ORDER_CARDS",
    "CHOOSE_PLAYER",
    "CHOOSE_OPTION_OR_VALUE",
    "CHOOSE_CARD_NAME",
    "CHOOSE_UNTAP_SET",
    "SHORTCUT_RESPONSE",
    "MULLIGAN_DECISION",
    "PREGAME_ACTION",
];

/// REQ-NET-003: `ChoiceType::ALL` lists the 20 SRS 15.3 types in table order with these exact
/// wire names, and the variants match the names one to one.
#[test]
fn req_net_003_all_twenty_types_in_order() {
    assert_eq!(ChoiceType::ALL.len(), 20);
    let names: Vec<&str> = ChoiceType::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(names, NAMES);
    let variants = [
        ChoiceType::PriorityAction,
        ChoiceType::ChooseModes,
        ChoiceType::ChooseCostsAndX,
        ChoiceType::ChooseTargets,
        ChoiceType::DivideValue,
        ChoiceType::PayManaAndCosts,
        ChoiceType::DeclareAttackers,
        ChoiceType::DeclareBlockers,
        ChoiceType::OrderSimultaneousTriggers,
        ChoiceType::ChooseOptionalTrigger,
        ChoiceType::ChooseReplacementEffect,
        ChoiceType::SelectObjectsFromZone,
        ChoiceType::OrderCards,
        ChoiceType::ChoosePlayer,
        ChoiceType::ChooseOptionOrValue,
        ChoiceType::ChooseCardName,
        ChoiceType::ChooseUntapSet,
        ChoiceType::ShortcutResponse,
        ChoiceType::MulliganDecision,
        ChoiceType::PregameAction,
    ];
    assert_eq!(ChoiceType::ALL, variants);
}

/// REQ-NET-003: `from_wire` round-trips every name.
#[test]
fn req_net_003_from_wire_round_trips() {
    for t in ChoiceType::ALL {
        assert_eq!(ChoiceType::from_wire(t.as_str()), Some(t), "{}", t.as_str());
    }
    for (i, name) in NAMES.iter().enumerate() {
        assert_eq!(ChoiceType::from_wire(name), Some(ChoiceType::ALL[i]));
    }
}

/// REQ-NET-003: `from_wire` returns None for unknown, lowercase, padded and empty strings.
#[test]
fn req_net_003_from_wire_rejects_unknown() {
    let mut bad: Vec<String> = vec![
        String::new(),
        " ".to_owned(),
        "UNKNOWN".to_owned(),
        "PriorityAction".to_owned(),
        "CHOOSE_MODE".to_owned(),
        "CHOOSE-MODES".to_owned(),
        "test.choice_type".to_owned(),
    ];
    for name in NAMES {
        bad.push(name.to_lowercase());
        bad.push(format!(" {name}"));
        bad.push(format!("{name} "));
        bad.push(format!("{name}_"));
    }
    for s in &bad {
        assert_eq!(ChoiceType::from_wire(s), None, "{s:?}");
    }
}

/// REQ-NET-003: the enumeration limit is the SRS 15.3 threshold of 10,000.
#[test]
fn req_net_003_enumeration_limit() {
    assert_eq!(ENUMERATION_LIMIT, 10_000);
    assert_eq!(ENUMERATION_LIMIT, LIMIT);
}

/// REQ-NET-003: every `ChoiceResponse` variant with pinned fields can be constructed with those
/// fields (compile-time check of the pinned API), and the variants are pairwise distinct.
#[test]
fn req_net_003_pinned_response_variants_construct() {
    let samples = pinned_samples();
    // 21 pinned variants; SubmitChoiceValue appears twice (Number and Text).
    assert_eq!(samples.len(), 22);
    let mut distinct = 0;
    for (i, a) in samples.iter().enumerate() {
        if samples[..i].iter().all(|b| !same_variant(a, b)) {
            distinct += 1;
        }
        assert_eq!(a.clone(), *a);
    }
    assert_eq!(distinct, 21);
    // The pinned field names, destructured.
    let ChoiceResponse::SubmitOrdering {
        top,
        bottom,
        graveyard,
    } = ordering(&[1], &[], &[])
    else {
        unreachable!()
    };
    assert_eq!((top.len(), bottom.len(), graveyard.len()), (1, 0, 0));
    let ChoiceResponse::Shorten { at_index } = (ChoiceResponse::Shorten { at_index: 7 }) else {
        unreachable!()
    };
    assert_eq!(at_index, 7);
}

/// REQ-NET-003: the pinned value types (RequestId, ChoiceTarget, ChoiceValue, ChoiceOption,
/// ChoiceReason) construct, compare and order as pinned.
#[test]
fn req_net_003_pinned_value_types() {
    let a = RequestId::new(3);
    let b = RequestId::new(4);
    assert_eq!(a.get(), 3);
    assert!(a < b);
    let copy = a;
    assert_eq!(copy, a);
    let t1 = tp(2);
    let t2 = to(2);
    assert_ne!(t1, t2);
    let copied = t1;
    assert_eq!(copied, tp(2));
    assert!(to(1) < to(2));
    assert!(tp(1) < tp(2));
    assert_ne!(ChoiceValue::Number(1), ChoiceValue::Text("1".to_owned()));
    let options = [
        ChoiceOption::Object(oid(1)),
        ChoiceOption::Player(pid(1)),
        ChoiceOption::Text("test.value_a".to_owned()),
        ChoiceOption::Number(-1),
        ChoiceOption::Mode(0),
        ChoiceOption::Yes,
        ChoiceOption::No,
    ];
    for (i, x) in options.iter().enumerate() {
        for (j, y) in options.iter().enumerate() {
            assert_eq!(i == j, x == y);
        }
    }
    let r = reason();
    assert_eq!(r.cr, "test.reason_a");
    assert_eq!(r.source, Some(oid(900)));
}

/// REQ-NET-003: `ChoiceError` implements Display (non-empty) and std::error::Error for every
/// pinned variant.
#[test]
fn req_net_003_choice_error_display() {
    let errors = [
        ChoiceError::Invalid {
            request_id: RequestId::new(1),
            explanation: "test.explanation".to_owned(),
        },
        ChoiceError::StaleRequest {
            submitted: RequestId::new(0),
            pending: Some(RequestId::new(1)),
        },
        ChoiceError::StaleRequest {
            submitted: RequestId::new(0),
            pending: None,
        },
        ChoiceError::NotPending,
        ChoiceError::Divergence { index: 2 },
        ChoiceError::Procedure("test.failure".to_owned()),
        ChoiceError::InvalidSpec("test.spec".to_owned()),
        ChoiceError::MissingCandidates,
        ChoiceError::Unsatisfiable("test.bounds".to_owned()),
        ChoiceError::NoDefault,
        ChoiceError::InvalidDefault("test.default".to_owned()),
    ];
    for e in &errors {
        let dyn_err: &dyn std::error::Error = e;
        assert!(!dyn_err.to_string().is_empty(), "{e:?}");
    }
    let _: Interrupt = ChoiceError::NotPending.into();
}
