// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of turn control: REQ-TURN-003 (SRS 5.3; CR 500.7-500.12, CR 505.1a, CR 614.10,
// CR 614.10a, CR 614.10b, CR 800.4k, CR 725.1, CR 725.2, CR 724, CR 728), with REQ-CORE-001 for
// deterministic replay. Recording fake hosts and synthetic triggers only (`test.*` ids); no card
// data (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse,
    ChoiceSession, ChoiceTarget, ChoiceType, Interrupt, PendingChoiceRequest, RequestId, Status,
    ValidatorContract, ValidatorRule,
};
use crate::events::{Phase, Step};
use crate::format::{FirstTurnDrawRule, FormatConfig, PRESET_IDS, preset};
use crate::replacement::BusInterrupt;
use crate::sba::SbaOutcome;
use crate::state::GameState;
use crate::triggers::{
    PendingTrigger, PlacementEnv, StackedTrigger, TriggerOrigin, TriggerPlacement, place_triggers,
};
use crate::txn::Transactional;

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn all_in(_: PlayerId) -> bool {
    true
}

fn seats(n: u64) -> Vec<PlayerId> {
    (1..=n).map(p).collect()
}

/// Takes the next turn with everyone in the game and no first-strike step.
fn take(s: &mut TurnScheduler, seat_order: &[PlayerId]) -> PlannedTurn {
    s.next_turn(seat_order, &all_in, false).unwrap()
}

/// Every remaining item of the current turn.
fn drain(s: &mut TurnScheduler) -> Vec<PlanItem> {
    let mut v = Vec::new();
    while let Some(i) = s.next_item() {
        v.push(i);
    }
    v
}

/// Yields items until (and including) `item`; panics if the turn ends first.
fn advance_to(s: &mut TurnScheduler, item: PlanItem) {
    loop {
        match s.next_item() {
            Some(i) if i == item => return,
            Some(_) => {}
            None => panic!("{item:?} never yielded"),
        }
    }
}

fn seg(step: Step) -> PlanItem {
    PlanItem::Segment(Segment::Step(step))
}

/// The items of a normal turn (CR 505.1a: one precombat main, one postcombat main).
fn normal_items(first_strike: bool) -> Vec<PlanItem> {
    let mut v = Vec::new();
    for phase in [
        Phase::Beginning,
        Phase::PrecombatMain,
        Phase::Combat,
        Phase::PostcombatMain,
        Phase::Ending,
    ] {
        v.push(PlanItem::BeginPhase(phase));
        v.extend(
            segments_of(phase, first_strike)
                .into_iter()
                .map(PlanItem::Segment),
        );
        v.push(PlanItem::EndPhase(phase));
    }
    v
}

/// The players of a sequence of turns.
fn players_of(s: &mut TurnScheduler, seat_order: &[PlayerId], n: usize) -> Vec<(u64, bool)> {
    (0..n)
        .map(|_| {
            let t = take(s, seat_order);
            (t.player.get(), t.extra)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// (A) Extra turns
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_003_normal_turns_follow_seat_order_from_the_starting_player() {
    // REQ-TURN-003: the first normal turn is the starting player's, then seat order (wrapping).
    let order = seats(3);
    let mut s = TurnScheduler::new(p(2));
    assert_eq!(s.active_player(), None);
    assert_eq!(
        players_of(&mut s, &order, 4),
        vec![(2, false), (3, false), (1, false), (2, false)]
    );
    assert_eq!(s.active_player(), Some(p(2)));
}

#[test]
fn req_turn_003_cr_500_7_extra_turns_are_lifo_in_both_creation_orders() {
    // REQ-TURN-003, CR 500.7: "The most recently created turn will be taken first."
    for (first, second) in [(2, 3), (3, 2)] {
        let order = seats(3);
        let mut s = TurnScheduler::new(p(1));
        take(&mut s, &order);
        s.add_extra_turn(p(first));
        s.add_extra_turn(p(second));
        assert_eq!(s.pending_extra_turns(), vec![p(second), p(first)]);
        assert_eq!(
            players_of(&mut s, &order, 3),
            vec![(second, true), (first, true), (2, false)]
        );
        assert!(s.pending_extra_turns().is_empty());
    }
}

#[test]
fn req_turn_003_cr_500_7_apnap_extra_turns_last_player_first() {
    // REQ-TURN-003, CR 500.7: added one at a time in APNAP order, most recent taken first.
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    s.add_extra_turns_apnap(&[p(1), p(2), p(3)]);
    assert_eq!(s.pending_extra_turns(), vec![p(3), p(2), p(1)]);
    assert_eq!(
        players_of(&mut s, &order, 4),
        vec![(3, true), (2, true), (1, true), (2, false)]
    );
}

#[test]
fn req_turn_003_cr_800_4k_extra_turn_of_departed_player_is_dropped() {
    // REQ-TURN-003, CR 800.4k: "If a player who has left the game would begin a turn, that turn
    // doesn't begin."
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    s.add_extra_turn(p(3));
    let not_three = |q: PlayerId| q != p(3);
    let t = s.next_turn(&order, &not_three, false).unwrap();
    assert_eq!((t.player, t.extra), (p(2), false));
    assert!(t.skipped.is_empty());
    assert!(s.pending_extra_turns().is_empty());
    // The departed player's normal turn does not begin either.
    let t = s.next_turn(&order, &not_three, false).unwrap();
    assert_eq!(t.player, p(1));
}

#[test]
fn req_turn_003_cr_800_4k_departed_starting_player_does_not_begin() {
    // REQ-TURN-003, CR 800.4k: the starting player left before the first turn.
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    let not_one = |q: PlayerId| q != p(1);
    assert_eq!(s.next_turn(&order, &not_one, false).unwrap().player, p(2));
}

#[test]
fn req_turn_003_unseated_last_normal_player_falls_back_to_first_seat() {
    // REQ-TURN-003 (docs/DECISIONS.md "#50 FP-048 — Turn scheduler judgement calls"): a
    // starting player missing from the seat order is followed by the first in-game seat.
    let order = vec![p(4), p(5)];
    let mut s = TurnScheduler::new(p(9));
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(9));
    assert_eq!(take(&mut s, &order).player, p(4));
    let mut s = TurnScheduler::new(p(9));
    let not_four = |q: PlayerId| q != p(4);
    take(&mut s, &order);
    assert_eq!(s.next_turn(&order, &not_four, false).unwrap().player, p(5));
}

#[test]
fn req_turn_003_no_player_in_game_gives_no_turn() {
    // REQ-TURN-003, CR 800.4k: nobody can begin a turn.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_extra_turn(p(1));
    assert_eq!(s.next_turn(&order, &|_| false, false), None);
    assert_eq!(s.next_item(), None);
    assert_eq!(s.current_phase(), None);
}

#[test]
fn req_turn_003_cr_500_7_extra_turn_created_during_extra_turn_is_taken_next() {
    // REQ-TURN-003, CR 500.7: an extra turn created during an extra turn goes directly after it.
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    s.add_extra_turn(p(2));
    s.add_extra_turn(p(3));
    assert_eq!(take(&mut s, &order).player, p(3));
    s.add_extra_turn(p(1));
    assert_eq!(
        players_of(&mut s, &order, 3),
        vec![(1, true), (2, true), (2, false)]
    );
}

#[test]
fn req_turn_003_cr_500_7_normal_order_resumes_after_last_normal_player() {
    // REQ-TURN-003, CR 500.7: a taken extra turn does not move the normal turn order.
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    s.add_extra_turn(p(3));
    assert_eq!(
        players_of(&mut s, &order, 3),
        vec![(3, true), (2, false), (3, false)]
    );
}

// ---------------------------------------------------------------------------------------------
// (A) Turn skips
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_003_cr_614_10_one_skip_skips_only_the_next_turn() {
    // REQ-TURN-003, CR 614.10: "skip" is "instead of doing [something], do nothing".
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_turn_skip(p(1), None);
    assert_eq!(s.pending_turn_skips(p(1)), 1);
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(2));
    assert_eq!(
        t.skipped,
        vec![SkippedTurn {
            player: p(1),
            extra: false
        }]
    );
    assert!(t.follow_ups.is_empty());
    assert_eq!(s.pending_turn_skips(p(1)), 0);
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(1));
    assert!(t.skipped.is_empty());
}

