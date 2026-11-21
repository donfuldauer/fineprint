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

// REQ-SBA-001, CR 704.5j, 101.4: two disjoint groups (interleaved ids, different controllers)
// are asked one request each, each by its controller, in APNAP order of the controller (the
// active player P1 first) even though the nonactive player's group holds the lowest id; both
// moves happen in one event.
// (PR #214 round 1, F3: this test asserted ascending lowest-id order across controllers; the
// brief's contract 1 orders requests by the controller's APNAP rank first.)
#[test]
fn req_sba_001_704_5j_two_groups_apnap_then_lowest_id() {
    let mut s = Sim::new();
    // Group X (controller P2, nonactive) gets the lowest id.
    let x1 = s.legend(P2, P2, "test.legend_x");
    let y1 = s.legend(P1, P1, "test.legend_y");
    let x2 = s.legend(P2, P2, "test.legend_x");
    let y2 = s.legend(P1, P1, "test.legend_y");
    let r = drive_sba(s, |i, req| {
        if i == 0 {
            keep(candidates(req)[0])
        } else {
            keep(candidates(req)[1])
        }
    });
    assert_eq!(r.requests.len(), 2);
    assert_legend_request(&r.requests[0], P1, &[y1, y2]);
    assert_legend_request(&r.requests[1], P2, &[x1, x2]);
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

// =============================================================================================
// PR #214 round 1: F7 (same name is pairwise, CR 201.2a), F3 (APNAP, CR 101.4), F4 (evaluated
// controller, CR 613.1b, 110.2)
// =============================================================================================

use fineprint_base::ids::{ObjectId, PlayerId};

/// One legend of a hand-built board: names and owner, all controlled by `controller`.
fn build(
    s: &mut Sim,
    controller: PlayerId,
    legends: &[(&[&str], PlayerId)],
) -> (Vec<ObjectId>, NameMap) {
    let mut ids = Vec::new();
    let mut names = NameMap::new();
    for (n, owner) in legends {
        let id = s.legend_named(*owner, controller, n);
        names.insert(id, name_set(n));
        ids.push(id);
    }
    (ids, names)
}

/// A={X,Y}, B={Y}, C={X} (ids A<B<C) under P1; B is owned by P2.
fn non_transitive() -> (Sim, [ObjectId; 3], NameMap) {
    let mut s = Sim::new();
    let (ids, names) = build(
        &mut s,
        P1,
        &[
            (&["test.name_x", "test.name_y"], P1),
            (&["test.name_y"], P2),
            (&["test.name_x"], P1),
        ],
    );
    (s, [ids[0], ids[1], ids[2]], names)
}

/// A={X,Y}, B={Y,Z}, C={X,Z} under P1: every pair shares a name, no name is common to all.
fn cyclic() -> (Sim, [ObjectId; 3], NameMap) {
    let mut s = Sim::new();
    let (ids, names) = build(
        &mut s,
        P1,
        &[
            (&["test.name_x", "test.name_y"], P1),
            (&["test.name_y", "test.name_z"], P1),
            (&["test.name_x", "test.name_z"], P1),
        ],
    );
    (s, [ids[0], ids[1], ids[2]], names)
}

/// A path A={P,Q}, B={Q,R}, C={R,S}, D={S} under P1 (valid survivor sets {A,C}, {A,D}, {B,D}).
fn path_of_four() -> (Sim, Vec<ObjectId>, NameMap) {
    let mut s = Sim::new();
    let (ids, names) = build(
        &mut s,
        P1,
        &[
            (&["test.name_p", "test.name_q"], P1),
            (&["test.name_q", "test.name_r"], P2),
            (&["test.name_r", "test.name_s"], P1),
            (&["test.name_s"], P2),
        ],
    );
    (s, ids, names)
}

/// Asserts the final-state property of CR 704.5j on `group` after `r`: survivors are exactly
/// `kept`; no two survivors share a name; every removed member shared a name with a survivor
/// and went to its owner's graveyard as an SBA, all in one event.
fn assert_survivors(r: &SbaRun, group: &[ObjectId], names: &NameMap, kept: &[ObjectId]) {
    let left: Vec<ObjectId> = group
        .iter()
        .copied()
        .filter(|id| r.after.on_battlefield(*id))
        .collect();
    let mut want = kept.to_vec();
    want.sort();
    assert_eq!(left, want, "survivors");
    for (i, a) in left.iter().enumerate() {
        for b in &left[i + 1..] {
            assert!(
                !share_name(&names[a], &names[b]),
                "survivors {a:?} and {b:?} share a name"
            );
        }
        assert_still_on_battlefield(r, *a);
    }
    for id in group.iter().filter(|id| !left.contains(id)) {
        assert!(
            left.iter().any(|k| share_name(&names[id], &names[k])),
            "{id:?} was removed without sharing a name with a survivor"
        );
        assert_to_owners_graveyard(r, *id, ZoneChangeCause::StateBasedAction);
    }
    if left.len() < group.len() {
        assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");
    }
}

/// Every subset of `group` (ascending) with 1..=max members.
fn subsets(group: &[ObjectId], max: u64) -> Vec<Vec<ObjectId>> {
    let mut out = Vec::new();
    for mask in 1u32..(1u32 << group.len()) {
        let sub: Vec<ObjectId> = group
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, id)| *id)
            .collect();
        if sub.len() as u64 <= max {
            out.push(sub);
        }
    }
    out
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): A={X,Y}, B={Y}, C={X} under one controller form ONE
// component: `check` reports all three; one request with candidates [A,B,C], min 1, max 2 (not
// a clique: B and C share no name) and the greedy default {A}.
#[test]
fn req_sba_001_704_5j_f7_non_transitive_request_shape() {
    let (mut s, [a, b, c], names) = non_transitive();
    assert_eq!(
        s.check(),
        vec![
            finding("704.5j", obj(a)),
            finding("704.5j", obj(b)),
            finding("704.5j", obj(c))
        ]
    );
    let r = drive_answers(s, &[survivors(&[a])]);
    let req = &r.requests[0];
    assert_component_request(req, P1, &[a, b, c], &names);
    assert_eq!(candidates(req), vec![a, b, c]);
    assert_eq!((req.min, req.max), (1, 2));
    assert_eq!(req.default_on_timeout, survivors(&[a]));
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): both legal end states are reachable: answering {A}
// leaves {A} (B and C to their owners' graveyards), answering {B,C} leaves {B,C} (A to its
// owner's graveyard).
#[test]
fn req_sba_001_704_5j_f7_non_transitive_both_end_states() {
    for kept in [vec![0usize], vec![1, 2]] {
        let (s, ids, names) = non_transitive();
        let keep_ids: Vec<ObjectId> = kept.iter().map(|i| ids[*i]).collect();
        let r = drive_answers(s, &[survivors(&keep_ids)]);
        assert!(r.outcome().performed);
        assert_eq!(
            r.outcome().findings,
            ids.iter()
                .map(|id| finding("704.5j", obj(*id)))
                .collect::<Vec<_>>()
        );
        assert_survivors(&r, &ids, &names, &keep_ids);
    }
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): invalid survivor sets are rejected and nothing is
// mutated: {A,B} (share Y), {B} alone (C shares no name with B: not maximal), {C} alone (B
// shares no name with C: not maximal), {} (empty), a duplicate id and a non-candidate id.
#[test]
fn req_sba_001_704_5j_f7_non_transitive_invalid_answers_rejected() {
    let (mut s, [a, b, c], names) = non_transitive();
    // A battlefield object that is not a candidate.
    let outsider = s.creature(P1, "test.vanilla_2_2", 2, 2);
    for bad in [
        vec![a, b],
        vec![b],
        vec![c],
        vec![],
        vec![b, b],
        vec![a, a],
        vec![b, c, c],
        vec![a, outsider],
    ] {
        assert!(!is_valid_survivor_set(&[a, b, c], &names, &bad));
        let _ = assert_answer_rejected(s.clone(), &[], survivors(&bad));
    }
}

