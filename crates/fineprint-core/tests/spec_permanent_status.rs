// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::permanent`. Black-box: written from the work-item brief only.
//!
//! Permanent status and designations: defaults on entering, each status setter and flag
//! independence, open designations, unknown-id errors, identifier edge values and clone
//! independence (REQ-ZONE-001 item 3; REQ-CORE-006).

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::permanent::{
    AttachTarget, AttackState, AttackTarget, CombatState, PermanentError, Permanents, Status,
};
use fineprint_core::state::DesignationValue;

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

/// A container holding `ids`, each controlled by player 1 since turn 1.
fn holding(ids: &[u64]) -> Permanents {
    let mut p = Permanents::new();
    for &i in ids {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    p
}

fn status_of(p: &Permanents, id: u64) -> Status {
    p.get(oid(id)).unwrap().status()
}

fn set_flag(p: &mut Permanents, flag: usize, id: ObjectId, value: bool) {
    match flag {
        0 => p.set_tapped(id, value).unwrap(),
        1 => p.set_flipped(id, value).unwrap(),
        2 => p.set_face_down(id, value).unwrap(),
        _ => p.set_phased_out(id, value).unwrap(),
    }
}

fn flag_value(s: Status, flag: usize) -> bool {
    match flag {
        0 => s.tapped,
        1 => s.flipped,
        2 => s.face_down,
        _ => s.phased_out,
    }
}

// REQ-ZONE-001 item 3; CR 110.5b: permanents enter untapped, unflipped, face up and phased in.
#[test]
fn req_zone_001_status_default_is_all_false() {
    let s = Status::default();
    assert!(!s.tapped);
    assert!(!s.flipped);
    assert!(!s.face_down);
    assert!(!s.phased_out);
}

// REQ-ZONE-001 item 3; CR 110.5b.
#[test]
fn req_zone_001_status_defaults_on_enter() {
    let mut p = Permanents::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    p.enter(oid(7), pid(3), 12).unwrap();
    let perm = p.get(oid(7)).unwrap();
    assert_eq!(perm.id(), oid(7));
    assert_eq!(perm.controller(), pid(3));
    assert_eq!(perm.status(), Status::default());
    assert!(perm.designations().is_empty());
    assert_eq!(perm.designation("test.marker_a"), None);
    assert_eq!(perm.controlled_since_turn(), 12);
    assert_eq!(perm.attached_to(), None);
    assert_eq!(perm.combat(), &CombatState::default());
    assert_eq!(p.attached_to(oid(7)), None);
    assert_eq!(p.len(), 1);
    assert!(!p.is_empty());
}

// REQ-ZONE-001 item 3: CombatState and AttackState defaults as the contract states.
#[test]
fn req_zone_001_combat_state_default() {
    let c = CombatState::default();
    assert_eq!(c.attack, AttackState::NotAttacking);
    assert_eq!(AttackState::default(), AttackState::NotAttacking);
    assert!(c.blocking.is_empty());
    assert!(!c.blocked);
    assert!(!c.removed_from_combat);
}

// REQ-ZONE-001 item 3: new() and default() agree; ids() ascending regardless of insertion order.
#[test]
fn req_zone_001_new_default_len_and_ascending_ids() {
    assert_eq!(Permanents::new(), Permanents::default());
    let p = holding(&[30, 5, 900, 1, 77]);
    assert_eq!(p.len(), 5);
    assert_eq!(p.ids(), vec![oid(1), oid(5), oid(30), oid(77), oid(900)]);
}

// REQ-ZONE-001 item 3: an id already held cannot enter again; container unchanged.
#[test]
fn req_zone_001_enter_already_present() {
    let mut p = holding(&[4]);
    p.set_tapped(oid(4), true).unwrap();
    let before = p.clone();
    assert_eq!(
        p.enter(oid(4), pid(2), 9),
        Err(PermanentError::AlreadyPresent(oid(4)))
    );
    assert_eq!(p, before);
}

// REQ-ZONE-001 item 3; CR 110.5, 110.5c: each setter sets and clears its own flag only.
#[test]
fn req_zone_001_each_status_setter_round_trips() {
    for flag in 0..4 {
        let mut p = holding(&[1, 2]);
        set_flag(&mut p, flag, oid(1), true);
        let s = status_of(&p, 1);
        for other in 0..4 {
            assert_eq!(
                flag_value(s, other),
                other == flag,
                "flag {flag} other {other}"
            );
        }
        assert_eq!(status_of(&p, 2), Status::default());
        // Setting true twice keeps it true (CR 110.5c: retained until changed).
        set_flag(&mut p, flag, oid(1), true);
        assert!(flag_value(status_of(&p, 1), flag));
        set_flag(&mut p, flag, oid(1), false);
        assert_eq!(status_of(&p, 1), Status::default());
        set_flag(&mut p, flag, oid(1), false);
        assert_eq!(status_of(&p, 1), Status::default());
    }
}

// REQ-ZONE-001 item 3; CR 110.5: four independent categories; every one of 16 combinations,
// reached from every other combination, holds exactly.
#[test]
fn req_zone_001_status_flags_independent_all_combinations() {
    let mut p = holding(&[1]);
    for from in 0u8..16 {
        for to in 0u8..16 {
            for flag in 0..4 {
                set_flag(&mut p, flag, oid(1), from & (1 << flag) != 0);
            }
            for flag in 0..4 {
                set_flag(&mut p, flag, oid(1), to & (1 << flag) != 0);
            }
            let s = status_of(&p, 1);
            for flag in 0..4 {
                assert_eq!(flag_value(s, flag), to & (1 << flag) != 0, "{from}->{to}");
            }
        }
    }
}

// REQ-ZONE-001 item 3; CR 506.4b: tapping, flipping or turning face down never touches combat.
#[test]
fn req_zone_001_set_tapped_leaves_combat_alone() {
    let mut p = holding(&[1, 2, 3]);
    p.set_attack(
        oid(1),
        AttackState::Attacking(Some(AttackTarget::Player(pid(2)))),
    )
    .unwrap();
    p.set_blocked(oid(1), true).unwrap();
    p.set_blocking(oid(2), &[oid(1)]).unwrap();
    let combat1 = p.get(oid(1)).unwrap().combat().clone();
    let combat2 = p.get(oid(2)).unwrap().combat().clone();
    for flag in 0..3 {
        for &v in &[true, false, true] {
            set_flag(&mut p, flag, oid(1), v);
            set_flag(&mut p, flag, oid(2), v);
            assert_eq!(p.get(oid(1)).unwrap().combat(), &combat1);
            assert_eq!(p.get(oid(2)).unwrap().combat(), &combat2);
        }
    }
    assert!(!p.get(oid(1)).unwrap().combat().removed_from_combat);
}

// REQ-ZONE-001 item 3; REQ-CORE-006: designations of each value kind, set, read, replaced.
#[test]
fn req_zone_001_designation_kinds_and_replace() {
    let mut p = holding(&[1]);
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Flag)
        .unwrap();
    p.set_designation(oid(1), "test.marker_b", DesignationValue::Int(-5))
        .unwrap();
    p.set_designation(
        oid(1),
        "test.marker_c",
        DesignationValue::Text("alpha".to_string()),
    )
    .unwrap();
    let perm = p.get(oid(1)).unwrap();
    assert_eq!(
        perm.designation("test.marker_a"),
        Some(&DesignationValue::Flag)
    );
    assert_eq!(
        perm.designation("test.marker_b"),
        Some(&DesignationValue::Int(-5))
    );
    assert_eq!(
        perm.designation("test.marker_c"),
        Some(&DesignationValue::Text("alpha".to_string()))
    );
    assert_eq!(perm.designation("test.marker_d"), None);
    assert_eq!(perm.designations().len(), 3);

    // Setting twice replaces, including across value kinds.
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Int(i64::MIN))
        .unwrap();
    p.set_designation(oid(1), "test.marker_b", DesignationValue::Int(i64::MAX))
        .unwrap();
    p.set_designation(oid(1), "test.marker_c", DesignationValue::Flag)
        .unwrap();
    let perm = p.get(oid(1)).unwrap();
    assert_eq!(
        perm.designation("test.marker_a"),
        Some(&DesignationValue::Int(i64::MIN))
    );
    assert_eq!(
        perm.designation("test.marker_b"),
        Some(&DesignationValue::Int(i64::MAX))
    );
    assert_eq!(
        perm.designation("test.marker_c"),
        Some(&DesignationValue::Flag)
    );
    assert_eq!(perm.designations().len(), 3);
}

