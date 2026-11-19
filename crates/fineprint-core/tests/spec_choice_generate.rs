// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `generate_request`: requests derived automatically from resolved ChoiceSpecs
//! (REQ-DSL-011, REQ-NET-003), their defaults on timeout, and generation errors. Black-box:
//! written from the work-item brief only.

#[path = "spec_choice_support.rs"]
mod support;

use fineprint_core::choice::*;
use fineprint_dsl::choice::ChoiceKind;
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST, ValueExpressionAST};
use support::*;

/// A representative valid resolved choice for every kind. Object and player candidates are
/// given ascending here so that tests about other properties do not depend on the sorting
/// rule, which has its own tests (`*_items_*`, `req_net_003_computed_defaults`).
fn sample(kind: ChoiceKind) -> ResolvedChoice {
    match kind {
        ChoiceKind::Objects => resolved(objects(&[3, 5, 7]), 1, 2),
        ChoiceKind::Mode => resolved(strings(&labels("mode", 3)), 1, 2),
        ChoiceKind::Number => resolved(ResolvedCandidates::None, 2, 6),
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            let l = vec![
                "test.value_c".to_owned(),
                "test.value_a".to_owned(),
                "test.value_b".to_owned(),
            ];
            resolved(strings(&l), 1, 1)
        }
        ChoiceKind::CardName => {
            let mut r = resolved(ResolvedCandidates::None, 1, 1);
            r.default = Some(ChoiceResponse::SubmitCardName {
                name: "test.name_a".to_owned(),
            });
            r
        }
        ChoiceKind::Player => resolved(players(&[1, 2, 3]), 1, 1),
        ChoiceKind::YesNo => resolved(ResolvedCandidates::None, 1, 1),
        ChoiceKind::Order => resolved(objects(&[4, 6, 9]), 3, 3),
        ChoiceKind::Division => division(5, vec![(to(8), 1), (tp(2), 0), (to(3), 1)]),
    }
}

/// REQ-DSL-011, REQ-NET-003: every kind maps to its SRS 15.3 ChoiceType; chooser, request id
/// and reason are copied; the request validates its own default.
#[test]
fn req_dsl_011_kind_maps_to_choice_type() {
    for kind in ChoiceKind::ALL {
        let mut r = sample(*kind);
        r.chooser = pid(2);
        let req = expect_ask(generate_request(RequestId::new(42), &spec(*kind), r));
        assert_eq!(req.choice_type, expected_type(*kind), "{kind:?}");
        assert_eq!(req.request_id, RequestId::new(42), "{kind:?}");
        assert_eq!(req.chooser, pid(2), "{kind:?}");
        assert_eq!(req.reason, reason(), "{kind:?}");
        accept(&req, &req.default_on_timeout);
    }
}

/// REQ-DSL-011: `hidden` is true only for `spec.hidden == Some(true)`; `reveal_to` is copied.
#[test]
fn req_dsl_011_hidden_and_reveal_to() {
    for kind in ChoiceKind::ALL {
        for (hidden, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
            let mut s = spec(*kind);
            s.hidden = hidden;
            s.reveal_to = Some(PlayerSelectorAST::Players(PlayerGroup::AllPlayers));
            let mut r = sample(*kind);
            r.reveal_to = vec![pid(2), pid(3)];
            let req = expect_ask(generate_request(RequestId::new(1), &s, r));
            assert_eq!(req.hidden, expected, "{kind:?} {hidden:?}");
            assert_eq!(req.reveal_to, vec![pid(2), pid(3)], "{kind:?}");
        }
        let req = ask_kind(*kind, sample(*kind));
        assert!(req.reveal_to.is_empty(), "{kind:?}");
    }
}

/// REQ-DSL-011: `reason` (CR reference and source) is copied verbatim, including no source.
#[test]
fn req_dsl_011_reason_copied() {
    let mut r = sample(ChoiceKind::YesNo);
    r.reason = ChoiceReason {
        cr: "601.2d".to_owned(),
        source: None,
    };
    let req = ask_kind(ChoiceKind::YesNo, r);
    assert_eq!(
        req.reason,
        ChoiceReason {
            cr: "601.2d".to_owned(),
            source: None
        }
    );
}

