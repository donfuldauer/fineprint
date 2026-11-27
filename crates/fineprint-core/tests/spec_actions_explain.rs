// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the determinism and order of the legal actions, the PRIORITY_ACTION payload
//! and the consistency of `explain` (REQ-DSL-013, REQ-ARCH-002, R3; CR 117.1, 101.2).
//! Black-box: written from the work-item brief only. Every name is invented (R5).

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_core::actions::*;
use fineprint_core::choice::{ChoicePayload, ChoiceResponse};
use fineprint_dsl::ability::{ActivationTiming, RuleKind};
use fineprint_dsl::expr::Zone;
use serde_json::json;
use support::*;

/// A host with candidates of every class, legal and illegal ones, with duplicates.
fn rich_host() -> FakeHost {
    let mut h = FakeHost::new().with_rules(&[
        r(
            1,
            RuleKind::CastPermission,
            json!({"action": "CAST", "zone": "GRAVEYARD"}),
            &[P1],
            1,
        ),
        r(
            2,
            RuleKind::PlayPermission,
            json!({"action": "PLAY_LAND", "object": 31}),
            &[P1],
            2,
        ),
        r(
            3,
            RuleKind::Prohibition,
            json!({"action": "ACTIVATE", "object": 52}),
            &[P1],
            3,
        ),
        r(
            4,
            RuleKind::Prohibition,
            json!({"action": "SPECIAL_ACTION", "kind": "PLOT"}),
            &[P1],
            4,
        ),
        r(
            5,
            RuleKind::Prohibition,
            json!({"action": "CAST", "object": 44}),
            &[P1, P2],
            5,
        ),
    ]);
    h.lands = vec![
        land(30, Zone::Hand),
        land(31, Zone::Graveyard),
        land(32, Zone::Graveyard),
        LandCandidate {
            face_index: 1,
            ..land(30, Zone::Hand)
        },
        land(30, Zone::Hand),
    ];
    h.casts = vec![
        cast(40, Zone::Hand),
        instant(41, Zone::Graveyard),
        cast(42, Zone::Exile),
        CastCandidate {
            cast_mode: "test.alt".to_owned(),
            ..cast(40, Zone::Hand)
        },
        CastCandidate {
            is_land: true,
            ..cast(43, Zone::Hand)
        },
        instant(44, Zone::Hand),
        cast(40, Zone::Hand),
    ];
    h.abilities = vec![
        ability(50, "test.b"),
        ability(50, "test.a"),
        timed(51, "test.a", ActivationTiming::CombatOnly),
        ability(52, "test.a"),
        AbilityCandidate {
            is_mana_ability: true,
            ..timed(53, "test.m", ActivationTiming::OpponentTurn)
        },
        ability(50, "test.b"),
    ];
    h.specials = vec![
        special("PLOT", 60),
        special("FORETELL", 61),
        special("END_EFFECT", 62),
        special("test.unknown_kind", 63),
        SpecialCandidate {
            available: false,
            ..special("UNLOCK", 64)
        },
        special("END_EFFECT", 62),
    ];
    h.face_up = vec![
        face_up(70, FaceDownOrigin::Morph { has_cost: true }),
        face_up(71, FaceDownOrigin::Morph { has_cost: false }),
        face_up(70, FaceDownOrigin::Morph { has_cost: true }),
    ];
    h
}

/// Every response the host's candidates map to.
fn candidate_responses(h: &FakeHost) -> Vec<ChoiceResponse> {
    let mut v: Vec<ChoiceResponse> = Vec::new();
    v.extend(h.lands.iter().map(land_resp));
    v.extend(h.casts.iter().map(cast_resp));
    v.extend(h.abilities.iter().map(ability_resp));
    v.extend(h.specials.iter().map(special_resp));
    v.extend(h.face_up.iter().map(face_up_resp));
    v
}

fn class_rank(r: &ChoiceResponse) -> u8 {
    match r {
        ChoiceResponse::PlayLand { .. } => 0,
        ChoiceResponse::InitiateCast { .. } => 1,
        ChoiceResponse::Activate { .. } => 2,
        ChoiceResponse::SpecialAction { .. } => 3,
        other => panic!("not an action: {other:?}"),
    }
}

