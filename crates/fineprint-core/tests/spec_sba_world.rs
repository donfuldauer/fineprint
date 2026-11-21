// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the world rule (REQ-SBA-001, REQ-CORE-001; CR 704.5k). Black-box: written
//! from the work-item brief only. Every card is synthetic (R5).
//!
//! CR 704.5k: "If two or more permanents have the supertype world, all except the one that has
//! had the world supertype for the shortest amount of time are put into their owners'
//! graveyards. In the event of a tie for the shortest amount of time, all are put into their
//! owners' graveyards." The test environment's `world_supertype_since` stamp is larger for a
//! permanent that gained the supertype later (a shorter time).

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::ZoneChangeCause;
use support::*;

/// Every permutation of `items` (small inputs only).
fn permutations(items: &[u64]) -> Vec<Vec<u64>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head);
            out.push(tail);
        }
    }
    out
}

/// Three world permanents with stamps `stamps` (by id order), the second owned by P2 and
/// controlled by P1, the third controlled by P2.
fn three_worlds(stamps: &[u64]) -> (Sim, [ObjectId; 3]) {
    let mut s = Sim::new();
    let a = s.world(P1, P1, "test.world_a", Some(stamps[0]));
    let b = s.world(P2, P1, "test.world_b", Some(stamps[1]));
    let c = s.world(P2, P2, "test.world_c", Some(stamps[2]));
    (s, [a, b, c])
}

// REQ-SBA-001, CR 704.5k: the unique newest world permanent stays; every other goes to its
// owner's graveyard (across controllers, and to the owner when owner and controller differ).
#[test]
fn req_sba_001_704_5k_unique_newest_stays() {
    let (s, ids) = three_worlds(&[1, 7, 3]);
    let [a, b, c] = ids;
    let mut s = s;
    assert_eq!(
        s.check(),
        vec![finding("704.5k", obj(a)), finding("704.5k", obj(c))]
    );
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(
        r.outcome().findings,
        vec![finding("704.5k", obj(a)), finding("704.5k", obj(c))]
    );
    assert_still_on_battlefield(&r, b);
    assert_to_owners_graveyard(&r, a, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, c, ZoneChangeCause::StateBasedAction);
    assert_eq!(r.nonempty_batches().len(), 1);
}

// REQ-SBA-001, CR 704.5k: a tie for the newest sends ALL world permanents, the tied ones and
// the older ones; there is no tie-break by id or timestamp.
#[test]
fn req_sba_001_704_5k_tie_for_newest_sends_all() {
    for stamps in permutations(&[5, 5, 2]) {
        let (mut s, ids) = three_worlds(&stamps);
        let expected: Vec<_> = ids.iter().map(|id| finding("704.5k", obj(*id))).collect();
        assert_eq!(s.check(), expected, "stamps {stamps:?}");
        let r = run_sba(s);
        for id in ids {
            assert_to_owners_graveyard(&r, id, ZoneChangeCause::StateBasedAction);
        }
    }
}

// REQ-SBA-001, CR 704.5k: a single world permanent stays, whatever its stamp.
#[test]
fn req_sba_001_704_5k_single_world_stays() {
    for stamp in [None, Some(0), Some(9)] {
        let mut s = Sim::new();
        let a = s.world(P1, P1, "test.world_a", stamp);
        // A non-world enchantment does not make a pair.
        let other = s.permanent(P1, P1, typed(&["test.world_a"], &[], &["Enchantment"], &[]));
        assert!(s.check().is_empty());
        let r = run_sba(s);
        assert!(!r.outcome().performed);
        assert_still_on_battlefield(&r, a);
        assert_still_on_battlefield(&r, other);
    }
}

// REQ-SBA-001, CR 704.5k: world permanents under different controllers are compared together.
#[test]
fn req_sba_001_704_5k_different_controllers() {
    let mut s = Sim::new();
    let old = s.world(P1, P1, "test.world_a", Some(1));
    let new = s.world(P2, P2, "test.world_b", Some(2));
    assert_eq!(s.check(), vec![finding("704.5k", obj(old))]);
    let r = run_sba(s);
    assert_still_on_battlefield(&r, new);
    assert_to_owners_graveyard(&r, old, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5k: the result follows the stamps, never the object ids:
// for every assignment of the stamps {1, 2, 3} to three ids, exactly the id holding 3 stays.
#[test]
fn req_sba_001_704_5k_independent_of_id_order() {
    for stamps in permutations(&[1, 2, 3]) {
        let (s, ids) = three_worlds(&stamps);
        let newest = ids[stamps.iter().position(|x| *x == 3).expect("present")];
        let r = run_sba(s);
        for id in ids {
            if id == newest {
                assert_still_on_battlefield(&r, id);
            } else {
                assert_to_owners_graveyard(&r, id, ZoneChangeCause::StateBasedAction);
            }
        }
        assert_eq!(r.outcome().findings.len(), 2, "stamps {stamps:?}");
    }
}

// REQ-SBA-001, CR 704.5k: with no stamps recorded (default 0 for all), two or more world
// permanents tie and all go.
#[test]
fn req_sba_001_704_5k_default_stamps_all_go() {
    let mut s = Sim::new();
    let a = s.world(P1, P1, "test.world_a", None);
    let b = s.world(P2, P2, "test.world_b", None);
    assert_eq!(
        s.check(),
        vec![finding("704.5k", obj(a)), finding("704.5k", obj(b))]
    );
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, a, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, b, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5k, 702.26b: a phased-out world permanent is not compared.
#[test]
fn req_sba_001_704_5k_phased_out_skipped() {
    let mut s = Sim::new();
    let a = s.world(P1, P1, "test.world_a", Some(1));
    let b = s.world(P1, P1, "test.world_b", Some(2));
    s.phase_out(b);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
}
