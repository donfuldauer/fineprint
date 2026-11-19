// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of the choice protocol (REQ-NET-003, REQ-DSL-011, REQ-CORE-001). Synthetic data
//! only; literal seeds only; no clock.

use std::cell::Cell;
use std::collections::BTreeSet;

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::choice::{ChoiceCandidates, ChoiceKind, ChoiceSpec};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST, ValueExpressionAST};

use super::*;
use crate::state::GameState;
use crate::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn objs(range: std::ops::Range<u64>) -> Vec<ObjectId> {
    range.map(o).collect()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn spec(kind: ChoiceKind) -> ChoiceSpec {
    ChoiceSpec::new(PlayerSelectorAST::Players(PlayerGroup::AllPlayers), kind)
}

fn reason() -> ChoiceReason {
    ChoiceReason {
        cr: "CR 601.2b".to_owned(),
        source: Some(o(900)),
    }
}

fn resolved(candidates: ResolvedCandidates, min: u64, max: u64) -> ResolvedChoice {
    ResolvedChoice {
        chooser: p(1),
        candidates,
        min,
        max,
        reveal_to: Vec::new(),
        division: None,
        default: None,
        reason: reason(),
    }
}

fn division(total: u64, recipients: Vec<(ChoiceTarget, u64)>) -> ResolvedChoice {
    ResolvedChoice {
        division: Some(ResolvedDivision { total, recipients }),
        ..resolved(ResolvedCandidates::None, 0, 0)
    }
}

fn ask(kind: ChoiceKind, r: ResolvedChoice) -> PendingChoiceRequest {
    match generate_request(RequestId::new(0), &spec(kind), r) {
        Ok(Generated::Ask(req)) => req,
        other => panic!("expected a request, got {other:?}"),
    }
}

fn gen_err(kind: ChoiceKind, r: ResolvedChoice) -> ChoiceError {
    match generate_request(RequestId::new(0), &spec(kind), r) {
        Err(e) => e,
        Ok(g) => panic!("expected an error, got {g:?}"),
    }
}

fn is_enumerated(r: &PendingChoiceRequest) -> bool {
    matches!(r.options, ChoiceOptions::Enumerated(_))
}

fn reject(r: &PendingChoiceRequest, resp: &ChoiceResponse) -> String {
    match r.validate(resp) {
        Err(e) => {
            assert!(!e.trim().is_empty(), "empty explanation for {resp:?}");
            e
        }
        Ok(()) => panic!("{resp:?} accepted by {:?}", r.choice_type),
    }
}

fn objects_req(n: u64, min: u64, max: u64) -> PendingChoiceRequest {
    ask(
        ChoiceKind::Objects,
        resolved(ResolvedCandidates::Objects(objs(1..n + 1)), min, max),
    )
}

/// A hand-built request of `choice_type` with `payload` (for the procedure-raised types).
fn manual(
    choice_type: ChoiceType,
    payload: ChoicePayload,
    default: ChoiceResponse,
) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser: p(1),
        choice_type,
        reason: reason(),
        min: 0,
        max: 3,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: default,
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 0,
            max: 3,
            rule: ValidatorRule::Payload,
        }),
        payload,
    }
}

/// One sample response of every variant, in tag order.
fn every_response() -> Vec<ChoiceResponse> {
    vec![
        ChoiceResponse::PassPriority,
        ChoiceResponse::PlayLand {
            object: o(1),
            face_index: 0,
        },
        ChoiceResponse::InitiateCast {
            object: o(1),
            face_index: 0,
            cast_mode: "normal".into(),
        },
        ChoiceResponse::Activate {
            object: o(1),
            ability_id: "a0".into(),
        },
        ChoiceResponse::SpecialAction {
            kind: "turn_face_up".into(),
            object: o(1),
        },
        ChoiceResponse::SelectModes { modes: vec![0] },
        ChoiceResponse::ConfigureCosts {
            alternative: None,
            additional: vec![],
            x: None,
            symbol_options: vec![],
            harmonize: None,
        },
        ChoiceResponse::SelectTargets { targets: vec![] },
        ChoiceResponse::SubmitDivision {
            assignments: vec![],
        },
        ChoiceResponse::PayCosts {
            auto_pay: false,
            methods: vec![],
            mana_abilities: vec![],
        },
        ChoiceResponse::DeclineOptionalPayment,
        ChoiceResponse::CancelAndRollback,
        ChoiceResponse::SubmitAttackers { attacks: vec![] },
        ChoiceResponse::SubmitBlockers { blocks: vec![] },
        ChoiceResponse::OrderTriggers { order: vec![] },
        ChoiceResponse::Yes,
        ChoiceResponse::No,
        ChoiceResponse::SelectReplacement { index: 0 },
        ChoiceResponse::SubmitObjectSelection { objects: vec![] },
        ChoiceResponse::SubmitOrdering {
            top: vec![],
            bottom: vec![],
            graveyard: vec![],
        },
        ChoiceResponse::SubmitPlayer { player: p(1) },
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(0),
        },
        ChoiceResponse::SubmitCardName {
            name: "test.name".into(),
        },
        ChoiceResponse::SubmitSet { objects: vec![] },
        ChoiceResponse::Accept,
        ChoiceResponse::Shorten { at_index: 0 },
        ChoiceResponse::KeepHand { bottom: vec![] },
        ChoiceResponse::TakeMulligan,
        ChoiceResponse::SubmitPregame { objects: vec![] },
    ]
}

/// A permissive payload for every choice type.
fn payload_for(t: ChoiceType) -> ChoicePayload {
    match t {
        ChoiceType::PriorityAction => ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: vec![
                ChoiceResponse::PlayLand {
                    object: o(1),
                    face_index: 0,
                },
                ChoiceResponse::InitiateCast {
                    object: o(1),
                    face_index: 0,
                    cast_mode: "normal".into(),
                },
                ChoiceResponse::Activate {
                    object: o(1),
                    ability_id: "a0".into(),
                },
                ChoiceResponse::SpecialAction {
                    kind: "turn_face_up".into(),
                    object: o(1),
                },
            ],
        },
        ChoiceType::ChooseModes => ChoicePayload::ChooseModes {
            labels: strs(&["m0", "m1", "m2"]),
            repeats_allowed: false,
            mode_costs: vec![],
        },
        ChoiceType::ChooseCostsAndX => ChoicePayload::ChooseCostsAndX {
            alternative_costs: strs(&["alt0"]),
            additional_costs: strs(&["add0", "add1"]),
            x_range: None,
            symbol_slots: vec![],
            harmonize_candidates: vec![o(5)],
        },
        ChoiceType::ChooseTargets => ChoicePayload::ChooseTargets {
            slots: vec![],
            may_keep_illegal: false,
        },
        ChoiceType::DivideValue => ChoicePayload::DivideValue {
            total: 0,
            recipients: vec![],
        },
        ChoiceType::PayManaAndCosts => ChoicePayload::PayManaAndCosts {
            locked_total: "2".into(),
            methods: vec![
                ("convoke".into(), vec![o(1), o(2)]),
                ("delve".into(), vec![o(3)]),
            ],
            mana_abilities: vec![(o(7), "tap".into())],
            auto_pay: true,
            optional: true,
            cancellable: true,
        },
        ChoiceType::DeclareAttackers => ChoicePayload::DeclareAttackers {
            attackers: vec![(o(1), vec![ChoiceTarget::Player(p(2))])],
            max_requirements: 0,
        },
        ChoiceType::DeclareBlockers => ChoicePayload::DeclareBlockers {
            blockers: vec![(o(3), vec![o(1), o(2)], 1)],
        },
        ChoiceType::OrderSimultaneousTriggers => {
            ChoicePayload::OrderSimultaneousTriggers { triggers: vec![] }
        }
        ChoiceType::ChooseOptionalTrigger => ChoicePayload::ChooseOptionalTrigger {
            description: "d".into(),
        },
        ChoiceType::ChooseReplacementEffect => ChoicePayload::ChooseReplacementEffect {
            event: "e".into(),
            effects: strs(&["r0", "r1"]),
        },
        ChoiceType::SelectObjectsFromZone => ChoicePayload::SelectObjectsFromZone {
            candidates: objs(1..4),
        },
        ChoiceType::OrderCards => ChoicePayload::OrderCards {
            cards: vec![],
            destinations: vec![OrderDestination::Top],
        },
        ChoiceType::ChoosePlayer => ChoicePayload::ChoosePlayer {
            players: vec![p(1)],
        },
        ChoiceType::ChooseOptionOrValue => ChoicePayload::ChooseOptionOrValue {
            option_type: "NUMBER".into(),
            allowed: AllowedValues::Range,
        },
        ChoiceType::ChooseCardName => ChoicePayload::ChooseCardName,
        ChoiceType::ChooseUntapSet => ChoicePayload::ChooseUntapSet {
            permanents: objs(1..4),
        },
        ChoiceType::ShortcutResponse => ChoicePayload::ShortcutResponse {
            sequence: strs(&["s0"]),
        },
        ChoiceType::MulliganDecision => ChoicePayload::MulliganDecision {
            hand: objs(1..8),
            mulligans_taken: 0,
            to_bottom: 0,
        },
        ChoiceType::PregameAction => ChoicePayload::PregameAction {
            eligible: objs(1..3),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Catalogue (REQ-NET-003)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: the 20 SRS 15.3 names, in table order, round-tripping through `from_wire`.
#[test]
fn req_net_003_choice_type_catalogue() {
    let names = [
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
    assert_eq!(ChoiceType::ALL.len(), 20);
    for (i, (t, name)) in ChoiceType::ALL.iter().zip(names).enumerate() {
        assert_eq!(t.as_str(), name);
        assert_eq!(t.to_string(), name);
        assert_eq!(ChoiceType::from_wire(name), Some(*t));
        assert_eq!(usize::from(t.tag()), i);
        assert_eq!(encode_to_vec(t), vec![t.tag()]);
        assert_eq!(payload_for(*t).choice_type(), *t);
        assert_eq!(encode_to_vec(&payload_for(*t))[0], t.tag());
    }
    assert_eq!(ChoiceType::ALL[0], ChoiceType::PriorityAction);
    assert_eq!(ChoiceType::ALL[19], ChoiceType::PregameAction);
    for unknown in [
        "",
        "priority_action",
        "PRIORITY ACTION",
        "CHOOSE_MODE",
        "PASS_PRIORITY",
    ] {
        assert_eq!(ChoiceType::from_wire(unknown), None);
    }
}

/// REQ-NET-003: every response variant has a distinct tag, belongs to one type, and every type
/// has at least one response; encodings are injective on the samples.
#[test]
fn req_net_003_response_catalogue_and_encoding() {
    let all = every_response();
    assert_eq!(all.len(), 29);
    let tags: BTreeSet<u8> = all.iter().map(ChoiceResponse::tag).collect();
    assert_eq!(tags.len(), 29);
    for (i, r) in all.iter().enumerate() {
        assert_eq!(usize::from(r.tag()), i);
        assert_eq!(encode_to_vec(r)[0], r.tag());
        assert!(!r.wire_name().is_empty());
    }
    let names: BTreeSet<&str> = all.iter().map(ChoiceResponse::wire_name).collect();
    assert_eq!(names.len(), 29);
    let types: BTreeSet<ChoiceType> = all.iter().map(ChoiceResponse::choice_type).collect();
    assert_eq!(types.len(), 20);
    let encodings: BTreeSet<Vec<u8>> = all.iter().map(encode_to_vec).collect();
    assert_eq!(encodings.len(), all.len());
    assert_ne!(
        encode_to_vec(&ChoiceValue::Number(1)),
        encode_to_vec(&ChoiceValue::Text("1".into()))
    );
    assert_ne!(
        encode_to_vec(&ChoiceTarget::Player(p(1))),
        encode_to_vec(&ChoiceTarget::Object(o(1)))
    );
    let opts = [
        ChoiceOption::Object(o(1)),
        ChoiceOption::Player(p(1)),
        ChoiceOption::Text("1".into()),
        ChoiceOption::Number(1),
        ChoiceOption::Mode(1),
        ChoiceOption::Yes,
        ChoiceOption::No,
    ];
    let enc: BTreeSet<Vec<u8>> = opts.iter().map(encode_to_vec).collect();
    assert_eq!(enc.len(), opts.len());
    let rules = [
        ValidatorRule::Subset { candidates: vec![] },
        ValidatorRule::Modes {
            count: 0,
            repeats_allowed: false,
        },
        ValidatorRule::IntegerRange,
        ValidatorRule::OneOf { values: vec![] },
        ValidatorRule::OneOfPlayers { players: vec![] },
        ValidatorRule::Permutation { items: vec![] },
        ValidatorRule::Division {
            total: 0,
            recipients: vec![],
        },
        ValidatorRule::CardName { max_bytes: 0 },
        ValidatorRule::Payload,
    ];
    let enc: BTreeSet<Vec<u8>> = rules.iter().map(encode_to_vec).collect();
    assert_eq!(enc.len(), rules.len());
    let dests = [
        OrderDestination::Top,
        OrderDestination::Bottom,
        OrderDestination::Graveyard,
    ];
    let enc: BTreeSet<Vec<u8>> = dests.iter().map(encode_to_vec).collect();
    assert_eq!(enc.len(), 3);
    assert_ne!(
        encode_to_vec(&AllowedValues::Range),
        encode_to_vec(&AllowedValues::Texts(vec![]))
    );
}

/// REQ-NET-003: the request digest is SHA-256 of the canonical encoding and changes with any
/// field.
#[test]
fn req_net_003_request_digest() {
    let r = objects_req(3, 0, 2);
    assert_eq!(r.digest(), fineprint_base::hash::sha256(&encode_to_vec(&r)));
    let mut changed = r.clone();
    changed.hidden = true;
    assert_ne!(changed.digest(), r.digest());
    let mut changed = r.clone();
    changed.request_id = RequestId::new(1);
    assert_ne!(changed.digest(), r.digest());
    let mut changed = r.clone();
    changed.reason.source = None;
    assert_ne!(changed.digest(), r.digest());
    let mut changed = r.clone();
    changed.payload = ChoicePayload::SelectObjectsFromZone {
        candidates: objs(1..3),
    };
    assert_ne!(changed.digest(), r.digest());
    assert_eq!(RequestId::new(7).get(), 7);
    assert_eq!(RequestId::new(7).to_string(), "7");
}

// ---------------------------------------------------------------------------------------------
// Generation (REQ-DSL-011)
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-011: OBJECTS -> SELECT_OBJECTS_FROM_ZONE with the candidates, clamped max, default
/// of the `min` smallest ids, hidden and reveal_to copied.
#[test]
fn req_dsl_011_objects() {
    let mut s = spec(ChoiceKind::Objects);
    s.hidden = Some(true);
    let mut r = resolved(ResolvedCandidates::Objects(vec![o(3), o(5), o(9)]), 2, 7);
    r.reveal_to = vec![p(2)];
    let Ok(Generated::Ask(req)) = generate_request(RequestId::new(4), &s, r) else {
        panic!("expected a request");
    };
    assert_eq!(req.request_id, RequestId::new(4));
    assert_eq!(req.choice_type, ChoiceType::SelectObjectsFromZone);
    assert_eq!((req.min, req.max), (2, 3));
    assert!(req.hidden);
    assert_eq!(req.reveal_to, vec![p(2)]);
    assert_eq!(req.chooser, p(1));
    assert_eq!(req.reason, reason());
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Object(o(3)),
            ChoiceOption::Object(o(5)),
            ChoiceOption::Object(o(9))
        ])
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(3), o(5)]
        }
    );
    let mut s = spec(ChoiceKind::Objects);
    s.hidden = Some(false);
    let Ok(Generated::Ask(req)) = generate_request(
        RequestId::new(0),
        &s,
        resolved(ResolvedCandidates::Objects(objs(1..3)), 0, 1),
    ) else {
        panic!("expected a request");
    };
    assert!(!req.hidden);
    assert!(!objects_req(2, 0, 1).hidden);
}