// REQ-SBA-001, CR 704.5j (F7): the timeout default of the non-transitive component is {A}
// (lowest id, then no id sharing a name with it), and applying it leaves {A}.
#[test]
fn req_sba_001_704_5j_f7_non_transitive_default_answer() {
    let (s, ids, names) = non_transitive();
    let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
    assert_eq!(r.requests.len(), 1);
    assert_eq!(r.answers, vec![survivors(&[ids[0]])]);
    assert_survivors(&r, &ids, &names, &[ids[0]]);
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): the cyclic component {X,Y},{Y,Z},{X,Z}: every pair
// shares a name, so it is a clique: min = max = 1 (contract 1; the brief's T1 "max = 2" for
// this case contradicts contract 1's clique rule and is reported); any single member is a
// valid answer and leaves exactly that member; any two members are rejected.
#[test]
fn req_sba_001_704_5j_f7_cyclic_component() {
    let (s, ids, names) = cyclic();
    assert!(is_clique(&ids, &names));
    for keep_one in ids {
        let r = drive_answers(s.clone(), &[survivors(&[keep_one])]);
        assert_component_request(&r.requests[0], P1, &ids, &names);
        assert_eq!((r.requests[0].min, r.requests[0].max), (1, 1));
        assert_survivors(&r, &ids, &names, &[keep_one]);
    }
    for pair in [[ids[0], ids[1]], [ids[0], ids[2]], [ids[1], ids[2]]] {
        let _ = assert_answer_rejected(s.clone(), &[], survivors(&pair));
    }
    let _ = assert_answer_rejected(s, &[], survivors(&ids));
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): exhaustively over every nonempty subset of at most
// `max` members of three components (non-transitive, cyclic, a path of four), a subset is
// accepted iff it is a maximal independent set, and an accepted answer leaves exactly it; the
// oracle and the request agree on the bounds and the default.
#[test]
fn req_sba_001_704_5j_f7_exhaustive_answers() {
    let boards: Vec<(Sim, Vec<ObjectId>, NameMap)> = vec![
        {
            let (s, ids, n) = non_transitive();
            (s, ids.to_vec(), n)
        },
        {
            let (s, ids, n) = cyclic();
            (s, ids.to_vec(), n)
        },
        path_of_four(),
    ];
    let mut accepted = 0;
    let mut rejected = 0;
    for (s, ids, names) in boards {
        assert_eq!(conflict_components(&names), vec![ids.clone()]);
        let max = expected_max(&ids, &names);
        let valid = all_survivor_sets(&ids, &names);
        assert!(!valid.is_empty());
        for sub in subsets(&ids, max) {
            if valid.contains(&sub) {
                let r = drive_answers(s.clone(), &[survivors(&sub)]);
                assert_component_request(&r.requests[0], P1, &ids, &names);
                assert_survivors(&r, &ids, &names, &sub);
                accepted += 1;
            } else {
                let _ = assert_answer_rejected(s.clone(), &[], survivors(&sub));
                rejected += 1;
            }
        }
    }
    // non-transitive: {A}, {B,C}; cyclic: 3 singles; path: {A,C}, {A,D}, {B,D}.
    assert_eq!(accepted, 2 + 3 + 3);
    assert!(rejected > 0);
}

