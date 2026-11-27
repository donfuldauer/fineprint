// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for ending the turn and ending the combat phase (REQ-TURN-003; CR 725.1,
//! 725.1a-725.1f, 725.2d, 725.2g, 514.3a). Black-box: written from the work-item brief only.

#[path = "spec_turn_control_support.rs"]
mod support;

use fineprint_core::events::{Phase, Step};
use fineprint_core::turn::control::{
    EndTurnReport, FollowUpId, PlanItem, TurnScheduler, end_the_combat_phase, end_the_turn,
    run_planned_turn,
};
use fineprint_core::turn::{ExpiryPoint, Segment, TurnBasedRegistry, TurnReport};
use support::*;

const S2: [fineprint_base::ids::PlayerId; 2] = [P1, P2];

/// The tail of a turn after `end_turn_now` when the current phase is not the ending phase.
fn ended_from(p: Phase) -> Vec<PlanItem> {
    vec![
        ep(p),
        bp(Phase::Ending),
        st(Step::Cleanup),
        ep(Phase::Ending),
    ]
}

/// A scheduler in P1's first turn, the last item yielded being `at`.
fn at(item: PlanItem) -> TurnScheduler {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    advance_to(&mut s, item);
    s
}

// ---------------------------------------------------------------------------------------------
// the scheduler: end_turn_now
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003, CR 725.1d, CR 725.1e: from the upkeep, the game skips straight to the cleanup
/// step; the end step never begins.
#[test]
fn end_turn_now_from_upkeep() {
    let mut s = at(st(Step::Upkeep));
    s.end_turn_now();
    let rest = drain(&mut s);
    assert_eq!(rest, ended_from(Phase::Beginning));
    assert!(!rest.contains(&st(Step::End)));
}

/// REQ-TURN-003, CR 725.1d: from the precombat main phase.
#[test]
fn end_turn_now_from_precombat_main() {
    let mut s = at(seg(Segment::PrecombatMain));
    s.end_turn_now();
    assert_eq!(drain(&mut s), ended_from(Phase::PrecombatMain));
}

/// REQ-TURN-003, CR 725.1d: from declare blockers (combat).
#[test]
fn end_turn_now_from_declare_blockers() {
    let mut s = at(st(Step::DeclareBlockers));
    s.end_turn_now();
    assert_eq!(drain(&mut s), ended_from(Phase::Combat));
}

/// REQ-TURN-003, CR 725.1d: from the end step: the cleanup step follows directly.
#[test]
fn end_turn_now_from_end_step() {
    let mut s = at(st(Step::End));
    s.end_turn_now();
    assert_eq!(drain(&mut s), vec![st(Step::Cleanup), ep(Phase::Ending)]);
}

/// REQ-TURN-003, CR 725.1d, CR 725.1e: at the start of the ending phase, before the end step:
/// the end step never begins.
#[test]
fn end_turn_now_at_ending_phase_start() {
    let mut s = at(bp(Phase::Ending));
    s.end_turn_now();
    assert_eq!(drain(&mut s), vec![st(Step::Cleanup), ep(Phase::Ending)]);
}

/// REQ-TURN-003, CR 725.1d: "If an effect ends the turn during the cleanup step, a new cleanup
/// step begins."
#[test]
fn end_turn_now_from_cleanup_gives_another_cleanup() {
    let mut s = at(st(Step::Cleanup));
    s.end_turn_now();
    assert_eq!(drain(&mut s), vec![st(Step::Cleanup), ep(Phase::Ending)]);
}

/// REQ-TURN-003, CR 725.1d: extra phases and steps still to come are discarded.
#[test]
fn end_turn_now_discards_extra_phases_and_steps() {
    let mut s = at(st(Step::Upkeep));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert!(s.add_extra_step_after(Step::End, Step::End, None));
    assert!(s.add_extra_step_after_phase(Phase::PostcombatMain, Step::Upkeep, None));
    s.end_turn_now();
    assert_eq!(drain(&mut s), ended_from(Phase::Beginning));
}

