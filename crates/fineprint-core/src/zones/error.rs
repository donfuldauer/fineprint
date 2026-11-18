// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the zone container (REQ-ZONE-001, REQ-CORE-002).

use core::fmt;

use fineprint_base::ids::{CardId, ObjectId, PlayerId};

/// An error from a [`Zones`](super::Zones) operation. An operation that returns an error leaves
/// the container and the identifier generator unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ZoneError {
    /// The player id names no player of this container.
    UnknownPlayer(PlayerId),
    /// The object id names no object currently in a zone.
    UnknownObject(ObjectId),
    /// The card is already in a zone or an outside-the-game pool.
    CardAlreadyPresent(CardId),
    /// The object id generator has already issued `u64::MAX` (REQ-CORE-002).
    IdExhausted,
    /// A controller was given for, or set on, an object outside the battlefield and the stack.
    ControllerNotApplicable,
    /// Graveyard reordering is disabled by [`ZonesConfig`](super::ZonesConfig).
    ReorderNotAllowed,
    /// A new graveyard order that is not a permutation of exactly the current contents.
    InvalidOrder,
    /// The card is not in the named outside-the-game pool of that player.
    NoSuchOutsideCard(CardId),
}

impl fmt::Display for ZoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZoneError::UnknownPlayer(id) => write!(f, "unknown player {}", id.get()),
            ZoneError::UnknownObject(id) => write!(f, "unknown object {}", id.get()),
            ZoneError::CardAlreadyPresent(id) => {
                write!(
                    f,
                    "card {} is already in a zone or outside-the-game pool",
                    id.get()
                )
            }
            ZoneError::IdExhausted => f.write_str("identifier space exhausted"),
            ZoneError::ControllerNotApplicable => {
                f.write_str("a controller applies only on the battlefield or the stack")
            }
            ZoneError::ReorderNotAllowed => f.write_str("graveyard reordering is not allowed"),
            ZoneError::InvalidOrder => {
                f.write_str("the new order is not a permutation of the current contents")
            }
            ZoneError::NoSuchOutsideCard(id) => {
                write!(f, "card {} is not in that outside-the-game pool", id.get())
            }
        }
    }
}

impl std::error::Error for ZoneError {}