#[test]
fn req_turn_003_cr_614_10a_two_skips_skip_the_next_two_turns() {
    // REQ-TURN-003, CR 614.10a: "that player must skip the next two".
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_turn_skip(p(2), None);
    s.add_turn_skip(p(2), None);
    assert_eq!(s.pending_turn_skips(p(2)), 2);
    let turns: Vec<PlannedTurn> = (0..4).map(|_| take(&mut s, &order)).collect();
    let players: Vec<u64> = turns.iter().map(|t| t.player.get()).collect();
    assert_eq!(players, vec![1, 1, 1, 2]);
    assert!(turns[0].skipped.is_empty());
    assert_eq!(turns[1].skipped.len(), 1);
    assert_eq!(turns[2].skipped.len(), 1);
    assert!(turns[3].skipped.is_empty());
    assert_eq!(s.pending_turn_skips(p(2)), 0);
}

#[test]
fn req_turn_003_cr_614_10_skip_hits_pending_extra_turn_first() {
    // REQ-TURN-003, CR 614.10: the skip applies to the next turn the player would begin: the
    // extra turn; the normal turn is kept.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    s.add_extra_turn(p(1));
    s.add_turn_skip(p(1), None);
    let t = take(&mut s, &order);
    assert_eq!((t.player, t.extra), (p(2), false));
    assert_eq!(
        t.skipped,
        vec![SkippedTurn {
            player: p(1),
            extra: true
        }]
    );
    assert_eq!(take(&mut s, &order).player, p(1));
}

#[test]
fn req_turn_003_cr_614_10_skip_made_during_the_turn_waits_for_the_next() {
    // REQ-TURN-003, CR 614.10: "Once a step, phase, or turn has started, it can no longer be
    // skipped—any skip effects will wait until the next occurrence."
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    assert_eq!(take(&mut s, &order).player, p(1));
    s.add_turn_skip(p(1), None);
    assert_eq!(drain(&mut s), normal_items(false));
    assert_eq!(take(&mut s, &order).player, p(2));
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(2));
    assert_eq!(t.skipped.len(), 1);
}

#[test]
fn req_turn_003_cr_614_10b_follow_ups_go_to_next_turn_taken_in_creation_order() {
    // REQ-TURN-003, CR 614.10b: the follow-up "is considered to be the first thing that
    // happens during the next step, phase, or turn to actually occur".
    let order = seats(3);
    let mut s = TurnScheduler::new(p(1));
    s.add_turn_skip(p(1), Some(FollowUpId(10)));
    s.add_turn_skip(p(2), Some(FollowUpId(20)));
    s.add_turn_skip(p(1), Some(FollowUpId(30)));
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(3));
    assert_eq!(
        t.skipped,
        vec![
            SkippedTurn {
                player: p(1),
                extra: false
            },
            SkippedTurn {
                player: p(2),
                extra: false
            },
        ]
    );
    assert_eq!(t.follow_ups, vec![FollowUpId(10), FollowUpId(20)]);
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(2));
    assert_eq!(t.follow_ups, vec![FollowUpId(30)]);
    assert!(FollowUpId(1) < FollowUpId(2));
    // Created in the opposite order to the one they are consumed in.
    let mut s = TurnScheduler::new(p(1));
    s.add_turn_skip(p(2), Some(FollowUpId(2)));
    s.add_turn_skip(p(1), Some(FollowUpId(1)));
    let t = take(&mut s, &order);
    assert_eq!(t.player, p(3));
    let skipped: Vec<PlayerId> = t.skipped.iter().map(|k| k.player).collect();
    assert_eq!(skipped, vec![p(1), p(2)]);
    assert_eq!(t.follow_ups, vec![FollowUpId(2), FollowUpId(1)]);
}

// ---------------------------------------------------------------------------------------------
// (A) Phase and step skips
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_003_cr_614_10_step_skip_consumed_as_the_step_would_begin() {
    // REQ-TURN-003, CR 614.10, CR 500.11: a skipped step yields no item.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_step_skip(p(1), Step::Upkeep);
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 1);
    take(&mut s, &order);
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 1);
    advance_to(&mut s, seg(Step::Untap));
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 1);
    assert_eq!(s.next_item(), Some(seg(Step::Draw)));
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 0);
    let mut expected = normal_items(false);
    expected.retain(|i| *i != seg(Step::Upkeep));
    let mut s = TurnScheduler::new(p(1));
    s.add_step_skip(p(1), Step::Upkeep);
    take(&mut s, &order);
    assert_eq!(drain(&mut s), expected);
}

#[test]
fn req_turn_003_cr_614_10_step_skip_made_while_step_runs_waits() {
    // REQ-TURN-003, CR 614.10: a skip created during the step waits for the next occurrence,
    // which is the player's next turn, not the other player's.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::Upkeep));
    s.add_step_skip(p(1), Step::Upkeep);
    assert!(!drain(&mut s).contains(&seg(Step::Upkeep)));
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 1);
    take(&mut s, &order);
    assert_eq!(drain(&mut s), normal_items(false));
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 1);
    take(&mut s, &order);
    assert!(!drain(&mut s).contains(&seg(Step::Upkeep)));
    assert_eq!(s.pending_step_skips(p(1), Step::Upkeep), 0);
}

#[test]
fn req_turn_003_cr_614_10a_two_step_skips_cover_two_turns() {
    // REQ-TURN-003, CR 614.10a.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    s.add_step_skip(p(1), Step::Draw);
    s.add_step_skip(p(1), Step::Draw);
    for left in [1, 0] {
        take(&mut s, &order);
        assert!(!drain(&mut s).contains(&seg(Step::Draw)));
        assert_eq!(s.pending_step_skips(p(1), Step::Draw), left);
    }
    take(&mut s, &order);
    assert!(drain(&mut s).contains(&seg(Step::Draw)));
}

#[test]
fn req_turn_003_cr_500_11_phase_skip_removes_its_steps() {
    // REQ-TURN-003, CR 500.11: no BeginPhase and no step of a skipped phase.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_phase_skip(p(1), Phase::Combat);
    assert_eq!(s.pending_phase_skips(p(1), Phase::Combat), 1);
    take(&mut s, &order);
    let items = drain(&mut s);
    let mut expected = normal_items(false);
    expected.retain(|i| match i {
        PlanItem::BeginPhase(ph) | PlanItem::EndPhase(ph) => *ph != Phase::Combat,
        PlanItem::Segment(sg) => sg.phase() != Phase::Combat,
    });
    assert_eq!(items, expected);
    assert_eq!(s.pending_phase_skips(p(1), Phase::Combat), 0);
}

#[test]
fn req_turn_003_cr_614_10_phase_skip_made_during_the_phase_waits() {
    // REQ-TURN-003, CR 614.10.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, PlanItem::BeginPhase(Phase::Beginning));
    s.add_phase_skip(p(1), Phase::Beginning);
    assert_eq!(s.next_item(), Some(seg(Step::Untap)));
    drain(&mut s);
    assert_eq!(s.pending_phase_skips(p(1), Phase::Beginning), 1);
    take(&mut s, &order);
    assert_eq!(
        s.next_item(),
        Some(PlanItem::BeginPhase(Phase::PrecombatMain))
    );
}

#[test]
fn req_turn_003_cr_500_11_skipping_every_step_keeps_the_phase() {
    // REQ-TURN-003, CR 500.11: skipping every step of a phase leaves the phase itself.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    for st in [Step::Untap, Step::Upkeep, Step::Draw] {
        s.add_step_skip(p(1), st);
    }
    take(&mut s, &order);
    assert_eq!(s.next_item(), Some(PlanItem::BeginPhase(Phase::Beginning)));
    assert_eq!(s.next_item(), Some(PlanItem::EndPhase(Phase::Beginning)));
}

#[test]
fn req_turn_003_cr_614_10_skips_apply_only_to_the_active_players_turn() {
    // REQ-TURN-003, CR 614.10: "skip your next draw step" waits for that player's turn.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    s.add_step_skip(p(2), Step::Draw);
    s.add_phase_skip(p(2), Phase::Combat);
    take(&mut s, &order);
    assert_eq!(drain(&mut s), normal_items(false));
    assert_eq!(s.pending_step_skips(p(2), Step::Draw), 1);
    assert_eq!(s.pending_phase_skips(p(2), Phase::Combat), 1);
    take(&mut s, &order);
    let items = drain(&mut s);
    assert!(!items.contains(&seg(Step::Draw)));
    assert!(!items.contains(&PlanItem::BeginPhase(Phase::Combat)));
}

