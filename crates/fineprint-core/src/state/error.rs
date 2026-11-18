// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the game-state model (REQ-CORE-001).

use core::fmt;

use fineprint_base::ids::PlayerId;

use super::MAX_PLAYERS;

/// An error from building, mutating, serializing or deserializing a [`GameState`]
/// (REQ-CORE-001). A mutator that returns an error leaves the state unchanged.
///
/// [`GameState`]: super::GameState
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StateError {
    /// A game state needs at least one player.
    NoPlayers,
    /// A game state holds at most [`MAX_PLAYERS`] players.
    TooManyPlayers,
    /// An identifier generator has already issued `u64::MAX` (REQ-CORE-002).
    IdExhausted,
    /// The player id names no player of this state.
    UnknownPlayer(PlayerId),
    /// A turn order that is not a permutation of exactly the state's player ids.
    InvalidTurnOrder,
    /// Serialization failed (the message describes why).
    Encode(String),
    /// Deserialization failed: malformed bytes, trailing bytes, wrong version or failed
    /// validation (the message describes why).
    Decode(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::NoPlayers => f.write_str("a game state needs at least one player"),
            StateError::TooManyPlayers => {
                write!(f, "a game state holds at most {MAX_PLAYERS} players")
            }
            StateError::IdExhausted => f.write_str("identifier space exhausted"),
            StateError::UnknownPlayer(id) => write!(f, "unknown player {}", id.get()),
            StateError::InvalidTurnOrder => {
                f.write_str("turn order is not a permutation of the player ids")
            }
            StateError::Encode(msg) => write!(f, "state serialization failed: {msg}"),
            StateError::Decode(msg) => write!(f, "state deserialization failed: {msg}"),
        }
    }
}

impl std::error::Error for StateError {}
