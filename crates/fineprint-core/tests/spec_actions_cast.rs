// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the INITIATE_CAST actions offered by the legal-action generator
//! (REQ-DSL-013, REQ-ARCH-002; CR 117.1a, 601.3, 601.3b-d, 601.2b, 712.8b, 712.8c, 709.3a,
//! 101.2, 307.5). Black-box: written from the work-item brief only. Every name is invented (R5).

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_core::actions::*;
use fineprint_core::choice::ChoiceResponse;
use fineprint_core::events::{Phase, Step};
use fineprint_dsl::ability::RuleKind;
use fineprint_dsl::expr::Zone;
use serde_json::json;
use support::*;

/// The situations that are not sorcery timing for P1 (CR 117.1a, 307.5).
fn not_sorcery_timing() -> Vec<FakeHost> {
    vec![
        FakeHost::new().active(Some(P2)),
        FakeHost::new().stack(false),
        FakeHost::new().phase(Phase::Combat, Some(Step::DeclareBlockers)),
        FakeHost::new().phase(Phase::Beginning, Some(Step::Upkeep)),
        FakeHost::new().phase(Phase::Ending, Some(Step::End)),
        FakeHost::new()
            .active(Some(P2))
            .phase(Phase::Combat, Some(Step::CombatDamage))
            .stack(false),
    ]
}

/// The situations that are sorcery timing for P1.
fn sorcery_timing() -> Vec<FakeHost> {
    vec![
        FakeHost::new(),
        FakeHost::new().phase(Phase::PostcombatMain, None),
    ]
}

/// REQ-DSL-013: a non-instant spell from hand is offered at sorcery timing only; otherwise
/// Timing(SorceryTiming) (CR 117.1a, 307.5).
#[test]
fn req_dsl_013_noninstant_needs_sorcery_timing() {
    // CR 117.1a, CR 307.5
    let c = cast(10, Zone::Hand);
    for mut h in sorcery_timing() {
        h.casts.push(c.clone());
        let basis = assert_legal(&h, P1, &cast_resp(&c));
        assert!(has_rule(&basis), "{basis:?}");
        assert!(rule_strings_ok(&basis));
    }
    for mut h in not_sorcery_timing() {
        h.casts.push(c.clone());
        assert_illegal(
            &h,
            P1,
            &cast_resp(&c),
            &[Reason::Timing(TimingRequirement::SorceryTiming)],
        );
    }
}

/// REQ-DSL-013: an instant-timing spell from hand is offered any time its caster has priority
/// (CR 117.1a).
#[test]
fn req_dsl_013_instant_any_priority() {
    // CR 117.1a
    let c = instant(10, Zone::Hand);
    for mut h in sorcery_timing().into_iter().chain(not_sorcery_timing()) {
        h.casts.push(c.clone());
        let basis = assert_legal(&h, P1, &cast_resp(&c));
        assert!(rule_strings_ok(&basis));
    }
}

/// REQ-ARCH-002, REQ-DSL-013: a TIMING_PERMISSION ("action": "CAST") lets a non-instant spell be
/// cast outside sorcery timing; its basis names the effect (CR 601.3b, 601.3c, 601.3d).
#[test]
fn req_arch_002_timing_permission_grants_flash() {
    // CR 601.3b, CR 601.3c, CR 601.3d
    let c = cast(10, Zone::Hand);
    let perm = r(
        5,
        RuleKind::TimingPermission,
        json!({"action": "CAST"}),
        &[P1],
        3,
    );
    for h in not_sorcery_timing() {
        let mut h = h.with_rules(std::slice::from_ref(&perm));
        h.casts.push(c.clone());
        let basis = assert_legal(&h, P1, &cast_resp(&c));
        assert!(has_effect(&basis, 5), "{basis:?}");
    }
    // For the other player, or for another action, it does not apply.
    for item in [
        r(
            5,
            RuleKind::TimingPermission,
            json!({"action": "CAST"}),
            &[P2],
            3,
        ),
        r(
            5,
            RuleKind::TimingPermission,
            json!({"action": "ACTIVATE"}),
            &[P1],
            3,
        ),
        r(
            5,
            RuleKind::TimingPermission,
            json!({"action": "CAST", "object": 11}),
            &[P1],
            3,
        ),
        r(
            5,
            RuleKind::CastPermission,
            json!({"action": "CAST"}),
            &[P1],
            3,
        ),
    ] {
        let mut h = FakeHost::new().active(Some(P2)).with_rules(&[item]);
        h.casts.push(c.clone());
        assert_illegal(
            &h,
            P1,
            &cast_resp(&c),
            &[Reason::Timing(TimingRequirement::SorceryTiming)],
        );
    }
}

/// REQ-ARCH-002: a CAST prohibition beats a TIMING_PERMISSION in both timestamp orders
/// (CR 101.2, 601.3).
#[test]
fn req_arch_002_cast_prohibition_beats_timing_permission() {
    // CR 101.2, CR 601.3
    let c = cast(10, Zone::Hand);
    for (pt, tt) in [(1, 9), (9, 1)] {
        let mut h = FakeHost::new().active(Some(P2)).with_rules(&[
            r(
                4,
                RuleKind::Prohibition,
                json!({"action": "CAST"}),
                &[P1],
                pt,
            ),
            r(
                5,
                RuleKind::TimingPermission,
                json!({"action": "CAST"}),
                &[P1],
                tt,
            ),
        ]);
        h.casts.push(c.clone());
        assert_prohibited(&h, P1, &cast_resp(&c), 4);
        assert!(offered(&h, P1).is_empty());
    }
}

