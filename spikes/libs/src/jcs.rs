// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! RFC 8785 JSON Canonicalization Scheme (REQ-NFR-006) via `serde_json_canonicalizer`.

/// Parses arbitrary (untrusted) JSON bytes and returns their RFC 8785 canonical form.
///
/// Numbers are parsed by `serde_json` with the `float_roundtrip` feature, so every decimal
/// literal maps to the correctly rounded IEEE-754 double before ECMAScript serialisation.
///
/// # Errors
/// Returns an error if the input is not valid JSON.
pub fn canonicalize(json: &[u8]) -> Result<Vec<u8>, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(json)?;
    serde_json_canonicalizer::to_vec(&value)
}
