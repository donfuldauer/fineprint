// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for phased-out permanents and the state-based actions (REQ-SBA-001; CR 702.26b,
//! 702.26d, 704.5d, 704.5f, 704.5g, 704.5h, 704.5i). Black-box: written from the work-item
//! brief only. A phased-out permanent is treated as though it does not exist (CR 702.26b), so
//! 704.5f-704.5i ignore it; marked damage and counters stay while it is phased out
//! (CR 702.26d), so 704.5f, 704.5g and 704.5i apply again once it phases in.
//!
//! The deathtouch flag (CR 704.5h, 702.2b: dealt deathtouch damage "since the last time
//! state-based actions were checked") is different, and the tests separate two cases:
//! - **phasing alone** (no completed SBA check while it is phased out, only the pure `check`):
//!   the flag is kept, and 704.5h applies once the creature phases in;
//! - **an intervening completed check** (a `perform_all` pass while it is phased out, even one
//!   that performs nothing): that check closes the window and consumes the flag, so after
//!   phasing in 704.5h does not apply until the creature is dealt deathtouch damage again.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::layers::keyword_counter_ability;
use fineprint_core::sba::SbaSubject;
use fineprint_core::txn::Transactional;
use fineprint_core::zones::{ZoneId, ZoneKind};
use support::*;

/// Sets the phased-out status of `id` and checks it reads back.
fn phase(s: &mut Sim, id: ObjectId, out: bool) {
    s.permanents
        .set_phased_out(id, out)
        .expect("a permanent on the battlefield");
    assert_eq!(
        s.permanents.get(id).expect("permanent").status().phased_out,
        out
    );
}

/// No finding of `check` names `id`.
fn no_finding_for(s: &mut Sim, id: ObjectId) {
    let findings = s.check();
    assert!(
        findings.iter().all(|f| f.subject != SbaSubject::Object(id)),
        "phased-out {id:?} must have no finding, got {findings:?}"
    );
}

/// Asserts that `id` is still on the battlefield, unmoved, after `r`, and still phased out.
fn assert_stayed_phased_out(r: &SbaRun, id: ObjectId) {
    assert!(
        r.after.on_battlefield(id),
        "{id:?} stayed on the battlefield"
    );
    assert!(r.moved_to(id).is_none(), "{id:?} did not move");
    assert!(!r.outcome().ceased.contains(&id), "{id:?} did not cease");
    assert!(
        r.outcome()
            .findings
            .iter()
            .all(|f| f.subject != SbaSubject::Object(id)),
        "no finding for phased-out {id:?}"
    );
    assert!(
        r.after
            .permanents
            .get(id)
            .expect("still a permanent")
            .status()
            .phased_out,
        "{id:?} is still phased out"
    );
}

/// Asserts that `old` left the battlefield for its owner's graveyard through one SBA move.
fn assert_died(r: &SbaRun, old: ObjectId) {
    let owner = r.before.zones.object(old).expect("existed").owner;
    assert!(r.after.zones.object(old).is_none(), "{old:?} left");
    let moves: Vec<_> = r.moves().into_iter().filter(|m| m.0 == old).collect();
    assert_eq!(moves.len(), 1, "{old:?} moved exactly once");
    let (_, new, kind, cause) = moves[0];
    assert_eq!(kind, ZoneKind::Graveyard);
    // CR 701.8b: 704.5g and 704.5h destroy, so those moves carry `StateBasedDestroy`; every other
    // SBA move here carries `StateBasedAction`.
    let destroyed =
        r.outcome().findings.iter().any(|f| {
            f.subject == SbaSubject::Object(old) && matches!(f.entry_id, "704.5g" | "704.5h")
        });
    let expected = if destroyed {
        ZoneChangeCause::StateBasedDestroy
    } else {
        ZoneChangeCause::StateBasedAction
    };
    assert_eq!(cause, expected);
    assert_eq!(
        r.after.zones.object(new).expect("moved object").zone,
        ZoneId::Graveyard(owner)
    );
}

/// A pass over a board whose only candidate is phased out does nothing.
fn assert_idle_pass(s: &Sim, id: ObjectId) -> SbaRun {
    let r = run_sba(s.clone());
    assert!(!r.outcome().performed, "nothing qualifies");
    assert!(r.outcome().findings.is_empty());
    assert!(r.nonempty_batches().is_empty(), "no events executed");
    assert!(r.outcome().ceased.is_empty());
    assert!(r.outcome().lost_players.is_empty());
    assert_stayed_phased_out(&r, id);
    r
}

