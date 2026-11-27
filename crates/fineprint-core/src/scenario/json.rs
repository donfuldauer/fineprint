// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! A defensive JSON reader for `.pzl` files (REQ-TEST-001, R12, R3).
//!
//! It produces a tree of [`Node`]s that remember their 1-based line and column, and it enforces
//! the loader limits while it reads: input size, nesting depth, the total number of array
//! elements, duplicate object keys, integers only (any fraction or exponent is rejected) and
//! integers within `i64::MIN..=u64::MAX`. It never panics and uses no recursion deeper than
//! [`MAX_DEPTH`].

use std::collections::BTreeSet;

use super::error::{ScenarioError, ScenarioErrorKind, quote, segment};
use super::{MAX_ARRAY_ELEMENTS, MAX_DEPTH, MAX_SCENARIO_BYTES};

/// A JSON value with its position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    /// 1-based line of the first character of the value.
    pub line: u32,
    /// 1-based column of the first character of the value.
    pub column: u32,
    /// The value.
    pub value: Value,
}

/// One object member: its key, the key's position and the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    /// The key.
    pub key: String,
    /// 1-based line of the key.
    pub line: u32,
    /// 1-based column of the key.
    pub column: u32,
    /// The value.
    pub node: Node,
}

/// A JSON value. Numbers are integers only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer in `i64::MIN..=u64::MAX`.
    Int(i128),
    /// A string.
    Str(String),
    /// An array.
    Array(Vec<Node>),
    /// An object, members in file order (keys are unique).
    Object(Vec<Member>),
}

impl Node {
    /// The position as a pair.
    pub fn pos(&self) -> (u32, u32) {
        (self.line, self.column)
    }

    /// The JSON type name of the value, for messages.
    pub fn type_name(&self) -> &'static str {
        match self.value {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Int(_) => "an integer",
            Value::Str(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "an object",
        }
    }

    /// The value as a `serde_json::Value` (integers stay integers).
    pub fn to_json(&self) -> serde_json::Value {
        match &self.value {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(i) => {
                if let Ok(v) = i64::try_from(*i) {
                    serde_json::Value::from(v)
                } else if let Ok(v) = u64::try_from(*i) {
                    serde_json::Value::from(v)
                } else {
                    // Unreachable: the reader rejects anything outside i64::MIN..=u64::MAX.
                    serde_json::Value::Null
                }
            }
            Value::Str(s) => serde_json::Value::String(s.clone()),
            Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(Node::to_json).collect())
            }
            Value::Object(members) => {
                let mut map = serde_json::Map::new();
                for m in members {
                    map.insert(m.key.clone(), m.node.to_json());
                }
                serde_json::Value::Object(map)
            }
        }
    }
}

/// Parses `bytes` as one JSON document under the loader limits.
pub(crate) fn parse(bytes: &[u8]) -> Result<Node, ScenarioError> {
    if bytes.len() > MAX_SCENARIO_BYTES {
        return Err(ScenarioError::new(
            ScenarioErrorKind::TooLarge,
            "",
            None,
            format!(
                "the file is {} bytes; the limit is {MAX_SCENARIO_BYTES} bytes",
                bytes.len()
            ),
        ));
    }
    let text = match core::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            let valid = &bytes[..e.valid_up_to()];
            // The prefix is valid UTF-8 by construction.
            let prefix = core::str::from_utf8(valid).unwrap_or("");
            let (line, column) = position_after(prefix);
            return Err(ScenarioError::new(
                ScenarioErrorKind::Syntax,
                "",
                Some((line, column)),
                format!("the file is not valid UTF-8 at byte {}", e.valid_up_to()),
            ));
        }
    };
    // A leading byte-order mark is tolerated.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim_matches(is_ws).is_empty() {
        return Err(ScenarioError::new(
            ScenarioErrorKind::Syntax,
            "",
            Some((1, 1)),
            "the file is empty",
        ));
    }
    let mut reader = Reader {
        chars: text.chars().collect(),
        index: 0,
        line: 1,
        column: 1,
        elements: 0,
        path: Vec::new(),
    };
    reader.skip_ws();
    let node = reader.value(0)?;
    reader.skip_ws();
    if reader.index < reader.chars.len() {
        return Err(reader.error_here(
            ScenarioErrorKind::Syntax,
            "unexpected content after the JSON document".to_owned(),
        ));
    }
    Ok(node)
}

