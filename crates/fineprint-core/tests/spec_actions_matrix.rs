// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the permission/prohibition matrix of the legal-action generator
//! (REQ-ARCH-002, REQ-DSL-013; CR 101.2, 601.3, 602.5, 116.2) and for malformed rule-entry
//! params (R12). Black-box: written from the work-item brief only. Every name is invented (R5).

// The brief promises Clone (not Copy) on the action types; keep `.clone()` so the tests do
// not depend on an extra derive.
#![allow(clippy::clone_on_copy)]

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_base::ids::PlayerId;
use fineprint_core::actions::*;
use fineprint_core::choice::ChoiceResponse;
use fineprint_dsl::ability::RuleKind;
use fineprint_dsl::expr::Zone;
use serde_json::json;
use support::*;

// ---------------------------------------------------------------------------------------------
// fixtures: one candidate of each class, legal with no effect or with a permission
// ---------------------------------------------------------------------------------------------

/// The four action classes of the matrix.
#[derive(Clone, Copy, Debug)]
enum Class {
    PlayLand,
    Cast,
    Activate,
    Special,
}

const CLASSES: [Class; 4] = [
    Class::PlayLand,
    Class::Cast,
    Class::Activate,
    Class::Special,
];

impl Class {
    fn action(self) -> &'static str {
        match self {
            Class::PlayLand => ACTION_PLAY_LAND,
            Class::Cast => ACTION_CAST,
            Class::Activate => ACTION_ACTIVATE,
            Class::Special => ACTION_SPECIAL,
        }
    }

    /// The permission kind used for this class. PLAY_LAND and CAST from a graveyard need a
    /// permission (CR 601.3); ACTIVATE and SPECIAL_ACTION need no effect, so the "permission"
    /// is a TIMING_PERMISSION entry naming the action (docs/DECISIONS.md "#49 FP-047 tester —
    /// permission for ACTIVATE and SPECIAL_ACTION in the matrix").
    fn permission(self) -> RuleKind {
        match self {
            Class::PlayLand => RuleKind::PlayPermission,
            Class::Cast => RuleKind::CastPermission,
            Class::Activate | Class::Special => RuleKind::TimingPermission,
        }
    }

    /// A host with one candidate of this class, object 10, that is legal exactly when the
    /// permission applies (PLAY_LAND, CAST: graveyard) or always (ACTIVATE, SPECIAL).
    fn host(self) -> FakeHost {
        let mut h = FakeHost::new();
        match self {
            Class::PlayLand => h.lands.push(land(10, Zone::Graveyard)),
            Class::Cast => h.casts.push(instant(10, Zone::Graveyard)),
            Class::Activate => h.abilities.push(ability(10, "test.ability_a")),
            Class::Special => h.specials.push(special("END_EFFECT", 10)),
        }
        h
    }

    fn response(self) -> ChoiceResponse {
        match self {
            Class::PlayLand => play_land(10),
            Class::Cast => cast_resp(&instant(10, Zone::Graveyard)),
            Class::Activate => ability_resp(&ability(10, "test.ability_a")),
            Class::Special => special_resp(&special("END_EFFECT", 10)),
        }
    }

    /// Whether the permission is needed for the candidate to be offered.
    fn needs_permission(self) -> bool {
        matches!(self, Class::PlayLand | Class::Cast)
    }
}

fn permission(class: Class, id: u64, ts: u64) -> R {
    r(
        id,
        class.permission(),
        json!({"action": class.action()}),
        &[P1],
        ts,
    )
}

fn prohibition(class: Class, id: u64, ts: u64, players: &[PlayerId]) -> R {
    r(
        id,
        RuleKind::Prohibition,
        json!({"action": class.action()}),
        players,
        ts,
    )
}

// ---------------------------------------------------------------------------------------------
// T1 (a)-(c)
// ---------------------------------------------------------------------------------------------

