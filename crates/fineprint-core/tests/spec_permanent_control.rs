// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::permanent`. Black-box: written from the work-item brief only.
//!
//! Continuous control: the turn control began, control changes resetting it and removing the
//! permanent from combat, the strict "since the start of turn t" predicate, and phasing leaving
//! control untouched (REQ-ZONE-001 item 3; REQ-CORE-006).

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::permanent::{
    AttackState, AttackTarget, CombatState, PermanentError, Permanents, Status,
};

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn since(p: &Permanents, id: u64) -> u64 {
    p.get(oid(id)).unwrap().controlled_since_turn()
}

fn controller(p: &Permanents, id: u64) -> PlayerId {
    p.get(oid(id)).unwrap().controller()
}

fn combat(p: &Permanents, id: u64) -> CombatState {
    p.get(oid(id)).unwrap().combat().clone()
}

/// Puts `id` fully into combat: attacking player 9, blocking `blocks`, blocked.
fn in_combat(p: &mut Permanents, id: u64, blocks: &[u64]) {
    p.set_attack(
        oid(id),
        AttackState::Attacking(Some(AttackTarget::Player(pid(9)))),
    )
    .unwrap();
    let b: Vec<ObjectId> = blocks.iter().map(|&n| oid(n)).collect();
    p.set_blocking(oid(id), &b).unwrap();
    p.set_blocked(oid(id), true).unwrap();
}

fn removed_state() -> CombatState {
    CombatState {
        removed_from_combat: true,
        ..CombatState::default()
    }
}

// REQ-ZONE-001 item 3: enter records the controller and the turn control began.
#[test]
fn req_zone_001_enter_records_turn() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 0).unwrap();
    p.enter(oid(2), pid(2), 7).unwrap();
    p.enter(oid(3), pid(1), u64::MAX).unwrap();
    assert_eq!(since(&p, 1), 0);
    assert_eq!(since(&p, 2), 7);
    assert_eq!(since(&p, 3), u64::MAX);
    assert_eq!(controller(&p, 1), pid(1));
    assert_eq!(controller(&p, 2), pid(2));
}

// REQ-ZONE-001 item 3; CR 613.1b: a different controller resets continuous control to the given
// turn, even when that turn is smaller than or equal to the old one.
#[test]
fn req_zone_001_set_controller_different_resets_since() {
    for (enter_turn, change_turn) in [(3u64, 8u64), (10, 3), (5, 5), (0, 0), (u64::MAX, 0)] {
        let mut p = Permanents::new();
        p.enter(oid(1), pid(1), enter_turn).unwrap();
        p.set_controller(oid(1), pid(2), change_turn).unwrap();
        assert_eq!(controller(&p, 1), pid(2));
        assert_eq!(since(&p, 1), change_turn);
    }
}

// REQ-ZONE-001 item 3; CR 506.4, 613.1b: a control change removes the permanent from combat.
#[test]
fn req_zone_001_set_controller_removes_from_combat() {
    let mut p = Permanents::new();
    for i in 1..=3 {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    in_combat(&mut p, 1, &[2, 3]);
    p.set_controller(oid(1), pid(2), 4).unwrap();
    assert_eq!(combat(&p, 1), removed_state());
}

// REQ-ZONE-001 item 3; CR 506.4, 506.4c: control change of an attacked permanent leaves its
// attackers attacking nothing; player-targeted attacks are not rewritten.
#[test]
fn req_zone_001_set_controller_of_attacked_permanent_rewrites_attackers() {
    let mut p = Permanents::new();
    for i in 1..=4 {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    p.set_attack(
        oid(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(oid(1)))),
    )
    .unwrap();
    p.set_attack(
        oid(3),
        AttackState::Attacking(Some(AttackTarget::Player(pid(1)))),
    )
    .unwrap();
    p.set_attack(
        oid(4),
        AttackState::Attacking(Some(AttackTarget::Permanent(oid(3)))),
    )
    .unwrap();
    p.set_controller(oid(1), pid(2), 2).unwrap();
    assert_eq!(combat(&p, 2).attack, AttackState::Attacking(None));
    assert!(!combat(&p, 2).removed_from_combat);
    assert_eq!(
        combat(&p, 3).attack,
        AttackState::Attacking(Some(AttackTarget::Player(pid(1))))
    );
    assert_eq!(
        combat(&p, 4).attack,
        AttackState::Attacking(Some(AttackTarget::Permanent(oid(3))))
    );
}

// REQ-ZONE-001 item 3: the same controller again is a complete no-op, including combat and
// the continuous-control turn.
#[test]
fn req_zone_001_set_controller_same_is_noop() {
    let mut p = Permanents::new();
    for i in 1..=3 {
        p.enter(oid(i), pid(1), 2).unwrap();
    }
    p.set_attack(
        oid(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(oid(1)))),
    )
    .unwrap();
    in_combat(&mut p, 1, &[3]);
    p.set_tapped(oid(1), true).unwrap();
    let before = p.clone();
    for turn in [0u64, 2, 3, u64::MAX] {
        p.set_controller(oid(1), pid(1), turn).unwrap();
        assert_eq!(p, before);
    }
    assert_eq!(since(&p, 1), 2);
}

