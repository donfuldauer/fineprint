// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `PendingChoiceRequest::validate`: valid answers are accepted, invalid ones
//! rejected with a non-empty explanation, hostile inputs never panic (REQ-NET-003, R12).
//! Black-box: written from the work-item brief only. Every name is invented (R5).

#[path = "spec_choice_support.rs"]
mod support;

use fineprint_core::choice::*;
use fineprint_dsl::choice::ChoiceKind;
use support::*;

/// A valid request and a valid answer for each spec-derived kind; the answers name ids 1..=3,
/// player 1 and `test.color_00000`, like `pinned_samples`, so only the variant can be wrong.
fn request_and_answer(kind: ChoiceKind) -> (PendingChoiceRequest, ChoiceResponse) {
    let colors = labels("color", 3);
    let (r, a) = match kind {
        ChoiceKind::Objects => (resolved(objects(&[1, 2, 3]), 1, 2), sel(&[1])),
        ChoiceKind::Mode => (
            resolved(strings(&labels("mode", 3)), 1, 2),
            ChoiceResponse::SelectModes { modes: vec![0] },
        ),
        ChoiceKind::Number => (resolved(ResolvedCandidates::None, 0, 5), num(1)),
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            (resolved(strings(&colors), 1, 1), text("test.color_00000"))
        }
        ChoiceKind::CardName => {
            let mut r = resolved(ResolvedCandidates::None, 1, 1);
            r.default = Some(ChoiceResponse::SubmitCardName {
                name: "test.name_b".to_owned(),
            });
            (
                r,
                ChoiceResponse::SubmitCardName {
                    name: "test.name_a".to_owned(),
                },
            )
        }
        ChoiceKind::Player => (
            resolved(players(&[1, 2]), 1, 1),
            ChoiceResponse::SubmitPlayer { player: pid(1) },
        ),
        ChoiceKind::YesNo => (
            resolved(ResolvedCandidates::None, 1, 1),
            ChoiceResponse::Yes,
        ),
        ChoiceKind::Order => (
            resolved(objects(&[1, 2, 3]), 3, 3),
            ordering(&[1, 2, 3], &[], &[]),
        ),
        ChoiceKind::Division => (division(1, vec![(to(1), 0)]), div(&[(to(1), 1)])),
    };
    (ask_kind(kind, r), a)
}

/// The variants that may pass for each kind.
fn right_variants(kind: ChoiceKind) -> Vec<ChoiceResponse> {
    match kind {
        ChoiceKind::YesNo => vec![ChoiceResponse::Yes, ChoiceResponse::No],
        other => vec![request_and_answer(other).1],
    }
}

/// REQ-NET-003: every pinned `ChoiceResponse` variant against every spec-derived request type:
/// only the right variant(s) can pass; every other variant is rejected with an explanation.
#[test]
fn req_net_003_wrong_variant_rejected_everywhere() {
    for kind in ChoiceKind::ALL {
        let (req, valid) = request_and_answer(*kind);
        accept(&req, &valid);
        let right = right_variants(*kind);
        for sample in pinned_samples() {
            if right.iter().any(|r| same_variant(r, &sample)) {
                continue;
            }
            reject(&req, &sample);
        }
    }
}

/// REQ-NET-003: SELECT_OBJECTS_FROM_ZONE accepts distinct candidates within [min, max] (any
/// order) and rejects too few, too many, duplicates and non-candidates.
#[test]
fn req_net_003_select_objects_rules() {
    let req = ask_kind(ChoiceKind::Objects, resolved(objects(&[1, 2, 3, 4]), 1, 3));
    for ok in [&[1][..], &[4, 2], &[3, 1, 2]] {
        accept(&req, &sel(ok));
    }
    reject(&req, &sel(&[]));
    reject(&req, &sel(&[1, 2, 3, 4]));
    reject(&req, &sel(&[2, 2]));
    reject(&req, &sel(&[5]));
    reject(&req, &sel(&[1, 0]));
}

/// REQ-NET-003: CHOOSE_MODES accepts distinct indices < n within [min, max] and rejects
/// out-of-range, duplicate and out-of-bounds counts.
#[test]
fn req_net_003_choose_modes_rules() {
    let req = ask_kind(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 4)), 2, 3),
    );
    let modes = |m: &[u32]| ChoiceResponse::SelectModes { modes: m.to_vec() };
    accept(&req, &modes(&[0, 3]));
    accept(&req, &modes(&[2, 1, 0]));
    reject(&req, &modes(&[0, 4]));
    reject(&req, &modes(&[0, u32::MAX]));
    reject(&req, &modes(&[1, 1]));
    reject(&req, &modes(&[1]));
    reject(&req, &modes(&[0, 1, 2, 3]));
}

