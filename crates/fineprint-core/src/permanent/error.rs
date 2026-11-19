// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the permanent container (REQ-ZONE-001 item 3).

use core::fmt;

use fineprint_base::ids::ObjectId;

/// An error from a [`Permanents`](super::Permanents) operation. An operation that returns an
/// error leaves the container unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PermanentError {
    /// The object id names no permanent of this container.
    UnknownObject(ObjectId),
    /// The object id already names a permanent of this container.
    AlreadyPresent(ObjectId),
    /// A permanent was to be attached to itself.
    SelfAttach(ObjectId),
    /// Attaching `object` to `target` would close an attachment cycle.
    AttachmentCycle {
        /// The permanent being attached.
        object: ObjectId,
        /// The permanent it was to be attached to.
        target: ObjectId,
    },
}

impl fmt::Display for PermanentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PermanentError::UnknownObject(id) => write!(f, "unknown permanent {}", id.get()),
            PermanentError::AlreadyPresent(id) => {
                write!(f, "permanent {} is already on the battlefield", id.get())
            }
            PermanentError::SelfAttach(id) => {
                write!(f, "permanent {} cannot be attached to itself", id.get())
            }
            PermanentError::AttachmentCycle { object, target } => write!(
                f,
                "attaching permanent {} to permanent {} would form an attachment cycle",
                object.get(),
                target.get()
            ),
        }
    }
}

impl core::error::Error for PermanentError {}
