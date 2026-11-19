// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the SRS 15.3 enumeration threshold: fewer than 10,000 valid answers gives a
//! list of pre-validated options, 10,000 or more a validator contract (REQ-NET-003). Huge
//! inputs must not panic or overflow (R12). Black-box: written from the work-item brief only.

#[path = "spec_choice_support.rs"]
mod support;

use fineprint_core::choice::*;
use fineprint_dsl::choice::ChoiceKind;
use support::*;

/// Asserts an enumerated request with exactly the brief's items, and that the oracle count is
/// below the limit (a check on the test's own arithmetic).
fn expect_enumerated(kind: ChoiceKind, r: ResolvedChoice, item_count: usize) {
    assert!(oracle_count(kind, &r) < LIMIT, "test arithmetic");
    let req = ask_kind(kind, r.clone());
    let got = items(&req);
    assert_eq!(got.len(), item_count, "{kind:?}");
    assert_eq!(got, oracle_items(kind, &r), "{kind:?}");
    accept(&req, &req.default_on_timeout);
}

/// Asserts a validator request, and that the oracle count reaches the limit.
fn expect_validator(kind: ChoiceKind, r: ResolvedChoice) -> PendingChoiceRequest {
    assert!(oracle_count(kind, &r) >= LIMIT, "test arithmetic");
    let req = ask_kind(kind, r);
    assert!(
        is_validator(&req),
        "{kind:?} should carry a validator contract"
    );
    accept(&req, &req.default_on_timeout);
    req
}

/// REQ-NET-003: NUMBER with 9,999 values is enumerated (9,999 items); 10,000 is a validator.
#[test]
fn req_net_003_number_threshold() {
    expect_enumerated(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, 9_998),
        9_999,
    );
    expect_enumerated(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 1, 9_999),
        9_999,
    );
    let req = expect_validator(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, 9_999),
    );
    accept(&req, &num(0));
    accept(&req, &num(9_999));
    reject(&req, &num(10_000));
    reject(&req, &num(-1));
}

/// REQ-NET-003: OBJECTS n=13 min 0 max 13 (8,192 answers) is enumerated; n=14 (16,384) is a
/// validator; n=14 min=max=7 (3,432) is enumerated.
#[test]
fn req_net_003_objects_threshold() {
    expect_enumerated(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(13)), 0, 13),
        13,
    );
    let req = expect_validator(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(14)), 0, 14),
    );
    accept(&req, &sel(&[]));
    accept(&req, &sel(&range_ids(14)));
    reject(&req, &sel(&[15]));
    expect_enumerated(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(14)), 7, 7),
        14,
    );
}

/// REQ-NET-003: MODE follows the same counts as OBJECTS.
#[test]
fn req_net_003_mode_threshold() {
    expect_enumerated(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 13)), 0, 13),
        13,
    );
    let req = expect_validator(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 14)), 0, 14),
    );
    accept(&req, &ChoiceResponse::SelectModes { modes: vec![13, 0] });
    reject(&req, &ChoiceResponse::SelectModes { modes: vec![14] });
    expect_enumerated(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 14)), 7, 7),
        14,
    );
}

/// REQ-NET-003: ORDER n=7 (5,040 orderings) is enumerated; n=8 (40,320) is a validator.
#[test]
fn req_net_003_order_threshold() {
    expect_enumerated(ChoiceKind::Order, resolved(objects(&range_ids(7)), 7, 7), 7);
    let req = expect_validator(ChoiceKind::Order, resolved(objects(&range_ids(8)), 8, 8));
    accept(&req, &ordering(&[8, 7, 6, 5, 4, 3, 2, 1], &[], &[]));
    reject(&req, &ordering(&[8, 7, 6, 5, 4, 3, 2], &[], &[]));
}

