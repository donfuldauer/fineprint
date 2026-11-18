// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Strict JSON plumbing shared by the `expr`, `text` and `choice` schema modules (REQ-DSL-003,
//! REQ-DSL-011, REQ-DSL-012, REQ-NFR-006).
//!
//! Every `Deserialize` impl of those modules first buffers its input into a
//! [`serde_json::Value`] through [`deserialize_via_json`], which rejects (whatever the
//! deserializer):
//!
//! - nesting deeper than [`MAX_DEPTH`] open arrays and objects (checked before recursing, so no
//!   input can overflow the stack here);
//! - duplicate member names;
//! - any non-integer number (a fraction, an exponent, `-0`), because no position of these schemas
//!   holds anything but an integer (REQ-CORE-006), and integers outside ±[`MAX_SAFE_INTEGER`];
//! - noncharacters in member names and strings (the same rule as [`crate::canonical`], so every
//!   decoded value can be exported again).
//!
//! The buffered value is then converted by hand-written [`FromJson`] impls that reject unknown
//! and missing members, a present `null` on an optional member, and unknown identifier strings.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{self, Serialize, SerializeMap, Serializer};
use serde_json::{Map, Number, Value};

use crate::canonical::{MAX_DEPTH, MAX_SAFE_INTEGER};

/// Result of a conversion from buffered JSON; the error is a human-readable message.
pub(crate) type JResult<T> = Result<T, String>;

/// Conversion from an already buffered (and therefore depth-bounded) JSON value.
pub(crate) trait FromJson: Sized {
    /// Converts `v`, rejecting anything outside the wire contract.
    fn from_json(v: &Value) -> JResult<Self>;
}

/// The body of every schema `Deserialize` impl: buffer strictly, then convert.
///
/// # Errors
/// Any error of the deserializer, of the strict buffering (depth, duplicate member, fractional
/// or unsafe number, noncharacter) or of the conversion.
pub(crate) fn deserialize_via_json<'de, D: Deserializer<'de>, T: FromJson>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = DepthSeed(1).deserialize(deserializer)?;
    T::from_json(&value).map_err(de::Error::custom)
}

/// Implements `Deserialize` through [`deserialize_via_json`] for types that implement
/// [`FromJson`].
macro_rules! deserialize_via_from_json {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl<'de> serde::Deserialize<'de> for $ty {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    $crate::expr::json::deserialize_via_json(d)
                }
            }
        )+
    };
}
pub(crate) use deserialize_via_from_json;

/// Declares a fieldless enum whose wire form is one fixed UPPER_SNAKE string per variant, with
/// `ALL`, `as_str` and `from_wire`, a string `Serialize` and a strict `Deserialize`.
macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// The wire string of this variant.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $text, )+
                }
            }

            /// The variant whose wire string is exactly `text` (case-sensitive), if any.
            #[must_use]
            pub fn from_wire(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == text)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl $crate::expr::json::FromJson for $name {
            fn from_json(v: &serde_json::Value) -> $crate::expr::json::JResult<Self> {
                let text = $crate::expr::json::str_of(v, stringify!($name))?;
                Self::from_wire(text)
                    .ok_or_else(|| format!("unknown {} {:?}", stringify!($name), text))
            }
        }

        $crate::expr::json::deserialize_via_from_json!($name);
    };
}
pub(crate) use wire_enum;

/// Strict buffering seed; the field is the depth a container at this position would have.
#[derive(Clone, Copy)]
struct DepthSeed(usize);

impl DepthSeed {
    /// Fails if a container at this position would exceed [`MAX_DEPTH`].
    fn open<E: de::Error>(self) -> Result<DepthSeed, E> {
        if self.0 > MAX_DEPTH {
            Err(E::custom(format_args!(
                "nesting deeper than {MAX_DEPTH} arrays and objects"
            )))
        } else {
            Ok(DepthSeed(self.0 + 1))
        }
    }
}