#[test]
fn req_turn_003_cr_614_10_main_phase_skips_match_their_label() {
    // REQ-TURN-003, CR 614.10, CR 505.1a: skipping the precombat main phase leaves the
    // postcombat main phase a postcombat main phase (docs/DECISIONS.md "#50 FP-048 — Turn
    // scheduler judgement calls").
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    s.add_phase_skip(p(1), Phase::PrecombatMain);
    take(&mut s, &order);
    let items = drain(&mut s);
    assert!(!items.contains(&PlanItem::BeginPhase(Phase::PrecombatMain)));
    assert!(!items.contains(&PlanItem::Segment(Segment::PrecombatMain)));
    assert!(items.contains(&PlanItem::Segment(Segment::PostcombatMain)));
}

// ---------------------------------------------------------------------------------------------
// (A) Extra phases and steps
// ---------------------------------------------------------------------------------------------

/// The phases begun, in order.
fn phases_begun(items: &[PlanItem]) -> Vec<Phase> {
    items
        .iter()
        .filter_map(|i| match i {
            PlanItem::BeginPhase(p) => Some(*p),
            _ => None,
        })
        .collect()
}

#[test]
fn req_turn_003_cr_500_8_extra_combat_after_combat() {
    // REQ-TURN-003, CR 500.8: an additional combat phase directly after the combat phase.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::DeclareAttackers));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let items = drain(&mut s);
    assert_eq!(
        phases_begun(&items),
        vec![Phase::Combat, Phase::PostcombatMain, Phase::Ending]
    );
    let combat_steps = segments_of(Phase::Combat, false).len();
    let combat_items = items
        .iter()
        .filter(|i| matches!(i, PlanItem::Segment(sg) if sg.phase() == Phase::Combat))
        .count();
    assert_eq!(combat_items, 3 + combat_steps);
}

#[test]
fn req_turn_003_cr_500_8_extra_phases_after_same_phase_most_recent_first() {
    // REQ-TURN-003, CR 500.8: "the most recently created phase will occur first", both
    // creation orders.
    for (first, second) in [
        (Phase::Combat, Phase::PostcombatMain),
        (Phase::PostcombatMain, Phase::Combat),
    ] {
        let order = seats(1);
        let mut s = TurnScheduler::new(p(1));
        take(&mut s, &order);
        advance_to(&mut s, PlanItem::BeginPhase(Phase::Combat));
        assert!(s.add_extra_phase(Phase::Combat, first, None));
        assert!(s.add_extra_phase(Phase::Combat, second, None));
        let items = drain(&mut s);
        assert_eq!(
            phases_begun(&items),
            vec![second, first, Phase::PostcombatMain, Phase::Ending]
        );
    }
}

#[test]
fn req_turn_003_cr_505_1a_extra_main_phase_is_postcombat() {
    // REQ-TURN-003, CR 505.1a: "Only the first main phase of the turn is a precombat main
    // phase. All other main phases are postcombat main phases."
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    assert!(s.add_extra_phase(Phase::Combat, Phase::PrecombatMain, None));
    let items = drain(&mut s);
    let mains: Vec<PlanItem> = items
        .iter()
        .copied()
        .filter(|i| {
            matches!(
                i,
                PlanItem::Segment(Segment::PrecombatMain | Segment::PostcombatMain)
            )
        })
        .collect();
    assert_eq!(
        mains,
        vec![
            PlanItem::Segment(Segment::PrecombatMain),
            PlanItem::Segment(Segment::PostcombatMain),
            PlanItem::Segment(Segment::PostcombatMain),
        ]
    );
}

#[test]
fn req_turn_003_cr_505_1a_only_first_main_phase_is_precombat() {
    // REQ-TURN-003, CR 505.1a: an extra main phase before the normal precombat main phase is
    // the first main phase; the normal one becomes a postcombat main phase.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    assert!(s.add_extra_phase(Phase::Beginning, Phase::PostcombatMain, None));
    let items = drain(&mut s);
    assert_eq!(
        phases_begun(&items),
        vec![
            Phase::Beginning,
            Phase::PrecombatMain,
            Phase::PostcombatMain,
            Phase::Combat,
            Phase::PostcombatMain,
            Phase::Ending
        ]
    );
    let pre = items
        .iter()
        .filter(|i| **i == PlanItem::Segment(Segment::PrecombatMain))
        .count();
    assert_eq!(pre, 1);
}

#[test]
fn req_turn_003_cr_500_8_extra_phase_after_main_anchors_on_labels() {
    // REQ-TURN-003, CR 500.8: "after your postcombat main phase" anchors on the first
    // not-yet-started postcombat main phase.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    assert!(s.add_extra_phase(Phase::PostcombatMain, Phase::Combat, None));
    assert_eq!(
        phases_begun(&drain(&mut s)),
        vec![
            Phase::Beginning,
            Phase::PrecombatMain,
            Phase::Combat,
            Phase::PostcombatMain,
            Phase::Combat,
            Phase::Ending
        ]
    );
}

#[test]
fn req_turn_003_cr_500_9_extra_steps_after_lifo_and_before() {
    // REQ-TURN-003, CR 500.9: "the most recently created step will occur first" after the
    // same step; a step before a step goes directly before it.
    let order = seats(1);
    for (a, b) in [(Step::Untap, Step::Draw), (Step::Draw, Step::Untap)] {
        let mut s = TurnScheduler::new(p(1));
        take(&mut s, &order);
        assert!(s.add_extra_step_after(Step::Upkeep, a, None));
        assert!(s.add_extra_step_after(Step::Upkeep, b, None));
        // Directly before the anchor: the newest step is the one right before it.
        assert!(s.add_extra_step_before(Step::Untap, Step::Upkeep, None));
        assert!(s.add_extra_step_before(Step::Untap, Step::Draw, None));
        let items = drain(&mut s);
        assert_eq!(
            items[..9],
            [
                PlanItem::BeginPhase(Phase::Beginning),
                seg(Step::Upkeep),
                seg(Step::Draw),
                seg(Step::Untap),
                seg(Step::Upkeep),
                seg(b),
                seg(a),
                seg(Step::Draw),
                PlanItem::EndPhase(Phase::Beginning),
            ]
        );
    }
}

#[test]
fn req_turn_003_cr_500_9_extra_step_after_the_current_step_runs_next() {
    // REQ-TURN-003, CR 500.9: the current step is a valid "after" anchor, not a "before" one.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::CombatDamage));
    assert!(!s.add_extra_step_before(Step::CombatDamage, Step::DeclareBlockers, None));
    assert!(s.add_extra_step_after(Step::CombatDamage, Step::DeclareAttackers, None));
    assert!(s.add_extra_step_after(Step::CombatDamage, Step::DeclareBlockers, None));
    assert_eq!(s.next_item(), Some(seg(Step::DeclareBlockers)));
    assert_eq!(s.next_item(), Some(seg(Step::DeclareAttackers)));
    assert_eq!(s.next_item(), Some(seg(Step::EndOfCombat)));
}

#[test]
fn req_turn_003_cr_500_9_wrong_phase_or_past_anchor_adds_nothing() {
    // REQ-TURN-003, CR 500.9: the step must belong to the anchor's phase; an anchor that is
    // already over has no "next" occurrence this turn.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    assert!(!s.add_extra_step_after(Step::Upkeep, Step::End, None));
    assert!(!s.add_extra_step_before(Step::Upkeep, Step::CombatDamage, None));
    advance_to(&mut s, PlanItem::BeginPhase(Phase::PrecombatMain));
    assert!(!s.add_extra_step_after(Step::Upkeep, Step::Draw, None));
    assert!(!s.add_extra_step_before(Step::Draw, Step::Upkeep, None));
    assert!(!s.add_extra_phase(Phase::Beginning, Phase::Combat, None));
    assert!(!s.add_extra_step_after_phase(Phase::Beginning, Step::Upkeep, None));
    assert_eq!(
        drain(&mut s),
        normal_items(false)[6..].to_vec(),
        "nothing was added"
    );
}