// REQ-ZONE-001 item 3; REQ-CORE-006: clear returns the old value, or None when absent.
#[test]
fn req_zone_001_designation_clear() {
    let mut p = holding(&[1]);
    assert_eq!(p.clear_designation(oid(1), "test.marker_a"), Ok(None));
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Int(3))
        .unwrap();
    p.set_designation(oid(1), "test.marker_b", DesignationValue::Flag)
        .unwrap();
    assert_eq!(
        p.clear_designation(oid(1), "test.marker_a"),
        Ok(Some(DesignationValue::Int(3)))
    );
    assert_eq!(p.clear_designation(oid(1), "test.marker_a"), Ok(None));
    let perm = p.get(oid(1)).unwrap();
    assert_eq!(perm.designation("test.marker_a"), None);
    assert_eq!(
        perm.designation("test.marker_b"),
        Some(&DesignationValue::Flag)
    );
    assert_eq!(perm.designations().len(), 1);
}

// REQ-ZONE-001 item 3; REQ-CORE-006: designations() iterates in ascending key order.
#[test]
fn req_zone_001_designations_ascending_key_order() {
    let mut p = holding(&[1]);
    for key in ["test.z", "test.a", "test.m", "TEST.upper", "test.a2", ""] {
        p.set_designation(oid(1), key, DesignationValue::Flag)
            .unwrap();
    }
    let keys: Vec<&str> = p
        .get(oid(1))
        .unwrap()
        .designations()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["", "TEST.upper", "test.a", "test.a2", "test.m", "test.z"]
    );
}