// REQ-SBA-001, CR 704.5f, 702.26b: a phased-out 0-toughness creature has no finding and stays;
// phased in, 704.5f finds it and it goes to its owner's graveyard.
#[test]
fn req_sba_001_phasing_704_5f_ignored_while_phased_out() {
    let mut s = Sim::new();
    let a = s.creature(P2, "test.vanilla_0_0", 0, 0);
    phase(&mut s, a, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, a);
    let mut s = r.after;
    phase(&mut s, a, false);
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, vec![finding("704.5f", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5g, 702.26b, 702.26d: a phased-out 2-toughness creature with 2 damage
// marked has no finding and stays; the damage stays marked, and phased in it is destroyed.
#[test]
fn req_sba_001_phasing_704_5g_ignored_while_phased_out() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(a, 2);
    phase(&mut s, a, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, a);
    let mut s = r.after;
    phase(&mut s, a, false);
    assert_eq!(s.check(), vec![finding("704.5g", obj(a))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5g", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5h, 702.2b, 702.26b, 702.26d: a phased-out 3-toughness creature flagged
// as dealt deathtouch damage has no finding. The idle pass over it is an intervening completed
// check: it consumes the flag, so phased in (flag NOT re-set) 704.5h does not apply and a pass
// leaves the creature. A new deathtouch hit then makes 704.5h apply and it is destroyed.
#[test]
fn req_sba_001_phasing_704_5h_ignored_while_phased_out() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.damage(a, 1);
    s.env.deathtouch.insert(a);
    phase(&mut s, a, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, a);
    assert!(
        r.after.env.deathtouch.is_empty(),
        "the completed check consumed the flag"
    );
    assert_eq!(r.after.env.clear_calls, s.env.clear_calls + 1);
    let mut s = r.after;

    // Phased in after the intervening check, without a new hit: no 704.5h.
    phase(&mut s, a, false);
    no_finding_for(&mut s, a);
    assert_eq!(s.check(), vec![]);
    let r = run_sba(s);
    assert!(!r.outcome().performed, "nothing qualifies");
    assert!(r.outcome().findings.is_empty());
    assert!(r.after.on_battlefield(a), "{a:?} stayed on the battlefield");
    assert!(r.moved_to(a).is_none());
    let mut s = r.after;

    // A new deathtouch hit (one more damage, flag set again): 704.5h applies.
    s.damage(a, 1);
    s.env.deathtouch.insert(a);
    assert_eq!(s.check(), vec![finding("704.5h", obj(a))]);
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_eq!(r.outcome().findings, vec![finding("704.5h", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5h, 702.26b, 702.26d: phasing alone (no intervening completed check, only
// the pure `check`, which does not clear the deathtouch flag): the same phased-out creature is
// found by 704.5h once phased in without re-setting the flag, and a pass destroys it.
#[test]
fn req_sba_001_phasing_704_5h_check_keeps_flag() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_3_3", 3, 3);
    s.damage(a, 1);
    s.env.deathtouch.insert(a);
    phase(&mut s, a, true);
    no_finding_for(&mut s, a);
    assert!(s.env.deathtouch.contains(&a), "check keeps the flag");
    phase(&mut s, a, false);
    assert_eq!(s.check(), vec![finding("704.5h", obj(a))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5h", obj(a))]);
    assert_died(&r, a);
}

// REQ-SBA-001, CR 704.5i, 702.26b, 702.26d: a phased-out planeswalker with no loyalty counters
// has no finding and stays; phased in, 704.5i moves it to the graveyard.
#[test]
fn req_sba_001_phasing_704_5i_ignored_while_phased_out() {
    let mut s = Sim::new();
    let w = s.walker(P1, 0);
    phase(&mut s, w, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, w);
    let mut s = r.after;
    phase(&mut s, w, false);
    assert_eq!(s.check(), vec![finding("704.5i", obj(w))]);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5i", obj(w))]);
    assert_died(&r, w);
}

// REQ-SBA-001, CR 702.26d, 704.5d: a phased-out token continues to exist on the battlefield (no
// 704.5d finding, not ceased, still in its zone); a token in a graveyard or exile still ceases;
// the same token phased back in and moved to a graveyard ceases.
#[test]
fn req_sba_001_phasing_token_survives_phasing() {
    let mut s = Sim::new();
    let t = s.token(P1, ZoneKind::Battlefield);
    phase(&mut s, t, true);
    let in_gy = s.token(P1, ZoneKind::Graveyard);
    let in_exile = s.token(P2, ZoneKind::Exile);
    let mut expected = vec![
        finding("704.5d", obj(in_gy)),
        finding("704.5d", obj(in_exile)),
    ];
    expected.sort_by_key(|f| f.subject);
    assert_eq!(s.check(), expected);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, expected);
    assert!(!r.outcome().ceased.contains(&t));
    assert!(r.outcome().ceased.contains(&in_gy));
    assert!(r.outcome().ceased.contains(&in_exile));
    assert!(r.after.zones.object(t).is_some(), "the token still exists");
    assert_stayed_phased_out(&r, t);

    let outcome = r.outcome().clone();
    let mut s = r.after;
    s.sync_after(&outcome);
    phase(&mut s, t, false);
    let moved = s.move_to(t, ZoneKind::Graveyard);
    assert_eq!(s.check(), vec![finding("704.5d", obj(moved))]);
    let r = run_sba(s);
    assert!(r.outcome().ceased.contains(&moved));
    assert!(r.after.zones.object(moved).is_none());
}

// REQ-SBA-001, CR 702.26b, 702.26d, 704.5f: a phased-out token with a 0-toughness body on the
// battlefield neither dies (704.5f ignored) nor ceases; phased in, 704.5f applies.
#[test]
fn req_sba_001_phasing_zero_toughness_token_does_not_die() {
    let mut s = Sim::new();
    let t = s.token(P1, ZoneKind::Battlefield);
    s.add_layer_effect(1, modify_pt_effect(&[t], -1, -1, 2));
    phase(&mut s, t, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, t);
    assert!(r.after.zones.object(t).is_some());
    let mut s = r.after;
    phase(&mut s, t, false);
    assert_eq!(s.check(), vec![finding("704.5f", obj(t))]);
}

/// Item 6 for one creation order of the two creatures.
fn mixed_board(phased_first: bool) {
    let mut s = Sim::new();
    let (out, inn) = if phased_first {
        let o = s.creature(P1, "test.vanilla_0_0_a", 0, 0);
        let i = s.creature(P2, "test.vanilla_0_0_b", 0, 0);
        (o, i)
    } else {
        let i = s.creature(P2, "test.vanilla_0_0_b", 0, 0);
        let o = s.creature(P1, "test.vanilla_0_0_a", 0, 0);
        (o, i)
    };
    phase(&mut s, out, true);
    s.set_life(P2, 0);
    no_finding_for(&mut s, out);
    let r = run_sba(s);
    let o = r.outcome();
    assert!(o.performed);
    assert!(
        o.findings.contains(&finding("704.5f", obj(inn))),
        "{:?}",
        o.findings
    );
    assert!(o.findings.contains(&finding("704.5a", player(P2))));
    assert_eq!(o.findings.len(), 2, "{:?}", o.findings);
    assert!(o.lost_players.contains(&P2));
    assert_eq!(o.lost_players.len(), 1);
    assert!(r.after.lost(P2));
    assert_eq!(r.nonempty_batches().len(), 1, "one batch");
    let moves = r.moves();
    assert_eq!(moves.len(), 1, "exactly the phased-in creature moves");
    assert_died(&r, inn);
    assert_stayed_phased_out(&r, out);
}

// REQ-SBA-001, CR 702.26b, 704.5f, 704.5a, 704.3: a mixed board in one pass, phased-out
// creature created first: only the phased-in 0-toughness creature moves, in one batch, the
// 0-life player loses, nothing is found for the phased-out creature.
#[test]
fn req_sba_001_phasing_mixed_board_phased_first() {
    mixed_board(true);
}

// REQ-SBA-001, CR 702.26b, 704.5f, 704.5a, 704.3: the same mixed board with the phased-in
// creature created first (the other ObjectId order).
#[test]
fn req_sba_001_phasing_mixed_board_phased_second() {
    mixed_board(false);
}

// REQ-SBA-001, CR 702.26b: `check` over a board with a phased-out creature is pure (no
// component and no state hash changes).
#[test]
fn req_sba_001_phasing_check_is_pure() {
    let mut s = Sim::new();
    let a = s.creature(P1, "test.vanilla_2_2", 2, 2);
    s.damage(a, 2);
    s.env.deathtouch.insert(a);
    let b = s.creature(P2, "test.vanilla_0_0", 0, 0);
    phase(&mut s, a, true);
    phase(&mut s, b, true);
    let snap = s.snap();
    let hash = s.state_hash();
    let game_hash = s.game.state_hash();
    assert_eq!(s.check(), vec![]);
    assert_eq!(s.check(), vec![]);
    assert_eq!(s.snap(), snap);
    assert_eq!(s.state_hash(), hash);
    assert_eq!(s.game.state_hash(), game_hash);
}

// REQ-SBA-001, CR 702.26b, 704.5f, 702.12b: a phased-out indestructible 0-toughness creature
// is left alone; phased in, 704.5f moves it (indestructible does not stop 704.5f).
#[test]
fn req_sba_001_phasing_indestructible_zero_toughness() {
    let mut s = Sim::new();
    let mut c = creature_chars("test.indestructible_1_1", 1, 1);
    c.abilities.push(keyword_counter_ability("indestructible"));
    let a = s.create(ZoneKind::Battlefield, P1, c);
    s.add_layer_effect(1, modify_pt_effect(&[a], -1, -1, 2));
    phase(&mut s, a, true);
    assert_eq!(s.check(), vec![]);
    let r = assert_idle_pass(&s, a);
    let mut s = r.after;
    phase(&mut s, a, false);
    assert_eq!(s.check(), vec![finding("704.5f", obj(a))]);
    let r = run_sba(s);
    assert_died(&r, a);
}