#[test]
fn req_turn_003_cr_500_10_step_after_phase_creates_phase_with_only_that_step() {
    // REQ-TURN-003, CR 500.10: "Any other steps that phase would normally have are skipped".
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, PlanItem::BeginPhase(Phase::Combat));
    assert!(s.add_extra_step_after_phase(Phase::Combat, Step::Upkeep, None));
    advance_to(&mut s, PlanItem::EndPhase(Phase::Combat));
    assert_eq!(
        drain(&mut s)[..3],
        [
            PlanItem::BeginPhase(Phase::Beginning),
            seg(Step::Upkeep),
            PlanItem::EndPhase(Phase::Beginning),
        ]
    );
}

#[test]
fn req_turn_003_cr_500_10a_you_get_for_another_players_turn_adds_nothing() {
    // REQ-TURN-003, CR 500.10a: a "you get" effect adds nothing to another player's turn.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    assert!(!s.add_extra_phase(Phase::Combat, Phase::Combat, Some(p(2))));
    assert!(!s.add_extra_step_after(Step::Upkeep, Step::Draw, Some(p(2))));
    assert!(!s.add_extra_step_before(Step::Draw, Step::Upkeep, Some(p(2))));
    assert!(!s.add_extra_step_after_phase(Phase::Combat, Step::End, Some(p(2))));
    assert_eq!(drain(&mut s), normal_items(false));
    take(&mut s, &order);
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, Some(p(2))));
    assert_eq!(phases_begun(&drain(&mut s)).len(), 6);
}

#[test]
fn req_turn_003_cr_500_8_extra_phase_after_a_phase_that_just_ended() {
    // REQ-TURN-003, CR 500.8: between the end of the combat phase and the next phase, "after
    // this combat phase" is the front of the plan.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, PlanItem::EndPhase(Phase::Combat));
    assert_eq!(s.current_phase(), Some(Phase::Combat));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert_eq!(s.next_item(), Some(PlanItem::BeginPhase(Phase::Combat)));
}

#[test]
fn req_turn_003_nothing_is_added_before_or_after_a_turn() {
    // REQ-TURN-003: there is no plan before the first turn or after the turn is over.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    assert!(!s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert_eq!(s.next_item(), None);
    take(&mut s, &order);
    assert_eq!(s.current_phase(), None);
    drain(&mut s);
    assert_eq!(s.current_phase(), None);
    assert!(!s.add_extra_phase(Phase::Ending, Phase::Combat, None));
    assert!(!s.add_extra_step_after(Step::Cleanup, Step::End, None));
    assert!(!s.end_combat_phase_now());
}

#[test]
fn req_turn_003_first_strike_damage_step_in_plan() {
    // REQ-TURN-003: the plan uses segments_of with the first-strike flag, extra combats too.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    s.next_turn(&order, &all_in, true).unwrap();
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    let n = drain(&mut s)
        .iter()
        .filter(|i| **i == seg(Step::FirstStrikeDamage))
        .count();
    assert_eq!(n, 2);
}

// ---------------------------------------------------------------------------------------------
// (A) Ending the turn and the combat phase (plan only)
// ---------------------------------------------------------------------------------------------

fn tail_from_other_phase(phase: Phase) -> Vec<PlanItem> {
    vec![
        PlanItem::EndPhase(phase),
        PlanItem::BeginPhase(Phase::Ending),
        seg(Step::Cleanup),
        PlanItem::EndPhase(Phase::Ending),
    ]
}

#[test]
fn req_turn_003_cr_725_1d_end_turn_from_every_point_of_the_turn() {
    // REQ-TURN-003, CR 725.1d: "The game skips straight to the cleanup step"; CR 725.1e: the
    // end step never begins.
    let order = seats(1);
    for item in normal_items(false) {
        let mut s = TurnScheduler::new(p(1));
        take(&mut s, &order);
        s.add_extra_turn(p(1));
        advance_to(&mut s, item);
        let current = s.current_phase().unwrap();
        s.end_turn_now();
        let rest = drain(&mut s);
        let expected = match item {
            PlanItem::EndPhase(Phase::Ending) => Vec::new(),
            PlanItem::EndPhase(_) => vec![
                PlanItem::BeginPhase(Phase::Ending),
                seg(Step::Cleanup),
                PlanItem::EndPhase(Phase::Ending),
            ],
            _ if current == Phase::Ending => {
                vec![seg(Step::Cleanup), PlanItem::EndPhase(Phase::Ending)]
            }
            _ => tail_from_other_phase(current),
        };
        assert_eq!(rest, expected, "ending the turn at {item:?}");
        assert!(!rest.contains(&seg(Step::End)));
        assert_eq!(s.pending_extra_turns(), vec![p(1)]);
    }
}

#[test]
fn req_turn_003_cr_725_1d_end_turn_during_cleanup_starts_another_cleanup() {
    // REQ-TURN-003, CR 725.1d: "If an effect ends the turn during the cleanup step, a new
    // cleanup step begins."
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::Cleanup));
    s.end_turn_now();
    assert_eq!(
        drain(&mut s),
        vec![seg(Step::Cleanup), PlanItem::EndPhase(Phase::Ending)]
    );
}

#[test]
fn req_turn_003_cr_725_1d_end_turn_discards_extras_and_ignores_skips() {
    // REQ-TURN-003, CR 725.1d: extra phases and steps are discarded; skips never apply to the
    // cleanup step reached this way and stay pending.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::Upkeep));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert!(s.add_extra_step_after(Step::Upkeep, Step::Untap, None));
    s.add_phase_skip(p(1), Phase::Ending);
    s.add_step_skip(p(1), Step::Cleanup);
    s.add_turn_skip(p(1), None);
    s.end_turn_now();
    assert_eq!(drain(&mut s), tail_from_other_phase(Phase::Beginning));
    assert_eq!(s.pending_phase_skips(p(1), Phase::Ending), 1);
    assert_eq!(s.pending_step_skips(p(1), Step::Cleanup), 1);
    assert_eq!(s.pending_turn_skips(p(1)), 1);
}

#[test]
fn req_turn_003_cr_725_1d_end_turn_outside_a_turn_does_nothing() {
    // REQ-TURN-003, CR 725.1d: nothing before the first item or after the turn is over.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    let fresh = s.clone();
    s.end_turn_now();
    assert_eq!(s, fresh);
    take(&mut s, &order);
    let planned = s.clone();
    s.end_turn_now();
    assert_eq!(s, planned);
    assert_eq!(drain(&mut s), normal_items(false));
    let over = s.clone();
    s.end_turn_now();
    assert_eq!(s, over);
    assert_eq!(s.next_item(), None);
}

#[test]
fn req_turn_003_cr_725_2d_end_combat_phase_keeps_end_phase() {
    // REQ-TURN-003, CR 725.2d: "The game skips straight to the next phase"; CR 725.2g outside
    // combat nothing happens.
    let order = seats(1);
    let mut s = TurnScheduler::new(p(1));
    take(&mut s, &order);
    advance_to(&mut s, seg(Step::DeclareAttackers));
    assert!(s.end_combat_phase_now());
    assert_eq!(s.next_item(), Some(PlanItem::EndPhase(Phase::Combat)));
    assert!(!s.end_combat_phase_now());
    assert_eq!(
        s.next_item(),
        Some(PlanItem::BeginPhase(Phase::PostcombatMain))
    );
    let before = s.clone();
    assert!(!s.end_combat_phase_now());
    assert_eq!(before, s);
}

// ---------------------------------------------------------------------------------------------
// Recording fake host for the procedures
// ---------------------------------------------------------------------------------------------

/// A host call, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Turn(PlayerId),
    Phase(Phase),
    Step(Step),
    Expire(ExpiryPoint),
    Mana,
    Draw(PlayerId),
    Eot,
    Sba,
    Place,
    Resolve,
    Cease,
    Exile,
    RemoveCombat,
}

#[derive(Clone, Debug)]
struct Fake {
    game: GameState,
    log: Vec<Ev>,
    stack: Vec<String>,
    waiting: Vec<PendingTrigger>,
    /// The next `n` SBA passes perform something.
    sba_performs: u32,
    /// The next SBA pass makes a trigger wait.
    sba_triggers: bool,
}

impl Fake {
    fn new(n: usize) -> Fake {
        Fake {
            game: GameState::new(50, n, 20).unwrap(),
            log: Vec::new(),
            stack: Vec::new(),
            waiting: Vec::new(),
            sba_performs: 0,
            sba_triggers: false,
        }
    }

    fn p(&self, i: usize) -> PlayerId {
        self.game.turn_order()[i]
    }
}

