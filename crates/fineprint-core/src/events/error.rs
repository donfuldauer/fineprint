// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of event execution (REQ-EVENT-001).

use core::fmt;

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::memory::MemoryError;
use crate::permanent::PermanentError;
use crate::state::StateError;
use crate::zones::ZoneError;

/// An error from executing an event or a batch (REQ-EVENT-001). On any error the whole call
/// changes nothing: every component of the [`EventContext`](super::EventContext) is restored.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventError {
    /// The object id names no object currently in a zone.
    UnknownObject(ObjectId),
    /// The player id names no player.
    UnknownPlayer(PlayerId),
    /// The object is in a zone but is not a permanent on the battlefield.
    NotOnBattlefield(ObjectId),
    /// A negative amount where only 0 or more is allowed.
    InvalidAmount(i64),
    /// A life total, counter count, marked damage or turn number would leave the 64-bit range.
    Overflow,
    /// The zone container rejected the operation.
    Zone(ZoneError),
    /// The permanent container rejected the operation.
    Permanent(PermanentError),
    /// Last-known-information capture failed.
    Memory(MemoryError),
    /// The game state rejected the operation.
    State(StateError),
    /// A graveyard order (CR 404.3) names an owner with no simultaneous graveyard entry in the
    /// batch, names an owner twice, or is not a permutation of that owner's group of cards
    /// (see [`EventContext::execute_batch_ordered`](super::EventContext::execute_batch_ordered)
    /// and [`GraveyardOrderGroup::order_from_response`](super::GraveyardOrderGroup::order_from_response)).
    InvalidGraveyardOrder {
        /// The owner the rejected order names.
        owner: PlayerId,
    },
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventError::UnknownObject(id) => write!(f, "unknown object {}", id.get()),
            EventError::UnknownPlayer(id) => write!(f, "unknown player {}", id.get()),
            EventError::NotOnBattlefield(id) => {
                write!(
                    f,
                    "object {} is not a permanent on the battlefield",
                    id.get()
                )
            }
            EventError::InvalidAmount(n) => write!(f, "invalid negative amount {n}"),
            EventError::Overflow => f.write_str("arithmetic overflow"),
            EventError::Zone(e) => write!(f, "zone error: {e}"),
            EventError::Permanent(e) => write!(f, "permanent error: {e}"),
            EventError::Memory(e) => write!(f, "memory error: {e}"),
            EventError::State(e) => write!(f, "state error: {e}"),
            EventError::InvalidGraveyardOrder { owner } => {
                write!(f, "invalid graveyard order for player {}", owner.get())
            }
        }
    }
}

impl std::error::Error for EventError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EventError::Zone(e) => Some(e),
            EventError::Permanent(e) => Some(e),
            EventError::Memory(e) => Some(e),
            EventError::State(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ZoneError> for EventError {
    fn from(e: ZoneError) -> EventError {
        EventError::Zone(e)
    }
}

impl From<PermanentError> for EventError {
    fn from(e: PermanentError) -> EventError {
        EventError::Permanent(e)
    }
}

impl From<MemoryError> for EventError {
    fn from(e: MemoryError) -> EventError {
        EventError::Memory(e)
    }
}

impl From<StateError> for EventError {
    fn from(e: StateError) -> EventError {
        EventError::State(e)
    }
}
