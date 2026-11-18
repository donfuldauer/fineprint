// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Turn position (REQ-CORE-001). The turn order itself lives on the game state; phases and
//! steps belong to the later `turn` module.

use fineprint_base::ids::PlayerId;

/// The turn position of the game (REQ-CORE-001): the turn number and the active and priority
/// players. A fresh value is pre-game: turn 0, no active player, no priority player.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct TurnInfo {
    turn_number: u64,
    active_player: Option<PlayerId>,
    priority_player: Option<PlayerId>,
}

impl TurnInfo {
    /// The turn number (0 before the first turn).
    #[must_use]
    pub fn turn_number(&self) -> u64 {
        self.turn_number
    }

    /// The active player, if any.
    #[must_use]
    pub fn active_player(&self) -> Option<PlayerId> {
        self.active_player
    }

    /// The player who holds priority, if any.
    #[must_use]
    pub fn priority_player(&self) -> Option<PlayerId> {
        self.priority_player
    }

    /// Builds a turn position from parts already validated by the caller.
    pub(super) fn from_parts(
        turn_number: u64,
        active_player: Option<PlayerId>,
        priority_player: Option<PlayerId>,
    ) -> TurnInfo {
        TurnInfo {
            turn_number,
            active_player,
            priority_player,
        }
    }

    /// Sets the turn number.
    pub(super) fn set_turn_number(&mut self, n: u64) {
        self.turn_number = n;
    }

    /// Sets the active player (validated by the caller).
    pub(super) fn set_active_player(&mut self, p: Option<PlayerId>) {
        self.active_player = p;
    }

    /// Sets the priority player (validated by the caller).
    pub(super) fn set_priority_player(&mut self, p: Option<PlayerId>) {
        self.priority_player = p;
    }
}