// REQ-ZONE-001 item 3; REQ-CORE-006: designations are independent per key and per permanent.
#[test]
fn req_zone_001_designations_independent_per_key_and_permanent() {
    let mut p = holding(&[1, 2]);
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Int(1))
        .unwrap();
    p.set_designation(oid(2), "test.marker_a", DesignationValue::Int(2))
        .unwrap();
    p.set_designation(oid(1), "test.marker_b", DesignationValue::Flag)
        .unwrap();
    assert_eq!(
        p.get(oid(2)).unwrap().designation("test.marker_a"),
        Some(&DesignationValue::Int(2))
    );
    assert_eq!(p.get(oid(2)).unwrap().designation("test.marker_b"), None);
    p.clear_designation(oid(1), "test.marker_a").unwrap();
    assert_eq!(
        p.get(oid(2)).unwrap().designation("test.marker_a"),
        Some(&DesignationValue::Int(2))
    );
    assert_eq!(
        p.get(oid(1)).unwrap().designation("test.marker_b"),
        Some(&DesignationValue::Flag)
    );
    // Designations do not alter status.
    assert_eq!(status_of(&p, 1), Status::default());
}

// REQ-ZONE-001 item 3; REQ-CORE-006: any string key works: empty, 1000 chars, unicode.
#[test]
fn req_zone_001_designation_odd_keys() {
    let long: String = "k".repeat(1000);
    let unicode = "test.\u{00e9}\u{4e2d}\u{1f600}\u{0000}";
    let mut p = holding(&[1]);
    for (i, key) in ["", long.as_str(), unicode, " "].iter().enumerate() {
        p.set_designation(oid(1), key, DesignationValue::Int(i as i64))
            .unwrap();
    }
    let perm = p.get(oid(1)).unwrap();
    assert_eq!(perm.designations().len(), 4);
    assert_eq!(perm.designation(""), Some(&DesignationValue::Int(0)));
    assert_eq!(
        perm.designation(long.as_str()),
        Some(&DesignationValue::Int(1))
    );
    assert_eq!(perm.designation(unicode), Some(&DesignationValue::Int(2)));
    assert_eq!(perm.designation(" "), Some(&DesignationValue::Int(3)));
    assert_eq!(
        p.clear_designation(oid(1), long.as_str()),
        Ok(Some(DesignationValue::Int(1)))
    );
    assert_eq!(
        p.clear_designation(oid(1), unicode),
        Ok(Some(DesignationValue::Int(2)))
    );
    // Text values may be long or unicode too.
    p.set_designation(oid(1), "", DesignationValue::Text(long.clone()))
        .unwrap();
    p.set_designation(oid(1), " ", DesignationValue::Text(unicode.to_string()))
        .unwrap();
    let perm = p.get(oid(1)).unwrap();
    assert_eq!(perm.designation(""), Some(&DesignationValue::Text(long)));
    assert_eq!(
        perm.designation(" "),
        Some(&DesignationValue::Text(unicode.to_string()))
    );
}