/// REQ-NET-003: NUMBER accepts min..=max and rejects below, above, extremes and `Text`.
#[test]
fn req_net_003_number_rules() {
    let req = ask_kind(ChoiceKind::Number, resolved(ResolvedCandidates::None, 2, 6));
    for v in 2..=6 {
        accept(&req, &num(v));
    }
    for v in [1, 7, 0, -1, i64::MIN, i64::MAX] {
        reject(&req, &num(v));
    }
    reject(&req, &text("3"));
}

/// REQ-NET-003: COLOR-like requests accept a listed string and reject an unlisted one or a
/// `Number`.
#[test]
fn req_net_003_string_value_rules() {
    for kind in [
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
    ] {
        let l = labels("value", 3);
        let req = ask_kind(kind, resolved(strings(&l), 1, 1));
        for s in &l {
            accept(&req, &text(s));
        }
        reject(&req, &text("test.value_9"));
        reject(&req, &text(""));
        reject(&req, &text(&l[0].to_uppercase()));
        reject(&req, &num(0));
    }
}

/// REQ-NET-003: CHOOSE_PLAYER accepts a candidate and rejects any other player.
#[test]
fn req_net_003_choose_player_rules() {
    let req = ask_kind(ChoiceKind::Player, resolved(players(&[2, 4]), 1, 1));
    let p = |n| ChoiceResponse::SubmitPlayer { player: pid(n) };
    accept(&req, &p(2));
    accept(&req, &p(4));
    for n in [0, 1, 3, 5, u64::MAX] {
        reject(&req, &p(n));
    }
}

/// REQ-NET-003: CHOOSE_OPTIONAL_TRIGGER accepts both Yes and No.
#[test]
fn req_net_003_yes_no_rules() {
    let req = ask_kind(ChoiceKind::YesNo, resolved(ResolvedCandidates::None, 1, 1));
    accept(&req, &ChoiceResponse::Yes);
    accept(&req, &ChoiceResponse::No);
}

/// REQ-NET-003: ORDER_CARDS accepts any permutation of all candidates on top and rejects a
/// missing card, an extra card, a duplicate, and a non-empty bottom or graveyard.
#[test]
fn req_net_003_order_cards_rules() {
    let req = ask_kind(ChoiceKind::Order, resolved(objects(&[3, 5, 7]), 3, 3));
    accept(&req, &ordering(&[3, 5, 7], &[], &[]));
    accept(&req, &ordering(&[7, 3, 5], &[], &[]));
    reject(&req, &ordering(&[3, 5], &[], &[]));
    reject(&req, &ordering(&[3, 5, 7, 9], &[], &[]));
    reject(&req, &ordering(&[3, 5, 5], &[], &[]));
    reject(&req, &ordering(&[3, 5, 7, 7], &[], &[]));
    reject(&req, &ordering(&[3, 5], &[7], &[]));
    reject(&req, &ordering(&[3, 5], &[], &[7]));
    reject(&req, &ordering(&[3, 5, 7], &[7], &[]));
    reject(&req, &ordering(&[3, 5, 7], &[], &[3]));
    reject(&req, &ordering(&[], &[], &[]));
}

/// REQ-NET-003, CR 601.2d, CR 510.1c, CR 510.1d: DIVIDE_VALUE accepts one assignment per
/// recipient summing to the total with each at least its floor (in any order); rejects a wrong
/// sum, a floor violation, a missing, duplicate or unknown recipient, and amounts whose sum
/// overflows u64.
#[test]
fn req_net_003_divide_value_rules() {
    let req = ask_kind(
        ChoiceKind::Division,
        division(5, vec![(to(1), 1), (tp(2), 0), (to(3), 1)]),
    );
    accept(&req, &div(&[(to(1), 1), (tp(2), 3), (to(3), 1)]));
    accept(&req, &div(&[(to(1), 4), (tp(2), 0), (to(3), 1)]));
    accept(&req, &div(&[(to(3), 2), (to(1), 1), (tp(2), 2)]));
    // Sum != total.
    reject(&req, &div(&[(to(1), 1), (tp(2), 2), (to(3), 1)]));
    reject(&req, &div(&[(to(1), 2), (tp(2), 2), (to(3), 2)]));
    // Below a floor (sum still 5).
    reject(&req, &div(&[(to(1), 0), (tp(2), 4), (to(3), 1)]));
    // Missing recipient (sum still 5, floors of the rest met).
    reject(&req, &div(&[(to(1), 4), (to(3), 1)]));
    // Duplicate recipient.
    reject(
        &req,
        &div(&[(to(1), 1), (tp(2), 1), (to(3), 1), (to(1), 2)]),
    );
    reject(&req, &div(&[(to(1), 1), (to(1), 3), (to(3), 1)]));
    // Unknown recipient (an object id that is a player id elsewhere, and a new id).
    reject(&req, &div(&[(to(1), 1), (to(2), 3), (to(3), 1)]));
    reject(
        &req,
        &div(&[(to(1), 1), (tp(2), 2), (to(3), 1), (to(9), 1)]),
    );
    // Overflow: the amounts wrap to the total in u64 arithmetic.
    reject(&req, &div(&[(to(1), u64::MAX), (tp(2), 5), (to(3), 1)]));
    reject(
        &req,
        &div(&[(to(1), u64::MAX), (tp(2), u64::MAX), (to(3), 7)]),
    );
    reject(&req, &div(&[]));
}