// REQ-SBA-001, CR 704.5j (F7): a plain same-name pair and a plain same-name triple are cliques:
// min = max = 1 and the default keeps the lowest id, answered as before.
#[test]
fn req_sba_001_704_5j_f7_plain_groups_keep_max_one() {
    for size in [2usize, 3] {
        let mut s = Sim::new();
        let spec: Vec<(&[&str], PlayerId)> = vec![(&["test.legend_a"], P1); size];
        let (ids, names) = build(&mut s, P1, &spec);
        let kept = ids[size - 1];
        let r = drive_answers(s, &[keep(kept)]);
        assert_legend_request(&r.requests[0], P1, &ids);
        assert_component_request(&r.requests[0], P1, &ids, &names);
        assert_survivors(&r, &ids, &names, &[kept]);
    }
}

// REQ-SBA-001, CR 704.5j, 201.2a (F7): isolated legends (no name in common with any other
// legend of their controller, including nameless ones) are never asked or reported, even
// next to a component.
#[test]
fn req_sba_001_704_5j_f7_isolated_legends_never_asked() {
    let mut s = Sim::new();
    let (ids, names) = build(
        &mut s,
        P1,
        &[
            (&["test.name_x"], P1),
            (&["test.name_y", "test.name_w"], P1),
            (&[], P1),
            (&["test.name_x", "test.name_v"], P1),
            (&[], P1),
            (&["test.name_u"], P1),
        ],
    );
    let comp = vec![ids[0], ids[3]];
    assert_eq!(conflict_components(&names), vec![comp.clone()]);
    assert_eq!(
        s.check(),
        vec![
            finding("704.5j", obj(ids[0])),
            finding("704.5j", obj(ids[3]))
        ]
    );
    let r = drive_answers(s, &[survivors(&[ids[3]])]);
    assert_component_request(&r.requests[0], P1, &comp, &names);
    for id in [ids[1], ids[2], ids[3], ids[4], ids[5]] {
        assert_still_on_battlefield(&r, id);
    }
    assert_to_owners_graveyard(&r, ids[0], ZoneChangeCause::StateBasedAction);

    // Only isolated legends: no request, nothing reported or performed.
    let mut s = Sim::new();
    let (ids, _) = build(
        &mut s,
        P1,
        &[
            (&["test.name_x"], P1),
            (&["test.name_y"], P1),
            (&[], P1),
            (&["test.name_z", "test.name_w"], P1),
        ],
    );
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    for id in ids {
        assert_still_on_battlefield(&r, id);
    }
}

