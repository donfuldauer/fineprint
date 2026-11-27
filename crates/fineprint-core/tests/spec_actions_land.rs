// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for land plays offered by the legal-action generator (REQ-DSL-013, REQ-ARCH-002;
//! CR 116.2a, 305.1, 305.2, 305.2a, 305.2b, 305.3, 305.9, 101.2). Black-box: written from the
//! work-item brief only. Every name is invented (R5).

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_core::actions::*;
use fineprint_core::events::{Phase, Step};
use fineprint_dsl::ability::RuleKind;
use fineprint_dsl::expr::Zone;
use serde_json::json;
use support::*;

fn hand_land_host() -> FakeHost {
    let mut h = FakeHost::new();
    h.lands.push(land(10, Zone::Hand));
    h
}

fn extra(id: u64, count: Option<i64>) -> R {
    let params = match count {
        Some(n) => json!({ "count": n }),
        None => json!({}),
    };
    r(id, RuleKind::AdditionalLandPlay, params, &[P1], id)
}

/// REQ-DSL-013: a land in hand is offered on its owner's turn in both main phases with an
/// empty stack; the basis is Rule "CR 305.1" (CR 116.2a, 305.1).
#[test]
fn req_dsl_013_land_offered_in_both_main_phases() {
    // CR 116.2a, CR 305.1
    for phase in [Phase::PrecombatMain, Phase::PostcombatMain] {
        let h = hand_land_host().phase(phase, None);
        let basis = assert_legal(&h, P1, &play_land(10));
        assert!(
            basis.contains(&Allowance::Rule { cr: "CR 305.1" }),
            "{phase:?}: {basis:?}"
        );
        assert_eq!(offered(&h, P1), vec![play_land(10)]);
    }
}

/// REQ-DSL-013: no land play on the opponent's turn: Timing(OwnTurn) only (CR 305.3).
#[test]
fn req_dsl_013_land_not_on_opponents_turn() {
    // CR 305.3
    let h = hand_land_host().active(Some(P2));
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::Timing(TimingRequirement::OwnTurn)],
    );
    // Not even in the opponent's main phase with an empty stack, nor any other phase.
    for phase in [
        Phase::Beginning,
        Phase::PrecombatMain,
        Phase::Combat,
        Phase::PostcombatMain,
        Phase::Ending,
    ] {
        let h = hand_land_host().active(Some(P2)).phase(phase, None);
        assert_illegal(
            &h,
            P1,
            &play_land(10),
            &[Reason::Timing(TimingRequirement::OwnTurn)],
        );
    }
    // No active player at all: not P1's turn.
    let h = hand_land_host().active(None);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::Timing(TimingRequirement::OwnTurn)],
    );
}

/// REQ-DSL-013: on its own turn outside a main phase, or with a non-empty stack, the land is
/// refused with Timing(SorceryTiming) (CR 116.2a, 305.1).
#[test]
fn req_dsl_013_land_needs_main_phase_and_empty_stack() {
    // CR 116.2a, CR 305.1
    let cases = [
        (Phase::Beginning, Some(Step::Upkeep), true),
        (Phase::Combat, Some(Step::DeclareAttackers), true),
        (Phase::Combat, Some(Step::BeginningOfCombat), true),
        (Phase::Ending, Some(Step::End), true),
        (Phase::PrecombatMain, None, false),
        (Phase::PostcombatMain, None, false),
    ];
    for (phase, step, empty) in cases {
        let h = hand_land_host().phase(phase, step).stack(empty);
        assert_illegal(
            &h,
            P1,
            &play_land(10),
            &[Reason::Timing(TimingRequirement::SorceryTiming)],
        );
    }
}

/// REQ-DSL-013: one land per turn by default; after one, LandPlaysExhausted{1, 1}
/// (CR 305.2, 305.2a, 305.2b).
#[test]
fn req_dsl_013_land_plays_exhausted_by_default() {
    // CR 305.2, CR 305.2a, CR 305.2b
    let h = hand_land_host().played(P1, 1);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 1,
            played: 1,
        }],
    );
    let h = hand_land_host().played(P1, 3);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 1,
            played: 3,
        }],
    );
    // The other player's count does not matter.
    let h = hand_land_host().played(P2, 5);
    assert_legal(&h, P1, &play_land(10));
}

