// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The canonical binary encoding hook (REQ-CORE-006; consumed by the state hash).

/// Canonical fixed-width little-endian encoding of a value.
///
/// Implementations APPEND to `out` and never clear or read it. Integers are written as
/// fixed-width little-endian bytes (8 bytes for every `u64` identifier and every `i64` number).
/// There is no tag byte and no length prefix: those belong to the state-hash framework.
pub trait CanonicalEncode {
    /// Appends the canonical bytes of `self` to `out`.
    fn encode_canonical(&self, out: &mut Vec<u8>);
}
