// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the turn scheduler (REQ-TURN-003): extra turns (CR 500.7), turn skips
//! (CR 614.1b, 614.10, 614.10a, 614.10b, 800.4k), phase and step skips (CR 500.11, 614.10,
//! 614.10a) and extra phases and steps (CR 500.8, 500.9, 500.10, 500.10a, 505.1a). Black-box:
//! written from the work-item brief only. Seats are three players `[P1, P2, P3]` or two
//! `[P1, P2]`.

#[path = "spec_turn_control_support.rs"]
mod support;

use fineprint_core::events::{Phase, Step};
use fineprint_core::turn::Segment;
use fineprint_core::turn::control::{
    FollowUpId, PlanItem, PlannedTurn, SkippedTurn, TurnScheduler,
};
use support::*;

const S3: [fineprint_base::ids::PlayerId; 3] = [P1, P2, P3];
const S2: [fineprint_base::ids::PlayerId; 2] = [P1, P2];

/// A scheduler whose first (normal) turn, P1's, has been taken.
fn after_first_turn(seats: &[fineprint_base::ids::PlayerId]) -> TurnScheduler {
    let mut s = TurnScheduler::new(P1);
    let (t, _) = take(&mut s, seats);
    assert_eq!(t.player, P1);
    s
}

// ---------------------------------------------------------------------------------------------
// the normal order and plan
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003: the starting player takes the first normal turn, then seat order; the plan is
/// the normal plan (first main phase only is precombat, CR 505.1a).
#[test]
fn normal_turn_order_and_plan() {
    let mut s = TurnScheduler::new(P2);
    let t = next(&mut s, &S3);
    assert_eq!(
        t,
        PlannedTurn {
            player: P2,
            extra: false,
            skipped: vec![],
            follow_ups: vec![]
        }
    );
    assert_eq!(s.current_phase(), None);
    assert_eq!(drain(&mut s), normal_plan(false));
    assert_eq!(s.current_phase(), None);
    assert_eq!(order(&mut s, &S3, 5), vec![P3, P1, P2, P3, P1]);
}

/// REQ-TURN-003: the first-strike flag adds the first-strike damage step.
#[test]
fn normal_plan_with_first_strike_step() {
    let mut s = TurnScheduler::new(P1);
    s.next_turn(&S2, &everyone, true).expect("turn");
    let items = drain(&mut s);
    assert_eq!(items, normal_plan(true));
    assert!(items.contains(&st(Step::FirstStrikeDamage)));
}

/// REQ-TURN-003, CR 505.1a: the first main phase is the only `PrecombatMain`.
#[test]
fn only_first_main_phase_is_precombat() {
    let mut s = TurnScheduler::new(P1);
    let (_, items) = take(&mut s, &S2);
    let segs = segments(&items);
    assert_eq!(
        segs.iter()
            .filter(|x| **x == Segment::PrecombatMain)
            .count(),
        1
    );
    assert_eq!(
        segs.iter()
            .filter(|x| **x == Segment::PostcombatMain)
            .count(),
        1
    );
}

/// REQ-TURN-003: `current_phase` follows the last item yielded.
#[test]
fn current_phase_tracks_items() {
    let mut s = TurnScheduler::new(P1);
    assert_eq!(s.current_phase(), None);
    next(&mut s, &S2);
    assert_eq!(s.current_phase(), None);
    advance_to(&mut s, st(Step::Upkeep));
    assert_eq!(s.current_phase(), Some(Phase::Beginning));
    advance_to(&mut s, st(Step::DeclareBlockers));
    assert_eq!(s.current_phase(), Some(Phase::Combat));
    advance_to(&mut s, seg(Segment::PostcombatMain));
    assert_eq!(s.current_phase(), Some(Phase::PostcombatMain));
    drain(&mut s);
    assert_eq!(s.current_phase(), None);
}

/// REQ-TURN-003: no player in the game, no turn.
#[test]
fn no_player_in_game_gives_none() {
    let mut s = TurnScheduler::new(P1);
    assert_eq!(s.next_turn(&S3, &|_| false, false), None);
}

// ---------------------------------------------------------------------------------------------
// T1 extra turns
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003, CR 500.7: A then B pushed: B (most recent) first, then A, then the normal
/// order resumes after the last NORMAL turn's player.
#[test]
fn extra_turns_a_then_b() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S3);
    advance_to(&mut s, seg(Segment::PrecombatMain));
    s.add_extra_turn(P1);
    s.add_extra_turn(P2);
    assert_eq!(s.pending_extra_turns(), vec![P2, P1]);
    drain(&mut s);
    let t = next(&mut s, &S3);
    assert_eq!((t.player, t.extra), (P2, true));
    drain(&mut s);
    let t = next(&mut s, &S3);
    assert_eq!((t.player, t.extra), (P1, true));
    drain(&mut s);
    assert!(s.pending_extra_turns().is_empty());
    let t = next(&mut s, &S3);
    // After P1's normal turn, P2's normal turn (not after the last extra player).
    assert_eq!((t.player, t.extra), (P2, false));
}