/// The 1-based line and column just after `prefix`.
fn position_after(prefix: &str) -> (u32, u32) {
    let mut line = 1u32;
    let mut column = 1u32;
    for c in prefix.chars() {
        if c == '\n' {
            line = line.saturating_add(1);
            column = 1;
        } else {
            column = column.saturating_add(1);
        }
    }
    (line, column)
}

/// JSON whitespace (RFC 8259).
fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// The reader state.
struct Reader {
    chars: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
    /// Array elements read so far.
    elements: usize,
    /// The path of the value being read, as pointer segments.
    path: Vec<String>,
}

impl Reader {
    fn pointer(&self) -> String {
        let mut out = String::new();
        for s in &self.path {
            out.push('/');
            out.push_str(s);
        }
        out
    }

    fn error_at(&self, kind: ScenarioErrorKind, pos: (u32, u32), message: String) -> ScenarioError {
        ScenarioError::new(kind, self.pointer(), Some(pos), message)
    }

    fn error_here(&self, kind: ScenarioErrorKind, message: String) -> ScenarioError {
        self.error_at(kind, (self.line, self.column), message)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.index += 1;
        if c == '\n' {
            self.line = self.line.saturating_add(1);
            self.column = 1;
        } else {
            self.column = self.column.saturating_add(1);
        }
        Some(c)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(is_ws) {
            self.bump();
        }
    }

    fn unexpected(&self) -> ScenarioError {
        let message = match self.peek() {
            None => "unexpected end of input".to_owned(),
            Some(c) if c.is_control() => format!("unexpected control character U+{:04X}", c as u32),
            Some(c) => format!("unexpected character {}", quote(&c.to_string())),
        };
        self.error_here(ScenarioErrorKind::Syntax, message)
    }

    fn expect_word(&mut self, word: &str) -> Result<(), ScenarioError> {
        for w in word.chars() {
            if self.peek() != Some(w) {
                return Err(self.unexpected());
            }
            self.bump();
        }
        Ok(())
    }