// REQ-ZONE-001 item 3; REQ-CORE-006: every id-taking method returns UnknownObject for an
// unheld id and leaves the container unchanged.
#[test]
fn req_zone_001_unknown_id_errors_leave_container_unchanged() {
    let mut p = holding(&[1, 2]);
    p.set_tapped(oid(1), true).unwrap();
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Flag)
        .unwrap();
    p.attach(oid(2), AttachTarget::Permanent(oid(1))).unwrap();
    p.set_attack(oid(1), AttackState::Attacking(None)).unwrap();
    let before = p.clone();
    for raw in [0u64, 3, 99, u64::MAX] {
        let u = oid(raw);
        let e = Err(PermanentError::UnknownObject(u));
        assert_eq!(p.set_tapped(u, true), e);
        assert_eq!(p.set_flipped(u, true), e);
        assert_eq!(p.set_face_down(u, true), e);
        assert_eq!(p.set_phased_out(u, true), e);
        assert_eq!(p.set_phased_out(u, false), e);
        assert_eq!(
            p.set_designation(u, "test.marker_a", DesignationValue::Flag),
            e
        );
        assert_eq!(
            p.clear_designation(u, "test.marker_a"),
            Err(PermanentError::UnknownObject(u))
        );
        assert_eq!(p.set_controller(u, pid(2), 5), e);
        assert_eq!(p.attach(u, AttachTarget::Player(pid(1))), e);
        assert_eq!(p.detach(u), Err(PermanentError::UnknownObject(u)));
        assert_eq!(p.set_attack(u, AttackState::NotAttacking), e);
        assert_eq!(p.set_blocking(u, &[]), e);
        assert_eq!(p.set_blocked(u, true), e);
        assert_eq!(p.remove_from_combat(u), e);
        assert_eq!(p.clear_combat(u), e);
        assert_eq!(
            p.leave(u).map(|perm| perm.id()),
            Err(PermanentError::UnknownObject(u))
        );
        assert!(p.get(u).is_none());
        assert_eq!(p.attached_to(u), None);
        assert_eq!(p, before);
    }
}