fn trig(controller: PlayerId, id: &str) -> PendingTrigger {
    PendingTrigger {
        origin: TriggerOrigin::Event,
        controller,
        source: None,
        ability_id: id.to_owned(),
        bindings: BTreeMap::new(),
        optional: false,
        responds_to_trigger: false,
        targets: Vec::new(),
        modes: None,
    }
}

fn sba_outcome(performed: bool) -> SbaOutcome {
    SbaOutcome {
        performed,
        findings: Vec::new(),
        executed: Vec::new(),
        lost_players: Vec::new(),
        loss_reasons: BTreeMap::new(),
        ceased: Vec::new(),
        unattached: Vec::new(),
        removed_from_game: Vec::new(),
        protectors_chosen: Vec::new(),
        speed_set: Vec::new(),
    }
}

struct NoTargets;

impl PlacementEnv for NoTargets {
    fn legal_targets(
        &self,
        _: &PendingTrigger,
        _: Option<usize>,
        _: usize,
        _: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        Vec::new()
    }
}

impl HeartbeatHost for Fake {
    fn game(&self) -> &GameState {
        &self.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }
    fn sba_pass(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<SbaOutcome, BusInterrupt> {
        self.log.push(Ev::Sba);
        if std::mem::take(&mut self.sba_triggers)
            && let Some(a) = self.game.turn().active_player()
        {
            self.waiting.push(trig(a, "test.sba_trigger"));
        }
        let performed = self.sba_performs > 0;
        self.sba_performs = self.sba_performs.saturating_sub(1);
        Ok(sba_outcome(performed))
    }
    fn place_triggers(
        &mut self,
        apnap: &[PlayerId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<TriggerPlacement, Interrupt> {
        self.log.push(Ev::Place);
        let waiting = std::mem::take(&mut self.waiting);
        let placement = place_triggers(&NoTargets, choices, apnap, waiting)?;
        for s in &placement.stacked {
            self.stack.push(s.trigger.ability_id.clone());
        }
        Ok(placement)
    }
    fn stack_is_empty(&self) -> bool {
        self.stack.is_empty()
    }
    fn resolve_top(&mut self, _choices: &mut ChoiceContext<'_>) -> Result<(), Interrupt> {
        self.log.push(Ev::Resolve);
        self.stack.pop();
        Ok(())
    }
    fn priority_actions(&self, _player: PlayerId) -> Vec<ChoiceResponse> {
        Vec::new()
    }
    fn take_action(
        &mut self,
        _player: PlayerId,
        _action: &ChoiceResponse,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

impl TurnHost for Fake {
    fn begin_turn(&mut self, active: PlayerId) -> Result<(), Interrupt> {
        self.log.push(Ev::Turn(active));
        let n = self.game.turn().turn_number() + 1;
        self.game.set_turn_number(n);
        self.game
            .set_active_player(Some(active))
            .map_err(|e| Interrupt::Error(ChoiceError::Procedure(e.to_string())))
    }
    fn begin_phase(&mut self, phase: Phase) -> Result<(), Interrupt> {
        self.log.push(Ev::Phase(phase));
        Ok(())
    }
    fn begin_step(&mut self, step: Step) -> Result<(), Interrupt> {
        self.log.push(Ev::Step(step));
        Ok(())
    }
    fn expire_effects(&mut self, at: ExpiryPoint) -> Result<(), Interrupt> {
        self.log.push(Ev::Expire(at));
        Ok(())
    }
    fn empty_mana_pools(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Mana);
        Ok(())
    }
    fn apply_phasing(&mut self, _active: PlayerId) -> Result<(), Interrupt> {
        Ok(())
    }
    fn check_day_night(&mut self, _previous: Option<PlayerId>) -> Result<(), Interrupt> {
        Ok(())
    }
    fn untap_options(&self, _active: PlayerId) -> UntapOptions {
        UntapOptions::default()
    }
    fn untap(
        &mut self,
        _active: PlayerId,
        _permanents: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
    fn first_turn_draw_rule(&self) -> FirstTurnDrawRule {
        FirstTurnDrawRule::NeverSkip
    }
    fn starting_player(&self) -> Option<PlayerId> {
        None
    }
    fn draw_card(
        &mut self,
        player: PlayerId,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push(Ev::Draw(player));
        Ok(())
    }
    fn hand(&self, _player: PlayerId) -> Vec<ObjectId> {
        Vec::new()
    }
    fn max_hand_size(&self, _player: PlayerId) -> Option<u64> {
        None
    }
    fn discard_cards(
        &mut self,
        _player: PlayerId,
        _cards: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
    fn end_of_turn_cleanup(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Eot);
        Ok(())
    }
    fn sagas_with_chapter_abilities(&self, _player: PlayerId) -> Vec<ObjectId> {
        Vec::new()
    }
    fn add_lore_counters(
        &mut self,
        _sagas: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

impl EndTurnHost for Fake {
    fn cease_pending_triggers(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Cease);
        self.waiting.clear();
        Ok(())
    }
    fn exile_stack(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::Exile);
        self.stack.clear();
        Ok(())
    }
    fn remove_all_from_combat(&mut self) -> Result<(), Interrupt> {
        self.log.push(Ev::RemoveCombat);
        Ok(())
    }
}

/// The session state: the host, the sequencer state and the scheduler.
#[derive(Clone, Debug)]
struct World {
    host: Fake,
    ts: TurnState,
    sched: TurnScheduler,
}

impl Transactional for World {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        self.host.game().turn().priority_player()
    }
    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

fn world(n: usize) -> World {
    let host = Fake::new(n);
    let sched = TurnScheduler::new(host.p(0));
    World {
        host,
        ts: TurnState::new(),
        sched,
    }
}

/// Runs `f` to the end in a session, passing every priority request; returns the final world,
/// the output and the number of requests asked.
fn run<O, F>(w: World, f: F) -> (World, O, usize)
where
    O: Clone,
    F: Fn(&mut World, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let mut session = ChoiceSession::start(w, f).unwrap();
    let mut asked = 0;
    loop {
        let req = match session.status() {
            Status::Done { state, output } => return (state.clone(), output.clone(), asked),
            Status::Pending(r) => r.clone(),
        };
        asked += 1;
        assert_eq!(req.choice_type, ChoiceType::PriorityAction);
        session
            .submit(req.request_id, ChoiceResponse::PassPriority)
            .unwrap();
    }
}

/// Runs one item as `run_planned_turn` does.
fn dispatch(
    w: &mut World,
    item: PlanItem,
    c: &mut ChoiceContext<'_>,
    reports: &mut Vec<SegmentReport>,
) -> Result<(), Interrupt> {
    match item {
        PlanItem::BeginPhase(ph) => {
            w.host.begin_phase(ph)?;
            w.host.expire_effects(ExpiryPoint::BeginPhase(ph))
        }
        PlanItem::Segment(sg) => {
            reports.push(run_segment(
                &mut w.host,
                &mut w.ts,
                &TurnBasedRegistry::standard(),
                sg,
                c,
            )?);
            Ok(())
        }
        PlanItem::EndPhase(ph) => {
            w.host.expire_effects(ExpiryPoint::EndPhase(ph))?;
            w.host.empty_mana_pools()
        }
    }
}

// ---------------------------------------------------------------------------------------------
// (B) end_the_turn / end_the_combat_phase
// ---------------------------------------------------------------------------------------------

/// Begins a planned turn and runs items until `stop` (inclusive) through `dispatch`.
fn run_until(
    w: &mut World,
    stop: PlanItem,
    c: &mut ChoiceContext<'_>,
    reports: &mut Vec<SegmentReport>,
) -> Result<(), Interrupt> {
    let order = w.host.game().turn_order().to_vec();
    let t = w.sched.next_turn(&order, &all_in, false).unwrap();
    w.host.begin_turn(t.player)?;
    loop {
        let item = w.sched.next_item().unwrap();
        dispatch(w, item, c, reports)?;
        if item == stop {
            return Ok(());
        }
    }
}

#[test]
fn req_turn_003_cr_725_1_end_the_turn_steps_in_order() {
    // REQ-TURN-003, CR 725.1a, CR 725.1b, CR 725.1c, CR 725.1d in order; the SBA check repeats
    // while it performs; no trigger placement; no combat removal outside combat.
    let (w, report, asked) = run(world(2), |w, c| {
        let mut reports = Vec::new();
        run_until(w, seg(Step::Untap), c, &mut reports)?;
        w.host.log.clear();
        w.host.sba_performs = 2;
        w.host.waiting.push(trig(w.host.p(0), "test.before"));
        w.host.stack.push("test.resolving".to_owned());
        end_the_turn(&mut w.host, &mut w.sched, c)
    });
    assert_eq!(asked, 0);
    assert_eq!(
        report,
        EndTurnReport {
            phase_ended: Some(Phase::Beginning)
        }
    );
    assert_eq!(
        w.host.log,
        vec![Ev::Cease, Ev::Exile, Ev::Sba, Ev::Sba, Ev::Sba]
    );
    assert!(w.host.waiting.is_empty() && w.host.stack.is_empty());
    let mut sched = w.sched.clone();
    assert_eq!(drain(&mut sched), tail_from_other_phase(Phase::Beginning));
}

#[test]
fn req_turn_003_cr_725_1d_end_the_turn_in_combat_removes_from_combat() {
    // REQ-TURN-003, CR 725.1d: "If this happens during combat, remove all creatures and
    // planeswalkers from combat."
    let (w, report, _) = run(world(2), |w, c| {
        let mut reports = Vec::new();
        run_until(w, seg(Step::DeclareAttackers), c, &mut reports)?;
        w.host.log.clear();
        end_the_turn(&mut w.host, &mut w.sched, c)
    });
    assert_eq!(report.phase_ended, Some(Phase::Combat));
    assert_eq!(
        w.host.log,
        vec![Ev::Cease, Ev::Exile, Ev::Sba, Ev::RemoveCombat]
    );
}

#[test]
fn req_turn_003_cr_725_1f_triggers_during_the_process_stack_in_cleanup() {
    // REQ-TURN-003, CR 725.1f: a trigger that triggers during the process waits, is put on the
    // stack during the cleanup step, the active player gets priority, then another cleanup
    // step (CR 514.3a); CR 725.1e: the end step never begins.
    let (w, reports, asked) = run(world(2), |w, c| {
        let mut reports = Vec::new();
        run_until(w, seg(Step::Upkeep), c, &mut reports)?;
        w.host.log.clear();
        w.host.sba_triggers = true;
        end_the_turn(&mut w.host, &mut w.sched, c)?;
        assert!(!w.host.waiting.is_empty(), "the trigger still waits");
        while let Some(item) = w.sched.next_item() {
            dispatch(w, item, c, &mut reports)?;
        }
        Ok(reports)
    });
    let log = &w.host.log;
    assert_eq!(log[..3], [Ev::Cease, Ev::Exile, Ev::Sba]);
    assert!(!log.contains(&Ev::Step(Step::End)));
    assert!(!log.contains(&Ev::Draw(w.host.p(0))));
    let first_place = log.iter().position(|e| *e == Ev::Place).unwrap();
    let first_cleanup = log
        .iter()
        .position(|e| *e == Ev::Step(Step::Cleanup))
        .unwrap();
    assert!(first_cleanup < first_place, "no placement before cleanup");
    assert!(log.contains(&Ev::Resolve));
    let cleanup = reports.last().unwrap();
    assert_eq!(cleanup.segment, Segment::Step(Step::Cleanup));
    assert_eq!(cleanup.cleanup_steps, 2);
    assert!(cleanup.heartbeat_ran);
    assert!(asked >= 4, "upkeep and cleanup priority rounds");
    assert!(w.host.stack.is_empty());
}

#[test]
fn req_turn_003_cr_725_1c_sba_check_is_bounded() {
    // REQ-TURN-003, CR 725.1c: a check that never settles is a procedure error.
    let mut w = world(2);
    w.host.sba_performs = u32::MAX;
    let err = ChoiceSession::start(w, |w: &mut World, c: &mut ChoiceContext<'_>| {
        end_the_turn(&mut w.host, &mut w.sched, c)
    })
    .err()
    .unwrap();
    assert!(matches!(err, ChoiceError::Procedure(_)));
}

#[test]
fn req_turn_003_cr_725_2_end_the_combat_phase() {
    // REQ-TURN-003, CR 725.2d in combat; CR 725.2g outside combat nothing happens.
    let (w, ended, _) = run(world(2), |w, c| {
        let mut reports = Vec::new();
        run_until(w, seg(Step::BeginningOfCombat), c, &mut reports)?;
        w.host.log.clear();
        let ended = end_the_combat_phase(&mut w.host, &mut w.sched)?;
        assert_eq!(w.host.log, vec![Ev::RemoveCombat]);
        while let Some(item) = w.sched.next_item() {
            dispatch(w, item, c, &mut reports)?;
        }
        Ok(ended)
    });
    assert!(ended);
    assert!(!w.host.log.contains(&Ev::Step(Step::DeclareAttackers)));
    assert!(
        w.host
            .log
            .contains(&Ev::Expire(ExpiryPoint::EndPhase(Phase::Combat)))
    );
    let (w, ended, _) = run(world(2), |w, c| {
        let mut reports = Vec::new();
        run_until(w, seg(Step::Upkeep), c, &mut reports)?;
        w.host.log.clear();
        end_the_combat_phase(&mut w.host, &mut w.sched)
    });
    assert!(!ended);
    assert!(w.host.log.is_empty());
}

// ---------------------------------------------------------------------------------------------
// (E) run_planned_turn
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_003_run_planned_turn_mirrors_run_turn() {
    // REQ-TURN-003: an unmodified planned turn makes exactly the host calls of run_turn.
    let (planned, report, asked) = run(world(2), |w, c| {
        let order = w.host.game().turn_order().to_vec();
        let t = w.sched.next_turn(&order, &all_in, false).unwrap();
        run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            &mut w.sched,
            &t,
            c,
        )
    });
    let (plain, plain_report, plain_asked) = run(world(2), |w, c| {
        let a = w.host.p(0);
        run_turn(&mut w.host, &mut w.ts, &TurnBasedRegistry::standard(), a, c)
    });
    assert_eq!(planned.host.log, plain.host.log);
    assert_eq!(report, plain_report);
    assert_eq!(asked, plain_asked);
    assert_eq!(report.active_player, planned.host.p(0));
}

#[test]
fn req_turn_003_run_planned_turn_records_previous_active() {
    // REQ-TURN-003: a planned turn records its active player for the next CR 502.2 check.
    let (w, _, _) = run(world(2), |w, c| {
        assert_eq!(w.ts.previous_active_player(), None);
        let order = w.host.game().turn_order().to_vec();
        let t = w.sched.next_turn(&order, &all_in, false).unwrap();
        run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            &mut w.sched,
            &t,
            c,
        )
    });
    assert_eq!(w.ts.previous_active_player(), Some(w.host.p(0)));
}

#[test]
fn req_turn_003_run_planned_turn_applies_skips_and_extra_phases() {
    // REQ-TURN-003, CR 614.10, CR 500.8: skips and extra phases reach the sequencer.
    let (w, report, _) = run(world(2), |w, c| {
        let order = w.host.game().turn_order().to_vec();
        let a = order[0];
        let t = w.sched.next_turn(&order, &all_in, false).unwrap();
        w.sched.add_step_skip(a, Step::Draw);
        w.sched.add_phase_skip(a, Phase::Combat);
        assert!(
            w.sched
                .add_extra_phase(Phase::PostcombatMain, Phase::Combat, None)
        );
        run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            &mut w.sched,
            &t,
            c,
        )
    });
    assert!(!w.host.log.contains(&Ev::Step(Step::Draw)));
    let combats = w
        .host
        .log
        .iter()
        .filter(|e| **e == Ev::Phase(Phase::Combat))
        .count();
    assert_eq!(
        combats, 1,
        "the normal combat is skipped, the extra one runs"
    );
    assert_eq!(
        report.segments.len(),
        2 + 1 + 1 + segments_of(Phase::Combat, false).len() + 2
    );
}

#[test]
fn req_core_001_req_turn_003_planned_turn_replay_is_deterministic() {
    // REQ-CORE-001, REQ-TURN-003: the same base and answers give the same final state.
    let go = || {
        run(world(3), |w, c| {
            let order = w.host.game().turn_order().to_vec();
            w.sched.add_extra_turn(order[2]);
            let t = w.sched.next_turn(&order, &all_in, false).unwrap();
            run_planned_turn(
                &mut w.host,
                &mut w.ts,
                &TurnBasedRegistry::standard(),
                &mut w.sched,
                &t,
                c,
            )
        })
    };
    let (a, ra, na) = go();
    let (b, rb, nb) = go();
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!((ra, na), (rb, nb));
}

// ---------------------------------------------------------------------------------------------
// (C) Controlling another player
// ---------------------------------------------------------------------------------------------

fn request(chooser: PlayerId, reveal_to: Vec<PlayerId>) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser,
        choice_type: ChoiceType::PriorityAction,
        reason: ChoiceReason {
            cr: "CR 724.5".to_owned(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: true,
        reveal_to,
        default_on_timeout: ChoiceResponse::PassPriority,
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        }),
        payload: ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: false,
            actions: Vec::new(),
        },
    }
}