    /// Reads one value; `depth` is the number of arrays and objects around it.
    fn value(&mut self, depth: usize) -> Result<Node, ScenarioError> {
        let (line, column) = (self.line, self.column);
        let value = match self.peek() {
            Some('{') => self.object(depth + 1)?,
            Some('[') => self.array(depth + 1)?,
            Some('"') => Value::Str(self.string()?),
            Some('t') => {
                self.expect_word("true")?;
                Value::Bool(true)
            }
            Some('f') => {
                self.expect_word("false")?;
                Value::Bool(false)
            }
            Some('n') => {
                self.expect_word("null")?;
                Value::Null
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.number()?,
            _ => return Err(self.unexpected()),
        };
        Ok(Node {
            line,
            column,
            value,
        })
    }

    fn check_depth(&self, depth: usize) -> Result<(), ScenarioError> {
        if depth > MAX_DEPTH {
            return Err(self.error_here(
                ScenarioErrorKind::TooDeep,
                format!("arrays and objects nest deeper than {MAX_DEPTH} levels"),
            ));
        }
        Ok(())
    }

    fn object(&mut self, depth: usize) -> Result<Value, ScenarioError> {
        self.check_depth(depth)?;
        self.bump(); // '{'
        let mut members: Vec<Member> = Vec::new();
        // The keys seen so far: a duplicate is found in O(log n), not by scanning `members`
        // (R12: a wide object must not cost quadratic time; R3: an ordered set).
        let mut seen: BTreeSet<String> = BTreeSet::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err(self.unexpected());
            }
            let (line, column) = (self.line, self.column);
            let key = self.string()?;
            // The path of a duplicate is the object holding it; the position is the key's.
            if !seen.insert(key.clone()) {
                return Err(self.error_at(
                    ScenarioErrorKind::DuplicateKey,
                    (line, column),
                    format!("the key {} appears twice in one object", quote(&key)),
                ));
            }
            self.path.push(segment(&key));
            self.skip_ws();
            if self.peek() != Some(':') {
                return Err(self.unexpected());
            }
            self.bump();
            self.skip_ws();
            let node = self.value(depth)?;
            self.path.pop();
            members.push(Member {
                key,
                line,
                column,
                node,
            });
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {
                    self.bump();
                    return Ok(Value::Object(members));
                }
                _ => return Err(self.unexpected()),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, ScenarioError> {
        self.check_depth(depth)?;
        self.bump(); // '['
        let mut items: Vec<Node> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            self.elements += 1;
            if self.elements > MAX_ARRAY_ELEMENTS {
                return Err(self.error_here(
                    ScenarioErrorKind::TooLarge,
                    format!("the file holds more than {MAX_ARRAY_ELEMENTS} array elements"),
                ));
            }
            self.path.push(items.len().to_string());
            let node = self.value(depth)?;
            self.path.pop();
            items.push(node);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.unexpected()),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ScenarioError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let Some(d) = self.peek().and_then(|c| c.to_digit(16)) else {
                return Err(self.error_here(
                    ScenarioErrorKind::Syntax,
                    "a \\u escape needs four hex digits".to_owned(),
                ));
            };
            self.bump();
            v = v * 16 + d;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ScenarioError> {
        self.bump(); // '"'
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(
                    self.error_here(ScenarioErrorKind::Syntax, "unterminated string".to_owned())
                );
            };
            match c {
                '"' => {
                    self.bump();
                    return Ok(out);
                }
                '\\' => {
                    self.bump();
                    let Some(e) = self.peek() else {
                        return Err(self.unexpected());
                    };
                    match e {
                        '"' | '\\' | '/' => {
                            self.bump();
                            out.push(e);
                        }
                        'b' | 'f' | 'n' | 'r' | 't' => {
                            self.bump();
                            out.push(match e {
                                'b' => '\u{8}',
                                'f' => '\u{c}',
                                'n' => '\n',
                                'r' => '\r',
                                _ => '\t',
                            });
                        }
                        'u' => {
                            self.bump();
                            let high = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&high) {
                                if self.peek() != Some('\\') {
                                    return Err(self.lone_surrogate());
                                }
                                self.bump();
                                if self.peek() != Some('u') {
                                    return Err(self.lone_surrogate());
                                }
                                self.bump();
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err(self.lone_surrogate());
                                }
                                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&high) {
                                return Err(self.lone_surrogate());
                            } else {
                                high
                            };
                            match char::from_u32(code) {
                                Some(ch) => out.push(ch),
                                None => return Err(self.lone_surrogate()),
                            }
                        }
                        _ => {
                            return Err(self.error_here(
                                ScenarioErrorKind::Syntax,
                                format!("invalid escape {}", quote(&format!("\\{e}"))),
                            ));
                        }
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.error_here(
                        ScenarioErrorKind::Syntax,
                        format!(
                            "control character U+{:04X} inside a string must be escaped",
                            c as u32
                        ),
                    ));
                }
                c => {
                    self.bump();
                    out.push(c);
                }
            }
        }
    }

    fn lone_surrogate(&self) -> ScenarioError {
        self.error_here(
            ScenarioErrorKind::Syntax,
            "a \\u escape names an unpaired surrogate".to_owned(),
        )
    }

    /// Consumes a run of ASCII digits into `text`; false if there was none.
    fn take_digits(&mut self, text: &mut String) -> bool {
        let mut any = false;
        while let Some(d) = self.peek().filter(char::is_ascii_digit) {
            self.bump();
            if text.len() <= 4 * super::error::MAX_QUOTE_CHARS {
                text.push(d);
            }
            any = true;
        }
        any
    }

    fn number(&mut self) -> Result<Value, ScenarioError> {
        let start = (self.line, self.column);
        let negative = self.peek() == Some('-');
        if negative {
            self.bump();
        }
        let mut digits = String::new();
        match self.peek() {
            Some('0') => {
                self.bump();
                digits.push('0');
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(self.error_at(
                        ScenarioErrorKind::Syntax,
                        start,
                        "a number must not have a leading zero".to_owned(),
                    ));
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while let Some(d) = self.peek().filter(char::is_ascii_digit) {
                    self.bump();
                    digits.push(d);
                }
            }
            _ => return Err(self.unexpected()),
        }
        if matches!(self.peek(), Some('.' | 'e' | 'E')) {
            // Consume exactly what the JSON number grammar allows (a fraction is `.` and at
            // least one digit; an exponent is `e|E`, an optional sign and at least one digit),
            // so a malformed number is a syntax error and the next token is reported correctly.
            let mut text = format!("{}{digits}", if negative { "-" } else { "" });
            let mut well_formed = true;
            if self.peek() == Some('.') {
                self.bump();
                text.push('.');
                well_formed &= self.take_digits(&mut text);
            }
            if well_formed && matches!(self.peek(), Some('e' | 'E')) {
                if let Some(e) = self.peek() {
                    self.bump();
                    text.push(e);
                }
                if let Some(sign) = self.peek().filter(|c| matches!(c, '+' | '-')) {
                    self.bump();
                    text.push(sign);
                }
                well_formed &= self.take_digits(&mut text);
            }
            if !well_formed {
                return Err(self.error_at(
                    ScenarioErrorKind::Syntax,
                    start,
                    format!("{} is not a valid number", quote(&text)),
                ));
            }
            return Err(self.error_at(
                ScenarioErrorKind::WrongType,
                start,
                format!(
                    "{} is not an integer: floating-point numbers are not allowed",
                    quote(&text)
                ),
            ));
        }
        let out_of_range = || {
            ScenarioError::new(
                ScenarioErrorKind::OutOfRange,
                self.pointer(),
                Some(start),
                format!(
                    "the integer {}{} is outside the 64-bit range",
                    if negative { "-" } else { "" },
                    quote(&digits)
                ),
            )
        };
        // 21 digits are more than any 64-bit integer has; stop before i128 could overflow.
        if digits.len() > 20 {
            return Err(out_of_range());
        }
        let magnitude: i128 = digits.parse().map_err(|_| out_of_range())?;
        let value = if negative { -magnitude } else { magnitude };
        if value < i128::from(i64::MIN) || value > i128::from(u64::MAX) {
            return Err(out_of_range());
        }
        Ok(Value::Int(value))
    }
}

/// The position of the value at JSON pointer `pointer` in `bytes`, if the bytes parse and the
/// pointer names a value (REQ-TEST-001). `""` is the root. Used to print `path:line:col` for a
/// failed assertion or script step.
#[must_use]
pub fn pointer_position(bytes: &[u8], pointer: &str) -> Option<(u32, u32)> {
    let root = parse(bytes).ok()?;
    let mut node = &root;
    if pointer.is_empty() {
        return Some(node.pos());
    }
    let rest = pointer.strip_prefix('/')?;
    for raw in rest.split('/') {
        let key = raw.replace("~1", "/").replace("~0", "~");
        node = match &node.value {
            Value::Object(members) => &members.iter().find(|m| m.key == key)?.node,
            Value::Array(items) => items.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(node.pos())
}
