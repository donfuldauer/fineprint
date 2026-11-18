// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! RFC 8785 JSON Canonicalization Scheme (JCS) for content-addressed artifacts (REQ-NFR-006).
//!
//! The serialisation itself is `serde_json_canonicalizer` (docs/DECISIONS.md, "FP-004 — RFC 8785
//! canonical JSON"). This module adds the checks that library does not make:
//!
//! - **Duplicate member names** are rejected at every depth, compared after string decoding
//!   (RFC 8785 requires I-JSON input; `serde_json::Value` would silently keep the last one).
//! - **Unsafe integers** (magnitude above [`MAX_SAFE_INTEGER`]) are rejected, because JCS would
//!   serialise them as rounded doubles without any error. Large values must travel as strings.
//! - **Noncharacters** (U+FDD0..=U+FDEF and every code point ending in FFFE or FFFF) are rejected
//!   in decoded member names and string values, whether written literally or escaped
//!   (RFC 7493 §2.1).
//! - **Nesting** is limited to [`MAX_DEPTH`] open arrays and objects, the same budget for
//!   loading, hashing and export.
//!
//! [`to_canonical_vec`] uses a checked serializer instead of `serde_json::to_value`, so a Rust
//! value is refused for the same reasons as input bytes (plus non-finite floats, which
//! `serde_json::to_value` would silently turn into `null`): what is exported can be loaded.
//!
//! No floating-point type is named here (REQ-CORE-006) except in the checked serializer's two
//! float methods, the boundary that rejects non-finite values: number syntax is validated by
//! `serde_json`, and integer literals are range-checked lexically on the input bytes.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;
use serde_json::Value;

mod ser;
#[cfg(test)]
mod tests;

pub(crate) use ser::to_checked_value;

/// The largest integer magnitude that survives JCS unchanged: 2^53 − 1.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// The largest document nesting depth accepted everywhere: the count of simultaneously open
/// arrays and objects (a scalar adds none, so `[[1]]` has depth 2). It equals the largest depth
/// `serde_json`'s parser accepts, so [`canonicalize`] and [`parse_checked`] (loading),
/// [`to_canonical_vec`] (export) and every content hash share one budget: whatever can be
/// exported can be loaded again.
pub const MAX_DEPTH: usize = 127;

/// Why a JSON document could not be canonicalised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// Not valid JSON: syntax error, nesting deeper than the parser's limit, trailing bytes,
    /// invalid UTF-8, a lone surrogate escape or a number outside the double range.
    Syntax(String),
    /// An object contains the same member name twice (the decoded name is carried).
    DuplicateKey(String),
    /// An integer whose magnitude exceeds [`MAX_SAFE_INTEGER`] (the literal or value is carried).
    UnsafeInteger(String),
    /// A value that serde cannot serialise to JSON (for example a map with non-string keys).
    Serialize(String),
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::Syntax(msg) => write!(f, "invalid JSON: {msg}"),
            CanonicalError::DuplicateKey(key) => {
                write!(f, "duplicate JSON object member name {key:?}")
            }
            CanonicalError::UnsafeInteger(lit) => write!(
                f,
                "integer {lit} is outside the safe range ±{MAX_SAFE_INTEGER} \
                 (larger values must be carried as strings)"
            ),
            CanonicalError::Serialize(msg) => {
                write!(f, "value cannot be serialised to JSON: {msg}")
            }
        }
    }
}

impl std::error::Error for CanonicalError {}

/// Parses untrusted JSON bytes and returns their RFC 8785 canonical form.
///
/// Members are sorted by UTF-16 code units, numbers use ECMAScript formatting, strings use the
/// minimal escaping of RFC 8785 and no whitespace is emitted. The result is idempotent:
/// `canonicalize(&canonicalize(x)?) == canonicalize(x)`. Non-integer numbers are accepted (JCS
/// defines them). Integers must lie within ±[`MAX_SAFE_INTEGER`]: both an integer literal and a
/// number of any spelling whose value is an integer above that magnitude (for example `1e20`)
/// are rejected, which is what keeps the function idempotent (the canonical form of `1e20` is
/// the integer literal `100000000000000000000`).
///
/// # Errors
/// [`CanonicalError::Syntax`] for invalid JSON (including nesting deeper than [`MAX_DEPTH`]) and
/// for a noncharacter in any decoded member name or string value,
/// [`CanonicalError::DuplicateKey`] for a repeated member name in any object,
/// [`CanonicalError::UnsafeInteger`] for an out-of-range integer.
pub fn canonicalize(json: &[u8]) -> Result<Vec<u8>, CanonicalError> {
    checked(json).map(|(_, bytes)| bytes)
}