/// REQ-TURN-003, CR 500.7: B then A pushed: A first.
#[test]
fn extra_turns_b_then_a() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turn(P2);
    s.add_extra_turn(P1);
    assert_eq!(s.pending_extra_turns(), vec![P1, P2]);
    let taken: Vec<(_, bool)> = (0..3)
        .map(|_| {
            let (t, _) = take(&mut s, &S3);
            (t.player, t.extra)
        })
        .collect();
    assert_eq!(taken, vec![(P1, true), (P2, true), (P2, false)]);
}

/// REQ-TURN-003, CR 500.7: several players' extra turns are added one at a time in APNAP
/// order, so the last APNAP player's is taken first.
#[test]
fn extra_turns_apnap_order() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turns_apnap(&[P1, P2, P3]);
    assert_eq!(s.pending_extra_turns(), vec![P3, P2, P1]);
    let taken: Vec<(_, bool)> = (0..4)
        .map(|_| {
            let (t, _) = take(&mut s, &S3);
            (t.player, t.extra)
        })
        .collect();
    assert_eq!(taken, vec![(P3, true), (P2, true), (P1, true), (P2, false)]);
}

/// REQ-TURN-003, CR 500.7: APNAP with a different active player (`[P2, P3, P1]`).
#[test]
fn extra_turns_apnap_other_active() {
    let mut s = TurnScheduler::new(P2);
    take(&mut s, &S3);
    s.add_extra_turns_apnap(&[P2, P3, P1]);
    assert_eq!(s.pending_extra_turns(), vec![P1, P3, P2]);
    assert_eq!(order(&mut s, &S3, 4), vec![P1, P3, P2, P3]);
}

/// REQ-TURN-003, CR 500.7: an extra turn created during an extra turn is taken before the
/// older pending ones ("the most recently created turn will be taken first").
#[test]
fn extra_turn_created_during_extra_turn_goes_first() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turn(P2);
    s.add_extra_turn(P3);
    let t = next(&mut s, &S3);
    assert_eq!((t.player, t.extra), (P3, true));
    advance_to(&mut s, st(Step::Upkeep));
    s.add_extra_turn(P1);
    assert_eq!(s.pending_extra_turns(), vec![P1, P2]);
    drain(&mut s);
    let taken: Vec<(_, bool)> = (0..3)
        .map(|_| {
            let (t, _) = take(&mut s, &S3);
            (t.player, t.extra)
        })
        .collect();
    assert_eq!(taken, vec![(P1, true), (P2, true), (P2, false)]);
}

/// REQ-TURN-003, CR 500.7: a taken extra turn does not change whose normal turn is next.
#[test]
fn extra_turn_does_not_move_normal_order() {
    let mut s = TurnScheduler::new(P1);
    assert_eq!(order(&mut s, &S3, 2), vec![P1, P2]);
    s.add_extra_turn(P1);
    let (t, _) = take(&mut s, &S3);
    assert_eq!((t.player, t.extra), (P1, true));
    // Normal order resumes after P2 (the last NORMAL turn), not after P1 (the extra one).
    assert_eq!(order(&mut s, &S3, 3), vec![P3, P1, P2]);
}

/// REQ-TURN-003, CR 500.7: `pending_extra_turns()` lists exactly the order taken.
#[test]
fn pending_extra_turns_is_the_taken_order() {
    for pushes in [
        vec![P1, P2, P3],
        vec![P3, P2, P1],
        vec![P2, P2, P1],
        vec![P1, P3],
    ] {
        let mut s = after_first_turn(&S3);
        for &p in &pushes {
            s.add_extra_turn(p);
        }
        let pending = s.pending_extra_turns();
        let taken: Vec<_> = (0..pending.len())
            .map(|_| {
                let (t, _) = take(&mut s, &S3);
                assert!(t.extra);
                t.player
            })
            .collect();
        assert_eq!(taken, pending);
        let mut rev = pushes.clone();
        rev.reverse();
        assert_eq!(taken, rev);
    }
}

/// REQ-TURN-003, CR 800.4k: an extra turn for a player who left is dropped and does not
/// begin; it is not a skip.
#[test]
fn extra_turn_of_departed_player_is_dropped() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turn(P3);
    let gone = |p: fineprint_base::ids::PlayerId| p != P3;
    let t = s.next_turn(&S3, &gone, false).expect("turn");
    assert_eq!(
        t,
        PlannedTurn {
            player: P2,
            extra: false,
            skipped: vec![],
            follow_ups: vec![]
        }
    );
    assert!(s.pending_extra_turns().is_empty());
    drain(&mut s);
    // P3's normal turn is passed over too: P1 follows P2.
    let t = s.next_turn(&S3, &gone, false).expect("turn");
    assert_eq!(t.player, P1);
}