/// REQ-TURN-003: no-op before the first item and after the turn is over.
#[test]
fn end_turn_now_noop_outside_a_turn() {
    let mut s = TurnScheduler::new(P1);
    next(&mut s, &S2);
    s.end_turn_now();
    assert_eq!(drain(&mut s), normal_plan(false));
    s.end_turn_now();
    assert_eq!(s.next_item(), None);
    assert_eq!(s.current_phase(), None);
    // The next turn is unaffected.
    let (t, items) = take(&mut s, &S2);
    assert_eq!(t.player, P2);
    assert_eq!(items, normal_plan(false));
}

/// REQ-TURN-003, CR 725.1d, CR 614.10: pending extra turns and turn, phase and step skips
/// survive ending the turn (the skipped phases and steps never would begin).
#[test]
fn end_turn_now_keeps_pending_turns_and_skips() {
    let mut s = at(st(Step::Upkeep));
    s.add_extra_turn(P2);
    s.add_turn_skip(P1, Some(FollowUpId(3)));
    s.add_phase_skip(P1, Phase::Combat);
    s.add_step_skip(P1, Step::Draw);
    s.add_step_skip(P1, Step::End);
    s.add_step_skip(P2, Step::Upkeep);
    s.end_turn_now();
    drain(&mut s);
    assert_eq!(s.pending_extra_turns(), vec![P2]);
    assert_eq!(s.pending_turn_skips(P1), 1);
    assert_eq!(s.pending_phase_skips(P1, Phase::Combat), 1);
    assert_eq!(s.pending_step_skips(P1, Step::Draw), 1);
    assert_eq!(s.pending_step_skips(P1, Step::End), 1);
    assert_eq!(s.pending_step_skips(P2, Step::Upkeep), 1);
    // They apply later: P2's extra turn has no upkeep.
    let (t, items) = take(&mut s, &S2);
    assert_eq!((t.player, t.extra), (P2, true));
    assert!(!items.contains(&st(Step::Upkeep)));
}

/// REQ-TURN-003, CR 725.2d: ending the combat phase discards its remaining steps and keeps
/// its end; the next phase follows.
#[test]
fn end_combat_phase_now_in_combat() {
    let mut s = at(st(Step::DeclareAttackers));
    assert!(s.end_combat_phase_now());
    let rest = drain(&mut s);
    assert_eq!(
        rest,
        cat(vec![
            vec![ep(Phase::Combat)],
            phase_items(Phase::PostcombatMain, false),
            phase_items(Phase::Ending, false),
        ])
    );
}

/// REQ-TURN-003, CR 725.2d: an extra combat created earlier still follows the ended one.
#[test]
fn end_combat_phase_now_keeps_later_phases() {
    let mut s = at(st(Step::BeginningOfCombat));
    assert!(s.add_extra_phase(Phase::Combat, Phase::Combat, None));
    assert!(s.end_combat_phase_now());
    let rest = drain(&mut s);
    assert_eq!(
        rest,
        cat(vec![
            vec![ep(Phase::Combat)],
            phase_items(Phase::Combat, false),
            phase_items(Phase::PostcombatMain, false),
            phase_items(Phase::Ending, false),
        ])
    );
}

/// REQ-TURN-003, CR 725.2g: outside a combat phase nothing happens.
#[test]
fn end_combat_phase_now_outside_combat() {
    for item in [
        st(Step::Upkeep),
        seg(Segment::PrecombatMain),
        seg(Segment::PostcombatMain),
        st(Step::End),
    ] {
        let mut s = TurnScheduler::new(P1);
        next(&mut s, &S2);
        let mut all = advance_to(&mut s, item);
        assert!(!s.end_combat_phase_now(), "at {item:?}");
        all.extend(drain(&mut s));
        assert_eq!(all, normal_plan(false));
    }
    let mut s = TurnScheduler::new(P1);
    assert!(!s.end_combat_phase_now());
    next(&mut s, &S2);
    assert!(!s.end_combat_phase_now());
    assert_eq!(drain(&mut s), normal_plan(false));
}

// ---------------------------------------------------------------------------------------------
// end_the_turn with a recording host
// ---------------------------------------------------------------------------------------------

/// A world in P1's turn, the scheduler positioned just after `item`, the host's phase and step
/// set to match.
fn world_at(item: PlanItem, phase: Phase, step: Option<Step>) -> CW {
    let mut w = CW::two();
    next(&mut w.sched, &S2);
    advance_to(&mut w.sched, item);
    w.host.t.phase = (Some(phase), step);
    w
}