/// REQ-ARCH-002, REQ-DSL-013: (a) with only a permission each class is offered; for PLAY_LAND
/// and CAST from a graveyard the basis names the permission's effect (CR 601.3, 305.1).
#[test]
fn req_arch_002_permission_only_offered() {
    // CR 601.3, CR 117.1b, CR 117.1c
    for class in CLASSES {
        let h = class.host().with_rules(&[permission(class, 1, 5)]);
        let basis = assert_legal(&h, P1, &class.response());
        if class.needs_permission() {
            assert!(has_effect(&basis, 1), "{class:?}: {basis:?}");
        }
        assert!(rule_strings_ok(&basis), "{class:?}: {basis:?}");
    }
}

/// REQ-ARCH-002: without the permission, PLAY_LAND and CAST from a graveyard are refused with
/// NoPermission (CR 601.3); ACTIVATE and SPECIAL_ACTION need none.
#[test]
fn req_arch_002_no_permission_baseline() {
    // CR 601.3
    for class in CLASSES {
        let h = class.host();
        if class.needs_permission() {
            assert_illegal(&h, P1, &class.response(), &[Reason::NoPermission]);
        } else {
            assert_legal(&h, P1, &class.response());
        }
    }
}

/// REQ-ARCH-002: (b) a prohibition alone forbids each class and explain names it (CR 101.2,
/// 602.5).
#[test]
fn req_arch_002_prohibition_only_not_offered() {
    // CR 101.2, CR 602.5
    for class in CLASSES {
        let h = class.host().with_rules(&[prohibition(class, 7, 5, &[P1])]);
        assert_prohibited(&h, P1, &class.response(), 7);
        assert!(offered(&h, P1).is_empty(), "{class:?}");
    }
}

/// REQ-ARCH-002: (c) a prohibition older than the permission beats it (CR 101.2).
#[test]
fn req_arch_002_prohibition_beats_permission_older_first() {
    // CR 101.2: the "can't" effect takes precedence whatever the timestamps.
    for class in CLASSES {
        let h = class
            .host()
            .with_rules(&[prohibition(class, 2, 1, &[P1]), permission(class, 3, 9)]);
        assert_prohibited(&h, P1, &class.response(), 2);
        assert!(offered(&h, P1).is_empty(), "{class:?}");
    }
}

/// REQ-ARCH-002: (c) a prohibition newer than the permission beats it (CR 101.2).
#[test]
fn req_arch_002_prohibition_beats_permission_newer_first() {
    // CR 101.2
    for class in CLASSES {
        let h = class
            .host()
            .with_rules(&[permission(class, 3, 1), prohibition(class, 2, 9, &[P1])]);
        assert_prohibited(&h, P1, &class.response(), 2);
        assert!(offered(&h, P1).is_empty(), "{class:?}");
    }
}

/// REQ-ARCH-002: of two applicable prohibitions, `by` names the earliest in application order
/// (timestamp order, CR 101.2 with CR 613.11 order), whatever the ids.
#[test]
fn req_arch_002_prohibited_by_names_earliest_prohibition() {
    // CR 101.2
    for class in CLASSES {
        let h = class.host().with_rules(&[
            prohibition(class, 4, 8, &[P1]),
            prohibition(class, 9, 2, &[P1]),
            permission(class, 1, 1),
        ]);
        assert_prohibited(&h, P1, &class.response(), 9);
    }
}

// ---------------------------------------------------------------------------------------------
// T1 (d), (e)
// ---------------------------------------------------------------------------------------------

/// REQ-ARCH-002: (d) a prohibition that affects only the other player leaves this player's
/// action offered, and forbids the other player's (CR 101.2).
#[test]
fn req_arch_002_prohibition_for_other_player_only() {
    // CR 101.2
    for class in CLASSES {
        let h = class.host().with_rules(&[
            prohibition(class, 2, 5, &[P2]),
            r(
                3,
                class.permission(),
                json!({"action": class.action()}),
                &[P1, P2],
                4,
            ),
        ]);
        assert_legal(&h, P1, &class.response());
        // P2 asks on P1's turn: the prohibition is still named first.
        let rs = reasons(&h, P2, &class.response());
        assert_eq!(
            rs.first(),
            Some(&Reason::Prohibited { by: eid(2) }),
            "{class:?}"
        );
    }
}