/// REQ-DSL-011: OBJECTS candidate and bound errors.
#[test]
fn req_dsl_011_objects_errors() {
    assert_eq!(
        gen_err(
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::None, 0, 1)
        ),
        ChoiceError::MissingCandidates
    );
    assert!(matches!(
        gen_err(
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::Players(vec![p(1)]), 0, 1)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    // Unsorted candidates are sorted ascending.
    let req = ask(
        ChoiceKind::Objects,
        resolved(ResolvedCandidates::Objects(vec![o(2), o(1)]), 1, 1),
    );
    assert_eq!(
        req.payload,
        ChoicePayload::SelectObjectsFromZone {
            candidates: vec![o(1), o(2)]
        }
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(1)]
        }
    );
    assert!(matches!(
        gen_err(
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::Objects(vec![o(2), o(2)]), 0, 1)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::Objects(objs(1..3)), 3, 4)
        ),
        ChoiceError::Unsatisfiable(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::Objects(objs(1..3)), 2, 1)
        ),
        ChoiceError::Unsatisfiable(_)
    ));
    let mut r = resolved(ResolvedCandidates::Objects(objs(1..3)), 0, 1);
    r.division = Some(ResolvedDivision {
        total: 1,
        recipients: vec![],
    });
    assert_eq!(
        gen_err(ChoiceKind::Objects, r),
        ChoiceError::MissingCandidates
    );
}

/// REQ-DSL-011: MODE -> CHOOSE_MODES; labels in order; default 0..min.
#[test]
fn req_dsl_011_mode() {
    let req = ask(
        ChoiceKind::Mode,
        resolved(ResolvedCandidates::Strings(strs(&["b", "a", "c"])), 2, 9),
    );
    assert_eq!(req.choice_type, ChoiceType::ChooseModes);
    assert_eq!((req.min, req.max), (2, 3));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Mode(0),
            ChoiceOption::Mode(1),
            ChoiceOption::Mode(2)
        ])
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SelectModes { modes: vec![0, 1] }
    );
    match &req.payload {
        ChoicePayload::ChooseModes {
            labels,
            repeats_allowed,
            ..
        } => {
            assert_eq!(labels, &strs(&["b", "a", "c"]));
            assert!(!repeats_allowed);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        gen_err(ChoiceKind::Mode, resolved(ResolvedCandidates::None, 0, 1)),
        ChoiceError::MissingCandidates
    );
    assert!(matches!(
        gen_err(
            ChoiceKind::Mode,
            resolved(ResolvedCandidates::Strings(strs(&["a", "a"])), 0, 1)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Mode,
            resolved(ResolvedCandidates::Strings(strs(&["a", ""])), 0, 1)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Mode,
            resolved(ResolvedCandidates::Objects(objs(1..2)), 0, 1)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Mode,
            resolved(ResolvedCandidates::Strings(strs(&["a"])), 2, 2)
        ),
        ChoiceError::Unsatisfiable(_)
    ));
}

/// REQ-DSL-011: NUMBER -> CHOOSE_OPTION_OR_VALUE over min..=max; default min; candidates are
/// extra; bounds above i64::MAX clamp.
#[test]
fn req_dsl_011_number() {
    let req = ask(ChoiceKind::Number, resolved(ResolvedCandidates::None, 2, 4));
    assert_eq!(req.choice_type, ChoiceType::ChooseOptionOrValue);
    assert_eq!((req.min, req.max), (2, 4));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Number(2),
            ChoiceOption::Number(3),
            ChoiceOption::Number(4)
        ])
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(2)
        }
    );
    assert_eq!(
        gen_err(
            ChoiceKind::Number,
            resolved(ResolvedCandidates::Strings(strs(&["a"])), 0, 1)
        ),
        ChoiceError::MissingCandidates
    );
    assert!(matches!(
        gen_err(ChoiceKind::Number, resolved(ResolvedCandidates::None, 5, 4)),
        ChoiceError::Unsatisfiable(_)
    ));
    let req = ask(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, u64::MAX),
    );
    assert_eq!(req.max, i64::MAX as u64);
    assert!(!is_enumerated(&req));
    assert!(matches!(
        gen_err(
            ChoiceKind::Number,
            resolved(ResolvedCandidates::None, u64::MAX, u64::MAX)
        ),
        ChoiceError::Unsatisfiable(_)
    ));
}

/// REQ-DSL-011: COLOR, CREATURE_TYPE and PILE -> CHOOSE_OPTION_OR_VALUE over the strings;
/// exactly one; default the first string.
#[test]
fn req_dsl_011_string_kinds() {
    for kind in [
        ChoiceKind::Color,
        ChoiceKind::CreatureType,
        ChoiceKind::Pile,
    ] {
        let req = ask(
            kind,
            resolved(ResolvedCandidates::Strings(strs(&["z", "y"])), 0, 5),
        );
        assert_eq!(req.choice_type, ChoiceType::ChooseOptionOrValue);
        assert_eq!((req.min, req.max), (1, 1));
        assert_eq!(
            req.options,
            ChoiceOptions::Enumerated(vec![
                ChoiceOption::Text("z".into()),
                ChoiceOption::Text("y".into())
            ])
        );
        assert_eq!(
            req.default_on_timeout,
            ChoiceResponse::SubmitChoiceValue {
                value: ChoiceValue::Text("z".into())
            }
        );
        match &req.payload {
            ChoicePayload::ChooseOptionOrValue { option_type, .. } => {
                assert_eq!(option_type, kind.as_str())
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            gen_err(kind, resolved(ResolvedCandidates::Strings(vec![]), 0, 1)),
            ChoiceError::Unsatisfiable(_)
        ));
        assert_eq!(
            gen_err(kind, resolved(ResolvedCandidates::None, 0, 1)),
            ChoiceError::MissingCandidates
        );
        assert!(matches!(
            gen_err(
                kind,
                resolved(ResolvedCandidates::Players(vec![p(1)]), 0, 1)
            ),
            ChoiceError::InvalidSpec(_)
        ));
    }
}

/// REQ-DSL-011, REQ-NET-003: CARD_NAME is always a validator contract; no default unless
/// provided.
#[test]
fn req_dsl_011_card_name() {
    assert_eq!(
        gen_err(
            ChoiceKind::CardName,
            resolved(ResolvedCandidates::None, 1, 1)
        ),
        ChoiceError::NoDefault
    );
    assert_eq!(
        gen_err(
            ChoiceKind::CardName,
            resolved(ResolvedCandidates::Strings(strs(&["a"])), 1, 1)
        ),
        ChoiceError::MissingCandidates
    );
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName {
        name: "test.vanilla_1_1".into(),
    });
    let req = ask(ChoiceKind::CardName, r);
    assert_eq!(req.choice_type, ChoiceType::ChooseCardName);
    assert_eq!((req.min, req.max), (1, 1));
    assert_eq!(
        req.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::CardName {
                max_bytes: MAX_CARD_NAME_BYTES
            }
        })
    );
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName { name: "  ".into() });
    assert!(matches!(
        gen_err(ChoiceKind::CardName, r),
        ChoiceError::InvalidDefault(_)
    ));
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::Yes);
    assert!(matches!(
        gen_err(ChoiceKind::CardName, r),
        ChoiceError::InvalidDefault(_)
    ));
}

/// REQ-DSL-011: PLAYER -> CHOOSE_PLAYER, min = max = 1, default the lowest id.
#[test]
fn req_dsl_011_player() {
    let req = ask(
        ChoiceKind::Player,
        resolved(ResolvedCandidates::Players(vec![p(2), p(4)]), 0, 0),
    );
    assert_eq!(req.choice_type, ChoiceType::ChoosePlayer);
    assert_eq!((req.min, req.max), (1, 1));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Player(p(2)), ChoiceOption::Player(p(4))])
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitPlayer { player: p(2) }
    );
    let req = ask(
        ChoiceKind::Player,
        resolved(ResolvedCandidates::Players(vec![p(4), p(2)]), 0, 0),
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitPlayer { player: p(2) }
    );
    assert!(matches!(
        gen_err(
            ChoiceKind::Player,
            resolved(ResolvedCandidates::Players(vec![p(4), p(4)]), 0, 0)
        ),
        ChoiceError::InvalidSpec(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Player,
            resolved(ResolvedCandidates::Players(vec![]), 0, 0)
        ),
        ChoiceError::Unsatisfiable(_)
    ));
    assert_eq!(
        gen_err(ChoiceKind::Player, resolved(ResolvedCandidates::None, 0, 0)),
        ChoiceError::MissingCandidates
    );
    assert!(matches!(
        gen_err(
            ChoiceKind::Player,
            resolved(ResolvedCandidates::Objects(objs(1..2)), 0, 0)
        ),
        ChoiceError::InvalidSpec(_)
    ));
}

