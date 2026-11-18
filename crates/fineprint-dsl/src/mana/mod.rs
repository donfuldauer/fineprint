// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Structured mana costs (REQ-OBJ-001, SRS §4.1).
//!
//! A [`ManaCostDSL`] is an ordered, non-empty sequence of [`ManaSymbol`]s. "No mana cost" is not
//! a `ManaCostDSL` at all: it is `Option::None` (see [`ManaCostDSL::parse_optional`]), which is
//! different from the cost `{0}` (`Some` cost with the single symbol `Generic(0)`).
//!
//! Every symbol has exactly one canonical text spelling and the parser accepts only that
//! spelling, so `print(parse(s)) == s` for every accepted `s`. This unit does not compute mana
//! value.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(test)]
mod tests;

/// The largest generic amount accepted in `{n}` and `{n/c}`.
pub const MAX_GENERIC: u32 = 1_000_000;

/// One of the five colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Color {
    /// White, letter `W`.
    White,
    /// Blue, letter `U`.
    Blue,
    /// Black, letter `B`.
    Black,
    /// Red, letter `R`.
    Red,
    /// Green, letter `G`.
    Green,
}

impl Color {
    /// All colors in declaration order (W, U, B, R, G).
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ];

    /// The uppercase letter used in symbol text.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Color::White => 'W',
            Color::Blue => 'U',
            Color::Black => 'B',
            Color::Red => 'R',
            Color::Green => 'G',
        }
    }

    /// The color for an uppercase letter, or `None` (lowercase letters are not colors).
    #[must_use]
    pub const fn from_letter(letter: char) -> Option<Color> {
        match letter {
            'W' => Some(Color::White),
            'U' => Some(Color::Blue),
            'B' => Some(Color::Black),
            'R' => Some(Color::Red),
            'G' => Some(Color::Green),
            _ => None,
        }
    }
}

/// A variable amount symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Variable {
    /// `{X}`.
    X,
    /// `{Y}`.
    Y,
    /// `{Z}`.
    Z,
}

impl Variable {
    /// All variables in declaration order.
    pub const ALL: [Variable; 3] = [Variable::X, Variable::Y, Variable::Z];

    /// The uppercase letter used in symbol text.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Variable::X => 'X',
            Variable::Y => 'Y',
            Variable::Z => 'Z',
        }
    }
}

/// One mana symbol of SRS §4.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ManaSymbol {
    /// Generic mana `{n}`, `0 <= n <= MAX_GENERIC`.
    Generic(u32),
    /// Colored mana `{W}` `{U}` `{B}` `{R}` `{G}`.
    Colored(Color),
    /// Colorless mana `{C}`.
    Colorless,
    /// Snow mana `{S}`.
    Snow,
    /// `{X}`, `{Y}` or `{Z}`.
    Variable(Variable),
    /// Hybrid `{a/b}` with `a != b`, in the order written.
    Hybrid(Color, Color),
    /// Monocolored hybrid `{n/c}` with `1 <= n <= MAX_GENERIC`.
    GenericHybrid(u32, Color),
    /// Phyrexian `{c/P}`.
    Phyrexian(Color),
    /// Hybrid Phyrexian `{a/b/P}` with `a != b`.
    HybridPhyrexian(Color, Color),
}

impl ManaSymbol {
    /// Parses exactly one symbol, braces included (for example `{2/W}`). Leading or trailing
    /// text is an error.
    ///
    /// # Errors
    /// [`ManaParseError::Empty`] for `""`, [`ManaParseError::Malformed`] for bracket errors or
    /// trailing text, [`ManaParseError::UnknownSymbol`] for a braced text that is not a symbol.
    pub fn parse(text: &str) -> Result<ManaSymbol, ManaParseError> {
        let bytes = text.as_bytes();
        if bytes.is_empty() {
            return Err(ManaParseError::Empty);
        }
        let (symbol, next) = parse_symbol_at(text, 0)?;
        if next < bytes.len() {
            return Err(ManaParseError::Malformed { offset: next });
        }
        Ok(symbol)
    }

    /// Whether this symbol is one the parser accepts: `Generic(n)` with `n <= MAX_GENERIC`,
    /// `GenericHybrid(n, _)` with `1 <= n <= MAX_GENERIC`, `Hybrid` and `HybridPhyrexian` with
    /// two different colors; every other symbol is always valid. The parser and
    /// [`ManaCostDSL::new`] both use this rule, so they cannot disagree.
    #[must_use]
    pub fn is_valid(self) -> bool {
        match self {
            ManaSymbol::Generic(n) => n <= MAX_GENERIC,
            ManaSymbol::GenericHybrid(n, _) => (1..=MAX_GENERIC).contains(&n),
            ManaSymbol::Hybrid(a, b) | ManaSymbol::HybridPhyrexian(a, b) => a != b,
            ManaSymbol::Colored(_)
            | ManaSymbol::Colorless
            | ManaSymbol::Snow
            | ManaSymbol::Variable(_)
            | ManaSymbol::Phyrexian(_) => true,
        }
    }
}