/// REQ-TURN-003, CR 800.4k: a departed player's extra turn under another pending one is
/// dropped when it comes up; the others are taken.
#[test]
fn departed_extra_turn_dropped_between_others() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turn(P1);
    s.add_extra_turn(P3);
    s.add_extra_turn(P2);
    let gone = |p: fineprint_base::ids::PlayerId| p != P3;
    let mut taken = Vec::new();
    for _ in 0..3 {
        let t = s.next_turn(&S3, &gone, false).expect("turn");
        taken.push((t.player, t.extra));
        drain(&mut s);
    }
    assert_eq!(taken, vec![(P2, true), (P1, true), (P2, false)]);
}

/// REQ-TURN-003: if the last normal player is not in the seat order, the first in-game seat
/// is next.
#[test]
fn last_normal_player_not_seated_goes_to_first_in_game_seat() {
    let mut s = TurnScheduler::new(P3);
    take(&mut s, &S3);
    let t = s.next_turn(&[P1, P2], &everyone, false).expect("turn");
    assert_eq!(t.player, P1);
    drain(&mut s);
    let t = s.next_turn(&[P1, P2], &|p| p != P1, false).expect("turn");
    assert_eq!(t.player, P2);
}

// ---------------------------------------------------------------------------------------------
// T2 turn skips
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003, CR 614.1b, CR 614.10: one skip skips exactly that player's next turn; it is
/// reported and the normal order advances past it.
#[test]
fn one_turn_skip() {
    let mut s = after_first_turn(&S3);
    s.add_turn_skip(P2, None);
    assert_eq!(s.pending_turn_skips(P2), 1);
    assert_eq!(s.pending_turn_skips(P3), 0);
    let (t, _) = take(&mut s, &S3);
    assert_eq!(
        t,
        PlannedTurn {
            player: P3,
            extra: false,
            skipped: vec![SkippedTurn {
                player: P2,
                extra: false
            }],
            follow_ups: vec![]
        }
    );
    assert_eq!(s.pending_turn_skips(P2), 0);
    let (t, _) = take(&mut s, &S3);
    assert_eq!(t.player, P1);
    assert!(t.skipped.is_empty());
    assert_eq!(order(&mut s, &S3, 2), vec![P2, P3]);
}

/// REQ-TURN-003, CR 614.10a: two skips on one player skip that player's next two turns.
#[test]
fn two_turn_skips_skip_two_turns() {
    let mut s = after_first_turn(&S3);
    s.add_turn_skip(P2, None);
    s.add_turn_skip(P2, None);
    assert_eq!(s.pending_turn_skips(P2), 2);
    let turns: Vec<PlannedTurn> = (0..5).map(|_| take(&mut s, &S3).0).collect();
    let players: Vec<_> = turns.iter().map(|t| t.player).collect();
    assert_eq!(players, vec![P3, P1, P3, P1, P2]);
    assert_eq!(turns[0].skipped.len(), 1);
    assert!(turns[1].skipped.is_empty());
    assert_eq!(turns[2].skipped.len(), 1);
    assert_eq!(s.pending_turn_skips(P2), 0);
}

/// REQ-TURN-003, CR 614.10a: two-player game, two skips on P2: P1 takes three turns in a row.
#[test]
fn two_turn_skips_two_players() {
    let mut s = after_first_turn(&S2);
    s.add_turn_skip(P2, None);
    s.add_turn_skip(P2, None);
    assert_eq!(order(&mut s, &S2, 4), vec![P1, P1, P2, P1]);
}

/// REQ-TURN-003, CR 614.10: a skip on a player with a pending extra turn skips the extra turn
/// (their next turn) and leaves their later normal turn intact.
#[test]
fn skip_takes_the_pending_extra_turn() {
    let mut s = after_first_turn(&S3);
    s.add_extra_turn(P2);
    s.add_turn_skip(P2, None);
    let (t, _) = take(&mut s, &S3);
    assert_eq!(
        t,
        PlannedTurn {
            player: P2,
            extra: false,
            skipped: vec![SkippedTurn {
                player: P2,
                extra: true
            }],
            follow_ups: vec![]
        }
    );
    assert!(s.pending_extra_turns().is_empty());
    assert_eq!(s.pending_turn_skips(P2), 0);
    assert_eq!(order(&mut s, &S3, 1), vec![P3]);
}

/// REQ-TURN-003, CR 614.10: a skip created during the player's own turn (after `next_turn`)
/// waits for their next turn.
#[test]
fn skip_created_during_turn_waits() {
    let mut s = after_first_turn(&S3);
    let t = next(&mut s, &S3);
    assert_eq!(t.player, P2);
    advance_to(&mut s, seg(Segment::PrecombatMain));
    s.add_turn_skip(P2, None);
    // The current turn goes on to the end.
    let rest = drain(&mut s);
    assert_eq!(rest.last(), Some(&ep(Phase::Ending)));
    assert_eq!(s.pending_turn_skips(P2), 1);
    let turns: Vec<PlannedTurn> = (0..3).map(|_| take(&mut s, &S3).0).collect();
    let players: Vec<_> = turns.iter().map(|t| t.player).collect();
    assert_eq!(players, vec![P3, P1, P3]);
    assert_eq!(
        turns[2].skipped,
        vec![SkippedTurn {
            player: P2,
            extra: false
        }]
    );
}

