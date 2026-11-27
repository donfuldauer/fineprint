// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the planned-turn driver `run_planned_turn` (REQ-TURN-003 with
//! REQ-TURN-002; CR 500.8, 500.11, 614.10). Black-box: written from the work-item brief only.

#[path = "spec_turn_control_support.rs"]
mod support;

use fineprint_base::ids::PlayerId;
use fineprint_core::events::{Phase, Step};
use fineprint_core::turn::control::run_planned_turn;
use fineprint_core::turn::{TurnBasedRegistry, TurnReport, run_turn};
use support::*;

const S2: [PlayerId; 2] = [P1, P2];

fn base(first_strike: bool) -> CW {
    let mut t = TurnFake::new(2);
    t.first_strike = first_strike;
    CW {
        host: Ctl::new(t),
        ts: fineprint_core::turn::TurnState::new(),
        sched: fineprint_core::turn::TurnScheduler::new(P1),
    }
}

/// `run_turn` for each player in `players`.
fn with_run_turn(w: CW, players: Vec<PlayerId>) -> (CW, Vec<TurnReport>) {
    run_cw(w, move |w, c| {
        let reg = TurnBasedRegistry::standard();
        let mut out = Vec::new();
        for &p in &players {
            out.push(run_turn(&mut w.host, &mut w.ts, &reg, p, c)?);
        }
        Ok(out)
    })
    .expect("run_turn")
}

/// `turns` turns through the scheduler and `run_planned_turn`.
fn with_planned(w: CW, turns: usize) -> (CW, Vec<TurnReport>) {
    run_cw(w, move |w, c| {
        let reg = TurnBasedRegistry::standard();
        let mut out = Vec::new();
        for _ in 0..turns {
            let fs = w.host.t.first_strike;
            let t = w
                .sched
                .next_turn(&S2, &everyone, fs)
                .expect("a player in game");
            out.push(run_planned_turn(
                &mut w.host,
                &mut w.ts,
                &reg,
                &mut w.sched,
                &t,
                c,
            )?);
        }
        Ok(out)
    })
    .expect("run_planned_turn")
}

/// REQ-TURN-003, REQ-TURN-002: over the normal plan of one turn `run_planned_turn` makes
/// exactly the host calls `run_turn` makes, with the same segment reports (with and without a
/// first-strike damage step).
#[test]
fn planned_turn_equals_run_turn() {
    for fs in [false, true] {
        let (a, ra) = with_run_turn(base(fs), vec![P1]);
        let (b, rb) = with_planned(base(fs), 1);
        assert_eq!(rb, ra, "first strike {fs}");
        assert_eq!(
            b.host.base_calls(),
            a.host.base_calls(),
            "first strike {fs}"
        );
        assert_eq!(rb[0].active_player, P1);
    }
}

/// REQ-TURN-003, REQ-TURN-002: the same turn after turn: `run_planned_turn` leaves the
/// sequencer state as `run_turn` does, so the next turn's untap step sees the previous active
/// player exactly as with `run_turn` (the day/night check argument).
#[test]
fn planned_turns_equal_run_turns_consecutively() {
    for fs in [false, true] {
        let (a, ra) = with_run_turn(base(fs), vec![P1, P2, P1]);
        let (b, rb) = with_planned(base(fs), 3);
        assert_eq!(rb, ra, "first strike {fs}");
        assert_eq!(
            b.host.base_calls(),
            a.host.base_calls(),
            "first strike {fs}"
        );
        assert_eq!(b.ts, a.ts);
        assert_eq!(b.ts.previous_active_player(), Some(P1));
        assert_eq!(rb[1].active_player, P2);
    }
}

/// REQ-TURN-003, REQ-TURN-002: the same with a cleanup step that gives priority (a second
/// cleanup step) and a drawn card.
#[test]
fn planned_turn_equals_run_turn_with_cleanup_work() {
    let mk = || {
        let mut w = base(false);
        w.host.t = TurnFake::new(2).with_cleanup_work(vec![Some(steps::Work::Trigger), None]);
        w
    };
    let (a, ra) = with_run_turn(mk(), vec![P1]);
    let (b, rb) = with_planned(mk(), 1);
    assert_eq!(rb, ra);
    assert_eq!(b.host.base_calls(), a.host.base_calls());
}