/// REQ-NET-003: CHOOSE_CARD_NAME accepts invented names and rejects empty, whitespace-only,
/// over-256-byte and control-character names.
#[test]
fn req_net_003_card_name_rules() {
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName {
        name: "test.name_a".to_owned(),
    });
    let req = ask_kind(ChoiceKind::CardName, r);
    let name = |s: &str| ChoiceResponse::SubmitCardName { name: s.to_owned() };
    for ok in [
        "test.name_a",
        "test.name_b",
        "test name with spaces",
        "x",
        &"a".repeat(256),
    ] {
        accept(&req, &name(ok));
    }
    for bad in [
        "",
        " ",
        "   \t ",
        "\n",
        &"a".repeat(257),
        &"a".repeat(100_000),
        "test.name\u{7}",
        "test\nname",
        "test\u{0}name",
        "test\u{7f}name",
        "test\u{9b}name",
    ] {
        reject(&req, &name(bad));
    }
}

/// REQ-NET-003, R12: hostile inputs (huge vectors, extreme integers, empty vectors) are
/// rejected or accepted per the rules, never with a panic.
#[test]
fn req_net_003_hostile_inputs_never_panic() {
    let huge_ids: Vec<u64> = (0..100_000).collect();
    let max_ids = [u64::MAX; 1_000];
    let mut requests = Vec::new();
    for kind in ChoiceKind::ALL {
        requests.push(request_and_answer(*kind).0);
    }
    requests.push(ask_kind(
        ChoiceKind::Objects,
        resolved(objects(&range_ids(200)), 0, 200),
    ));
    requests.push(ask_kind(
        ChoiceKind::Division,
        division(u64::MAX, vec![(to(1), 0), (to(2), 0), (to(3), 0)]),
    ));
    let hostile = vec![
        sel(&huge_ids),
        sel(&max_ids),
        sel(&[]),
        ChoiceResponse::SelectModes {
            modes: (0..100_000).collect(),
        },
        ChoiceResponse::SelectModes {
            modes: vec![u32::MAX; 1_000],
        },
        ChoiceResponse::SelectModes { modes: vec![] },
        num(i64::MIN),
        num(i64::MAX),
        text(&"z".repeat(1_000_000)),
        ordering(&huge_ids, &huge_ids, &huge_ids),
        ordering(&[], &[], &[]),
        ordering(&max_ids, &[], &[]),
        ChoiceResponse::SubmitDivision {
            assignments: vec![(to(u64::MAX), u64::MAX); 10_000],
        },
        ChoiceResponse::SubmitDivision {
            assignments: vec![(to(1), u64::MAX), (to(2), u64::MAX), (to(3), u64::MAX)],
        },
        ChoiceResponse::SubmitDivision {
            assignments: Vec::new(),
        },
        ChoiceResponse::SubmitPlayer {
            player: pid(u64::MAX),
        },
        ChoiceResponse::SubmitCardName {
            name: "\u{0}".repeat(100_000),
        },
        ChoiceResponse::Shorten { at_index: u64::MAX },
        ChoiceResponse::PlayLand {
            object: oid(u64::MAX),
            face_index: u32::MAX,
        },
        ChoiceResponse::KeepHand {
            bottom: oids(&huge_ids),
        },
        ChoiceResponse::SubmitSet {
            objects: oids(&huge_ids),
        },
    ];
    for req in &requests {
        for h in &hostile {
            // Whatever the verdict, a rejection carries an explanation.
            if let Err(e) = req.validate(h) {
                assert!(!e.trim().is_empty());
            }
        }
    }
    // The huge selections are rejected by the 200-candidate request.
    reject(&requests[ChoiceKind::ALL.len()], &sel(&huge_ids));
    reject(&requests[ChoiceKind::ALL.len()], &sel(&max_ids));
    accept(&requests[ChoiceKind::ALL.len()], &sel(&[]));
    // The wrapped-sum division is rejected by the u64::MAX-total request.
    reject(
        &requests[ChoiceKind::ALL.len() + 1],
        &div(&[(to(1), u64::MAX), (to(2), 1), (to(3), u64::MAX)]),
    );
    accept(
        &requests[ChoiceKind::ALL.len() + 1],
        &div(&[(to(1), u64::MAX - 2), (to(2), 1), (to(3), 1)]),
    );
}

