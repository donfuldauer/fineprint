// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the knowledge state (REQ-CORE-004).

use core::fmt;

use fineprint_base::ids::{ObjectId, PlayerId};

/// An error from a [`Knowledge`](super::Knowledge) operation. An operation that returns an error
/// leaves the knowledge state unchanged (REQ-CORE-004, R12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KnowledgeError {
    /// The object id names no object currently in a zone.
    UnknownObject(ObjectId),
    /// The player id names no player of the zone container.
    UnknownPlayer(PlayerId),
    /// The object is unknown or is not in the library of `owner`.
    NotInLibrary {
        /// The library's owner.
        owner: PlayerId,
        /// The object that is not in that library.
        object: ObjectId,
    },
    /// The object is in the library of `owner` but not at `index` (0 is the top), or `index` is
    /// past the bottom.
    IndexMismatch {
        /// The library's owner.
        owner: PlayerId,
        /// The claimed index.
        index: usize,
        /// The object that is not at that index.
        object: ObjectId,
    },
    /// The object is not face down (or, for a look permission, not a face-down exiled card).
    NotFaceDown(ObjectId),
}

impl fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KnowledgeError::UnknownObject(id) => write!(f, "unknown object {}", id.get()),
            KnowledgeError::UnknownPlayer(id) => write!(f, "unknown player {}", id.get()),
            KnowledgeError::NotInLibrary { owner, object } => write!(
                f,
                "object {} is not in the library of player {}",
                object.get(),
                owner.get()
            ),
            KnowledgeError::IndexMismatch {
                owner,
                index,
                object,
            } => write!(
                f,
                "object {} is not at index {} of the library of player {}",
                object.get(),
                index,
                owner.get()
            ),
            KnowledgeError::NotFaceDown(id) => {
                write!(f, "object {} is not a face-down card here", id.get())
            }
        }
    }
}

impl std::error::Error for KnowledgeError {}
