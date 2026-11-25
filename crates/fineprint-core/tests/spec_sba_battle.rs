// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the battle state-based actions (REQ-SBA-001, REQ-TEST-001, REQ-CORE-001,
//! REQ-CORE-006; CR 704.5v, 704.5w, 704.5x, 704.5y, 310.4c, 310.7, 310.8, 310.9a, 310.11,
//! 310.12a, 205.3q, 506.4c, 702.26b, 101.4). Black-box: written from the work-item brief only.
//! Every card is synthetic (R5).
//!
//! CR 704.5v: "If a Siege battle has defense 0 and it isn't the source of an ability that has
//! triggered but not yet left the stack, it's put into its owner's graveyard."
//! CR 704.5w: "If a non-Siege battle has defense 0, it's put into its owner's graveyard."
//! CR 704.5x: "If a battle has no player in the game designated as its protector and no
//! attacking creatures are currently attacking that battle, that battle's controller chooses an
//! appropriate player to be its protector based on its battle type. If the battle has no battle
//! type, its controller must be chosen. If no player can be chosen this way, the battle is put
//! into its owner's graveyard."
//! CR 704.5y: "If a battle's protector is a player who can't be its protector, that battle's
//! controller chooses an appropriate player to become its protector. If no player can be chosen
//! this way, the battle is put into its owner's graveyard."

#[path = "spec_sba_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::ChoiceSession;
use fineprint_core::events::ZoneChangeCause;
use fineprint_core::sba::{SbaEnvironment, StoredSbaEnvironment};
use fineprint_core::state::DesignationValue;
use support::*;

const SBA: ZoneChangeCause = ZoneChangeCause::StateBasedAction;

/// `check` on the state after `r`, with the layer environment re-synced to the pass's moves.
fn check_after(r: &SbaRun) -> Vec<fineprint_core::sba::SbaFinding> {
    let mut after = r.after.clone();
    after.sync_after(r.outcome());
    after.check()
}

/// Asserts the pass changed nothing about battle protectors.
fn assert_no_protector_change(r: &SbaRun) {
    assert!(r.outcome().protectors_chosen.is_empty());
    assert_eq!(
        r.after.env.battle_protectors, r.before.env.battle_protectors,
        "protectors unchanged"
    );
    assert!(r.after.env.protector_calls.is_empty());
}

// =============================================================================================
// CR 704.5v
// =============================================================================================

// REQ-SBA-001, CR 704.5v, 310.4c, 310.7: a Siege with defense 0 (no defense counter at all, or
// an explicit count of 0) that is not the source of a pending ability goes to its OWNER's
// graveyard (the owner differs from the controller), in one event.
#[test]
fn req_sba_001_704_5v_siege_zero_defense_to_owners_graveyard() {
    for explicit_zero in [false, true] {
        let mut s = Sim::new();
        let siege = if explicit_zero {
            s.siege(P2, P1, 0, Some(P2))
        } else {
            let id = s.permanent(P2, P1, siege_chars("test.siege_a"));
            s.env.battle_protectors.insert(id, P2);
            id
        };
        assert_eq!(s.check(), vec![finding("704.5v", obj(siege))]);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, vec![finding("704.5v", obj(siege))]);
        assert!(r.outcome().performed);
        assert_to_owners_graveyard(&r, siege, SBA);
        assert_eq!(r.nonempty_batches().len(), 1, "one event (CR 704.3)");
        assert_no_protector_change(&r);
        assert!(check_after(&r).is_empty());
    }
}

// REQ-SBA-001, CR 704.5v, 310.7: a Siege at defense 0 that is the source of an ability that
// has triggered but not yet left the stack survives and is not found; once that ability has
// left the stack (the flag cleared) it is found and goes.
#[test]
fn req_sba_001_704_5v_pending_ability_keeps_siege() {
    let mut s = Sim::new();
    let siege = s.siege(P1, P1, 0, Some(P2));
    s.env.battle_pending.insert(siege);
    assert!(s.check().is_empty());
    let r = run_sba(s.clone());
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, siege);

    s.env.battle_pending.remove(&siege);
    assert_eq!(s.check(), vec![finding("704.5v", obj(siege))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, siege, SBA);
}

// REQ-SBA-001, CR 704.5v, 310.4c: a Siege with one or more defense counters is untouched.
#[test]
fn req_sba_001_704_5v_positive_defense_untouched() {
    for defense in [1, 2, 7] {
        let mut s = Sim::new();
        let siege = s.siege(P1, P1, defense, Some(P2));
        assert!(s.check().is_empty(), "defense {defense}");
        let r = run_sba(s);
        assert!(r.outcome().findings.is_empty());
        assert!(!r.outcome().performed);
        assert_still_on_battlefield(&r, siege);
    }
}

// REQ-SBA-001, CR 704.5v, 310.4c: a negative defense count counts as defense 0.
#[test]
fn req_sba_001_704_5v_negative_defense_counts_as_zero() {
    let mut s = Sim::new();
    let siege = s.siege(P2, P1, 0, Some(P2));
    s.set_defense(siege, -2);
    assert_eq!(s.check(), vec![finding("704.5v", obj(siege))]);
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, siege, SBA);
}

// REQ-SBA-001, CR 704.5v, 704.5w, 205.3q: a non-Siege battle at defense 0 is not matched by
// 704.5v (it is a 704.5w subject); a Siege at defense 0 is not matched by 704.5w.
#[test]
fn req_sba_001_704_5v_non_siege_not_matched() {
    let mut s = Sim::new();
    let plain = s.battle(P1, P1, 0, Some(P1));
    let siege = s.siege(P1, P1, 0, Some(P2));
    assert_eq!(
        s.check(),
        vec![finding("704.5v", obj(siege)), finding("704.5w", obj(plain))]
    );
    let r = run_sba(s);
    assert_to_owners_graveyard(&r, siege, SBA);
    assert_to_owners_graveyard(&r, plain, SBA);
    assert_eq!(
        r.nonempty_batches().len(),
        1,
        "both in one event (CR 704.3)"
    );
}

// REQ-SBA-001, CR 704.5v, 702.26b: a phased-out Siege at defense 0 is treated as though it does
// not exist: not found, not moved.
#[test]
fn req_sba_001_704_5v_phased_out_siege_ignored() {
    let mut s = Sim::new();
    let siege = s.siege(P1, P1, 0, Some(P2));
    s.phase_out(siege);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert!(r.after.on_battlefield(siege));
    assert!(r.moved_to(siege).is_none());
}

// =============================================================================================
// CR 704.5w
// =============================================================================================