/// REQ-ARCH-002: a prohibition of one class never touches another class (CR 101.2 applies to
/// the action it names).
#[test]
fn req_arch_002_prohibition_is_per_action() {
    // CR 101.2
    for forbidden in CLASSES {
        let mut h = FakeHost::new().with_rules(&[
            prohibition(forbidden, 2, 5, &[P1]),
            r(
                3,
                RuleKind::PlayPermission,
                json!({"action": "PLAY_LAND"}),
                &[P1],
                1,
            ),
            r(
                4,
                RuleKind::CastPermission,
                json!({"action": "CAST"}),
                &[P1],
                1,
            ),
        ]);
        h.lands.push(land(10, Zone::Graveyard));
        h.casts.push(instant(11, Zone::Graveyard));
        h.abilities.push(ability(12, "test.ability_a"));
        h.specials.push(special("END_EFFECT", 13));
        let resps = [
            play_land(10),
            cast_resp(&instant(11, Zone::Graveyard)),
            ability_resp(&ability(12, "test.ability_a")),
            special_resp(&special("END_EFFECT", 13)),
        ];
        for (class, resp) in CLASSES.iter().zip(resps.iter()) {
            let same = std::mem::discriminant(class) == std::mem::discriminant(&forbidden);
            assert_eq!(
                is_offered(&h, P1, resp),
                !same,
                "forbidden {forbidden:?}, checked {class:?}"
            );
        }
    }
}

/// REQ-ARCH-002: (e) a CAST prohibition with "zone" forbids only candidates in that zone; with
/// "object" only that object (CR 101.2, 601.3).
#[test]
fn req_arch_002_cast_prohibition_filters() {
    // CR 101.2, CR 601.3
    let perm = r(
        1,
        RuleKind::CastPermission,
        json!({"action": "CAST"}),
        &[P1],
        1,
    );
    let mut base = FakeHost::new();
    base.casts = vec![
        instant(10, Zone::Hand),
        instant(11, Zone::Graveyard),
        instant(12, Zone::Hand),
    ];
    let a = cast_resp(&instant(10, Zone::Hand));
    let b = cast_resp(&instant(11, Zone::Graveyard));
    let c = cast_resp(&instant(12, Zone::Hand));

    let mut h = base.clone();
    h.rules = rules(&[
        perm.clone(),
        r(
            5,
            RuleKind::Prohibition,
            json!({"action": "CAST", "zone": "GRAVEYARD"}),
            &[P1],
            2,
        ),
    ]);
    assert_legal(&h, P1, &a);
    assert_prohibited(&h, P1, &b, 5);
    assert_legal(&h, P1, &c);

    let mut h = base.clone();
    h.rules = rules(&[
        perm.clone(),
        r(
            6,
            RuleKind::Prohibition,
            json!({"action": "CAST", "object": 12}),
            &[P1],
            2,
        ),
    ]);
    assert_legal(&h, P1, &a);
    assert_legal(&h, P1, &b);
    assert_prohibited(&h, P1, &c, 6);

    // Both filters: only an object in that zone.
    let mut h = base;
    h.rules = rules(&[
        perm,
        r(
            7,
            RuleKind::Prohibition,
            json!({"action": "CAST", "zone": "GRAVEYARD", "object": 10}),
            &[P1],
            2,
        ),
    ]);
    assert_eq!(offered(&h, P1), vec![a, b, c]);
}