/// REQ-TURN-003, CR 614.10b: follow-ups of skipped turns go, in creation order, to the next
/// turn actually taken, and not to any earlier turn. Both creation orders.
#[test]
fn follow_ups_collected_in_creation_order() {
    // P3's skip (follow-up 20) created before P2's (follow-up 10): consumption order is P2
    // then P3, creation order is 20 then 10.
    for (first, second, expected) in [
        (
            (P3, FollowUpId(20)),
            (P2, FollowUpId(10)),
            vec![FollowUpId(20), FollowUpId(10)],
        ),
        (
            (P2, FollowUpId(10)),
            (P3, FollowUpId(20)),
            vec![FollowUpId(10), FollowUpId(20)],
        ),
        // Consumption order P2 (20) then P3 (10); creation order 10 then 20.
        (
            (P3, FollowUpId(10)),
            (P2, FollowUpId(20)),
            vec![FollowUpId(10), FollowUpId(20)],
        ),
    ] {
        let mut s = after_first_turn(&S3);
        s.add_turn_skip(first.0, Some(first.1));
        s.add_turn_skip(second.0, Some(second.1));
        let (t, _) = take(&mut s, &S3);
        assert_eq!(t.player, P1);
        assert_eq!(
            t.skipped,
            vec![
                SkippedTurn {
                    player: P2,
                    extra: false
                },
                SkippedTurn {
                    player: P3,
                    extra: false
                }
            ]
        );
        assert_eq!(t.follow_ups, expected);
        // Nothing carried over.
        let (t, _) = take(&mut s, &S3);
        assert!(t.follow_ups.is_empty() && t.skipped.is_empty());
    }
}

/// REQ-TURN-003, CR 614.10b: a follow-up is not delivered to a turn taken before the skipped
/// one.
#[test]
fn follow_up_not_delivered_early() {
    let mut s = after_first_turn(&S3);
    s.add_turn_skip(P3, Some(FollowUpId(7)));
    s.add_turn_skip(P1, None);
    let (t, _) = take(&mut s, &S3);
    assert_eq!(t.player, P2);
    assert!(t.follow_ups.is_empty());
    assert!(t.skipped.is_empty());
    let (t, _) = take(&mut s, &S3);
    // P3 skipped (follow-up 7), P1 skipped (none): P2 takes the turn with the follow-up.
    assert_eq!(t.player, P2);
    assert_eq!(t.follow_ups, vec![FollowUpId(7)]);
    assert_eq!(t.skipped.len(), 2);
}

/// REQ-TURN-003, CR 614.10a: `pending_turn_skips` counts each call.
#[test]
fn pending_turn_skips_counts() {
    let mut s = TurnScheduler::new(P1);
    for k in 1..=3 {
        s.add_turn_skip(P3, None);
        assert_eq!(s.pending_turn_skips(P3), k);
    }
    s.add_turn_skip(P2, Some(FollowUpId(1)));
    assert_eq!(s.pending_turn_skips(P2), 1);
    assert_eq!(s.pending_turn_skips(P1), 0);
}

// ---------------------------------------------------------------------------------------------
// T3 phase and step skips
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003, CR 500.11, CR 614.10: a skipped phase yields no `BeginPhase`, segments or
/// `EndPhase`; skipping the combat phase removes every combat step.
#[test]
fn skipped_combat_phase_vanishes() {
    let mut s = TurnScheduler::new(P1);
    s.add_phase_skip(P1, Phase::Combat);
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 1);
    let (_, items) = take(&mut s, &S2);
    let expected = cat(vec![
        phase_items(Phase::Beginning, false),
        phase_items(Phase::PrecombatMain, false),
        phase_items(Phase::PostcombatMain, false),
        phase_items(Phase::Ending, false),
    ]);
    assert_eq!(items, expected);
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 0);
}

/// REQ-TURN-003, CR 500.11: skipping a step removes only it.
#[test]
fn skipped_step_removes_only_it() {
    let mut s = TurnScheduler::new(P1);
    s.add_step_skip(P1, Step::Draw);
    assert_eq!(s.pending_step_skips(P1, Step::Draw), 1);
    let (_, items) = take(&mut s, &S2);
    let mut expected = normal_plan(false);
    expected.retain(|i| *i != st(Step::Draw));
    assert_eq!(items, expected);
    assert_eq!(s.pending_step_skips(P1, Step::Draw), 0);
}

/// REQ-TURN-003, CR 614.10: a step skip added during the turn, before the step begins, is
/// consumed this turn (skips are consumed as the step would begin).
#[test]
fn step_skip_added_before_step_begins_applies_this_turn() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::Upkeep));
    s.add_step_skip(P1, Step::Draw);
    let rest = drain(&mut s);
    assert!(!rest.contains(&st(Step::Draw)));
    assert_eq!(rest[0], ep(Phase::Beginning));
    assert_eq!(s.pending_step_skips(P1, Step::Draw), 0);
}