// ---------------------------------------------------------------------------------------------
// PAY_MANA_AND_COSTS: payment-method eligibility (REQ-NET-003, R12).
// ---------------------------------------------------------------------------------------------

/// A PAY_MANA_AND_COSTS request with methods `("test.convoke", [o1, o2])` and
/// `("test.delve", [o3])`, optional and cancellable, auto-pay offered.
fn pay_request() -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser: pid(1),
        choice_type: ChoiceType::PayManaAndCosts,
        reason: reason(),
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::CancelAndRollback,
        options: ChoiceOptions::Enumerated(Vec::new()),
        payload: ChoicePayload::PayManaAndCosts {
            locked_total: "test.total_a".to_owned(),
            methods: vec![
                ("test.convoke".to_owned(), oids(&[11, 12])),
                ("test.delve".to_owned(), oids(&[13])),
            ],
            mana_abilities: Vec::new(),
            auto_pay: true,
            optional: true,
            cancellable: true,
        },
    }
}

/// `PayCosts` without auto-pay and without mana abilities.
fn pay(methods: Vec<(u32, Vec<u64>)>) -> ChoiceResponse {
    ChoiceResponse::PayCosts {
        auto_pay: false,
        methods: methods.into_iter().map(|(i, v)| (i, oids(&v))).collect(),
        mana_abilities: Vec::new(),
    }
}

/// REQ-NET-003, R12: a method may be paid with any subset of its own eligible objects.
#[test]
fn req_net_003_pay_costs_accepts_eligible_objects() {
    let req = pay_request();
    accept(&req, &pay(vec![(0, vec![11])]));
    accept(&req, &pay(vec![(0, vec![11, 12])]));
    accept(&req, &pay(vec![(1, vec![13])]));
    accept(&req, &pay(vec![(0, vec![])]));
    accept(&req, &pay(vec![(0, vec![12]), (1, vec![13])]));
}

/// REQ-NET-003, R12: objects eligible only for another method, duplicates, unlisted objects and
/// out-of-range method indices are rejected with non-empty explanations that do not leak the
/// other methods' eligible lists.
#[test]
fn req_net_003_pay_costs_rejects_ineligible_objects() {
    let req = pay_request();
    let e = reject(&req, &pay(vec![(0, vec![13])]));
    // The answer's own item may be named; another method's eligible list may not.
    assert!(!e.contains("11") && !e.contains("12"), "leaks: {e}");
    let e = reject(&req, &pay(vec![(0, vec![11, 11])]));
    assert!(!e.contains("12") && !e.contains("13"), "leaks: {e}");
    let e = reject(&req, &pay(vec![(0, vec![999])]));
    assert!(!e.contains("11") && !e.contains("12") && !e.contains("13"));
    let e = reject(&req, &pay(vec![(2, vec![])]));
    assert!(!e.contains("11") && !e.contains("12") && !e.contains("13"));
    reject(&req, &pay(vec![(1, vec![11])]));
}

/// REQ-NET-003, R12: a payment method may be listed at most once per PAY_COSTS response; the
/// objects for a method go in one entry. Repeats are rejected even when every entry is
/// individually valid. (The request's objects are 11, 12 for method 0 and 13 for method 1.)
#[test]
fn req_net_003_pay_costs_rejects_repeated_method_index() {
    let req = pay_request();
    reject(&req, &pay(vec![(0, vec![11]), (0, vec![11])]));
    reject(&req, &pay(vec![(0, vec![11]), (0, vec![12])]));
    reject(&req, &pay(vec![(1, vec![13]), (0, vec![11]), (1, vec![])]));
    accept(&req, &pay(vec![(0, vec![11, 12])]));
    accept(&req, &pay(vec![(0, vec![11]), (1, vec![13])]));
    reject(&req, &pay(vec![(0, vec![11, 11])]));
}

/// REQ-NET-003: auto-pay, declining and cancelling still validate against the payment request.
#[test]
fn req_net_003_pay_costs_auto_decline_cancel() {
    let req = pay_request();
    accept(
        &req,
        &ChoiceResponse::PayCosts {
            auto_pay: true,
            methods: Vec::new(),
            mana_abilities: Vec::new(),
        },
    );
    accept(&req, &ChoiceResponse::DeclineOptionalPayment);
    accept(&req, &ChoiceResponse::CancelAndRollback);
}