/// REQ-DSL-011: `spec.random == Some(true)` gives `Generated::Random` for every kind;
/// `Some(false)` and `None` give a request.
#[test]
fn req_dsl_011_random_spec() {
    for kind in ChoiceKind::ALL {
        let mut s = spec(*kind);
        s.random = Some(true);
        let g = generate_request(RequestId::new(0), &s, sample(*kind));
        assert!(matches!(g, Ok(Generated::Random)), "{kind:?}: {g:?}");
        s.random = Some(false);
        expect_ask(generate_request(RequestId::new(0), &s, sample(*kind)));
    }
}

/// REQ-DSL-011: OBJECTS -> SELECT_OBJECTS_FROM_ZONE; items are the candidates ascending; min
/// and max copied, max clamped to n.
#[test]
fn req_dsl_011_objects_items_and_bounds() {
    let req = ask_kind(ChoiceKind::Objects, resolved(objects(&[7, 3, 5]), 1, 2));
    assert_eq!(
        items(&req),
        vec![
            ChoiceOption::Object(oid(3)),
            ChoiceOption::Object(oid(5)),
            ChoiceOption::Object(oid(7))
        ]
    );
    assert_eq!((req.min, req.max), (1, 2));
    let req = ask_kind(ChoiceKind::Objects, resolved(objects(&[7, 3, 5]), 0, 9));
    assert_eq!((req.min, req.max), (0, 3), "max clamped to n");
    let req = ask_kind(
        ChoiceKind::Objects,
        resolved(objects(&[7, 3, 5]), 2, u64::MAX),
    );
    assert_eq!((req.min, req.max), (2, 3), "u64::MAX clamped to n");
}

/// REQ-DSL-011: MODE -> CHOOSE_MODES; items are Mode(0..n); max clamped to n.
#[test]
fn req_dsl_011_mode_items_and_bounds() {
    let req = ask_kind(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 4)), 1, 2),
    );
    assert_eq!(
        items(&req),
        (0..4).map(ChoiceOption::Mode).collect::<Vec<_>>()
    );
    assert_eq!((req.min, req.max), (1, 2));
    let req = ask_kind(
        ChoiceKind::Mode,
        resolved(strings(&labels("mode", 4)), 2, 10),
    );
    assert_eq!((req.min, req.max), (2, 4));
}

/// REQ-DSL-011: NUMBER -> CHOOSE_OPTION_OR_VALUE; items Number(min..=max) ascending; min and
/// max copied.
#[test]
fn req_dsl_011_number_items() {
    let req = ask_kind(ChoiceKind::Number, resolved(ResolvedCandidates::None, 2, 6));
    assert_eq!(req.choice_type, ChoiceType::ChooseOptionOrValue);
    assert_eq!(
        items(&req),
        (2..=6).map(ChoiceOption::Number).collect::<Vec<_>>()
    );
    assert_eq!((req.min, req.max), (2, 6));
    let req = ask_kind(ChoiceKind::Number, resolved(ResolvedCandidates::None, 0, 0));
    assert_eq!(items(&req), vec![ChoiceOption::Number(0)]);
}

/// REQ-DSL-011: COLOR, CREATURE_TYPE and PILE -> CHOOSE_OPTION_OR_VALUE; items are Text in the
/// given (unsorted) order.
#[test]
fn req_dsl_011_string_kinds_keep_order() {
    for kind in [
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
    ] {
        let req = ask_kind(kind, sample(kind));
        assert_eq!(req.choice_type, ChoiceType::ChooseOptionOrValue);
        assert_eq!(
            items(&req),
            vec![
                ChoiceOption::Text("test.value_c".to_owned()),
                ChoiceOption::Text("test.value_a".to_owned()),
                ChoiceOption::Text("test.value_b".to_owned()),
            ],
            "{kind:?}"
        );
    }
}

