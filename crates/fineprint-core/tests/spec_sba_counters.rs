// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the counter SBAs (REQ-SBA-001, REQ-CORE-001; CR 704.5q, 704.5r, 704.3).
//! Black-box: written from the work-item brief only. Every card is synthetic (R5).
//!
//! CR 704.5q: "If a permanent has both a +1/+1 counter and a -1/-1 counter on it, N +1/+1 and
//! N -1/-1 counters are removed from it, where N is the smaller of the number of +1/+1 and
//! -1/-1 counters on it." CR 704.5r: "If a permanent with an ability that says it can't have
//! more than N counters of a certain kind on it has more than N counters of that kind on it,
//! all but N of those counters are removed from it."

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use support::*;

/// The `(kind, amount)` removals of `id` in `r`, sorted by kind.
fn removals_of(r: &SbaRun, id: ObjectId) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = counter_removals(r)
        .into_iter()
        .filter(|(o, _, _)| *o == id)
        .map(|(_, k, n)| (k, n))
        .collect();
    v.sort();
    v
}

/// A plain artifact (no toughness, so 704.5f-704.5g cannot apply).
fn artifact(s: &mut Sim) -> ObjectId {
    s.permanent(P1, P1, artifact_chars("test.artifact_a"))
}

// ---------------------------------------------------------------------------------------------
// 704.5q
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5q: two +1/+1 and three -1/-1 counters: two of each are removed (one
// RemoveCounters event per kind, in one batch), leaving one -1/-1 counter; no choice is asked.
#[test]
fn req_sba_001_704_5q_two_vs_three() {
    for (plus, minus) in [(2, 3), (3, 2)] {
        let mut s = Sim::new();
        let c = s.creature(P1, "test.vanilla_3_5", 3, 5);
        s.counters(c, "+1/+1", plus);
        s.counters(c, "-1/-1", minus);
        assert_eq!(s.check(), vec![finding("704.5q", obj(c))]);
        let r = run_sba(s);
        assert!(r.outcome().performed);
        assert_eq!(r.outcome().findings, vec![finding("704.5q", obj(c))]);
        assert_eq!(
            removals_of(&r, c),
            vec![("+1/+1".to_owned(), 2), ("-1/-1".to_owned(), 2)]
        );
        assert_eq!(r.after.counter(c, "+1/+1"), plus - 2);
        assert_eq!(r.after.counter(c, "-1/-1"), minus - 2);
        assert_still_on_battlefield(&r, c);
        assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");
        assert!(r.requests.is_empty());
    }
}

// REQ-SBA-001, CR 704.5q: equal counts remove all counters of both kinds.
#[test]
fn req_sba_001_704_5q_equal_counts() {
    let mut s = Sim::new();
    let c = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.counters(c, "+1/+1", 2);
    s.counters(c, "-1/-1", 2);
    let r = run_sba(s);
    assert_eq!(
        removals_of(&r, c),
        vec![("+1/+1".to_owned(), 2), ("-1/-1".to_owned(), 2)]
    );
    assert_eq!(r.after.counter(c, "+1/+1"), 0);
    assert_eq!(r.after.counter(c, "-1/-1"), 0);
    assert_still_on_battlefield(&r, c);
}

// REQ-SBA-001, CR 704.5q: only one kind of P/T counter: untouched, no finding.
#[test]
fn req_sba_001_704_5q_one_kind_untouched() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.counters(a, "+1/+1", 3);
    let b = s.creature(P1, "test.vanilla_2_4", 2, 4);
    s.counters(b, "-1/-1", 2);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(counter_removals(&r).is_empty());
    assert_eq!(r.after.counter(a, "+1/+1"), 3);
    assert_eq!(r.after.counter(b, "-1/-1"), 2);
}

// REQ-SBA-001, CR 704.5q: the rule applies to any permanent, not only creatures.
#[test]
fn req_sba_001_704_5q_noncreature_permanent() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "+1/+1", 1);
    s.counters(a, "-1/-1", 4);
    assert_eq!(s.check(), vec![finding("704.5q", obj(a))]);
    let r = run_sba(s);
    assert_eq!(r.after.counter(a, "+1/+1"), 0);
    assert_eq!(r.after.counter(a, "-1/-1"), 3);
}

// REQ-SBA-001, CR 704.5q, 702.26b: a phased-out permanent is skipped.
#[test]
fn req_sba_001_704_5q_phased_out_skipped() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.counters(c, "+1/+1", 1);
    s.counters(c, "-1/-1", 1);
    s.phase_out(c);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(counter_removals(&r).is_empty());
    assert_eq!(r.after.counter(c, "+1/+1"), 1);
}

// ---------------------------------------------------------------------------------------------
// 704.5r
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5r: five counters of a kind with a limit of three: two are removed.
#[test]
fn req_sba_001_704_5r_limit_with_excess() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "charge", 5);
    s.limit(a, "charge", 3);
    assert_eq!(s.check(), vec![finding("704.5r", obj(a))]);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, vec![finding("704.5r", obj(a))]);
    assert_eq!(removals_of(&r, a), vec![("charge".to_owned(), 2)]);
    assert_eq!(r.after.counter(a, "charge"), 3);
    assert_still_on_battlefield(&r, a);
    assert!(r.requests.is_empty());
}