/// REQ-ARCH-002: a spell in a graveyard is offered only with a CAST_PERMISSION or the
/// candidate's own permission, named in the basis (CR 601.3).
#[test]
fn req_arch_002_cast_from_graveyard_needs_permission() {
    // CR 601.3
    let c = cast(10, Zone::Graveyard);
    let mut base = FakeHost::new();
    base.casts.push(c.clone());
    assert_illegal(&base, P1, &cast_resp(&c), &[Reason::NoPermission]);

    for params in [
        json!({"action": "CAST"}),
        json!({"action": "CAST", "zone": "GRAVEYARD"}),
        json!({"action": "CAST", "object": 10}),
    ] {
        let h = base
            .clone()
            .with_rules(&[r(3, RuleKind::CastPermission, params, &[P1], 1)]);
        assert!(has_effect(&assert_legal(&h, P1, &cast_resp(&c)), 3));
    }
    for item in [
        r(
            3,
            RuleKind::CastPermission,
            json!({"action": "CAST"}),
            &[P2],
            1,
        ),
        r(
            3,
            RuleKind::CastPermission,
            json!({"action": "PLAY_LAND"}),
            &[P1],
            1,
        ),
        r(
            3,
            RuleKind::PlayPermission,
            json!({"action": "CAST"}),
            &[P1],
            1,
        ),
        r(
            3,
            RuleKind::CastPermission,
            json!({"action": "CAST", "zone": "HAND"}),
            &[P1],
            1,
        ),
    ] {
        let h = base.clone().with_rules(&[item]);
        assert_illegal(&h, P1, &cast_resp(&c), &[Reason::NoPermission]);
    }

    let mut h = base.clone();
    h.casts[0].own_permission = Some("test.own_cast_permission".to_owned());
    let basis = assert_legal(&h, P1, &cast_resp(&c));
    assert!(basis.contains(&Allowance::Own {
        label: "test.own_cast_permission".to_owned()
    }));

    // A permission does not change the timing of a non-instant (CR 117.1a).
    let h = base
        .with_rules(&[r(
            3,
            RuleKind::CastPermission,
            json!({"action": "CAST"}),
            &[P1],
            1,
        )])
        .stack(false);
    assert_illegal(
        &h,
        P1,
        &cast_resp(&c),
        &[Reason::Timing(TimingRequirement::SorceryTiming)],
    );
}

/// REQ-DSL-013: each (object, face_index, cast_mode) is its own action, evaluated separately,
/// in ascending order (CR 712.8b, 712.8c, 709.3a, 601.2b).
#[test]
fn req_dsl_013_faces_and_cast_modes_are_separate_actions() {
    // CR 712.8b, CR 712.8c, CR 709.3a, CR 601.2b
    let mut h = FakeHost::new();
    for face in [1, 0] {
        for mode in ["test.mode_b", "test.mode_a"] {
            h.casts.push(CastCandidate {
                face_index: face,
                cast_mode: mode.to_owned(),
                ..cast(10, Zone::Hand)
            });
        }
    }
    let want: Vec<ChoiceResponse> = [
        (0, "test.mode_a"),
        (0, "test.mode_b"),
        (1, "test.mode_a"),
        (1, "test.mode_b"),
    ]
    .iter()
    .map(|&(f, m)| ChoiceResponse::InitiateCast {
        object: oid(10),
        face_index: f,
        cast_mode: m.to_owned(),
    })
    .collect();
    assert_eq!(offered(&h, P1), want);

    // Only the chosen face is evaluated: face 1 has instant timing, face 0 does not
    // (CR 712.8c, 709.3a).
    let mut h = FakeHost::new().stack(false);
    h.casts.push(cast(10, Zone::Hand));
    h.casts.push(CastCandidate {
        face_index: 1,
        ..instant(10, Zone::Hand)
    });
    let face1 = ChoiceResponse::InitiateCast {
        object: oid(10),
        face_index: 1,
        cast_mode: "test.normal".to_owned(),
    };
    assert_eq!(offered(&h, P1), vec![face1]);
    assert_illegal(
        &h,
        P1,
        &cast_resp(&cast(10, Zone::Hand)),
        &[Reason::Timing(TimingRequirement::SorceryTiming)],
    );
}

/// REQ-DSL-013: failing candidates report every failing check, Prohibited first, then in
/// declaration order (CR 101.2, 601.3, 117.1a).
#[test]
fn req_dsl_013_cast_reasons_accumulate() {
    // CR 101.2, CR 601.3, CR 117.1a
    let c = cast(10, Zone::Exile);
    let mut h = FakeHost::new().active(Some(P2)).with_rules(&[r(
        4,
        RuleKind::Prohibition,
        json!({"action": "CAST"}),
        &[P1],
        1,
    )]);
    h.casts.push(c.clone());
    assert_illegal(
        &h,
        P1,
        &cast_resp(&c),
        &[
            Reason::Prohibited { by: eid(4) },
            Reason::NoPermission,
            Reason::Timing(TimingRequirement::SorceryTiming),
        ],
    );
    // Without the prohibition.
    let mut h = FakeHost::new().active(Some(P2));
    h.casts.push(c.clone());
    assert_illegal(
        &h,
        P1,
        &cast_resp(&c),
        &[
            Reason::NoPermission,
            Reason::Timing(TimingRequirement::SorceryTiming),
        ],
    );
}
