// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The CR text as an ordered list of rules, plus the text helpers the vocabulary parsers share
//! (sentences, parentheticals, lists).
//!
//! The rules body is the text between the LAST line equal to [`START_MARKER`] and the LAST line
//! equal to [`END_MARKER`] (the file opens with a table of contents that repeats both), as in
//! `cargo xtask cr`. A rule starts at a line `ddd.n[x[x]][.] `; its text continues over the
//! following non-blank lines up to a blank line or the next rule start. A section heading
//! `ddd. Title` ends the previous rule. `Example:` lines are not part of a rule's text here.

use super::GenError;

/// The line that starts the rules body (its last occurrence).
pub const START_MARKER: &str = "1. Game Concepts";
/// The line that ends the rules body (its last occurrence).
pub const END_MARKER: &str = "Glossary";
/// The phrase that introduces the effective date in the CR header.
pub const EFFECTIVE_PHRASE: &str = "effective as of ";

/// One numbered rule of the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Section digits (`205`).
    pub section: String,
    /// Rule digits (`3`).
    pub number: String,
    /// Letter suffix (`g`), empty for an unlettered rule.
    pub letters: String,
    /// The rule's first line after its number.
    pub first_line: String,
    /// How many continuation lines (not `Example:` lines) followed the first line.
    pub continuation_lines: usize,
    /// First line plus continuation lines, joined with single spaces.
    pub text: String,
}

impl Rule {
    /// `section.number` (`205.3`).
    #[must_use]
    pub fn base(&self) -> String {
        format!("{}.{}", self.section, self.number)
    }

    /// The full rule number (`205.3g`).
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}.{}{}", self.section, self.number, self.letters)
    }
}

/// The decoded CR document.
#[derive(Debug, Clone)]
pub struct CrDoc {
    /// Every line of the file, LF-normalised, BOM stripped.
    pub lines: Vec<String>,
    /// The rules of the body, in file order.
    pub rules: Vec<Rule>,
}

