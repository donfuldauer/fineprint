// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the game-state model (REQ-CORE-001).

use core::fmt;

use fineprint_base::ids::{EffectId, PlayerId};

use super::MAX_PLAYERS;
use crate::layers::LayerError;

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
    /// The layer system refused the operation (for example
    /// [`LayerError::LayerMismatch`] from registering an effect).
    Layer(LayerError),
    /// An effect in layer 7a that is not from a characteristic-defining ability (SRS 6.2 item 1,
    /// CR 613.4a: layer 7a consists only of characteristic-defining abilities).
    NonCdaInLayer7a,
    /// An effect of a static ability with a source was registered without the link to its
    /// generating ability (CR 613.8a).
    MissingGenerator,
    /// The generating ability's source is not the effect's source.
    GeneratorSourceMismatch,
    /// A generating ability was given for a resolved effect or a static effect without a
    /// source (only a static effect with a source has one).
    UnexpectedGenerator,
    /// The effect id was not issued by this state's effect generator.
    EffectIdNotIssued(EffectId),
    /// The effect id already has a generating-ability link.
    AlreadyLinked(EffectId),
    /// The effect id is a registered continuous effect (its link is made when it is registered).
    EffectRegistered(EffectId),
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
            StateError::Layer(e) => write!(f, "layer system: {e}"),
            StateError::NonCdaInLayer7a => {
                f.write_str("layer 7a holds only characteristic-defining abilities (CR 613.4a)")
            }
            StateError::MissingGenerator => {
                f.write_str("a static effect with a source needs its generating ability")
            }
            StateError::GeneratorSourceMismatch => {
                f.write_str("the generating ability is not on the effect's source")
            }
            StateError::UnexpectedGenerator => {
                f.write_str("only a static effect with a source has a generating ability")
            }
            StateError::EffectIdNotIssued(id) => {
                write!(f, "effect id {} was not issued by this state", id.get())
            }
            StateError::AlreadyLinked(id) => {
                write!(f, "effect {} already has a generating ability", id.get())
            }
            StateError::EffectRegistered(id) => {
                write!(f, "effect {} is a registered continuous effect", id.get())
            }
        }
    }
}

impl std::error::Error for StateError {}