// REQ-SBA-001, CR 704.5w, 310.8: a non-Siege battle at defense 0 goes to its OWNER's graveyard
// even when it is the source of a pending triggered ability (704.5w has no such exception); a
// battle with another, non-Siege subtype is non-Siege too.
#[test]
fn req_sba_001_704_5w_non_siege_zero_defense_even_with_pending() {
    for other_subtype in [false, true] {
        let mut s = Sim::new();
        let battle = if other_subtype {
            let id = s.permanent(
                P2,
                P1,
                typed(&["test.battle_typed"], &[], &["Battle"], &["TestType"]),
            );
            s.set_defense(id, 0);
            s.env.battle_protectors.insert(id, P1);
            id
        } else {
            s.battle(P2, P1, 0, Some(P1))
        };
        s.env.battle_pending.insert(battle);
        assert_eq!(s.check(), vec![finding("704.5w", obj(battle))]);
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, vec![finding("704.5w", obj(battle))]);
        assert!(r.outcome().performed);
        assert_to_owners_graveyard(&r, battle, SBA);
        assert_no_protector_change(&r);
    }
}

// REQ-SBA-001, CR 704.5w, 704.5v: a Siege at defense 0 is matched by 704.5v only, never by
// 704.5w, with or without a pending ability.
#[test]
fn req_sba_001_704_5w_siege_not_matched() {
    for pending in [false, true] {
        let mut s = Sim::new();
        let siege = s.siege(P1, P1, 0, Some(P2));
        if pending {
            s.env.battle_pending.insert(siege);
        }
        let ids = s.check_ids();
        assert!(!ids.contains(&"704.5w"), "pending {pending}: {ids:?}");
        if pending {
            assert!(ids.is_empty());
        } else {
            assert_eq!(ids, vec!["704.5v"]);
        }
    }
}

// REQ-SBA-001, CR 704.5w, 310.4c: a non-Siege battle with defense 1 is untouched.
#[test]
fn req_sba_001_704_5w_positive_defense_untouched() {
    let mut s = Sim::new();
    let battle = s.battle(P2, P1, 1, Some(P1));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert_still_on_battlefield(&r, battle);
}

// REQ-SBA-001, CR 704.5w, 702.26b: a phased-out non-Siege battle at defense 0 is ignored.
#[test]
fn req_sba_001_704_5w_phased_out_ignored() {
    let mut s = Sim::new();
    let battle = s.battle(P1, P1, 0, Some(P1));
    s.phase_out(battle);
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(!r.outcome().performed);
    assert!(r.after.on_battlefield(battle));
    assert!(r.moved_to(battle).is_none());
}

// =============================================================================================
// CR 704.5x
// =============================================================================================

// REQ-SBA-001, CR 704.5x, 310.9a, 310.11: a battle with no battle type and no protector, not
// being attacked: one CHOOSE_PLAYER request to its controller with only the controller as a
// candidate; the answer becomes its protector (`protectors_chosen`, the environment), the pass
// is performed, nothing moves, and the next check finds nothing.
#[test]
fn req_sba_001_704_5x_typeless_battle_controller_chosen() {
    let mut s = Sim::new();
    // Owned by P2, controlled by P1: the controller chooses and is the only candidate.
    let battle = s.battle(P2, P1, 3, None);
    assert_eq!(s.check(), vec![finding("704.5x", obj(battle))]);
    let r = drive_answers(s, &[submit_player(P1)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P1]);
    let o = r.outcome();
    assert_eq!(o.findings, vec![finding("704.5x", obj(battle))]);
    assert_eq!(o.protectors_chosen, vec![(battle, P1)]);
    assert!(o.performed);
    assert_eq!(r.after.env.battle_protectors.get(&battle), Some(&P1));
    assert_eq!(r.after.env.protector_calls, vec![(battle, P1)]);
    assert_still_on_battlefield(&r, battle);
    assert!(r.moves().is_empty());
    assert!(check_after(&r).is_empty(), "the finding is gone");
}

// REQ-SBA-001, CR 704.5x, 310.12a: a Siege's candidates are exactly the opponents of its
// controller (three players); each candidate is an accepted answer and becomes the protector.
#[test]
fn req_sba_001_704_5x_siege_candidates_are_opponents() {
    for (controller, opponents) in [(P1, [P2, P3]), (P2, [P1, P3]), (P3, [P1, P2])] {
        for pick in opponents {
            let mut s = Sim::with_players(3);
            let siege = s.siege(P1, controller, 4, None);
            let r = drive_answers(s, &[submit_player(pick)]);
            assert_protector_request(&r.requests[0], controller, "CR 704.5x", &opponents);
            assert_eq!(r.outcome().protectors_chosen, vec![(siege, pick)]);
            assert_eq!(r.after.env.battle_protectors.get(&siege), Some(&pick));
            assert!(r.outcome().performed);
            assert_still_on_battlefield(&r, siege);
            assert!(check_after(&r).is_empty());
        }
    }
}

// REQ-SBA-001, CR 704.5x, 310.12a: a player who can't be the protector, or who lost the game,
// is not a candidate.
#[test]
fn req_sba_001_704_5x_siege_candidates_exclude_illegal_and_lost() {
    // can't be protector
    let mut s = Sim::with_players(3);
    let siege = s.siege(P1, P1, 4, None);
    s.env.illegal_protectors.insert((siege, P3));
    let r = drive_answers(s, &[submit_player(P2)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P2]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P2)]);

    // lost the game
    let mut s = Sim::with_players(3);
    let siege = s.siege(P1, P1, 4, None);
    s.mark_lost(P2);
    let r = drive_answers(s, &[submit_player(P3)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P3]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P3)]);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5x, 704.3: while the protector request is pending, the
// pass has changed nothing: the session's state is byte-identical (`canonical()`) to the
// input, no protector has been set, and a toughness-0 creature of the same pass is still on
// the battlefield.
#[test]
fn req_sba_001_704_5x_pending_leaves_state_byte_identical() {
    let mut s = Sim::new();
    let battle = s.siege(P1, P1, 2, None);
    let zero = s.creature(P1, "test.vanilla_2_0", 2, 0);
    let before = s.canonical();
    let session = ChoiceSession::start(s, call_sba).expect("session starts");
    let req = session.pending().cloned().expect("a protector request");
    assert_protector_request(&req, P1, "CR 704.5x", &[P2]);
    assert_eq!(session.base().canonical(), before, "byte-identical");
    assert!(session.base().env.battle_protectors.is_empty());
    assert!(session.base().env.protector_calls.is_empty());
    assert!(session.base().on_battlefield(zero));
    assert!(session.base().on_battlefield(battle));
}

