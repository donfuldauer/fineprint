// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The turn structure (REQ-TURN-001, SRS 5).
//!
//! [`heartbeat`] is the priority-and-SBA heartbeat of SRS 5.1: it runs whenever a player would
//! receive priority (CR 117.5, CR 704.3), counts passes, resolves the top object after a full
//! round of passes and ends the step when every player still in the game passes with an empty
//! stack (CR 117.4). It is written against the [`HeartbeatHost`] seam: the stack, resolution,
//! casting and legal-action generation belong to later units. Step and phase sequencing,
//! including the CR 500.5 end-of-step work, belongs to the turn sequencer.

#![deny(missing_docs)]

pub mod heartbeat;

#[cfg(test)]
mod heartbeat_tests;

pub use heartbeat::{
    HeartbeatHost, HeartbeatOutcome, HeartbeatState, MAX_SETTLE_ITERATIONS, PassRecord, PassResult,
    SbaStage, SettleReport, next_player_in_turn_order, players_in_game, run_heartbeat,
    sba_pass_post_batch, settle_before_priority,
};