fn end_turn_run(w: CW) -> (CW, EndTurnReport) {
    run_cw(w, |w, c| end_the_turn(&mut w.host, &mut w.sched, c)).expect("end the turn")
}

/// REQ-TURN-003, CR 725.1a, 725.1b, 725.1c, 725.1d: the exact order outside combat: cease
/// waiting triggers, exile the stack, SBA passes until none is performed; no trigger is put on
/// the stack, no priority, no removal from combat.
#[test]
fn end_the_turn_order_outside_combat() {
    let mut w = world_at(st(Step::Upkeep), Phase::Beginning, Some(Step::Upkeep));
    w.host.sba_script = [true, true, false].into();
    let (w, rep) = end_turn_run(w);
    assert_eq!(
        rep,
        EndTurnReport {
            phase_ended: Some(Phase::Beginning)
        }
    );
    assert_eq!(
        w.host.calls(),
        vec![
            C::Cease,
            C::ExileStack,
            C::Base(TC::Sba),
            C::Base(TC::Sba),
            C::Base(TC::Sba)
        ]
    );
    let mut s = w.sched.clone();
    assert_eq!(drain(&mut s), ended_from(Phase::Beginning));
}

/// REQ-TURN-003, CR 725.1c: with no state-based action performed, exactly one pass.
#[test]
fn end_the_turn_single_sba_pass() {
    let w = world_at(seg(Segment::PrecombatMain), Phase::PrecombatMain, None);
    let (w, rep) = end_turn_run(w);
    assert_eq!(rep.phase_ended, Some(Phase::PrecombatMain));
    assert_eq!(
        w.host.calls(),
        vec![C::Cease, C::ExileStack, C::Base(TC::Sba)]
    );
}

/// REQ-TURN-003, CR 725.1d: during combat, every creature and planeswalker is removed from
/// combat, after the state-based actions.
#[test]
fn end_the_turn_in_combat_removes_from_combat() {
    let mut w = world_at(
        st(Step::DeclareBlockers),
        Phase::Combat,
        Some(Step::DeclareBlockers),
    );
    w.host.sba_script = [true, false].into();
    let (w, rep) = end_turn_run(w);
    assert_eq!(rep.phase_ended, Some(Phase::Combat));
    assert_eq!(
        w.host.calls(),
        vec![
            C::Cease,
            C::ExileStack,
            C::Base(TC::Sba),
            C::Base(TC::Sba),
            C::RemoveCombat
        ]
    );
    let mut s = w.sched.clone();
    assert_eq!(drain(&mut s), ended_from(Phase::Combat));
}

/// REQ-TURN-003, CR 725.1d: from the end step and from the cleanup step.
#[test]
fn end_the_turn_in_ending_phase() {
    let (w, rep) = end_turn_run(world_at(st(Step::End), Phase::Ending, Some(Step::End)));
    assert_eq!(rep.phase_ended, Some(Phase::Ending));
    assert!(!w.host.calls().contains(&C::RemoveCombat));
    let mut s = w.sched.clone();
    assert_eq!(drain(&mut s), vec![st(Step::Cleanup), ep(Phase::Ending)]);

    let (w, rep) = end_turn_run(world_at(
        st(Step::Cleanup),
        Phase::Ending,
        Some(Step::Cleanup),
    ));
    assert_eq!(rep.phase_ended, Some(Phase::Ending));
    let mut s = w.sched.clone();
    assert_eq!(drain(&mut s), vec![st(Step::Cleanup), ep(Phase::Ending)]);
}

/// REQ-TURN-003: before the first item there is no current phase.
#[test]
fn end_the_turn_before_first_item() {
    let mut w = CW::two();
    next(&mut w.sched, &S2);
    let (w, rep) = end_turn_run(w);
    assert_eq!(rep.phase_ended, None);
    assert!(!w.host.calls().contains(&C::RemoveCombat));
    let mut s = w.sched.clone();
    assert_eq!(drain(&mut s), normal_plan(false));
}

