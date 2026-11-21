// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the legend rule (REQ-SBA-001, REQ-CORE-001; CR 704.5j, 201.2a, 708.2a,
//! 704.3). Black-box: written from the work-item brief only. Every card is synthetic (R5).
//!
//! CR 704.5j: "If two or more legendary permanents with the same name are controlled by the
//! same player, that player chooses one of them, and the rest are put into their owners'
//! graveyards."

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_core::choice::{ChoiceSession, Status};
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::zones::ZoneKind;
use support::*;

// REQ-SBA-001, CR 704.5j: two same-named legends under one controller: the controller is asked
// once; the chosen one stays and the other goes to its OWNER's graveyard, even when the owner
// is another player. Both orders of the answer are covered.
#[test]
fn req_sba_001_704_5j_group_of_two_chosen_stays() {
    for keep_first in [true, false] {
        let mut s = Sim::new();
        let a = s.legend(P1, P1, "test.legend_a");
        // Owned by P2, controlled by P1.
        let b = s.legend(P2, P1, "test.legend_a");
        assert_eq!(
            s.check(),
            vec![finding("704.5j", obj(a)), finding("704.5j", obj(b))]
        );
        let (kept, gone) = if keep_first { (a, b) } else { (b, a) };
        let r = drive_sba(s, |_, _| keep(kept));
        assert_eq!(r.requests.len(), 1, "one request for the group");
        assert_legend_request(&r.requests[0], P1, &[a, b]);
        assert_eq!(
            r.outcome().findings,
            vec![finding("704.5j", obj(a)), finding("704.5j", obj(b))]
        );
        assert!(r.outcome().performed);
        assert_still_on_battlefield(&r, kept);
        assert_to_owners_graveyard(&r, gone, ZoneChangeCause::StateBasedAction);
        assert_eq!(r.nonempty_batches().len(), 1);
    }
}

// REQ-SBA-001, CR 704.5j: a group of three: one request with all three candidates; the chosen
// one stays and the two others go in the same batch.
#[test]
fn req_sba_001_704_5j_group_of_three() {
    for pick in 0..3 {
        let mut s = Sim::new();
        let group = [
            s.legend(P2, P2, "test.legend_b"),
            s.legend(P1, P2, "test.legend_b"),
            s.legend(P2, P2, "test.legend_b"),
        ];
        let r = drive_sba(s, |_, _| keep(group[pick]));
        assert_eq!(r.requests.len(), 1);
        assert_legend_request(&r.requests[0], P2, &group);
        for (i, id) in group.iter().enumerate() {
            if i == pick {
                assert_still_on_battlefield(&r, *id);
            } else {
                assert_to_owners_graveyard(&r, *id, ZoneChangeCause::StateBasedAction);
            }
        }
        let batches = r.nonempty_batches();
        assert_eq!(batches.len(), 1, "one event (CR 704.3)");
        assert_eq!(r.moves().len(), 2);
    }
}

// REQ-SBA-001, CR 704.5j: two disjoint groups (interleaved ids, different controllers) are
// asked in ascending order of their lowest id, one request each, each by its controller.
#[test]
fn req_sba_001_704_5j_two_groups_ascending_lowest_id() {
    let mut s = Sim::new();
    // Group X (controller P2) gets the lowest id.
    let x1 = s.legend(P2, P2, "test.legend_x");
    let y1 = s.legend(P1, P1, "test.legend_y");
    let x2 = s.legend(P2, P2, "test.legend_x");
    let y2 = s.legend(P1, P1, "test.legend_y");
    let r = drive_sba(s, |i, req| {
        if i == 0 {
            keep(candidates(req)[1])
        } else {
            keep(candidates(req)[0])
        }
    });
    assert_eq!(r.requests.len(), 2);
    assert_legend_request(&r.requests[0], P2, &[x1, x2]);
    assert_legend_request(&r.requests[1], P1, &[y1, y2]);
    assert_still_on_battlefield(&r, x2);
    assert_to_owners_graveyard(&r, x1, ZoneChangeCause::StateBasedAction);
    assert_still_on_battlefield(&r, y1);
    assert_to_owners_graveyard(&r, y2, ZoneChangeCause::StateBasedAction);
    assert_eq!(r.nonempty_batches().len(), 1, "both groups in one event");
    let mut expected = vec![
        finding("704.5j", obj(x1)),
        finding("704.5j", obj(y1)),
        finding("704.5j", obj(x2)),
        finding("704.5j", obj(y2)),
    ];
    expected.sort_by_key(|f| f.subject);
    assert_eq!(r.outcome().findings, expected);
}