// REQ-SBA-001, CR 704.5r: at the limit (and below it) nothing happens.
#[test]
fn req_sba_001_704_5r_at_limit_untouched() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "charge", 3);
    s.limit(a, "charge", 3);
    let b = artifact(&mut s);
    s.counters(b, "charge", 1);
    s.limit(b, "charge", 3);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(counter_removals(&r).is_empty());
    assert_eq!(r.after.counter(a, "charge"), 3);
}

// REQ-SBA-001, CR 704.5r: a limit for a different kind does not touch other kinds.
#[test]
fn req_sba_001_704_5r_limit_for_other_kind() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "charge", 5);
    s.limit(a, "time", 1);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(counter_removals(&r).is_empty());
    assert_eq!(r.after.counter(a, "charge"), 5);
}

// REQ-SBA-001, CR 704.5r: a limit of zero removes every counter of that kind; two limited kinds
// on one permanent each lose their excess.
#[test]
fn req_sba_001_704_5r_limit_zero_and_two_kinds() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "charge", 2);
    s.counters(a, "time", 4);
    s.limit(a, "charge", 0);
    s.limit(a, "time", 1);
    assert_eq!(s.check(), vec![finding("704.5r", obj(a))]);
    let r = run_sba(s);
    assert_eq!(
        removals_of(&r, a),
        vec![("charge".to_owned(), 2), ("time".to_owned(), 3)]
    );
    assert_eq!(r.after.counter(a, "charge"), 0);
    assert_eq!(r.after.counter(a, "time"), 1);
}

// REQ-SBA-001, CR 704.5r, 702.26b: a phased-out permanent is skipped.
#[test]
fn req_sba_001_704_5r_phased_out_skipped() {
    let mut s = Sim::new();
    let a = artifact(&mut s);
    s.counters(a, "charge", 4);
    s.limit(a, "charge", 1);
    s.phase_out(a);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert_eq!(r.after.counter(a, "charge"), 4);
}

// ---------------------------------------------------------------------------------------------
// one pass, findings and no looping (CR 704.3)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5q, 704.5r, 704.3: findings report the permanents in registry order then
// subject order; every removal is a RemoveCounters event of the pass's single batch.
#[test]
fn req_sba_001_counters_findings_and_events() {
    let mut s = Sim::new();
    let limited = artifact(&mut s);
    s.counters(limited, "charge", 2);
    s.limit(limited, "charge", 1);
    let pt = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.counters(pt, "+1/+1", 1);
    s.counters(pt, "-1/-1", 1);
    let expected = vec![finding("704.5q", obj(pt)), finding("704.5r", obj(limited))];
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    let batches = r.nonempty_batches();
    assert_eq!(batches.len(), 1);
    let mut all = counter_removals(&r);
    all.sort();
    let mut want = vec![
        (pt, "+1/+1".to_owned(), 1),
        (pt, "-1/-1".to_owned(), 1),
        (limited, "charge".to_owned(), 1),
    ];
    want.sort();
    assert_eq!(all, want);
    assert_eq!(batches[0].events.len(), 3, "only the removals");
}

// REQ-SBA-001, CR 704.3, 704.5r, 704.5f: one `perform_all` does not loop: a 1/0 creature with a
// +1/+1 counter and a limit of zero +1/+1 counters loses the counter; the toughness-0 result is
// found only by the next `check`.
#[test]
fn req_sba_001_counters_no_loop_within_one_pass() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_1_0", 1, 0);
    s.counters(c, "+1/+1", 1);
    s.limit(c, "+1/+1", 0);
    assert_eq!(s.check(), vec![finding("704.5r", obj(c))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5r", obj(c))]);
    assert_still_on_battlefield(&r, c);
    assert_eq!(r.after.counter(c, "+1/+1"), 0);
    let mut after = r.after.clone();
    after.sync_after(r.outcome());
    after.sync_counters();
    assert_eq!(after.check(), vec![finding("704.5f", obj(c))]);
}

// REQ-SBA-001, CR 704.3, 704.5q, 704.5r: a permanent found by both 704.5q and 704.5r in one
// check is reported by both entries in registry order; after the pass (and as many further
// passes as the heartbeat would run) it has no P/T counters of both kinds and at most the
// limit of the limited kind.
#[test]
fn req_sba_001_counters_q_and_r_together_settle() {
    let mut s = Sim::new();
    let c = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.counters(c, "+1/+1", 4);
    s.counters(c, "-1/-1", 1);
    s.limit(c, "+1/+1", 2);
    assert_eq!(
        s.check(),
        vec![finding("704.5q", obj(c)), finding("704.5r", obj(c))]
    );
    let mut cur = s;
    for _ in 0..4 {
        let r = run_sba(cur);
        let mut next = r.after.clone();
        next.sync_after(r.outcome());
        next.sync_counters();
        cur = next;
        if !r.outcome().performed {
            break;
        }
    }
    assert!(cur.check().is_empty(), "the heartbeat settles");
    let plus = cur.counter(c, "+1/+1");
    let minus = cur.counter(c, "-1/-1");
    assert!(plus <= 2, "+1/+1 within the limit: {plus}");
    assert!(plus == 0 || minus == 0, "not both kinds: {plus}/{minus}");
}
