// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::permanent`. Black-box: written from the work-item brief only.
//!
//! Combat state: attacking, blocking and blocked bookkeeping, removal from combat, leaving the
//! battlefield during combat, clearing combat, determinism across operation orders, and the
//! canonical encoding of the whole container (REQ-ZONE-001 item 3; REQ-CORE-006).

use std::collections::BTreeSet;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::permanent::{
    AttachTarget, AttackState, AttackTarget, CombatState, PermanentError, Permanents,
};
use fineprint_core::state::DesignationValue;

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn holding(ids: &[u64]) -> Permanents {
    let mut p = Permanents::new();
    for &i in ids {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    p
}

fn combat(p: &Permanents, id: u64) -> CombatState {
    p.get(oid(id)).unwrap().combat().clone()
}

fn at_player(n: u64) -> AttackState {
    AttackState::Attacking(Some(AttackTarget::Player(pid(n))))
}

fn at_perm(n: u64) -> AttackState {
    AttackState::Attacking(Some(AttackTarget::Permanent(oid(n))))
}

fn set_of(ids: &[u64]) -> BTreeSet<ObjectId> {
    ids.iter().map(|&n| oid(n)).collect()
}

fn bytes(p: &Permanents) -> Vec<u8> {
    let mut out = Vec::new();
    p.encode_canonical(&mut out);
    out
}

// REQ-ZONE-001 item 3; CR 506.3: set_attack stores every shape without validating the target.
#[test]
fn req_zone_001_set_attack_stores_without_validation() {
    let mut p = holding(&[1, 2]);
    for state in [
        at_player(2),
        at_player(u64::MAX),
        at_perm(2),
        at_perm(999),
        at_perm(1),
        AttackState::Attacking(None),
        AttackState::NotAttacking,
    ] {
        p.set_attack(oid(1), state).unwrap();
        assert_eq!(combat(&p, 1).attack, state);
        assert!(!combat(&p, 1).removed_from_combat);
        assert_eq!(combat(&p, 2), CombatState::default());
    }
}

// REQ-ZONE-001 item 3: set_attack does not touch removed_from_combat.
#[test]
fn req_zone_001_set_attack_keeps_removed_flag() {
    let mut p = holding(&[1]);
    p.remove_from_combat(oid(1)).unwrap();
    p.set_attack(oid(1), at_player(2)).unwrap();
    let c = combat(&p, 1);
    assert_eq!(c.attack, at_player(2));
    assert!(c.removed_from_combat);
}

// REQ-ZONE-001 item 3: set_blocking replaces the set; duplicates collapse; empty clears.
#[test]
fn req_zone_001_set_blocking_replaces_and_dedups() {
    let mut p = holding(&[1, 2, 3, 4]);
    p.set_blocking(oid(1), &[oid(3), oid(2), oid(3), oid(2)])
        .unwrap();
    assert_eq!(combat(&p, 1).blocking, set_of(&[2, 3]));
    p.set_blocking(oid(1), &[oid(4)]).unwrap();
    assert_eq!(combat(&p, 1).blocking, set_of(&[4]));
    p.set_blocking(oid(1), &[]).unwrap();
    assert!(combat(&p, 1).blocking.is_empty());
    assert_eq!(combat(&p, 2), CombatState::default());
}

// REQ-ZONE-001 item 3: an unheld id in the blocking slice is UnknownObject of that id and the
// container is unchanged.
#[test]
fn req_zone_001_set_blocking_unknown_member() {
    let mut p = holding(&[1, 2, 3]);
    p.set_blocking(oid(1), &[oid(2)]).unwrap();
    let before = p.clone();
    assert_eq!(
        p.set_blocking(oid(1), &[oid(3), oid(9)]),
        Err(PermanentError::UnknownObject(oid(9)))
    );
    assert_eq!(
        p.set_blocking(oid(1), &[oid(u64::MAX)]),
        Err(PermanentError::UnknownObject(oid(u64::MAX)))
    );
    assert_eq!(
        p.set_blocking(oid(1), &[oid(0), oid(2)]),
        Err(PermanentError::UnknownObject(oid(0)))
    );
    assert_eq!(p, before);
}

// REQ-ZONE-001 item 3: set_blocked sets the flag.
#[test]
fn req_zone_001_set_blocked() {
    let mut p = holding(&[1]);
    p.set_blocked(oid(1), true).unwrap();
    assert!(combat(&p, 1).blocked);
    p.set_blocked(oid(1), true).unwrap();
    assert!(combat(&p, 1).blocked);
    p.set_blocked(oid(1), false).unwrap();
    assert_eq!(combat(&p, 1), CombatState::default());
}

// REQ-ZONE-001 item 3; CR 506.4: a removed permanent stops attacking, blocking and being blocked.
#[test]
fn req_zone_001_remove_from_combat_clears_own_state() {
    let mut p = holding(&[1, 2, 3]);
    p.set_attack(oid(1), at_player(2)).unwrap();
    p.set_blocking(oid(1), &[oid(2), oid(3)]).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.set_tapped(oid(1), true).unwrap();
    p.remove_from_combat(oid(1)).unwrap();
    assert_eq!(
        combat(&p, 1),
        CombatState {
            removed_from_combat: true,
            ..CombatState::default()
        }
    );
    // Status untouched.
    assert!(p.get(oid(1)).unwrap().status().tapped);
    // Removing twice is the same state.
    let before = p.clone();
    p.remove_from_combat(oid(1)).unwrap();
    assert_eq!(p, before);
}

// REQ-ZONE-001 item 3; CR 506.4, 506.4c, 509.1h: removing a planeswalker-like target leaves its
// attackers attacking nothing; player targets and other permanents' blocking and blocked stay.
#[test]
fn req_zone_001_remove_from_combat_effects_on_others() {
    let mut p = holding(&[1, 2, 3, 4, 5, 6]);
    p.set_attack(oid(2), at_perm(1)).unwrap();
    p.set_attack(oid(3), at_perm(1)).unwrap();
    p.set_attack(oid(4), at_player(1)).unwrap();
    p.set_attack(oid(5), at_perm(6)).unwrap();
    p.set_blocked(oid(2), true).unwrap();
    // 6 blocks 1 (as if 1 were attacking) and 2.
    p.set_blocking(oid(6), &[oid(1), oid(2)]).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.remove_from_combat(oid(1)).unwrap();

    let c2 = combat(&p, 2);
    assert_eq!(c2.attack, AttackState::Attacking(None));
    assert!(c2.blocked);
    assert!(!c2.removed_from_combat);
    assert_eq!(combat(&p, 3).attack, AttackState::Attacking(None));
    assert_eq!(combat(&p, 4).attack, at_player(1));
    assert_eq!(combat(&p, 5).attack, at_perm(6));
    // A removed id is not dropped from others' blocking sets.
    assert_eq!(combat(&p, 6).blocking, set_of(&[1, 2]));
    assert!(!combat(&p, 6).removed_from_combat);

    // Player(1) is not rewritten when permanent 1 is removed, even with the same number.
    let mut q = holding(&[1, 2]);
    q.set_attack(oid(2), at_player(1)).unwrap();
    q.remove_from_combat(oid(1)).unwrap();
    assert_eq!(combat(&q, 2).attack, at_player(1));
}

// REQ-ZONE-001 item 3; CR 509.1h: an attacker stays blocked after all its blockers are removed.
#[test]
fn req_zone_001_blocked_stays_after_blockers_removed() {
    let mut p = holding(&[1, 2, 3]);
    p.set_attack(oid(1), at_player(2)).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.set_blocking(oid(2), &[oid(1)]).unwrap();
    p.set_blocking(oid(3), &[oid(1)]).unwrap();
    p.remove_from_combat(oid(2)).unwrap();
    p.remove_from_combat(oid(3)).unwrap();
    let c = combat(&p, 1);
    assert!(c.blocked);
    assert_eq!(c.attack, at_player(2));
}

// REQ-ZONE-001 item 3; CR 509.1h, 506.4: an attacker stays blocked after all its blockers leave,
// and a left id disappears from every blocking set.
#[test]
fn req_zone_001_blocked_stays_after_blockers_leave() {
    let mut p = holding(&[1, 2, 3, 4]);
    p.set_attack(oid(1), at_player(2)).unwrap();
    p.set_attack(oid(4), at_player(2)).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.set_blocked(oid(4), true).unwrap();
    p.set_blocking(oid(2), &[oid(1), oid(4)]).unwrap();
    p.set_blocking(oid(3), &[oid(1)]).unwrap();
    let left = p.leave(oid(2)).unwrap();
    assert_eq!(left.combat().blocking, set_of(&[1, 4]));
    p.leave(oid(3)).unwrap();
    assert!(combat(&p, 1).blocked);
    assert!(combat(&p, 4).blocked);

    // A leaving attacker is removed from its blockers' sets.
    let mut q = holding(&[1, 2, 3]);
    q.set_attack(oid(1), at_player(2)).unwrap();
    q.set_attack(oid(3), at_player(2)).unwrap();
    q.set_blocking(oid(2), &[oid(1), oid(3)]).unwrap();
    q.set_blocked(oid(3), true).unwrap();
    q.leave(oid(1)).unwrap();
    assert_eq!(combat(&q, 2).blocking, set_of(&[3]));
    assert!(combat(&q, 3).blocked);
    assert!(!combat(&q, 2).removed_from_combat);
    q.leave(oid(3)).unwrap();
    assert!(combat(&q, 2).blocking.is_empty());
}

// REQ-ZONE-001 item 3; CR 506.4, 506.4c: when an attacked planeswalker-like permanent leaves,
// its attackers keep attacking nothing; leave returns the pre-removal combat state.
#[test]
fn req_zone_001_attacked_permanent_leaves() {
    let mut p = holding(&[1, 2, 3, 4]);
    p.set_attack(oid(1), at_player(3)).unwrap();
    p.set_attack(oid(2), at_perm(4)).unwrap();
    p.set_attack(oid(3), at_player(4)).unwrap();
    p.set_blocked(oid(2), true).unwrap();
    let left = p.leave(oid(4)).unwrap();
    assert_eq!(left.combat(), &CombatState::default());
    let c2 = combat(&p, 2);
    assert_eq!(c2.attack, AttackState::Attacking(None));
    assert!(c2.blocked);
    assert!(!c2.removed_from_combat);
    assert_eq!(combat(&p, 3).attack, at_player(4));
    assert_eq!(combat(&p, 1).attack, at_player(3));
    // The left attacker's own state comes back with it.
    let left = p.leave(oid(1)).unwrap();
    assert_eq!(left.combat().attack, at_player(3));
    // Re-entering gives default combat (CR 400.7).
    p.enter(oid(4), pid(1), 1).unwrap();
    assert_eq!(combat(&p, 4), CombatState::default());
    assert_eq!(combat(&p, 2).attack, AttackState::Attacking(None));
}

// REQ-ZONE-001 item 3; CR 506.4b: tapping or untapping an attacker or blocker keeps it in combat.
#[test]
fn req_zone_001_tapping_keeps_combat() {
    let mut p = holding(&[1, 2]);
    p.set_attack(oid(1), at_perm(2)).unwrap();
    p.set_blocking(oid(2), &[oid(1)]).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    let before = (combat(&p, 1), combat(&p, 2));
    p.set_tapped(oid(1), true).unwrap();
    p.set_tapped(oid(2), true).unwrap();
    p.set_tapped(oid(1), false).unwrap();
    assert_eq!((combat(&p, 1), combat(&p, 2)), before);
}

// REQ-ZONE-001 item 3: clear_combat resets one permanent, including removed_from_combat.
#[test]
fn req_zone_001_clear_combat() {
    let mut p = holding(&[1, 2]);
    p.set_attack(oid(2), at_player(1)).unwrap();
    p.remove_from_combat(oid(1)).unwrap();
    p.set_attack(oid(1), at_perm(2)).unwrap();
    p.set_blocking(oid(1), &[oid(2)]).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    let other = combat(&p, 2);
    p.clear_combat(oid(1)).unwrap();
    assert_eq!(combat(&p, 1), CombatState::default());
    assert_eq!(combat(&p, 2), other);
}

// REQ-ZONE-001 item 3: clear_all_combat resets every combat state and changes nothing else.
#[test]
fn req_zone_001_clear_all_combat() {
    let mut empty = Permanents::new();
    empty.clear_all_combat();
    assert_eq!(empty, Permanents::new());

    let base = || {
        let mut p = Permanents::new();
        p.enter(oid(1), pid(1), 2).unwrap();
        p.enter(oid(2), pid(2), 3).unwrap();
        p.enter(oid(3), pid(1), 4).unwrap();
        p.set_tapped(oid(1), true).unwrap();
        p.set_face_down(oid(2), true).unwrap();
        p.set_designation(oid(3), "test.marker_a", DesignationValue::Int(2))
            .unwrap();
        p.attach(oid(3), AttachTarget::Permanent(oid(2))).unwrap();
        p.attach(oid(2), AttachTarget::Player(pid(1))).unwrap();
        p
    };
    let mut p = base();
    p.set_attack(oid(1), at_perm(2)).unwrap();
    p.set_attack(oid(3), AttackState::Attacking(None)).unwrap();
    p.set_blocking(oid(2), &[oid(1), oid(3)]).unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.remove_from_combat(oid(3)).unwrap();
    p.set_blocked(oid(3), true).unwrap();
    p.clear_all_combat();
    assert_eq!(p, base());
    for id in 1..=3 {
        assert_eq!(combat(&p, id), CombatState::default());
    }
}

// REQ-ZONE-001 item 3: unknown ids in combat methods are errors that change nothing.
#[test]
fn req_zone_001_combat_unknown_ids() {
    let mut p = holding(&[1]);
    p.set_attack(oid(1), at_perm(5)).unwrap();
    let before = p.clone();
    for raw in [0u64, 5, u64::MAX] {
        let e = Err(PermanentError::UnknownObject(oid(raw)));
        assert_eq!(p.set_attack(oid(raw), at_player(1)), e);
        assert_eq!(p.set_blocking(oid(raw), &[oid(1)]), e);
        assert_eq!(p.set_blocked(oid(raw), true), e);
        assert_eq!(p.remove_from_combat(oid(raw)), e);
        assert_eq!(p.clear_combat(oid(raw)), e);
        assert_eq!(p, before);
    }
}

// REQ-ZONE-001 item 3: AttackTarget and AttackState derives distinguish kinds.
#[test]
fn req_zone_001_attack_types_derives() {
    let a = AttackTarget::Player(pid(1));
    let b = AttackTarget::Permanent(oid(1));
    assert_ne!(a, b);
    assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
    let s = AttackState::Attacking(Some(a));
    let t = s;
    assert_eq!(s, t);
    assert_ne!(AttackState::Attacking(None), AttackState::NotAttacking);
    assert_ne!(
        AttackState::Attacking(None),
        AttackState::Attacking(Some(b))
    );
    let c = CombatState::default();
    assert_eq!(c.clone(), c);
    assert!(!format!("{c:?}").is_empty());
}

// ---------------------------------------------------------------------------------------------
// Determinism and canonical encoding.

/// A plain description of one permanent, used to build containers in different orders.
#[derive(Clone)]
struct Spec {
    id: u64,
    controller: u64,
    since: u64,
    flags: [bool; 4],
    designations: Vec<(String, DesignationValue)>,
    attach: Option<AttachTarget>,
    attack: AttackState,
    blocking: Vec<u64>,
    blocked: bool,
    removed: bool,
}

/// Builds a container from `specs`; `reverse` reverses every insertion order (permanents,
/// designations, blocking slices). Combat is applied after all removals so no rewrites happen.
fn build(specs: &[Spec], reverse: bool) -> Permanents {
    let mut order: Vec<&Spec> = specs.iter().collect();
    if reverse {
        order.reverse();
    }
    let mut p = Permanents::new();
    for s in &order {
        p.enter(oid(s.id), pid(s.controller), s.since).unwrap();
    }
    for s in &order {
        let id = oid(s.id);
        p.set_tapped(id, s.flags[0]).unwrap();
        p.set_flipped(id, s.flags[1]).unwrap();
        p.set_face_down(id, s.flags[2]).unwrap();
        p.set_phased_out(id, s.flags[3]).unwrap();
        let mut d = s.designations.clone();
        if reverse {
            d.reverse();
        }
        for (k, v) in d {
            p.set_designation(id, &k, v).unwrap();
        }
    }
    p.clear_all_combat();
    for s in &order {
        if s.removed {
            p.remove_from_combat(oid(s.id)).unwrap();
        }
    }
    for s in &order {
        let id = oid(s.id);
        p.set_attack(id, s.attack).unwrap();
        let mut b: Vec<ObjectId> = s.blocking.iter().map(|&n| oid(n)).collect();
        if reverse {
            b.reverse();
        }
        p.set_blocking(id, &b).unwrap();
        p.set_blocked(id, s.blocked).unwrap();
    }
    for s in &order {
        if let Some(t) = s.attach {
            p.attach(oid(s.id), t).unwrap();
        }
    }
    p
}

/// Asserts the built container shows exactly what `specs` describes.
fn assert_matches(p: &Permanents, specs: &[Spec]) {
    let mut ids: Vec<ObjectId> = specs.iter().map(|s| oid(s.id)).collect();
    ids.sort();
    assert_eq!(p.ids(), ids);
    for s in specs {
        let perm = p.get(oid(s.id)).unwrap();
        assert_eq!(perm.controller(), pid(s.controller));
        assert_eq!(perm.controlled_since_turn(), s.since);
        let st = perm.status();
        assert_eq!(
            [st.tapped, st.flipped, st.face_down, st.phased_out],
            s.flags
        );
        assert_eq!(perm.designations().len(), s.designations.len());
        for (k, v) in &s.designations {
            assert_eq!(perm.designation(k), Some(v));
        }
        assert_eq!(perm.attached_to(), s.attach);
        let c = perm.combat();
        assert_eq!(c.attack, s.attack);
        assert_eq!(c.blocking, set_of(&s.blocking));
        assert_eq!(c.blocked, s.blocked);
        assert_eq!(c.removed_from_combat, s.removed);
    }
}

fn base_specs() -> Vec<Spec> {
    vec![
        Spec {
            id: 10,
            controller: 1,
            since: 3,
            flags: [true, false, false, false],
            designations: vec![
                ("test.marker_a".to_string(), DesignationValue::Flag),
                ("test.marker_b".to_string(), DesignationValue::Int(7)),
            ],
            attach: Some(AttachTarget::Permanent(oid(20))),
            attack: at_player(2),
            blocking: vec![],
            blocked: true,
            removed: false,
        },
        Spec {
            id: 20,
            controller: 2,
            since: 4,
            flags: [false, true, false, false],
            designations: vec![(
                "test.marker_c".to_string(),
                DesignationValue::Text("x".to_string()),
            )],
            attach: Some(AttachTarget::Player(pid(1))),
            attack: AttackState::NotAttacking,
            blocking: vec![10, 30],
            blocked: false,
            removed: true,
        },
        Spec {
            id: 30,
            controller: 1,
            since: 5,
            flags: [false, false, true, false],
            designations: vec![],
            attach: None,
            attack: at_perm(20),
            blocking: vec![],
            blocked: false,
            removed: false,
        },
    ]
}

type Mutation = (&'static str, fn(&mut Vec<Spec>));

fn mutations() -> Vec<Mutation> {
    vec![
        ("tapped", |s| s[0].flags[0] = false),
        ("flipped", |s| s[0].flags[1] = true),
        ("face_down", |s| s[0].flags[2] = true),
        ("phased_out", |s| s[0].flags[3] = true),
        ("designation key", |s| {
            s[0].designations[0].0 = "test.marker_z".to_string();
        }),
        ("designation Flag->Int", |s| {
            s[0].designations[0].1 = DesignationValue::Int(0);
        }),
        ("designation Int value", |s| {
            s[0].designations[1].1 = DesignationValue::Int(8);
        }),
        ("designation Int->Text", |s| {
            s[0].designations[1].1 = DesignationValue::Text("7".to_string());
        }),
        ("designation Text value", |s| {
            s[1].designations[0].1 = DesignationValue::Text("y".to_string());
        }),
        ("designation added", |s| {
            s[2].designations
                .push(("test.marker_a".to_string(), DesignationValue::Flag));
        }),
        ("designation removed", |s| {
            s[0].designations.pop();
        }),
        ("controller", |s| s[0].controller = 3),
        ("controlled_since", |s| s[0].since = 2),
        ("attachment kind", |s| {
            s[0].attach = Some(AttachTarget::Player(pid(20)));
        }),
        ("attachment id", |s| {
            s[0].attach = Some(AttachTarget::Permanent(oid(30)));
        }),
        ("attachment player id", |s| {
            s[1].attach = Some(AttachTarget::Player(pid(2)));
        }),
        ("attachment none->some", |s| {
            s[2].attach = Some(AttachTarget::Player(pid(1)));
        }),
        ("attachment some->none", |s| s[0].attach = None),
        ("attack Attacking->NotAttacking", |s| {
            s[0].attack = AttackState::NotAttacking;
        }),
        ("attack Some->None", |s| {
            s[0].attack = AttackState::Attacking(None);
        }),
        ("attack NotAttacking->Attacking(None)", |s| {
            s[1].attack = AttackState::Attacking(None);
        }),
        ("attack target kind", |s| s[0].attack = at_perm(2)),
        ("attack player id", |s| s[0].attack = at_player(3)),
        ("attack permanent id", |s| s[2].attack = at_perm(10)),
        ("attack permanent->player same number", |s| {
            s[2].attack = at_player(20);
        }),
        ("blocking member removed", |s| s[1].blocking = vec![10]),
        ("blocking member swapped", |s| s[1].blocking = vec![10, 20]),
        ("blocking other member removed", |s| {
            s[1].blocking = vec![30]
        }),
        ("blocking emptied", |s| s[1].blocking = vec![]),
        ("blocking added elsewhere", |s| s[2].blocking = vec![10]),
        ("blocked", |s| s[0].blocked = false),
        ("blocked set", |s| s[2].blocked = true),
        ("removed true->false", |s| s[1].removed = false),
        ("removed false->true", |s| s[0].removed = true),
        ("id set: added", |s| {
            s.push(Spec {
                id: 40,
                controller: 1,
                since: 1,
                flags: [false; 4],
                designations: vec![],
                attach: None,
                attack: AttackState::NotAttacking,
                blocking: vec![],
                blocked: false,
                removed: false,
            });
        }),
        ("id set: renumbered", |s| {
            s[2].id = 31;
            s[1].blocking = vec![10, 31];
        }),
    ]
}

// REQ-ZONE-001 item 3; REQ-CORE-006: the builder produces exactly what each spec describes, in
// both insertion orders, so the encoding checks below compare the intended values.
#[test]
fn req_zone_001_encoding_builder_matches_specs() {
    let base = base_specs();
    assert_matches(&build(&base, false), &base);
    assert_matches(&build(&base, true), &base);
    for (name, m) in mutations() {
        let mut s = base_specs();
        m(&mut s);
        let p = build(&s, false);
        assert_matches(&p, &s);
        assert_eq!(p, build(&s, true), "{name}");
    }
}

// REQ-ZONE-001 item 3; REQ-CORE-006: equal values encode identically, including values built in
// different insertion orders, and encoding is stable over repeated calls.
#[test]
fn req_zone_001_encoding_equal_values_equal_bytes() {
    let a = build(&base_specs(), false);
    let b = build(&base_specs(), true);
    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
    let c = a.clone();
    assert_eq!(bytes(&c), bytes(&a));
    let first = bytes(&a);
    assert!(!first.is_empty());
    for _ in 0..10 {
        assert_eq!(bytes(&a), first);
    }
    // Appends to an existing buffer without disturbing its prefix.
    let mut out = vec![0xAB, 0xCD];
    a.encode_canonical(&mut out);
    assert_eq!(&out[..2], &[0xAB, 0xCD]);
    assert_eq!(&out[2..], first.as_slice());
    // Empty containers encode identically too.
    assert_eq!(bytes(&Permanents::new()), bytes(&Permanents::default()));
    assert_ne!(bytes(&Permanents::new()), first);
}

// REQ-ZONE-001 item 3; REQ-CORE-006: changing any single field changes the canonical bytes and
// the value.
#[test]
fn req_zone_001_encoding_single_field_changes_bytes() {
    let base = build(&base_specs(), false);
    let base_bytes = bytes(&base);
    let mut seen: Vec<(&str, Vec<u8>)> = vec![("base", base_bytes.clone())];
    for (name, m) in mutations() {
        let mut s = base_specs();
        m(&mut s);
        let p = build(&s, false);
        assert_ne!(p, base, "{name}: value unchanged");
        let b = bytes(&p);
        assert_ne!(b, base_bytes, "{name}: bytes unchanged");
        assert_eq!(b, bytes(&build(&s, true)), "{name}: order-dependent bytes");
        // Every distinct value encodes to distinct bytes.
        for (other, ob) in &seen {
            assert_ne!(&b, ob, "{name} collides with {other}");
        }
        seen.push((name, b));
    }
}

// REQ-ZONE-001 item 3; REQ-CORE-006: the same final state reached through two different
// operation orders gives equal values and equal canonical bytes.
#[test]
fn req_zone_001_determinism_two_operation_orders() {
    let mut a = holding(&[1, 2, 3, 4]);
    a.set_tapped(oid(1), true).unwrap();
    a.set_designation(oid(2), "test.marker_a", DesignationValue::Int(5))
        .unwrap();
    a.set_designation(oid(2), "test.marker_b", DesignationValue::Flag)
        .unwrap();
    a.attach(oid(3), AttachTarget::Permanent(oid(4))).unwrap();
    // Control change before the attack is declared, so the attack target is kept.
    a.set_controller(oid(4), pid(2), 3).unwrap();
    a.set_attack(oid(1), at_perm(4)).unwrap();
    a.set_blocking(oid(2), &[oid(1)]).unwrap();
    a.set_blocked(oid(1), true).unwrap();

    let mut b = Permanents::new();
    for i in [4u64, 2, 3, 1] {
        b.enter(oid(i), pid(1), 1).unwrap();
    }
    b.enter(oid(9), pid(1), 1).unwrap();
    b.set_controller(oid(4), pid(2), 3).unwrap();
    b.set_blocked(oid(1), true).unwrap();
    b.set_blocking(oid(2), &[oid(9), oid(1)]).unwrap();
    b.set_attack(oid(1), at_perm(4)).unwrap();
    b.attach(oid(3), AttachTarget::Player(pid(1))).unwrap();
    b.attach(oid(3), AttachTarget::Permanent(oid(4))).unwrap();
    b.set_designation(oid(2), "test.marker_b", DesignationValue::Flag)
        .unwrap();
    b.set_designation(oid(2), "test.marker_a", DesignationValue::Int(1))
        .unwrap();
    b.set_designation(oid(2), "test.marker_c", DesignationValue::Flag)
        .unwrap();
    b.clear_designation(oid(2), "test.marker_c").unwrap();
    b.set_designation(oid(2), "test.marker_a", DesignationValue::Int(5))
        .unwrap();
    b.set_flipped(oid(1), true).unwrap();
    b.set_tapped(oid(1), true).unwrap();
    b.set_flipped(oid(1), false).unwrap();
    b.leave(oid(9)).unwrap();

    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
}