// REQ-SBA-001, CR 704.5x, 310.12a: an invalid answer (a player who is not a candidate, a
// player not in the game, a response of the wrong type) is rejected: the request stays
// pending, nothing is recorded and the state is unchanged.
#[test]
fn req_sba_001_704_5x_invalid_answers_rejected() {
    let base = || {
        let mut s = Sim::with_players(3);
        let siege = s.siege(P1, P2, 2, None);
        (s, siege)
    };
    for bad in [
        submit_player(P2),
        submit_player(PlayerId::new(9)),
        keep(base().1),
        select(0),
    ] {
        let (s, _) = base();
        assert_answer_rejected(s, &[], bad);
    }
    // After the rejected answers, a valid one completes the pass.
    let (s, siege) = base();
    let r = drive_answers(s, &[submit_player(P3)]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P3)]);
}

// REQ-SBA-001, CR 704.5x, 506.3: a battle being attacked by an attacking creature gets no
// protector request and is not found, whichever player's creature attacks it.
#[test]
fn req_sba_001_704_5x_attacked_battle_exempt() {
    for attacker_owner in [P1, P2] {
        let mut s = Sim::new();
        let siege = s.siege(P1, P1, 3, None);
        let plain = s.battle(P2, P2, 3, None);
        let a = s.creature(attacker_owner, "test.vanilla_2_2", 2, 2);
        let b = s.creature(attacker_owner, "test.vanilla_2_2", 2, 2);
        s.attack_battle(a, siege);
        s.attack_battle(b, plain);
        assert!(s.check().is_empty(), "attacker of {attacker_owner:?}");
        let r = run_sba(s);
        assert!(r.outcome().findings.is_empty());
        assert!(!r.outcome().performed);
        assert_no_protector_change(&r);
        assert_still_on_battlefield(&r, siege);
        assert_still_on_battlefield(&r, plain);
    }
}

// REQ-SBA-001, CR 704.5x, 506.4c: an attacking creature attacking nothing (its battle was
// removed from combat) does not exempt a battle; nor does an attacker attacking another
// battle or a player.
#[test]
fn req_sba_001_704_5x_attacking_nothing_does_not_exempt() {
    let mut s = Sim::new();
    let siege = s.siege(P1, P1, 3, None);
    let other = s.battle(P2, P2, 3, Some(P2));
    let a = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let b = s.creature(P2, "test.vanilla_2_2", 2, 2);
    let c = s.creature(P2, "test.vanilla_2_2", 2, 2);
    s.attack_nothing(a);
    s.attack_battle(b, other);
    s.permanents
        .set_attack(
            c,
            fineprint_core::permanent::AttackState::Attacking(Some(
                fineprint_core::permanent::AttackTarget::Player(P1),
            )),
        )
        .expect("a permanent");
    assert_eq!(s.check(), vec![finding("704.5x", obj(siege))]);
    let r = drive_answers(s, &[submit_player(P2)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P2]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P2)]);
}

// REQ-SBA-001, CR 704.5x: a protector who has lost the game, or a player id that is not in the
// game, is no protector "in the game": 704.5x applies (not 704.5y).
#[test]
fn req_sba_001_704_5x_protector_not_in_game() {
    // lost the game
    let mut s = Sim::with_players(3);
    let siege = s.siege(P1, P1, 3, Some(P2));
    s.mark_lost(P2);
    assert_eq!(s.check(), vec![finding("704.5x", obj(siege))]);
    let r = drive_answers(s, &[submit_player(P3)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P3]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P3)]);
    assert_eq!(r.after.env.battle_protectors.get(&siege), Some(&P3));
    assert!(check_after(&r).is_empty());

    // not a player of this game at all
    let mut s = Sim::new();
    let plain = s.battle(P1, P1, 3, Some(PlayerId::new(9)));
    assert_eq!(s.check(), vec![finding("704.5x", obj(plain))]);
    let r = drive_answers(s, &[submit_player(P1)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P1]);
}

// REQ-SBA-001, CR 704.5x, 310.11: when no player can be chosen, no request is made and the
// battle goes to its OWNER's graveyard (the owner differs from the controller).
#[test]
fn req_sba_001_704_5x_no_candidates_to_owners_graveyard() {
    // A typeless battle whose controller can't be its protector.
    let mut s = Sim::new();
    let plain = s.battle(P2, P1, 3, None);
    s.env.illegal_protectors.insert((plain, P1));
    // A Siege whose only opponent can't be its protector.
    let siege = s.siege(P2, P1, 3, None);
    s.env.illegal_protectors.insert((siege, P2));
    assert_eq!(
        s.check(),
        vec![finding("704.5x", obj(plain)), finding("704.5x", obj(siege))]
    );
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_to_owners_graveyard(&r, plain, SBA);
    assert_to_owners_graveyard(&r, siege, SBA);
    assert_eq!(r.nonempty_batches().len(), 1, "one event");
    assert_no_protector_change(&r);

    // A Siege in a three-player game whose opponents both lost the game.
    let mut s = Sim::with_players(3);
    let siege = s.siege(P3, P1, 3, None);
    s.mark_lost(P2);
    s.mark_lost(P3);
    let r = run_sba(s);
    assert_eq!(r.outcome().findings, vec![finding("704.5x", obj(siege))]);
    assert_to_owners_graveyard(&r, siege, SBA);
}

// REQ-SBA-001, CR 704.5x, 101.4: battles of different controllers are asked in APNAP order
// (the active player first, then turn order), whatever the id order; every request is asked
// before anything changes (`call_sba` asserts the state is unchanged at each pending request),
// and a toughness-0 creature of the same pass moves only once all are answered.
#[test]
fn req_sba_001_704_5x_apnap_order_all_asked_first() {
    for (active, expected) in [(P1, [P1, P2, P3]), (P2, [P2, P3, P1]), (P3, [P3, P1, P2])] {
        for creation in [[P3, P1, P2], [P1, P2, P3], [P2, P3, P1]] {
            let mut s = Sim::with_players(3);
            s.set_turn(&[P1, P2, P3], active);
            let mut battles: BTreeMap<PlayerId, ObjectId> = BTreeMap::new();
            for c in creation {
                battles.insert(c, s.battle(c, c, 2, None));
            }
            let zero = s.creature(P1, "test.vanilla_2_0", 2, 0);
            let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
            let choosers: Vec<PlayerId> = r.requests.iter().map(|q| q.chooser).collect();
            assert_eq!(
                choosers,
                expected.to_vec(),
                "active {active:?}, creation {creation:?}"
            );
            for (i, req) in r.requests.iter().enumerate() {
                assert_protector_request(req, expected[i], "CR 704.5x", &[expected[i]]);
            }
            let mut chosen: Vec<(ObjectId, PlayerId)> =
                battles.iter().map(|(p, b)| (*b, *p)).collect();
            chosen.sort();
            assert_eq!(r.outcome().protectors_chosen, chosen, "ascending by battle");
            assert_eq!(r.after.env.protector_calls, chosen, "once each, ascending");
            assert_to_owners_graveyard(&r, zero, SBA);
            assert_eq!(r.nonempty_batches().len(), 1);
        }
    }
}

