// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of the permanent container (REQ-ZONE-001, REQ-CORE-006, REQ-NFR-008).

use std::collections::BTreeSet;

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;

use super::*;
use crate::state::DesignationValue;

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn perm(c: &Permanents, n: u64) -> &Permanent {
    c.get(o(n)).expect("held")
}

/// Permanents 1..=n, each controlled by player 1 since turn 1.
fn with(n: u64) -> Permanents {
    let mut c = Permanents::new();
    for i in 1..=n {
        c.enter(o(i), p(1), 1).unwrap();
    }
    c
}

fn bytes(c: &Permanents) -> Vec<u8> {
    encode_to_vec(c)
}

// ---- construction and lookup ----

#[test]
fn new_and_default_are_empty() {
    // REQ-ZONE-001, REQ-NFR-008
    let c = Permanents::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert!(c.ids().is_empty());
    assert_eq!(c, Permanents::default());
    assert!(c.get(o(1)).is_none());
}

#[test]
fn enter_gives_defaults() {
    // REQ-ZONE-001 item 3; CR 110.2, CR 110.5b
    let mut c = Permanents::new();
    c.enter(o(7), p(2), 4).unwrap();
    let x = perm(&c, 7);
    assert_eq!(x.id(), o(7));
    assert_eq!(x.controller(), p(2));
    assert_eq!(x.status(), Status::default());
    assert_eq!(
        x.status(),
        Status {
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false
        }
    );
    assert!(x.designations().is_empty());
    assert_eq!(x.controlled_since_turn(), 4);
    assert_eq!(x.attached_to(), None);
    assert_eq!(x.combat(), &CombatState::default());
    let d = CombatState::default();
    assert_eq!(d.attack, AttackState::NotAttacking);
    assert!(d.blocking.is_empty());
    assert!(!d.blocked);
    assert!(!d.removed_from_combat);
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
}

#[test]
fn enter_already_present_leaves_container_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(1);
    c.set_tapped(o(1), true).unwrap();
    let before = c.clone();
    assert_eq!(
        c.enter(o(1), p(2), 9),
        Err(PermanentError::AlreadyPresent(o(1)))
    );
    assert_eq!(c, before);
}

#[test]
fn enter_accepts_extreme_values() {
    // REQ-ZONE-001, REQ-NFR-008 (R12: no panic on odd arguments)
    let mut c = Permanents::new();
    c.enter(o(u64::MAX), p(u64::MAX), u64::MAX).unwrap();
    c.enter(o(0), p(0), 0).unwrap();
    assert_eq!(perm(&c, u64::MAX).controlled_since_turn(), u64::MAX);
    assert_eq!(c.ids(), vec![o(0), o(u64::MAX)]);
}

#[test]
fn ids_are_ascending_regardless_of_insertion_order() {
    // REQ-CORE-006
    let mut c = Permanents::new();
    for n in [5, 1, 9, 3] {
        c.enter(o(n), p(1), 1).unwrap();
    }
    assert_eq!(c.ids(), vec![o(1), o(3), o(5), o(9)]);
    assert_eq!(c.len(), 4);
}

#[test]
fn leave_returns_pre_removal_value() {
    // REQ-ZONE-001; CR 110.1, CR 400.7
    let mut c = with(2);
    c.set_tapped(o(1), true).unwrap();
    c.set_designation(o(1), "Monstrous", DesignationValue::Flag)
        .unwrap();
    c.attach(o(1), AttachTarget::Permanent(o(2))).unwrap();
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    let expected = perm(&c, 1).clone();
    let left = c.leave(o(1)).unwrap();
    assert_eq!(left, expected);
    assert!(c.get(o(1)).is_none());
    assert_eq!(c.ids(), vec![o(2)]);
}

#[test]
fn leave_unknown_leaves_container_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(2);
    let before = c.clone();
    assert_eq!(c.leave(o(3)), Err(PermanentError::UnknownObject(o(3))));
    assert_eq!(c, before);
}

#[test]
fn leave_then_reenter_gives_fresh_defaults() {
    // REQ-ZONE-001; CR 400.7, CR 110.5b
    let mut c = with(2);
    c.set_tapped(o(1), true).unwrap();
    c.set_flipped(o(1), true).unwrap();
    c.set_face_down(o(1), true).unwrap();
    c.set_designation(o(1), "Renowned", DesignationValue::Flag)
        .unwrap();
    c.attach(o(1), AttachTarget::Permanent(o(2))).unwrap();
    c.set_attack(o(1), AttackState::Attacking(None)).unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.leave(o(1)).unwrap();
    c.enter(o(1), p(2), 8).unwrap();
    let x = perm(&c, 1);
    assert_eq!(x.status(), Status::default());
    assert!(x.designations().is_empty());
    assert_eq!(x.attached_to(), None);
    assert_eq!(x.combat(), &CombatState::default());
    assert_eq!(x.controller(), p(2));
    assert_eq!(x.controlled_since_turn(), 8);
}

