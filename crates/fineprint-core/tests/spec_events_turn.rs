// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for TurnBegins / PhaseBegins / StepBegins and `Step::phase` (REQ-EVENT-001;
//! CR 500.1, 500.6, 603.2b). Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_core::events::{EventError, EventState, GameEvent, Outcome, Phase, Step};
use support::*;

// REQ-EVENT-001, CR 500.1: every step belongs to its phase.
#[test]
fn step_phase_mapping() {
    let table = [
        (Step::Untap, Phase::Beginning),
        (Step::Upkeep, Phase::Beginning),
        (Step::Draw, Phase::Beginning),
        (Step::BeginningOfCombat, Phase::Combat),
        (Step::DeclareAttackers, Phase::Combat),
        (Step::DeclareBlockers, Phase::Combat),
        (Step::FirstStrikeDamage, Phase::Combat),
        (Step::CombatDamage, Phase::Combat),
        (Step::EndOfCombat, Phase::Combat),
        (Step::End, Phase::Ending),
        (Step::Cleanup, Phase::Ending),
    ];
    for (step, phase) in table {
        assert_eq!(step.phase(), phase, "{step:?}");
    }
}

// REQ-EVENT-001: a fresh EventState has no phase and no step.
#[test]
fn fresh_event_state_has_no_phase_or_step() {
    let s = EventState::new();
    assert_eq!(s.phase(), None);
    assert_eq!(s.step(), None);
    let d = EventState::default();
    assert_eq!(bytes(&d), bytes(&s));
}

// REQ-EVENT-001, CR 500.6, 603.2b: PhaseBegins sets the phase and clears the step; StepBegins
// sets the step and its phase.
#[test]
fn phase_and_step_begin() {
    let mut w = World::new();
    assert_eq!(
        w.ok(GameEvent::StepBegins { step: Step::Upkeep }).outcome,
        Outcome::Applied
    );
    assert_eq!(w.events.step(), Some(Step::Upkeep));
    assert_eq!(w.events.phase(), Some(Phase::Beginning));

    assert_eq!(
        w.ok(GameEvent::PhaseBegins {
            phase: Phase::PrecombatMain
        })
        .outcome,
        Outcome::Applied
    );
    assert_eq!(w.events.phase(), Some(Phase::PrecombatMain));
    assert_eq!(w.events.step(), None);

    w.ok(GameEvent::StepBegins {
        step: Step::DeclareBlockers,
    });
    assert_eq!(w.events.phase(), Some(Phase::Combat));
    assert_eq!(w.events.step(), Some(Step::DeclareBlockers));

    w.ok(GameEvent::StepBegins {
        step: Step::Cleanup,
    });
    assert_eq!(w.events.phase(), Some(Phase::Ending));
    assert_eq!(w.events.step(), Some(Step::Cleanup));
}

// REQ-EVENT-001, CR 500.1: TurnBegins increments the turn number, sets the active player,
// clears phase and step, and reports the new turn.
#[test]
fn turn_begins() {
    let mut w = World::new();
    assert_eq!(w.turn(), 1);
    w.ok(GameEvent::StepBegins { step: Step::End });
    let done = w.ok(GameEvent::TurnBegins { player: P2 });
    assert_eq!(done.outcome, Outcome::TurnBegan { turn: 2 });
    assert_eq!(w.turn(), 2);
    assert_eq!(w.game.turn().active_player(), Some(P2));
    assert_eq!(w.events.phase(), None);
    assert_eq!(w.events.step(), None);

    let done = w.ok(GameEvent::TurnBegins { player: P1 });
    assert_eq!(done.outcome, Outcome::TurnBegan { turn: 3 });
    assert_eq!(w.game.turn().active_player(), Some(P1));
}

// REQ-EVENT-001: an unknown player is UnknownPlayer and nothing changes.
#[test]
fn turn_begins_unknown_player_fails_cleanly() {
    let mut w = World::new();
    w.ok(GameEvent::PhaseBegins {
        phase: Phase::Ending,
    });
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::TurnBegins { player: NOBODY }),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.snap(), before);
    assert_eq!(w.events.phase(), Some(Phase::Ending));
}

// REQ-EVENT-001: the turn number is incremented with checked arithmetic; an overflow is an
// error that changes nothing.
#[test]
fn turn_number_overflow_fails_cleanly() {
    let mut w = World::new();
    w.game.set_turn_number(u64::MAX);
    let before = w.snap();
    let r = w.exec(GameEvent::TurnBegins { player: P2 });
    assert!(r.is_err(), "{r:?}");
    assert_eq!(w.snap(), before);
}