/// REQ-TURN-003, CR 614.10: a skip created while the step is current waits for the next
/// occurrence: the step still runs this turn.
#[test]
fn step_skip_created_during_step_waits() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    let before = advance_to(&mut s, st(Step::Upkeep));
    s.add_step_skip(P1, Step::Upkeep);
    let rest = drain(&mut s);
    let mut this_turn = before;
    this_turn.extend(rest);
    assert_eq!(this_turn, normal_plan(false));
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 1);
    // P2's turn: not P1's skip.
    let (t, items) = take(&mut s, &S2);
    assert_eq!(t.player, P2);
    assert_eq!(items, normal_plan(false));
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 1);
    // P1's next turn has no upkeep.
    let (t, items) = take(&mut s, &S2);
    assert_eq!(t.player, P1);
    assert!(!items.contains(&st(Step::Upkeep)));
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 0);
}

/// REQ-TURN-003, CR 614.10: a phase skip created while the phase is current waits.
#[test]
fn phase_skip_created_during_phase_waits() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    let before = advance_to(&mut s, st(Step::DeclareAttackers));
    s.add_phase_skip(P1, Phase::Combat);
    let mut this_turn = before;
    this_turn.extend(drain(&mut s));
    assert_eq!(this_turn, normal_plan(false));
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 1);
    take(&mut s, &S2);
    let (_, items) = take(&mut s, &S2);
    assert!(!items.contains(&bp(Phase::Combat)));
    assert!(!items.contains(&st(Step::DeclareAttackers)));
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 0);
}

/// REQ-TURN-003, CR 614.10a: two skips of the same step cover two consecutive turns of that
/// player; a skip for P1 does not apply in P2's turn.
#[test]
fn two_step_skips_cover_two_turns() {
    let mut s = TurnScheduler::new(P1);
    s.add_step_skip(P1, Step::Upkeep);
    s.add_step_skip(P1, Step::Upkeep);
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 2);
    let mut upkeeps = Vec::new();
    for _ in 0..6 {
        let (t, items) = take(&mut s, &S2);
        upkeeps.push((t.player, items.contains(&st(Step::Upkeep))));
    }
    assert_eq!(
        upkeeps,
        vec![
            (P1, false),
            (P2, true),
            (P1, false),
            (P2, true),
            (P1, true),
            (P2, true)
        ]
    );
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 0);
}

/// REQ-TURN-003, CR 614.10: a skip for P2 created during P1's turn does not apply to P1.
#[test]
fn skip_for_other_player_does_not_apply() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S3);
    advance_to(&mut s, st(Step::Untap));
    s.add_step_skip(P2, Step::Draw);
    s.add_phase_skip(P2, Phase::Combat);
    let rest = drain(&mut s);
    assert!(rest.contains(&st(Step::Draw)));
    assert!(rest.contains(&bp(Phase::Combat)));
    assert_eq!(s.pending_step_skips(P2, Step::Draw), 1);
    assert_eq!(s.pending_phase_skips(P2, Phase::Combat), 1);
    let (t, items) = take(&mut s, &S3);
    assert_eq!(t.player, P2);
    assert!(!items.contains(&st(Step::Draw)));
    assert!(!items.contains(&bp(Phase::Combat)));
}

/// REQ-TURN-003, CR 500.11: skipping every step of the beginning phase leaves its
/// `BeginPhase` and `EndPhase` only.
#[test]
fn skipping_every_beginning_step_leaves_the_phase() {
    let mut s = TurnScheduler::new(P1);
    for step in [Step::Untap, Step::Upkeep, Step::Draw] {
        s.add_step_skip(P1, step);
    }
    let (_, items) = take(&mut s, &S2);
    let expected = cat(vec![
        vec![bp(Phase::Beginning), ep(Phase::Beginning)],
        phase_items(Phase::PrecombatMain, false),
        phase_items(Phase::Combat, false),
        phase_items(Phase::PostcombatMain, false),
        phase_items(Phase::Ending, false),
    ]);
    assert_eq!(items, expected);
}

/// REQ-TURN-003, CR 500.11: skipping a phase removes its steps with it; a step skip of that
/// phase stays pending (its step never would begin).
#[test]
fn phase_skip_removes_its_steps() {
    let mut s = TurnScheduler::new(P1);
    s.add_phase_skip(P1, Phase::Beginning);
    s.add_step_skip(P1, Step::Upkeep);
    let (_, items) = take(&mut s, &S2);
    assert_eq!(items[0], bp(Phase::PrecombatMain));
    assert_eq!(s.pending_phase_skips(P1, Phase::Beginning), 0);
    assert_eq!(s.pending_step_skips(P1, Step::Upkeep), 1);
    assert!(!items.iter().any(|i| matches!(
        i,
        PlanItem::Segment(Segment::Step(Step::Untap | Step::Upkeep | Step::Draw))
    )));
}

