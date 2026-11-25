// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the speed rule (REQ-SBA-001, REQ-TEST-001, REQ-CORE-001, REQ-CORE-006;
//! CR 704.5aa, 702.179a, 702.179b, 702.179f, 702.26b, 613.1f). Black-box: written from the
//! work-item brief only. Every card is synthetic (R5).
//!
//! CR 704.5aa: "If a player controls a permanent with start your engines! and that player has
//! no speed, that player's speed becomes 1."

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_core::sba::SPEED;
use fineprint_core::state::DesignationValue;
use fineprint_core::zones::ZoneKind;
use support::*;

/// Asserts the pass executed no event.
fn assert_no_events(r: &SbaRun) {
    assert!(r.nonempty_batches().is_empty(), "nothing in `executed`");
    assert!(r.moves().is_empty());
}

// REQ-SBA-001, CR 704.5aa, 702.179a, 702.179b: a player who controls a permanent with start
// your engines! (owned by the opponent) and has no speed gets speed 1, appears in `speed_set`;
// the pass is performed with nothing in `executed`, and a repeat pass finds nothing and is not
// performed.
#[test]
fn req_sba_001_704_5aa_no_speed_becomes_one() {
    assert_eq!(SPEED, "speed");
    let mut s = Sim::new();
    let engine = s.engine(P2, P1);
    assert_eq!(s.speed(P1), None, "no speed until set (CR 702.179b)");
    assert_eq!(s.check(), vec![finding("704.5aa", player(P1))]);
    let r = run_sba(s);
    let o = r.outcome();
    assert_eq!(o.findings, vec![finding("704.5aa", player(P1))]);
    assert_eq!(o.speed_set, vec![P1]);
    assert!(o.performed);
    assert_no_events(&r);
    assert!(o.protectors_chosen.is_empty());
    assert_eq!(r.after.speed(P1), Some(DesignationValue::Int(1)));
    assert_eq!(r.after.speed(P2), None);
    assert_still_on_battlefield(&r, engine);

    let mut again = r.after.clone();
    assert!(again.check().is_empty());
    let r2 = run_sba(again);
    assert!(r2.outcome().findings.is_empty());
    assert!(!r2.outcome().performed);
    assert!(r2.outcome().speed_set.is_empty());
    assert_eq!(r2.after.speed(P1), Some(DesignationValue::Int(1)));
}

// REQ-SBA-001, CR 704.5aa: a player who already has speed (any value 1-4) keeps it: nothing is
// found.
#[test]
fn req_sba_001_704_5aa_existing_speed_kept() {
    for n in [1, 2, 3, 4] {
        let mut s = Sim::new();
        s.engine(P1, P1);
        s.set_speed(P1, n);
        assert!(s.check().is_empty(), "speed {n}");
        let r = run_sba(s);
        assert!(r.outcome().findings.is_empty());
        assert!(!r.outcome().performed);
        assert!(r.outcome().speed_set.is_empty());
        assert_eq!(r.after.speed(P1), Some(DesignationValue::Int(n)));
    }
}

// REQ-SBA-001, CR 704.5aa: two permanents with start your engines! under one player set the
// speed once (one finding, one `speed_set` entry).
#[test]
fn req_sba_001_704_5aa_two_engines_set_once() {
    let mut s = Sim::new();
    s.engine(P1, P1);
    s.engine(P2, P1);
    assert_eq!(s.check(), vec![finding("704.5aa", player(P1))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().speed_set, vec![P1]);
    assert_eq!(r.after.speed(P1), Some(DesignationValue::Int(1)));
}

// REQ-SBA-001, CR 704.5aa: an opponent's permanent with start your engines! gives the opponent
// speed, not me; both players with engines get speed, ascending in `speed_set`.
#[test]
fn req_sba_001_704_5aa_opponents_engine_not_mine() {
    let mut s = Sim::new();
    s.engine(P1, P2);
    assert_eq!(s.check(), vec![finding("704.5aa", player(P2))]);
    let r = run_sba(s.clone());
    assert_eq!(r.outcome().speed_set, vec![P2]);
    assert_eq!(r.after.speed(P1), None);
    assert_eq!(r.after.speed(P2), Some(DesignationValue::Int(1)));

    s.engine(P2, P1);
    assert_eq!(
        s.check(),
        vec![
            finding("704.5aa", player(P1)),
            finding("704.5aa", player(P2))
        ]
    );
    let r = run_sba(s);
    assert_eq!(r.outcome().speed_set, vec![P1, P2]);
}

// REQ-SBA-001, CR 704.5aa, 702.26b: a phased-out permanent with start your engines! is
// ignored; a card with it in a graveyard or hand is not a permanent and is ignored too.
#[test]
fn req_sba_001_704_5aa_phased_out_and_nonpermanent_engine_ignored() {
    let mut s = Sim::new();
    let e = s.engine(P1, P1);
    s.phase_out(e);
    s.create(ZoneKind::Graveyard, P1, engine_chars("test.engine_b"));
    s.create(ZoneKind::Hand, P1, engine_chars("test.engine_c"));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(r.outcome().speed_set.is_empty());
    assert_eq!(r.after.speed(P1), None);
}

// REQ-SBA-001, CR 704.5aa, 613.1f: start your engines! granted by a layer 6 effect counts (the
// evaluated abilities); without the effect the same permanent gives nothing.
#[test]
fn req_sba_001_704_5aa_layer_granted_engine_counts() {
    let mut s = Sim::new();
    let art = s.permanent(P1, P2, artifact_chars("test.artifact_a"));
    assert!(s.check().is_empty(), "no keyword before the effect");
    s.add_layer_effect(5, grant_engine_effect(&[art], 5));
    assert_eq!(s.check(), vec![finding("704.5aa", player(P2))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().speed_set, vec![P2]);
    assert_eq!(r.after.speed(P2), Some(DesignationValue::Int(1)));
}

// REQ-SBA-001, CR 704.5aa: a player who has lost the game gets no speed, though they control a
// permanent with start your engines!.
#[test]
fn req_sba_001_704_5aa_lost_player_gets_none() {
    let mut s = Sim::with_players(3);
    s.engine(P2, P2);
    s.engine(P3, P3);
    s.mark_lost(P2);
    assert_eq!(s.check(), vec![finding("704.5aa", player(P3))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().speed_set, vec![P3]);
    assert_eq!(r.after.speed(P2), None);
    assert_eq!(r.after.speed(P3), Some(DesignationValue::Int(1)));
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5aa: `check` is pure (canonical bytes unchanged) and the
// same input gives the same outcome and bytes twice.
#[test]
fn req_sba_001_704_5aa_check_pure_and_deterministic() {
    let build = || {
        let mut s = Sim::with_players(3);
        s.engine(P3, P1);
        s.engine(P1, P3);
        s
    };
    let mut s = build();
    let before = s.canonical();
    let found = s.check();
    assert_eq!(s.canonical(), before, "check changes nothing");
    assert_eq!(
        found,
        vec![
            finding("704.5aa", player(P1)),
            finding("704.5aa", player(P3))
        ]
    );
    let a = run_sba(build());
    let b = run_sba(build());
    assert_eq!(a.result, b.result);
    assert_eq!(a.after.canonical(), b.after.canonical());
    assert_eq!(a.outcome().speed_set, vec![P1, P3]);
}
