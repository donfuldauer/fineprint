// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the memory module (REQ-CORE-003, REQ-CORE-007).

use core::fmt;

use fineprint_base::ids::ObjectId;

/// An error from a memory, linked-store or LKI operation (REQ-CORE-003, REQ-CORE-007). An
/// operation that returns an error leaves its inputs unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemoryError {
    /// The object id names no object currently in a zone.
    UnknownObject(ObjectId),
    /// A last-known-information record for this object id already exists; records are never
    /// overwritten (CR 400.7: an object id names one zone residency and is never reused).
    AlreadyCaptured(ObjectId),
    /// The characteristics provider knows no characteristics for this object, so no
    /// last-known information can be captured (CR 608.2h).
    NoCharacteristics(ObjectId),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::UnknownObject(id) => write!(f, "unknown object {}", id.get()),
            MemoryError::AlreadyCaptured(id) => write!(
                f,
                "last known information of object {} was already captured",
                id.get()
            ),
            MemoryError::NoCharacteristics(id) => {
                write!(f, "no characteristics are known for object {}", id.get())
            }
        }
    }
}

impl std::error::Error for MemoryError {}