/// REQ-TURN-003, CR 500.8, CR 500.11, CR 614.10: a plan with an extra combat and a skipped
/// upkeep: the host sees the extra phase begin and its steps, and no upkeep.
#[test]
fn planned_turn_with_extra_combat_and_skipped_upkeep() {
    let (w, reps) = run_cw(base(false), |w, c| {
        w.sched.add_step_skip(P1, Step::Upkeep);
        let t = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        assert!(
            w.sched
                .add_extra_phase(Phase::Combat, Phase::Combat, Some(P1))
        );
        let reg = TurnBasedRegistry::standard();
        let r = run_planned_turn(&mut w.host, &mut w.ts, &reg, &mut w.sched, &t, c)?;
        Ok(vec![r])
    })
    .expect("run");
    let log = w.host.base_calls();
    assert!(!log.contains(&TC::BeginStep(Step::Upkeep)));
    assert_eq!(steps::count(&log, &TC::BeginPhase(Phase::Combat)), 2);
    for s in steps::combat_steps(false) {
        assert_eq!(steps::count(&log, &TC::BeginStep(s)), 2, "{s:?}");
        assert_eq!(steps::count(&log, &TC::Combat(s)), 2, "{s:?}");
    }
    assert_eq!(
        steps::count(
            &log,
            &TC::Expire(fineprint_core::turn::ExpiryPoint::EndPhase(Phase::Combat))
        ),
        2
    );
    // The extra combat comes directly after the first one.
    let second = steps::between(
        &log,
        &TC::BeginPhase(Phase::Combat),
        1,
        &TC::BeginPhase(Phase::PostcombatMain),
    );
    let expected = steps::phase(
        Phase::Combat,
        steps::cat(
            steps::combat_steps(false)
                .into_iter()
                .map(|s| steps::step(s, steps::cat(vec![vec![TC::Combat(s)], steps::hb(&S2)])))
                .collect(),
        ),
    );
    assert_eq!(second, expected);
    // The beginning phase is the untap and draw steps only.
    let beginning = steps::between(
        &log,
        &TC::BeginPhase(Phase::Beginning),
        0,
        &TC::BeginPhase(Phase::PrecombatMain),
    );
    assert!(beginning.contains(&TC::BeginStep(Step::Untap)));
    assert!(beginning.contains(&TC::BeginStep(Step::Draw)));
    let r = &reps[0];
    assert_eq!(r.active_player, P1);
    let segs: Vec<_> = r.segments.iter().map(|s| s.segment).collect();
    let mut expected_segs = steps::turn_segments(false);
    expected_segs.retain(|s| *s != fineprint_core::turn::Segment::Step(Step::Upkeep));
    let extra: Vec<_> = steps::combat_steps(false)
        .into_iter()
        .map(fineprint_core::turn::Segment::Step)
        .collect();
    let at = expected_segs
        .iter()
        .position(|s| *s == fineprint_core::turn::Segment::PostcombatMain)
        .expect("postcombat main");
    expected_segs.splice(at..at, extra);
    assert_eq!(segs, expected_segs);
}

/// REQ-TURN-003: `TurnReport.active_player` is the planned turn's player, also for an extra
/// turn.
#[test]
fn report_active_player_is_turn_player() {
    let (_, reps) = run_cw(base(false), |w, c| {
        let reg = TurnBasedRegistry::standard();
        let mut out = Vec::new();
        let t = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        assert_eq!(t.player, P1);
        w.sched.add_extra_turn(P2);
        out.push(run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &reg,
            &mut w.sched,
            &t,
            c,
        )?);
        let t = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        assert!(t.extra);
        out.push(run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &reg,
            &mut w.sched,
            &t,
            c,
        )?);
        Ok(out)
    })
    .expect("run");
    assert_eq!(reps[0].active_player, P1);
    assert_eq!(reps[1].active_player, P2);
}

/// REQ-TURN-003: the scheduler is consulted one item at a time: a change made between items
/// affects the next item (an extra step created after part of the turn ran).
#[test]
fn scheduler_changes_between_items_apply() {
    let (w, _) = run_cw(base(false), |w, c| {
        let reg = TurnBasedRegistry::standard();
        let t = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        // Pull the first two items (the beginning phase and its untap step) by hand ...
        advance_to(&mut w.sched, st(Step::Untap));
        // ... then add an extra upkeep and let the driver run the rest.
        assert!(
            w.sched
                .add_extra_step_after(Step::Upkeep, Step::Upkeep, None)
        );
        run_planned_turn(&mut w.host, &mut w.ts, &reg, &mut w.sched, &t, c)
    })
    .expect("run");
    let log = w.host.base_calls();
    assert_eq!(steps::count(&log, &TC::BeginStep(Step::Upkeep)), 2);
    assert!(!log.contains(&TC::BeginStep(Step::Untap)));
    assert!(!log.contains(&TC::BeginPhase(Phase::Beginning)));
    assert_eq!(log[0], TC::BeginTurn(P1));
    assert_eq!(log[1], TC::BeginStep(Step::Upkeep));
}