/// Serialises `value` to RFC 8785 canonical JSON bytes. The output is always accepted by
/// [`canonicalize`], which returns it unchanged.
///
/// # Errors
/// [`CanonicalError::Serialize`] if serde cannot represent the value as JSON (for example a map
/// with non-string keys), for a NaN or infinite float at any depth, for a noncharacter in a
/// string value or member name, and for nesting deeper than [`MAX_DEPTH`] (detected before
/// recursing further); [`CanonicalError::DuplicateKey`] if a struct or map emits the same member
/// name twice; [`CanonicalError::UnsafeInteger`] if it contains an integer outside
/// ±[`MAX_SAFE_INTEGER`].
pub fn to_canonical_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    let value = to_checked_value(value)?;
    let bytes = jcs(&value)?;
    scan_valid_json(&bytes, Pass::Canonical)?;
    Ok(bytes)
}

/// Parses untrusted JSON into a [`Value`] after the syntax, depth, duplicate-key,
/// safe-integer and noncharacter checks of [`canonicalize`]. Used by checked loaders that need
/// the tree, not the bytes.
///
/// # Errors
/// The same errors as [`canonicalize`] (a noncharacter in a member name or string value is
/// [`CanonicalError::Syntax`]), except [`CanonicalError::Serialize`].
pub fn parse_checked(json: &[u8]) -> Result<Value, CanonicalError> {
    checked(json).map(|(value, _)| value)
}

/// [`parse_checked`], but the returned tree is parsed from the canonical bytes, so every
/// number has its canonical form: `1.0`, `1e0` and `1` all give the integer `1`, `-0.0` gives
/// `0`; non-integral numbers keep their value. Used by loaders whose result must equal a
/// re-serialisation of what they decoded.
pub(crate) fn parse_normalized(json: &[u8]) -> Result<Value, CanonicalError> {
    let (_, bytes) = checked(json)?;
    serde_json::from_slice(&bytes).map_err(|e| CanonicalError::Syntax(e.to_string()))
}

/// Shared body of [`canonicalize`] and [`parse_checked`]: the checked value and its JCS bytes.
fn checked(json: &[u8]) -> Result<(Value, Vec<u8>), CanonicalError> {
    // Syntax first: serde_json enforces its recursion limit (MAX_DEPTH), rejects trailing
    // bytes, invalid UTF-8, lone surrogates and numbers outside the double range.
    let value: Value =
        serde_json::from_slice(json).map_err(|e| CanonicalError::Syntax(e.to_string()))?;
    // The input is now known to be valid JSON; the scan only checks member names and integers.
    scan_valid_json(json, Pass::Input)?;
    check_noncharacters(&value)?;
    let bytes = jcs(&value)?;
    // Numbers spelled with a fraction or exponent can still have a large integer value; their
    // ECMAScript form in the output shows it.
    scan_valid_json(&bytes, Pass::Canonical)?;
    Ok((value, bytes))
}

/// JCS serialisation of an already checked value.
fn jcs(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    serde_json_canonicalizer::to_vec(value).map_err(|e| CanonicalError::Serialize(e.to_string()))
}

/// Whether `c` is a Unicode noncharacter: U+FDD0..=U+FDEF, or a code point whose low 16 bits
/// are FFFE or FFFF (in every plane).
fn is_noncharacter(c: char) -> bool {
    let c = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&c) || c & 0xFFFE == 0xFFFE
}

/// The first noncharacter in `text`, if any.
fn first_noncharacter(text: &str) -> Option<char> {
    text.chars().find(|&c| is_noncharacter(c))
}