// REQ-SBA-001, CR 704.5x: two battles of one controller are asked in ascending battle id order,
// whichever candidate sets they have.
#[test]
fn req_sba_001_704_5x_same_controller_ascending_id() {
    for swap in [false, true] {
        let mut s = Sim::with_players(3);
        let first = s.siege(P1, P1, 2, None);
        let second = s.siege(P1, P1, 2, None);
        assert!(first < second);
        // One battle can only be protected by P2, the other only by P3.
        let (only_p2, only_p3) = if swap {
            (second, first)
        } else {
            (first, second)
        };
        s.env.illegal_protectors.insert((only_p2, P3));
        s.env.illegal_protectors.insert((only_p3, P2));
        let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
        assert_eq!(r.requests.len(), 2);
        let first_cands = if swap { [P3] } else { [P2] };
        let second_cands = if swap { [P2] } else { [P3] };
        assert_protector_request(&r.requests[0], P1, "CR 704.5x", &first_cands);
        assert_protector_request(&r.requests[1], P1, "CR 704.5x", &second_cands);
        assert_eq!(
            r.outcome().protectors_chosen,
            vec![(first, first_cands[0]), (second, second_cands[0])]
        );
    }
}

// =============================================================================================
// CR 704.5y
// =============================================================================================

// REQ-SBA-001, CR 704.5y, 310.11: a protector who can't be the battle's protector yields one
// request with cr "CR 704.5y" to the controller; the answer replaces the protector and the next
// check finds nothing (reselection removes the finding).
#[test]
fn req_sba_001_704_5y_illegal_protector_reselected() {
    let mut s = Sim::with_players(3);
    let siege = s.siege(P2, P1, 3, Some(P2));
    s.env.illegal_protectors.insert((siege, P2));
    assert_eq!(s.check(), vec![finding("704.5y", obj(siege))]);
    let r = drive_answers(s, &[submit_player(P3)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5y", &[P3]);
    let o = r.outcome();
    assert_eq!(o.findings, vec![finding("704.5y", obj(siege))]);
    assert_eq!(o.protectors_chosen, vec![(siege, P3)]);
    assert!(o.performed);
    assert_eq!(r.after.env.battle_protectors.get(&siege), Some(&P3));
    assert_still_on_battlefield(&r, siege);
    assert!(
        check_after(&r).is_empty(),
        "reselection removes the finding"
    );
    let again = run_sba(r.after.clone());
    assert!(!again.outcome().performed);
}

// REQ-SBA-001, CR 704.5y, 310.12a, 613.1b: a Siege whose protector became its controller
// (control changed by a layer 2 effect) yields 704.5y; the NEW controller chooses among its own
// opponents.
#[test]
fn req_sba_001_704_5y_siege_protector_became_controller() {
    let mut s = Sim::new();
    let siege = s.siege(P1, P1, 3, Some(P2));
    assert!(s.check().is_empty(), "legal before the control change");
    s.gain_control(10, siege, P2);
    assert_eq!(s.check(), vec![finding("704.5y", obj(siege))]);
    let r = drive_answers(s, &[submit_player(P1)]);
    assert_protector_request(&r.requests[0], P2, "CR 704.5y", &[P1]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P1)]);
    assert!(check_after(&r).is_empty());
}

// REQ-SBA-001, CR 704.5y, 310.9a: a typeless battle whose protector is not its controller
// yields 704.5y; the only candidate is the controller.
#[test]
fn req_sba_001_704_5y_typeless_protector_not_controller() {
    for (controller, protector) in [(P1, P2), (P2, P1)] {
        let mut s = Sim::new();
        let plain = s.battle(P1, controller, 3, Some(protector));
        assert_eq!(s.check(), vec![finding("704.5y", obj(plain))]);
        let r = drive_answers(s, &[submit_player(controller)]);
        assert_protector_request(&r.requests[0], controller, "CR 704.5y", &[controller]);
        assert_eq!(r.outcome().protectors_chosen, vec![(plain, controller)]);
        assert!(check_after(&r).is_empty());
    }
}

// REQ-SBA-001, CR 704.5y: a legal protector yields nothing (a Siege protected by an opponent, a
// typeless battle protected by its controller).
#[test]
fn req_sba_001_704_5y_legal_protector_nothing() {
    let mut s = Sim::with_players(3);
    let a = s.siege(P1, P1, 3, Some(P2));
    let b = s.siege(P2, P2, 3, Some(P3));
    let c = s.battle(P3, P3, 3, Some(P3));
    assert!(s.check().is_empty());
    let r = run_sba(s);
    assert!(r.outcome().findings.is_empty());
    assert!(!r.outcome().performed);
    assert_no_protector_change(&r);
    for id in [a, b, c] {
        assert_still_on_battlefield(&r, id);
    }
}

// REQ-SBA-001, CR 704.5y, 704.5x, 506.4 (gate 02-state-objects F4): CR 704.5y has no "being
// attacked" exemption. A battle being attacked whose protector can't be its protector IS asked
// (reason CR 704.5y) and its protector is replaced; a battle being attacked with NO protector
// is still not asked (CR 704.5x keeps its attack condition). The pass reports the change only
// through `protectors_chosen`: the attack itself is untouched (combat applies CR 506.4).
#[test]
fn req_sba_001_704_5y_attacked_battle_with_illegal_protector_is_asked() {
    for attacker_owner in [P1, P2] {
        let mut s = Sim::new();
        let siege = s.siege(P1, P1, 3, Some(P2));
        s.env.illegal_protectors.insert((siege, P2));
        let plain = s.battle(P1, P1, 3, Some(P2));
        let unprotected = s.siege(P1, P1, 3, None);
        let a = s.creature(attacker_owner, "test.vanilla_2_2", 2, 2);
        let b = s.creature(attacker_owner, "test.vanilla_2_2", 2, 2);
        let c = s.creature(attacker_owner, "test.vanilla_2_2", 2, 2);
        s.attack_battle(a, siege);
        s.attack_battle(b, plain);
        s.attack_battle(c, unprotected);
        // The Siege has no candidate left (P2 forbidden, P1 its controller): no request, it
        // goes to its owner's graveyard; the typeless battle is asked, offering only P1.
        assert_eq!(
            s.check(),
            vec![finding("704.5y", obj(siege)), finding("704.5y", obj(plain))],
            "attacker of {attacker_owner:?}"
        );
        let r = drive_answers(s, &[submit_player(P1)]);
        assert_protector_request(&r.requests[0], P1, "CR 704.5y", &[P1]);
        assert_eq!(r.requests[0].reason.source, Some(plain));
        let o = r.outcome();
        assert!(o.performed);
        assert_eq!(o.protectors_chosen, vec![(plain, P1)]);
        assert_eq!(r.after.env.battle_protectors.get(&plain), Some(&P1));
        assert_to_owners_graveyard(&r, siege, SBA);
        assert_still_on_battlefield(&r, plain);
        assert_still_on_battlefield(&r, unprotected);
        assert!(!o.protectors_chosen.iter().any(|(x, _)| *x == unprotected));
        assert_eq!(r.after.env.battle_protectors.get(&unprotected), None);
    }
}