/// The sort key of the brief: class, then (object, face_index) / (object, face_index,
/// cast_mode) / (object, ability_id) / (kind, object).
fn order_key(r: &ChoiceResponse) -> (u8, String, u64, u32, String) {
    match r {
        ChoiceResponse::PlayLand { object, face_index } => {
            (0, String::new(), object.get(), *face_index, String::new())
        }
        ChoiceResponse::InitiateCast {
            object,
            face_index,
            cast_mode,
        } => (
            1,
            String::new(),
            object.get(),
            *face_index,
            cast_mode.clone(),
        ),
        ChoiceResponse::Activate { object, ability_id } => {
            (2, String::new(), object.get(), 0, ability_id.clone())
        }
        ChoiceResponse::SpecialAction { kind, object } => {
            (3, kind.clone(), object.get(), 0, String::new())
        }
        other => panic!("not an action: {other:?}"),
    }
}

/// REQ-DSL-013 (R3): shuffling every candidate vector over 32 seeds never changes the legal
/// actions, for both players and in several situations (CR 117.1).
#[test]
fn req_dsl_013_candidate_order_does_not_matter() {
    // CR 117.1
    let bases = [
        rich_host(),
        rich_host().active(Some(P2)),
        rich_host().stack(false),
        rich_host().played(P1, 1),
    ];
    for base in &bases {
        let want1 = legal_actions(base, P1);
        let want2 = legal_actions(base, P2);
        assert!(!want1.is_empty());
        for seed in 0..32u64 {
            let mut rng = Rng::new(seed);
            let mut h = base.clone();
            rng.shuffle(&mut h.lands);
            rng.shuffle(&mut h.casts);
            rng.shuffle(&mut h.abilities);
            rng.shuffle(&mut h.specials);
            rng.shuffle(&mut h.face_up);
            assert_eq!(legal_actions(&h, P1), want1, "seed {seed}");
            assert_eq!(legal_actions(&h, P2), want2, "seed {seed}");
            assert_eq!(
                priority_payload(&h, P1, false),
                priority_payload(base, P1, false)
            );
        }
    }
}

/// REQ-DSL-013: output order is PlayLand, InitiateCast, Activate, SpecialAction, ascending within
/// each class; exact duplicates appear once (CR 117.1).
#[test]
fn req_dsl_013_output_order_and_dedup() {
    // CR 117.1
    let h = rich_host();
    let got = offered(&h, P1);
    let mut sorted = got.clone();
    sorted.sort_by_key(order_key);
    sorted.dedup();
    assert_eq!(got, sorted);
    let ranks: Vec<u8> = got.iter().map(class_rank).collect();
    assert!(ranks.contains(&0) && ranks.contains(&1) && ranks.contains(&2) && ranks.contains(&3));
    // The expected set for P1 on its own main phase with an empty stack.
    let want = vec![
        play_land(30),
        ChoiceResponse::PlayLand {
            object: oid(30),
            face_index: 1,
        },
        play_land(31),
        // "test.alt" sorts before "test.normal".
        ChoiceResponse::InitiateCast {
            object: oid(40),
            face_index: 0,
            cast_mode: "test.alt".to_owned(),
        },
        cast_resp(&cast(40, Zone::Hand)),
        cast_resp(&instant(41, Zone::Graveyard)),
        ability_resp(&ability(50, "test.a")),
        ability_resp(&ability(50, "test.b")),
        ability_resp(&ability(53, "test.m")),
        special_resp(&special("END_EFFECT", 62)),
        special_resp(&special("FORETELL", 61)),
        face_up_resp(&face_up(70, FaceDownOrigin::Morph { has_cost: true })),
    ];
    assert_eq!(got, want);
}

/// REQ-DSL-013: priority_actions is the responses of legal_actions; priority_payload wraps them
/// with can_pass true and the shortcut flag as given (CR 117.1).
#[test]
fn req_dsl_013_priority_payload_shape() {
    // CR 117.1
    for h in [rich_host(), rich_host().active(Some(P2)), FakeHost::new()] {
        for p in [P1, P2] {
            let responses: Vec<ChoiceResponse> = legal_actions(&h, p)
                .into_iter()
                .map(|a| a.response)
                .collect();
            assert_eq!(priority_actions(&h, p), responses);
            assert_eq!(
                priority_payload(&h, p, false),
                ChoicePayload::PriorityAction {
                    can_pass: true,
                    can_propose_shortcut: false,
                    actions: priority_actions(&h, p),
                }
            );
            assert_eq!(
                priority_payload(&h, p, true),
                ChoicePayload::PriorityAction {
                    can_pass: true,
                    can_propose_shortcut: true,
                    actions: responses,
                }
            );
        }
    }
}