/// REQ-DSL-011: YES_NO -> CHOOSE_OPTIONAL_TRIGGER, [Yes, No], default No.
#[test]
fn req_dsl_011_yes_no() {
    let req = ask(ChoiceKind::YesNo, resolved(ResolvedCandidates::None, 1, 1));
    assert_eq!(req.choice_type, ChoiceType::ChooseOptionalTrigger);
    assert_eq!((req.min, req.max), (1, 1));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Yes, ChoiceOption::No])
    );
    assert_eq!(req.default_on_timeout, ChoiceResponse::No);
    assert_eq!(
        gen_err(
            ChoiceKind::YesNo,
            resolved(ResolvedCandidates::Objects(objs(1..2)), 1, 1)
        ),
        ChoiceError::MissingCandidates
    );
}

/// REQ-DSL-011: ORDER -> ORDER_CARDS, top only, default ascending ids on top.
#[test]
fn req_dsl_011_order() {
    let req = ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 0),
    );
    assert_eq!(req.choice_type, ChoiceType::OrderCards);
    assert_eq!((req.min, req.max), (3, 3));
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitOrdering {
            top: objs(1..4),
            bottom: vec![],
            graveyard: vec![]
        }
    );
    assert_eq!(
        gen_err(ChoiceKind::Order, resolved(ResolvedCandidates::None, 0, 0)),
        ChoiceError::MissingCandidates
    );
}

/// REQ-DSL-011: DIVISION -> DIVIDE_VALUE; default floors plus remainder on the first.
#[test]
fn req_dsl_011_division() {
    let recips = vec![
        (ChoiceTarget::Object(o(8)), 1),
        (ChoiceTarget::Player(p(2)), 2),
    ];
    let req = ask(ChoiceKind::Division, division(5, recips.clone()));
    assert_eq!(req.choice_type, ChoiceType::DivideValue);
    assert_eq!((req.min, req.max), (2, 2));
    assert_eq!(
        req.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Object(o(8)), ChoiceOption::Player(p(2))])
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitDivision {
            assignments: vec![
                (ChoiceTarget::Object(o(8)), 3),
                (ChoiceTarget::Player(p(2)), 2)
            ]
        }
    );
    assert_eq!(
        gen_err(
            ChoiceKind::Division,
            resolved(ResolvedCandidates::None, 0, 0)
        ),
        ChoiceError::MissingCandidates
    );
    let mut extra = division(5, recips.clone());
    extra.candidates = ResolvedCandidates::Objects(objs(1..2));
    assert_eq!(
        gen_err(ChoiceKind::Division, extra),
        ChoiceError::MissingCandidates
    );
    assert!(matches!(
        gen_err(ChoiceKind::Division, division(5, vec![])),
        ChoiceError::Unsatisfiable(_)
    ));
    assert!(matches!(
        gen_err(ChoiceKind::Division, division(2, recips)),
        ChoiceError::Unsatisfiable(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Division,
            division(
                5,
                vec![
                    (ChoiceTarget::Object(o(1)), u64::MAX),
                    (ChoiceTarget::Object(o(2)), u64::MAX)
                ]
            )
        ),
        ChoiceError::Unsatisfiable(_)
    ));
    assert!(matches!(
        gen_err(
            ChoiceKind::Division,
            division(
                5,
                vec![
                    (ChoiceTarget::Object(o(1)), 0),
                    (ChoiceTarget::Object(o(1)), 0)
                ]
            )
        ),
        ChoiceError::InvalidSpec(_)
    ));
    let req = ask(
        ChoiceKind::Division,
        division(
            u64::MAX,
            vec![
                (ChoiceTarget::Object(o(1)), 0),
                (ChoiceTarget::Object(o(2)), 0),
            ],
        ),
    );
    assert!(!is_enumerated(&req));
    let req = ask(
        ChoiceKind::Division,
        division(u64::MAX, vec![(ChoiceTarget::Object(o(1)), 0)]),
    );
    assert!(is_enumerated(&req));
}

/// REQ-DSL-011: random -> Generated::Random; spec.validate() failures -> InvalidSpec; a
/// provided default overrides the generated one.
#[test]
fn req_dsl_011_random_validate_and_default() {
    let mut s = spec(ChoiceKind::Objects);
    s.random = Some(true);
    assert_eq!(
        generate_request(
            RequestId::new(0),
            &s,
            resolved(ResolvedCandidates::Objects(objs(1..3)), 1, 1)
        ),
        Ok(Generated::Random)
    );
    s.random = Some(false);
    assert!(matches!(
        generate_request(
            RequestId::new(0),
            &s,
            resolved(ResolvedCandidates::Objects(objs(1..3)), 1, 1)
        ),
        Ok(Generated::Ask(_))
    ));
    let mut bad = spec(ChoiceKind::Number);
    bad.min = Some(ValueExpressionAST::Literal(-1));
    assert!(matches!(
        generate_request(
            RequestId::new(0),
            &bad,
            resolved(ResolvedCandidates::None, 0, 1)
        ),
        Err(ChoiceError::InvalidSpec(_))
    ));
    let mut bad = spec(ChoiceKind::Color);
    bad.candidates = Some(ChoiceCandidates::Enum(vec![]));
    assert!(matches!(
        generate_request(
            RequestId::new(0),
            &bad,
            resolved(ResolvedCandidates::Strings(strs(&["a"])), 1, 1)
        ),
        Err(ChoiceError::InvalidSpec(_))
    ));
    let mut r = resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 2);
    r.default = Some(ChoiceResponse::SubmitObjectSelection {
        objects: vec![o(3)],
    });
    assert_eq!(
        ask(ChoiceKind::Objects, r).default_on_timeout,
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(3)]
        }
    );
    let mut r = resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 2);
    r.default = Some(ChoiceResponse::SubmitObjectSelection {
        objects: vec![o(9)],
    });
    assert!(matches!(
        gen_err(ChoiceKind::Objects, r),
        ChoiceError::InvalidDefault(_)
    ));
}

/// REQ-DSL-011, REQ-NET-003: an "up to" spec (resolved min 0) offers the empty answer.
#[test]
fn req_dsl_011_up_to() {
    let mut s = spec(ChoiceKind::Objects);
    s.up_to = Some(true);
    s.max = Some(ValueExpressionAST::Literal(2));
    let Ok(Generated::Ask(req)) = generate_request(
        RequestId::new(0),
        &s,
        resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 2),
    ) else {
        panic!("expected a request");
    };
    assert_eq!(
        req.validate(&ChoiceResponse::SubmitObjectSelection { objects: vec![] }),
        Ok(())
    );
    assert_eq!(
        req.default_on_timeout,
        ChoiceResponse::SubmitObjectSelection { objects: vec![] }
    );
}

// ---------------------------------------------------------------------------------------------
// Threshold (REQ-NET-003)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: 9,999 answers enumerate; 10,000 switch to the validator contract.
#[test]
fn req_net_003_threshold_boundaries() {
    assert_eq!(ENUMERATION_LIMIT, 10_000);
    let r = ask(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, 9_998),
    );
    match &r.options {
        ChoiceOptions::Enumerated(items) => assert_eq!(items.len(), 9_999),
        other => panic!("{other:?}"),
    }
    let r = ask(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 0, 9_999),
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 0,
            max: 9_999,
            rule: ValidatorRule::IntegerRange
        })
    );
    assert!(is_enumerated(&objects_req(13, 0, 13)));
    let r = objects_req(14, 0, 14);
    match &r.options {
        ChoiceOptions::Validator(c) => {
            assert_eq!((c.min, c.max), (0, 14));
            assert_eq!(
                c.rule,
                ValidatorRule::Subset {
                    candidates: objs(1..15)
                }
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(is_enumerated(&ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..8)), 0, 0)
    )));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..9)), 0, 0)
    )));
    let many = |n: usize| (0..n).map(|i| format!("v{i}")).collect::<Vec<_>>();
    assert!(is_enumerated(&ask(
        ChoiceKind::Color,
        resolved(ResolvedCandidates::Strings(many(9_999)), 1, 1)
    )));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Color,
        resolved(ResolvedCandidates::Strings(many(10_000)), 1, 1)
    )));
    let players = |n: u64| (1..=n).map(p).collect::<Vec<_>>();
    assert!(is_enumerated(&ask(
        ChoiceKind::Player,
        resolved(ResolvedCandidates::Players(players(9_999)), 1, 1)
    )));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Player,
        resolved(ResolvedCandidates::Players(players(10_000)), 1, 1)
    )));
    // Division: C(rem + 1, 1) = rem + 1 for two recipients.
    let two = |total| {
        division(
            total,
            vec![
                (ChoiceTarget::Object(o(1)), 0),
                (ChoiceTarget::Object(o(2)), 0),
            ],
        )
    };
    assert!(is_enumerated(&ask(ChoiceKind::Division, two(9_998))));
    assert!(!is_enumerated(&ask(ChoiceKind::Division, two(9_999))));
    // C(100,2) = 4950; C(100,2) + C(100,3) = 166650.
    assert!(is_enumerated(&objects_req(100, 2, 2)));
    assert!(!is_enumerated(&objects_req(100, 2, 3)));
    // C(141,2) = 9870; C(142,2) = 10011.
    assert!(is_enumerated(&objects_req(141, 2, 2)));
    assert!(!is_enumerated(&objects_req(142, 2, 2)));
    let labels = |n: usize| ResolvedCandidates::Strings((0..n).map(|i| format!("m{i}")).collect());
    assert!(is_enumerated(&ask(
        ChoiceKind::Mode,
        resolved(labels(13), 0, 13)
    )));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Mode,
        resolved(labels(14), 0, 14)
    )));
}

/// REQ-NET-003: counting saturates for huge inputs without panicking.
#[test]
fn req_net_003_threshold_saturation() {
    let r = objects_req(50_000, 0, u64::MAX);
    assert_eq!(r.max, 50_000);
    assert!(!is_enumerated(&r));
    assert!(is_enumerated(&objects_req(50_000, 0, 0)));
    assert!(is_enumerated(&objects_req(50_000, 50_000, 50_000)));
    assert!(is_enumerated(&objects_req(9_999, 9_998, 9_998)));
    assert!(!is_enumerated(&objects_req(10_000, 9_999, 9_999)));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, 1, u64::MAX)
    )));
    let r = ask(
        ChoiceKind::Number,
        resolved(ResolvedCandidates::None, i64::MAX as u64, u64::MAX),
    );
    assert!(is_enumerated(&r));
    assert!(!is_enumerated(&ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..1001)), 0, 0)
    )));
    let recips = (1..=1000)
        .map(|i| (ChoiceTarget::Object(o(i)), 0))
        .collect();
    assert!(!is_enumerated(&ask(
        ChoiceKind::Division,
        division(u64::MAX, recips)
    )));
}

// ---------------------------------------------------------------------------------------------
// Validation (REQ-NET-003, R12)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: SELECT_OBJECTS_FROM_ZONE accepts distinct candidates in count; rejects others.
#[test]
fn req_net_003_validate_objects() {
    let r = objects_req(4, 1, 2);
    let sel = |v: Vec<ObjectId>| ChoiceResponse::SubmitObjectSelection { objects: v };
    assert_eq!(r.validate(&sel(vec![o(2)])), Ok(()));
    assert_eq!(r.validate(&sel(vec![o(4), o(1)])), Ok(()));
    assert!(reject(&r, &sel(vec![])).contains("at least 1"));
    assert!(reject(&r, &sel(vec![o(1), o(2), o(3)])).contains("at most 2"));
    assert!(reject(&r, &sel(vec![o(9)])).contains("not one of"));
    assert!(reject(&r, &sel(vec![o(1), o(1)])).contains("more than once"));
    let mut widened = r.clone();
    widened.max = 3;
    assert_eq!(widened.validate(&sel(vec![o(1), o(2), o(3)])), Ok(()));
}

/// REQ-NET-003: CHOOSE_MODES.
#[test]
fn req_net_003_validate_modes() {
    let r = ask(
        ChoiceKind::Mode,
        resolved(ResolvedCandidates::Strings(strs(&["a", "b", "c"])), 1, 2),
    );
    let m = |v: Vec<u32>| ChoiceResponse::SelectModes { modes: v };
    assert_eq!(r.validate(&m(vec![2])), Ok(()));
    assert_eq!(r.validate(&m(vec![2, 0])), Ok(()));
    reject(&r, &m(vec![]));
    reject(&r, &m(vec![0, 1, 2]));
    assert!(reject(&r, &m(vec![3])).contains("out of range"));
    reject(&r, &m(vec![u32::MAX]));
    assert!(reject(&r, &m(vec![1, 1])).contains("more than once"));
    let mut rep = r.clone();
    rep.payload = ChoicePayload::ChooseModes {
        labels: strs(&["a", "b", "c"]),
        repeats_allowed: true,
        mode_costs: vec![],
    };
    assert_eq!(rep.validate(&m(vec![1, 1])), Ok(()));
}