// REQ-SBA-001, CR 704.5y, 310.11: when no player can be chosen, no request is made and the
// battle goes to its OWNER's graveyard.
#[test]
fn req_sba_001_704_5y_no_candidates_to_owners_graveyard() {
    let mut s = Sim::new();
    let plain = s.battle(P2, P1, 3, Some(P2));
    s.env.illegal_protectors.insert((plain, P1));
    let siege = s.siege(P2, P1, 3, Some(P2));
    s.env.illegal_protectors.insert((siege, P2));
    assert_eq!(
        s.check(),
        vec![finding("704.5y", obj(plain)), finding("704.5y", obj(siege))]
    );
    let r = run_sba(s);
    assert!(r.outcome().performed);
    assert_to_owners_graveyard(&r, plain, SBA);
    assert_to_owners_graveyard(&r, siege, SBA);
    assert_no_protector_change(&r);
}

// REQ-SBA-001, CR 704.5x, 704.5y: 704.5x and 704.5y are mutually exclusive and a battle never
// gets two requests in one pass: each battle has exactly one finding and one request.
#[test]
fn req_sba_001_704_5y_never_two_requests() {
    let mut s = Sim::with_players(3);
    // Protector is its own controller AND can't be protector: one y.
    let a = s.siege(P1, P1, 3, Some(P1));
    s.env.illegal_protectors.insert((a, P1));
    // Protector lost the game AND can't be protector: one x.
    let b = s.siege(P1, P1, 3, Some(P3));
    s.env.illegal_protectors.insert((b, P3));
    s.mark_lost(P3);
    // Typeless, protector is an opponent AND can't be protector: one y.
    let c = s.battle(P1, P1, 3, Some(P2));
    s.env.illegal_protectors.insert((c, P2));
    let found = s.check();
    assert_eq!(
        found,
        vec![
            finding("704.5x", obj(b)),
            finding("704.5y", obj(a)),
            finding("704.5y", obj(c)),
        ]
    );
    let r = drive_sba(s, |_, req| req.default_on_timeout.clone());
    assert_eq!(r.requests.len(), 3, "one request per battle");
    assert_protector_request(&r.requests[0], P1, "CR 704.5y", &[P2]);
    assert_protector_request(&r.requests[1], P1, "CR 704.5x", &[P2]);
    assert_protector_request(&r.requests[2], P1, "CR 704.5y", &[P1]);
    assert_eq!(
        r.outcome().protectors_chosen,
        vec![(a, P2), (b, P2), (c, P1)]
    );
    assert!(check_after(&r).is_empty());
}

// =============================================================================================
// interaction of 704.5v/w with 704.5x/y
// =============================================================================================

// REQ-SBA-001, CR 704.5v, 704.5w, 704.5x, 704.5y: a battle at defense 0 that 704.5v or 704.5w
// puts into the graveyard gets NO protector request: its 704.5x or 704.5y finding is still
// reported, but it just moves and `protectors_chosen` has no entry for it. Both id orders.
#[test]
fn req_sba_001_704_5x_zero_defense_battle_moves_without_request() {
    for reverse in [false, true] {
        let mut s = Sim::new();
        let mut made: Vec<(&str, ObjectId)> = Vec::new();
        let kinds = if reverse {
            ["plain_y", "plain_x", "siege_y", "siege_x"]
        } else {
            ["siege_x", "siege_y", "plain_x", "plain_y"]
        };
        for k in kinds {
            let id = match k {
                "siege_x" => s.siege(P2, P1, 0, None),
                "siege_y" => {
                    let id = s.siege(P2, P1, 0, Some(P2));
                    s.env.illegal_protectors.insert((id, P2));
                    id
                }
                "plain_x" => s.battle(P2, P1, 0, None),
                _ => s.battle(P2, P1, 0, Some(P2)),
            };
            made.push((k, id));
        }
        let id_of = |k: &str| made.iter().find(|(n, _)| *n == k).unwrap().1;
        let mut expected = vec![
            finding("704.5v", obj(id_of("siege_x"))),
            finding("704.5v", obj(id_of("siege_y"))),
            finding("704.5w", obj(id_of("plain_x"))),
            finding("704.5w", obj(id_of("plain_y"))),
            finding("704.5x", obj(id_of("siege_x"))),
            finding("704.5x", obj(id_of("plain_x"))),
            finding("704.5y", obj(id_of("siege_y"))),
            finding("704.5y", obj(id_of("plain_y"))),
        ];
        expected.sort_by_key(|f| (entry_index(f.entry_id), f.subject));
        assert_eq!(s.check(), expected, "reverse {reverse}");
        let r = run_sba(s);
        assert_eq!(r.outcome().findings, expected);
        for (_, id) in &made {
            assert_to_owners_graveyard(&r, *id, SBA);
        }
        assert_no_protector_change(&r);
        assert_eq!(r.nonempty_batches().len(), 1, "one event");
    }
}

// REQ-SBA-001, CR 704.5v, 704.5x: a Siege at defense 0 that is the source of a pending
// triggered ability survives 704.5v, so with no protector it still gets the 704.5x request.
#[test]
fn req_sba_001_704_5x_pending_zero_defense_siege_still_asked() {
    let mut s = Sim::new();
    let siege = s.siege(P2, P1, 0, None);
    s.env.battle_pending.insert(siege);
    assert_eq!(s.check(), vec![finding("704.5x", obj(siege))]);
    let r = drive_answers(s, &[submit_player(P2)]);
    assert_protector_request(&r.requests[0], P1, "CR 704.5x", &[P2]);
    assert_eq!(r.outcome().protectors_chosen, vec![(siege, P2)]);
    assert_still_on_battlefield(&r, siege);
}

// =============================================================================================
// the environment seams
// =============================================================================================

