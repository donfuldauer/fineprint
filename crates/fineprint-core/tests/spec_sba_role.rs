// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the Role rule (REQ-SBA-001, REQ-TEST-001, REQ-CORE-001, REQ-CORE-006;
//! CR 704.5z, 303.7a, 613.7e, 704.5d, 702.26b). Black-box: written from the work-item brief
//! only. Every card is synthetic (R5).
//!
//! CR 704.5z: "If a permanent has more than one Role controlled by the same player attached to
//! it, each of those Roles except the one with the most recent timestamp is put into its
//! owner's graveyard."

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::zones::ZoneKind;
use support::*;

const SBA: ZoneChangeCause = ZoneChangeCause::StateBasedAction;

// REQ-SBA-001, CR 704.5z, 303.7a, 613.7e: two Roles controlled by the same player on one
// permanent: the one with the older attach stamp goes to its OWNER's graveyard (owner differs
// from controller); the newer stays. Both id orders against both stamp orders.
#[test]
fn req_sba_001_704_5z_two_roles_older_goes() {
    for newer_first in [false, true] {
        let mut s = Sim::new();
        let host = s.creature(P2, "test.vanilla_2_2", 2, 2);
        let (s1, s2) = if newer_first { (9, 4) } else { (4, 9) };
        let a = s.role_aura(P2, P1, host, s1);
        let b = s.role_aura(P2, P1, host, s2);
        let (older, newer) = if newer_first { (b, a) } else { (a, b) };
        assert_eq!(s.check(), vec![finding("704.5z", obj(older))]);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, vec![finding("704.5z", obj(older))]);
        assert!(r.outcome().performed);
        assert_to_owners_graveyard(&r, older, SBA);
        assert_eq!(
            r.after
                .zones
                .object(r.moved_to(older).unwrap())
                .unwrap()
                .zone,
            fineprint_core::zones::ZoneId::Graveyard(P2),
            "the owner's (P2's) graveyard, not the controller's"
        );
        assert_still_on_battlefield(&r, newer);
        assert_still_on_battlefield(&r, host);
        assert_eq!(r.nonempty_batches().len(), 1);
        let mut after = r.after.clone();
        after.sync_after(r.outcome());
        assert!(after.check().is_empty(), "one Role left");
    }
}

// REQ-SBA-001, CR 704.5z: one Role each from two different controllers on one permanent: both
// stay (the rule groups by the Role's controller).
#[test]
fn req_sba_001_704_5z_different_controllers_both_stay() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    // Both owned by P1; controlled by P1 and P2.
    let a = s.role_aura(P1, P1, host, 1);
    let b = s.role_aura(P1, P2, host, 2);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
}

// REQ-SBA-001, CR 704.5z: two controllers with two Roles each on one permanent: each group
// loses its older Role, in one event.
#[test]
fn req_sba_001_704_5z_two_groups_one_host() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let a1 = s.role_aura(P1, P1, host, 3);
    let b1 = s.role_aura(P2, P2, host, 8);
    let a2 = s.role_aura(P2, P1, host, 1);
    let b2 = s.role_aura(P1, P2, host, 9);
    assert_eq!(
        s.check(),
        vec![finding("704.5z", obj(b1)), finding("704.5z", obj(a2))]
    );
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, b1, SBA);
    assert_to_owners_graveyard(&r, a2, SBA);
    assert_still_on_battlefield(&r, a1);
    assert_still_on_battlefield(&r, b2);
    assert_eq!(r.nonempty_batches().len(), 1);
}

// REQ-SBA-001, CR 704.5z, 613.7e: three Roles of one controller, run over every permutation of
// the stamps against the ids: exactly the Role with the greatest stamp stays; the two others
// go to their owners' graveyards in one event; findings ascending by id.
#[test]
fn req_sba_001_704_5z_three_roles_every_permutation() {
    let perms: [[u64; 3]; 6] = [
        [10, 20, 30],
        [10, 30, 20],
        [20, 10, 30],
        [20, 30, 10],
        [30, 10, 20],
        [30, 20, 10],
    ];
    for stamps in perms {
        let mut s = Sim::new();
        let host = s.creature(P2, "test.vanilla_3_3", 3, 3);
        let owners = [P1, P2, P2];
        let roles: Vec<ObjectId> = (0..3)
            .map(|i| s.role_aura(owners[i], P1, host, stamps[i]))
            .collect();
        let winner = roles[stamps.iter().position(|t| *t == 30).unwrap()];
        let losers: Vec<ObjectId> = roles.iter().copied().filter(|r| *r != winner).collect();
        let expected: Vec<_> = losers.iter().map(|l| finding("704.5z", obj(*l))).collect();
        assert_eq!(s.check(), expected, "{stamps:?}");
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, expected);
        assert_still_on_battlefield(&r, winner);
        for l in &losers {
            assert_to_owners_graveyard(&r, *l, SBA);
        }
        assert_eq!(r.moves().len(), 2);
        assert_eq!(r.nonempty_batches().len(), 1, "{stamps:?}: one event");
    }
}