/// REQ-TURN-003, CR 725.1d: pending extra turns and skips survive `end_the_turn`.
#[test]
fn end_the_turn_keeps_pending() {
    let mut w = world_at(st(Step::Draw), Phase::Beginning, Some(Step::Draw));
    w.sched.add_extra_turn(P1);
    w.sched.add_turn_skip(P2, None);
    w.sched.add_phase_skip(P1, Phase::Combat);
    w.sched.add_step_skip(P1, Step::Upkeep);
    let (w, _) = end_turn_run(w);
    assert_eq!(w.sched.pending_extra_turns(), vec![P1]);
    assert_eq!(w.sched.pending_turn_skips(P2), 1);
    assert_eq!(w.sched.pending_phase_skips(P1, Phase::Combat), 1);
    assert_eq!(w.sched.pending_step_skips(P1, Step::Upkeep), 1);
}

// ---------------------------------------------------------------------------------------------
// end_the_turn followed by the rest of the turn through run_planned_turn
// ---------------------------------------------------------------------------------------------

/// Ends P1's turn during the upkeep, then runs the rest of the turn with `run_planned_turn`.
fn end_then_run(trigger_during_process: bool) -> (CW, EndTurnReport, TurnReport, usize) {
    let mut w = CW::two();
    w.host.arm_trigger_on_sba = trigger_during_process;
    let (w, (rep, tr)) = run_cw(w, |w, c| {
        let t = next(&mut w.sched, &S2);
        advance_to(&mut w.sched, st(Step::Upkeep));
        w.host.t.phase = (Some(Phase::Beginning), Some(Step::Upkeep));
        let rep = end_the_turn(&mut w.host, &mut w.sched, c)?;
        let tr = run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &TurnBasedRegistry::standard(),
            &mut w.sched,
            &t,
            c,
        )?;
        Ok((rep, tr))
    })
    .expect("run");
    // The index of the first entry after the end-the-turn process.
    let n = w
        .host
        .calls()
        .iter()
        .position(|c| *c == C::Base(TC::BeginTurn(P1)))
        .expect("the turn ran");
    (w, rep, tr, n)
}

/// REQ-TURN-003, CR 725.1c, CR 725.1f, CR 514.3a: a trigger that triggers during the process is
/// not put on the stack and nobody gets priority during it; the cleanup step stacks it, the
/// active player gets priority, then another cleanup step follows.
#[test]
fn trigger_during_end_the_turn_waits_for_cleanup() {
    let (w, rep, tr, n) = end_then_run(true);
    assert_eq!(rep.phase_ended, Some(Phase::Beginning));
    let calls = w.host.calls();
    let process = &calls[..n];
    assert_eq!(process, &[C::Cease, C::ExileStack, C::Base(TC::Sba)]);
    assert!(
        !process
            .iter()
            .any(|c| matches!(c, C::Base(TC::Place(_) | TC::Prio(_))))
    );
    let rest: Vec<TC> = w.host.base_calls()[1..].to_vec();
    // No step between the upkeep and the cleanup step runs; the end step never begins.
    for s in [
        Step::Upkeep,
        Step::Draw,
        Step::BeginningOfCombat,
        Step::DeclareAttackers,
        Step::DeclareBlockers,
        Step::CombatDamage,
        Step::EndOfCombat,
        Step::End,
    ] {
        assert!(!rest.contains(&TC::BeginStep(s)), "{s:?} began");
    }
    for p in [Phase::PrecombatMain, Phase::Combat, Phase::PostcombatMain] {
        assert!(!rest.contains(&TC::BeginPhase(p)), "{p:?} began");
    }
    assert_eq!(
        rest[..5],
        [
            TC::BeginTurn(P1),
            TC::Expire(ExpiryPoint::EndPhase(Phase::Beginning)),
            TC::EmptyMana,
            TC::BeginPhase(Phase::Ending),
            TC::Expire(ExpiryPoint::BeginPhase(Phase::Ending)),
        ]
    );
    // Two cleanup steps; the active player gets priority in the first.
    assert_eq!(steps::count(&rest, &TC::BeginStep(Step::Cleanup)), 2);
    let first_cleanup = steps::between(
        &rest,
        &TC::BeginStep(Step::Cleanup),
        0,
        &TC::BeginStep(Step::Cleanup),
    );
    assert!(first_cleanup.contains(&TC::Place(vec![P1, P2])));
    let first_prio = first_cleanup
        .iter()
        .position(|c| matches!(c, TC::Prio(_)))
        .expect("priority in the first cleanup step");
    assert_eq!(first_cleanup[first_prio], TC::Prio(P1));
    let second_cleanup = steps::between(
        &rest,
        &TC::BeginStep(Step::Cleanup),
        1,
        &TC::BeginStep(Step::Cleanup),
    );
    assert!(!second_cleanup.iter().any(|c| matches!(c, TC::Prio(_))));
    assert_eq!(
        rest[rest.len() - 2..],
        [
            TC::Expire(ExpiryPoint::EndPhase(Phase::Ending)),
            TC::EmptyMana
        ]
    );
    assert_eq!(tr.active_player, P1);
    let cleanup = tr
        .segments
        .iter()
        .find(|r| r.segment == Segment::Step(Step::Cleanup))
        .expect("cleanup report");
    assert_eq!(cleanup.cleanup_steps, 2);
    assert!(cleanup.heartbeat_ran);
    assert_eq!(
        tr.segments.iter().map(|r| r.segment).collect::<Vec<_>>(),
        vec![Segment::Step(Step::Cleanup)]
    );
    assert!(w.host.t.anomalies.is_empty(), "{:?}", w.host.t.anomalies);
}

