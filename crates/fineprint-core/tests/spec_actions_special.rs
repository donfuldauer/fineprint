// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the special-action registry and the SPECIAL_ACTION actions, including
//! turning a face-down permanent face up (REQ-DSL-013, REQ-ARCH-002; CR 116.2a-116.2k, 116.2m,
//! 116.3, 117.1c, 708.7, 702.37e, 702.168d, 701.40b, 701.40g, 701.58b, 731.2g, 101.2).
//! Black-box: written from the work-item brief only. Every name is invented (R5).

// The brief promises Clone (not Copy) on the action types; keep `.clone()` so the tests do
// not depend on an extra derive.
#![allow(clippy::clone_on_copy)]

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_core::actions::*;
use fineprint_core::events::{Phase, Step};
use fineprint_dsl::ability::RuleKind;
use serde_json::json;
use support::*;

/// The registry table of the brief: (id, cr, timing), in order.
fn table() -> Vec<(&'static str, &'static str, SpecialTiming)> {
    use SpecialTiming::*;
    vec![
        ("TURN_FACE_UP", "CR 116.2b", AnyPriority),
        ("END_EFFECT", "CR 116.2c", AnyPriority),
        ("IGNORE_STATIC_EFFECT", "CR 116.2d", AnyPriority),
        ("DISCARD_AS_INSTANT", "CR 116.2e", AnyPriority),
        ("SUSPEND", "CR 116.2f", AnyPriorityIfCouldBeginToCast),
        ("COMPANION", "CR 116.2g", SorceryTiming),
        ("FORETELL", "CR 116.2h", OwnTurn),
        ("ROLL_PLANAR_DIE", "CR 116.2i", SorceryTiming),
        ("TURN_CONSPIRACY_FACE_UP", "CR 116.2j", AnyPriority),
        ("PLOT", "CR 116.2k", OwnTurnStackEmpty),
        ("UNLOCK", "CR 116.2m", SorceryTiming),
    ]
}

/// REQ-DSL-013: the standard registry lists exactly the eleven kinds of the table, in order,
/// each with its CR rule and timing (CR 116.2a-116.2k, 116.2m: playing a land is not a registry
/// kind).
#[test]
fn req_dsl_013_registry_table() {
    // CR 116.2b-116.2k, CR 116.2m
    let reg = SpecialActionRegistry::standard();
    let got: Vec<(&str, &str, SpecialTiming)> = reg
        .kinds()
        .iter()
        .map(|k| (k.id, k.cr, k.timing.clone()))
        .collect();
    assert_eq!(got, table());
    for (id, cr, timing) in table() {
        let k = reg.get(id).unwrap_or_else(|| panic!("{id} missing"));
        assert_eq!(k, &SpecialActionKind { id, cr, timing }, "{id}");
    }
    // CR 116.2a: playing a land is its own action, not a registry kind.
    assert!(reg.get("PLAY_LAND").is_none());
    assert!(reg.get("x").is_none());
    assert!(reg.get("").is_none());
    assert!(reg.get("turn_face_up").is_none());
    assert_eq!(KIND_IDS.len(), reg.kinds().len());
}

/// A situation for P1: (own turn, main phase, stack empty).
fn situation(own: bool, main: bool, empty: bool) -> FakeHost {
    let (phase, step) = if main {
        (Phase::PostcombatMain, None)
    } else {
        (Phase::Combat, Some(Step::DeclareAttackers))
    };
    FakeHost::new()
        .active(Some(if own { P1 } else { P2 }))
        .phase(phase, step)
        .stack(empty)
}

/// What the timing group requires in a situation (None: holds).
fn timing_requirement(
    t: &SpecialTiming,
    own: bool,
    main: bool,
    empty: bool,
    could: bool,
) -> Option<TimingRequirement> {
    match t {
        SpecialTiming::AnyPriority => None,
        SpecialTiming::SorceryTiming => {
            (!(own && main && empty)).then_some(TimingRequirement::SorceryTiming)
        }
        SpecialTiming::OwnTurnStackEmpty => {
            (!(own && empty)).then_some(TimingRequirement::OwnTurnStackEmpty)
        }
        SpecialTiming::OwnTurn => (!own).then_some(TimingRequirement::OwnTurn),
        SpecialTiming::AnyPriorityIfCouldBeginToCast => {
            (!could).then_some(TimingRequirement::CouldBeginToCast)
        }
    }
}