/// REQ-NET-003: CHOOSE_OPTION_OR_VALUE numbers and texts.
#[test]
fn req_net_003_validate_values() {
    let n = ask(ChoiceKind::Number, resolved(ResolvedCandidates::None, 3, 5));
    let num = |v| ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Number(v),
    };
    let txt = |s: &str| ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Text(s.into()),
    };
    assert_eq!(n.validate(&num(3)), Ok(()));
    assert_eq!(n.validate(&num(5)), Ok(()));
    assert!(reject(&n, &num(2)).contains("outside"));
    reject(&n, &num(6));
    reject(&n, &num(i64::MIN));
    reject(&n, &num(-1));
    reject(&n, &txt("3"));
    let c = ask(
        ChoiceKind::Color,
        resolved(ResolvedCandidates::Strings(strs(&["red", "blue"])), 1, 1),
    );
    assert_eq!(c.validate(&txt("blue")), Ok(()));
    reject(&c, &txt("Blue"));
    reject(&c, &txt(""));
    reject(&c, &num(0));
}

/// REQ-NET-003: CHOOSE_CARD_NAME structural validation.
#[test]
fn req_net_003_validate_card_name() {
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.default = Some(ChoiceResponse::SubmitCardName {
        name: "test.x".into(),
    });
    let req = ask(ChoiceKind::CardName, r);
    let name = |s: &str| ChoiceResponse::SubmitCardName { name: s.into() };
    assert_eq!(req.validate(&name("test.flier_2_2")), Ok(()));
    assert_eq!(req.validate(&name(&"a".repeat(256))), Ok(()));
    assert!(reject(&req, &name("")).contains("empty"));
    reject(&req, &name(" \t "));
    assert!(reject(&req, &name(&"a".repeat(257))).contains("256"));
    assert!(reject(&req, &name("a\u{0}b")).contains("control"));
    reject(&req, &name("a\nb"));
}

/// REQ-NET-003: CHOOSE_PLAYER and CHOOSE_OPTIONAL_TRIGGER.
#[test]
fn req_net_003_validate_player_and_yes_no() {
    let r = ask(
        ChoiceKind::Player,
        resolved(ResolvedCandidates::Players(vec![p(1), p(3)]), 1, 1),
    );
    assert_eq!(
        r.validate(&ChoiceResponse::SubmitPlayer { player: p(3) }),
        Ok(())
    );
    reject(&r, &ChoiceResponse::SubmitPlayer { player: p(2) });
    let y = ask(ChoiceKind::YesNo, resolved(ResolvedCandidates::None, 1, 1));
    assert_eq!(y.validate(&ChoiceResponse::Yes), Ok(()));
    assert_eq!(y.validate(&ChoiceResponse::No), Ok(()));
    reject(&y, &ChoiceResponse::Accept);
}

/// REQ-NET-003: ORDER_CARDS permutations on top only.
#[test]
fn req_net_003_validate_order() {
    let r = ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 0),
    );
    let ord = |top: Vec<ObjectId>, bottom: Vec<ObjectId>, graveyard: Vec<ObjectId>| {
        ChoiceResponse::SubmitOrdering {
            top,
            bottom,
            graveyard,
        }
    };
    assert_eq!(
        r.validate(&ord(vec![o(3), o(1), o(2)], vec![], vec![])),
        Ok(())
    );
    reject(&r, &ord(vec![o(3), o(1)], vec![], vec![]));
    reject(&r, &ord(vec![o(3), o(1), o(1)], vec![], vec![]));
    reject(&r, &ord(vec![o(3), o(1), o(9)], vec![], vec![]));
    assert!(reject(&r, &ord(vec![o(3), o(1)], vec![o(2)], vec![])).contains("bottom"));
    assert!(reject(&r, &ord(vec![o(3), o(1)], vec![], vec![o(2)])).contains("graveyard"));
    let mut scry = r.clone();
    scry.payload = ChoicePayload::OrderCards {
        cards: objs(1..4),
        destinations: vec![OrderDestination::Top, OrderDestination::Bottom],
    };
    assert_eq!(
        scry.validate(&ord(vec![o(3)], vec![o(2), o(1)], vec![])),
        Ok(())
    );
    reject(&scry, &ord(vec![o(3)], vec![o(2), o(3)], vec![]));
}

/// REQ-NET-003: DIVIDE_VALUE, including overflow in sums.
#[test]
fn req_net_003_validate_division() {
    let a = ChoiceTarget::Object(o(1));
    let b = ChoiceTarget::Player(p(2));
    let r = ask(ChoiceKind::Division, division(4, vec![(a, 1), (b, 0)]));
    let d = |v: Vec<(ChoiceTarget, u64)>| ChoiceResponse::SubmitDivision { assignments: v };
    assert_eq!(r.validate(&d(vec![(a, 1), (b, 3)])), Ok(()));
    assert_eq!(r.validate(&d(vec![(b, 0), (a, 4)])), Ok(()));
    assert!(reject(&r, &d(vec![(a, 0), (b, 4)])).contains("at least 1"));
    assert!(reject(&r, &d(vec![(a, 1), (b, 2)])).contains("sum"));
    reject(&r, &d(vec![(a, 4)]));
    reject(&r, &d(vec![(a, 2), (a, 2)]));
    reject(&r, &d(vec![(a, 2), (ChoiceTarget::Object(o(9)), 2)]));
    reject(&r, &d(vec![(a, 1), (b, 3), (b, 0)]));
    assert!(reject(&r, &d(vec![(a, u64::MAX), (b, u64::MAX)])).contains("sum"));
}

/// REQ-NET-003: the procedure-raised types validate the variant and the payload bounds.
#[test]
fn req_net_003_validate_other_types() {
    let req = |t| manual(t, payload_for(t), ChoiceResponse::PassPriority);
    // PRIORITY_ACTION
    let r = req(ChoiceType::PriorityAction);
    assert_eq!(r.validate(&ChoiceResponse::PassPriority), Ok(()));
    assert_eq!(
        r.validate(&ChoiceResponse::PlayLand {
            object: o(1),
            face_index: 0
        }),
        Ok(())
    );
    reject(
        &r,
        &ChoiceResponse::PlayLand {
            object: o(1),
            face_index: 1,
        },
    );
    reject(
        &r,
        &ChoiceResponse::Activate {
            object: o(1),
            ability_id: "a9".into(),
        },
    );
    let mut no_pass = r.clone();
    no_pass.payload = ChoicePayload::PriorityAction {
        can_pass: false,
        can_propose_shortcut: false,
        actions: vec![],
    };
    reject(&no_pass, &ChoiceResponse::PassPriority);
    // CHOOSE_COSTS_AND_X
    let r = req(ChoiceType::ChooseCostsAndX);
    let cc = |alternative, additional: Vec<u32>, x, symbol_options: Vec<u32>, harmonize| {
        ChoiceResponse::ConfigureCosts {
            alternative,
            additional,
            x,
            symbol_options,
            harmonize,
        }
    };
    assert_eq!(
        r.validate(&cc(Some(0), vec![1, 0], None, vec![], Some(o(5)))),
        Ok(())
    );
    reject(&r, &cc(Some(1), vec![], None, vec![], None));
    reject(&r, &cc(None, vec![1, 1], None, vec![], None));
    reject(&r, &cc(None, vec![2], None, vec![], None));
    reject(&r, &cc(None, vec![], Some(1), vec![], None));
    reject(&r, &cc(None, vec![], None, vec![0], None));
    reject(&r, &cc(None, vec![], None, vec![], Some(o(6))));
    let mut xr = r.clone();
    xr.payload = ChoicePayload::ChooseCostsAndX {
        alternative_costs: vec![],
        additional_costs: vec![],
        x_range: Some((1, 3)),
        symbol_slots: vec![strs(&["w", "life"])],
        harmonize_candidates: vec![],
    };
    assert_eq!(
        xr.validate(&cc(None, vec![], Some(3), vec![1], None)),
        Ok(())
    );
    reject(&xr, &cc(None, vec![], None, vec![1], None));
    reject(&xr, &cc(None, vec![], Some(4), vec![1], None));
    reject(&xr, &cc(None, vec![], Some(0), vec![1], None));
    reject(&xr, &cc(None, vec![], Some(2), vec![2], None));
    reject(&xr, &cc(None, vec![], Some(2), vec![], None));
    // CHOOSE_TARGETS
    let mut t = req(ChoiceType::ChooseTargets);
    let legal = vec![ChoiceTarget::Player(p(2)), ChoiceTarget::Object(o(4))];
    t.payload = ChoicePayload::ChooseTargets {
        slots: vec![TargetSlot {
            min: 1,
            max: 1,
            legal: legal.clone(),
            current: vec![ChoiceTarget::Object(o(9))],
        }],
        may_keep_illegal: false,
    };
    let st = |v: Vec<Vec<ChoiceTarget>>| ChoiceResponse::SelectTargets { targets: v };
    assert_eq!(
        t.validate(&st(vec![vec![ChoiceTarget::Object(o(4))]])),
        Ok(())
    );
    reject(&t, &st(vec![]));
    assert!(reject(&t, &st(vec![vec![]])).contains("slot 0"));
    reject(&t, &st(vec![vec![ChoiceTarget::Object(o(9))]]));
    let mut keep = t.clone();
    keep.payload = ChoicePayload::ChooseTargets {
        slots: vec![TargetSlot {
            min: 1,
            max: 2,
            legal,
            current: vec![ChoiceTarget::Object(o(9))],
        }],
        may_keep_illegal: true,
    };
    assert_eq!(
        keep.validate(&st(vec![vec![ChoiceTarget::Object(o(9))]])),
        Ok(())
    );
    reject(
        &keep,
        &st(vec![vec![
            ChoiceTarget::Object(o(9)),
            ChoiceTarget::Object(o(9)),
        ]]),
    );
    // PAY_MANA_AND_COSTS
    let r = req(ChoiceType::PayManaAndCosts);
    let pay = |auto_pay, methods, mana_abilities| ChoiceResponse::PayCosts {
        auto_pay,
        methods,
        mana_abilities,
    };
    assert_eq!(r.validate(&pay(true, vec![], vec![])), Ok(()));
    assert_eq!(
        r.validate(&pay(
            false,
            vec![(0, vec![o(1), o(2)])],
            vec![(o(7), "tap".into())]
        )),
        Ok(())
    );
    assert_eq!(r.validate(&ChoiceResponse::DeclineOptionalPayment), Ok(()));
    assert_eq!(r.validate(&ChoiceResponse::CancelAndRollback), Ok(()));
    reject(&r, &pay(true, vec![(0, vec![])], vec![]));
    reject(&r, &pay(false, vec![(2, vec![])], vec![]));
    reject(&r, &pay(false, vec![(0, vec![o(1), o(1)])], vec![]));
    reject(&r, &pay(false, vec![], vec![(o(7), "other".into())]));
    let mut strict = r.clone();
    strict.payload = ChoicePayload::PayManaAndCosts {
        locked_total: "1".into(),
        methods: vec![],
        mana_abilities: vec![],
        auto_pay: false,
        optional: false,
        cancellable: false,
    };
    reject(&strict, &pay(true, vec![], vec![]));
    reject(&strict, &ChoiceResponse::DeclineOptionalPayment);
    reject(&strict, &ChoiceResponse::CancelAndRollback);
    // DECLARE_ATTACKERS
    let r = req(ChoiceType::DeclareAttackers);
    let att = |v: Vec<(ObjectId, ChoiceTarget)>| ChoiceResponse::SubmitAttackers { attacks: v };
    assert_eq!(r.validate(&att(vec![])), Ok(()));
    assert_eq!(
        r.validate(&att(vec![(o(1), ChoiceTarget::Player(p(2)))])),
        Ok(())
    );
    reject(&r, &att(vec![(o(2), ChoiceTarget::Player(p(2)))]));
    reject(&r, &att(vec![(o(1), ChoiceTarget::Player(p(3)))]));
    reject(
        &r,
        &att(vec![
            (o(1), ChoiceTarget::Player(p(2))),
            (o(1), ChoiceTarget::Player(p(2))),
        ]),
    );
    // DECLARE_BLOCKERS
    let r = req(ChoiceType::DeclareBlockers);
    let blk = |v: Vec<(ObjectId, Vec<ObjectId>)>| ChoiceResponse::SubmitBlockers { blocks: v };
    assert_eq!(r.validate(&blk(vec![(o(3), vec![o(2)])])), Ok(()));
    reject(&r, &blk(vec![(o(3), vec![o(1), o(2)])]));
    reject(&r, &blk(vec![(o(3), vec![])]));
    reject(&r, &blk(vec![(o(3), vec![o(9)])]));
    reject(&r, &blk(vec![(o(4), vec![o(1)])]));
    reject(&r, &blk(vec![(o(3), vec![o(1)]), (o(3), vec![o(2)])]));
    // ORDER_SIMULTANEOUS_TRIGGERS
    let mut r = req(ChoiceType::OrderSimultaneousTriggers);
    r.payload = ChoicePayload::OrderSimultaneousTriggers {
        triggers: strs(&["t0", "t1", "t2"]),
    };
    let ot = |v: Vec<u32>| ChoiceResponse::OrderTriggers { order: v };
    assert_eq!(r.validate(&ot(vec![2, 0, 1])), Ok(()));
    reject(&r, &ot(vec![2, 0]));
    reject(&r, &ot(vec![2, 0, 0]));
    reject(&r, &ot(vec![2, 0, 3]));
    // CHOOSE_REPLACEMENT_EFFECT
    let r = req(ChoiceType::ChooseReplacementEffect);
    assert_eq!(
        r.validate(&ChoiceResponse::SelectReplacement { index: 1 }),
        Ok(())
    );
    reject(&r, &ChoiceResponse::SelectReplacement { index: 2 });
    // CHOOSE_UNTAP_SET
    let r = req(ChoiceType::ChooseUntapSet);
    assert_eq!(
        r.validate(&ChoiceResponse::SubmitSet {
            objects: vec![o(1), o(3)]
        }),
        Ok(())
    );
    reject(
        &r,
        &ChoiceResponse::SubmitSet {
            objects: vec![o(1), o(1)],
        },
    );
    reject(
        &r,
        &ChoiceResponse::SubmitSet {
            objects: vec![o(5)],
        },
    );
    reject(
        &r,
        &ChoiceResponse::SubmitSet {
            objects: objs(1..5),
        },
    );
    // SHORTCUT_RESPONSE
    let r = req(ChoiceType::ShortcutResponse);
    assert_eq!(r.validate(&ChoiceResponse::Accept), Ok(()));
    assert_eq!(r.validate(&ChoiceResponse::Shorten { at_index: 0 }), Ok(()));
    reject(&r, &ChoiceResponse::Shorten { at_index: 1 });
    reject(&r, &ChoiceResponse::Shorten { at_index: u64::MAX });
    // MULLIGAN_DECISION
    let mut r = req(ChoiceType::MulliganDecision);
    r.payload = ChoicePayload::MulliganDecision {
        hand: objs(1..8),
        mulligans_taken: 1,
        to_bottom: 1,
    };
    assert_eq!(r.validate(&ChoiceResponse::TakeMulligan), Ok(()));
    assert_eq!(
        r.validate(&ChoiceResponse::KeepHand { bottom: vec![o(7)] }),
        Ok(())
    );
    reject(&r, &ChoiceResponse::KeepHand { bottom: vec![] });
    reject(&r, &ChoiceResponse::KeepHand { bottom: vec![o(8)] });
    reject(
        &r,
        &ChoiceResponse::KeepHand {
            bottom: vec![o(7), o(6)],
        },
    );
    // PREGAME_ACTION
    let r = req(ChoiceType::PregameAction);
    assert_eq!(
        r.validate(&ChoiceResponse::SubmitPregame {
            objects: vec![o(2)]
        }),
        Ok(())
    );
    reject(
        &r,
        &ChoiceResponse::SubmitPregame {
            objects: vec![o(3)],
        },
    );
    reject(
        &r,
        &ChoiceResponse::SubmitPregame {
            objects: vec![o(2), o(2)],
        },
    );
    // A payload of another type is a malformed request: rejected, not accepted.
    let mut bad = req(ChoiceType::ChooseUntapSet);
    bad.payload = payload_for(ChoiceType::PregameAction);
    assert!(reject(&bad, &ChoiceResponse::SubmitSet { objects: vec![] }).contains("malformed"));
}