/// REQ-TURN-003, CR 725.1f, CR 514.3a: with no trigger during the process, no player gets
/// priority in the (single) cleanup step.
#[test]
fn no_trigger_no_priority_in_cleanup() {
    let (w, rep, tr, n) = end_then_run(false);
    assert_eq!(rep.phase_ended, Some(Phase::Beginning));
    let calls = w.host.calls();
    assert_eq!(&calls[..n], &[C::Cease, C::ExileStack, C::Base(TC::Sba)]);
    let rest: Vec<TC> = w.host.base_calls()[1..].to_vec();
    assert!(!rest.iter().any(|c| matches!(c, TC::Prio(_))));
    assert_eq!(steps::count(&rest, &TC::BeginStep(Step::Cleanup)), 1);
    let expected = steps::cat(vec![
        vec![
            TC::BeginTurn(P1),
            TC::Expire(ExpiryPoint::EndPhase(Phase::Beginning)),
            TC::EmptyMana,
        ],
        steps::phase(
            Phase::Ending,
            steps::step(Step::Cleanup, steps::quiet_cleanup_body(&[P1, P2])),
        ),
    ]);
    assert_eq!(rest, expected);
    assert_eq!(tr.segments.len(), 1);
    assert_eq!(tr.segments[0].cleanup_steps, 1);
    assert!(!tr.segments[0].heartbeat_ran);
}

// ---------------------------------------------------------------------------------------------
// end_the_combat_phase
// ---------------------------------------------------------------------------------------------

/// REQ-TURN-003, CR 725.2d: during combat it ends the combat phase and removes everything from
/// combat.
#[test]
fn end_the_combat_phase_in_combat() {
    let w = world_at(
        st(Step::DeclareAttackers),
        Phase::Combat,
        Some(Step::DeclareAttackers),
    );
    let (w, ended) =
        run_cw(w, |w, _c| end_the_combat_phase(&mut w.host, &mut w.sched)).expect("run");
    assert!(ended);
    assert_eq!(w.host.calls(), vec![C::RemoveCombat]);
    let mut s = w.sched.clone();
    assert_eq!(
        drain(&mut s),
        cat(vec![
            vec![ep(Phase::Combat)],
            phase_items(Phase::PostcombatMain, false),
            phase_items(Phase::Ending, false),
        ])
    );
}

/// REQ-TURN-003, CR 725.2g: outside a combat phase nothing happens and nothing is called.
#[test]
fn end_the_combat_phase_outside_combat() {
    for (item, phase, step) in [
        (st(Step::Upkeep), Phase::Beginning, Some(Step::Upkeep)),
        (seg(Segment::PrecombatMain), Phase::PrecombatMain, None),
        (seg(Segment::PostcombatMain), Phase::PostcombatMain, None),
        (st(Step::End), Phase::Ending, Some(Step::End)),
    ] {
        let w = world_at(item, phase, step);
        let before = w.sched.clone();
        let (w, ended) =
            run_cw(w, |w, _c| end_the_combat_phase(&mut w.host, &mut w.sched)).expect("run");
        assert!(!ended, "at {item:?}");
        assert!(w.host.calls().is_empty(), "at {item:?}");
        let (mut a, mut b) = (before, w.sched.clone());
        assert_eq!(drain(&mut a), drain(&mut b));
    }
}
