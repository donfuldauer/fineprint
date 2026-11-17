// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Minimal JSON reader and string writer (std only) for `docs/cr-citations.json` and
//! `cr-cache/source.json` (REQ-NFR-007).
//!
//! The reader never panics on any input and caps nesting at [`MAX_DEPTH`]; the same approach as
//! the reader in `xtask/src/coverage.rs`, copied so this module stays self-contained.

/// Maximum nesting depth accepted by [`parse`].
pub const MAX_DEPTH: usize = 64;

/// A parsed JSON value. Numbers keep their source text.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// A number, as written.
    Num(String),
    /// A string with escapes decoded.
    Str(String),
    /// An array.
    Arr(Vec<Json>),
    /// An object, in source order.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// The value of `key` in an object (first occurrence).
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The source text of a number, if this is a number.
    pub fn as_num(&self) -> Option<&str> {
        match self {
            Json::Num(s) => Some(s),
            _ => None,
        }
    }
}

/// Encode `s` as a JSON string literal (quotes, backslashes and control characters escaped).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Parse a complete JSON document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(format!("trailing characters at offset {}", p.pos));
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{c}` at offset {}", self.pos))
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        for c in word.chars() {
            self.expect(c)?;
        }
        Ok(v)
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting too deep".to_string());
        }
        match self.peek() {
            None => Err("unexpected end of input".to_string()),
            Some('{') => self.object(depth),
            Some('[') => self.array(depth),
            Some('"') => self.string().map(Json::Str),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected `{c}` at offset {}", self.pos)),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect('{')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Json::Obj(items));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(':')?;
            self.skip_ws();
            let v = self.value(depth + 1)?;
            items.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(items));
                }
                _ => return Err(format!("expected `,` or `}}` at offset {}", self.pos)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("expected `,` or `]` at offset {}", self.pos)),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut n = 0u32;
        for _ in 0..4 {
            let d = self
                .peek()
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| format!("bad \\u escape at offset {}", self.pos))?;
            n = n * 16 + d;
            self.pos += 1;
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or("unterminated string")?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or("unterminated string")?;
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                self.expect('\\')?;
                                self.expect('u')?;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err("bad surrogate pair".to_string());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(cp).ok_or("bad code point")?);
                        }
                        other => return Err(format!("bad escape `\\{other}`")),
                    }
                }
                c if (c as u32) < 0x20 => return Err("control character in string".to_string()),
                c => out.push(c),
            }
        }
    }

    fn digits(&mut self) -> Result<(), String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            Err(format!("expected digit at offset {}", self.pos))
        } else {
            Ok(())
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        self.digits()?;
        if self.peek() == Some('.') {
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            self.digits()?;
        }
        Ok(Json::Num(self.chars[start..self.pos].iter().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-NFR-007: values, escapes and errors.
    #[test]
    fn req_nfr_007_json_reader() {
        let v = parse(r#" {"a": [1, -2.5e3, true, false, null], "b": "x\"\\\/\b\f\n\r\t\u00e9\ud83d\ude00"} "#)
            .unwrap();
        assert_eq!(
            v.get("a").and_then(|a| match a {
                Json::Arr(x) => x.first().and_then(Json::as_num).map(str::to_string),
                _ => None,
            }),
            Some("1".to_string())
        );
        assert_eq!(
            v.get("b").and_then(Json::as_str),
            Some("x\"\\/\u{8}\u{c}\n\r\t\u{e9}\u{1f600}")
        );
        assert!(v.get("zz").is_none());
        assert!(Json::Null.get("a").is_none());
        assert!(Json::Null.as_str().is_none());
        assert!(Json::Null.as_num().is_none());
        assert_eq!(parse("{}").unwrap(), Json::Obj(vec![]));
        assert_eq!(parse("[]").unwrap(), Json::Arr(vec![]));
        for bad in [
            "",
            "{",
            "[1,",
            "{\"a\" 1}",
            "{\"a\":1,}",
            "[1 2]",
            "\"\\x\"",
            "\"\\u12\"",
            "\"\\ud800x\"",
            "\"\\ud800\\u0041\"",
            "\"\u{1}\"",
            "01x",
            "-",
            "1.",
            "1e",
            "tru",
            "@",
            "1 2",
            "{1:2}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert!(parse(&deep).is_err());
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
    }

    /// REQ-NFR-007: the writer escapes and round-trips through the reader.
    #[test]
    fn req_nfr_007_json_quote_round_trip() {
        let s = "q\"b\\n\nr\rt\tc\u{1}\u{1f}e\u{e9}";
        let q = quote(s);
        assert!(!q.contains('\n'));
        assert_eq!(parse(&q).unwrap(), Json::Str(s.to_string()));
    }
}