impl<'de> DeserializeSeed<'de> for DepthSeed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for DepthSeed {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value with integer numbers only")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        if v.unsigned_abs() > MAX_SAFE_INTEGER.unsigned_abs() {
            return Err(E::custom(format_args!(
                "integer {v} is outside ±{MAX_SAFE_INTEGER}"
            )));
        }
        Ok(Value::Number(Number::from(v)))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        if v > MAX_SAFE_INTEGER.unsigned_abs() {
            return Err(E::custom(format_args!(
                "integer {v} is outside ±{MAX_SAFE_INTEGER}"
            )));
        }
        Ok(Value::Number(Number::from(v)))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        check_chars(v).map_err(E::custom)?;
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        check_chars(&v).map_err(E::custom)?;
        Ok(Value::String(v))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        self.deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let inner = self.open()?;
        let mut out = Vec::new();
        while let Some(item) = seq.next_element_seed(inner)? {
            out.push(item);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let inner = self.open()?;
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            check_chars(&key).map_err(de::Error::custom)?;
            if out.contains_key(&key) {
                return Err(de::Error::custom(format_args!(
                    "duplicate member name {key:?}"
                )));
            }
            let value = map.next_value_seed(inner)?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

/// True for the Unicode noncharacters (U+FDD0..=U+FDEF and every code point ending in FFFE or
/// FFFF), which [`crate::canonical`] refuses.
pub(crate) fn is_noncharacter(c: char) -> bool {
    let cp = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&cp) || cp & 0xFFFE == 0xFFFE
}

/// Rejects a string containing a noncharacter.
fn check_chars(s: &str) -> JResult<()> {
    if s.chars().any(is_noncharacter) {
        Err(format!("string {s:?} contains a Unicode noncharacter"))
    } else {
        Ok(())
    }
}

/// Validity of a free-form string (G3): non-empty and free of noncharacters.
pub(crate) fn check_free_str(s: &str, what: &str) -> JResult<()> {
    if s.is_empty() {
        return Err(format!("{what} must be a non-empty string"));
    }
    check_chars(s)
}

/// Validity of a `BOUND` variable name (G3): `$` then one or more of `A-Z a-z 0-9 _`.
pub(crate) fn check_bound_var(s: &str) -> JResult<()> {
    let ok = s.strip_prefix('$').is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "BOUND var {s:?} must be `$` followed by one or more of A-Z a-z 0-9 _"
        ))
    }
}

/// Validity of an integer value (G4): within ±[`MAX_SAFE_INTEGER`].
pub(crate) fn check_int(i: i64, what: &str) -> JResult<()> {
    if i.unsigned_abs() > MAX_SAFE_INTEGER.unsigned_abs() {
        Err(format!("{what} {i} is outside ±{MAX_SAFE_INTEGER}"))
    } else {
        Ok(())
    }
}

/// Lifts a validation message into a serializer error.
pub(crate) fn ser_check<E: ser::Error>(r: JResult<()>) -> Result<(), E> {
    r.map_err(E::custom)
}

/// The string in `v`.
pub(crate) fn str_of<'a>(v: &'a Value, what: &str) -> JResult<&'a str> {
    v.as_str()
        .ok_or_else(|| format!("{what} must be a string, found {}", kind_of(v)))
}

/// The non-empty free-form string in `v` (G3).
pub(crate) fn free_str(v: &Value, what: &str) -> JResult<String> {
    let s = str_of(v, what)?;
    check_free_str(s, what)?;
    Ok(s.to_owned())
}

/// The boolean in `v`.
pub(crate) fn bool_of(v: &Value, what: &str) -> JResult<bool> {
    v.as_bool()
        .ok_or_else(|| format!("{what} must be a boolean, found {}", kind_of(v)))
}

/// The safe integer in `v` (G4).
pub(crate) fn int_of(v: &Value, what: &str) -> JResult<i64> {
    let Value::Number(n) = v else {
        return Err(format!("{what} must be an integer, found {}", kind_of(v)));
    };
    let Some(i) = n.as_i64() else {
        return Err(format!("{what} must be a safe integer, found {n}"));
    };
    check_int(i, what)?;
    Ok(i)
}