/// REQ-DSL-013: each kind is offered exactly when its timing group holds, in all eight
/// situations (own/opponent turn x main/other phase x empty/non-empty stack), with
/// could_begin_to_cast true and false; otherwise the matching Timing reason (CR 116.2b-116.2k,
/// 116.2m, 117.1c, 307.5).
#[test]
fn req_dsl_013_special_kinds_timing_groups() {
    // CR 116.2b-116.2k, CR 116.2m, CR 117.1c
    for (id, _cr, timing) in table() {
        for own in [true, false] {
            for main in [true, false] {
                for empty in [true, false] {
                    for could in [true, false] {
                        let mut h = situation(own, main, empty);
                        let resp = if id == "TURN_FACE_UP" {
                            // TURN_FACE_UP comes from the face-up candidates (CR 116.2b).
                            let c = face_up(10, FaceDownOrigin::Morph { has_cost: true });
                            h.face_up.push(c.clone());
                            face_up_resp(&c)
                        } else {
                            let c = SpecialCandidate {
                                could_begin_to_cast: could,
                                ..special(id, 10)
                            };
                            h.specials.push(c.clone());
                            special_resp(&c)
                        };
                        let need = if id == "TURN_FACE_UP" {
                            None
                        } else {
                            timing_requirement(&timing, own, main, empty, could)
                        };
                        match need {
                            None => {
                                let basis = assert_legal(&h, P1, &resp);
                                assert!(rule_strings_ok(&basis), "{id}: {basis:?}");
                            }
                            Some(req) => assert_illegal(&h, P1, &resp, &[Reason::Timing(req)]),
                        }
                    }
                }
            }
        }
    }
}

/// REQ-DSL-013: AnyPriority kinds are offered even on the opponent's turn with a non-empty stack
/// outside a main phase; FORETELL on its own turn with a non-empty stack; PLOT on its own turn
/// in combat with an empty stack (CR 116.2c, 116.2h, 116.2k).
#[test]
fn req_dsl_013_special_timing_edges() {
    // CR 116.2c, CR 116.2d, CR 116.2e, CR 116.2j
    for id in [
        "END_EFFECT",
        "IGNORE_STATIC_EFFECT",
        "DISCARD_AS_INSTANT",
        "TURN_CONSPIRACY_FACE_UP",
    ] {
        let mut h = situation(false, false, false).phase(Phase::Ending, Some(Step::End));
        h.specials.push(special(id, 10));
        assert_legal(&h, P1, &special_resp(&special(id, 10)));
    }
    // CR 116.2h
    let mut h = situation(true, false, false);
    h.specials.push(special("FORETELL", 10));
    assert_legal(&h, P1, &special_resp(&special("FORETELL", 10)));
    // CR 116.2k
    let mut h = situation(true, false, true);
    h.specials.push(special("PLOT", 10));
    assert_legal(&h, P1, &special_resp(&special("PLOT", 10)));
    let mut h = situation(true, true, false);
    h.specials.push(special("PLOT", 10));
    assert_illegal(
        &h,
        P1,
        &special_resp(&special("PLOT", 10)),
        &[Reason::Timing(TimingRequirement::OwnTurnStackEmpty)],
    );
    // CR 116.2f: suspend on the opponent's turn with a non-empty stack, if it could be cast.
    let mut h = situation(false, false, false);
    h.specials.push(special("SUSPEND", 10));
    assert_legal(&h, P1, &special_resp(&special("SUSPEND", 10)));
}

/// REQ-DSL-013: available false -> NotAvailable (CR 116.2c, 116.2g: the action must still be
/// allowed).
#[test]
fn req_dsl_013_special_not_available() {
    // CR 116.2c, CR 116.2g
    for id in KIND_IDS.iter().skip(1) {
        let c = SpecialCandidate {
            available: false,
            ..special(id, 10)
        };
        let mut h = FakeHost::new();
        h.specials.push(c.clone());
        assert_illegal(&h, P1, &special_resp(&c), &[Reason::NotAvailable]);
    }
}

/// REQ-DSL-013: an unknown kind is never offered: UnknownSpecialKind (CR 116.2).
#[test]
fn req_dsl_013_unknown_special_kind() {
    // CR 116.2
    for kind in ["test.unknown_kind", "", "PLAY_LAND", "plot"] {
        let c = special(kind, 10);
        let mut h = FakeHost::new();
        h.specials.push(c.clone());
        assert!(offered(&h, P1).is_empty(), "{kind}");
        let rs = reasons(&h, P1, &special_resp(&c));
        assert_eq!(rs, vec![Reason::UnknownSpecialKind], "{kind}");
    }
}

