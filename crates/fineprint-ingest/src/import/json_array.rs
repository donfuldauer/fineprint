// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Splitting a top-level JSON array into its element slices (R12).
//!
//! A bulk file is one large JSON array. Parsing it whole into a `serde_json::Value` would hold
//! the entire tree in memory; instead [`ArrayElements`] finds the byte range of each top-level
//! element with a small scanner (strings, escapes and bracket depth only), yielding one element
//! at a time, and each element is then parsed on its own by `serde_json`, which does the full
//! JSON validation (with its default recursion limit). The scanner never panics on any input.
//! [`split_array`] collects the same slices into a `Vec` for callers that need them all at once.

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
/// in source order: [`ArrayElements`] collected. Each slice still has to be parsed (and so
/// validated) by `serde_json`; an empty slice (as in `[1,,2]` or `[1,]`) is returned as is and
/// fails that parse. This keeps one slice per element in memory; bulk paths iterate
/// [`ArrayElements`] instead.
///
/// # Errors
/// [`ArrayError`] when `bytes` is not UTF-8, does not start with `[` (after whitespace), or the
/// bracket structure is broken (including trailing non-whitespace after the closing `]`).
pub fn split_array(bytes: &[u8]) -> Result<Vec<&[u8]>, ArrayError> {
    ArrayElements::new(bytes).collect()
}

/// Where an [`ArrayElements`] scan stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Nothing checked yet.
    Start,
    /// Inside the array; the next element starts at `pos`.
    Scan,
    /// The closing `]` was seen; only whitespace may follow from this offset.
    Trailing(usize),
    /// Finished (or failed): yields nothing more.
    Done,
}

/// The top-level elements of a JSON array, one whitespace-trimmed byte slice at a time, in
/// source order (R12): the streaming form of [`split_array`], with the same elements and the
/// same [`ArrayError`] values, but holding no index of the elements (memory stays constant
/// whatever the element count).
///
/// The first `next` checks UTF-8 (one pass over the input, no allocation) and the opening `[`.
/// A structural error is yielded as `Some(Err(_))` once, after the elements that precede it,
/// and the iterator is fused afterwards; a caller that needs all-or-nothing semantics discards
/// what it built from the earlier elements when it sees an error.
#[derive(Clone, Debug)]
pub struct ArrayElements<'a> {
    bytes: &'a [u8],
    pos: usize,
    phase: Phase,
}

impl<'a> ArrayElements<'a> {
    /// A scanner over the JSON array in `bytes` (nothing is checked until the first `next`).
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        ArrayElements {
            bytes,
            pos: 0,
            phase: Phase::Start,
        }
    }

    fn fail(&mut self, e: ArrayError) -> Option<Result<&'a [u8], ArrayError>> {
        self.phase = Phase::Done;
        Some(Err(e))
    }

    fn begin(&mut self) -> Option<Result<&'a [u8], ArrayError>> {
        let bytes = self.bytes;
        if std::str::from_utf8(bytes).is_err() {
            return self.fail(ArrayError::NotUtf8);
        }
        let mut i = 0usize;
        while bytes.get(i).copied().is_some_and(is_ws) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'[') {
            return self.fail(ArrayError::NotArray);
        }
        i += 1;
        // Empty array: `[` ws `]`.
        let mut j = i;
        while bytes.get(j).copied().is_some_and(is_ws) {
            j += 1;
        }
        if bytes.get(j) == Some(&b']') {
            self.phase = Phase::Done;
            return trailing_ok(bytes, j + 1).err().map(Err);
        }
        self.pos = i;
        self.phase = Phase::Scan;
        self.scan()
    }

    /// Scans one element from `pos` (each element starts outside any string at depth 0).
    fn scan(&mut self) -> Option<Result<&'a [u8], ArrayError>> {
        let bytes = self.bytes;
        let start = self.pos;
        let mut i = start;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
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
                        self.pos = i + 1;
                        return Some(Ok(trim(bytes.get(start..i).unwrap_or_default())));
                    }
                    b']' => {
                        self.phase = Phase::Trailing(i + 1);
                        return Some(Ok(trim(bytes.get(start..i).unwrap_or_default())));
                    }
                    b'}' => return self.fail(ArrayError::Malformed),
                    _ => {}
                }
            }
            i += 1;
        }
        self.fail(ArrayError::Malformed)
    }
}

impl<'a> Iterator for ArrayElements<'a> {
    type Item = Result<&'a [u8], ArrayError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.phase {
            Phase::Done => None,
            Phase::Start => self.begin(),
            Phase::Scan => self.scan(),
            Phase::Trailing(from) => {
                self.phase = Phase::Done;
                trailing_ok(self.bytes, from).err().map(Err)
            }
        }
    }
}

impl std::iter::FusedIterator for ArrayElements<'_> {}

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

/// Validates that `bytes` is a well-formed JSON array whose every element parses, streaming
/// through [`ArrayElements`] and dropping each element after parsing it: no per-element index
/// is built, so memory stays bounded by the largest element (R12). Returns the element count.
///
/// # Errors
/// [`ArrayError`]; an element that does not parse is [`ArrayError::Malformed`].
pub fn validate_array(bytes: &[u8]) -> Result<usize, ArrayError> {
    let mut count = 0usize;
    for p in ArrayElements::new(bytes) {
        if serde_json::from_slice::<serde_json::Value>(p?).is_err() {
            return Err(ArrayError::Malformed);
        }
        count += 1;
    }
    Ok(count)
}
