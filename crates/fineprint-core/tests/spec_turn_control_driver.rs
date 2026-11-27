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
    // The beginning phase is the untap step only: the upkeep is skipped, and CR 103.8a skips
    // the starting player's first draw step in this two-player game before it begins (F48),
    // though its segment is still reported (no priority).
    let beginning = steps::between(
        &log,
        &TC::BeginPhase(Phase::Beginning),
        0,
        &TC::BeginPhase(Phase::PrecombatMain),
    );
    assert!(beginning.contains(&TC::BeginStep(Step::Untap)));
    assert!(!beginning.contains(&TC::BeginStep(Step::Draw)));
    assert!(!beginning.contains(&TC::Draw(P1)));
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

// ---------------------------------------------------------------------------------------------
// Gate 06-turn: plan identity, follow-ups, the first draw step and the combat decisions
// ---------------------------------------------------------------------------------------------

/// Runs `t` through `run_planned_turn` on the world's own state.
fn planned(
    w: &mut CW,
    t: &fineprint_core::turn::PlannedTurn,
    c: &mut fineprint_core::choice::ChoiceContext<'_>,
) -> Result<TurnReport, fineprint_core::choice::Interrupt> {
    let reg = TurnBasedRegistry::standard();
    run_planned_turn(&mut w.host, &mut w.ts, &reg, &mut w.sched, t, c)
}

/// The combat steps that began, in order.
fn combat_begun(log: &[TC]) -> Vec<Step> {
    log.iter()
        .filter_map(|c| match c {
            TC::BeginStep(s) if s.phase() == Phase::Combat => Some(*s),
            _ => None,
        })
        .collect()
}

/// REQ-TURN-003, CR 614.10: a saved `PlannedTurn` that no longer owns the installed plan is
/// rejected with `ChoiceError::Procedure` before `begin_turn`: an earlier turn of another
/// player, and an earlier turn of the same player (an extra turn follows). No host method is
/// called and the current plan, with a pending draw-step skip, is unchanged (F39).
#[test]
fn stale_planned_turn_is_rejected() {
    use fineprint_core::choice::{ChoiceError, Interrupt};
    for same_player in [false, true] {
        let (w, (err, calls, sched_unchanged)) = run_cw(base(false), move |w, c| {
            let first = w.sched.next_turn(&S2, &everyone, false).expect("turn");
            if same_player {
                w.sched.add_extra_turn(P1);
            }
            drain(&mut w.sched);
            let current = w.sched.next_turn(&S2, &everyone, false).expect("turn");
            assert_eq!(current.player, if same_player { P1 } else { P2 });
            w.sched.add_step_skip(current.player, Step::Draw);
            let before = w.sched.clone();
            w.host.clear();
            let err = planned(w, &first, c).err();
            Ok((err, w.host.calls(), w.sched == before))
        })
        .expect("run");
        assert!(
            matches!(err, Some(Interrupt::Error(ChoiceError::Procedure(_)))),
            "same player {same_player}: {err:?}"
        );
        assert!(calls.is_empty(), "same player {same_player}: {calls:?}");
        assert!(sched_unchanged, "same player {same_player}");
        let active = w.sched.active_player().expect("current turn");
        assert_eq!(w.sched.pending_step_skips(active, Step::Draw), 1);
    }
}

/// REQ-TURN-003, CR 614.10b: the follow-ups of the skipped turns are performed by the host
/// right after `begin_turn` and before the first phase begins, in the order the skip effects
/// were created (F40).
#[test]
fn follow_ups_are_dispatched_before_the_first_phase() {
    use fineprint_core::turn::FollowUpId;
    let (w, _) = run_cw(base(false), |w, c| {
        let first = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        planned(w, &first, c)?;
        // Skips created in this order: P1 (follow-up 9), P2 (4), P1 again (2). P2's next turn
        // is skipped (consuming 4), then P1's (consuming 9, P1's oldest); P2 plays, with the
        // follow-ups in creation order: 9, then 4. P1's second skip (2) waits.
        w.sched.add_turn_skip(P1, Some(FollowUpId(9)));
        w.sched.add_turn_skip(P2, Some(FollowUpId(4)));
        w.sched.add_turn_skip(P1, Some(FollowUpId(2)));
        let t = w.sched.next_turn(&S2, &everyone, false).expect("turn");
        assert_eq!(t.player, P2);
        assert_eq!(t.follow_ups, vec![FollowUpId(9), FollowUpId(4)]);
        assert_eq!(w.sched.pending_turn_skips(P1), 1);
        w.host.clear();
        planned(w, &t, c)
    })
    .expect("run");
    let log = w.host.base_calls();
    assert_eq!(
        log[..4],
        [
            TC::BeginTurn(P2),
            TC::FollowUp(FollowUpId(9)),
            TC::FollowUp(FollowUpId(4)),
            TC::BeginPhase(Phase::Beginning),
        ]
    );
    assert_eq!(
        log.iter().filter(|c| matches!(c, TC::FollowUp(_))).count(),
        2
    );
}

