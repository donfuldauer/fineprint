// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MessagePack transport encoding (REQ-NFR-006): transport only, never a hash input.
//!
//! Configuration: structs are encoded as maps keyed by field name (`rmp_serde::to_vec_named`)
//! and enums use serde's externally tagged form, which `rmp-serde` writes as a one-entry map
//! `{variant name: payload}` (a bare string for unit variants).

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Encodes `value` as MessagePack with named struct fields.
///
/// # Errors
/// Returns an error if `value` cannot be serialised.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec_named(value)
}

/// Decodes MessagePack bytes (untrusted input) into `T`.
///
/// # Errors
/// Returns an error if the bytes are not a valid encoding of `T`.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}