// REQ-ZONE-001 item 3; CR 613.1b: A to B to A resets continuous control twice.
#[test]
fn req_zone_001_set_controller_round_trip_resets_twice() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 1).unwrap();
    p.set_controller(oid(1), pid(2), 4).unwrap();
    assert_eq!((controller(&p, 1), since(&p, 1)), (pid(2), 4));
    in_combat(&mut p, 1, &[]);
    p.set_controller(oid(1), pid(1), 6).unwrap();
    assert_eq!((controller(&p, 1), since(&p, 1)), (pid(1), 6));
    assert_eq!(combat(&p, 1), removed_state());
    assert!(
        !p.get(oid(1))
            .unwrap()
            .controlled_continuously_since_start_of(6)
    );
    assert!(
        p.get(oid(1))
            .unwrap()
            .controlled_continuously_since_start_of(7)
    );
}

// REQ-ZONE-001 item 3; CR 302.6: the predicate is strictly since < t.
#[test]
fn req_zone_001_continuous_control_predicate_is_strict() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 5).unwrap();
    let perm = p.get(oid(1)).unwrap();
    assert!(!perm.controlled_continuously_since_start_of(4));
    assert!(!perm.controlled_continuously_since_start_of(5));
    assert!(perm.controlled_continuously_since_start_of(6));
    assert!(!perm.controlled_continuously_since_start_of(0));
    assert!(perm.controlled_continuously_since_start_of(u64::MAX));
    for t in 0..20u64 {
        assert_eq!(perm.controlled_continuously_since_start_of(t), 5 < t);
    }

    p.enter(oid(2), pid(1), 0).unwrap();
    let perm = p.get(oid(2)).unwrap();
    assert!(!perm.controlled_continuously_since_start_of(0));
    assert!(perm.controlled_continuously_since_start_of(1));
    assert!(perm.controlled_continuously_since_start_of(u64::MAX));

    p.enter(oid(3), pid(1), u64::MAX).unwrap();
    let perm = p.get(oid(3)).unwrap();
    assert!(!perm.controlled_continuously_since_start_of(u64::MAX - 1));
    assert!(!perm.controlled_continuously_since_start_of(u64::MAX));
    assert!(!perm.controlled_continuously_since_start_of(0));
}

// REQ-ZONE-001 item 3; CR 302.6: control gained during the controller's own turn is not
// continuous since that turn began; it is from the next turn on.
#[test]
fn req_zone_001_control_gained_this_turn_is_not_continuous() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 2).unwrap();
    assert!(
        p.get(oid(1))
            .unwrap()
            .controlled_continuously_since_start_of(7)
    );
    p.set_controller(oid(1), pid(2), 7).unwrap();
    let perm = p.get(oid(1)).unwrap();
    assert!(!perm.controlled_continuously_since_start_of(7));
    assert!(perm.controlled_continuously_since_start_of(8));
}

// REQ-ZONE-001 item 3; CR 702.26d: phasing never changes controller or continuous control.
#[test]
fn req_zone_001_phasing_leaves_control_unchanged() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(3), 4).unwrap();
    for v in [true, true, false, false, true, false] {
        p.set_phased_out(oid(1), v).unwrap();
        assert_eq!(p.get(oid(1)).unwrap().status().phased_out, v);
        assert_eq!(controller(&p, 1), pid(3));
        assert_eq!(since(&p, 1), 4);
        assert!(
            p.get(oid(1))
                .unwrap()
                .controlled_continuously_since_start_of(5)
        );
    }
}