// REQ-SBA-001, CR 704.5j: answering with the request's default keeps the lowest id.
#[test]
fn req_sba_001_704_5j_default_answer_keeps_lowest() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let b = s.legend(P1, P1, "test.legend_a");
    let c = s.legend(P1, P1, "test.legend_a");
    let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
    assert_eq!(r.requests.len(), 1);
    assert_still_on_battlefield(&r, a);
    assert_to_owners_graveyard(&r, b, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, c, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5j: same-named legends under different controllers do not interact.
#[test]
fn req_sba_001_704_5j_different_controllers_do_not_interact() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    // Owned by P1 but controlled by P2: control decides, not ownership.
    let b = s.legend(P1, P2, "test.legend_a");
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
}

// REQ-SBA-001, CR 704.5j: nonlegendary permanents with the same name do not interact, nor does
// a legendary permanent with a nonlegendary one of the same name.
#[test]
fn req_sba_001_704_5j_nonlegendary_same_names_do_not_interact() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.legend_a", 2, 2);
    let b = s.creature(P1, "test.legend_a", 2, 2);
    let c = s.legend(P1, P1, "test.legend_a");
    assert!(s.check().is_empty());
    let r = run_sba(s);
    for id in [a, b, c] {
        assert_still_on_battlefield(&r, id);
    }
}

// REQ-SBA-001, CR 704.5j, 201.2a: objects share a name if they have at least one name in
// common, even if one has more names; a legend with an unrelated name is not in the group.
#[test]
fn req_sba_001_704_5j_one_shared_name_of_two() {
    let mut s = Sim::new();
    let two = s.permanent(
        P1,
        P1,
        typed_creature(
            &["test.legend_a", "test.legend_b"],
            &["Legendary"],
            &[],
            2,
            2,
        ),
    );
    let one = s.legend(P1, P1, "test.legend_b");
    let other = s.legend(P1, P1, "test.legend_c");
    assert_eq!(
        s.check(),
        vec![finding("704.5j", obj(two)), finding("704.5j", obj(one))]
    );
    let r = drive_sba(s, |_, _| keep(one));
    assert_eq!(r.requests.len(), 1);
    assert_legend_request(&r.requests[0], P1, &[two, one]);
    assert_still_on_battlefield(&r, one);
    assert_still_on_battlefield(&r, other);
    assert_to_owners_graveyard(&r, two, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5j, 201.2a, 708.2a: an object with no name never has the same name as
// another object, including another nameless one.
#[test]
fn req_sba_001_704_5j_nameless_never_match() {
    let mut s = Sim::new();
    let a = s.permanent(P1, P1, typed_creature(&[], &["Legendary"], &[], 2, 2));
    let b = s.permanent(P1, P1, typed_creature(&[], &["Legendary"], &[], 2, 2));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
}

// REQ-SBA-001, CR 704.5j, 702.26b: a phased-out legend is not a group member: a pair with one
// phased out is no group, and a triple with one phased out is a group of two.
#[test]
fn req_sba_001_704_5j_phased_out_member_skipped() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let b = s.legend(P1, P1, "test.legend_a");
    s.phase_out(b);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);

    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let out = s.legend(P1, P1, "test.legend_a");
    let c = s.legend(P1, P1, "test.legend_a");
    s.phase_out(out);
    assert_eq!(
        s.check(),
        vec![finding("704.5j", obj(a)), finding("704.5j", obj(c))]
    );
    let r = drive_sba(s, |_, _| keep(c));
    assert_eq!(r.requests.len(), 1);
    assert_legend_request(&r.requests[0], P1, &[a, c]);
    assert_still_on_battlefield(&r, c);
    assert_still_on_battlefield(&r, out);
    assert_to_owners_graveyard(&r, a, ZoneChangeCause::StateBasedAction);
}

