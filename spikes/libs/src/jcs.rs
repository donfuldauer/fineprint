// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! RFC 8785 JSON Canonicalization Scheme (REQ-NFR-006) via `serde_json_canonicalizer`.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

/// Parses arbitrary (untrusted) JSON bytes and returns their RFC 8785 canonical form.
///
/// Numbers are parsed by `serde_json` with the `float_roundtrip` feature, so every decimal
/// literal maps to the correctly rounded IEEE-754 double before ECMAScript serialisation.
///
/// RFC 8785 requires I-JSON input (RFC 7493 section 2.3: member names are unique), so any
/// object with a duplicate property name, compared after string decoding and at any nesting
/// depth, is rejected before canonicalisation.
///
/// # Errors
/// Returns an error if the input is not valid JSON or contains a duplicate property name.
pub fn canonicalize(json: &[u8]) -> Result<Vec<u8>, serde_json::Error> {
    reject_duplicate_keys(json)?;
    let value: serde_json::Value = serde_json::from_slice(json)?;
    serde_json_canonicalizer::to_vec(&value)
}

/// Walks the whole document and fails on the first object with a repeated decoded key.
/// serde_json's default recursion limit stays on, so deep nesting is an error, not an overflow.
fn reject_duplicate_keys(json: &[u8]) -> Result<(), serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(json);
    UniqueKeys.deserialize(&mut deserializer)?;
    deserializer.end()
}

/// Visitor that checks every object in a JSON value for duplicate member names.
#[derive(Clone, Copy)]
struct UniqueKeys;

impl<'de> DeserializeSeed<'de> for UniqueKeys {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for UniqueKeys {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    // non-rules crate: REQ-CORE-006 applies to the rules crates only. The value is discarded;
    // only the shape matters here, the `Value` parse handles numbers.
    #[allow(clippy::disallowed_types)]
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(self)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut seen = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if seen.contains(&key) {
                return Err(de::Error::custom(format_args!(
                    "duplicate object key {key:?}"
                )));
            }
            map.next_value_seed(self)?;
            seen.insert(key);
        }
        Ok(())
    }
}
