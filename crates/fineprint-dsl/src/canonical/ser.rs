// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Checked serialisation of a Rust value into a [`Value`] (REQ-NFR-006, REQ-CORE-006).
//!
//! `serde_json::to_value` silently turns NaN and infinities into `null` and keeps only the last
//! of two members with the same name. This serializer builds the same [`Value`] as
//! `serde_json::to_value` for every input it accepts, and instead returns an error for what the
//! checked loader would refuse: non-finite floats, duplicate member names, noncharacters in
//! strings or member names, integers outside ±[`MAX_SAFE_INTEGER`] and nesting deeper than
//! [`MAX_DEPTH`]. The depth is checked before a container is entered, so no input can recurse
//! deeper than [`MAX_DEPTH`] levels here.

use std::fmt;

use serde::ser::{self, Serialize, SerializeMap as _};
use serde_json::{Map, Value};

use super::{CanonicalError, MAX_DEPTH, MAX_SAFE_INTEGER, first_noncharacter};

/// Serialises `value` into a [`Value`] with the checks described in the module documentation.
pub(crate) fn to_checked_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, CanonicalError> {
    value.serialize(Checked::nested(0)).map_err(|e| e.0)
}

/// As [`to_checked_value`], but a member named `member` of the outermost struct or map is left
/// out (REQ-DSL-002, REQ-NFR-006). It is skipped before its value is serialised, so its content
/// is never checked; for duplicate detection it counts as absent. Members of that name nested
/// at any deeper level are serialised and checked as usual.
pub(crate) fn to_checked_value_without_member<T: Serialize + ?Sized>(
    value: &T,
    member: &'static str,
) -> Result<Value, CanonicalError> {
    value
        .serialize(Checked {
            depth: 0,
            skip: Some(member),
        })
        .map_err(|e| e.0)
}

/// The serializer's error: a [`CanonicalError`], so the caller sees the precise kind.
#[derive(Debug)]
struct SerError(CanonicalError);

impl fmt::Display for SerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for SerError {}

impl ser::Error for SerError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerError(CanonicalError::Serialize(msg.to_string()))
    }
}

/// A `serde_json` error raised by a delegated scalar or key conversion.
fn json_err(e: &serde_json::Error) -> SerError {
    SerError(CanonicalError::Serialize(e.to_string()))
}

/// Rejects a string value or member name that contains a noncharacter (RFC 7493 §2.1).
fn check_text(text: &str, what: &str) -> Result<(), SerError> {
    match first_noncharacter(text) {
        Some(c) => Err(SerError(CanonicalError::Serialize(format!(
            "{what} contains the noncharacter U+{:04X}",
            u32::from(c)
        )))),
        None => Ok(()),
    }
}

/// Rejects an integer whose magnitude exceeds [`MAX_SAFE_INTEGER`].
fn check_magnitude(magnitude: u128, text: impl FnOnce() -> String) -> Result<(), SerError> {
    if magnitude > u128::from(MAX_SAFE_INTEGER.unsigned_abs()) {
        Err(SerError(CanonicalError::UnsafeInteger(text())))
    } else {
        Ok(())
    }
}

/// The JSON object `{variant: value}` of serde's externally tagged enum representation.
fn tagged(variant: &'static str, value: Value) -> Result<Value, SerError> {
    check_text(variant, "member name")?;
    let mut map = Map::new();
    map.insert(variant.to_owned(), value);
    Ok(Value::Object(map))
}

/// The serializer. `depth` is the number of arrays and objects already open around the value
/// being serialised (0 at the top of the document). `skip` names a member to leave out of the
/// outermost struct or map; it is only ever set at depth 0 and never passed to a nested value.
#[derive(Clone, Copy)]
struct Checked {
    depth: usize,
    skip: Option<&'static str>,
}

impl Checked {
    /// A serializer for a value inside `depth` open containers, with no member skipped.
    fn nested(depth: usize) -> Self {
        Checked { depth, skip: None }
    }