/// REQ-ARCH-002: (e) an ACTIVATE prohibition with "object" forbids only abilities of that source
/// object; with "zone" only abilities in that zone (CR 602.5, 101.2).
#[test]
fn req_arch_002_activate_prohibition_filters() {
    // CR 602.5, CR 101.2
    let mut base = FakeHost::new();
    let a = ability(10, "test.ability_a");
    let b = ability(10, "test.ability_b");
    let c = ability(11, "test.ability_a");
    let d = AbilityCandidate {
        zone: Zone::Graveyard,
        ..ability(12, "test.ability_a")
    };
    base.abilities = vec![a.clone(), b.clone(), c.clone(), d.clone()];

    let h = base.clone().with_rules(&[r(
        5,
        RuleKind::Prohibition,
        json!({"action": "ACTIVATE", "object": 10}),
        &[P1],
        2,
    )]);
    assert_prohibited(&h, P1, &ability_resp(&a), 5);
    assert_prohibited(&h, P1, &ability_resp(&b), 5);
    assert_legal(&h, P1, &ability_resp(&c));
    assert_legal(&h, P1, &ability_resp(&d));

    let h = base.with_rules(&[r(
        6,
        RuleKind::Prohibition,
        json!({"action": "ACTIVATE", "zone": "GRAVEYARD"}),
        &[P1],
        2,
    )]);
    assert_legal(&h, P1, &ability_resp(&a));
    assert_legal(&h, P1, &ability_resp(&c));
    assert_prohibited(&h, P1, &ability_resp(&d), 6);
}

/// REQ-ARCH-002: (e) a SPECIAL_ACTION prohibition with "kind" forbids only that kind, with
/// "object" only that object, with neither every kind (CR 116.2, 101.2).
#[test]
fn req_arch_002_special_prohibition_filters() {
    // CR 116.2, CR 101.2
    let mut base = FakeHost::new();
    let e = special("END_EFFECT", 10);
    let i = special("IGNORE_STATIC_EFFECT", 11);
    let f = face_up(12, FaceDownOrigin::Morph { has_cost: true });
    base.specials = vec![e.clone(), i.clone()];
    base.face_up = vec![f.clone()];

    let h = base.clone().with_rules(&[r(
        5,
        RuleKind::Prohibition,
        json!({"action": "SPECIAL_ACTION", "kind": "END_EFFECT"}),
        &[P1],
        2,
    )]);
    assert_prohibited(&h, P1, &special_resp(&e), 5);
    assert_legal(&h, P1, &special_resp(&i));
    assert_legal(&h, P1, &face_up_resp(&f));

    let h = base.clone().with_rules(&[r(
        6,
        RuleKind::Prohibition,
        json!({"action": "SPECIAL_ACTION", "object": 12}),
        &[P1],
        2,
    )]);
    assert_legal(&h, P1, &special_resp(&e));
    assert_legal(&h, P1, &special_resp(&i));
    assert_prohibited(&h, P1, &face_up_resp(&f), 6);

    let h = base.with_rules(&[r(
        7,
        RuleKind::Prohibition,
        json!({"action": "SPECIAL_ACTION"}),
        &[P1],
        2,
    )]);
    assert_prohibited(&h, P1, &special_resp(&e), 7);
    assert_prohibited(&h, P1, &special_resp(&i), 7);
    assert_prohibited(&h, P1, &face_up_resp(&f), 7);
    assert!(offered(&h, P1).is_empty());
}

/// REQ-ARCH-002: (e) PLAY_PERMISSION filters: "zone" grants only in that zone, "object" only
/// that object (CR 305.1, 601.3; PLAY_LAND prohibitions are unfiltered, so not tested here).
#[test]
fn req_arch_002_play_permission_filters() {
    // CR 305.1
    let mut base = FakeHost::new();
    base.lands = vec![land(10, Zone::Graveyard), land(11, Zone::Exile)];

    let h = base.clone().with_rules(&[r(
        3,
        RuleKind::PlayPermission,
        json!({"action": "PLAY_LAND", "zone": "GRAVEYARD"}),
        &[P1],
        1,
    )]);
    assert!(has_effect(&assert_legal(&h, P1, &play_land(10)), 3));
    assert_illegal(&h, P1, &play_land(11), &[Reason::NoPermission]);

    let h = base.with_rules(&[r(
        4,
        RuleKind::PlayPermission,
        json!({"action": "PLAY_LAND", "object": 11}),
        &[P1],
        1,
    )]);
    assert_illegal(&h, P1, &play_land(10), &[Reason::NoPermission]);
    assert!(has_effect(&assert_legal(&h, P1, &play_land(11)), 4));
}