/// REQ-DSL-011: PLAYER -> CHOOSE_PLAYER; items ascending; request min = max = 1.
#[test]
fn req_dsl_011_player_items_and_bounds() {
    let req = ask_kind(ChoiceKind::Player, resolved(players(&[3, 1, 2]), 1, 1));
    assert_eq!(
        items(&req),
        vec![
            ChoiceOption::Player(pid(1)),
            ChoiceOption::Player(pid(2)),
            ChoiceOption::Player(pid(3))
        ]
    );
    assert_eq!((req.min, req.max), (1, 1));
}

/// REQ-DSL-011: YES_NO -> CHOOSE_OPTIONAL_TRIGGER; items [Yes, No]; request min = max = 1
/// whatever the resolved bounds (a yes/no has no candidates to bound).
#[test]
fn req_dsl_011_yes_no_items_and_bounds() {
    for (min, max) in [(1, 1), (0, 0), (0, 5)] {
        let req = ask_kind(
            ChoiceKind::YesNo,
            resolved(ResolvedCandidates::None, min, max),
        );
        assert_eq!(req.choice_type, ChoiceType::ChooseOptionalTrigger);
        assert_eq!(items(&req), vec![ChoiceOption::Yes, ChoiceOption::No]);
        assert_eq!((req.min, req.max), (1, 1), "resolved {min}/{max}");
    }
}

/// REQ-DSL-011: ORDER -> ORDER_CARDS; items are the candidates ascending.
#[test]
fn req_dsl_011_order_items() {
    let req = ask_kind(ChoiceKind::Order, resolved(objects(&[9, 4, 6]), 3, 3));
    assert_eq!(
        items(&req),
        vec![
            ChoiceOption::Object(oid(4)),
            ChoiceOption::Object(oid(6)),
            ChoiceOption::Object(oid(9))
        ]
    );
}

/// REQ-DSL-011, CR 601.2d, CR 510.1c, CR 510.1d: DIVISION -> DIVIDE_VALUE; items name the
/// recipients (objects and players) in the given order.
#[test]
fn req_dsl_011_division_items_in_given_order() {
    let req = ask_kind(
        ChoiceKind::Division,
        division(5, vec![(to(8), 1), (tp(2), 0), (to(3), 1)]),
    );
    assert_eq!(req.choice_type, ChoiceType::DivideValue);
    assert_eq!(
        items(&req),
        vec![
            ChoiceOption::Object(oid(8)),
            ChoiceOption::Player(pid(2)),
            ChoiceOption::Object(oid(3))
        ]
    );
}

/// REQ-DSL-011: CARD_NAME -> CHOOSE_CARD_NAME, always a validator contract.
#[test]
fn req_dsl_011_card_name_is_validator() {
    let req = ask_kind(ChoiceKind::CardName, sample(ChoiceKind::CardName));
    assert_eq!(req.choice_type, ChoiceType::ChooseCardName);
    assert!(is_validator(&req));
}

/// REQ-DSL-011: a spec that fails `ChoiceSpec::validate` is `InvalidSpec`.
#[test]
fn req_dsl_011_invalid_spec_rejected() {
    let mut s = spec(ChoiceKind::Objects);
    s.up_to = Some(true);
    assert!(s.validate().is_err());
    let g = generate_request(RequestId::new(0), &s, sample(ChoiceKind::Objects));
    assert!(matches!(g, Err(ChoiceError::InvalidSpec(_))), "{g:?}");

    let mut s = spec(ChoiceKind::Number);
    s.min = Some(ValueExpressionAST::Literal(5));
    s.max = Some(ValueExpressionAST::Literal(2));
    assert!(s.validate().is_err());
    let g = generate_request(RequestId::new(0), &s, sample(ChoiceKind::Number));
    assert!(matches!(g, Err(ChoiceError::InvalidSpec(_))), "{g:?}");
}