/// REQ-TURN-003, CR 614.10a: oldest phase skip consumed first; two phase skips cover two of
/// the player's turns.
#[test]
fn two_phase_skips() {
    let mut s = TurnScheduler::new(P1);
    s.add_phase_skip(P1, Phase::Combat);
    s.add_phase_skip(P1, Phase::Combat);
    let mut combats = Vec::new();
    for _ in 0..5 {
        let (t, items) = take(&mut s, &S2);
        combats.push((t.player, items.contains(&bp(Phase::Combat))));
    }
    assert_eq!(
        combats,
        vec![(P1, false), (P2, true), (P1, false), (P2, true), (P1, true)]
    );
}

// ---------------------------------------------------------------------------------------------
// T4 extra phases and steps
// ---------------------------------------------------------------------------------------------

/// The items of the rest of a normal turn after the combat phase.
fn after_combat() -> Vec<PlanItem> {
    cat(vec![
        phase_items(Phase::PostcombatMain, false),
        phase_items(Phase::Ending, false),
    ])
}

/// REQ-TURN-003, CR 500.8: an extra combat created during combat comes directly after it.
#[test]
fn extra_combat_during_combat() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::DeclareAttackers));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let rest = drain(&mut s);
    let expected = cat(vec![
        vec![
            st(Step::DeclareBlockers),
            st(Step::CombatDamage),
            st(Step::EndOfCombat),
            ep(Phase::Combat),
        ],
        phase_items(Phase::Combat, false),
        after_combat(),
    ]);
    assert_eq!(rest, expected);
}

/// REQ-TURN-003, CR 500.8: two extra combats created one after the other: three combats.
#[test]
fn two_extra_combats() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::BeginningOfCombat));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let rest = drain(&mut s);
    assert_eq!(rest.iter().filter(|i| **i == bp(Phase::Combat)).count(), 2);
    assert_eq!(rest.iter().filter(|i| **i == ep(Phase::Combat)).count(), 3);
    let tail = cat(vec![
        phase_items(Phase::Combat, false),
        phase_items(Phase::Combat, false),
        after_combat(),
    ]);
    assert!(rest.ends_with(&tail));
}

/// REQ-TURN-003, CR 500.8: several extra phases created after the same phase: the most
/// recently created occurs first. Both creation orders.
#[test]
fn extra_phases_after_same_phase_lifo() {
    for (first, second) in [
        (Phase::PostcombatMain, Phase::Combat),
        (Phase::Combat, Phase::PostcombatMain),
    ] {
        let mut s = TurnScheduler::new(P1);
        next(&mut s, &S2);
        advance_to(&mut s, st(Step::DeclareBlockers));
        assert!(s.add_extra_phase(Phase::Combat, first, None));
        assert!(s.add_extra_phase(Phase::Combat, second, None));
        let rest = drain(&mut s);
        let expected = cat(vec![
            vec![
                st(Step::CombatDamage),
                st(Step::EndOfCombat),
                ep(Phase::Combat),
            ],
            phase_items(second, false),
            phase_items(first, false),
            after_combat(),
        ]);
        assert_eq!(rest, expected, "created {first:?} then {second:?}");
    }
}

/// REQ-TURN-003, CR 500.8: an extra phase whose anchor has not started goes after the first
/// not-yet-started occurrence of it.
#[test]
fn extra_phase_after_future_phase() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::Upkeep));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let rest = drain(&mut s);
    let expected = cat(vec![
        vec![st(Step::Draw), ep(Phase::Beginning)],
        phase_items(Phase::PrecombatMain, false),
        phase_items(Phase::Combat, false),
        phase_items(Phase::Combat, false),
        after_combat(),
    ]);
    assert_eq!(rest, expected);
}

/// REQ-TURN-003, CR 500.8: an anchor phase that is over (and does not occur again) adds
/// nothing.
#[test]
fn extra_phase_after_past_phase_is_rejected() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, seg(Segment::PostcombatMain));
    assert!(!s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let rest = drain(&mut s);
    assert_eq!(
        rest,
        cat(vec![
            vec![ep(Phase::PostcombatMain)],
            phase_items(Phase::Ending, false)
        ])
    );
}

/// REQ-TURN-003, CR 505.1a: an extra main phase is a postcombat main phase; the first main
/// phase stays the only precombat one.
#[test]
fn extra_main_phase_is_postcombat() {
    // Created during the first main phase, directly after it.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, seg(Segment::PrecombatMain));
    assert!(s.add_extra_phase(Phase::PrecombatMain, Phase::PostcombatMain, None));
    let rest = drain(&mut s);
    assert_eq!(
        rest,
        cat(vec![
            vec![ep(Phase::PrecombatMain)],
            phase_items(Phase::PostcombatMain, false),
            phase_items(Phase::Combat, false),
            after_combat(),
        ])
    );
    // Created with the precombat phase kind after combat: its segment is still postcombat.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    let mut all = advance_to(&mut s, st(Step::Upkeep));
    assert!(s.add_extra_phase(Phase::Combat, Phase::PrecombatMain, None));
    all.extend(drain(&mut s));
    let segs = segments(&all);
    assert_eq!(
        segs.iter()
            .filter(|x| **x == Segment::PrecombatMain)
            .count(),
        1
    );
    assert_eq!(
        segs.iter()
            .filter(|x| **x == Segment::PostcombatMain)
            .count(),
        2
    );
    let main_positions: Vec<Segment> = segs
        .iter()
        .copied()
        .filter(|x| matches!(x, Segment::PrecombatMain | Segment::PostcombatMain))
        .collect();
    assert_eq!(
        main_positions,
        vec![
            Segment::PrecombatMain,
            Segment::PostcombatMain,
            Segment::PostcombatMain
        ]
    );
}