/// REQ-NET-003, R12: for every choice type, every response of another type is rejected with an
/// explanation naming both.
#[test]
fn req_net_003_wrong_variant_for_every_type() {
    for t in ChoiceType::ALL {
        let r = manual(t, payload_for(t), ChoiceResponse::PassPriority);
        for resp in every_response() {
            if resp.choice_type() == t {
                continue;
            }
            let e = reject(&r, &resp);
            assert!(
                e.contains(t.as_str()) && e.contains(resp.wire_name()),
                "{e}"
            );
        }
    }
}

/// R12, REQ-NET-003: hostile responses never panic.
#[test]
fn req_net_003_hostile_responses_do_not_panic() {
    let big: Vec<ObjectId> = (0..200_000).map(|i| o(i % 7)).collect();
    let hostile = vec![
        ChoiceResponse::SubmitObjectSelection {
            objects: big.clone(),
        },
        ChoiceResponse::SubmitSet {
            objects: big.clone(),
        },
        ChoiceResponse::SubmitPregame {
            objects: big.clone(),
        },
        ChoiceResponse::KeepHand {
            bottom: big.clone(),
        },
        ChoiceResponse::SubmitOrdering {
            top: big.clone(),
            bottom: big.clone(),
            graveyard: big.clone(),
        },
        ChoiceResponse::SelectModes {
            modes: vec![u32::MAX; 100_000],
        },
        ChoiceResponse::OrderTriggers {
            order: vec![u32::MAX; 1000],
        },
        ChoiceResponse::SubmitDivision {
            assignments: vec![(ChoiceTarget::Object(o(1)), u64::MAX); 1000],
        },
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(i64::MIN),
        },
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text("x".repeat(100_000)),
        },
        ChoiceResponse::SubmitCardName {
            name: "\u{7f}".repeat(100_000),
        },
        ChoiceResponse::SelectTargets {
            targets: vec![vec![ChoiceTarget::Object(o(1)); 100]; 100],
        },
        ChoiceResponse::SubmitBlockers {
            blocks: vec![(o(3), big.clone())],
        },
        ChoiceResponse::SubmitAttackers {
            attacks: vec![(o(1), ChoiceTarget::Player(p(2))); 1000],
        },
        ChoiceResponse::PayCosts {
            auto_pay: false,
            methods: vec![(u32::MAX, big.clone())],
            mana_abilities: vec![],
        },
        ChoiceResponse::ConfigureCosts {
            alternative: Some(u32::MAX),
            additional: vec![u32::MAX; 10],
            x: Some(u64::MAX),
            symbol_options: vec![u32::MAX; 10],
            harmonize: Some(o(u64::MAX)),
        },
        ChoiceResponse::Shorten { at_index: u64::MAX },
        ChoiceResponse::SelectReplacement { index: u32::MAX },
    ];
    let mut requests: Vec<PendingChoiceRequest> = ChoiceType::ALL
        .into_iter()
        .map(|t| manual(t, payload_for(t), ChoiceResponse::PassPriority))
        .collect();
    requests.push(objects_req(5, 0, 5));
    requests.push(ask(
        ChoiceKind::Division,
        division(3, vec![(ChoiceTarget::Object(o(1)), 0)]),
    ));
    requests.push(ask(
        ChoiceKind::Order,
        resolved(ResolvedCandidates::Objects(objs(1..4)), 0, 0),
    ));
    for r in &requests {
        for resp in &hostile {
            if let Err(e) = r.validate(resp) {
                assert!(!e.is_empty());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Defaults and property tests (REQ-NET-003)
// ---------------------------------------------------------------------------------------------

fn bounded(rng: &mut Pcg64Dxsm, bound: u64) -> u64 {
    rng.next_bounded(bound).unwrap_or(0)
}

/// A random generated request of a random spec-derived kind (small domains).
fn random_request(rng: &mut Pcg64Dxsm) -> PendingChoiceRequest {
    let n = bounded(rng, 7);
    let min = bounded(rng, n + 1);
    let max = min + bounded(rng, 4);
    let pick = bounded(rng, 11);
    let labels = || ResolvedCandidates::Strings((0..n.max(1)).map(|i| format!("s{i}")).collect());
    let (kind, r) = match pick {
        0 => (
            ChoiceKind::Objects,
            resolved(ResolvedCandidates::Objects(objs(1..n + 1)), min, max),
        ),
        1 => (
            ChoiceKind::Mode,
            resolved(
                ResolvedCandidates::Strings((0..n).map(|i| format!("m{i}")).collect()),
                min,
                max,
            ),
        ),
        2 => (
            ChoiceKind::Number,
            resolved(ResolvedCandidates::None, min, max),
        ),
        3 => (ChoiceKind::Color, resolved(labels(), 1, 1)),
        4 => (ChoiceKind::CreatureType, resolved(labels(), 1, 1)),
        5 => (ChoiceKind::Pile, resolved(labels(), 1, 1)),
        6 => (
            ChoiceKind::Player,
            resolved(
                ResolvedCandidates::Players((1..=n.max(1)).map(p).collect()),
                1,
                1,
            ),
        ),
        7 => (ChoiceKind::YesNo, resolved(ResolvedCandidates::None, 1, 1)),
        8 => (
            ChoiceKind::Order,
            resolved(ResolvedCandidates::Objects(objs(1..n + 1)), 0, 0),
        ),
        9 => {
            let recips = (0..=n)
                .map(|i| (ChoiceTarget::Object(o(i + 1)), bounded(rng, 3)))
                .collect::<Vec<_>>();
            let floors: u64 = recips.iter().map(|(_, f)| *f).sum();
            (
                ChoiceKind::Division,
                division(floors + bounded(rng, 6), recips),
            )
        }
        _ => {
            let mut r = resolved(ResolvedCandidates::None, 1, 1);
            r.default = Some(ChoiceResponse::SubmitCardName {
                name: format!("test.name_{n}"),
            });
            (ChoiceKind::CardName, r)
        }
    };
    ask(kind, r)
}

/// An answer built from the payload domain, valid by construction.
fn random_valid_answer(rng: &mut Pcg64Dxsm, r: &PendingChoiceRequest) -> ChoiceResponse {
    match &r.payload {
        ChoicePayload::SelectObjectsFromZone { candidates } => {
            let mut c = candidates.clone();
            rng.shuffle(&mut c);
            let k = r.min + bounded(rng, r.max - r.min + 1);
            c.truncate(usize::try_from(k).unwrap_or(0));
            ChoiceResponse::SubmitObjectSelection { objects: c }
        }
        ChoicePayload::ChooseModes { labels, .. } => {
            let mut c: Vec<u32> = (0..labels.len())
                .map(|i| u32::try_from(i).unwrap_or(0))
                .collect();
            rng.shuffle(&mut c);
            let k = r.min + bounded(rng, r.max - r.min + 1);
            c.truncate(usize::try_from(k).unwrap_or(0));
            ChoiceResponse::SelectModes { modes: c }
        }
        ChoicePayload::ChooseOptionOrValue { allowed, .. } => match allowed {
            AllowedValues::Range => ChoiceResponse::SubmitChoiceValue {
                value: ChoiceValue::Number(
                    i64::try_from(r.min + bounded(rng, r.max - r.min + 1)).unwrap_or(0),
                ),
            },
            AllowedValues::Texts(t) => ChoiceResponse::SubmitChoiceValue {
                value: ChoiceValue::Text(rng.choose(t).cloned().unwrap_or_default()),
            },
        },
        ChoicePayload::ChoosePlayer { players } => ChoiceResponse::SubmitPlayer {
            player: rng.choose(players).copied().unwrap_or(p(0)),
        },
        ChoicePayload::ChooseOptionalTrigger { .. } => {
            if bounded(rng, 2) == 0 {
                ChoiceResponse::Yes
            } else {
                ChoiceResponse::No
            }
        }
        ChoicePayload::OrderCards { cards, .. } => {
            let mut c = cards.clone();
            rng.shuffle(&mut c);
            ChoiceResponse::SubmitOrdering {
                top: c,
                bottom: vec![],
                graveyard: vec![],
            }
        }
        ChoicePayload::DivideValue { total, recipients } => {
            let floors: u64 = recipients.iter().map(|(_, f)| *f).sum();
            let mut rem = total - floors;
            let mut out = Vec::new();
            for (i, (t, f)) in recipients.iter().enumerate() {
                let extra = if i + 1 == recipients.len() {
                    rem
                } else {
                    bounded(rng, rem + 1)
                };
                rem -= extra;
                out.push((*t, f + extra));
            }
            rng.shuffle(&mut out);
            ChoiceResponse::SubmitDivision { assignments: out }
        }
        _ => ChoiceResponse::SubmitCardName {
            name: format!("test.n{}", bounded(rng, 100)),
        },
    }
}

/// An independent oracle for the spec-derived types.
fn oracle(r: &PendingChoiceRequest, resp: &ChoiceResponse) -> bool {
    let distinct = |v: &[ObjectId]| v.iter().collect::<BTreeSet<_>>().len() == v.len();
    let in_count = |len: usize| (r.min..=r.max).contains(&(len as u64));
    match (&r.payload, resp) {
        (
            ChoicePayload::SelectObjectsFromZone { candidates },
            ChoiceResponse::SubmitObjectSelection { objects },
        ) => {
            in_count(objects.len())
                && distinct(objects)
                && objects.iter().all(|x| candidates.contains(x))
        }
        (ChoicePayload::ChooseModes { labels, .. }, ChoiceResponse::SelectModes { modes }) => {
            in_count(modes.len())
                && modes.iter().collect::<BTreeSet<_>>().len() == modes.len()
                && modes.iter().all(|m| (*m as usize) < labels.len())
        }
        (
            ChoicePayload::ChooseOptionOrValue { allowed, .. },
            ChoiceResponse::SubmitChoiceValue { value },
        ) => match (allowed, value) {
            (AllowedValues::Range, ChoiceValue::Number(v)) => {
                *v >= 0 && (r.min..=r.max).contains(&(*v as u64))
            }
            (AllowedValues::Texts(t), ChoiceValue::Text(s)) => t.contains(s),
            _ => false,
        },
        (ChoicePayload::ChoosePlayer { players }, ChoiceResponse::SubmitPlayer { player }) => {
            players.contains(player)
        }
        (ChoicePayload::ChooseOptionalTrigger { .. }, ChoiceResponse::Yes | ChoiceResponse::No) => {
            true
        }
        (
            ChoicePayload::OrderCards { cards, .. },
            ChoiceResponse::SubmitOrdering {
                top,
                bottom,
                graveyard,
            },
        ) => {
            let mut sorted = top.clone();
            sorted.sort();
            bottom.is_empty() && graveyard.is_empty() && &sorted == cards
        }
        (
            ChoicePayload::DivideValue { total, recipients },
            ChoiceResponse::SubmitDivision { assignments },
        ) => {
            let mut a = assignments.clone();
            a.sort();
            let mut targets: Vec<ChoiceTarget> = recipients.iter().map(|(t, _)| *t).collect();
            targets.sort();
            let got: Vec<ChoiceTarget> = a.iter().map(|(t, _)| *t).collect();
            let sum: u128 = a.iter().map(|(_, x)| u128::from(*x)).sum();
            got == targets
                && sum == u128::from(*total)
                && a.iter()
                    .all(|(t, x)| recipients.iter().any(|(rt, f)| rt == t && x >= f))
        }
        (ChoicePayload::ChooseCardName, ChoiceResponse::SubmitCardName { name }) => {
            !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
        }
        _ => false,
    }
}

/// Mutates a response a little (it may or may not stay valid).
fn mutate(rng: &mut Pcg64Dxsm, resp: &ChoiceResponse) -> ChoiceResponse {
    let bump = |rng: &mut Pcg64Dxsm, v: &mut Vec<ObjectId>| match bounded(rng, 3) {
        0 => {
            v.pop();
        }
        1 => v.push(o(bounded(rng, 9))),
        _ => {
            if let Some(x) = v.first().copied() {
                v.push(x);
            }
        }
    };
    let mut r = resp.clone();
    match &mut r {
        ChoiceResponse::SubmitObjectSelection { objects } => bump(rng, objects),
        ChoiceResponse::SelectModes { modes } => match bounded(rng, 3) {
            0 => {
                modes.pop();
            }
            1 => modes.push(u32::try_from(bounded(rng, 9)).unwrap_or(0)),
            _ => {
                if let Some(x) = modes.first().copied() {
                    modes.push(x);
                }
            }
        },
        ChoiceResponse::SubmitChoiceValue { value } => match value {
            ChoiceValue::Number(n) => {
                *n = n.wrapping_add(i64::try_from(bounded(rng, 5)).unwrap_or(0) - 2)
            }
            ChoiceValue::Text(s) => s.push('x'),
        },
        ChoiceResponse::SubmitPlayer { player } => *player = p(bounded(rng, 9)),
        ChoiceResponse::SubmitOrdering { top, bottom, .. } => {
            if bounded(rng, 2) == 0 {
                bump(rng, top);
            } else if let Some(x) = top.pop() {
                bottom.push(x);
            }
        }
        ChoiceResponse::SubmitDivision { assignments } => {
            if let Some(first) = assignments.first_mut() {
                first.1 = first.1.wrapping_add(bounded(rng, 3)).wrapping_sub(1);
            }
        }
        ChoiceResponse::SubmitCardName { name } => match bounded(rng, 3) {
            0 => name.clear(),
            1 => name.push('\u{1}'),
            _ => name.push('y'),
        },
        ChoiceResponse::Yes => r = ChoiceResponse::No,
        ChoiceResponse::No => r = ChoiceResponse::Accept,
        _ => {}
    }
    r
}

/// REQ-NET-003: every generated default validates (128 seeds of random specs).
#[test]
fn req_net_003_property_defaults_validate() {
    for seed in 0..128u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let r = random_request(&mut rng);
        assert_eq!(
            r.validate(&r.default_on_timeout),
            Ok(()),
            "seed {seed}: {r:?}"
        );
        assert!(oracle(&r, &r.default_on_timeout), "seed {seed}");
    }
}

/// REQ-NET-003: random valid answers are always accepted; mutated answers are accepted exactly
/// when the independent oracle says they are valid, and otherwise rejected with an explanation.
#[test]
fn req_net_003_property_valid_and_mutated_answers() {
    let mut accepted_mutations = 0;
    let mut rejected_mutations = 0;
    for seed in 0..256u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let r = random_request(&mut rng);
        for _ in 0..8 {
            let good = random_valid_answer(&mut rng, &r);
            assert!(oracle(&r, &good), "seed {seed}: {good:?}");
            assert_eq!(r.validate(&good), Ok(()), "seed {seed}: {good:?}");
            let bad = mutate(&mut rng, &good);
            match r.validate(&bad) {
                Ok(()) => {
                    assert!(oracle(&r, &bad), "seed {seed}: accepted invalid {bad:?}");
                    accepted_mutations += 1;
                }
                Err(e) => {
                    assert!(!e.is_empty());
                    assert!(
                        !oracle(&r, &bad),
                        "seed {seed}: rejected valid {bad:?}: {e}"
                    );
                    rejected_mutations += 1;
                }
            }
        }
    }
    assert!(accepted_mutations > 0 && rejected_mutations > 0);
}

/// REQ-NET-003: enumerated whole-answer options are each valid; building-block options are in
/// the payload domain.
#[test]
fn req_net_003_property_enumerated_items_match_domain() {
    for seed in 0..64u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let r = random_request(&mut rng);
        let ChoiceOptions::Enumerated(items) = &r.options else {
            assert_eq!(r.choice_type, ChoiceType::ChooseCardName);
            continue;
        };
        for item in items {
            let answer = match (item, &r.payload) {
                (ChoiceOption::Player(x), _) => ChoiceResponse::SubmitPlayer { player: *x },
                (ChoiceOption::Text(t), _) => ChoiceResponse::SubmitChoiceValue {
                    value: ChoiceValue::Text(t.clone()),
                },
                (ChoiceOption::Number(n), _) => ChoiceResponse::SubmitChoiceValue {
                    value: ChoiceValue::Number(*n),
                },
                (ChoiceOption::Yes, _) => ChoiceResponse::Yes,
                (ChoiceOption::No, _) => ChoiceResponse::No,
                (ChoiceOption::Object(x), ChoicePayload::SelectObjectsFromZone { candidates }) => {
                    assert!(candidates.contains(x));
                    continue;
                }
                (ChoiceOption::Object(x), ChoicePayload::OrderCards { cards, .. }) => {
                    assert!(cards.contains(x));
                    continue;
                }
                (ChoiceOption::Object(x), ChoicePayload::DivideValue { recipients, .. }) => {
                    assert!(
                        recipients
                            .iter()
                            .any(|(t, _)| *t == ChoiceTarget::Object(*x))
                    );
                    continue;
                }
                (ChoiceOption::Mode(m), ChoicePayload::ChooseModes { labels, .. }) => {
                    assert!((*m as usize) < labels.len());
                    continue;
                }
                other => panic!("unexpected item {other:?}"),
            };
            assert_eq!(r.validate(&answer), Ok(()), "seed {seed}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Session (REQ-NET-003, REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

/// A toy state: a value and a log.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Toy {
    value: i64,
    log: Vec<u64>,
}

impl Transactional for Toy {
    fn state_hash(&self) -> Digest {
        let mut bytes = encode_to_vec(&self.value);
        bytes.extend(encode_to_vec(&self.log));
        leaf_digest(&bytes)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

fn number_request(
    ctx: &ChoiceContext<'_>,
    min: u64,
    max: u64,
) -> Result<PendingChoiceRequest, Interrupt> {
    match generate_request(
        ctx.next_request_id(),
        &spec(ChoiceKind::Number),
        resolved(ResolvedCandidates::None, min, max),
    )? {
        Generated::Ask(r) => Ok(r),
        Generated::Random => Err(ChoiceError::Procedure("unexpected random".into()).into()),
    }
}

fn answer_number(r: ChoiceResponse) -> Result<i64, Interrupt> {
    match r {
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(n),
        } => Ok(n),
        other => Err(ChoiceError::Procedure(format!("unexpected {other:?}")).into()),
    }
}

/// A two-ask procedure over the toy state: mutate, ask, mutate, ask, mutate.
fn toy_procedure(s: &mut Toy, ctx: &mut ChoiceContext<'_>) -> Result<i64, Interrupt> {
    s.value += 100;
    s.log.push(1);
    let a = answer_number(ctx.ask(number_request(ctx, 0, 9)?)?)?;
    s.value += a;
    s.log.push(2);
    let b = answer_number(ctx.ask(number_request(ctx, 0, a.unsigned_abs() + 1)?)?)?;
    s.value *= b + 1;
    Ok(s.value)
}

type ToyProc = fn(&mut Toy, &mut ChoiceContext<'_>) -> Result<i64, Interrupt>;

fn toy_base() -> Toy {
    Toy {
        value: 1,
        log: vec![],
    }
}

fn num(n: i64) -> ChoiceResponse {
    ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Number(n),
    }
}

/// REQ-NET-003, REQ-CORE-001: a toy multi-ask run; the pending state equals the base; replay and
/// incremental submit agree.
#[test]
fn req_net_003_session_toy_multi_ask() {
    let mut s = ChoiceSession::start(toy_base(), toy_procedure as ToyProc).expect("start");
    let base_hash = toy_base().state_hash();
    assert_eq!(s.state_hash(), base_hash);
    assert_eq!(s.base(), &toy_base());
    let first = s.pending().expect("pending").clone();
    assert_eq!(first.request_id, RequestId::new(0));
    s.submit(RequestId::new(0), num(4)).expect("first answer");
    assert_eq!(s.state_hash(), base_hash);
    assert_eq!(s.base(), &toy_base());
    let second = s.pending().expect("pending").clone();
    assert_eq!(second.request_id, RequestId::new(1));
    assert_eq!(second.max, 5);
    s.submit(RequestId::new(1), num(2)).expect("second answer");
    match s.status() {
        Status::Done { state, output } => {
            assert_eq!(*output, (1 + 100 + 4) * 3);
            assert_eq!(state.value, *output);
            assert_eq!(state.log, vec![1, 2]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s.answers(), &[num(4), num(2)]);
    let replayed = ChoiceSession::replay(toy_base(), toy_procedure as ToyProc, &[num(4), num(2)])
        .expect("replay");
    assert_eq!(replayed.state_hash(), s.state_hash());
    assert_eq!(replayed.status(), s.status());
    assert!(format!("{s:?}").contains("ChoiceSession"));
}

/// A procedure over `GameState`: changes life and draws from the PRNG before and between asks.
fn game_procedure(state: &mut GameState, ctx: &mut ChoiceContext<'_>) -> Result<u64, Interrupt> {
    let drawn = state.prng_mut().next_u64();
    if let Some(mut pl) = state.player_mut(p(1)) {
        pl.set_life(20 - i64::try_from(drawn % 5).unwrap_or(0));
    }
    let objects = generate_request(
        ctx.next_request_id(),
        &spec(ChoiceKind::Objects),
        resolved(ResolvedCandidates::Objects(objs(1..6)), 1, 3),
    )?;
    let Generated::Ask(req) = objects else {
        return Err(ChoiceError::Procedure("random".into()).into());
    };
    let chosen = match ctx.ask(req)? {
        ChoiceResponse::SubmitObjectSelection { objects } => objects,
        other => return Err(ChoiceError::Procedure(format!("{other:?}")).into()),
    };
    let mut total = state.prng_mut().next_u64() % 1000;
    if let Some(mut pl) = state.player_mut(p(2)) {
        pl.set_life(i64::try_from(chosen.len()).unwrap_or(0));
    }
    let yes = generate_request(
        ctx.next_request_id(),
        &spec(ChoiceKind::YesNo),
        resolved(ResolvedCandidates::None, 1, 1),
    )?;
    let Generated::Ask(req) = yes else {
        return Err(ChoiceError::Procedure("random".into()).into());
    };
    if ctx.ask(req)? == ChoiceResponse::Yes {
        total += state.prng_mut().next_u64() % 1000;
        if let Some(mut pl) = state.player_mut(p(1)) {
            pl.set_life(1);
        }
    }
    Ok(total)
}

type GameProc = fn(&mut GameState, &mut ChoiceContext<'_>) -> Result<u64, Interrupt>;

fn game_base() -> GameState {
    GameState::new(0xC0FFEE, 2, 20).expect("state")
}

/// REQ-NET-003, REQ-CORE-001: over `GameState`, the same base and the same answers give the same
/// hash across 10 runs, incremental and replay; different answers give a different result.
#[test]
fn req_net_003_req_core_001_session_game_state_determinism() {
    let answers = vec![
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(2), o(5)],
        },
        ChoiceResponse::Yes,
    ];
    let mut hashes = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for _ in 0..10 {
        let mut s = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
        assert_eq!(s.state_hash(), game_base().state_hash());
        for a in &answers {
            let id = s.pending().expect("pending").request_id;
            s.submit(id, a.clone()).expect("submit");
            if s.pending().is_some() {
                assert_eq!(s.state_hash(), game_base().state_hash());
                assert_eq!(s.base(), &game_base());
            }
        }
        let Status::Done { state, output } = s.status() else {
            panic!("not done");
        };
        assert_eq!(state.player(p(2)).map(|x| x.life()), Some(2));
        assert_eq!(state.player(p(1)).map(|x| x.life()), Some(1));
        hashes.insert(s.state_hash());
        outputs.insert(*output);
        let r = ChoiceSession::replay(game_base(), game_procedure as GameProc, &answers)
            .expect("replay");
        hashes.insert(r.state_hash());
    }
    assert_eq!(hashes.len(), 1);
    assert_eq!(outputs.len(), 1);
    let other = ChoiceSession::replay(
        game_base(),
        game_procedure as GameProc,
        &[
            ChoiceResponse::SubmitObjectSelection {
                objects: vec![o(2)],
            },
            ChoiceResponse::No,
        ],
    )
    .expect("replay");
    assert!(!hashes.contains(&other.state_hash()));
    assert_ne!(other.state_hash(), game_base().state_hash());
}

type GameSession = ChoiceSession<GameState, u64, GameProc>;

fn snapshot(s: &GameSession) -> (Status<GameState, u64>, Vec<ChoiceResponse>, Digest) {
    (s.status().clone(), s.answers().to_vec(), s.state_hash())
}

/// REQ-NET-003: invalid, stale and after-done answers fail and change nothing.
#[test]
fn req_net_003_session_rejections_leave_session_unchanged() {
    let mut s = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
    s.submit(
        RequestId::new(0),
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(1)],
        },
    )
    .expect("submit");
    let before = snapshot(&s);
    let e = s
        .submit(RequestId::new(1), ChoiceResponse::Accept)
        .expect_err("invalid");
    match &e {
        ChoiceError::Invalid {
            request_id,
            explanation,
        } => {
            assert_eq!(*request_id, RequestId::new(1));
            assert!(!explanation.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(!e.to_string().is_empty());
    assert_eq!(snapshot(&s), before);
    assert_eq!(
        s.submit(RequestId::new(0), ChoiceResponse::Yes),
        Err(ChoiceError::StaleRequest {
            submitted: RequestId::new(0),
            pending: Some(RequestId::new(1))
        })
    );
    assert!(matches!(
        s.submit_default(RequestId::new(5)),
        Err(ChoiceError::StaleRequest { .. })
    ));
    assert_eq!(snapshot(&s), before);
    s.submit(RequestId::new(1), ChoiceResponse::No)
        .expect("submit");
    let done = snapshot(&s);
    assert_eq!(
        s.submit(RequestId::new(2), ChoiceResponse::No),
        Err(ChoiceError::NotPending)
    );
    assert_eq!(
        s.submit_default(RequestId::new(2)),
        Err(ChoiceError::NotPending)
    );
    assert_eq!(snapshot(&s), done);
    assert!(matches!(
        ChoiceSession::replay(
            game_base(),
            game_procedure as GameProc,
            &[ChoiceResponse::Yes]
        ),
        Err(ChoiceError::Invalid { .. })
    ));
    assert!(matches!(
        ChoiceSession::replay(
            game_base(),
            game_procedure as GameProc,
            &[
                ChoiceResponse::SubmitObjectSelection {
                    objects: vec![o(1)]
                },
                ChoiceResponse::No,
                ChoiceResponse::No
            ]
        ),
        Err(ChoiceError::NotPending)
    ));
}

/// REQ-NET-003: submit_default is exactly submit with the pending default (the caller's
/// timeout; no clock).
#[test]
fn req_net_003_session_submit_default() {
    let mut a = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
    let mut b = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
    while let Some(req) = a.pending().cloned() {
        a.submit_default(req.request_id).expect("default");
        b.submit(req.request_id, req.default_on_timeout.clone())
            .expect("explicit");
        assert_eq!(a.answers(), b.answers());
        assert_eq!(a.status(), b.status());
    }
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(
        a.answers(),
        &[
            ChoiceResponse::SubmitObjectSelection {
                objects: vec![o(1)]
            },
            ChoiceResponse::No
        ]
    );
}

/// REQ-NET-003: a procedure whose requests depend on a counter outside the state diverges on
/// re-execution; the session reports it and stays unchanged.
#[test]
fn req_net_003_session_divergence_detected() {
    let calls = Cell::new(0u64);
    let proc = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        calls.set(calls.get() + 1);
        let a = answer_number(ctx.ask(number_request(ctx, 0, calls.get())?)?)?;
        let b = answer_number(ctx.ask(number_request(ctx, 0, 3)?)?)?;
        Ok(a + b)
    };
    let mut s = ChoiceSession::start(toy_base(), proc).expect("start");
    let before = (s.status().clone(), s.answers().to_vec(), s.state_hash());
    assert_eq!(
        s.submit(RequestId::new(0), num(1)),
        Err(ChoiceError::Divergence { index: 0 })
    );
    assert_eq!(
        (s.status().clone(), s.answers().to_vec(), s.state_hash()),
        before
    );
    assert!(
        ChoiceError::Divergence { index: 0 }
            .to_string()
            .contains('0')
    );
}

/// REQ-NET-003: a procedure that stops asking for a recorded answer diverges.
#[test]
fn req_net_003_session_divergence_fewer_asks() {
    let calls = Cell::new(0u64);
    let proc = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        calls.set(calls.get() + 1);
        if calls.get() > 1 {
            return Ok(0);
        }
        answer_number(ctx.ask(number_request(ctx, 0, 3)?)?)
    };
    let mut s = ChoiceSession::start(toy_base(), proc).expect("start");
    assert_eq!(
        s.submit(RequestId::new(0), num(1)),
        Err(ChoiceError::Divergence { index: 0 })
    );
}

/// REQ-NET-003: request ids are positions; a wrong id is rejected; protocol misuse is reported.
#[test]
fn req_net_003_session_request_ids_and_misuse() {
    let wrong = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        let mut r = number_request(ctx, 0, 1)?;
        r.request_id = RequestId::new(7);
        answer_number(ctx.ask(r)?)
    };
    assert_eq!(
        ChoiceSession::start(toy_base(), wrong).map(|_| ()),
        Err(ChoiceError::RequestIdMismatch {
            expected: RequestId::new(0),
            got: RequestId::new(7)
        })
    );
    let swallow = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        let _ = ctx.ask(number_request(ctx, 0, 1)?);
        Ok(0)
    };
    assert!(matches!(
        ChoiceSession::start(toy_base(), swallow).map(|_| ()),
        Err(ChoiceError::Procedure(_))
    ));
    let twice = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        let _ = ctx.ask(number_request(ctx, 0, 1)?);
        let r = number_request(ctx, 0, 1)?;
        answer_number(ctx.ask(r)?)
    };
    assert!(matches!(
        ChoiceSession::start(toy_base(), twice).map(|_| ()),
        Err(ChoiceError::Procedure(_))
    ));
    let forged = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        Err(Interrupt::Pending(Box::new(number_request(ctx, 0, 1)?)))
    };
    assert!(matches!(
        ChoiceSession::start(toy_base(), forged).map(|_| ()),
        Err(ChoiceError::Procedure(_))
    ));
    let failing = |_: &mut Toy, _: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        Err(ChoiceError::Procedure("boom".into()).into())
    };
    assert_eq!(
        ChoiceSession::start(toy_base(), failing).map(|_| ()),
        Err(ChoiceError::Procedure("boom".into()))
    );
    let none = |s: &mut Toy, _: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        s.value = 42;
        Ok(1)
    };
    let s = ChoiceSession::start(toy_base(), none).expect("start");
    assert!(s.pending().is_none());
    assert_eq!(
        s.state_hash(),
        Toy {
            value: 42,
            log: vec![]
        }
        .state_hash()
    );
}

