// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The located error of the `.pzl` loader (REQ-TEST-001, R12).

use core::fmt;

/// What is wrong with a `.pzl` file (REQ-TEST-001). New kinds may be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ScenarioErrorKind {
    /// The bytes are not well-formed JSON text: empty input, not UTF-8, a bad token, trailing
    /// content.
    Syntax,
    /// The input is over [`MAX_SCENARIO_BYTES`](super::MAX_SCENARIO_BYTES), or it holds more
    /// than [`MAX_ARRAY_ELEMENTS`](super::MAX_ARRAY_ELEMENTS) array elements in total.
    TooLarge,
    /// Arrays and objects nest deeper than [`MAX_DEPTH`](super::MAX_DEPTH).
    TooDeep,
    /// An object member the schema does not define.
    UnknownField,
    /// A required member is missing.
    MissingField,
    /// A value has the wrong JSON type (a floating-point number is always this kind, R3).
    WrongType,
    /// A number, a length or a fixed-format value (a `definitionHash`) is outside its allowed
    /// range.
    OutOfRange,
    /// A string that names an enumerated value (a phase, a step, an assertion kind, a choice
    /// type, an answer type, ...) is not one of the allowed names.
    UnknownTag,
    /// The same key appears twice in one JSON object.
    DuplicateKey,
    /// A player, object or card name is used but not declared.
    UnresolvedReference,
    /// A player or object id is declared twice.
    Duplicate,
    /// The file is well-formed but asks for something this version does not support (a
    /// `pzlVersion` other than 1).
    Unsupported,
    /// A value is well-typed but malformed: a date that is not `YYYY-MM-DD`, a step that does not belong to its phase, an empty string where
    /// a name is required.
    Invalid,
}

impl ScenarioErrorKind {
    /// The kind as a stable lowercase word (`syntax`, `too-large`, ...).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ScenarioErrorKind::Syntax => "syntax",
            ScenarioErrorKind::TooLarge => "too-large",
            ScenarioErrorKind::TooDeep => "too-deep",
            ScenarioErrorKind::UnknownField => "unknown-field",
            ScenarioErrorKind::MissingField => "missing-field",
            ScenarioErrorKind::WrongType => "wrong-type",
            ScenarioErrorKind::OutOfRange => "out-of-range",
            ScenarioErrorKind::UnknownTag => "unknown-tag",
            ScenarioErrorKind::DuplicateKey => "duplicate-key",
            ScenarioErrorKind::UnresolvedReference => "unresolved-reference",
            ScenarioErrorKind::Duplicate => "duplicate",
            ScenarioErrorKind::Unsupported => "unsupported",
            ScenarioErrorKind::Invalid => "invalid",
        }
    }
}

impl fmt::Display for ScenarioErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One located loader error (REQ-TEST-001, R12).
///
/// `path` is a JSON-pointer-style location (`/players/1/zones/hand/0`; `""` is the document
/// root), complete and escaped (`~0`, `~1`), so it always round-trips through
/// [`pointer_position`](super::pointer_position). `Display` (only) prints each segment cut to
/// 64 characters and with control characters as `?`, so a hostile key cannot flood a log. `line` and `column` are 1-based
/// (the column counts Unicode scalar values) and are present for syntax errors and wherever the
/// offending value's position is known. `message` never quotes more than 80 characters of input
/// text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioError {
    /// The JSON-pointer-style location of the problem.
    pub path: String,
    /// The 1-based line, when known.
    pub line: Option<u32>,
    /// The 1-based column, when known.
    pub column: Option<u32>,
    /// What is wrong.
    pub kind: ScenarioErrorKind,
    /// A human-readable explanation (at most 80 characters of quoted input).
    pub message: String,
}

impl ScenarioError {
    /// An error at `path` with an optional position.
    pub(crate) fn new(
        kind: ScenarioErrorKind,
        path: impl Into<String>,
        pos: Option<(u32, u32)>,
        message: impl Into<String>,
    ) -> ScenarioError {
        ScenarioError {
            path: path.into(),
            line: pos.map(|p| p.0),
            column: pos.map(|p| p.1),
            kind,
            message: message.into(),
        }
    }

    /// The `path` for printing: control characters as `?`, each segment cut to 64 characters
    /// (then `...`). The same text `Display` shows; the stored `path` is untouched (R12).
    #[must_use]
    pub fn display_path(&self) -> String {
        display_path(&self.path)
    }
}

impl fmt::Display for ScenarioError {
    /// `path:line:col: message`, the path sanitised for display (control characters as `?`,
    /// each segment cut to 64 characters; empty for the root); an unknown line or column is
    /// printed as `?`. The stored `path` is untouched.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = self.line.map_or_else(|| "?".to_owned(), |l| l.to_string());
        let column = self
            .column
            .map_or_else(|| "?".to_owned(), |c| c.to_string());
        write!(
            f,
            "{}:{line}:{column}: {}",
            display_path(&self.path),
            self.message
        )
    }
}

impl std::error::Error for ScenarioError {}

/// The longest run of input text quoted in a message.
pub(crate) const MAX_QUOTE_CHARS: usize = 40;

/// The longest path segment taken from the input.
pub(crate) const MAX_SEGMENT_CHARS: usize = 64;

/// `text` quoted for a message: control characters escaped, at most [`MAX_QUOTE_CHARS`]
/// characters of input, then `...` if cut.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for (count, c) in text.chars().enumerate() {
        if count == MAX_QUOTE_CHARS {
            out.push_str("...");
            break;
        }
        if c.is_control() || c == '"' || c == '\\' {
            out.push('?');
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// One complete JSON-pointer segment from `key`: `~` as `~0`, `/` as `~1`, nothing else
/// changed (no truncation, no replacement), so a stored path always finds its member again
/// (`pointer_position`). Only [`ScenarioError`]'s `Display` shortens and sanitises it.
pub(crate) fn segment(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for c in key.chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
    out
}

/// `path` for display: every segment cut to [`MAX_SEGMENT_CHARS`] characters (then `...`),
/// control characters shown as `?`. The result is bounded per segment and one line.
fn display_path(path: &str) -> String {
    let mut out = String::new();
    for (i, seg) in path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        for (count, c) in seg.chars().enumerate() {
            if count == MAX_SEGMENT_CHARS {
                out.push_str("...");
                break;
            }
            out.push(if c.is_control() { '?' } else { c });
        }
    }
    out
}