/// REQ-TURN-003, CR 500.9: an extra step after the end step.
#[test]
fn extra_step_after_end_step() {
    // Created during the end step.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::End));
    assert!(s.add_extra_step_after(Step::End, Step::End, None));
    assert_eq!(
        drain(&mut s),
        vec![st(Step::End), st(Step::Cleanup), ep(Phase::Ending)]
    );
    // Created before the end step begins.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, seg(Segment::PostcombatMain));
    assert!(s.add_extra_step_after(Step::End, Step::End, None));
    assert_eq!(
        drain(&mut s),
        vec![
            ep(Phase::PostcombatMain),
            bp(Phase::Ending),
            st(Step::End),
            st(Step::End),
            st(Step::Cleanup),
            ep(Phase::Ending)
        ]
    );
}

/// REQ-TURN-003, CR 500.9: an extra step after an anchor step, and before one.
#[test]
fn extra_step_before_and_after_anchor() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::Untap));
    assert!(s.add_extra_step_before(Step::Upkeep, Step::Draw, None));
    assert!(s.add_extra_step_after(Step::Upkeep, Step::Untap, None));
    let rest = drain(&mut s);
    assert_eq!(
        rest[..5],
        [
            st(Step::Draw),
            st(Step::Upkeep),
            st(Step::Untap),
            st(Step::Draw),
            ep(Phase::Beginning)
        ]
    );
    // Combat steps: before declare blockers, after combat damage.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::BeginningOfCombat));
    assert!(s.add_extra_step_before(Step::DeclareBlockers, Step::DeclareAttackers, None));
    assert!(s.add_extra_step_after(Step::CombatDamage, Step::CombatDamage, None));
    let rest = drain(&mut s);
    assert_eq!(
        rest[..7],
        [
            st(Step::DeclareAttackers),
            st(Step::DeclareAttackers),
            st(Step::DeclareBlockers),
            st(Step::CombatDamage),
            st(Step::CombatDamage),
            st(Step::EndOfCombat),
            ep(Phase::Combat)
        ]
    );
}

/// REQ-TURN-003, CR 500.9: an extra step after the current step comes directly after it.
#[test]
fn extra_step_after_current_step() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::Upkeep));
    assert!(s.add_extra_step_after(Step::Upkeep, Step::Upkeep, None));
    assert_eq!(
        drain(&mut s)[..3],
        [st(Step::Upkeep), st(Step::Draw), ep(Phase::Beginning)]
    );
}

/// REQ-TURN-003, CR 500.9: two extra steps created after the same step: the most recent
/// first. Both creation orders.
#[test]
fn extra_steps_after_same_step_lifo() {
    for (first, second) in [(Step::Untap, Step::Draw), (Step::Draw, Step::Untap)] {
        let mut s = TurnScheduler::new(P1);
        next(&mut s, &S2);
        advance_to(&mut s, st(Step::Untap));
        assert!(s.add_extra_step_after(Step::Upkeep, first, None));
        assert!(s.add_extra_step_after(Step::Upkeep, second, None));
        assert_eq!(
            drain(&mut s)[..5],
            [
                st(Step::Upkeep),
                st(second),
                st(first),
                st(Step::Draw),
                ep(Phase::Beginning)
            ],
            "created {first:?} then {second:?}"
        );
    }
    // Same, created during the anchor step.
    for (first, second) in [
        (Step::DeclareAttackers, Step::BeginningOfCombat),
        (Step::BeginningOfCombat, Step::DeclareAttackers),
    ] {
        let mut s = TurnScheduler::new(P1);
        next(&mut s, &S2);
        advance_to(&mut s, st(Step::EndOfCombat));
        assert!(s.add_extra_step_after(Step::EndOfCombat, first, None));
        assert!(s.add_extra_step_after(Step::EndOfCombat, second, None));
        assert_eq!(
            drain(&mut s)[..3],
            [st(second), st(first), ep(Phase::Combat)],
            "created {first:?} then {second:?}"
        );
    }
}