/// REQ-NET-003, R3: two sessions over clones of one base never interfere.
#[test]
fn req_net_003_session_independent_sessions() {
    let mut a = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
    let mut b = ChoiceSession::start(game_base(), game_procedure as GameProc).expect("start");
    a.submit(
        RequestId::new(0),
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(3)],
        },
    )
    .expect("a");
    assert_eq!(b.pending().map(|r| r.request_id), Some(RequestId::new(0)));
    assert!(b.answers().is_empty());
    b.submit(
        RequestId::new(0),
        ChoiceResponse::SubmitObjectSelection {
            objects: vec![o(1), o(2), o(4)],
        },
    )
    .expect("b");
    a.submit(RequestId::new(1), ChoiceResponse::Yes).expect("a");
    b.submit(RequestId::new(1), ChoiceResponse::Yes).expect("b");
    let Status::Done { state: sa, .. } = a.status() else {
        panic!("a not done")
    };
    let Status::Done { state: sb, .. } = b.status() else {
        panic!("b not done")
    };
    assert_eq!(sa.player(p(2)).map(|x| x.life()), Some(1));
    assert_eq!(sb.player(p(2)).map(|x| x.life()), Some(3));
    let a2 = ChoiceSession::replay(game_base(), game_procedure as GameProc, a.answers())
        .expect("replay");
    assert_eq!(a2.state_hash(), a.state_hash());
}

