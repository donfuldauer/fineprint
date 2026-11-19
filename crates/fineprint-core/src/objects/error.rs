// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the object-characteristics module (REQ-OBJ-001).

use core::fmt;

/// Why a type line was rejected by [`validate_types`](super::validate_types) (CR 205).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectError {
    /// The named card type is not in the rules vocabulary (CR 205.2a).
    UnknownCardType(String),
    /// The named supertype is not in the rules vocabulary (CR 205.4a).
    UnknownSupertype(String),
    /// The subtype corresponds to none of the object's card types (CR 205.3c, 205.3d).
    SubtypeNotCorrelated {
        /// The offending subtype.
        subtype: String,
    },
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectError::UnknownCardType(name) => write!(f, "unknown card type {name:?}"),
            ObjectError::UnknownSupertype(name) => write!(f, "unknown supertype {name:?}"),
            ObjectError::SubtypeNotCorrelated { subtype } => {
                write!(
                    f,
                    "subtype {subtype:?} corresponds to none of the card types"
                )
            }
        }
    }
}

impl std::error::Error for ObjectError {}
