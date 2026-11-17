// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Numeric vocabulary of the engine (REQ-CORE-006).
//!
//! Rules paths never use floating point. Every number is a signed 64-bit integer ([`Int`]);
//! half-values use fixed point with scale 2 ([`Fixed2`]). Neither type overloads the arithmetic
//! operators: the overflow policy (`checked_*` or `saturating_*`) is always spelled in the method
//! name, and nothing panics or wraps.

mod fixed2;
mod int;

#[cfg(test)]
pub(crate) mod testkit;

pub use fixed2::Fixed2;
pub use int::{Int, OverflowError};