/// REQ-ARCH-002: a CAST_PERMISSION with "zone"/"object" grants only matching candidates
/// (CR 601.3).
#[test]
fn req_arch_002_cast_permission_filters() {
    // CR 601.3
    let mut base = FakeHost::new();
    base.casts = vec![instant(10, Zone::Graveyard), instant(11, Zone::Exile)];
    let g = cast_resp(&instant(10, Zone::Graveyard));
    let x = cast_resp(&instant(11, Zone::Exile));

    let h = base.clone().with_rules(&[r(
        3,
        RuleKind::CastPermission,
        json!({"action": "CAST", "zone": "EXILE"}),
        &[P1],
        1,
    )]);
    assert_illegal(&h, P1, &g, &[Reason::NoPermission]);
    assert!(has_effect(&assert_legal(&h, P1, &x), 3));

    let h = base.with_rules(&[r(
        4,
        RuleKind::CastPermission,
        json!({"action": "CAST", "object": 10}),
        &[P1],
        1,
    )]);
    assert!(has_effect(&assert_legal(&h, P1, &g), 4));
    assert_illegal(&h, P1, &x, &[Reason::NoPermission]);
}

// ---------------------------------------------------------------------------------------------
// T10: malformed params (R12)
// ---------------------------------------------------------------------------------------------

/// The malformed variants of a filtered entry for `action`, each paired with a description.
fn malformed(action: &str) -> Vec<serde_json::Value> {
    vec![
        json!({"action": action, "zone": 5}),
        json!({"action": action, "zone": ["GRAVEYARD"]}),
        json!({"action": action, "zone": "test.no_such_zone"}),
        json!({"action": action, "object": "x"}),
        json!({"action": action, "object": -1}),
        json!({"action": action, "object": 10.5}),
        json!({"action": action, "object": null}),
        json!({"action": action, "kind": []}),
        json!({"action": action, "kind": 3}),
        json!({"zone": "GRAVEYARD"}),
        json!({}),
        json!({"action": 5}),
        json!({"action": [action]}),
        json!({"action": null}),
        json!({"action": action.to_lowercase()}),
    ]
}

/// REQ-ARCH-002, REQ-DSL-013 (R12): a CAST or ACTIVATE or SPECIAL_ACTION prohibition with
/// malformed params ("zone": 5, "object": "x", "object": -1, "kind": [], missing or non-string
/// "action") never panics and never forbids the action (CR 101.2 needs an applicable effect).
#[test]
fn req_arch_002_malformed_prohibition_never_applies() {
    // CR 101.2
    for class in [Class::Cast, Class::Activate, Class::Special] {
        for params in malformed(class.action()) {
            // A "kind" filter on a CAST/ACTIVATE prohibition is not specified; skip those.
            // A "kind" filter is specified for SPECIAL_ACTION only and a "zone" filter is not
            // specified for SPECIAL_ACTION: skip those combinations.
            let has_kind = params.get("kind").is_some();
            let has_zone = params.get("zone").is_some() && params.get("action").is_some();
            if (has_kind && !matches!(class, Class::Special))
                || (has_zone && matches!(class, Class::Special))
            {
                continue;
            }
            let h = class.host().with_rules(&[
                permission(class, 1, 1),
                r(2, RuleKind::Prohibition, params.clone(), &[P1], 2),
            ]);
            assert_legal(&h, P1, &class.response());
            let _ = explain(&h, P1, &class.response());
            let _ = priority_payload(&h, P1, true);
            assert!(
                !reasons_or_empty(&h, &class.response())
                    .contains(&Reason::Prohibited { by: eid(2) }),
                "{class:?} {params}"
            );
        }
    }
}