    /// The depth of a container opened here, or an error if it would exceed [`MAX_DEPTH`].
    fn open(self) -> Result<usize, SerError> {
        let depth = self.depth + 1;
        if depth > MAX_DEPTH {
            Err(SerError(CanonicalError::Serialize(format!(
                "nesting depth exceeds {MAX_DEPTH}"
            ))))
        } else {
            Ok(depth)
        }
    }
}

/// Array under construction; `variant` wraps it as `{variant: [...]}` on `end`.
struct SeqBuilder {
    depth: usize,
    items: Vec<Value>,
    variant: Option<&'static str>,
}

impl SeqBuilder {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.items
            .push(value.serialize(Checked::nested(self.depth))?);
        Ok(())
    }

    fn finish(self) -> Result<Value, SerError> {
        let array = Value::Array(self.items);
        match self.variant {
            Some(variant) => tagged(variant, array),
            None => Ok(array),
        }
    }
}

/// Object under construction; `variant` wraps it as `{variant: {...}}` on `end`.
struct MapBuilder {
    depth: usize,
    map: Map<String, Value>,
    next_key: Option<String>,
    variant: Option<&'static str>,
    /// Member name to leave out of this object (set only for the outermost one).
    skip: Option<&'static str>,
    /// The key just given to `serialize_key` was `skip`, so the next value is discarded unseen.
    skipping: bool,
}

impl MapBuilder {
    /// Whether the member `key` is to be left out of this object.
    fn skips(&self, key: &str) -> bool {
        self.skip == Some(key)
    }

    /// Inserts a member, rejecting a name already present (compared after decoding; the map is
    /// ordered, REQ-CORE-006).
    fn insert(&mut self, key: String, value: Value) -> Result<(), SerError> {
        check_text(&key, "member name")?;
        if self.map.contains_key(&key) {
            return Err(SerError(CanonicalError::DuplicateKey(key)));
        }
        self.map.insert(key, value);
        Ok(())
    }

    fn finish(self) -> Result<Value, SerError> {
        let object = Value::Object(self.map);
        match self.variant {
            Some(variant) => tagged(variant, object),
            None => Ok(object),
        }
    }
}

/// The member name `serde_json` would use for `key` (strings, chars, integers, bools and
/// newtype or unit-variant wrappers of those), obtained from `serde_json` itself so the
/// stringification is identical. Any other key is an error.
fn key_string<K: Serialize + ?Sized>(key: &K) -> Result<String, SerError> {
    let mut one = ser::Serializer::serialize_map(serde_json::value::Serializer, Some(1))
        .map_err(|e| json_err(&e))?;
    one.serialize_key(key).map_err(|e| json_err(&e))?;
    one.serialize_value(&()).map_err(|e| json_err(&e))?;
    match one.end().map_err(|e| json_err(&e))? {
        Value::Object(map) => map.into_iter().next().map(|(k, _)| k),
        _ => None,
    }
    .ok_or_else(|| ser::Error::custom("map key did not produce a member name"))
}

impl ser::Serializer for Checked {
    type Ok = Value;
    type Error = SerError;
    type SerializeSeq = SeqBuilder;
    type SerializeTuple = SeqBuilder;
    type SerializeTupleStruct = SeqBuilder;
    type SerializeTupleVariant = SeqBuilder;
    type SerializeMap = MapBuilder;
    type SerializeStruct = MapBuilder;
    type SerializeStructVariant = MapBuilder;