/// p(1) is controlled by p(2) during p(1)'s turn.
fn controlled_turn() -> PlayerControl {
    let mut pc = PlayerControl::new();
    pc.add_control_effect(p(1), p(2));
    pc.begin_turn(p(1));
    pc
}

#[test]
fn req_turn_003_cr_724_5_requests_route_to_the_controller() {
    // REQ-TURN-003, CR 724.5: the controller makes all choices; CR 724.3: the request is
    // unchanged (the chooser still names the controlled player).
    let pc = controlled_turn();
    let req = request(p(1), vec![p(3)]);
    let routed = pc.route(&req);
    assert_eq!(routed.decider, p(2));
    assert_eq!(routed.request, req);
    assert_eq!(routed.request.chooser, p(1));
    assert_eq!(routed.viewers, BTreeSet::from([p(1), p(2), p(3)]));
    assert_eq!(pc.check_answerer(&req, p(2)), Ok(()));
    let err = pc.check_answerer(&req, p(1)).unwrap_err();
    assert_eq!(
        err,
        ControlError::NotTheDecider {
            chooser: p(1),
            expected: p(2),
            got: p(1)
        }
    );
    assert!(!err.to_string().is_empty());
    let as_error: &dyn std::error::Error = &err;
    assert!(as_error.source().is_none());
    assert!(pc.check_answerer(&req, p(3)).is_err());
}