/// REQ-DSL-011: duplicate Object or Player candidates are `InvalidSpec`; duplicate or empty
/// strings are an error.
#[test]
fn req_dsl_011_duplicate_candidates_rejected() {
    for kind in [ChoiceKind::Objects, ChoiceKind::Order] {
        let g = gen_kind(kind, resolved(objects(&[1, 2, 1]), 1, 1));
        assert!(
            matches!(g, Err(ChoiceError::InvalidSpec(_))),
            "{kind:?}: {g:?}"
        );
    }
    let g = gen_kind(ChoiceKind::Player, resolved(players(&[2, 2]), 1, 1));
    assert!(matches!(g, Err(ChoiceError::InvalidSpec(_))), "{g:?}");
    for kind in [
        ChoiceKind::Mode,
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
    ] {
        let dup = vec!["test.value_a".to_owned(), "test.value_a".to_owned()];
        assert!(
            gen_kind(kind, resolved(strings(&dup), 1, 1)).is_err(),
            "{kind:?}"
        );
        let empty = vec!["test.value_a".to_owned(), String::new()];
        assert!(
            gen_kind(kind, resolved(strings(&empty), 1, 1)).is_err(),
            "{kind:?}"
        );
    }
}

/// REQ-DSL-011: a kind that requires candidates and gets none is `MissingCandidates`; a
/// division without `resolved.division` is an error.
#[test]
fn req_dsl_011_missing_candidates() {
    for kind in [
        ChoiceKind::Objects,
        ChoiceKind::Mode,
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
        ChoiceKind::Player,
        ChoiceKind::Order,
    ] {
        let g = gen_kind(kind, resolved(ResolvedCandidates::None, 1, 1));
        assert!(
            matches!(g, Err(ChoiceError::MissingCandidates)),
            "{kind:?}: {g:?}"
        );
    }
    let g = gen_kind(
        ChoiceKind::Division,
        resolved(ResolvedCandidates::None, 1, 1),
    );
    assert!(g.is_err(), "{g:?}");
}

/// REQ-DSL-011: unsatisfiable bounds are `Unsatisfiable`: min > n, min > max, division floors
/// above the total, a division with no recipients.
#[test]
fn req_dsl_011_unsatisfiable() {
    let cases: Vec<(ChoiceKind, ResolvedChoice)> = vec![
        (ChoiceKind::Objects, resolved(objects(&[1, 2]), 3, 3)),
        (ChoiceKind::Objects, resolved(objects(&[1, 2, 3]), 2, 1)),
        (
            ChoiceKind::Mode,
            resolved(strings(&labels("mode", 2)), 3, 4),
        ),
        (
            ChoiceKind::Mode,
            resolved(strings(&labels("mode", 3)), 2, 1),
        ),
        (ChoiceKind::Number, resolved(ResolvedCandidates::None, 5, 4)),
        // CR 601.2d, CR 510.1c: per-recipient minimums summing above the total.
        (
            ChoiceKind::Division,
            division(2, vec![(to(1), 1), (to(2), 2)]),
        ),
        (ChoiceKind::Division, division(3, Vec::new())),
    ];
    for (kind, r) in cases {
        let g = gen_kind(kind, r.clone());
        assert!(
            matches!(g, Err(ChoiceError::Unsatisfiable(_))),
            "{kind:?} {r:?}: {g:?}"
        );
    }
}

/// REQ-DSL-011: CARD_NAME without a provided default is `NoDefault`; an invalid provided
/// default is `InvalidDefault`.
#[test]
fn req_dsl_011_card_name_default() {
    let g = gen_kind(
        ChoiceKind::CardName,
        resolved(ResolvedCandidates::None, 1, 1),
    );
    assert!(matches!(g, Err(ChoiceError::NoDefault)), "{g:?}");
    for bad in [
        ChoiceResponse::SubmitCardName {
            name: String::new(),
        },
        ChoiceResponse::SubmitCardName {
            name: "   ".to_owned(),
        },
        ChoiceResponse::Yes,
    ] {
        let mut r = resolved(ResolvedCandidates::None, 1, 1);
        r.default = Some(bad.clone());
        let g = gen_kind(ChoiceKind::CardName, r);
        assert!(
            matches!(g, Err(ChoiceError::InvalidDefault(_))),
            "{bad:?}: {g:?}"
        );
    }
}