/// REQ-ARCH-002: a SPECIAL_ACTION prohibition without "kind" blocks every kind (including
/// TURN_FACE_UP); with a "kind" only that kind (CR 101.2, 116.2).
#[test]
fn req_arch_002_special_prohibition_all_or_one_kind() {
    // CR 101.2, CR 116.2
    let mut base = FakeHost::new();
    for (i, id) in KIND_IDS.iter().enumerate().skip(1) {
        base.specials.push(special(id, 10 + i as u64));
    }
    base.face_up
        .push(face_up(10, FaceDownOrigin::Disguise { has_cost: true }));
    let all = offered(&base, P1);
    assert_eq!(all.len(), 11, "{all:?}");

    let h = base.clone().with_rules(&[r(
        5,
        RuleKind::Prohibition,
        json!({"action": "SPECIAL_ACTION"}),
        &[P1],
        1,
    )]);
    assert!(offered(&h, P1).is_empty());
    for resp in &all {
        assert_prohibited(&h, P1, resp, 5);
    }

    for (i, id) in KIND_IDS.iter().enumerate() {
        let h = base.clone().with_rules(&[r(
            6,
            RuleKind::Prohibition,
            json!({"action": "SPECIAL_ACTION", "kind": id}),
            &[P1],
            1,
        )]);
        let now = offered(&h, P1);
        for resp in &all {
            let is_kind = matches!(
                resp,
                fineprint_core::choice::ChoiceResponse::SpecialAction { kind, .. } if kind == id
            );
            assert_eq!(now.contains(resp), !is_kind, "{id} ({i}): {resp:?}");
            if is_kind {
                assert_prohibited(&h, P1, resp, 6);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// T6: TURN_FACE_UP eligibility
// ---------------------------------------------------------------------------------------------

fn manifest(creature_card: bool, has_mana_cost: bool, instant_or_sorcery: bool) -> FaceDownOrigin {
    FaceDownOrigin::Manifest {
        creature_card,
        has_mana_cost,
        instant_or_sorcery,
    }
}

fn cloak(creature_card: bool, has_mana_cost: bool, instant_or_sorcery: bool) -> FaceDownOrigin {
    FaceDownOrigin::Cloak {
        creature_card,
        has_mana_cost,
        instant_or_sorcery,
    }
}

fn check_face_up(c: TurnFaceUpCandidate, expected: &[Reason]) {
    let mut h = FakeHost::new();
    h.face_up.push(c.clone());
    if expected.is_empty() {
        assert_legal(&h, P1, &face_up_resp(&c));
    } else {
        assert_illegal(&h, P1, &face_up_resp(&c), expected);
    }
}

/// REQ-DSL-013: morph and disguise permanents turn face up only with a cost
/// (CR 116.2b, 702.37e, 702.168d).
#[test]
fn req_dsl_013_face_up_morph_and_disguise() {
    // CR 702.37e, CR 702.168d
    check_face_up(face_up(10, FaceDownOrigin::Morph { has_cost: true }), &[]);
    check_face_up(
        face_up(10, FaceDownOrigin::Disguise { has_cost: true }),
        &[],
    );
    check_face_up(
        face_up(10, FaceDownOrigin::Morph { has_cost: false }),
        &[Reason::FaceUp(FaceUpDenial::NoTurnFaceUpCost)],
    );
    check_face_up(
        face_up(10, FaceDownOrigin::Disguise { has_cost: false }),
        &[Reason::FaceUp(FaceUpDenial::NoTurnFaceUpCost)],
    );
}

/// REQ-DSL-013: manifested and cloaked permanents turn face up only if represented by a creature
/// card with a mana cost, never an instant or sorcery card (CR 701.40b, 701.40g, 701.58b).
#[test]
fn req_dsl_013_face_up_manifest_and_cloak() {
    // CR 701.40b, CR 701.40g, CR 701.58b
    for make in [manifest, cloak] {
        check_face_up(face_up(10, make(true, true, false)), &[]);
        check_face_up(
            face_up(10, make(false, true, false)),
            &[Reason::FaceUp(FaceUpDenial::NotCreatureCard)],
        );
        check_face_up(
            face_up(10, make(true, false, false)),
            &[Reason::FaceUp(FaceUpDenial::NoManaCost)],
        );
        check_face_up(
            face_up(10, make(true, true, true)),
            &[Reason::FaceUp(FaceUpDenial::InstantOrSorcery)],
        );
    }
}

/// REQ-DSL-013: a merged permanent containing an instant or sorcery card can't be turned face
/// up; one off the battlefield can't either; Other follows its flag (CR 731.2g, 708.7).
#[test]
fn req_dsl_013_face_up_merged_battlefield_other() {
    // CR 731.2g
    check_face_up(
        TurnFaceUpCandidate {
            merged_instant_or_sorcery: true,
            ..face_up(10, FaceDownOrigin::Morph { has_cost: true })
        },
        &[Reason::FaceUp(FaceUpDenial::InstantOrSorcery)],
    );
    // CR 708.7
    check_face_up(
        TurnFaceUpCandidate {
            on_battlefield: false,
            ..face_up(10, FaceDownOrigin::Morph { has_cost: true })
        },
        &[Reason::FaceUp(FaceUpDenial::NotOnBattlefield)],
    );
    check_face_up(
        face_up(
            10,
            FaceDownOrigin::Other {
                can_turn_face_up: false,
            },
        ),
        &[Reason::FaceUp(FaceUpDenial::NotAllowed)],
    );
    check_face_up(
        face_up(
            10,
            FaceDownOrigin::Other {
                can_turn_face_up: true,
            },
        ),
        &[],
    );
}

/// REQ-DSL-013: turning face up is offered any time its controller has priority, including the
/// opponent's turn with a non-empty stack (CR 116.2b, 116.3).
#[test]
fn req_dsl_013_face_up_any_priority() {
    // CR 116.2b, CR 116.3
    for own in [true, false] {
        for main in [true, false] {
            for empty in [true, false] {
                let mut h = situation(own, main, empty);
                let c = face_up(10, FaceDownOrigin::Morph { has_cost: true });
                h.face_up.push(c.clone());
                assert_legal(&h, P1, &face_up_resp(&c));
            }
        }
    }
}

/// REQ-DSL-013: several denials are all listed (CR 708.7, 701.40b, 731.2g).
#[test]
fn req_dsl_013_face_up_reasons_accumulate() {
    // CR 708.7, CR 701.40b, CR 731.2g
    let c = TurnFaceUpCandidate {
        on_battlefield: false,
        merged_instant_or_sorcery: true,
        ..face_up(10, manifest(false, false, true))
    };
    let mut h = FakeHost::new();
    h.face_up.push(c.clone());
    assert!(offered(&h, P1).is_empty());
    let rs = reasons(&h, P1, &face_up_resp(&c));
    let expected = [
        Reason::FaceUp(FaceUpDenial::NotOnBattlefield),
        Reason::FaceUp(FaceUpDenial::InstantOrSorcery),
        Reason::FaceUp(FaceUpDenial::NotCreatureCard),
        Reason::FaceUp(FaceUpDenial::NoManaCost),
    ];
    for e in &expected {
        assert!(rs.contains(e), "{e:?} missing from {rs:?}");
    }
    for got in &rs {
        assert!(expected.contains(got), "unexpected {got:?} in {rs:?}");
    }

    // Cloak with no creature card and no mana cost; plus a prohibition, named first.
    let c = face_up(11, cloak(false, false, false));
    let mut h = FakeHost::new().with_rules(&[r(
        7,
        RuleKind::Prohibition,
        json!({"action": "SPECIAL_ACTION", "kind": "TURN_FACE_UP"}),
        &[P1],
        1,
    )]);
    h.face_up.push(c.clone());
    let rs = reasons(&h, P1, &face_up_resp(&c));
    assert_eq!(rs.first(), Some(&Reason::Prohibited { by: eid(7) }));
    assert!(rs.contains(&Reason::FaceUp(FaceUpDenial::NotCreatureCard)));
    assert!(rs.contains(&Reason::FaceUp(FaceUpDenial::NoManaCost)));
    assert_eq!(rs.len(), 3, "{rs:?}");
}

/// REQ-DSL-013: special actions are ordered by (kind, object), ascending (CR 116.2).
#[test]
fn req_dsl_013_special_order() {
    // CR 116.2
    let mut h = FakeHost::new();
    h.specials = vec![
        special("PLOT", 3),
        special("END_EFFECT", 9),
        special("END_EFFECT", 2),
    ];
    h.face_up = vec![face_up(1, FaceDownOrigin::Morph { has_cost: true })];
    assert_eq!(
        offered(&h, P1),
        vec![
            special_resp(&special("END_EFFECT", 2)),
            special_resp(&special("END_EFFECT", 9)),
            special_resp(&special("PLOT", 3)),
            face_up_resp(&face_up(1, FaceDownOrigin::Morph { has_cost: true })),
        ]
    );
}