/// The `u32` in `v` (G4, `slot`).
pub(crate) fn u32_of(v: &Value, what: &str) -> JResult<u32> {
    let i = int_of(v, what)?;
    u32::try_from(i).map_err(|_| format!("{what} {i} is outside 0..=4294967295"))
}

/// The array in `v`.
pub(crate) fn array_of<'a>(v: &'a Value, what: &str) -> JResult<&'a [Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{what} must be an array, found {}", kind_of(v)))
}

/// The single `(tag, body)` member of an externally tagged object.
pub(crate) fn single_member<'a>(v: &'a Value, what: &str) -> JResult<(&'a str, &'a Value)> {
    match v.as_object() {
        Some(map) if map.len() == 1 => map
            .iter()
            .next()
            .map(|(k, v)| (k.as_str(), v))
            .ok_or_else(|| format!("{what} must have exactly one member")),
        _ => Err(format!("{what} must be an object with exactly one member")),
    }
}

/// A short name of the JSON type of `v`, for messages.
fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// An object being converted: tracks which members were consumed so that [`Obj::finish`] can
/// reject every unknown one (G2).
pub(crate) struct Obj<'a> {
    /// The members.
    map: &'a Map<String, Value>,
    /// Names already consumed.
    taken: BTreeSet<&'a str>,
    /// What the object is, for messages.
    what: &'static str,
}

impl<'a> Obj<'a> {
    /// Starts converting `v`, which must be an object.
    pub(crate) fn new(v: &'a Value, what: &'static str) -> JResult<Self> {
        let map = v
            .as_object()
            .ok_or_else(|| format!("{what} must be an object, found {}", kind_of(v)))?;
        Ok(Obj {
            map,
            taken: BTreeSet::new(),
            what,
        })
    }

    /// Whether member `key` is present (consumed or not).
    pub(crate) fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// The required member `key`.
    pub(crate) fn req(&mut self, key: &str) -> JResult<&'a Value> {
        match self.map.get_key_value(key) {
            Some((k, v)) => {
                self.taken.insert(k.as_str());
                Ok(v)
            }
            None => Err(format!("{} is missing member {key:?}", self.what)),
        }
    }

    /// The optional member `key`; a present `null` is an error (G2).
    pub(crate) fn opt(&mut self, key: &str) -> JResult<Option<&'a Value>> {
        match self.map.get_key_value(key) {
            Some((_, Value::Null)) => Err(format!(
                "{} member {key:?} must be omitted rather than null",
                self.what
            )),
            Some((k, v)) => {
                self.taken.insert(k.as_str());
                Ok(Some(v))
            }
            None => Ok(None),
        }
    }

    /// Rejects every member that was not consumed.
    pub(crate) fn finish(self) -> JResult<()> {
        match self.map.keys().find(|k| !self.taken.contains(k.as_str())) {
            Some(k) => Err(format!(
                "{} has unknown or misplaced member {k:?}",
                self.what
            )),
            None => Ok(()),
        }
    }
}

/// A value that writes its members into an enclosing JSON object (the flat styles of the wire
/// contract).
pub(crate) trait Members {
    /// Writes this value's members into `m`.
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error>;
}

/// Serialises a [`Members`] value as an object of its own.
pub(crate) struct AsMap<'a, T: ?Sized>(pub(crate) &'a T);

impl<T: Members + ?Sized> Serialize for AsMap<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        self.0.members(&mut m)?;
        m.end()
    }
}

/// Serialises `{tag: body}`.
pub(crate) fn tagged<S: Serializer, B: Serialize + ?Sized>(
    s: S,
    tag: &'static str,
    body: &B,
) -> Result<S::Ok, S::Error> {
    let mut m = s.serialize_map(Some(1))?;
    m.serialize_entry(tag, body)?;
    m.end()
}