/// REQ-TURN-003, REQ-TURN-002; CR 103.8a, 103.8c: through `run_planned_turn`, the starting
/// player's first draw step in a two-player CR_DEFAULT game never begins; the next turn, a
/// three-player game and NEVER_SKIP keep it (F48).
#[test]
fn first_draw_step_skip_through_run_planned_turn() {
    use fineprint_core::format::FirstTurnDrawRule;
    for (players, rule, draws) in [
        (2usize, FirstTurnDrawRule::CrDefault, [false, true]),
        (3, FirstTurnDrawRule::CrDefault, [true, true]),
        (2, FirstTurnDrawRule::NeverSkip, [true, true]),
    ] {
        let mut t = TurnFake::new(players);
        t.draw_rule = rule;
        let seats: Vec<PlayerId> = (1..=players as u64).map(PlayerId::new).collect();
        let w = CW {
            host: Ctl::new(t),
            ts: fineprint_core::turn::TurnState::new(),
            sched: fineprint_core::turn::TurnScheduler::new(P1),
        };
        let (w, reps) = run_cw(w, move |w, c| {
            let mut out = Vec::new();
            for _ in 0..2 {
                let t = w.sched.next_turn(&seats, &everyone, false).expect("turn");
                out.push(planned(w, &t, c)?);
            }
            Ok(out)
        })
        .expect("run");
        let log = w.host.base_calls();
        for (n, (r, &draw)) in reps.iter().zip(&draws).enumerate() {
            let label = format!("{players} players {rule:?} turn {}", n + 1);
            let turn = steps::between(
                &log,
                &TC::BeginTurn(r.active_player),
                0,
                &TC::BeginPhase(Phase::PrecombatMain),
            );
            assert_eq!(turn.contains(&TC::BeginStep(Step::Draw)), draw, "{label}");
            assert_eq!(turn.contains(&TC::Draw(r.active_player)), draw, "{label}");
            let d = r
                .segments
                .iter()
                .find(|s| s.segment == fineprint_core::turn::Segment::Step(Step::Draw))
                .expect("draw segment reported");
            assert_eq!(d.heartbeat_ran, draw, "{label}");
        }
    }
}

/// REQ-TURN-003, REQ-TURN-002; CR 508.8: through `run_planned_turn`, no attackers after
/// declare-attackers priority skips declare blockers and combat damage, even with a
/// first-strike step planned, and keeps end of combat; an attacker put onto the battlefield
/// during that priority keeps them (F46).
#[test]
fn no_attackers_skips_blockers_and_damage_through_run_planned_turn() {
    for (added, expected) in [
        (
            false,
            vec![
                Step::BeginningOfCombat,
                Step::DeclareAttackers,
                Step::EndOfCombat,
            ],
        ),
        (true, steps::combat_steps(false)),
    ] {
        let mut w = base(false);
        w.host.t.attackers = false;
        w.host.t.resolve_adds_attacker = added;
        w.host.t.combat_stack = vec![Step::DeclareAttackers];
        let (w, _) = run_cw(w, |w, c| {
            let t = w.sched.next_turn(&S2, &everyone, true).expect("turn");
            planned(w, &t, c)
        })
        .expect("run");
        let log = w.host.base_calls();
        assert_eq!(combat_begun(&log), expected, "added {added}");
        assert_eq!(
            steps::count(
                &log,
                &TC::Expire(fineprint_core::turn::ExpiryPoint::EndPhase(Phase::Combat))
            ),
            1
        );
    }
}

/// REQ-TURN-003, REQ-TURN-002; CR 510.4: through `run_planned_turn`, the first-strike step is
/// decided after declare-blockers priority, whatever `next_turn` was told: a creature that
/// gains first strike in that priority adds the step, one that loses it removes the step, and
/// with no change the host's flag decides (F49, F18).
#[test]
fn first_strike_decided_at_the_damage_boundary_through_run_planned_turn() {
    for (start, flips, provisional, expected) in [
        (false, true, false, true),
        (true, true, true, false),
        (false, false, true, false),
        (true, false, false, true),
    ] {
        let mut w = base(start);
        w.host.t.resolve_flips_first_strike = flips;
        w.host.t.combat_stack = vec![Step::DeclareBlockers];
        let (w, reps) = run_cw(w, move |w, c| {
            let t = w
                .sched
                .next_turn(&S2, &everyone, provisional)
                .expect("turn");
            planned(w, &t, c)
        })
        .expect("run");
        let label = format!("start {start}, flips {flips}, provisional {provisional}");
        let log = w.host.base_calls();
        assert_eq!(combat_begun(&log), steps::combat_steps(expected), "{label}");
        assert_eq!(
            *w.host.t.first_strike_reads.borrow(),
            vec![Some(Step::DeclareBlockers)],
            "{label}"
        );
        let combat: Vec<Step> = reps
            .segments
            .iter()
            .filter_map(|r| match r.segment {
                fineprint_core::turn::Segment::Step(s) if s.phase() == Phase::Combat => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(combat, steps::combat_steps(expected), "{label}");
    }
}