/// Rejects a noncharacter in any decoded member name or string value of `value` (RFC 7493
/// §2.1). Iterative, so the walk cannot overflow the stack.
fn check_noncharacters(value: &Value) -> Result<(), CanonicalError> {
    let found = |text: &str, what: &str| {
        first_noncharacter(text).map_or(Ok(()), |c| {
            Err(CanonicalError::Syntax(format!(
                "{what} contains the noncharacter U+{:04X}",
                u32::from(c)
            )))
        })
    };
    let mut stack: Vec<&Value> = vec![value];
    while let Some(v) = stack.pop() {
        match v {
            Value::String(text) => found(text, "string")?,
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(map) => {
                for (key, member) in map {
                    found(key, "member name")?;
                    stack.push(member);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    Ok(())
}

/// Which bytes a lexical scan is looking at; it decides how a number literal is classified.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Untrusted input: only literals without fraction or exponent are known to be integers.
    Input,
    /// JCS output: ECMAScript number form, where an integral value is written without `.` and
    /// without a negative exponent, and every value of 1e21 or more uses a positive exponent.
    Canonical,
}

/// One open container during the lexical scan.
enum Frame {
    /// An object: the decoded member names seen so far and whether the next string is a name.
    Object {
        keys: BTreeSet<String>,
        expect_key: bool,
    },
    /// An array.
    Array,
}

/// Lexical pass over bytes that `serde_json` has already accepted as valid JSON. Rejects
/// duplicate member names (compared after decoding, at every depth) and integer literals whose
/// magnitude exceeds [`MAX_SAFE_INTEGER`]. Written defensively: any unexpected byte is reported
/// as a syntax error rather than trusted, and no step can panic.
fn scan_valid_json(json: &[u8], pass: Pass) -> Result<(), CanonicalError> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0usize;
    while let Some(&b) = json.get(i) {
        match b {
            b' ' | b'\t' | b'\n' | b'\r' | b':' => i += 1,
            b'{' => {
                stack.push(Frame::Object {
                    keys: BTreeSet::new(),
                    expect_key: true,
                });
                i += 1;
            }
            b'[' => {
                stack.push(Frame::Array);
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                if let Some(Frame::Object { expect_key, .. }) = stack.last_mut() {
                    *expect_key = true;
                }
                i += 1;
            }
            b'"' => {
                let end = string_end(json, i)?;
                if let Some(Frame::Object { keys, expect_key }) = stack.last_mut()
                    && *expect_key
                {
                    let raw = json.get(i..end).ok_or_else(unexpected)?;
                    let key: String = serde_json::from_slice(raw)
                        .map_err(|e| CanonicalError::Syntax(e.to_string()))?;
                    if !keys.insert(key.clone()) {
                        return Err(CanonicalError::DuplicateKey(key));
                    }
                    *expect_key = false;
                }
                i = end;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while let Some(c) = json.get(i) {
                    if matches!(c, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let literal = json.get(start..i).ok_or_else(unexpected)?;
                check_integer_literal(literal, pass)?;
            }
            b't' | b'f' | b'n' => {
                while json.get(i).is_some_and(u8::is_ascii_lowercase) {
                    i += 1;
                }
            }
            _ => return Err(unexpected()),
        }
    }
    Ok(())
}

/// Error for a byte the lexical scan did not expect (cannot happen after a successful parse).
fn unexpected() -> CanonicalError {
    CanonicalError::Syntax("unexpected byte in JSON scan".to_owned())
}

/// Given `json[start] == b'"'`, returns the index just past the closing quote.
fn string_end(json: &[u8], start: usize) -> Result<usize, CanonicalError> {
    let mut i = start + 1;
    loop {
        match json.get(i) {
            None => return Err(unexpected()),
            Some(b'\\') => i += 2,
            Some(b'"') => return Ok(i + 1),
            Some(_) => i += 1,
        }
    }
}

/// Rejects a number literal whose value is an integer of magnitude above [`MAX_SAFE_INTEGER`].
/// On input, only literals without fraction or exponent are classified (others are checked in
/// their canonical form). In canonical form, a negative exponent means a non-integer,
/// a positive exponent means an integer of at least 1e21 (even when written as `1.5e+300`), and
/// otherwise a `.` means a non-integer.
fn check_integer_literal(literal: &[u8], pass: Pass) -> Result<(), CanonicalError> {
    let has_exponent = literal.iter().any(|c| matches!(c, b'e' | b'E'));
    let has_fraction = literal.contains(&b'.');
    let negative_exponent = literal.windows(2).any(|w| matches!(w, b"e-" | b"E-"));
    let unsafe_integer = |literal: &[u8]| {
        Err(CanonicalError::UnsafeInteger(
            String::from_utf8_lossy(literal).into_owned(),
        ))
    };
    match pass {
        Pass::Input if has_exponent || has_fraction => return Ok(()),
        Pass::Canonical if negative_exponent => return Ok(()),
        Pass::Canonical if has_exponent => return unsafe_integer(literal),
        Pass::Canonical if has_fraction => return Ok(()),
        Pass::Input | Pass::Canonical => {}
    }
    let digits = literal.strip_prefix(b"-").unwrap_or(literal);
    // JSON forbids leading zeros, so more than 16 digits is always above 2^53 − 1 (16 digits).
    let safe = digits.len() <= 16
        && digits
            .iter()
            .try_fold(0u64, |acc, d| {
                acc.checked_mul(10)?
                    .checked_add(u64::from(d.wrapping_sub(b'0')))
            })
            .is_some_and(|v| v <= MAX_SAFE_INTEGER.unsigned_abs());
    if safe {
        Ok(())
    } else {
        unsafe_integer(literal)
    }
}