#[test]
fn leave_unattaches_everything_attached_to_it() {
    // REQ-ZONE-001; CR 701.3d
    let mut c = with(4);
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    c.attach(o(3), AttachTarget::Permanent(o(1))).unwrap();
    c.attach(o(4), AttachTarget::Player(p(1))).unwrap();
    c.leave(o(1)).unwrap();
    assert_eq!(c.attached_to(o(2)), None);
    assert_eq!(c.attached_to(o(3)), None);
    assert_eq!(c.attached_to(o(4)), Some(AttachTarget::Player(p(1))));
    assert!(c.attachments_of(AttachTarget::Permanent(o(1))).is_empty());
}

#[test]
fn leave_of_an_attachment_keeps_its_host_unchanged() {
    // REQ-ZONE-001; CR 701.3d
    let mut c = with(2);
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    let host = perm(&c, 1).clone();
    c.leave(o(2)).unwrap();
    assert_eq!(perm(&c, 1), &host);
}

#[test]
fn leave_rewrites_combat_of_others() {
    // REQ-ZONE-001; CR 506.4, CR 506.4c, CR 509.1h
    // 1: planeswalker being attacked; 2: attacks 1; 3: attacks a player; 4: blocks 5 and 1;
    // 5: attacker, blocked by 4 and 6; 6: blocks 5.
    let mut c = with(6);
    c.set_attack(
        o(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    c.set_attack(
        o(3),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_attack(
        o(5),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(5), true).unwrap();
    c.set_blocking(o(4), &[o(5), o(1)]).unwrap();
    c.set_blocking(o(6), &[o(5)]).unwrap();

    c.leave(o(1)).unwrap();
    assert_eq!(perm(&c, 2).combat().attack, AttackState::Attacking(None));
    assert!(!perm(&c, 2).combat().removed_from_combat);
    assert_eq!(
        perm(&c, 3).combat().attack,
        AttackState::Attacking(Some(AttackTarget::Player(p(2))))
    );
    assert_eq!(perm(&c, 4).combat().blocking, BTreeSet::from([o(5)]));

    // Both blockers of 5 leave: 5 stays blocked (CR 509.1h).
    c.leave(o(4)).unwrap();
    c.leave(o(6)).unwrap();
    assert!(perm(&c, 5).combat().blocked);
    assert_eq!(
        perm(&c, 5).combat().attack,
        AttackState::Attacking(Some(AttackTarget::Player(p(2))))
    );

    // An attacker leaving drops out of its blockers' sets.
    let mut d = with(3);
    d.set_attack(o(1), AttackState::Attacking(None)).unwrap();
    d.set_blocking(o(2), &[o(1), o(3)]).unwrap();
    d.leave(o(1)).unwrap();
    assert_eq!(perm(&d, 2).combat().blocking, BTreeSet::from([o(3)]));
    assert!(!perm(&d, 2).combat().removed_from_combat);
}

// ---- status ----

#[test]
fn status_flags_are_independent() {
    // REQ-ZONE-001 item 3; CR 110.5, CR 110.5c
    let mut c = with(1);
    c.set_tapped(o(1), true).unwrap();
    assert_eq!(
        perm(&c, 1).status(),
        Status {
            tapped: true,
            ..Status::default()
        }
    );
    c.set_flipped(o(1), true).unwrap();
    c.set_face_down(o(1), true).unwrap();
    c.set_phased_out(o(1), true).unwrap();
    assert_eq!(
        perm(&c, 1).status(),
        Status {
            tapped: true,
            flipped: true,
            face_down: true,
            phased_out: true
        }
    );
    c.set_tapped(o(1), false).unwrap();
    assert_eq!(
        perm(&c, 1).status(),
        Status {
            tapped: false,
            flipped: true,
            face_down: true,
            phased_out: true
        }
    );
    c.set_flipped(o(1), false).unwrap();
    c.set_face_down(o(1), false).unwrap();
    c.set_phased_out(o(1), false).unwrap();
    assert_eq!(perm(&c, 1).status(), Status::default());
}

#[test]
fn status_setters_reject_unknown_ids_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(1);
    let before = c.clone();
    let e = Err(PermanentError::UnknownObject(o(9)));
    assert_eq!(c.set_tapped(o(9), true), e);
    assert_eq!(c.set_flipped(o(9), true), e);
    assert_eq!(c.set_face_down(o(9), true), e);
    assert_eq!(c.set_phased_out(o(9), true), e);
    assert_eq!(c, before);
}

#[test]
fn set_tapped_never_touches_combat() {
    // REQ-ZONE-001; CR 506.4b
    let mut c = with(3);
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.set_blocking(o(2), &[o(1)]).unwrap();
    let combat1 = perm(&c, 1).combat().clone();
    let combat2 = perm(&c, 2).combat().clone();
    for v in [true, false, true] {
        c.set_tapped(o(1), v).unwrap();
        c.set_tapped(o(2), v).unwrap();
        assert_eq!(perm(&c, 1).combat(), &combat1);
        assert_eq!(perm(&c, 2).combat(), &combat2);
    }
}

// ---- phasing ----

#[test]
fn phasing_out_removes_from_combat() {
    // REQ-ZONE-001; CR 702.26b, CR 506.4, CR 506.4c
    let mut c = with(3);
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.set_blocking(o(1), &[o(2)]).unwrap();
    c.set_attack(
        o(3),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    c.set_blocking(o(2), &[o(1)]).unwrap();
    let mut expected = c.clone();
    expected.remove_from_combat(o(1)).unwrap();
    c.set_phased_out(o(1), true).unwrap();
    let combat = perm(&c, 1).combat();
    assert_eq!(combat.attack, AttackState::NotAttacking);
    assert!(combat.blocking.is_empty());
    assert!(!combat.blocked);
    assert!(combat.removed_from_combat);
    assert_eq!(perm(&c, 3).combat().attack, AttackState::Attacking(None));
    assert_eq!(perm(&c, 2).combat().blocking, BTreeSet::from([o(1)]));
    // Exactly remove_from_combat plus the flag.
    expected.set_phased_out(o(1), true).unwrap();
    assert_eq!(c, expected);
}

#[test]
fn phasing_without_a_false_to_true_transition_changes_only_the_flag() {
    // REQ-ZONE-001; CR 702.26b
    let mut c = with(2);
    c.set_phased_out(o(1), true).unwrap();
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.set_blocking(o(1), &[o(2)]).unwrap();
    c.set_attack(
        o(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    let before = c.clone();
    // true -> true
    c.set_phased_out(o(1), true).unwrap();
    assert_eq!(c, before);
    // true -> false
    c.set_phased_out(o(1), false).unwrap();
    let mut expected = before.clone();
    expected
        .permanents
        .get_mut(&o(1))
        .unwrap()
        .status
        .phased_out = false;
    assert_eq!(c, expected);
    // false -> false
    let before = c.clone();
    c.set_phased_out(o(1), false).unwrap();
    assert_eq!(c, before);
}

#[test]
fn phasing_never_changes_control_tracking() {
    // REQ-ZONE-001 item 3; CR 702.26d
    let mut c = Permanents::new();
    c.enter(o(1), p(3), 5).unwrap();
    for v in [true, true, false, false, true, false] {
        c.set_phased_out(o(1), v).unwrap();
        assert_eq!(perm(&c, 1).controller(), p(3));
        assert_eq!(perm(&c, 1).controlled_since_turn(), 5);
    }
}

// ---- designations ----

#[test]
fn designations_are_an_open_ordered_registry() {
    // REQ-ZONE-001 item 3
    let mut c = with(1);
    c.set_designation(o(1), "Suspected", DesignationValue::Flag)
        .unwrap();
    c.set_designation(o(1), "Goaded", DesignationValue::Int(2))
        .unwrap();
    c.set_designation(o(1), "zz-set-defined", DesignationValue::Text("x".into()))
        .unwrap();
    c.set_designation(o(1), "", DesignationValue::Int(i64::MIN))
        .unwrap();
    let keys: Vec<&str> = perm(&c, 1)
        .designations()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["", "Goaded", "Suspected", "zz-set-defined"]);
    assert_eq!(
        perm(&c, 1).designation("Goaded"),
        Some(&DesignationValue::Int(2))
    );
    assert_eq!(perm(&c, 1).designation("absent"), None);
    // Replace.
    c.set_designation(o(1), "Goaded", DesignationValue::Int(3))
        .unwrap();
    assert_eq!(
        perm(&c, 1).designation("Goaded"),
        Some(&DesignationValue::Int(3))
    );
    // Clear.
    assert_eq!(
        c.clear_designation(o(1), "Goaded"),
        Ok(Some(DesignationValue::Int(3)))
    );
    assert_eq!(c.clear_designation(o(1), "Goaded"), Ok(None));
    assert_eq!(
        c.clear_designation(o(1), ""),
        Ok(Some(DesignationValue::Int(i64::MIN)))
    );
    assert_eq!(perm(&c, 1).designations().len(), 2);
}

#[test]
fn designation_ops_reject_unknown_ids_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(1);
    let before = c.clone();
    let e = PermanentError::UnknownObject(o(2));
    assert_eq!(
        c.set_designation(o(2), "Saddled", DesignationValue::Flag),
        Err(e)
    );
    assert_eq!(c.clear_designation(o(2), "Saddled"), Err(e));
    assert_eq!(c, before);
}

// ---- control ----

#[test]
fn continuity_predicate_is_strict() {
    // REQ-ZONE-001 item 3; CR 302.6
    let mut c = Permanents::new();
    c.enter(o(1), p(1), 10).unwrap();
    let x = perm(&c, 1);
    assert!(!x.controlled_continuously_since_start_of(9));
    assert!(!x.controlled_continuously_since_start_of(10));
    assert!(x.controlled_continuously_since_start_of(11));
    assert!(!x.controlled_continuously_since_start_of(0));
    assert!(x.controlled_continuously_since_start_of(u64::MAX));

    let mut d = Permanents::new();
    d.enter(o(1), p(1), 0).unwrap();
    d.enter(o(2), p(1), u64::MAX).unwrap();
    assert!(!perm(&d, 1).controlled_continuously_since_start_of(0));
    assert!(perm(&d, 1).controlled_continuously_since_start_of(1));
    assert!(!perm(&d, 2).controlled_continuously_since_start_of(u64::MAX));
    assert!(!perm(&d, 2).controlled_continuously_since_start_of(0));
}

#[test]
fn control_change_resets_tracking() {
    // REQ-ZONE-001 item 3; CR 613.1b, CR 302.6
    let mut c = Permanents::new();
    c.enter(o(1), p(1), 2).unwrap();
    assert!(perm(&c, 1).controlled_continuously_since_start_of(6));
    c.set_controller(o(1), p(2), 6).unwrap();
    assert_eq!(perm(&c, 1).controller(), p(2));
    assert_eq!(perm(&c, 1).controlled_since_turn(), 6);
    assert!(!perm(&c, 1).controlled_continuously_since_start_of(6));
    assert!(perm(&c, 1).controlled_continuously_since_start_of(7));
    // A smaller or equal turn is stored as given.
    c.set_controller(o(1), p(3), 1).unwrap();
    assert_eq!(perm(&c, 1).controlled_since_turn(), 1);
    c.set_controller(o(1), p(4), 1).unwrap();
    assert_eq!(perm(&c, 1).controller(), p(4));
    assert_eq!(perm(&c, 1).controlled_since_turn(), 1);
}

#[test]
fn control_round_trip_resets_twice() {
    // REQ-ZONE-001 item 3; CR 613.1b
    let mut c = Permanents::new();
    c.enter(o(1), p(1), 1).unwrap();
    c.set_controller(o(1), p(2), 3).unwrap();
    assert_eq!(perm(&c, 1).controlled_since_turn(), 3);
    c.set_controller(o(1), p(1), 5).unwrap();
    assert_eq!(perm(&c, 1).controller(), p(1));
    assert_eq!(perm(&c, 1).controlled_since_turn(), 5);
    assert!(!perm(&c, 1).controlled_continuously_since_start_of(5));
}

#[test]
fn same_controller_is_a_complete_no_op() {
    // REQ-ZONE-001 item 3
    let mut c = with(2);
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(2)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    let before = c.clone();
    c.set_controller(o(1), p(1), 99).unwrap();
    assert_eq!(c, before);
    assert_eq!(bytes(&c), bytes(&before));
}

#[test]
fn set_controller_unknown_leaves_container_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(1);
    let before = c.clone();
    assert_eq!(
        c.set_controller(o(2), p(2), 3),
        Err(PermanentError::UnknownObject(o(2)))
    );
    assert_eq!(c, before);
}

#[test]
fn set_controller_removes_from_combat() {
    // REQ-ZONE-001; CR 506.4, CR 506.4c, CR 509.1h
    let mut c = with(4);
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(4)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.set_blocking(o(1), &[o(2)]).unwrap();
    c.set_blocking(o(3), &[o(1)]).unwrap();
    c.set_attack(
        o(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    let mut expected = c.clone();
    expected.remove_from_combat(o(1)).unwrap();
    c.set_controller(o(1), p(2), 4).unwrap();
    let combat = perm(&c, 1).combat();
    assert_eq!(combat.attack, AttackState::NotAttacking);
    assert!(combat.removed_from_combat);
    assert!(combat.blocking.is_empty());
    assert!(!combat.blocked);
    assert_eq!(perm(&c, 2).combat().attack, AttackState::Attacking(None));
    assert_eq!(perm(&c, 3).combat().blocking, BTreeSet::from([o(1)]));
    // Exactly remove_from_combat plus the control fields.
    let e = expected.permanents.get_mut(&o(1)).unwrap();
    e.controller = p(2);
    e.controlled_since = 4;
    assert_eq!(c, expected);
}

// ---- attachments ----

#[test]
fn attach_detach_and_queries() {
    // REQ-ZONE-001 item 3; CR 701.3b
    let mut c = with(4);
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    c.attach(o(4), AttachTarget::Permanent(o(1))).unwrap();
    c.attach(o(3), AttachTarget::Player(p(7))).unwrap();
    assert_eq!(c.attached_to(o(2)), Some(AttachTarget::Permanent(o(1))));
    assert_eq!(
        perm(&c, 2).attached_to(),
        Some(AttachTarget::Permanent(o(1)))
    );
    assert_eq!(c.attached_to(o(1)), None);
    assert_eq!(c.attached_to(o(99)), None);
    assert_eq!(
        c.attachments_of(AttachTarget::Permanent(o(1))),
        vec![o(2), o(4)]
    );
    assert_eq!(c.attachments_of(AttachTarget::Player(p(7))), vec![o(3)]);
    assert!(c.attachments_of(AttachTarget::Player(p(1))).is_empty());
    assert!(c.attachments_of(AttachTarget::Permanent(o(99))).is_empty());

    // Same target: no change (CR 701.3b).
    let before = c.clone();
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    assert_eq!(c, before);

    // Replacing the link.
    c.attach(o(2), AttachTarget::Permanent(o(3))).unwrap();
    assert_eq!(c.attachments_of(AttachTarget::Permanent(o(1))), vec![o(4)]);
    assert_eq!(c.attachments_of(AttachTarget::Permanent(o(3))), vec![o(2)]);

    assert_eq!(c.detach(o(2)), Ok(Some(AttachTarget::Permanent(o(3)))));
    assert_eq!(c.detach(o(2)), Ok(None));
    assert_eq!(c.attached_to(o(2)), None);
}

#[test]
fn attach_error_order_and_unchanged_container() {
    // REQ-ZONE-001 item 3, REQ-NFR-008
    let mut c = with(3);
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    c.attach(o(3), AttachTarget::Permanent(o(2))).unwrap();
    let before = c.clone();
    // Unknown id first, even with an unknown target.
    assert_eq!(
        c.attach(o(9), AttachTarget::Permanent(o(8))),
        Err(PermanentError::UnknownObject(o(9)))
    );
    assert_eq!(
        c.attach(o(9), AttachTarget::Player(p(1))),
        Err(PermanentError::UnknownObject(o(9)))
    );
    // Unknown target.
    assert_eq!(
        c.attach(o(1), AttachTarget::Permanent(o(8))),
        Err(PermanentError::UnknownObject(o(8)))
    );
    // Self.
    assert_eq!(
        c.attach(o(1), AttachTarget::Permanent(o(1))),
        Err(PermanentError::SelfAttach(o(1)))
    );
    // Two-cycle and longer cycle.
    assert_eq!(
        c.attach(o(1), AttachTarget::Permanent(o(2))),
        Err(PermanentError::AttachmentCycle {
            object: o(1),
            target: o(2)
        })
    );
    assert_eq!(
        c.attach(o(1), AttachTarget::Permanent(o(3))),
        Err(PermanentError::AttachmentCycle {
            object: o(1),
            target: o(3)
        })
    );
    assert_eq!(c, before);
    // A chain through a player link does not count as a cycle.
    c.attach(o(1), AttachTarget::Player(p(1))).unwrap();
    assert_eq!(c.attached_to(o(1)), Some(AttachTarget::Player(p(1))));
}

#[test]
fn detach_unknown_leaves_container_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(1);
    let before = c.clone();
    assert_eq!(c.detach(o(5)), Err(PermanentError::UnknownObject(o(5))));
    assert_eq!(c, before);
}

/// Picks one element of `v` with the test PRNG.
fn pick<T: Copy>(rng: &mut Pcg64Dxsm, v: &[T]) -> Option<T> {
    let n = u64::try_from(v.len()).unwrap();
    if n == 0 {
        return None;
    }
    v.get(usize::try_from(rng.next_u64() % n).unwrap()).copied()
}

/// Every attachment chain ends within `len() + 1` steps, every permanent target is held, and
/// `attached_to` agrees with `attachments_of` in both directions.
fn assert_attachment_invariants(c: &Permanents) {
    let bound = c.len() + 1;
    for id in c.ids() {
        let mut current = id;
        let mut steps = 0;
        while let Some(AttachTarget::Permanent(next)) = c.attached_to(current) {
            assert!(c.get(next).is_some(), "attached to a missing permanent");
            assert_ne!(next, id, "attachment cycle through {id:?}");
            current = next;
            steps += 1;
            assert!(steps <= bound, "chain from {id:?} does not terminate");
        }
        if let Some(t) = c.attached_to(id) {
            assert!(c.attachments_of(t).contains(&id));
        }
        for x in c.attachments_of(AttachTarget::Permanent(id)) {
            assert_eq!(c.attached_to(x), Some(AttachTarget::Permanent(id)));
        }
    }
    for player in 1..=2 {
        for x in c.attachments_of(AttachTarget::Player(p(player))) {
            assert_eq!(c.attached_to(x), Some(AttachTarget::Player(p(player))));
        }
    }
}

#[test]
fn property_attachments_never_cycle() {
    // REQ-ZONE-001 item 3 (acceptance: attachments can never form a cycle)
    let mut cycles_rejected = 0_u32;
    for seed in 0..256_u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let mut c = Permanents::new();
        let universe: Vec<ObjectId> = (1..=8).map(o).collect();
        for step in 0..120_u64 {
            let id = pick(&mut rng, &universe).unwrap();
            let other = pick(&mut rng, &universe).unwrap();
            let before = c.clone();
            match rng.next_u64() % 8 {
                0 | 1 => {
                    let _ = c.enter(id, p(1 + rng.next_u64() % 2), step);
                }
                2 => {
                    let _ = c.leave(id);
                }
                3..=5 => {
                    let r = c.attach(id, AttachTarget::Permanent(other));
                    if r.is_err() {
                        assert_eq!(c, before, "failed attach changed the container");
                    }
                    if matches!(r, Err(PermanentError::AttachmentCycle { .. })) {
                        cycles_rejected += 1;
                    }
                }
                6 => {
                    let _ = c.attach(id, AttachTarget::Player(p(1 + rng.next_u64() % 2)));
                }
                _ => {
                    let _ = c.detach(id);
                }
            }
            assert_attachment_invariants(&c);
        }
    }
    // The generator really exercises the cycle check.
    assert!(
        cycles_rejected > 100,
        "only {cycles_rejected} cycles were attempted"
    );
}

// ---- combat ----

#[test]
fn combat_setters() {
    // REQ-ZONE-001 item 3; CR 508.1k, CR 509.1g, CR 509.1h
    let mut c = with(3);
    for a in [
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(9)))),
        AttackState::Attacking(None),
        AttackState::NotAttacking,
    ] {
        c.set_attack(o(1), a).unwrap();
        assert_eq!(perm(&c, 1).combat().attack, a);
    }
    // set_attack does not change removed_from_combat.
    c.remove_from_combat(o(1)).unwrap();
    c.set_attack(o(1), AttackState::Attacking(None)).unwrap();
    assert!(perm(&c, 1).combat().removed_from_combat);

    c.set_blocking(o(2), &[o(3), o(1), o(3)]).unwrap();
    assert_eq!(perm(&c, 2).combat().blocking, BTreeSet::from([o(1), o(3)]));
    c.set_blocking(o(2), &[o(1)]).unwrap();
    assert_eq!(perm(&c, 2).combat().blocking, BTreeSet::from([o(1)]));
    c.set_blocking(o(2), &[]).unwrap();
    assert!(perm(&c, 2).combat().blocking.is_empty());

    c.set_blocked(o(3), true).unwrap();
    assert!(perm(&c, 3).combat().blocked);
    c.set_blocked(o(3), false).unwrap();
    assert!(!perm(&c, 3).combat().blocked);
}