// REQ-SBA-001, CR 704.5z, 613.7m: equal attach stamps: the HIGHER ObjectId counts as the more
// recent and survives (explicit equal stamps and missing stamps, both read as equal).
#[test]
fn req_sba_001_704_5z_tie_higher_id_survives() {
    for missing in [false, true] {
        let mut s = Sim::new();
        let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
        let a = s.role_aura(P1, P1, host, 5);
        let b = s.role_aura(P1, P1, host, 5);
        let c = s.role_aura(P1, P1, host, 5);
        if missing {
            for id in [a, b, c] {
                s.env.attachment_since.remove(&id);
            }
        }
        assert!(a < b && b < c);
        assert_eq!(
            s.check(),
            vec![finding("704.5z", obj(a)), finding("704.5z", obj(b))]
        );
        let r = run_sba(s);
        assert_still_on_battlefield(&r, c);
        assert_to_owners_graveyard(&r, a, SBA);
        assert_to_owners_graveyard(&r, b, SBA);
    }
}

// REQ-SBA-001, CR 704.5z: Roles of one controller on different permanents do not interact.
#[test]
fn req_sba_001_704_5z_different_permanents_do_not_interact() {
    let mut s = Sim::new();
    let h1 = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let h2 = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let a = s.role_aura(P1, P1, h1, 1);
    let b = s.role_aura(P1, P1, h2, 2);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
}

// REQ-SBA-001, CR 704.5z, 303.7a: an Aura that is not a Role is untouched: a Role and a plain
// Aura of one controller on one permanent, or two plain Auras, give no finding.
#[test]
fn req_sba_001_704_5z_non_role_aura_untouched() {
    let mut s = Sim::new();
    let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
    let role = s.role_aura(P1, P1, host, 1);
    let aura = s.permanent(P1, P1, aura_chars("test.aura_a"));
    s.attach_to(aura, host);
    s.env.attachment_since.insert(aura, 9);
    let aura2 = s.permanent(P1, P1, aura_chars("test.aura_b"));
    s.attach_to(aura2, host);
    s.env.attachment_since.insert(aura2, 7);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    for id in [role, aura, aura2] {
        assert_still_on_battlefield(&r, id);
    }
}

// REQ-SBA-001, CR 704.5z, 702.26b: a phased-out Role is treated as though it does not exist:
// with the newer or the older of two Roles phased out, nothing is found and nothing moves.
#[test]
fn req_sba_001_704_5z_phased_out_role_ignored() {
    for phase_newer in [false, true] {
        let mut s = Sim::new();
        let host = s.creature(P1, "test.vanilla_2_2", 2, 2);
        let older = s.role_aura(P1, P1, host, 1);
        let newer = s.role_aura(P1, P1, host, 2);
        s.phase_out(if phase_newer { newer } else { older });
        assert!(s.check().is_empty(), "phase newer {phase_newer}");
        let r = run_sba(s);
        assert!(!r.outcome().performed);
        assert!(r.after.on_battlefield(older));
        assert!(r.after.on_battlefield(newer));
        assert!(r.moves().is_empty());
    }
}

// REQ-SBA-001, CR 704.5z, 704.5d, 704.3: a token Role that loses goes to its owner's graveyard
// in the first pass and ceases to exist in the next pass (704.5d).
#[test]
fn req_sba_001_704_5z_token_role_ceases_next_pass() {
    let mut s = Sim::new();
    let host = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let old = s.role_token(P1, host, 1);
    let new = s.role_token(P1, host, 2);
    assert_eq!(s.check(), vec![finding("704.5z", obj(old))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, old, SBA);
    assert_still_on_battlefield(&r, new);
    let moved = r.moved_to(old).expect("moved");
    assert_eq!(r.after.zone_of(moved), Some(ZoneKind::Graveyard));
    let mut next = r.after.clone();
    next.sync_after(r.outcome());
    assert_eq!(next.check(), vec![finding("704.5d", obj(moved))]);
    let r2 = run_sba(next);
    assert_eq!(r2.outcome().ceased, vec![moved]);
    assert!(r2.after.zones.object(moved).is_none());
    assert!(r2.after.on_battlefield(new));
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5z: the same input gives identical outcome and canonical
// bytes twice.
#[test]
fn req_sba_001_704_5z_deterministic() {
    let build = || {
        let mut s = Sim::new();
        let host = s.creature(P2, "test.vanilla_2_2", 2, 2);
        s.role_aura(P1, P1, host, 3);
        s.role_aura(P2, P1, host, 3);
        s.role_aura(P1, P1, host, 1);
        s
    };
    let a = run_sba(build());
    let b = run_sba(build());
    assert_eq!(a.result, b.result);
    assert_eq!(a.after.canonical(), b.after.canonical());
}