// REQ-SBA-001, CR 704.5j: a copy of a legend (here a token with the same legendary name) is in
// the group.
#[test]
fn req_sba_001_704_5j_copy_of_a_legend_counts() {
    let mut s = Sim::new();
    let original = s.legend(P1, P1, "test.legend_a");
    let copy = s.token(P1, ZoneKind::Battlefield);
    s.describe(copy, legend_chars("test.legend_a"));
    assert_eq!(
        s.check(),
        vec![
            finding("704.5j", obj(original)),
            finding("704.5j", obj(copy))
        ]
    );
    let r = drive_sba(s, |_, _| keep(original));
    assert_eq!(r.requests.len(), 1);
    assert_still_on_battlefield(&r, original);
    assert!(r.after.zones.object(copy).is_none(), "the copy left");
    assert!(
        r.moved_to(copy).is_some(),
        "the copy was put into a graveyard"
    );
}

// REQ-SBA-001, CR 704.5j, 704.3: `check` alone reports every member of every group and asks
// nothing (it needs no choice context) and changes nothing.
#[test]
fn req_sba_001_704_5j_check_reports_all_members_and_asks_nothing() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let b = s.legend(P1, P1, "test.legend_a");
    let c = s.legend(P2, P2, "test.legend_b");
    let d = s.legend(P2, P2, "test.legend_b");
    let e = s.legend(P2, P2, "test.legend_b");
    let before = s.snap();
    let findings = s.check();
    assert_eq!(
        findings,
        [a, b, c, d, e]
            .iter()
            .map(|id| finding("704.5j", obj(*id)))
            .collect::<Vec<_>>()
    );
    assert_eq!(s.snap(), before, "check is pure");
}

// REQ-SBA-001, CR 704.5j, 704.3: while the legend choice is pending, nothing has changed (the
// visible state is the base state; `call_sba` also asserts the pending call left every
// component unchanged); the state changes only once the request is answered.
#[test]
fn req_sba_001_704_5j_pending_choice_changes_nothing_until_answered() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let b = s.legend(P1, P1, "test.legend_a");
    let z = s.creature(P2, "test.vanilla_2_0", 2, 0);
    let base = s.clone();
    let base_snap = base.snap();
    let mut session = ChoiceSession::start(base.clone(), call_sba).expect("starts");
    let req = session
        .pending()
        .cloned()
        .expect("the legend choice is pending");
    assert_legend_request(&req, P1, &[a, b]);
    assert_eq!(session.base().snap(), base_snap);
    assert!(session.base().on_battlefield(a));
    assert!(session.base().on_battlefield(b));
    assert!(
        session.base().on_battlefield(z),
        "no other SBA ran before the answer"
    );
    assert!(matches!(session.status(), Status::Pending(_)));

    session
        .submit(req.request_id, keep(b))
        .expect("valid answer");
    match session.status() {
        Status::Done { state, output } => {
            let o = output.as_ref().expect("an outcome");
            assert!(o.performed);
            assert!(state.on_battlefield(b));
            assert!(state.zones.object(a).is_none());
            assert!(state.zones.object(z).is_none(), "704.5f in the same pass");
        }
        Status::Pending(p) => panic!("still pending: {p:?}"),
    }
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5j: replaying the recorded answers reproduces the same
// state hash and outcome.
#[test]
fn req_sba_001_704_5j_replay_is_deterministic() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let _b = s.legend(P1, P1, "test.legend_a");
    let r = drive_sba(s.clone(), |_, req| keep(*candidates(req).last().unwrap()));
    let replay = ChoiceSession::replay(s, call_sba, &r.answers).expect("replays");
    assert_eq!(replay.state_hash(), r.hash);
    match replay.status() {
        Status::Done { state, output } => {
            assert_eq!(state.snap(), r.after.snap());
            assert_eq!(output, &r.result);
            assert!(state.zones.object(a).is_none());
        }
        Status::Pending(p) => panic!("replay still pending: {p:?}"),
    }
}