// REQ-ZONE-001 item 3; REQ-CORE-006: extreme identifier, player and turn values never panic.
#[test]
fn req_zone_001_id_edge_values() {
    let mut p = Permanents::new();
    p.enter(oid(0), pid(0), 0).unwrap();
    p.enter(oid(u64::MAX), pid(u64::MAX), u64::MAX).unwrap();
    assert_eq!(p.ids(), vec![oid(0), oid(u64::MAX)]);
    let hi = p.get(oid(u64::MAX)).unwrap();
    assert_eq!(hi.controller(), pid(u64::MAX));
    assert_eq!(hi.controlled_since_turn(), u64::MAX);
    assert!(!hi.controlled_continuously_since_start_of(u64::MAX));
    let lo = p.get(oid(0)).unwrap();
    assert_eq!(lo.controller(), pid(0));
    assert_eq!(lo.controlled_since_turn(), 0);
    assert!(!lo.controlled_continuously_since_start_of(0));
    assert!(lo.controlled_continuously_since_start_of(u64::MAX));
    for flag in 0..4 {
        set_flag(&mut p, flag, oid(0), true);
        set_flag(&mut p, flag, oid(u64::MAX), true);
    }
    assert!(status_of(&p, 0).phased_out && status_of(&p, u64::MAX).tapped);
    p.set_controller(oid(0), pid(u64::MAX), u64::MAX).unwrap();
    assert_eq!(p.get(oid(0)).unwrap().controlled_since_turn(), u64::MAX);
    p.attach(oid(0), AttachTarget::Permanent(oid(u64::MAX)))
        .unwrap();
    p.attach(oid(u64::MAX), AttachTarget::Player(pid(0)))
        .unwrap();
    assert_eq!(
        p.attachments_of(AttachTarget::Permanent(oid(u64::MAX))),
        vec![oid(0)]
    );
    let left = p.leave(oid(u64::MAX)).unwrap();
    assert_eq!(left.id(), oid(u64::MAX));
    assert_eq!(p.attached_to(oid(0)), None);
}

// REQ-ZONE-001 item 3: a clone is equal, and mutating it leaves the original unchanged.
#[test]
fn req_zone_001_clone_independence() {
    let mut p = holding(&[1, 2]);
    p.set_flipped(oid(1), true).unwrap();
    p.set_designation(oid(2), "test.marker_a", DesignationValue::Int(4))
        .unwrap();
    let snapshot = p.clone();
    let mut c = p.clone();
    assert_eq!(c, p);
    c.set_tapped(oid(1), true).unwrap();
    c.set_designation(oid(2), "test.marker_a", DesignationValue::Int(5))
        .unwrap();
    c.enter(oid(3), pid(2), 2).unwrap();
    c.leave(oid(1)).unwrap();
    assert_ne!(c, p);
    assert_eq!(p, snapshot);
    assert!(!status_of(&p, 1).tapped);
    assert_eq!(
        p.get(oid(2)).unwrap().designation("test.marker_a"),
        Some(&DesignationValue::Int(4))
    );
    assert!(p.get(oid(3)).is_none());
}

// REQ-ZONE-001 item 3: leave returns the pre-removal permanent, including its status and
// designations; re-entering gives a fresh default permanent (CR 400.7).
#[test]
fn req_zone_001_leave_returns_status_and_reenter_is_fresh() {
    let mut p = holding(&[1]);
    p.set_tapped(oid(1), true).unwrap();
    p.set_face_down(oid(1), true).unwrap();
    p.set_designation(oid(1), "test.marker_a", DesignationValue::Flag)
        .unwrap();
    let left = p.leave(oid(1)).unwrap();
    assert_eq!(left.id(), oid(1));
    assert!(left.status().tapped && left.status().face_down);
    assert_eq!(
        left.designation("test.marker_a"),
        Some(&DesignationValue::Flag)
    );
    assert!(p.is_empty());
    p.enter(oid(1), pid(1), 1).unwrap();
    assert_eq!(p, holding(&[1]));
    assert_eq!(status_of(&p, 1), Status::default());
    assert!(p.get(oid(1)).unwrap().designations().is_empty());
}

// REQ-ZONE-001 item 3: PermanentError is Copy, Eq, and every variant displays non-empty text.
#[test]
fn req_zone_001_error_display_and_derives() {
    let errors = [
        PermanentError::UnknownObject(oid(1)),
        PermanentError::AlreadyPresent(oid(2)),
        PermanentError::SelfAttach(oid(3)),
        PermanentError::AttachmentCycle {
            object: oid(4),
            target: oid(5),
        },
    ];
    for (i, e) in errors.iter().enumerate() {
        let copy = *e;
        assert_eq!(copy, *e);
        assert!(!e.to_string().is_empty());
        assert!(!format!("{e:?}").is_empty());
        for (j, other) in errors.iter().enumerate() {
            assert_eq!(i == j, e == other);
        }
    }
    assert_ne!(
        PermanentError::UnknownObject(oid(1)),
        PermanentError::UnknownObject(oid(2))
    );
}