// REQ-SBA-001, CR 704.5v, 704.5x, 704.5y, 704.5z: the trait defaults: no protector, every
// player can be protector, no pending battle ability, attachment stamp 0, and
// `set_battle_protector` does nothing.
#[test]
fn req_sba_001_704_5x_environment_trait_defaults() {
    struct Plain;
    impl SbaEnvironment for Plain {
        fn deathtouch_damaged(&self) -> BTreeSet<ObjectId> {
            BTreeSet::new()
        }
        fn clear_deathtouch_damaged(&mut self) {}
    }
    let mut p = Plain;
    let b = ObjectId::new(7);
    assert_eq!(p.battle_protector(b), None);
    assert!(p.can_be_protector(b, P1));
    assert!(!p.battle_ability_pending(b));
    assert_eq!(p.attachment_timestamp(b), 0);
    p.set_battle_protector(b, P2);
    assert_eq!(p.battle_protector(b), None, "default set is a no-op");
}

// REQ-SBA-001, CR 704.5v, 704.5x, 704.5y, 704.5z: `StoredSbaEnvironment` starts empty, answers
// from its fields, and `set_battle_protector` writes `battle_protectors`.
#[test]
fn req_sba_001_704_5x_stored_environment_fields() {
    let mut e = StoredSbaEnvironment::default();
    assert!(e.battle_protectors.is_empty());
    assert!(e.illegal_protectors.is_empty());
    assert!(e.battle_pending.is_empty());
    assert!(e.attachment_since.is_empty());
    let b = ObjectId::new(3);
    let other = ObjectId::new(4);
    assert_eq!(e.battle_protector(b), None);
    e.set_battle_protector(b, P2);
    assert_eq!(e.battle_protectors.get(&b), Some(&P2));
    assert_eq!(e.battle_protector(b), Some(P2));
    e.set_battle_protector(b, P1);
    assert_eq!(e.battle_protector(b), Some(P1), "overwritten");
    e.illegal_protectors.insert((b, P1));
    assert!(!e.can_be_protector(b, P1));
    assert!(e.can_be_protector(b, P2));
    assert!(e.can_be_protector(other, P1));
    e.battle_pending.insert(b);
    assert!(e.battle_ability_pending(b));
    assert!(!e.battle_ability_pending(other));
    e.attachment_since.insert(b, 42);
    assert_eq!(e.attachment_timestamp(b), 42);
    assert_eq!(e.attachment_timestamp(other), 0);
}

// REQ-SBA-001, REQ-CORE-001, CR 704.5x: the same input gives the same requests, outcome and
// canonical bytes, twice.
#[test]
fn req_sba_001_704_5x_deterministic_replay() {
    let build = || {
        let mut s = Sim::with_players(3);
        s.siege(P1, P2, 2, None);
        s.battle(P3, P3, 2, Some(P1));
        s.siege(P2, P1, 0, None);
        s
    };
    let a = drive_sba(build(), |_, req| req.default_on_timeout.clone());
    let b = drive_sba(build(), |_, req| req.default_on_timeout.clone());
    assert_eq!(a.requests, b.requests);
    assert_eq!(a.result, b.result);
    assert_eq!(a.after.canonical(), b.after.canonical());
    assert_eq!(a.hash, b.hash);
}

// =============================================================================================
// simultaneity (CR 704.3, 704.7) and properties
// =============================================================================================

/// The FP-042 mixed board: 704.5a, 704.5f, 704.5j plus battle, Role and engine entries, built
/// in the component order `order`.
struct Mixed {
    sim: Sim,
    zero: ObjectId,
    legends: [ObjectId; 2],
    siege0: ObjectId,
    asked: ObjectId,
    battle_creature: ObjectId,
    old_role: ObjectId,
    new_role: ObjectId,
    engine: ObjectId,
}

fn mixed(order: &[usize]) -> Mixed {
    let mut s = Sim::new();
    s.set_life(P2, 0);
    let mut zero = None;
    let mut legends = None;
    let mut siege0 = None;
    let mut asked = None;
    let mut bc = None;
    let mut roles = None;
    let mut engine = None;
    for part in order {
        match part {
            0 => zero = Some(s.creature(P1, "test.vanilla_2_0", 2, 0)),
            1 => {
                legends = Some([
                    s.legend(P1, P1, "test.legend_a"),
                    s.legend(P2, P1, "test.legend_a"),
                ])
            }
            2 => siege0 = Some(s.siege(P2, P1, 0, Some(P2))),
            3 => asked = Some(s.battle(P2, P1, 2, None)),
            4 => {
                // A battle that is also a 0-toughness creature at defense 0: CR 704.5f and
                // 704.5w both put it into the graveyard; it moves once (CR 704.7).
                let mut c = typed(&["test.battle_creature"], &[], &["Battle", "Creature"], &[]);
                c.power = Some(lit(1));
                c.toughness = Some(lit(0));
                let id = s.permanent(P2, P1, c);
                s.set_defense(id, 0);
                s.env.battle_protectors.insert(id, P1);
                bc = Some(id);
            }
            5 => {
                let host = s.creature(P2, "test.vanilla_2_2", 2, 2);
                let a = s.role_aura(P2, P1, host, 4);
                let b = s.role_aura(P1, P1, host, 2);
                roles = Some((b, a));
            }
            _ => engine = Some(s.engine(P2, P1)),
        }
    }
    let (old_role, new_role) = roles.unwrap();
    Mixed {
        sim: s,
        zero: zero.unwrap(),
        legends: legends.unwrap(),
        siege0: siege0.unwrap(),
        asked: asked.unwrap(),
        battle_creature: bc.unwrap(),
        old_role,
        new_role,
        engine: engine.unwrap(),
    }
}