#[test]
fn req_turn_003_cr_724_8_controller_keeps_their_own_requests() {
    // REQ-TURN-003, CR 724.8: "A player who controls another player also continues to make
    // their own choices and decisions."
    let pc = controlled_turn();
    let own = request(p(2), Vec::new());
    let routed = pc.route(&own);
    assert_eq!(routed.decider, p(2));
    assert_eq!(routed.viewers, BTreeSet::from([p(2)]));
    assert_eq!(pc.check_answerer(&own, p(2)), Ok(()));
    assert!(pc.check_answerer(&own, p(1)).is_err());
    let other = request(p(3), Vec::new());
    assert_eq!(pc.route(&other).decider, p(3));
}

#[test]
fn req_turn_003_cr_724_1a_last_created_effect_wins() {
    // REQ-TURN-003, CR 724.1a: "The last one to be created is the one that works."
    let mut pc = PlayerControl::new();
    pc.add_control_effect(p(1), p(2));
    pc.add_control_effect(p(1), p(3));
    assert_eq!(pc.pending_controller(p(1)), Some(p(3)));
    assert!(!pc.is_controlled(p(1)), "pending until the turn begins");
    pc.begin_turn(p(1));
    assert_eq!(pc.controller_of(p(1)), p(3));
    assert!(pc.is_controlled(p(1)));
    assert_eq!(pc.pending_controller(p(1)), None);
}

#[test]
fn req_turn_003_cr_724_9_self_control_cancels() {
    // REQ-TURN-003, CR 724.9: "That player will make their own decisions and choices as
    // normal."
    let mut pc = PlayerControl::new();
    pc.add_control_effect(p(1), p(2));
    pc.add_control_effect(p(1), p(1));
    pc.begin_turn(p(1));
    assert_eq!(pc.controller_of(p(1)), p(1));
    assert!(!pc.is_controlled(p(1)));
}

#[test]
fn req_turn_003_cr_724_1_effect_ends_at_next_turn_and_waits_through_skips() {
    // REQ-TURN-003, CR 724.1: the effect ends at the beginning of the next turn; CR 724.1b: a
    // skipped turn leaves it pending.
    let order = seats(2);
    let mut s = TurnScheduler::new(p(1));
    let mut pc = PlayerControl::new();
    pc.add_control_effect(p(2), p(1));
    s.add_turn_skip(p(2), None);
    for (i, expected) in [1, 1, 2, 1].into_iter().enumerate() {
        let t = take(&mut s, &order);
        pc.begin_turn(t.player);
        assert_eq!(t.player, p(expected));
        assert_eq!(pc.is_controlled(p(2)), expected == 2);
        assert_eq!(pc.controller_of(p(2)) == p(1), expected == 2);
        let still_pending = if i < 2 { Some(p(1)) } else { None };
        assert_eq!(pc.pending_controller(p(2)), still_pending);
    }
}

#[test]
fn req_turn_003_cr_800_4a_control_effects_end_when_a_player_leaves() {
    // REQ-TURN-003, CR 800.4a: effects giving the departed player control end; effects on the
    // departed player are removed.
    let mut pc = controlled_turn();
    pc.add_control_effect(p(3), p(2));
    pc.add_control_effect(p(4), p(1));
    pc.player_left(p(2));
    assert!(!pc.is_controlled(p(1)));
    assert_eq!(pc.pending_controller(p(3)), None);
    assert_eq!(pc.pending_controller(p(4)), Some(p(1)));
    pc.player_left(p(4));
    assert_eq!(pc.pending_controller(p(4)), None);
    let mut pc = controlled_turn();
    pc.player_left(p(1));
    assert!(!pc.is_controlled(p(1)));
    let mut pc = controlled_turn();
    pc.player_left(p(3));
    assert!(pc.is_controlled(p(1)));
}

#[test]
fn req_turn_003_cr_724_4_outside_game_information_never_reaches_the_controller() {
    // REQ-TURN-003, CR 724.4, R12: in-game information is shared with the controller; outside
    // the game it is visible only to the controlled player.
    let pc = controlled_turn();
    assert!(pc.can_view(p(2), p(1), InfoScope::InGame));
    assert!(pc.can_view(p(1), p(1), InfoScope::InGame));
    assert!(!pc.can_view(p(3), p(1), InfoScope::InGame));
    assert!(!pc.can_view(p(1), p(2), InfoScope::InGame));
    assert!(!pc.can_view(p(2), p(1), InfoScope::OutsideGame));
    assert!(pc.can_view(p(1), p(1), InfoScope::OutsideGame));
    assert_eq!(pc.viewing_players(p(2)), BTreeSet::from([p(1), p(2)]));
    assert_eq!(pc.viewing_players(p(1)), BTreeSet::from([p(1)]));
    assert_eq!(pc.viewing_players(p(3)), BTreeSet::from([p(3)]));
}

#[test]
fn req_turn_003_cr_724_6_concede_and_cr_724_5a_resources() {
    // REQ-TURN-003, CR 724.6: a controlled player may concede, the controller cannot make
    // them; CR 724.5a: costs are paid from the controlled player's resources.
    let pc = controlled_turn();
    assert!(pc.can_concede(p(1)));
    assert!(pc.can_concede(p(2)));
    assert!(!pc.may_concede_for(p(2), p(1)));
    assert!(pc.may_concede_for(p(1), p(1)));
    assert_eq!(pc.cost_resource_owner(p(1)), p(1));
}

// ---------------------------------------------------------------------------------------------
// (D) Restarting the game
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct RHost {
    game: GameState,
    format: FormatConfig,
    libs: BTreeMap<PlayerId, Vec<ObjectId>>,
    hands: BTreeMap<PlayerId, Vec<ObjectId>>,
    exempt: BTreeSet<ObjectId>,
    log: Vec<String>,
}

impl RHost {
    fn new(n: usize) -> RHost {
        let game = GameState::new(77, n, 20).unwrap();
        let mut format = preset(PRESET_IDS[0]).unwrap();
        format.starting_hand_size = 2;
        let libs = game
            .turn_order()
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let base = 100 * (u64::try_from(i).unwrap() + 1);
                (*q, (1..=5).map(|k| ObjectId::new(base + k)).collect())
            })
            .collect();
        RHost {
            game,
            format,
            libs,
            hands: BTreeMap::new(),
            exempt: BTreeSet::from([ObjectId::new(103)]),
            log: Vec::new(),
        }
    }
}