/// REQ-DSL-011, REQ-NET-003: a provided default that does not validate is `InvalidDefault` for
/// every kind (wrong variant, and an out-of-bounds payload).
#[test]
fn req_dsl_011_invalid_default_every_kind() {
    for kind in ChoiceKind::ALL {
        let mut r = sample(*kind);
        r.default = Some(ChoiceResponse::TakeMulligan);
        let g = gen_kind(*kind, r);
        assert!(
            matches!(g, Err(ChoiceError::InvalidDefault(_))),
            "{kind:?}: {g:?}"
        );
    }
    let payload_cases: Vec<(ChoiceKind, ChoiceResponse)> = vec![
        (ChoiceKind::Objects, sel(&[3, 5, 7])),
        (
            ChoiceKind::Mode,
            ChoiceResponse::SelectModes { modes: vec![3] },
        ),
        (ChoiceKind::Number, num(7)),
        (ChoiceKind::Color, text("test.value_z")),
        (
            ChoiceKind::Player,
            ChoiceResponse::SubmitPlayer { player: pid(9) },
        ),
        (ChoiceKind::Order, ordering(&[4, 6], &[], &[])),
        (
            ChoiceKind::Division,
            div(&[(to(8), 5), (tp(2), 0), (to(3), 0)]),
        ),
    ];
    for (kind, bad) in payload_cases {
        let mut r = sample(kind);
        r.default = Some(bad.clone());
        let g = gen_kind(kind, r);
        assert!(
            matches!(g, Err(ChoiceError::InvalidDefault(_))),
            "{kind:?} {bad:?}: {g:?}"
        );
    }
}

/// REQ-NET-003: a provided valid default is used verbatim.
#[test]
fn req_net_003_provided_default_verbatim() {
    let cases: Vec<(ChoiceKind, ChoiceResponse)> = vec![
        (ChoiceKind::Objects, sel(&[7, 3])),
        (
            ChoiceKind::Mode,
            ChoiceResponse::SelectModes { modes: vec![2, 0] },
        ),
        (ChoiceKind::Number, num(5)),
        (ChoiceKind::Color, text("test.value_b")),
        (ChoiceKind::CreatureType, text("test.value_a")),
        (ChoiceKind::Pile, text("test.value_b")),
        (
            ChoiceKind::CardName,
            ChoiceResponse::SubmitCardName {
                name: "test.name_b".to_owned(),
            },
        ),
        (
            ChoiceKind::Player,
            ChoiceResponse::SubmitPlayer { player: pid(3) },
        ),
        (ChoiceKind::YesNo, ChoiceResponse::Yes),
        (ChoiceKind::Order, ordering(&[9, 4, 6], &[], &[])),
        (
            ChoiceKind::Division,
            div(&[(to(8), 1), (tp(2), 3), (to(3), 1)]),
        ),
    ];
    for (kind, d) in cases {
        let mut r = sample(kind);
        r.default = Some(d.clone());
        let req = ask_kind(kind, r);
        assert_eq!(req.default_on_timeout, d, "{kind:?}");
        accept(&req, &d);
    }
}

/// REQ-NET-003: the computed defaults on timeout, exactly as the brief pins them, and each one
/// passes `validate`.
#[test]
fn req_net_003_computed_defaults() {
    let cases: Vec<(ChoiceKind, ResolvedChoice, ChoiceResponse)> = vec![
        (
            ChoiceKind::YesNo,
            sample(ChoiceKind::YesNo),
            ChoiceResponse::No,
        ),
        (
            ChoiceKind::Number,
            resolved(ResolvedCandidates::None, 2, 6),
            num(2),
        ),
        (
            ChoiceKind::Color,
            sample(ChoiceKind::Color),
            text("test.value_c"),
        ),
        (
            ChoiceKind::CreatureType,
            sample(ChoiceKind::CreatureType),
            text("test.value_c"),
        ),
        (
            ChoiceKind::Pile,
            sample(ChoiceKind::Pile),
            text("test.value_c"),
        ),
        (
            ChoiceKind::Player,
            resolved(players(&[3, 1, 2]), 1, 1),
            ChoiceResponse::SubmitPlayer { player: pid(1) },
        ),
        // The `min` smallest ids, ascending, though the candidates came unsorted.
        (
            ChoiceKind::Objects,
            resolved(objects(&[7, 3, 5, 1]), 2, 3),
            sel(&[1, 3]),
        ),
        (
            ChoiceKind::Objects,
            resolved(objects(&[7, 3, 5]), 0, 3),
            sel(&[]),
        ),
        (
            ChoiceKind::Mode,
            resolved(strings(&labels("mode", 4)), 2, 4),
            ChoiceResponse::SelectModes { modes: vec![0, 1] },
        ),
        (
            ChoiceKind::Mode,
            resolved(strings(&labels("mode", 4)), 0, 4),
            ChoiceResponse::SelectModes { modes: vec![] },
        ),
        (
            ChoiceKind::Order,
            resolved(objects(&[9, 4, 6]), 3, 3),
            ordering(&[4, 6, 9], &[], &[]),
        ),
    ];
    for (kind, r, expected) in cases {
        let req = ask_kind(kind, r);
        assert_eq!(req.default_on_timeout, expected, "{kind:?}");
        accept(&req, &req.default_on_timeout);
    }
}