// REQ-SBA-001, REQ-TEST-001, CR 704.3, 704.7, 704.5a, 704.5f, 704.5j, 704.5v, 704.5w, 704.5x,
// 704.5z, 704.5aa: one pass performs every applicable SBA as ONE event: a player loss, the
// graveyard moves of 704.5f, 704.5j, 704.5v, 704.5w and 704.5z in one batch (a battle that is
// also a 0-toughness creature moves once), the protector choice and the speed; for every id
// order, including the reverse.
#[test]
fn req_sba_001_704_5v_to_704_5aa_one_pass_simultaneous() {
    let orders: [[usize; 7]; 3] = [
        [0, 1, 2, 3, 4, 5, 6],
        [6, 5, 4, 3, 2, 1, 0],
        [3, 5, 0, 6, 2, 4, 1],
    ];
    let mut shapes = Vec::new();
    for order in orders {
        let b = mixed(&order);
        let kept = b.legends[0];
        let mut expected = vec![
            finding("704.5a", player(P2)),
            finding("704.5f", obj(b.zero)),
            finding("704.5f", obj(b.battle_creature)),
            finding("704.5j", obj(b.legends[0])),
            finding("704.5j", obj(b.legends[1])),
            finding("704.5v", obj(b.siege0)),
            finding("704.5w", obj(b.battle_creature)),
            finding("704.5x", obj(b.asked)),
            finding("704.5z", obj(b.old_role)),
            finding("704.5aa", player(P1)),
        ];
        expected.sort_by_key(|f| (entry_index(f.entry_id), f.subject));
        let mut s = b.sim.clone();
        let before = s.canonical();
        assert_eq!(s.check(), expected, "{order:?}");
        assert_eq!(s.canonical(), before, "check is pure");
        let r = drive_sba(b.sim, |_, req| match &req.payload {
            fineprint_core::choice::ChoicePayload::ChoosePlayer { .. } => {
                req.default_on_timeout.clone()
            }
            _ => keep(kept),
        });
        assert_eq!(
            r.requests.len(),
            2,
            "{order:?}: the legend and the protector"
        );
        let protector_reqs: Vec<_> = r
            .requests
            .iter()
            .filter(|q| q.choice_type == fineprint_core::choice::ChoiceType::ChoosePlayer)
            .collect();
        assert_eq!(protector_reqs.len(), 1);
        assert_protector_request(protector_reqs[0], P1, "CR 704.5x", &[P1]);
        let o = r.outcome();
        assert!(o.performed);
        assert_eq!(o.findings, expected, "{order:?}");
        assert_eq!(o.lost_players, vec![P2]);
        assert_eq!(o.protectors_chosen, vec![(b.asked, P1)]);
        assert_eq!(o.speed_set, vec![P1]);
        for id in [
            b.zero,
            b.legends[1],
            b.siege0,
            b.battle_creature,
            b.old_role,
        ] {
            assert_to_owners_graveyard(&r, id, SBA);
        }
        for id in [kept, b.asked, b.new_role, b.engine] {
            assert_still_on_battlefield(&r, id);
        }
        assert_eq!(
            r.moves().len(),
            5,
            "{order:?}: five moves, the battle creature once"
        );
        assert_eq!(
            r.nonempty_batches().len(),
            1,
            "{order:?}: one event (CR 704.3)"
        );
        assert_eq!(r.after.speed(P1), Some(DesignationValue::Int(1)));
        assert_eq!(r.after.env.battle_protectors.get(&b.asked), Some(&P1));
        // Shape independent of ids: entry ids of findings and counts.
        shapes.push((
            o.findings.iter().map(|f| f.entry_id).collect::<Vec<_>>(),
            r.moves().len(),
            o.speed_set.clone(),
        ));
        // The same Sim twice: identical bytes and outcomes.
        let again = drive_sba(mixed(&order).sim, |_, req| match &req.payload {
            fineprint_core::choice::ChoicePayload::ChoosePlayer { .. } => {
                req.default_on_timeout.clone()
            }
            _ => keep(kept),
        });
        assert_eq!(again.result, r.result, "{order:?}");
        assert_eq!(again.requests, r.requests);
        assert_eq!(again.after.canonical(), r.after.canonical());
        assert_eq!(again.hash, r.hash);
    }
    assert!(
        shapes.windows(2).all(|w| w[0] == w[1]),
        "same shape: {shapes:?}"
    );
}