#[test]
fn combat_setters_reject_unknown_ids_unchanged() {
    // REQ-ZONE-001, REQ-NFR-008
    let mut c = with(2);
    c.set_blocking(o(1), &[o(2)]).unwrap();
    let before = c.clone();
    let e = |n| Err(PermanentError::UnknownObject(o(n)));
    assert_eq!(c.set_attack(o(9), AttackState::Attacking(None)), e(9));
    assert_eq!(c.set_blocking(o(9), &[o(1)]), e(9));
    assert_eq!(c.set_blocking(o(1), &[o(2), o(7)]), e(7));
    assert_eq!(c.set_blocking(o(1), &[o(8), o(7)]), e(8));
    assert_eq!(c.set_blocked(o(9), true), e(9));
    assert_eq!(c.remove_from_combat(o(9)), e(9));
    assert_eq!(c.clear_combat(o(9)), e(9));
    assert_eq!(c, before);
}

#[test]
fn remove_from_combat_semantics() {
    // REQ-ZONE-001; CR 506.4, CR 506.4c, CR 509.1h
    // 1: planeswalker/battle being attacked, also blocking 5; 2: attacks 1; 3: attacks a
    // player; 4: blocks 1 (as an attacker) and 5; 5: attacker blocked by 1 and 4.
    let mut c = with(5);
    c.set_blocking(o(1), &[o(5)]).unwrap();
    c.set_attack(
        o(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    c.set_attack(
        o(3),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocking(o(4), &[o(1), o(5)]).unwrap();
    c.set_attack(
        o(5),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(5), true).unwrap();

    c.remove_from_combat(o(1)).unwrap();
    let combat = perm(&c, 1).combat();
    assert_eq!(combat.attack, AttackState::NotAttacking);
    assert!(combat.blocking.is_empty());
    assert!(!combat.blocked);
    assert!(combat.removed_from_combat);
    assert_eq!(perm(&c, 2).combat().attack, AttackState::Attacking(None));
    assert!(!perm(&c, 2).combat().removed_from_combat);
    assert_eq!(
        perm(&c, 3).combat().attack,
        AttackState::Attacking(Some(AttackTarget::Player(p(2))))
    );
    // Others' blocking sets and blocked flags are untouched.
    assert_eq!(perm(&c, 4).combat().blocking, BTreeSet::from([o(1), o(5)]));
    assert!(perm(&c, 5).combat().blocked);

    // A permanent attacking itself (not validated) is not redirected: it is fully removed.
    let mut d = with(1);
    d.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    )
    .unwrap();
    d.remove_from_combat(o(1)).unwrap();
    assert_eq!(perm(&d, 1).combat().attack, AttackState::NotAttacking);
}

#[test]
fn clear_combat_resets_only_that_permanent() {
    // REQ-ZONE-001 item 3; CR 508.1k, CR 509.1g
    let mut c = with(2);
    for n in [1, 2] {
        c.set_attack(o(n), AttackState::Attacking(None)).unwrap();
        c.set_blocked(o(n), true).unwrap();
        c.set_blocking(o(n), &[o(1)]).unwrap();
        c.remove_from_combat(o(n)).unwrap();
        c.set_attack(o(n), AttackState::Attacking(None)).unwrap();
    }
    let other = perm(&c, 2).clone();
    c.clear_combat(o(1)).unwrap();
    assert_eq!(perm(&c, 1).combat(), &CombatState::default());
    assert_eq!(perm(&c, 2), &other);
}

#[test]
fn clear_all_combat_resets_every_permanent() {
    // REQ-ZONE-001 item 3; CR 508.1k, CR 509.1g
    let mut c = with(3);
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocked(o(1), true).unwrap();
    c.set_blocking(o(2), &[o(1)]).unwrap();
    c.remove_from_combat(o(3)).unwrap();
    c.set_tapped(o(1), true).unwrap();
    c.clear_all_combat();
    for n in 1..=3 {
        assert_eq!(perm(&c, n).combat(), &CombatState::default());
    }
    assert!(perm(&c, 1).status().tapped);
    let mut empty = Permanents::new();
    empty.clear_all_combat();
    assert!(empty.is_empty());
}

// ---- errors ----

#[test]
fn error_display_and_error_trait() {
    // REQ-NFR-008
    let cases = [
        (PermanentError::UnknownObject(o(1)), "unknown permanent 1"),
        (
            PermanentError::AlreadyPresent(o(2)),
            "permanent 2 is already on the battlefield",
        ),
        (
            PermanentError::SelfAttach(o(3)),
            "permanent 3 cannot be attached to itself",
        ),
        (
            PermanentError::AttachmentCycle {
                object: o(4),
                target: o(5),
            },
            "attaching permanent 4 to permanent 5 would form an attachment cycle",
        ),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
        let dynerr: &dyn core::error::Error = &e;
        assert!(dynerr.source().is_none());
        assert_eq!(e, e.clone());
        assert!(!format!("{e:?}").is_empty());
    }
}

// ---- canonical encoding and clone ----

/// A container exercising every field.
fn rich() -> Permanents {
    let mut c = with(3);
    c.set_designation(o(1), "Monstrous", DesignationValue::Flag)
        .unwrap();
    c.attach(o(2), AttachTarget::Permanent(o(1))).unwrap();
    c.set_attack(
        o(1),
        AttackState::Attacking(Some(AttackTarget::Player(p(2)))),
    )
    .unwrap();
    c.set_blocking(o(3), &[o(1)]).unwrap();
    c
}

#[test]
fn canonical_encoding_equal_values_equal_bytes() {
    // REQ-CORE-006
    let a = rich();
    let b = rich();
    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(bytes(&a), bytes(&a.clone()));
    // Empty container: the count only.
    assert_eq!(bytes(&Permanents::new()), vec![0, 0, 0, 0]);
}

#[test]
fn canonical_encoding_ignores_insertion_order() {
    // REQ-CORE-006
    let mut a = Permanents::new();
    let mut b = Permanents::new();
    for n in [1, 2, 3] {
        a.enter(o(n), p(1), n).unwrap();
    }
    for n in [3, 1, 2] {
        b.enter(o(n), p(1), n).unwrap();
    }
    for c in [&mut a, &mut b] {
        c.set_designation(o(2), "b", DesignationValue::Flag)
            .unwrap();
        c.set_designation(o(2), "a", DesignationValue::Int(1))
            .unwrap();
    }
    let mut d = Permanents::new();
    for n in [2, 3, 1] {
        d.enter(o(n), p(1), n).unwrap();
    }
    d.set_designation(o(2), "a", DesignationValue::Int(1))
        .unwrap();
    d.set_designation(o(2), "b", DesignationValue::Flag)
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(a, d);
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(bytes(&a), bytes(&d));
}

#[test]
fn canonical_encoding_every_field_change_changes_bytes() {
    // REQ-CORE-006, REQ-ZONE-001 item 3
    let base = rich();
    let base_bytes = bytes(&base);
    type Edit = fn(&mut Permanents);
    let edits: Vec<(&str, Edit)> = vec![
        ("enter", |c| c.enter(o(4), p(1), 1).unwrap()),
        ("leave", |c| {
            c.leave(o(3)).unwrap();
        }),
        ("controller", |c| c.set_controller(o(3), p(2), 1).unwrap()),
        ("controlled_since", |c| {
            c.permanents.get_mut(&o(3)).unwrap().controlled_since = 2;
        }),
        ("tapped", |c| c.set_tapped(o(1), true).unwrap()),
        ("flipped", |c| c.set_flipped(o(1), true).unwrap()),
        ("face_down", |c| c.set_face_down(o(1), true).unwrap()),
        ("phased_out", |c| {
            c.permanents.get_mut(&o(2)).unwrap().status.phased_out = true;
        }),
        ("designation add", |c| {
            c.set_designation(o(2), "Renowned", DesignationValue::Flag)
                .unwrap();
        }),
        ("designation value", |c| {
            c.set_designation(o(1), "Monstrous", DesignationValue::Int(0))
                .unwrap();
        }),
        ("designation text", |c| {
            c.set_designation(o(1), "Monstrous", DesignationValue::Text(String::new()))
                .unwrap();
        }),
        ("designation clear", |c| {
            c.clear_designation(o(1), "Monstrous").unwrap();
        }),
        ("attach player", |c| {
            c.attach(o(2), AttachTarget::Player(p(1))).unwrap();
        }),
        ("attach other permanent", |c| {
            c.attach(o(2), AttachTarget::Permanent(o(3))).unwrap();
        }),
        ("detach", |c| {
            c.detach(o(2)).unwrap();
        }),
        ("attack none", |c| {
            c.set_attack(o(1), AttackState::Attacking(None)).unwrap();
        }),
        ("attack not attacking", |c| {
            c.set_attack(o(1), AttackState::NotAttacking).unwrap();
        }),
        ("attack permanent", |c| {
            c.set_attack(
                o(1),
                AttackState::Attacking(Some(AttackTarget::Permanent(o(2)))),
            )
            .unwrap();
        }),
        ("attack other player", |c| {
            c.set_attack(
                o(1),
                AttackState::Attacking(Some(AttackTarget::Player(p(3)))),
            )
            .unwrap();
        }),
        ("blocking", |c| c.set_blocking(o(3), &[o(1), o(2)]).unwrap()),
        ("blocking cleared", |c| c.set_blocking(o(3), &[]).unwrap()),
        ("blocked", |c| c.set_blocked(o(1), true).unwrap()),
        ("removed_from_combat", |c| {
            c.permanents
                .get_mut(&o(2))
                .unwrap()
                .combat
                .removed_from_combat = true;
        }),
    ];
    let mut seen = BTreeSet::from([base_bytes.clone()]);
    for (name, edit) in edits {
        let mut c = base.clone();
        edit(&mut c);
        assert_ne!(c, base, "{name}: value did not change");
        let b = bytes(&c);
        assert_ne!(b, base_bytes, "{name}: bytes did not change");
        assert!(seen.insert(b), "{name}: bytes collide with another edit");
    }
}

#[test]
fn attack_state_shapes_encode_distinctly() {
    // REQ-CORE-006
    let shapes = [
        AttackState::NotAttacking,
        AttackState::Attacking(None),
        AttackState::Attacking(Some(AttackTarget::Player(p(1)))),
        AttackState::Attacking(Some(AttackTarget::Permanent(o(1)))),
    ];
    let encoded: BTreeSet<Vec<u8>> = shapes.iter().map(encode_to_vec).collect();
    assert_eq!(encoded.len(), shapes.len());
    let targets = [AttachTarget::Permanent(o(1)), AttachTarget::Player(p(1))];
    let encoded: BTreeSet<Vec<u8>> = targets.iter().map(encode_to_vec).collect();
    assert_eq!(encoded.len(), 2);
    assert_eq!(AttackState::default(), AttackState::NotAttacking);
}

#[test]
fn clone_is_independent() {
    // REQ-CORE-006
    let original = rich();
    let snapshot = original.clone();
    let mut copy = original.clone();
    copy.set_tapped(o(1), true).unwrap();
    copy.leave(o(2)).unwrap();
    copy.clear_all_combat();
    assert_eq!(original, snapshot);
    assert_eq!(bytes(&original), bytes(&snapshot));
    assert_ne!(copy, original);
}