/// REQ-NET-003, CR 601.2d, CR 510.1c: the DIVISION default gives every recipient its floor and
/// the remainder to the first recipient listed (compared as a set of pairs; the order of the
/// assignment list is not pinned).
#[test]
fn req_net_003_division_default() {
    let cases = [
        (
            10,
            vec![(to(5), 1), (tp(2), 0), (to(3), 2)],
            vec![(to(5), 8), (tp(2), 0), (to(3), 2)],
        ),
        (
            3,
            vec![(tp(4), 1), (tp(1), 1), (to(2), 1)],
            vec![(tp(4), 1), (tp(1), 1), (to(2), 1)],
        ),
        (7, vec![(to(9), 0)], vec![(to(9), 7)]),
        (
            u64::MAX,
            vec![(to(1), 0), (to(2), 0)],
            vec![(to(1), u64::MAX), (to(2), 0)],
        ),
    ];
    for (total, recipients, expected) in cases {
        let req = ask_kind(ChoiceKind::Division, division(total, recipients));
        let ChoiceResponse::SubmitDivision { assignments } = req.default_on_timeout.clone() else {
            panic!("default {:?}", req.default_on_timeout)
        };
        let mut got = assignments;
        got.sort();
        let mut want = expected;
        want.sort();
        assert_eq!(got, want, "total {total}");
        accept(&req, &req.default_on_timeout);
    }
}

/// REQ-NET-003: for every spec-derived request (including validator-sized ones), the default
/// on timeout passes `validate`.
#[test]
fn req_net_003_default_always_valid() {
    let big: Vec<(ChoiceKind, ResolvedChoice)> = vec![
        (
            ChoiceKind::Objects,
            resolved(objects(&range_ids(200)), 0, 200),
        ),
        (
            ChoiceKind::Objects,
            resolved(objects(&range_ids(200)), 150, 200),
        ),
        (
            ChoiceKind::Mode,
            resolved(strings(&labels("mode", 200)), 3, 200),
        ),
        (
            ChoiceKind::Number,
            resolved(ResolvedCandidates::None, 7, 1_000_000),
        ),
        (
            ChoiceKind::Color,
            resolved(strings(&labels("color", 10_000)), 1, 1),
        ),
        (
            ChoiceKind::Player,
            resolved(players(&range_ids(10_000)), 1, 1),
        ),
        (
            ChoiceKind::Order,
            resolved(objects(&range_ids(200)), 200, 200),
        ),
        (
            ChoiceKind::Division,
            division(u64::MAX, vec![(to(1), 5), (to(2), 7), (tp(1), 0)]),
        ),
    ];
    for kind in ChoiceKind::ALL {
        let req = ask_kind(*kind, sample(*kind));
        accept(&req, &req.default_on_timeout);
    }
    for (kind, r) in big {
        let req = ask_kind(kind, r);
        accept(&req, &req.default_on_timeout);
    }
}

/// REQ-NET-003: `digest` is deterministic: the same inputs give the same digest.
#[test]
fn req_net_003_digest_deterministic() {
    for kind in ChoiceKind::ALL {
        let a = ask_kind(*kind, sample(*kind));
        let b = ask_kind(*kind, sample(*kind));
        assert_eq!(a, b, "{kind:?}");
        assert_eq!(a.digest(), b.digest(), "{kind:?}");
    }
}