/// REQ-NET-003: errors and interrupts display distinctly; value types encode.
#[test]
fn req_net_003_error_display() {
    let errs = [
        ChoiceError::Invalid {
            request_id: RequestId::new(1),
            explanation: "x".into(),
        },
        ChoiceError::StaleRequest {
            submitted: RequestId::new(1),
            pending: None,
        },
        ChoiceError::StaleRequest {
            submitted: RequestId::new(1),
            pending: Some(RequestId::new(2)),
        },
        ChoiceError::NotPending,
        ChoiceError::Divergence { index: 3 },
        ChoiceError::Procedure("p".into()),
        ChoiceError::InvalidSpec("s".into()),
        ChoiceError::MissingCandidates,
        ChoiceError::Unsatisfiable("u".into()),
        ChoiceError::NoDefault,
        ChoiceError::InvalidDefault("d".into()),
        ChoiceError::RequestIdMismatch {
            expected: RequestId::new(0),
            got: RequestId::new(1),
        },
    ];
    let texts: BTreeSet<String> = errs.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), errs.len());
    let i: Interrupt = ChoiceError::NotPending.into();
    assert_eq!(i, Interrupt::Error(ChoiceError::NotPending));
    assert!(!i.to_string().is_empty());
    let pending = Interrupt::Pending(Box::new(objects_req(2, 0, 1)));
    assert!(pending.to_string().contains("SELECT_OBJECTS_FROM_ZONE"));
    assert_eq!(ChoiceTarget::Player(p(1)).to_string(), "player 1");
    let encodings: BTreeSet<Vec<u8>> = [
        encode_to_vec(&Generated::Random),
        encode_to_vec(&Generated::Ask(objects_req(2, 0, 1))),
        encode_to_vec(&ResolvedCandidates::None),
        encode_to_vec(&ResolvedCandidates::Objects(vec![])),
        encode_to_vec(&ResolvedCandidates::Players(vec![])),
        encode_to_vec(&ResolvedCandidates::Strings(vec![])),
    ]
    .into_iter()
    .collect();
    assert_eq!(encodings.len(), 6);
    assert!(!encode_to_vec(&division(1, vec![(ChoiceTarget::Player(p(1)), 0)])).is_empty());
}

