// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests of the first-turn draw skip in planned turns with an added draw step
//! (REQ-TURN-002, REQ-TURN-003; Gate 06-turn R9). Every object is a synthetic id (R5).
//!
//! CR 103.8a: "In a two-player game, the player who plays first skips the draw step (see rule
//! 504, “Draw Step”) of their first turn." CR 103.8c: "In all other multiplayer games, no
//! player skips the draw step of their first turn." CR 504.1: "First, the active player draws
//! a card." CR 500.11: "To skip a step, phase, or turn is to proceed past it as though it
//! didn't exist."
//!
//! The skip covers one draw step of that turn: the first draw step that runs in it. A draw
//! step an effect adds after the normal draw step (CR 500.9) or after the precombat main
//! phase (CR 500.10) runs normally. A draw step added directly before the normal draw step
//! (CR 500.9) takes the skip, and the normal draw step runs instead (docs/DECISIONS.md
//! "Gate 06-turn R9 — First-turn draw skip covers the normal draw step only").

#[path = "spec_turn_steps_support.rs"]
mod support;

use fineprint_core::choice::{ChoiceContext, ChoiceError, ChoiceSession, Interrupt, Status};
use fineprint_core::events::{Phase, Step};
use fineprint_core::format::FirstTurnDrawRule;
use fineprint_core::turn::{
    ExpiryPoint, Segment, TurnBasedRegistry, TurnReport, TurnScheduler, run_planned_turn,
};

use support::*;

/// How the extra draw step is added to the first planned turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Extra {
    /// CR 500.10: a beginning phase holding only a draw step, after the precombat main phase.
    AfterMainPhase,
    /// CR 500.9: a draw step directly after the normal draw step.
    AfterDrawStep,
    /// CR 500.9: a draw step directly before the normal draw step.
    BeforeDrawStep,
}

const EXTRAS: [Extra; 3] = [
    Extra::AfterMainPhase,
    Extra::AfterDrawStep,
    Extra::BeforeDrawStep,
];

/// Runs two planned turns (the starting player P1, then P2) from a fresh scheduler; `extra`
/// adds one draw step to the first of them.
fn two_planned_turns(
    w: &mut World,
    extra: Extra,
    c: &mut ChoiceContext<'_>,
) -> Result<Vec<TurnReport>, Interrupt> {
    let registry = TurnBasedRegistry::standard();
    let mut scheduler = TurnScheduler::new(P1);
    let mut reports = Vec::new();
    for turn in 0..2 {
        let seats = w.host.game.turn_order().to_vec();
        let planned = scheduler
            .next_turn(&seats, &|_| true, false)
            .expect("a player is in the game");
        if turn == 0 {
            let added = match extra {
                Extra::AfterMainPhase => {
                    scheduler.add_extra_step_after_phase(Phase::PrecombatMain, Step::Draw, None)
                }
                Extra::AfterDrawStep => {
                    scheduler.add_extra_step_after(Step::Draw, Step::Draw, None)
                }
                Extra::BeforeDrawStep => {
                    scheduler.add_extra_step_before(Step::Draw, Step::Draw, None)
                }
            };
            assert!(added, "{extra:?}: the extra draw step was added");
        }
        reports.push(run_planned_turn(
            &mut w.host,
            &mut w.ts,
            &registry,
            &mut scheduler,
            &planned,
            c,
        )?);
    }
    Ok(reports)
}

type PlannedFn = Box<
    dyn Fn(
        &mut World,
        &mut ChoiceContext<'_>,
    ) -> Result<Result<Vec<TurnReport>, ChoiceError>, Interrupt>,
>;

/// Drives the two planned turns to the end, passing every priority request.
fn drive_planned(host: TurnFake, extra: Extra) -> (World, Vec<TurnReport>) {
    let f: PlannedFn = Box::new(move |w, c| match two_planned_turns(w, extra, c) {
        Ok(r) => Ok(Ok(r)),
        Err(Interrupt::Pending(p)) => Err(Interrupt::Pending(p)),
        Err(Interrupt::Error(e)) => Ok(Err(e)),
    });
    let mut session = ChoiceSession::start(World::new(host), f).expect("session starts");
    let mut answers = Answers::none();
    let mut asks = 0;
    while let Some(req) = session.pending().cloned() {
        asks += 1;
        assert!(asks < MAX_ASKS, "the session did not terminate");
        let a = answers.answer(&req);
        session
            .submit(req.request_id, a)
            .expect("the answer is accepted");
    }
    match session.status() {
        Status::Done { state, output } => {
            let reports = output.clone().expect("the turns finish cleanly");
            (state.clone(), reports)
        }
        Status::Pending(_) => panic!("unfinished"),
    }
}

/// The heartbeat flags of the draw segments of `report`, in order.
fn draw_segments(report: &TurnReport) -> Vec<bool> {
    report
        .segments
        .iter()
        .filter(|s| s.segment == Segment::Step(Step::Draw))
        .map(|s| s.heartbeat_ran)
        .collect()
}

