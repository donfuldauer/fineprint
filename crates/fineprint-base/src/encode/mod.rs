// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Canonical binary encoding toolkit (REQ-NFR-006, consumed by the Merkle state hash of
//! REQ-LOOP-001).
//!
//! Every value is written as a deterministic, injective byte string: distinct values of the same
//! type always give distinct bytes. The layouts are:
//!
//! | Type | Bytes |
//! |---|---|
//! | `u8 u16 u32 u64 i8 i16 i32 i64` | fixed width, little-endian (`to_le_bytes`) |
//! | `bool` | one byte, `0x00` or `0x01` |
//! | `str`, `String` | [`put_bytes`](crate::encode::put_bytes) of the UTF-8 bytes |
//! | `[T]`, `Vec<T>` | [`put_len`](crate::encode::put_len) of the element count, then each element in order |
//! | `[T; N]` | each element in order, no length (N is part of the type) |
//! | `Option<T>` | `0x00` for `None`; `0x01` then the value for `Some` |
//! | `(A, B)`, `(A, B, C)` | the fields in order, no prefix |
//! | `BTreeMap<K, V>`, `BTreeSet<T>` | [`put_len`](crate::encode::put_len) of the entry count, then the entries in ascending key order (key, then value) |
//! | `&T` | the bytes of `T` |
//!
//! There is deliberately no implementation for `usize`/`isize` (platform-dependent width) or for
//! floating point (REQ-CORE-006). Only ordered containers are encoded; there is no hash-map
//! iteration anywhere. Structs list their fields in a fixed order with
//! [`impl_canonical_struct!`](crate::impl_canonical_struct).

use std::collections::{BTreeMap, BTreeSet};

/// The canonical encoding trait; the same trait as [`crate::ids::CanonicalEncode`].
pub use crate::ids::CanonicalEncode;
/// Implements [`CanonicalEncode`] for a struct by encoding the listed fields in order.
pub use crate::impl_canonical_struct;

#[cfg(test)]
mod tests;

/// The 4-byte marker that announces an 8-byte length in [`put_len`].
const LEN_ESCAPE: u32 = u32::MAX;

/// Returns the canonical encoding of `value` as a new byte vector.
#[must_use]
pub fn encode_to_vec<T: CanonicalEncode + ?Sized>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    value.encode_canonical(&mut out);
    out
}

/// Appends an injective length prefix for `len` to `out`.
///
/// A length below `u32::MAX` is written as a `u32` little-endian (4 bytes). Any other length
/// (including `u32::MAX` itself) is written as the escape `0xFFFF_FFFF` followed by the length as
/// a `u64` little-endian (12 bytes in all). Every `usize` therefore has exactly one encoding and
/// no two lengths share one.
pub fn put_len(out: &mut Vec<u8>, len: usize) {
    match u32::try_from(len) {
        Ok(short) if short != LEN_ESCAPE => out.extend_from_slice(&short.to_le_bytes()),
        _ => {
            out.extend_from_slice(&LEN_ESCAPE.to_le_bytes());
            // `usize` is at most 64 bits on every supported target, so this is lossless.
            out.extend_from_slice(&(len as u64).to_le_bytes());
        }
    }
}

/// Appends [`put_len`] of `bytes.len()` followed by `bytes` to `out`.
pub fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_len(out, bytes.len());
    out.extend_from_slice(bytes);
}

/// Appends a one-byte enum discriminant `tag` to `out`.
pub fn put_variant(out: &mut Vec<u8>, tag: u8) {
    out.push(tag);
}

/// Implements [`CanonicalEncode`](crate::encode::CanonicalEncode) for a struct by encoding the
/// listed fields, in the listed order, each through its own `encode_canonical`, with no prefix.
///
/// ```
/// struct Sample {
///     first: u8,
///     second: u16,
/// }
/// fineprint_base::impl_canonical_struct!(Sample { first, second });
///
/// let bytes = fineprint_base::encode::encode_to_vec(&Sample { first: 1, second: 2 });
/// assert_eq!(bytes, vec![1, 2, 0]);
/// ```
#[macro_export]
macro_rules! impl_canonical_struct {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::encode::CanonicalEncode for $ty {
            fn encode_canonical(&self, out: &mut ::std::vec::Vec<u8>) {
                let _ = &out;
                $( $crate::encode::CanonicalEncode::encode_canonical(&self.$field, out); )*
            }
        }
    };
}

macro_rules! impl_le_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl CanonicalEncode for $t {
                /// Fixed-width little-endian bytes.
                fn encode_canonical(&self, out: &mut Vec<u8>) {
                    out.extend_from_slice(&self.to_le_bytes());
                }
            }
        )*
    };
}

impl_le_int!(u8, u16, u32, u64, i8, i16, i32, i64);

impl CanonicalEncode for bool {
    /// One byte: `0x00` for `false`, `0x01` for `true`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.push(u8::from(*self));
    }
}

impl CanonicalEncode for str {
    /// [`put_bytes`] of the UTF-8 bytes.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_bytes(out, self.as_bytes());
    }
}

impl CanonicalEncode for String {
    /// Same as `str`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.as_str().encode_canonical(out);
    }
}

impl<T: CanonicalEncode> CanonicalEncode for [T] {
    /// [`put_len`] of the element count, then each element in order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        for item in self {
            item.encode_canonical(out);
        }
    }
}

impl<T: CanonicalEncode> CanonicalEncode for Vec<T> {
    /// Same as `[T]`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.as_slice().encode_canonical(out);
    }
}

impl<T: CanonicalEncode, const N: usize> CanonicalEncode for [T; N] {
    /// Each element in order, no length prefix.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        for item in self {
            item.encode_canonical(out);
        }
    }
}

impl<T: CanonicalEncode> CanonicalEncode for Option<T> {
    /// `0x00` for `None`; `0x01` followed by the value for `Some`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            None => put_variant(out, 0),
            Some(value) => {
                put_variant(out, 1);
                value.encode_canonical(out);
            }
        }
    }
}

impl<A: CanonicalEncode, B: CanonicalEncode> CanonicalEncode for (A, B) {
    /// The fields in order, no prefix.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.0.encode_canonical(out);
        self.1.encode_canonical(out);
    }
}

impl<A: CanonicalEncode, B: CanonicalEncode, C: CanonicalEncode> CanonicalEncode for (A, B, C) {
    /// The fields in order, no prefix.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.0.encode_canonical(out);
        self.1.encode_canonical(out);
        self.2.encode_canonical(out);
    }
}

impl<K: CanonicalEncode, V: CanonicalEncode> CanonicalEncode for BTreeMap<K, V> {
    /// [`put_len`] of the entry count, then each key and value in ascending key order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        for (key, value) in self {
            key.encode_canonical(out);
            value.encode_canonical(out);
        }
    }
}

impl<T: CanonicalEncode> CanonicalEncode for BTreeSet<T> {
    /// [`put_len`] of the element count, then each element in ascending order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        for item in self {
            item.encode_canonical(out);
        }
    }
}

impl<T: CanonicalEncode + ?Sized> CanonicalEncode for &T {
    /// The bytes of the referenced value.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        (**self).encode_canonical(out);
    }
}