/// REQ-DSL-013, REQ-ARCH-002: one ADDITIONAL_LAND_PLAY {"count": 1} allows a second land; its
/// basis names that effect; a third is refused (CR 305.2, 305.2a, 305.2b).
#[test]
fn req_dsl_013_additional_land_play() {
    // CR 305.2, CR 305.2a
    let h = hand_land_host()
        .with_rules(&[extra(4, Some(1))])
        .played(P1, 1);
    let basis = assert_legal(&h, P1, &play_land(10));
    assert!(has_effect(&basis, 4), "{basis:?}");
    // CR 305.2b
    let h = hand_land_host()
        .with_rules(&[extra(4, Some(1))])
        .played(P1, 2);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 2,
            played: 2,
        }],
    );
}

/// REQ-DSL-013: an absent "count" means 1, several entries add up, and every applicable entry is
/// named in the basis once lands have been played (CR 305.2, 305.2a).
#[test]
fn req_dsl_013_additional_land_play_counts_add_up() {
    // CR 305.2, CR 305.2a
    let items = [extra(4, None), extra(5, Some(2))];
    for played in 1..=3 {
        let h = hand_land_host().with_rules(&items).played(P1, played);
        let basis = assert_legal(&h, P1, &play_land(10));
        assert!(has_effect(&basis, 4) && has_effect(&basis, 5), "{basis:?}");
    }
    let h = hand_land_host().with_rules(&items).played(P1, 4);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 4,
            played: 4,
        }],
    );
    // An entry for the other player gives P1 nothing.
    let h = hand_land_host()
        .with_rules(&[r(
            6,
            RuleKind::AdditionalLandPlay,
            json!({"count": 3}),
            &[P2],
            1,
        )])
        .played(P1, 1);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 1,
            played: 1,
        }],
    );
}

/// REQ-ARCH-002: an additional land play and a "can't play lands" prohibition: never offered,
/// whatever the timestamps; Prohibited first, then LandPlaysExhausted{allowed: 0}
/// (CR 101.2 and its example, CR 305.2b).
#[test]
fn req_arch_002_additional_land_play_loses_to_prohibition() {
    // CR 101.2, CR 305.2b
    for (pt, et) in [(1, 9), (9, 1)] {
        for played in 0..=2 {
            let h = hand_land_host()
                .with_rules(&[
                    r(
                        3,
                        RuleKind::Prohibition,
                        json!({"action": "PLAY_LAND"}),
                        &[P1],
                        pt,
                    ),
                    r(
                        4,
                        RuleKind::AdditionalLandPlay,
                        json!({"count": 1}),
                        &[P1],
                        et,
                    ),
                ])
                .played(P1, played);
            assert_illegal(
                &h,
                P1,
                &play_land(10),
                &[
                    Reason::Prohibited { by: eid(3) },
                    Reason::LandPlaysExhausted { allowed: 0, played },
                ],
            );
            assert!(offered(&h, P1).is_empty());
        }
    }
}