/// REQ-DSL-013: explain(PassPriority) is Legal with an empty basis, for both players, in any
/// situation (CR 117.1).
#[test]
fn req_dsl_013_explain_pass_priority() {
    // CR 117.1
    for h in [
        rich_host(),
        rich_host().active(Some(P2)).stack(false),
        FakeHost::new(),
    ] {
        for p in [P1, P2] {
            assert_eq!(
                explain(&h, p, &ChoiceResponse::PassPriority),
                Legality::Legal { basis: vec![] }
            );
        }
    }
}

/// REQ-DSL-013: every offered action is Legal with a non-empty basis equal to the offered one;
/// every candidate not offered is Illegal with non-empty reasons, Prohibited first when a
/// prohibition applies (CR 101.2).
#[test]
fn req_dsl_013_explain_matches_offered() {
    // CR 101.2
    for h in [
        rich_host(),
        rich_host().active(Some(P2)),
        rich_host().stack(false),
        rich_host().played(P1, 1),
    ] {
        for p in [P1, P2] {
            let actions = legal_actions(&h, p);
            for a in &actions {
                assert!(!a.basis.is_empty(), "{a:?}");
                assert!(rule_strings_ok(&a.basis), "{a:?}");
                assert_eq!(
                    explain(&h, p, &a.response),
                    Legality::Legal {
                        basis: a.basis.clone()
                    }
                );
            }
            let offered: Vec<ChoiceResponse> = actions.into_iter().map(|a| a.response).collect();
            for resp in candidate_responses(&h) {
                if offered.contains(&resp) {
                    continue;
                }
                let rs = reasons(&h, p, &resp);
                assert!(!rs.is_empty(), "{resp:?}");
                assert!(!rs.contains(&Reason::NoSuchAction), "{resp:?}: {rs:?}");
                for (i, x) in rs.iter().enumerate() {
                    if matches!(x, Reason::Prohibited { .. }) {
                        assert_eq!(i, 0, "Prohibited must be first: {rs:?}");
                    }
                }
            }
        }
    }
    // Specific prohibited candidates of the rich host name their prohibitions.
    let h = rich_host();
    assert_prohibited(&h, P1, &ability_resp(&ability(52, "test.a")), 3);
    assert_prohibited(&h, P1, &special_resp(&special("PLOT", 60)), 4);
    assert_prohibited(&h, P1, &cast_resp(&instant(44, Zone::Hand)), 5);
    assert_prohibited(&h, P2, &cast_resp(&instant(44, Zone::Hand)), 5);
}

/// REQ-DSL-013: a response that matches no candidate, and a response that is not a priority
/// action, are Illegal with exactly [NoSuchAction] (CR 117.1).
#[test]
fn req_dsl_013_explain_no_such_action() {
    // CR 117.1
    let h = rich_host();
    let strangers = [
        play_land(999),
        ChoiceResponse::PlayLand {
            object: oid(30),
            face_index: 7,
        },
        ChoiceResponse::InitiateCast {
            object: oid(40),
            face_index: 0,
            cast_mode: "test.no_such_mode".to_owned(),
        },
        ChoiceResponse::Activate {
            object: oid(50),
            ability_id: "test.no_such_ability".to_owned(),
        },
        ChoiceResponse::SpecialAction {
            kind: "END_EFFECT".to_owned(),
            object: oid(999),
        },
        ChoiceResponse::SpecialAction {
            kind: "TURN_FACE_UP".to_owned(),
            object: oid(60),
        },
        // A cast response for a land candidate's object, and vice versa.
        cast_resp(&cast(30, Zone::Hand)),
        play_land(40),
        ChoiceResponse::SelectModes { modes: vec![0] },
        ChoiceResponse::SelectModes { modes: vec![] },
    ];
    for p in [P1, P2] {
        for resp in &strangers {
            assert_eq!(
                explain(&h, p, resp),
                Legality::Illegal {
                    reasons: vec![Reason::NoSuchAction]
                },
                "{resp:?}"
            );
            assert!(!is_offered(&h, p, resp));
        }
    }
    // An empty host: every action is NoSuchAction.
    let empty = FakeHost::new();
    assert_eq!(
        explain(&empty, P1, &play_land(1)),
        Legality::Illegal {
            reasons: vec![Reason::NoSuchAction]
        }
    );
    assert!(legal_actions(&empty, P1).is_empty());
}