impl Transactional for RHost {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }
    fn priority_player(&self) -> Option<PlayerId> {
        None
    }
    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

impl GameStartHost for RHost {
    fn game(&self) -> &GameState {
        &self.game
    }
    fn game_mut(&mut self) -> &mut GameState {
        &mut self.game
    }
    fn format(&self) -> &FormatConfig {
        &self.format
    }
    fn library(&self, player: PlayerId) -> Vec<ObjectId> {
        self.libs.get(&player).cloned().unwrap_or_default()
    }
    fn set_library(&mut self, player: PlayerId, order: Vec<ObjectId>) -> Result<(), Interrupt> {
        self.libs.insert(player, order);
        Ok(())
    }
    fn hand(&self, player: PlayerId) -> Vec<ObjectId> {
        self.hands.get(&player).cloned().unwrap_or_default()
    }
    fn draw_cards(
        &mut self,
        player: PlayerId,
        count: u32,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        let lib = self.libs.entry(player).or_default();
        let n = usize::try_from(count).unwrap().min(lib.len());
        let drawn: Vec<ObjectId> = lib.drain(..n).collect();
        let hand = self.hands.entry(player).or_default();
        hand.extend(drawn);
        hand.sort_unstable();
        Ok(())
    }
    fn return_hand_to_library(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        let hand = self.hands.remove(&player).unwrap_or_default();
        self.libs.entry(player).or_default().extend(hand);
        Ok(())
    }
    fn put_on_library_bottom(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
    ) -> Result<(), Interrupt> {
        self.hands
            .entry(player)
            .or_default()
            .retain(|c| !cards.contains(c));
        self.libs
            .entry(player)
            .or_default()
            .extend_from_slice(cards);
        Ok(())
    }
    fn set_starting_player(&mut self, player: PlayerId) -> Result<(), Interrupt> {
        self.log.push(format!("starting {}", player.get()));
        Ok(())
    }
    fn set_player_life(&mut self, player: PlayerId, life: i64) -> Result<(), Interrupt> {
        self.game.player_mut(player).unwrap().set_life(life);
        Ok(())
    }
    fn pregame_eligible(&self, _player: PlayerId) -> Vec<ObjectId> {
        Vec::new()
    }
    fn perform_pregame_actions(
        &mut self,
        _player: PlayerId,
        _cards: &[ObjectId],
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        Ok(())
    }
}

impl RestartHost for RHost {
    fn end_game_without_result(&mut self) -> Result<(), Interrupt> {
        self.log.push("end".to_owned());
        Ok(())
    }
    fn restart_exempt_cards(&self) -> BTreeSet<ObjectId> {
        self.exempt.clone()
    }
    fn reset_for_new_game(&mut self, exempt: &BTreeSet<ObjectId>) -> Result<(), Interrupt> {
        self.log.push(format!("reset {}", exempt.len()));
        // Every card goes back to its owner's library, except the exempt ones.
        let owners: Vec<PlayerId> = self.game.turn_order().to_vec();
        for q in owners {
            let mut cards = self.hands.remove(&q).unwrap_or_default();
            cards.extend(self.libs.remove(&q).unwrap_or_default());
            cards.retain(|c| !exempt.contains(c));
            cards.sort_unstable();
            self.libs.insert(q, cards);
        }
        Ok(())
    }
    fn finish_restarting_effect(
        &mut self,
        _choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt> {
        self.log.push("finish".to_owned());
        Ok(())
    }
}

#[test]
fn req_turn_003_cr_728_restart_order_starting_player_and_exempt_cards() {
    // REQ-TURN-003, CR 728.1 (no result, a new game), CR 728.1a (the restarter starts),
    // CR 728.2 (cards back to their owners), CR 728.4 (finish just before the first untap
    // step), CR 728.5 (exempt cards).
    for restarter_index in [0usize, 1] {
        let host = RHost::new(2);
        let restarter = host.game.turn_order()[restarter_index];
        let mut session =
            ChoiceSession::start(host, move |h: &mut RHost, c| restart_game(h, restarter, c))
                .unwrap();
        let mut asked = Vec::new();
        let (h, report) = loop {
            let r = match session.status() {
                Status::Done { state, output } => break (state.clone(), output.clone()),
                Status::Pending(r) => r.clone(),
            };
            asked.push(r.chooser);
            session
                .submit(r.request_id, r.default_on_timeout.clone())
                .unwrap();
        };
        assert_eq!(
            h.log,
            vec![
                "end".to_owned(),
                "reset 1".to_owned(),
                format!("starting {}", restarter.get()),
                "finish".to_owned(),
            ]
        );
        assert_eq!(report.start.starting_player, restarter);
        assert_eq!(
            asked.first(),
            Some(&restarter),
            "the restarter declares first"
        );
        assert_eq!(report.exempt, BTreeSet::from([ObjectId::new(103)]));
        assert_eq!(report.start.opening_hands.len(), 2);
        let all: Vec<ObjectId> = h
            .libs
            .values()
            .chain(h.hands.values())
            .flatten()
            .copied()
            .collect();
        assert_eq!(all.len(), 9);
        assert!(!all.contains(&ObjectId::new(103)));
    }
}

#[test]
fn req_turn_003_cr_728_1a_unknown_restarter_is_an_error() {
    // REQ-TURN-003, CR 728.1a: the restarter must be a player of the game.
    let host = RHost::new(2);
    let err = ChoiceSession::start(host, |h: &mut RHost, c| restart_game(h, p(99), c))
        .err()
        .unwrap();
    assert!(matches!(err, ChoiceError::Procedure(_)));
}

// ---------------------------------------------------------------------------------------------
// Property test
// ---------------------------------------------------------------------------------------------

#[test]
fn req_turn_003_property_extra_turns_and_skips_are_accounted_for() {
    // REQ-TURN-003, CR 500.7, CR 614.10a, CR 800.4k: over seeded random interleavings every
    // extra turn is taken, skipped or dropped exactly once, skip counts only drop by the
    // skips consumed, and extra turns are consumed in `pending_extra_turns` order.
    let order = seats(4);
    for seed in 0..300u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x50_0000 + seed);
        let mut r = |n: u64| rng.next_bounded(n).unwrap();
        let mut s = TurnScheduler::new(order[usize::try_from(r(4)).unwrap()]);
        let mut gone: BTreeSet<PlayerId> = BTreeSet::new();
        let (mut added, mut consumed) = (0usize, 0usize);
        for _ in 0..60 {
            let who = order[usize::try_from(r(4)).unwrap()];
            match r(6) {
                0 => {
                    s.add_extra_turn(who);
                    added += 1;
                }
                1 => {
                    let k = usize::try_from(r(3)).unwrap() + 1;
                    let ps: Vec<PlayerId> = order.iter().copied().take(k).collect();
                    s.add_extra_turns_apnap(&ps);
                    added += k;
                }
                2 => s.add_turn_skip(who, Some(FollowUpId(r(1000)))),
                3 if gone.len() < 3 && r(4) == 0 => {
                    gone.insert(who);
                }
                _ => {
                    let before = s.pending_extra_turns();
                    let skips_before: Vec<usize> =
                        order.iter().map(|&q| s.pending_turn_skips(q)).collect();
                    let in_game = |q: PlayerId| !gone.contains(&q);
                    let Some(t) = s.next_turn(&order, &in_game, false) else {
                        panic!("someone is always in the game");
                    };
                    assert!(in_game(t.player));
                    let after = s.pending_extra_turns();
                    let used = before.len() - after.len();
                    assert_eq!(
                        after[..],
                        before[used..],
                        "pending order is the order taken"
                    );
                    let prefix = &before[..used];
                    // The skipped extra turns appear in the consumed prefix, in order.
                    let mut it = prefix.iter();
                    for k in t.skipped.iter().filter(|k| k.extra) {
                        assert!(it.any(|q| *q == k.player));
                    }
                    if t.extra {
                        assert_eq!(prefix.last(), Some(&t.player));
                    }
                    for (i, &q) in order.iter().enumerate() {
                        let n = t.skipped.iter().filter(|k| k.player == q).count();
                        assert_eq!(s.pending_turn_skips(q) + n, skips_before[i]);
                    }
                    assert_eq!(t.follow_ups.len(), t.skipped.len());
                    consumed += used;
                    drain(&mut s);
                }
            }
        }
        assert_eq!(added, consumed + s.pending_extra_turns().len());
    }
}