// REQ-ZONE-001 item 3; CR 702.26b, 506.4: phasing out removes from combat; a repeated phase-out
// and a phase-in only set the flag.
#[test]
fn req_zone_001_phase_out_transition_removes_from_combat() {
    let mut p = Permanents::new();
    for i in 1..=3 {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    in_combat(&mut p, 1, &[2]);
    p.set_phased_out(oid(1), true).unwrap();
    assert_eq!(combat(&p, 1), removed_state());
    assert!(p.get(oid(1)).unwrap().status().phased_out);

    // Already phased out: only the flag (combat set after the first phase-out survives).
    in_combat(&mut p, 1, &[3]);
    let c = combat(&p, 1);
    assert!(c.removed_from_combat);
    p.set_phased_out(oid(1), true).unwrap();
    assert_eq!(combat(&p, 1), c);

    // Phasing in: only the flag.
    p.set_phased_out(oid(1), false).unwrap();
    assert_eq!(combat(&p, 1), c);
    assert_eq!(p.get(oid(1)).unwrap().status(), Status::default());

    // Setting phased-in on a phased-in permanent with fresh combat: nothing changes.
    p.clear_combat(oid(1)).unwrap();
    in_combat(&mut p, 1, &[2]);
    let before = p.clone();
    p.set_phased_out(oid(1), false).unwrap();
    assert_eq!(p, before);
}

// REQ-ZONE-001 item 3; CR 702.26b, 506.4c: phasing out an attacked permanent leaves its
// attackers attacking nothing.
#[test]
fn req_zone_001_phase_out_of_attacked_permanent_rewrites_attackers() {
    let mut p = Permanents::new();
    for i in 1..=2 {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    p.set_attack(
        oid(2),
        AttackState::Attacking(Some(AttackTarget::Permanent(oid(1)))),
    )
    .unwrap();
    p.set_phased_out(oid(1), true).unwrap();
    assert_eq!(combat(&p, 2).attack, AttackState::Attacking(None));
}

// REQ-ZONE-001 item 3; CR 400.7: leave then re-enter starts a new continuous-control period.
#[test]
fn req_zone_001_leave_then_reenter_resets_control() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 2).unwrap();
    p.set_controller(oid(1), pid(2), 3).unwrap();
    let left = p.leave(oid(1)).unwrap();
    assert_eq!(left.controller(), pid(2));
    assert_eq!(left.controlled_since_turn(), 3);
    p.enter(oid(1), pid(1), 9).unwrap();
    assert_eq!(controller(&p, 1), pid(1));
    assert_eq!(since(&p, 1), 9);
    assert_eq!(combat(&p, 1), CombatState::default());
    assert!(
        !p.get(oid(1))
            .unwrap()
            .controlled_continuously_since_start_of(9)
    );
}

// REQ-ZONE-001 item 3: a control change keeps the permanent's status (CR 110.5c).
#[test]
fn req_zone_001_set_controller_keeps_status() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 1).unwrap();
    p.set_tapped(oid(1), true).unwrap();
    p.set_face_down(oid(1), true).unwrap();
    let s = p.get(oid(1)).unwrap().status();
    p.set_controller(oid(1), pid(2), 2).unwrap();
    assert_eq!(p.get(oid(1)).unwrap().status(), s);
}

// REQ-ZONE-001 item 3: errors leave the container unchanged.
#[test]
fn req_zone_001_control_errors_leave_container_unchanged() {
    let mut p = Permanents::new();
    p.enter(oid(1), pid(1), 1).unwrap();
    in_combat(&mut p, 1, &[]);
    let before = p.clone();
    assert_eq!(
        p.set_controller(oid(2), pid(2), 3),
        Err(PermanentError::UnknownObject(oid(2)))
    );
    assert_eq!(
        p.enter(oid(1), pid(2), 3),
        Err(PermanentError::AlreadyPresent(oid(1)))
    );
    assert_eq!(
        p.set_phased_out(oid(u64::MAX), true),
        Err(PermanentError::UnknownObject(oid(u64::MAX)))
    );
    assert_eq!(p, before);
}