impl fmt::Display for ManaSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ManaSymbol::Generic(n) => write!(f, "{{{n}}}"),
            ManaSymbol::Colored(c) => write!(f, "{{{}}}", c.letter()),
            ManaSymbol::Colorless => f.write_str("{C}"),
            ManaSymbol::Snow => f.write_str("{S}"),
            ManaSymbol::Variable(v) => write!(f, "{{{}}}", v.letter()),
            ManaSymbol::Hybrid(a, b) => write!(f, "{{{}/{}}}", a.letter(), b.letter()),
            ManaSymbol::GenericHybrid(n, c) => write!(f, "{{{n}/{}}}", c.letter()),
            ManaSymbol::Phyrexian(c) => write!(f, "{{{}/P}}", c.letter()),
            ManaSymbol::HybridPhyrexian(a, b) => {
                write!(f, "{{{}/{}/P}}", a.letter(), b.letter())
            }
        }
    }
}

impl FromStr for ManaSymbol {
    type Err = ManaParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ManaSymbol::parse(s)
    }
}

/// Why mana symbol text was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaParseError {
    /// The text is empty. An empty cost is not `{0}`; use [`ManaCostDSL::parse_optional`] for
    /// "no mana cost".
    Empty,
    /// The bracket structure is wrong: text outside braces, an unterminated `{`, a stray `}`, a
    /// nested `{` or an empty `{}`. `offset` is the byte offset of the first offending byte
    /// (for an unterminated or empty pair, the opening `{`).
    Malformed {
        /// Byte offset of the first offending byte.
        offset: usize,
    },
    /// Well-bracketed text that is not a known symbol.
    UnknownSymbol {
        /// Byte offset of the symbol's `{`.
        offset: usize,
        /// The symbol text, braces included.
        symbol: String,
    },
}

impl fmt::Display for ManaParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManaParseError::Empty => f.write_str("empty mana cost text (an empty cost is not {0})"),
            ManaParseError::Malformed { offset } => {
                write!(
                    f,
                    "malformed mana cost text: bad bracket structure at byte offset {offset}"
                )
            }
            ManaParseError::UnknownSymbol { offset, symbol } => {
                write!(f, "unknown mana symbol {symbol:?} at byte offset {offset}")
            }
        }
    }
}

impl std::error::Error for ManaParseError {}

/// A structured mana cost: a non-empty ordered sequence of symbols (REQ-OBJ-001).
///
/// The JSON form is a string equal to the canonical text, for example `"{2}{W}{W}"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ManaCostDSL(Vec<ManaSymbol>);

impl ManaCostDSL {
    /// Builds a cost from symbols, keeping their order. Every symbol must satisfy the parser's
    /// rules ([`ManaSymbol::is_valid`]), so every constructed cost prints as text that
    /// [`ManaCostDSL::parse`] accepts and maps back to the same cost.
    ///
    /// # Errors
    /// [`ManaParseError::Empty`] if `symbols` is empty; [`ManaParseError::UnknownSymbol`] for
    /// the first invalid symbol (for example `GenericHybrid(0, _)` or `Hybrid(c, c)`), with
    /// `offset` its byte offset in the canonical text of the cost and `symbol` its text.
    pub fn new(symbols: Vec<ManaSymbol>) -> Result<ManaCostDSL, ManaParseError> {
        if symbols.is_empty() {
            return Err(ManaParseError::Empty);
        }
        let mut offset = 0usize;
        for symbol in &symbols {
            let text = symbol.to_string();
            if !symbol.is_valid() {
                return Err(ManaParseError::UnknownSymbol {
                    offset,
                    symbol: text,
                });
            }
            offset += text.len();
        }
        Ok(ManaCostDSL(symbols))
    }

    /// The symbols in written order; nothing is sorted or merged.
    #[must_use]
    pub fn symbols(&self) -> &[ManaSymbol] {
        &self.0
    }

    /// Parses concatenated symbols such as `{2}{W}{W}`.
    ///
    /// # Errors
    /// [`ManaParseError::Empty`] for `""` (an empty cost is not `{0}`), otherwise the first
    /// [`ManaParseError::Malformed`] or [`ManaParseError::UnknownSymbol`] found left to right.
    pub fn parse(text: &str) -> Result<ManaCostDSL, ManaParseError> {
        if text.is_empty() {
            return Err(ManaParseError::Empty);
        }
        let mut symbols = Vec::new();
        let mut at = 0usize;
        while at < text.len() {
            let (symbol, next) = parse_symbol_at(text, at)?;
            symbols.push(symbol);
            at = next;
        }
        Ok(ManaCostDSL(symbols))
    }

