// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The turn structure (REQ-TURN-001, SRS 5).
//!
//! [`heartbeat`] is the priority-and-SBA heartbeat of SRS 5.1: it runs whenever a player would
//! receive priority (CR 117.5, CR 704.3), counts passes, resolves the top object after a full
//! round of passes and ends the step when every player still in the game passes with an empty
//! stack (CR 117.4). It is written against the [`HeartbeatHost`] seam: the stack, resolution,
//! casting and legal-action generation belong to later units.
//!
//! [`steps`] is the turn sequencer of SRS 5.2 (REQ-TURN-002): it runs the phases and steps of a
//! turn in order (CR 500.1), performs the turn-based actions outside combat (untap, draw,
//! main-phase registry actions, cleanup) and the CR 500.4 / CR 500.5 boundary work, and runs the
//! heartbeat in every step that gives priority. It is written against the [`TurnHost`] seam, a
//! supertrait of [`HeartbeatHost`].
//!
//! [`start`] is the game start (REQ-FMT-001, REQ-NET-003): the starting player (CR 103.1), the
//! shuffles (CR 103.3), starting life (CR 103.4), opening hands and the London mulligan
//! (CR 103.5) and the pregame actions (CR 103.6). It is written against the [`GameStartHost`]
//! seam and records the starting player that `TurnHost::starting_player` reports.
//!
//! [`control`] is turn control (REQ-TURN-003, SRS 5.3): the [`TurnScheduler`] of extra and
//! skipped turns, phases and steps (CR 500.7-500.11, CR 614.10), ending the turn (CR 725.1)
//! and the combat phase (CR 725.2), the [`PlayerControl`] model of controlling another player
//! (CR 724), restarting the game (CR 728.1) and [`run_planned_turn`], which runs a planned turn
//! through [`run_segment`].

#![deny(missing_docs)]

pub mod heartbeat;

#[cfg(test)]
mod heartbeat_tests;

pub use heartbeat::{
    ActionOutcome, HeartbeatHost, HeartbeatOutcome, HeartbeatState, MAX_SETTLE_ITERATIONS,
    PassRecord, PassResult, SbaStage, SettleReport, next_player_in_turn_order, players_in_game,
    run_heartbeat, sba_pass_post_batch, settle_before_priority,
};

pub mod steps;

#[cfg(test)]
mod steps_tests;

pub use steps::{
    AsDynTurnHost, ExpiryPoint, MAX_CLEANUP_STEPS, Segment, SegmentReport, TurnBasedAction,
    TurnBasedRegistry, TurnHost, TurnReport, TurnState, UntapOptions, first_turn_draw_skipped,
    run_phase, run_segment, run_turn, segments_of,
};

pub mod start;

#[cfg(test)]
mod start_tests;

pub use start::{GameStartHost, StartOptions, StartReport, mulligan_to_bottom, run_game_start};

pub mod control;

#[cfg(test)]
mod control_tests;

pub use control::{
    ControlError, EndTurnHost, EndTurnReport, FollowUpId, InfoScope, PlanItem, PlannedTurn,
    PlayerControl, RestartHost, RestartReport, RoutedRequest, SkippedTurn, TurnScheduler,
    end_the_combat_phase, end_the_turn, restart_game, run_planned_turn,
};