// REQ-SBA-001, CR 704.5j, 704.3 (F7): `check` alone reports every member of each component
// with an edge (A, B, C of the non-transitive component), asks nothing and changes nothing.
#[test]
fn req_sba_001_704_5j_f7_check_reports_component_members() {
    let (mut s, [a, b, c], _) = non_transitive();
    let lone = s.legend_named(P1, P1, &["test.name_q"]);
    let before = s.snap();
    assert_eq!(
        s.check(),
        vec![
            finding("704.5j", obj(a)),
            finding("704.5j", obj(b)),
            finding("704.5j", obj(c))
        ]
    );
    assert!(!s.check().contains(&finding("704.5j", obj(lone))));
    assert_eq!(s.snap(), before, "check is pure");
}

// REQ-SBA-001, CR 704.5j, 101.4, 704.3 (F3): two players each with a same-name pair; the
// ACTIVE player's (P2's) legends have the higher ids: the first request goes to the active
// player, the second to the other; nothing moves before both are answered (the pending pass
// leaves every component unchanged, checked by `call_sba`); both moves land in one batch.
#[test]
fn req_sba_001_704_5j_f3_active_player_asked_first() {
    let mut s = Sim::new();
    let n1 = s.legend(P1, P1, "test.legend_a");
    let n2 = s.legend(P1, P1, "test.legend_a");
    let a1 = s.legend(P2, P2, "test.legend_a");
    let a2 = s.legend(P2, P2, "test.legend_a");
    s.set_turn(&[P1, P2], P2);
    let base = s.clone();

    // Step through the session: after the first answer the second request is pending and the
    // visible state is still the base state.
    let mut session = ChoiceSession::start(base.clone(), call_sba).expect("starts");
    let first = session.pending().cloned().expect("first request");
    assert_legend_request(&first, P2, &[a1, a2]);
    session
        .submit(first.request_id, keep(a2))
        .expect("valid answer");
    let second = session.pending().cloned().expect("second request");
    assert_legend_request(&second, P1, &[n1, n2]);
    assert_eq!(session.base().snap(), base.snap());

    let r = drive_answers(base, &[keep(a2), keep(n1)]);
    assert_eq!(r.requests[0].chooser, P2);
    assert_eq!(r.requests[1].chooser, P1);
    assert_still_on_battlefield(&r, a2);
    assert_still_on_battlefield(&r, n1);
    assert_to_owners_graveyard(&r, a1, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, n2, ZoneChangeCause::StateBasedAction);
    assert_eq!(r.nonempty_batches().len(), 1, "both moves in one event");
    assert_eq!(r.moves().len(), 2);
}

// REQ-SBA-001, CR 704.5j, 101.4, 101.4c (F3): three players, turn order different from id
// order, the active player is P2 (the middle id): requests go to the active player, then the
// next player in turn order, then the remaining one; one player's two disjoint groups are
// asked in ascending order of their lowest id. Object ids are created in the reverse of the
// expected request order. Two turn orders.
#[test]
fn req_sba_001_704_5j_f3_three_players_apnap() {
    for (order, apnap) in [([P1, P3, P2], [P2, P1, P3]), ([P2, P3, P1], [P2, P3, P1])] {
        let mut s = Sim::with_players(3);
        // Interleaved creation: P3's pair, P1's X group, P2's pair, P1's Y group.
        let p3a = s.legend(P3, P3, "test.legend_c");
        let p1x1 = s.legend(P1, P1, "test.legend_x");
        let p3b = s.legend(P3, P3, "test.legend_c");
        let p1y1 = s.legend(P1, P1, "test.legend_y");
        let p2a = s.legend(P2, P2, "test.legend_b");
        let p1x2 = s.legend(P1, P1, "test.legend_x");
        let p2b = s.legend(P2, P2, "test.legend_b");
        let p1y2 = s.legend(P1, P1, "test.legend_y");
        s.set_turn(&order, P2);
        let groups = |p: PlayerId| -> Vec<Vec<ObjectId>> {
            match p {
                p if p == P1 => vec![vec![p1x1, p1x2], vec![p1y1, p1y2]],
                p if p == P2 => vec![vec![p2a, p2b]],
                _ => vec![vec![p3a, p3b]],
            }
        };
        let mut expected: Vec<(PlayerId, Vec<ObjectId>)> = Vec::new();
        for p in apnap {
            for g in groups(p) {
                expected.push((p, g));
            }
        }
        let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
        assert_eq!(r.requests.len(), 4, "{order:?}");
        for (req, (p, g)) in r.requests.iter().zip(&expected) {
            assert_legend_request(req, *p, g);
        }
        for (_, g) in &expected {
            assert_still_on_battlefield(&r, g[0]);
            assert_to_owners_graveyard(&r, g[1], ZoneChangeCause::StateBasedAction);
        }
        assert_eq!(r.nonempty_batches().len(), 1, "one event: {order:?}");
        assert_eq!(r.moves().len(), 4);
    }
}

