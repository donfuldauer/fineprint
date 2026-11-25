// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Step 3 of the tokenizer: the lexical shapes of structured lines (REQ-ING-004(d)). Every parser
//! takes one reminder-stripped, trimmed line and returns `None` for anything that does not match
//! exactly, so ambiguous or malformed lines fall back to plain abilities. The shapes are data
//! conventions of the bulk text (decision "#89 FP-087 — Line-structure lexical conventions").

/// The em dash, the structural separator of chapters, class levels, mode headers and anchors.
pub(super) const EM_DASH: char = '\u{2014}';

/// The mode bullet (CR 700.2).
pub(super) const BULLET: char = '\u{2022}';

/// The pawprint symbol that leads a mode instead of a bullet (CR 700.2i).
pub(super) const PAWPRINT: &str = "{P}";

/// Maximum digits in a station threshold, level-band bound or class level (same cap as the
/// layout unit's station check).
pub(super) const MAX_DIGITS: usize = 6;

/// Largest accepted chapter number.
pub(super) const MAX_CHAPTER: u32 = 99;

/// Longest accepted anchor prefix in bytes, and in words.
const MAX_ANCHOR_BYTES: usize = 64;
const MAX_ANCHOR_WORDS: usize = 6;

/// Parses 1..=[`MAX_DIGITS`] ASCII digits as a `u32`.
fn number(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > MAX_DIGITS || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Splits leading ASCII digits from `s`.
fn leading_digits(s: &str) -> (&str, &str) {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    s.split_at(n)
}

/// A station symbol line `N+ | text`, `N+|text` or `N+ |` (CR 722.2): the threshold and the text
/// after the bar (possibly empty).
pub(super) fn station(line: &str) -> Option<(u32, &str)> {
    let (digits, rest) = leading_digits(line);
    let n = number(digits)?;
    let rest = rest.strip_prefix('+')?.trim_start_matches(' ');
    let rest = rest.strip_prefix('|')?;
    Some((n, rest.trim()))
}

/// A level band header `LEVEL a-b` or `LEVEL a+` (CR 711.2), keyword case-insensitive, with
/// `a <= b`: `(min, max)`.
pub(super) fn level_band(line: &str) -> Option<(u32, Option<u32>)> {
    let head = line.get(..6)?;
    if !head.eq_ignore_ascii_case("level ") {
        return None;
    }
    let rest = line[6..].trim_start_matches(' ');
    let (a, rest) = leading_digits(rest);
    let min = number(a)?;
    if rest == "+" {
        return Some((min, None));
    }
    let b = rest.strip_prefix('-')?;
    let max = number(b)?;
    (min <= max).then_some((min, Some(max)))
}

/// A class level header `<cost>: Level N — <text>` (CR 717.2a): the level, the cost text and the
/// text after the em dash (possibly empty).
pub(super) fn class_level(line: &str) -> Option<(u32, &str, &str)> {
    let (cost, rest) = line.split_once(':')?;
    let cost = cost.trim();
    if cost.is_empty() || cost.contains(EM_DASH) {
        return None;
    }
    let rest = rest.trim_start_matches(' ');
    let head = rest.get(..6)?;
    if !head.eq_ignore_ascii_case("level ") {
        return None;
    }
    let (digits, rest) = leading_digits(rest[6..].trim_start_matches(' '));
    let level = number(digits).filter(|n| *n >= 1)?;
    let body = rest.trim_start_matches(' ').strip_prefix(EM_DASH)?;
    Some((level, cost, body.trim()))
}

/// Canonical Roman numeral of `n` in 1..=[`MAX_CHAPTER`].
fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 9] = [
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
        (0, ""),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while v > 0 && n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

/// Parses one canonical Roman numeral in 1..=[`MAX_CHAPTER`] (CR 715.2a); anything else
/// (non-canonical, lower case, too large, too long) is `None`.
pub(super) fn parse_roman(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 8 {
        return None;
    }
    let mut total: u32 = 0;
    let mut prev: u32 = 0;
    for c in s.chars().rev() {
        let v = match c {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            _ => return None,
        };
        if v < prev {
            total = total.checked_sub(v)?;
        } else {
            total = total.checked_add(v)?;
            prev = v;
        }
    }
    (1..=MAX_CHAPTER)
        .contains(&total)
        .then_some(total)
        .filter(|n| roman(*n) == s)
}

/// A chapter line `<numerals> — <effect>` (CR 715.2b, 715.2c): the strictly increasing chapter
/// numbers and the effect text (non-empty).
pub(super) fn chapter(line: &str) -> Option<(Vec<u32>, &str)> {
    let (head, effect) = line.split_once(EM_DASH)?;
    let effect = effect.trim();
    if effect.is_empty() {
        return None;
    }
    let mut out: Vec<u32> = Vec::new();
    for part in head.split(',') {
        let n = parse_roman(part.trim())?;
        if out.last().is_some_and(|last| *last >= n) {
            return None;
        }
        out.push(n);
    }
    Some((out, effect))
}

/// A leading ability-word or flavor-word prefix `<Word(s)> — <text>` (CR 207.2c, 207.2d): the
/// anchor and the text after it. The prefix starts with an upper-case letter, holds only letters,
/// spaces, apostrophes and hyphens, has at most a few words, and is not made only of Roman
/// numeral letters (that shape is a chapter or nothing).
pub(super) fn anchor(line: &str) -> Option<(&str, &str)> {
    let sep = " \u{2014} ";
    let at = line.find(sep)?;
    let (prefix, rest) = (&line[..at], line[at + sep.len()..].trim());
    if rest.is_empty()
        || prefix.len() > MAX_ANCHOR_BYTES
        || prefix.split(' ').count() > MAX_ANCHOR_WORDS
        || !prefix.chars().next().is_some_and(char::is_uppercase)
        || !prefix
            .chars()
            .all(|c| c.is_alphabetic() || c == ' ' || c == '\'' || c == '-')
        || prefix.chars().all(|c| "IVXLC ".contains(c))
    {
        return None;
    }
    Some((prefix, rest))
}

/// True for a modal header (CR 700.2): the line contains the word "choose" (any case). Whether it
/// opens a mode group also depends on the next lines; see [`option_text`].
pub(super) fn is_modal_header(line: &str) -> bool {
    line.split(|c: char| !super::mark::is_word_char(c))
        .any(|w| w.eq_ignore_ascii_case("choose"))
}

/// True when the header ends with an em dash.
pub(super) fn ends_with_dash(line: &str) -> bool {
    line.ends_with(EM_DASH)
}

/// The mode text of an option line: the text after the bullet of a bullet line (CR 700.2) when
/// `bullets_ok` (the header ends with an em dash), or the whole of a pawprint-led line
/// (CR 700.2i; the pawprints are the mode's cost and stay in the text). A cost before the effect
/// stays in the mode text (CR 700.2h). `None` for other lines or an empty mode.
pub(super) fn option_text(line: &str, bullets_ok: bool) -> Option<&str> {
    if let Some(r) = line.strip_prefix(BULLET) {
        let r = r.trim();
        return (bullets_ok && !r.is_empty()).then_some(r);
    }
    if line.starts_with(PAWPRINT) {
        let effect = line
            .trim_start_matches(PAWPRINT)
            .trim()
            .trim_start_matches(EM_DASH)
            .trim();
        return (!effect.is_empty()).then_some(line);
    }
    None
}

/// A bare power/toughness line such as `2/3` or `*/1+*` (kept as text, never a number).
pub(super) fn is_pt_line(line: &str) -> bool {
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 8
            && s.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '*' | '+' | '-' | 'X'))
    };
    line.split_once('/').is_some_and(|(p, t)| ok(p) && ok(t))
}