/// The illegal reasons of `resp` for P1, or empty when legal.
fn reasons_or_empty(h: &FakeHost, resp: &ChoiceResponse) -> Vec<Reason> {
    match explain(h, P1, resp) {
        Legality::Illegal { reasons } => reasons,
        Legality::Legal { .. } => Vec::new(),
    }
}

/// REQ-ARCH-002, REQ-DSL-013 (R12): a PLAY_PERMISSION, CAST_PERMISSION or TIMING_PERMISSION
/// with malformed params never grants anything (CR 601.3, 305.1).
#[test]
fn req_arch_002_malformed_permission_never_applies() {
    // CR 601.3, CR 305.1
    for params in malformed("PLAY_LAND") {
        if params.get("kind").is_some() {
            continue;
        }
        let mut h =
            FakeHost::new().with_rules(&[r(3, RuleKind::PlayPermission, params.clone(), &[P1], 1)]);
        h.lands.push(land(10, Zone::Graveyard));
        assert_illegal(&h, P1, &play_land(10), &[Reason::NoPermission]);
    }
    for params in malformed("CAST") {
        if params.get("kind").is_some() {
            continue;
        }
        let mut h =
            FakeHost::new().with_rules(&[r(3, RuleKind::CastPermission, params.clone(), &[P1], 1)]);
        h.casts.push(instant(10, Zone::Graveyard));
        assert_illegal(
            &h,
            P1,
            &cast_resp(&instant(10, Zone::Graveyard)),
            &[Reason::NoPermission],
        );
        // A malformed TIMING_PERMISSION never grants flash (CR 601.3b-d need an applicable
        // effect).
        let mut h = FakeHost::new()
            .with_rules(&[r(4, RuleKind::TimingPermission, params.clone(), &[P1], 1)])
            .active(Some(P2));
        h.casts.push(cast(10, Zone::Hand));
        assert_illegal(
            &h,
            P1,
            &cast_resp(&cast(10, Zone::Hand)),
            &[Reason::Timing(TimingRequirement::SorceryTiming)],
        );
    }
}

/// REQ-ARCH-002 (R12): rule params of every JSON shape under every key never panic any entry
/// point, whatever the rule kind.
#[test]
fn req_arch_002_hostile_params_never_panic() {
    let values = [
        json!(null),
        json!(true),
        json!(-1),
        json!(u64::MAX),
        json!(1.5),
        json!(""),
        json!("GRAVEYARD"),
        json!([]),
        json!([1, "x"]),
        json!({}),
        json!({"action": "CAST"}),
    ];
    let keys = ["action", "zone", "object", "kind", "count"];
    let kinds = [
        RuleKind::Prohibition,
        RuleKind::PlayPermission,
        RuleKind::CastPermission,
        RuleKind::TimingPermission,
        RuleKind::AdditionalLandPlay,
    ];
    let mut id = 0;
    for kind in kinds {
        for key in keys {
            for v in &values {
                id += 1;
                let mut obj = serde_json::Map::new();
                obj.insert(key.to_owned(), v.clone());
                let mut h = FakeHost::new()
                    .with_rules(&[r(id, kind, serde_json::Value::Object(obj), &[P1, P2], 1)])
                    .played(P1, 1);
                h.lands.push(land(10, Zone::Graveyard));
                h.lands.push(land(11, Zone::Hand));
                h.casts.push(cast(12, Zone::Graveyard));
                h.abilities.push(ability(13, "test.ability_a"));
                h.specials.push(special("PLOT", 14));
                h.face_up.push(face_up(
                    15,
                    FaceDownOrigin::Other {
                        can_turn_face_up: true,
                    },
                ));
                for p in [P1, P2] {
                    let actions = legal_actions(&h, p);
                    for a in &actions {
                        assert!(matches!(
                            explain(&h, p, &a.response),
                            Legality::Legal { .. }
                        ));
                    }
                    let _ = priority_payload(&h, p, false);
                }
            }
        }
    }
}
