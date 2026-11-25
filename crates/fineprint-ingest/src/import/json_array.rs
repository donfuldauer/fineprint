// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Splitting a top-level JSON array into its element slices (R12).
//!
//! A bulk file is one large JSON array. Parsing it whole into a `serde_json::Value` would hold
//! the entire tree in memory; instead [`split_array`] finds the byte range of each top-level
//! element with a small scanner (strings, escapes and bracket depth only) and each element is
//! then parsed on its own by `serde_json`, which does the full JSON validation (with its default
//! recursion limit). The scanner never panics on any input.

/// Why a byte string is not a JSON array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayError {
    /// The input is not valid UTF-8.
    NotUtf8,
    /// The first non-whitespace byte is not `[`.
    NotArray,
    /// Unbalanced brackets, an unterminated string, a stray separator or trailing data.
    Malformed,
}

impl std::fmt::Display for ArrayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ArrayError::NotUtf8 => "the file is not valid UTF-8",
            ArrayError::NotArray => "the file is not a JSON array",
            ArrayError::Malformed => "the file is not well-formed JSON",
        })
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// The byte slices of the top-level elements of the JSON array in `bytes`, whitespace trimmed,
/// in source order. Each slice still has to be parsed (and so validated) by `serde_json`; an
/// empty slice (as in `[1,,2]` or `[1,]`) is returned as is and fails that parse.
///
/// # Errors
/// [`ArrayError`] when `bytes` is not UTF-8, does not start with `[` (after whitespace), or the
/// bracket structure is broken (including trailing non-whitespace after the closing `]`).
pub fn split_array(bytes: &[u8]) -> Result<Vec<&[u8]>, ArrayError> {
    if std::str::from_utf8(bytes).is_err() {
        return Err(ArrayError::NotUtf8);
    }
    let mut i = 0usize;
    while bytes.get(i).copied().is_some_and(is_ws) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'[') {
        return Err(ArrayError::NotArray);
    }
    i += 1;
    let mut out = Vec::new();
    // Empty array: `[` ws `]`.
    let mut j = i;
    while bytes.get(j).copied().is_some_and(is_ws) {
        j += 1;
    }
    if bytes.get(j) == Some(&b']') {
        return trailing_ok(bytes, j + 1).map(|()| out);
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = i;
    while let Some(&b) = bytes.get(i) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'[' | b'{' => depth += 1,
                b']' | b'}' if depth > 0 => depth -= 1,
                b',' if depth == 0 => {
                    out.push(trim(bytes.get(start..i).unwrap_or_default()));
                    start = i + 1;
                }
                b']' => {
                    out.push(trim(bytes.get(start..i).unwrap_or_default()));
                    return trailing_ok(bytes, i + 1).map(|()| out);
                }
                b'}' => return Err(ArrayError::Malformed),
                _ => {}
            }
        }
        i += 1;
    }
    Err(ArrayError::Malformed)
}

fn trailing_ok(bytes: &[u8], from: usize) -> Result<(), ArrayError> {
    if bytes
        .get(from..)
        .unwrap_or_default()
        .iter()
        .all(|&b| is_ws(b))
    {
        Ok(())
    } else {
        Err(ArrayError::Malformed)
    }
}

fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !is_ws(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|&b| !is_ws(b)).map_or(start, |p| p + 1);
    s.get(start..end).unwrap_or_default()
}

/// Parses `bytes` as a JSON array and each element as a `serde_json::Value`; the elements are
/// returned in order, a failing element as `Err(())`.
///
/// # Errors
/// [`ArrayError`] when the outer array structure is invalid.
pub fn parse_elements(bytes: &[u8]) -> Result<Vec<Result<serde_json::Value, ()>>, ArrayError> {
    Ok(split_array(bytes)?
        .into_iter()
        .map(|s| serde_json::from_slice(s).map_err(|_| ()))
        .collect())
}

/// Validates that `bytes` is a well-formed JSON array whose every element parses, dropping each
/// element after parsing it (memory stays bounded by the largest element).
///
/// # Errors
/// [`ArrayError`]; an element that does not parse is [`ArrayError::Malformed`].
pub fn validate_array(bytes: &[u8]) -> Result<usize, ArrayError> {
    let parts = split_array(bytes)?;
    for p in &parts {
        if serde_json::from_slice::<serde_json::Value>(p).is_err() {
            return Err(ArrayError::Malformed);
        }
    }
    Ok(parts.len())
}