/// One random FP-042 layout and its oracle.
struct Layout {
    sim: Sim,
    expected: Vec<fineprint_core::sba::SbaFinding>,
    /// The protector requests expected, in order: (battle, chooser, cr, candidates).
    requests: Vec<(ObjectId, PlayerId, &'static str, Vec<PlayerId>)>,
    /// Every battle that must leave the battlefield.
    moving: BTreeSet<ObjectId>,
}

const PLAYERS: [PlayerId; 3] = [P1, P2, P3];

fn random_layout(seed: u64) -> Layout {
    let mut g = Lcg::new(seed.wrapping_mul(0x0042_0042) ^ 0x5157);
    let mut s = Sim::with_players(3);
    let lost3 = g.chance(1, 4);
    let alive: Vec<PlayerId> = if lost3 {
        vec![P1, P2]
    } else {
        PLAYERS.to_vec()
    };
    let active = g.pick(&alive);
    s.set_turn(&[P1, P2, P3], active);
    if lost3 {
        s.mark_lost(P3);
    }
    let in_game = |p: PlayerId| alive.contains(&p);
    let rank = |p: PlayerId| {
        let i = PLAYERS.iter().position(|x| *x == p).unwrap();
        let a = PLAYERS.iter().position(|x| *x == active).unwrap();
        (i + 3 - a) % 3
    };
    let mut expected = Vec::new();
    let mut requests = Vec::new();
    let mut moving = BTreeSet::new();

    // battles
    let n_battles = g.below(5);
    let mut battles = Vec::new();
    for _ in 0..n_battles {
        let siege = g.chance(1, 2);
        let owner = g.pick(&PLAYERS);
        let controller = g.pick(&alive);
        let defense = g.range(-1, 2);
        let protector = match g.below(5) {
            0 => None,
            1 => Some(PlayerId::new(9)),
            _ => Some(g.pick(&PLAYERS)),
        };
        let id = if siege {
            s.siege(owner, controller, defense, protector)
        } else {
            s.battle(owner, controller, defense, protector)
        };
        let pending = g.chance(1, 3);
        if pending {
            s.env.battle_pending.insert(id);
        }
        for p in PLAYERS {
            if g.chance(1, 4) {
                s.env.illegal_protectors.insert((id, p));
            }
        }
        let attacked = g.chance(1, 5);
        if attacked {
            let a = s.creature(g.pick(&alive), "test.vanilla_2_2", 2, 2);
            s.attack_battle(a, id);
        } else if g.chance(1, 5) {
            let a = s.creature(g.pick(&alive), "test.vanilla_2_2", 2, 2);
            s.attack_nothing(a);
        }
        battles.push((
            id,
            siege,
            controller,
            defense.max(0),
            protector,
            pending,
            attacked,
        ));
    }
    let mut asked: Vec<(usize, ObjectId, PlayerId, &'static str, Vec<PlayerId>)> = Vec::new();
    for &(id, siege, controller, defense, protector, pending, attacked) in &battles {
        let can_be = |p: PlayerId| !s.env.illegal_protectors.contains(&(id, p));
        let v = siege && defense == 0 && !pending;
        let w = !siege && defense == 0;
        if v {
            expected.push(finding("704.5v", obj(id)));
        }
        if w {
            expected.push(finding("704.5w", obj(id)));
        }
        let legal = |p: PlayerId| {
            can_be(p)
                && if siege {
                    p != controller
                } else {
                    p == controller
                }
        };
        // CR 704.5y applies whether or not the battle is being attacked; only CR 704.5x has
        // the attack condition (gate 02-state-objects F4).
        let rule = match protector {
            Some(p) if in_game(p) => (!legal(p)).then_some("704.5y"),
            _ => (!attacked).then_some("704.5x"),
        };
        if v || w {
            moving.insert(id);
        }
        if let Some(rule) = rule {
            expected.push(finding(rule, obj(id)));
            let candidates: Vec<PlayerId> = if siege {
                alive
                    .iter()
                    .copied()
                    .filter(|p| *p != controller && can_be(*p))
                    .collect()
            } else if can_be(controller) {
                vec![controller]
            } else {
                Vec::new()
            };
            if !(v || w) {
                if candidates.is_empty() {
                    moving.insert(id);
                } else {
                    let cr = if rule == "704.5x" {
                        "CR 704.5x"
                    } else {
                        "CR 704.5y"
                    };
                    asked.push((rank(controller), id, controller, cr, candidates));
                }
            }
        }
    }
    asked.sort_by_key(|a| (a.0, a.1));
    for (_, id, c, cr, cands) in asked {
        requests.push((id, c, cr, cands));
    }

    // Roles
    let hosts = [
        s.creature(P1, "test.vanilla_2_2", 2, 2),
        s.creature(P2, "test.vanilla_2_2", 2, 2),
    ];
    let mut groups: BTreeMap<(ObjectId, PlayerId), Vec<(u64, ObjectId)>> = BTreeMap::new();
    for _ in 0..g.below(5) {
        let host = g.pick(&hosts);
        let controller = g.pick(&alive);
        let stamp = g.below(3);
        let id = s.role_aura(g.pick(&PLAYERS), controller, host, stamp);
        if g.chance(1, 6) {
            s.phase_out(id);
        } else {
            groups
                .entry((host, controller))
                .or_default()
                .push((stamp, id));
        }
    }
    for members in groups.values() {
        let best = members.iter().max().copied().unwrap();
        for m in members {
            if *m != best {
                expected.push(finding("704.5z", obj(m.1)));
            }
        }
    }

    // engines and speed
    let mut speedy = BTreeSet::new();
    for p in PLAYERS {
        if g.chance(1, 4) {
            s.set_speed(p, g.range(1, 4));
        }
    }
    for _ in 0..g.below(4) {
        let controller = g.pick(&PLAYERS);
        let id = s.engine(g.pick(&PLAYERS), controller);
        if g.chance(1, 5) {
            s.phase_out(id);
        } else if in_game(controller) && s.speed(controller).is_none() {
            speedy.insert(controller);
        }
    }
    for p in speedy {
        expected.push(finding("704.5aa", player(p)));
    }
    expected.sort_by_key(|f| (entry_index(f.entry_id), f.subject));
    Layout {
        sim: s,
        expected,
        requests,
        moving,
    }
}

// REQ-SBA-001, REQ-TEST-001, REQ-CORE-001, REQ-CORE-006, CR 704.3, 704.5v, 704.5w, 704.5x,
// 704.5y, 704.5z, 704.5aa, 101.4: over seeded random battle, Role and engine layouts: `check`
// is pure and matches the brief's oracle exactly (registry order, then subject order); the
// pass asks exactly the expected protector requests (APNAP, then ascending battle id; never
// for a battle that moves); every moving battle goes to its owner's graveyard in one event;
// after the pass (each request answered with its default) `check` finds nothing for
// 704.5v-704.5aa; and the same layout twice gives identical bytes and outcomes.
#[test]
fn req_sba_001_704_5v_to_704_5aa_property_random_layouts() {
    const SEEDS: u64 = 160;
    let (mut asked, mut moved, mut roles, mut speeds) = (0, 0, 0, 0);
    for seed in 0..SEEDS {
        let l = random_layout(seed);
        let mut s = l.sim.clone();
        let before = s.canonical();
        let found = s.check();
        assert_eq!(s.canonical(), before, "seed {seed}: check is pure");
        assert_findings_ordered(&found);
        assert_eq!(found, l.expected, "seed {seed}: the oracle");

        let r = drive_sba(l.sim.clone(), |_, req| req.default_on_timeout.clone());
        let o = r.outcome();
        assert_eq!(o.findings, found, "seed {seed}");
        assert_eq!(o.performed, !found.is_empty(), "seed {seed}");
        assert_eq!(r.requests.len(), l.requests.len(), "seed {seed}: requests");
        for (req, (battle, chooser, cr, cands)) in r.requests.iter().zip(&l.requests) {
            assert_protector_request(req, *chooser, cr, cands);
            assert_still_on_battlefield(&r, *battle);
        }
        let mut chosen: Vec<(ObjectId, PlayerId)> =
            l.requests.iter().map(|(b, _, _, c)| (*b, c[0])).collect();
        chosen.sort();
        assert_eq!(o.protectors_chosen, chosen, "seed {seed}");
        for b in &l.moving {
            assert_to_owners_graveyard(&r, *b, SBA);
            assert!(!o.protectors_chosen.iter().any(|(x, _)| x == b));
        }
        if !r.moves().is_empty() {
            assert_eq!(r.nonempty_batches().len(), 1, "seed {seed}: one event");
        }
        let mut speed_expected: Vec<PlayerId> = found
            .iter()
            .filter(|f| f.entry_id == "704.5aa")
            .map(|f| match f.subject {
                fineprint_core::sba::SbaSubject::Player(p) => p,
                other => panic!("704.5aa subject {other:?}"),
            })
            .collect();
        speed_expected.sort();
        assert_eq!(o.speed_set, speed_expected, "seed {seed}");

        let mut after = r.after.clone();
        after.sync_after(o);
        assert!(
            fp042_findings(&after.check()).is_empty(),
            "seed {seed}: nothing left for 704.5v-704.5aa"
        );

        let again = drive_sba(l.sim, |_, req| req.default_on_timeout.clone());
        assert_eq!(again.result, r.result, "seed {seed}");
        assert_eq!(again.after.canonical(), r.after.canonical(), "seed {seed}");

        asked += l.requests.len();
        moved += l.moving.len();
        roles += found.iter().filter(|f| f.entry_id == "704.5z").count();
        speeds += o.speed_set.len();
    }
    assert!(asked > 10, "the generator asks protectors ({asked})");
    assert!(moved > 10, "the generator moves battles ({moved})");
    assert!(roles > 10, "the generator finds Roles ({roles})");
    assert!(speeds > 10, "the generator sets speed ({speeds})");
}