    fn serialize_bool(self, v: bool) -> Result<Value, SerError> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, SerError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, SerError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, SerError> {
        self.serialize_i64(i64::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, SerError> {
        check_magnitude(u128::from(v.unsigned_abs()), || v.to_string())?;
        Ok(Value::from(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, SerError> {
        check_magnitude(v.unsigned_abs(), || v.to_string())?;
        ser::Serializer::serialize_i128(serde_json::value::Serializer, v).map_err(|e| json_err(&e))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, SerError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, SerError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, SerError> {
        self.serialize_u64(u64::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, SerError> {
        check_magnitude(u128::from(v), || v.to_string())?;
        Ok(Value::from(v))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, SerError> {
        check_magnitude(v, || v.to_string())?;
        ser::Serializer::serialize_u128(serde_json::value::Serializer, v).map_err(|e| json_err(&e))
    }

    // REQ-CORE-006 bans floats in rules arithmetic; this is the boundary that rejects
    // non-finite values and otherwise hands the float to serde_json unchanged.
    #[allow(clippy::disallowed_types)]
    fn serialize_f32(self, v: f32) -> Result<Value, SerError> {
        if !v.is_finite() {
            return Err(SerError(CanonicalError::Serialize(format!(
                "non-finite number {v} has no JSON form"
            ))));
        }
        ser::Serializer::serialize_f32(serde_json::value::Serializer, v).map_err(|e| json_err(&e))
    }

    // REQ-CORE-006 bans floats in rules arithmetic; this is the boundary that rejects
    // non-finite values and otherwise hands the float to serde_json unchanged.
    #[allow(clippy::disallowed_types)]
    fn serialize_f64(self, v: f64) -> Result<Value, SerError> {
        if !v.is_finite() {
            return Err(SerError(CanonicalError::Serialize(format!(
                "non-finite number {v} has no JSON form"
            ))));
        }
        ser::Serializer::serialize_f64(serde_json::value::Serializer, v).map_err(|e| json_err(&e))
    }

    fn serialize_char(self, v: char) -> Result<Value, SerError> {
        self.serialize_str(v.encode_utf8(&mut [0u8; 4]))
    }

    fn serialize_str(self, v: &str) -> Result<Value, SerError> {
        check_text(v, "string")?;
        Ok(Value::String(v.to_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, SerError> {
        self.open()?;
        Ok(Value::Array(v.iter().map(|&b| Value::from(b)).collect()))
    }

    fn serialize_none(self) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, SerError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, SerError> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, SerError> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, SerError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, SerError> {
        let depth = self.open()?;
        tagged(variant, value.serialize(Checked::nested(depth))?)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqBuilder, SerError> {
        Ok(SeqBuilder {
            depth: self.open()?,
            items: Vec::with_capacity(len.unwrap_or(0).min(4096)),
            variant: None,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqBuilder, SerError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, SerError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, SerError> {
        let outer = self.open()?;
        let mut seq = Checked::nested(outer).serialize_seq(Some(len))?;
        seq.variant = Some(variant);
        Ok(seq)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapBuilder, SerError> {
        Ok(MapBuilder {
            depth: self.open()?,
            map: Map::new(),
            next_key: None,
            variant: None,
            skip: self.skip,
            skipping: false,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<MapBuilder, SerError> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<MapBuilder, SerError> {
        let outer = self.open()?;
        let mut map = Checked::nested(outer).serialize_map(Some(len))?;
        map.variant = Some(variant);
        Ok(map)
    }
}

impl ser::SerializeSeq for SeqBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeTuple for SeqBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for SeqBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeTupleVariant for SeqBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeMap for MapBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SerError> {
        let key = key_string(key)?;
        if self.skips(&key) {
            self.next_key = None;
            self.skipping = true;
        } else {
            self.next_key = Some(key);
            self.skipping = false;
        }
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        if std::mem::take(&mut self.skipping) {
            return Ok(());
        }
        let key = self
            .next_key
            .take()
            .ok_or_else(|| ser::Error::custom("map value without a key"))?;
        let value = value.serialize(Checked::nested(self.depth))?;
        self.insert(key, value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeStruct for MapBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        if self.skips(key) {
            return Ok(());
        }
        let value = value.serialize(Checked::nested(self.depth))?;
        self.insert(key.to_owned(), value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}

impl ser::SerializeStructVariant for MapBuilder {
    type Ok = Value;
    type Error = SerError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        let value = value.serialize(Checked::nested(self.depth))?;
        self.insert(key.to_owned(), value)
    }

    fn end(self) -> Result<Value, SerError> {
        self.finish()
    }
}