/// REQ-ARCH-002, REQ-DSL-013: a land in a graveyard needs a PLAY_PERMISSION ("PLAY_LAND", with
/// or without "zone") or the candidate's own permission; the basis names it (CR 305.1, 601.3).
#[test]
fn req_arch_002_land_from_graveyard_needs_permission() {
    // CR 305.1
    let mut base = FakeHost::new();
    base.lands.push(land(10, Zone::Graveyard));
    assert_illegal(&base, P1, &play_land(10), &[Reason::NoPermission]);

    for params in [
        json!({"action": "PLAY_LAND"}),
        json!({"action": "PLAY_LAND", "zone": "GRAVEYARD"}),
    ] {
        let h = base
            .clone()
            .with_rules(&[r(3, RuleKind::PlayPermission, params, &[P1], 1)]);
        let basis = assert_legal(&h, P1, &play_land(10));
        assert!(has_effect(&basis, 3), "{basis:?}");
    }

    // A permission for another action, for the other player, or for another zone does not help.
    for item in [
        r(
            3,
            RuleKind::PlayPermission,
            json!({"action": "CAST"}),
            &[P1],
            1,
        ),
        r(
            3,
            RuleKind::PlayPermission,
            json!({"action": "PLAY_LAND"}),
            &[P2],
            1,
        ),
        r(
            3,
            RuleKind::PlayPermission,
            json!({"action": "PLAY_LAND", "zone": "EXILE"}),
            &[P1],
            1,
        ),
    ] {
        let h = base.clone().with_rules(&[item]);
        assert_illegal(&h, P1, &play_land(10), &[Reason::NoPermission]);
    }

    // The candidate's own permission.
    let mut h = base.clone();
    h.lands[0].own_permission = Some("test.own_play_permission".to_owned());
    let basis = assert_legal(&h, P1, &play_land(10));
    assert!(
        basis.contains(&Allowance::Own {
            label: "test.own_play_permission".to_owned()
        }),
        "{basis:?}"
    );

    // A permission does not lift the land-play count (CR 305.2b).
    let h = base
        .with_rules(&[r(
            3,
            RuleKind::PlayPermission,
            json!({"action": "PLAY_LAND"}),
            &[P1],
            1,
        )])
        .played(P1, 1);
    assert_illegal(
        &h,
        P1,
        &play_land(10),
        &[Reason::LandPlaysExhausted {
            allowed: 1,
            played: 1,
        }],
    );
}

/// REQ-DSL-013: a cast candidate that is a land is never offered as INITIATE_CAST (IsLand); a
/// land candidate for the same object is offered as PLAY_LAND (CR 305.9).
#[test]
fn req_dsl_013_land_is_never_cast() {
    // CR 305.9
    let mut h = FakeHost::new();
    let c = CastCandidate {
        is_land: true,
        ..cast(10, Zone::Hand)
    };
    h.casts.push(c.clone());
    assert_illegal(&h, P1, &cast_resp(&c), &[Reason::IsLand]);
    h.lands.push(land(10, Zone::Hand));
    assert_eq!(offered(&h, P1), vec![play_land(10)]);
    // Even with instant timing, a cast permission and at any time (CR 305.9).
    let c2 = CastCandidate {
        is_land: true,
        instant_timing: true,
        ..cast(11, Zone::Graveyard)
    };
    let h = FakeHost::new().with_rules(&[r(
        3,
        RuleKind::CastPermission,
        json!({"action": "CAST"}),
        &[P1],
        1,
    )]);
    let mut h = h;
    h.casts.push(c2.clone());
    let rs = reasons(&h, P1, &cast_resp(&c2));
    assert!(rs.contains(&Reason::IsLand), "{rs:?}");
    assert!(offered(&h, P1).is_empty());
}

/// REQ-DSL-013: several lands are offered in ascending (object, face_index) order, with each
/// face its own action (CR 305.1, 712.8b).
#[test]
fn req_dsl_013_land_faces_and_order() {
    // CR 305.1
    let mut h = FakeHost::new();
    h.lands = vec![
        LandCandidate {
            face_index: 1,
            ..land(12, Zone::Hand)
        },
        land(12, Zone::Hand),
        land(3, Zone::Hand),
    ];
    assert_eq!(
        offered(&h, P1),
        vec![play_land(3), play_land(12), ChoiceResponseExt::play(12, 1),]
    );
}

/// A small helper for PLAY_LAND responses with a face.
struct ChoiceResponseExt;

impl ChoiceResponseExt {
    fn play(n: u64, face: u32) -> fineprint_core::choice::ChoiceResponse {
        fineprint_core::choice::ChoiceResponse::PlayLand {
            object: oid(n),
            face_index: face,
        }
    }
}