// REQ-SBA-001, CR 704.5j, 613.1b, 110.2 (F4): two same-name legends whose STORED controllers
// differ (P2 and P1) but whose evaluated controller is P1 (a layer 2 control-changing effect
// gives P1 the first one) form one group asked to P1; the removed one goes to its owner's
// graveyard.
#[test]
fn req_sba_001_704_5j_f4_evaluated_controller_forms_group() {
    for keep_stolen in [true, false] {
        let mut s = Sim::new();
        let stolen = s.legend(P2, P2, "test.legend_a");
        let own = s.legend(P1, P1, "test.legend_a");
        s.gain_control(1, stolen, P1);
        assert_eq!(
            s.check(),
            vec![finding("704.5j", obj(stolen)), finding("704.5j", obj(own))]
        );
        let (kept, gone) = if keep_stolen {
            (stolen, own)
        } else {
            (own, stolen)
        };
        let r = drive_answers(s, &[keep(kept)]);
        assert_legend_request(&r.requests[0], P1, &[stolen, own]);
        assert_still_on_battlefield(&r, kept);
        assert_to_owners_graveyard(&r, gone, ZoneChangeCause::StateBasedAction);
    }
}

// REQ-SBA-001, CR 704.5j, 613.1b (F4): the reverse case: two same-name legends with the same
// STORED controller P1 whose evaluated controllers differ (a layer 2 effect gives P2 the second
// one): no group, no request, no change.
#[test]
fn req_sba_001_704_5j_f4_evaluated_controllers_differ_no_group() {
    let mut s = Sim::new();
    let a = s.legend(P1, P1, "test.legend_a");
    let b = s.legend(P1, P1, "test.legend_a");
    s.gain_control(1, b, P2);
    assert!(s.check().is_empty());
    let before = s.snap();
    let r = run_sba(s);
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, a);
    assert_still_on_battlefield(&r, b);
    let mut after = r.after.clone();
    after.env.clear_calls = before.env.clear_calls;
    assert_eq!(after.snap(), before, "no change");
}

// REQ-SBA-001, CR 704.5j, 613.1b, 101.4 (F4 with F3): the evaluated controller also decides the
// APNAP position: a pair stored under P1 but controlled by P2 (active) is asked first, to P2,
// before P1's own pair, although P1's pair has the lower ids.
#[test]
fn req_sba_001_704_5j_f4_evaluated_controller_decides_apnap() {
    let mut s = Sim::new();
    let o1 = s.legend(P1, P1, "test.legend_a");
    let o2 = s.legend(P1, P1, "test.legend_a");
    let t1 = s.legend(P1, P1, "test.legend_b");
    let t2 = s.legend(P1, P1, "test.legend_b");
    s.add_layer_effect(1, control_effect(&[t1, t2], P2, 1));
    s.set_turn(&[P1, P2], P2);
    let r = drive_answers(s, &[keep(t2), keep(o2)]);
    assert_legend_request(&r.requests[0], P2, &[t1, t2]);
    assert_legend_request(&r.requests[1], P1, &[o1, o2]);
    assert_still_on_battlefield(&r, t2);
    assert_still_on_battlefield(&r, o2);
    assert_to_owners_graveyard(&r, t1, ZoneChangeCause::StateBasedAction);
    assert_to_owners_graveyard(&r, o1, ZoneChangeCause::StateBasedAction);
    assert_eq!(r.nonempty_batches().len(), 1);
}