/// REQ-TURN-003, CR 500.9: an anchor step that already began or is past adds nothing; a step
/// of another phase adds nothing.
#[test]
fn extra_step_bad_anchor_rejected() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    let mut all = advance_to(&mut s, st(Step::Draw));
    // Past anchor.
    assert!(!s.add_extra_step_after(Step::Upkeep, Step::Upkeep, None));
    assert!(!s.add_extra_step_before(Step::Upkeep, Step::Upkeep, None));
    // "Before" the current step: it already began.
    assert!(!s.add_extra_step_before(Step::Draw, Step::Upkeep, None));
    // A step of another phase.
    assert!(!s.add_extra_step_after(Step::Draw, Step::CombatDamage, None));
    assert!(!s.add_extra_step_after(Step::End, Step::Upkeep, None));
    assert!(!s.add_extra_step_before(Step::DeclareBlockers, Step::End, None));
    all.extend(drain(&mut s));
    assert_eq!(all, normal_plan(false));
}

/// REQ-TURN-003, CR 500.10: an extra step after a phase creates that step's phase containing
/// only that step.
#[test]
fn extra_step_after_phase_creates_its_phase() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, seg(Segment::PrecombatMain));
    assert!(s.add_extra_step_after_phase(Phase::Combat, Step::Upkeep, None));
    let rest = drain(&mut s);
    let expected = cat(vec![
        vec![ep(Phase::PrecombatMain)],
        phase_items(Phase::Combat, false),
        phase_with(Phase::Beginning, &[Step::Upkeep]),
        after_combat(),
    ]);
    assert_eq!(rest, expected);
    // A combat step after the postcombat main phase, created during combat.
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::DeclareAttackers));
    assert!(s.add_extra_step_after_phase(Phase::PostcombatMain, Step::BeginningOfCombat, None));
    let rest = drain(&mut s);
    let tail = cat(vec![
        phase_items(Phase::PostcombatMain, false),
        phase_with(Phase::Combat, &[Step::BeginningOfCombat]),
        phase_items(Phase::Ending, false),
    ]);
    assert!(rest.ends_with(&tail), "{rest:?}");
}

/// REQ-TURN-003, CR 500.10a: a "you get" extra phase or step for a player other than the
/// active player adds nothing; the active player's, or no "you get", adds.
#[test]
fn you_get_only_in_your_own_turn() {
    let fresh = || {
        let mut s = TurnScheduler::new(P1);
        let t = next(&mut s, &S2);
        assert_eq!(t.player, P1);
        let before = advance_to(&mut s, st(Step::Upkeep));
        (s, before)
    };
    // Some(non-active): false, nothing changes.
    let (mut s, mut all) = fresh();
    assert!(!s.add_extra_phase(Phase::Combat, Phase::Combat, Some(P2)));
    assert!(!s.add_extra_step_after(Step::Upkeep, Step::Upkeep, Some(P2)));
    assert!(!s.add_extra_step_before(Step::Draw, Step::Upkeep, Some(P2)));
    assert!(!s.add_extra_step_after_phase(Phase::Combat, Step::Upkeep, Some(P2)));
    all.extend(drain(&mut s));
    assert_eq!(all, normal_plan(false));
    // Some(active) and None add.
    for who in [Some(P1), None] {
        let (mut s, _) = fresh();
        assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, who));
        let rest = drain(&mut s);
        assert_eq!(rest.iter().filter(|i| **i == bp(Phase::Combat)).count(), 2);

        let (mut s, _) = fresh();
        assert!(s.add_extra_step_after(Step::Upkeep, Step::Upkeep, who));
        assert_eq!(drain(&mut s)[0], st(Step::Upkeep));

        let (mut s, _) = fresh();
        assert!(s.add_extra_step_before(Step::Draw, Step::Upkeep, who));
        assert_eq!(drain(&mut s)[..2], [st(Step::Upkeep), st(Step::Draw)]);

        let (mut s, _) = fresh();
        assert!(s.add_extra_step_after_phase(Phase::Combat, Step::Upkeep, who));
        let rest = drain(&mut s);
        assert!(
            rest.windows(3)
                .any(|w| w == [bp(Phase::Beginning), st(Step::Upkeep), ep(Phase::Beginning)])
        );
    }
}

/// REQ-TURN-003, CR 500.10a: the "you get" check uses the turn's active player (P2's turn).
#[test]
fn you_get_in_other_players_turn() {
    let mut s = TurnScheduler::new(P1);
    take(&mut s, &S2);
    let t = next(&mut s, &S2);
    assert_eq!(t.player, P2);
    advance_to(&mut s, st(Step::Upkeep));
    assert!(!s.add_extra_phase(Phase::Combat, Phase::Combat, Some(P1)));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, Some(P2)));
}

/// REQ-TURN-003, CR 500.11, CR 500.8: a skip of the combat phase consumes the first combat
/// that would begin, an extra combat still runs.
#[test]
fn phase_skip_consumes_one_occurrence_with_extra_phase() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, st(Step::Upkeep));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    s.add_phase_skip(P1, Phase::Combat);
    let rest = drain(&mut s);
    assert_eq!(rest.iter().filter(|i| **i == bp(Phase::Combat)).count(), 1);
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 0);
}