/// The log of the turn that starts at the `n`-th `BeginTurn` (0-based).
fn turn_log(log: &[TC], n: usize) -> Vec<TC> {
    let starts: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, TC::BeginTurn(_)))
        .map(|(i, _)| i)
        .collect();
    let end = starts.get(n + 1).copied().unwrap_or(log.len());
    log[starts[n]..end].to_vec()
}

fn host(players: usize, rule: FirstTurnDrawRule) -> TurnFake {
    let mut h = TurnFake::new(players);
    h.draw_rule = rule;
    h.starting = Some(P1);
    h
}

/// REQ-TURN-002, REQ-TURN-003; Gate 06-turn R9. CR 103.8a: in a two-player game the starting
/// player's first turn skips one draw step, the first one that runs in it (no begin, no draw,
/// no priority; CR 500.11, F48); the other draw step draws exactly once (CR 504.1) with its
/// priority (CR 504.2). With a draw step added after the normal draw step (CR 500.9) or after
/// the precombat main phase (CR 500.10), the normal draw step takes the skip and the added one
/// draws; with a draw step added directly before the normal draw step (CR 500.9), the added
/// one takes the skip and the normal draw step draws. Either way the first draw segment is
/// skipped and the second draws. The second turn's normal draw step is unaffected.
#[test]
fn added_draw_step_draws_on_the_skipped_first_turn() {
    for extra in EXTRAS {
        let (w, reports) = drive_planned(host(2, FirstTurnDrawRule::CrDefault), extra);
        let log = w.calls();
        let first = turn_log(&log, 0);
        assert_eq!(count(&first, &TC::Draw(P1)), 1, "{extra:?}");
        assert_eq!(count(&first, &TC::BeginStep(Step::Draw)), 1, "{extra:?}");
        assert_eq!(draw_segments(&reports[0]), vec![false, true], "{extra:?}");
        // The draw step that ran gave priority to both players (CR 504.2).
        let begun = first
            .iter()
            .position(|c| *c == TC::BeginStep(Step::Draw))
            .expect("one draw step began");
        assert_eq!(
            first[begun..begun + 2 + 1 + hb(&[P1, P2]).len()],
            cat(vec![
                vec![
                    TC::BeginStep(Step::Draw),
                    TC::Expire(ExpiryPoint::BeginStep(Step::Draw)),
                    TC::Draw(P1),
                ],
                hb(&[P1, P2]),
            ])[..],
            "{extra:?}"
        );
        if extra == Extra::AfterMainPhase {
            // The normal draw step (in the beginning phase, before the main phase) is the
            // skipped one; the added phase's draw step draws.
            let main = first
                .iter()
                .position(|c| *c == TC::BeginPhase(Phase::PrecombatMain))
                .expect("a precombat main phase");
            assert!(begun > main, "{extra:?}");
        }
        let second = turn_log(&log, 1);
        assert_eq!(count(&second, &TC::Draw(P2)), 1, "{extra:?}");
        assert_eq!(count(&second, &TC::BeginStep(Step::Draw)), 1, "{extra:?}");
        assert_eq!(draw_segments(&reports[1]), vec![true], "{extra:?}");
    }
}

/// REQ-TURN-002, REQ-TURN-003; Gate 06-turn R9. CR 103.8c (three players under the CR default)
/// and the `NeverSkip` house rule: nothing is skipped, so both the normal and the added draw
/// step draw on the starting player's first turn; the second turn draws once.
#[test]
fn added_draw_step_without_a_first_turn_skip_draws_twice() {
    for (players, rule) in [
        (2, FirstTurnDrawRule::NeverSkip),
        (3, FirstTurnDrawRule::CrDefault),
    ] {
        for extra in EXTRAS {
            let label = format!("{players} players {rule:?} {extra:?}");
            let (w, reports) = drive_planned(host(players, rule), extra);
            let log = w.calls();
            let first = turn_log(&log, 0);
            assert_eq!(count(&first, &TC::Draw(P1)), 2, "{label}");
            assert_eq!(count(&first, &TC::BeginStep(Step::Draw)), 2, "{label}");
            assert_eq!(draw_segments(&reports[0]), vec![true, true], "{label}");
            let second = turn_log(&log, 1);
            assert_eq!(count(&second, &TC::Draw(P2)), 1, "{label}");
            assert_eq!(draw_segments(&reports[1]), vec![true], "{label}");
        }
    }
}

/// REQ-TURN-002, REQ-CORE-001; Gate 06-turn R9, CR 103.8a: the consumed skip does not leak
/// into the next session over a fresh base, and the same driven turns replay identically.
#[test]
fn planned_first_turn_skip_is_deterministic() {
    for extra in EXTRAS {
        let a = drive_planned(host(2, FirstTurnDrawRule::CrDefault), extra);
        let b = drive_planned(host(2, FirstTurnDrawRule::CrDefault), extra);
        assert_eq!(a.0, b.0, "{extra:?}");
        assert_eq!(a.1, b.1, "{extra:?}");
    }
}