fn exact_binomial(n: u128, k: u128) -> u128 {
    if k > n {
        return 0;
    }
    let mut c: u128 = 1;
    for i in 0..k {
        c = c * (n - i) / (i + 1);
    }
    c
}

/// REQ-NET-003: the enumerate/validator decision agrees with exact arithmetic for every small
/// OBJECTS, MODE, ORDER and DIVISION shape.
#[test]
fn req_net_003_threshold_matches_exact_counts() {
    let limit = u128::from(ENUMERATION_LIMIT);
    for n in 0..=22u64 {
        for min in 0..=n {
            for max in min..=n + 2 {
                let exact: u128 = (min..=max.min(n))
                    .map(|k| exact_binomial(u128::from(n), u128::from(k)))
                    .sum();
                let r = objects_req(n, min, max);
                assert_eq!(
                    is_enumerated(&r),
                    exact < limit,
                    "objects n={n} {min}..={max}"
                );
                let labels = ResolvedCandidates::Strings((0..n).map(|i| format!("m{i}")).collect());
                let m = ask(ChoiceKind::Mode, resolved(labels, min, max));
                assert_eq!(
                    is_enumerated(&m),
                    exact < limit,
                    "modes n={n} {min}..={max}"
                );
            }
        }
    }
    let mut fact: u128 = 1;
    for n in 0..=12u64 {
        if n > 0 {
            fact *= u128::from(n);
        }
        let r = ask(
            ChoiceKind::Order,
            resolved(ResolvedCandidates::Objects(objs(1..n + 1)), 0, 0),
        );
        assert_eq!(is_enumerated(&r), fact < limit, "order n={n}");
    }
    for recipients in 1..=6u64 {
        for rem in [0u64, 1, 5, 20, 99, 100, 1000, 9_998, 9_999, 10_000] {
            let exact =
                exact_binomial(u128::from(rem + recipients - 1), u128::from(recipients - 1));
            let recips = (1..=recipients)
                .map(|i| (ChoiceTarget::Object(o(i)), 1))
                .collect();
            let r = ask(ChoiceKind::Division, division(rem + recipients, recips));
            assert_eq!(
                is_enumerated(&r),
                exact < limit,
                "division r={recipients} rem={rem}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 1 (PR #177): request well-formedness at ask, payment-method eligibility,
// card-name errors from the raising procedure
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003: `ask` rejects a request whose default does not validate (a `CHOOSE_PLAYER`
/// request defaulting to `YES`); the session never publishes it.
#[test]
fn req_net_003_ask_rejects_invalid_default() {
    let proc = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        let Generated::Ask(mut r) = generate_request(
            ctx.next_request_id(),
            &spec(ChoiceKind::Player),
            resolved(ResolvedCandidates::Players(vec![p(1), p(2)]), 1, 1),
        )?
        else {
            return Err(ChoiceError::Procedure("random".into()).into());
        };
        r.default_on_timeout = ChoiceResponse::Yes;
        ctx.ask(r)?;
        Ok(0)
    };
    match ChoiceSession::start(toy_base(), proc).map(|_| ()) {
        Err(ChoiceError::InvalidDefault(e)) => assert!(!e.trim().is_empty()),
        other => panic!("expected InvalidDefault, got {other:?}"),
    }
}

/// REQ-NET-003: `ask` rejects a validator request whose contract bounds differ from the request
/// bounds (tightened without updating the contract); a consistent tightened request is accepted.
#[test]
fn req_net_003_ask_rejects_stale_validator_bounds() {
    fn tightened(
        ctx: &ChoiceContext<'_>,
        min: u64,
        max: u64,
        sync: bool,
    ) -> Result<PendingChoiceRequest, Interrupt> {
        let mut r = number_request(ctx, 0, 19_999)?;
        assert!(!is_enumerated(&r));
        r.min = min;
        r.max = max;
        r.default_on_timeout = num(i64::try_from(min).unwrap_or(i64::MAX));
        if sync && let ChoiceOptions::Validator(c) = &mut r.options {
            c.min = min;
            c.max = max;
        }
        Ok(r)
    }
    let max_stale = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        answer_number(ctx.ask(tightened(ctx, 0, 9_999, false)?)?)
    };
    match ChoiceSession::start(toy_base(), max_stale).map(|_| ()) {
        Err(ChoiceError::Procedure(e)) => {
            assert!(e.contains("0..19999") && e.contains("0..9999"), "{e}");
        }
        other => panic!("expected Procedure, got {other:?}"),
    }
    let min_stale = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        answer_number(ctx.ask(tightened(ctx, 5, 19_999, false)?)?)
    };
    assert!(matches!(
        ChoiceSession::start(toy_base(), min_stale).map(|_| ()),
        Err(ChoiceError::Procedure(_))
    ));
    let synced = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        answer_number(ctx.ask(tightened(ctx, 5, 9_999, true)?)?)
    };
    let mut s = ChoiceSession::start(toy_base(), synced).expect("consistent request accepted");
    let r = s.pending().expect("pending").clone();
    assert_eq!((r.min, r.max), (5, 9_999));
    reject(&r, &num(10_000));
    s.submit(RequestId::new(0), num(9_999)).expect("in range");
    assert!(s.pending().is_none());
}

/// REQ-NET-003, REQ-CORE-001: a procedure whose second ask is ill-formed fails `submit` and
/// leaves the session entirely unchanged.
#[test]
fn req_net_003_submit_ill_formed_second_ask_leaves_session_unchanged() {
    let proc = |s: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        s.value += 1;
        let a = answer_number(ctx.ask(number_request(ctx, 0, 9)?)?)?;
        s.value += a;
        let mut r = number_request(ctx, 0, 9)?;
        r.default_on_timeout = ChoiceResponse::No;
        answer_number(ctx.ask(r)?)
    };
    let mut s = ChoiceSession::start(toy_base(), proc).expect("start");
    let status = s.status().clone();
    let hash = s.state_hash();
    assert!(matches!(
        s.submit(RequestId::new(0), num(3)),
        Err(ChoiceError::InvalidDefault(_))
    ));
    assert_eq!(s.status(), &status);
    assert!(s.answers().is_empty());
    assert_eq!(s.state_hash(), hash);
    assert!(matches!(
        s.submit_default(RequestId::new(0)),
        Err(ChoiceError::InvalidDefault(_))
    ));
    assert_eq!(s.status(), &status);
    assert!(s.answers().is_empty());
    assert_eq!(s.state_hash(), hash);
}

/// REQ-NET-003, R12: a payment method may use only the objects eligible for that method, each
/// at most once, at an index in range.
#[test]
fn req_net_003_pay_costs_method_eligibility() {
    let r = manual(
        ChoiceType::PayManaAndCosts,
        ChoicePayload::PayManaAndCosts {
            locked_total: "3".into(),
            methods: vec![("m0".into(), vec![o(1), o(2)]), ("m1".into(), vec![o(3)])],
            mana_abilities: vec![],
            auto_pay: false,
            optional: false,
            cancellable: false,
        },
        ChoiceResponse::PayCosts {
            auto_pay: false,
            methods: vec![],
            mana_abilities: vec![],
        },
    );
    let pay = |methods: Vec<(u32, Vec<ObjectId>)>| ChoiceResponse::PayCosts {
        auto_pay: false,
        methods,
        mana_abilities: vec![],
    };
    assert_eq!(r.validate(&pay(vec![(0, vec![o(2), o(1)])])), Ok(()));
    assert_eq!(
        r.validate(&pay(vec![(0, vec![o(1)]), (1, vec![o(3)])])),
        Ok(())
    );
    // Outside every list.
    let e = reject(&r, &pay(vec![(0, vec![o(9)])]));
    assert!(e.contains("object 9"), "{e}");
    // Duplicate.
    reject(&r, &pay(vec![(0, vec![o(1), o(1)])]));
    // Eligible only for the other method.
    reject(&r, &pay(vec![(0, vec![o(3)])]));
    reject(&r, &pay(vec![(1, vec![o(1)])]));
    // Out-of-range method index.
    reject(&r, &pay(vec![(2, vec![])]));
    reject(&r, &pay(vec![(u32::MAX, vec![o(1)])]));
    // The payload change is part of the canonical encoding.
    let mut other = r.clone();
    if let ChoicePayload::PayManaAndCosts { methods, .. } = &mut other.payload {
        methods[1].1 = vec![o(4)];
    }
    assert_ne!(other.digest(), r.digest());
}

/// REQ-NET-003: an unknown card name is the raising procedure's explained error; the original
/// request stays pending for a valid retry, and a known name proceeds.
#[test]
fn req_net_003_card_name_unknown_is_procedure_error() {
    let proc = |_: &mut Toy, ctx: &mut ChoiceContext<'_>| -> Result<i64, Interrupt> {
        let id = ctx.next_request_id();
        let mut resolved_name = resolved(ResolvedCandidates::None, 1, 1);
        resolved_name.default = Some(ChoiceResponse::SubmitCardName {
            name: "test.vanilla_1_1".into(),
        });
        let Generated::Ask(r) = generate_request(id, &spec(ChoiceKind::CardName), resolved_name)?
        else {
            return Err(ChoiceError::Procedure("random".into()).into());
        };
        match ctx.ask(r)? {
            ChoiceResponse::SubmitCardName { name } if name.starts_with("test.") => {
                Ok(i64::try_from(name.len()).unwrap_or(0))
            }
            ChoiceResponse::SubmitCardName { .. } => Err(ChoiceError::Invalid {
                request_id: id,
                explanation: "the card name is not in the card database".into(),
            }
            .into()),
            other => Err(ChoiceError::Procedure(format!("unexpected {other:?}")).into()),
        }
    };
    let mut s = ChoiceSession::start(toy_base(), proc).expect("start");
    let pending = s.pending().cloned().expect("pending");
    // Structurally valid, unknown to the (synthetic) database.
    let unknown = ChoiceResponse::SubmitCardName {
        name: "not.a_card".into(),
    };
    assert_eq!(pending.validate(&unknown), Ok(()));
    match s.submit(RequestId::new(0), unknown) {
        Err(ChoiceError::Invalid {
            request_id,
            explanation,
        }) => {
            assert_eq!(request_id, RequestId::new(0));
            assert!(!explanation.trim().is_empty());
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert_eq!(s.pending(), Some(&pending));
    assert!(s.answers().is_empty());
    s.submit(
        RequestId::new(0),
        ChoiceResponse::SubmitCardName {
            name: "test.flier_2_2".into(),
        },
    )
    .expect("known name");
    assert!(matches!(s.status(), Status::Done { output: 14, .. }));
}