/// REQ-NET-003: string lists (COLOR, CREATURE_TYPE, PILE) and PLAYER lists of 9,999 candidates
/// are enumerated; 10,000 are a validator (exactly one chosen).
#[test]
fn req_net_003_list_threshold() {
    for kind in [
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
    ] {
        expect_enumerated(
            kind,
            resolved(strings(&labels("value", 9_999)), 1, 1),
            9_999,
        );
        let l = labels("value", 10_000);
        let req = expect_validator(kind, resolved(strings(&l), 1, 1));
        accept(&req, &text(&l[9_999]));
        reject(&req, &text("test.value_x"));
    }
    expect_enumerated(
        ChoiceKind::Player,
        resolved(players(&range_ids(9_999)), 1, 1),
        9_999,
    );
    let req = expect_validator(
        ChoiceKind::Player,
        resolved(players(&range_ids(10_000)), 1, 1),
    );
    accept(
        &req,
        &ChoiceResponse::SubmitPlayer {
            player: pid(10_000),
        },
    );
    reject(
        &req,
        &ChoiceResponse::SubmitPlayer {
            player: pid(10_001),
        },
    );
}

/// REQ-NET-003, CR 601.2d, CR 510.1c, CR 510.1d: DIVISION, 2 recipients with floors 0:
/// total 9,998 (9,999 divisions) is enumerated with 2 items; total 9,999 (10,000) is a
/// validator. 3 recipients: total 139 (9,870) enumerated; 140 (10,011) and large totals
/// validators.
#[test]
fn req_net_003_division_threshold() {
    let two = |total| division(total, vec![(to(1), 0), (tp(2), 0)]);
    expect_enumerated(ChoiceKind::Division, two(9_998), 2);
    let req = expect_validator(ChoiceKind::Division, two(9_999));
    accept(&req, &div(&[(to(1), 9_000), (tp(2), 999)]));
    reject(&req, &div(&[(to(1), 9_000), (tp(2), 1_000)]));

    let three = |total| division(total, vec![(to(1), 0), (to(2), 0), (tp(1), 0)]);
    expect_enumerated(ChoiceKind::Division, three(139), 3);
    expect_validator(ChoiceKind::Division, three(140));
    expect_validator(ChoiceKind::Division, three(1_000_000));
    // Floors reduce the space: total 9,999 with floors summing to 1 leaves 9,999 divisions.
    expect_enumerated(
        ChoiceKind::Division,
        division(9_999, vec![(to(1), 1), (tp(2), 0)]),
        2,
    );
}

/// REQ-NET-003: CARD_NAME is always a validator, whatever else is supplied.
#[test]
fn req_net_003_card_name_always_validator() {
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName {
        name: "test.name_a".to_owned(),
    });
    let req = ask_kind(ChoiceKind::CardName, r);
    assert!(is_validator(&req));
}

/// REQ-NET-003, R12: huge inputs give a validator and never panic or overflow: 200 candidates
/// with min 0 max 200 (OBJECTS, MODE), 200-card ORDER, DIVISION totals of u64::MAX, and
/// NUMBER ranges up to i64::MAX.
#[test]
fn req_net_003_huge_inputs_validator_no_overflow() {
    let req = expect_validator(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(200)), 0, 200),
    );
    assert_eq!((req.min, req.max), (0, 200));
    let req = expect_validator(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(200)), 0, u64::MAX),
    );
    assert_eq!((req.min, req.max), (0, 200));
    expect_validator(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 200)), 0, 200),
    );
    expect_validator(
        ChoiceKind::Order,
        resolved(objects(&range_ids(200)), 200, 200),
    );
    for recipients in [
        vec![(to(1), 0), (to(2), 0)],
        vec![(to(1), 0), (to(2), 0), (tp(1), 0)],
        vec![(to(1), 1), (to(2), 2), (tp(1), 3), (tp(2), 4)],
    ] {
        let req = expect_validator(ChoiceKind::Division, division(u64::MAX, recipients));
        reject(&req, &div(&[(to(1), u64::MAX), (to(2), u64::MAX)]));
    }
    let max = i64::MAX as u64;
    let req = expect_validator(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, max),
    );
    accept(&req, &num(i64::MAX));
    reject(&req, &num(i64::MIN));
    expect_validator(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 1, max),
    );
}