/// Decodes the CR bytes: strips a BOM, rejects invalid UTF-8, treats CRLF, CR and LF alike.
///
/// # Errors
/// [`GenError::Input`] for invalid UTF-8 or a missing marker.
pub fn parse_doc(bytes: &[u8]) -> Result<CrDoc, GenError> {
    let text = std::str::from_utf8(bytes).map_err(|e| {
        GenError::Input(format!(
            "the CR text is not valid UTF-8 (byte {})",
            e.valid_up_to()
        ))
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let last = |marker: &str| lines.iter().rposition(|l| l.trim() == marker);
    let start = last(START_MARKER).ok_or_else(|| {
        GenError::Input(format!(
            "missing START marker: no line `{START_MARKER}` in the CR text"
        ))
    })?;
    let end = last(END_MARKER).ok_or_else(|| {
        GenError::Input(format!(
            "missing END marker: no line `{END_MARKER}` in the CR text"
        ))
    })?;
    if end <= start {
        return Err(GenError::Input(format!(
            "missing END marker: the last `{END_MARKER}` line precedes the last `{START_MARKER}` line"
        )));
    }
    let mut rules = Vec::new();
    let mut current: Option<(Rule, Vec<String>)> = None;
    let finish = |cur: Option<(Rule, Vec<String>)>, rules: &mut Vec<Rule>| {
        if let Some((mut rule, parts)) = cur {
            rule.continuation_lines = parts.len() - 1;
            rule.text = parts.join(" ");
            rules.push(rule);
        }
    };
    for line in &lines[start + 1..end] {
        let line = line.trim_end();
        if line.trim().is_empty() {
            finish(current.take(), &mut rules);
        } else if let Some(start) = rule_start(line) {
            finish(current.take(), &mut rules);
            if let Some(rule) = start {
                let first = rule.first_line.clone();
                current = Some((rule, vec![first]));
            }
        } else if let Some((_, parts)) = current.as_mut()
            && !line.trim_start().starts_with("Example:")
        {
            parts.push(line.trim().to_owned());
        }
    }
    finish(current.take(), &mut rules);
    Ok(CrDoc { lines, rules })
}

/// `None` when `line` starts no rule; `Some(None)` for a section heading `ddd. Title`;
/// `Some(Some(rule))` for a numbered rule.
fn rule_start(line: &str) -> Option<Option<Rule>> {
    let b = line.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    if !(at(0).is_ascii_digit() && at(1).is_ascii_digit() && at(2).is_ascii_digit())
        || at(3) != b'.'
    {
        return None;
    }
    if at(4) == b' ' {
        return if line[5..].trim().is_empty() {
            None
        } else {
            Some(None)
        };
    }
    let mut i = 4;
    while at(i).is_ascii_digit() {
        i += 1;
    }
    if i == 4 {
        return None;
    }
    let digits_end = i;
    while at(i).is_ascii_lowercase() && i < digits_end + 2 {
        i += 1;
    }
    let letters_end = i;
    if at(i) == b'.' {
        i += 1;
    }
    if at(i) != b' ' {
        return None;
    }
    Some(Some(Rule {
        section: line[..3].to_owned(),
        number: line[4..digits_end].to_owned(),
        letters: line[digits_end..letters_end].to_owned(),
        first_line: line[i + 1..].trim().to_owned(),
        continuation_lines: 0,
        text: String::new(),
    }))
}

impl CrDoc {
    /// The rule numbered `id` (`205.2a`).
    ///
    /// # Errors
    /// [`GenError::Input`] when the rule is missing or appears more than once.
    pub fn require(&self, id: &str) -> Result<&Rule, GenError> {
        let mut found = self.rules.iter().filter(|r| r.id() == id);
        let first = found
            .next()
            .ok_or_else(|| GenError::Input(format!("missing required rule {id} in the CR text")))?;
        if found.next().is_some() {
            return Err(GenError::Input(format!(
                "rule {id} appears more than once in the CR text"
            )));
        }
        Ok(first)
    }

    /// The lettered sub-rules of `base` (`205.3` gives 205.3a, 205.3b, ...) in file order.
    ///
    /// # Errors
    /// [`GenError::Input`] when a sub-rule number repeats.
    pub fn children(&self, base: &str) -> Result<Vec<&Rule>, GenError> {
        let out: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| !r.letters.is_empty() && r.base() == base)
            .collect();
        for (i, r) in out.iter().enumerate() {
            if out[..i].iter().any(|o| o.letters == r.letters) {
                return Err(GenError::Input(format!(
                    "rule {} appears more than once in the CR text",
                    r.id()
                )));
            }
        }
        Ok(out)
    }

    /// The effective date from the first line containing [`EFFECTIVE_PHRASE`], as `YYYY-MM-DD`.
    ///
    /// # Errors
    /// [`GenError::Input`] when no such line exists or its date does not parse.
    pub fn effective_date(&self) -> Result<String, GenError> {
        let line = self
            .lines
            .iter()
            .find(|l| l.contains(EFFECTIVE_PHRASE))
            .ok_or_else(|| {
                GenError::Input(
                    "missing effective-date line (`... effective as of <Month D, YYYY>.`)"
                        .to_owned(),
                )
            })?;
        let after = line
            .split_once(EFFECTIVE_PHRASE)
            .map_or("", |(_, rest)| rest);
        parse_long_date(after).ok_or_else(|| {
            GenError::Input("the effective-date line has no `<Month D, YYYY>` date".to_owned())
        })
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `November 6, 2026.` -> `2026-11-06`.
fn parse_long_date(s: &str) -> Option<String> {
    let mut words = s.split_whitespace();
    let month = words.next()?;
    let m = MONTHS.iter().position(|n| *n == month)? + 1;
    let day = words.next()?.strip_suffix(',')?;
    let year = words.next()?;
    let year = year.strip_suffix('.').unwrap_or(year);
    let all_digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(day) || day.len() > 2 || !all_digits(year) || year.len() != 4 {
        return None;
    }
    let d: u32 = day.parse().ok()?;
    let y: u32 = year.parse().ok()?;
    let month_days = fineprint_dsl::vocab::days_in_month(y, u32::try_from(m).ok()?);
    if d == 0 || d > month_days {
        return None;
    }
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Removes parenthetical text (with the space before it): `A (see rule 1), B` -> `A, B`.
///
/// `None` for unbalanced parentheses.
pub fn strip_parentheticals(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => {
                if depth == 0 {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                }
                depth += 1;
            }
            ')' => depth = depth.checked_sub(1)?,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(out)
}

/// Quote-aware sentence split. A sentence ends at a `.` outside quotation marks followed by
/// whitespace or the end (the `.` is dropped), or at a closing quotation mark that follows a
/// `.` (optionally followed by closing single quotation marks) and is followed by whitespace or the end (the mark is kept). Curly double quotes nest;
/// a straight double quote toggles.
///
/// `None` for unbalanced quotation marks.
pub fn sentences(s: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut straight = false;
    let next_is_end = |i: usize| chars.get(i + 1).is_none_or(|c| c.is_whitespace());
    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '\u{201c}' => {
                depth += 1;
                cur.push(ch);
            }
            '\u{201d}' => {
                depth = depth.checked_sub(1)?;
                cur.push(ch);
                if depth == 0 && !straight && ends_with_period(&chars[..i]) && next_is_end(i) {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '"' => {
                straight = !straight;
                cur.push(ch);
                if !straight && depth == 0 && ends_with_period(&chars[..i]) && next_is_end(i) {
                    out.push(std::mem::take(&mut cur));
                }
            }
            '.' if depth == 0 && !straight && next_is_end(i) => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    if depth != 0 || straight {
        return None;
    }
    out.push(cur);
    Some(
        out.into_iter()
            .map(|x| x.trim().to_owned())
            .filter(|x| !x.is_empty())
            .collect(),
    )
}

/// True when `before` ends with `.`, possibly followed by closing single quotation marks
/// (a nested quote that ends with its own period, as in `.’”`).
fn ends_with_period(before: &[char]) -> bool {
    before
        .iter()
        .rev()
        .find(|c| !matches!(c, '\u{2019}' | '\''))
        .is_some_and(|c| *c == '.')
}

/// Splits a CR list (`A, B, and C`; `A and B`; `A`) into its items. Trailing qualifier items
/// that begin with `as well as` are dropped. Items must look like names.
///
/// `None` when an item is empty or does not look like a name.
pub fn split_list(s: &str) -> Option<Vec<String>> {
    let s = s.trim();
    let mut items: Vec<String> = if s.contains(',') {
        s.split(',').map(|i| i.trim().to_owned()).collect()
    } else if let Some((a, b)) = s.split_once(" and ") {
        vec![a.trim().to_owned(), format!("and {}", b.trim())]
    } else {
        vec![s.to_owned()]
    };
    while items.last().is_some_and(|i| i.starts_with("as well as ")) {
        items.pop();
    }
    let several = items.len() > 1;
    if let Some(last) = items.last_mut()
        && several
    {
        for conj in ["and ", "or "] {
            if let Some(rest) = last.strip_prefix(conj) {
                *last = rest.trim().to_owned();
                break;
            }
        }
    }
    if items.is_empty() || !items.iter().all(|i| looks_like_name(i)) {
        return None;
    }
    Some(items)
}

/// A list item that is a plausible name: non-empty, at most 6 words and 64 characters, and no
/// sentence punctuation or quotation marks.
#[must_use]
pub fn looks_like_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= 64
        && s.split_whitespace().count() <= 6
        && !s.contains(|c: char| {
            matches!(
                c,
                '.' | ':' | ';' | '(' | ')' | '"' | '\u{201c}' | '\u{201d}' | ',' | '!' | '?'
            ) || c.is_control()
        })
}

/// Upper-cases the first character, leaving the rest unchanged.
#[must_use]
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