    /// Ingestion form (SRS §16): `""` means "no mana cost" (`Ok(None)`); anything else is
    /// parsed with [`ManaCostDSL::parse`]. `"{0}"` gives `Some` cost with `Generic(0)`.
    ///
    /// # Errors
    /// As [`ManaCostDSL::parse`], for non-empty text.
    pub fn parse_optional(text: &str) -> Result<Option<ManaCostDSL>, ManaParseError> {
        if text.is_empty() {
            Ok(None)
        } else {
            ManaCostDSL::parse(text).map(Some)
        }
    }

    /// Inverse of [`ManaCostDSL::parse_optional`]: `None` prints as `""`.
    #[must_use]
    pub fn print_optional(cost: Option<&ManaCostDSL>) -> String {
        cost.map_or_else(String::new, ToString::to_string)
    }
}

impl fmt::Display for ManaCostDSL {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for symbol in &self.0 {
            fmt::Display::fmt(symbol, f)?;
        }
        Ok(())
    }
}

impl FromStr for ManaCostDSL {
    type Err = ManaParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ManaCostDSL::parse(s)
    }
}

impl Serialize for ManaCostDSL {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ManaCostDSL {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ManaCostDSL::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Parses the symbol whose `{` is expected at byte `at`; returns it and the offset just past its
/// `}`. All offsets are of ASCII bytes, so every slice is on a char boundary.
fn parse_symbol_at(text: &str, at: usize) -> Result<(ManaSymbol, usize), ManaParseError> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'{') {
        return Err(ManaParseError::Malformed { offset: at });
    }
    let mut i = at + 1;
    let close = loop {
        match bytes.get(i) {
            None => return Err(ManaParseError::Malformed { offset: at }),
            Some(b'{') => return Err(ManaParseError::Malformed { offset: i }),
            Some(b'}') => break i,
            Some(_) => i += 1,
        }
    };
    if close == at + 1 {
        return Err(ManaParseError::Malformed { offset: at });
    }
    let inner = text.get(at + 1..close).unwrap_or("");
    match symbol_from_inner(inner) {
        Some(symbol) => Ok((symbol, close + 1)),
        None => Err(ManaParseError::UnknownSymbol {
            offset: at,
            symbol: text.get(at..=close).unwrap_or("").to_owned(),
        }),
    }
}

/// The symbol for the text between the braces, if it is exactly a canonical spelling.
/// Every shape is first mapped to its candidate symbol and then filtered by
/// [`ManaSymbol::is_valid`], the same rule [`ManaCostDSL::new`] applies.
fn symbol_from_inner(inner: &str) -> Option<ManaSymbol> {
    let mut parts = inner.split('/');
    let candidate = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(one), None, None, None) => single(one),
        (Some(a), Some("P"), None, None) => color(a).map(ManaSymbol::Phyrexian),
        (Some(a), Some(b), None, None) => match (generic(a), color(a), color(b)) {
            (Some(n), _, Some(c)) => Some(ManaSymbol::GenericHybrid(n, c)),
            (_, Some(x), Some(y)) => Some(ManaSymbol::Hybrid(x, y)),
            _ => None,
        },
        (Some(a), Some(b), Some("P"), None) => match (color(a), color(b)) {
            (Some(x), Some(y)) => Some(ManaSymbol::HybridPhyrexian(x, y)),
            _ => None,
        },
        _ => None,
    };
    candidate.filter(|symbol| symbol.is_valid())
}

/// A one-part symbol: generic number, color, `C`, `S`, or a variable.
fn single(text: &str) -> Option<ManaSymbol> {
    if let Some(n) = generic(text) {
        return Some(ManaSymbol::Generic(n));
    }
    if let Some(c) = color(text) {
        return Some(ManaSymbol::Colored(c));
    }
    match text {
        "C" => Some(ManaSymbol::Colorless),
        "S" => Some(ManaSymbol::Snow),
        "X" => Some(ManaSymbol::Variable(Variable::X)),
        "Y" => Some(ManaSymbol::Variable(Variable::Y)),
        "Z" => Some(ManaSymbol::Variable(Variable::Z)),
        _ => None,
    }
}

/// A color given as exactly one uppercase letter.
fn color(text: &str) -> Option<Color> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Color::from_letter(c),
        _ => None,
    }
}

/// A canonical decimal number `0..=MAX_GENERIC`: ASCII digits, no sign, no leading zero except
/// `0` itself.
fn generic(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 7 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes.first() == Some(&b'0') {
        return None;
    }
    let n = bytes
        .iter()
        .fold(0u32, |acc, d| acc * 10 + u32::from(d - b'0'));
    (n <= MAX_GENERIC).then_some(n)
}
