// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Rule numbers, the Comprehensive Rules text parser and rule-text normalization
//! (REQ-NFR-007).
//!
//! # Rule numbers
//!
//! A rule number is a section (`ddd`) or a rule (`ddd.n`, `ddd.nx` or `ddd.nxx`: three digits,
//! a dot, one or more digits, then zero to two lowercase letters). Two letters occur in the
//! fetched text (for example a sub-rule that follows the `z` sub-rule); the SRS grammar uses at
//! most one. [`RuleNum`] orders by section number, then rule number, then letter suffix (shorter
//! suffix first, so `z` sorts before `aa`).
//!
//! # Parsed region
//!
//! The official file starts with a table of contents that repeats the chapter and section heading
//! lines, so the parser reads only the lines strictly between the LAST line equal to
//! [`START_MARKER`] and the LAST line equal to [`END_MARKER`] (the glossary that follows the rules).
//! Lines are compared after trimming. Either marker missing, or the end before the start, is an
//! "unrecognised CR layout" error.
//!
//! # Rule extent
//!
//! A rule starts at a line that begins with a rule number followed by an optional dot and a space
//! (`ddd.n[x[x]][.] `), or a section heading (`ddd. ` and a title). A rule's text is its first
//! line (number included) plus every following non-blank line up to the next blank line or the
//! next rule start (the fetched text has one place where a section heading follows the previous
//! rule without a blank line). `Example:` lines are part of the text, so editing an example
//! re-flags the rule (intended, conservative). A section heading's text is the heading line alone.
//! When a number repeats inside the region, the later entry wins and is counted as a duplicate.
//!
//! # Normalization (`sha256-normalized-v1`)
//!
//! [`normalize`]: strip a leading byte-order mark, then replace every run of Unicode whitespace
//! (which includes CR and LF, so CRLF and CR line endings count as LF) with a single ASCII space
//! and trim both ends. Nothing else changes: no Unicode normalization, quotes and dashes are kept
//! byte for byte. The rule hash is the lowercase hex SHA-256 of the UTF-8 bytes of the normalized
//! text alone.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::sha256::sha256_hex;

/// The line that starts the rules body (its last occurrence; the first is in the contents).
pub const START_MARKER: &str = "1. Game Concepts";
/// The line that ends the rules body (its last occurrence; the first is in the contents).
pub const END_MARKER: &str = "Glossary";

/// A rule number: a section (`ddd`) or a rule (`ddd.n` with an optional letter suffix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleNum(String);

impl RuleNum {
    /// Parse a complete rule number; `None` unless `s` is exactly rule-shaped.
    pub fn parse(s: &str) -> Option<RuleNum> {
        let b = s.as_bytes();
        if b.len() < 3 || !b[..3].iter().all(u8::is_ascii_digit) {
            return None;
        }
        if b.len() == 3 {
            return Some(RuleNum(s.to_string()));
        }
        if b[3] != b'.' {
            return None;
        }
        let mut i = 4;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == 4 {
            return None;
        }
        let letters = &b[i..];
        if letters.len() > 2 || !letters.iter().all(u8::is_ascii_lowercase) {
            return None;
        }
        Some(RuleNum(s.to_string()))
    }

    /// The rule number as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// (section digits, rule digits, letters); the last two are empty for a section.
    fn parts(&self) -> (&str, &str, &str) {
        let s = self.0.as_str();
        let section = s.get(..3).unwrap_or(s);
        let rest = s.get(4..).unwrap_or("");
        let split = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        (section, &rest[..split], &rest[split..])
    }

    /// The rule without its letter suffix (`116.2a` -> `116.2`).
    pub fn base(&self) -> String {
        let (section, num, _) = self.parts();
        if num.is_empty() {
            section.to_string()
        } else {
            format!("{section}.{num}")
        }
    }

    /// True for a rule with a dot (not a bare section).
    pub fn is_dotted(&self) -> bool {
        self.0.len() > 3
    }
}

/// Compare digit strings numerically (no leading-zero handling needed beyond length).
fn cmp_digits(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl Ord for RuleNum {
    fn cmp(&self, other: &Self) -> Ordering {
        let (s1, n1, l1) = self.parts();
        let (s2, n2, l2) = other.parts();
        cmp_digits(s1, s2)
            .then_with(|| self.is_dotted().cmp(&other.is_dotted()))
            .then_with(|| cmp_digits(n1, n2))
            .then_with(|| l1.len().cmp(&l2.len()))
            .then_with(|| l1.cmp(l2))
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for RuleNum {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for RuleNum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Normalize rule text (see the module documentation, `sha256-normalized-v1`).
pub fn normalize(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    for word in text.split(char::is_whitespace).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// The hash of a rule's text: lowercase hex SHA-256 of the normalized text.
pub fn rule_hash(text: &str) -> String {
    sha256_hex(normalize(text).as_bytes())
}

/// The parsed rules body: rule number -> hash of the normalized text.
#[derive(Debug, Default)]
pub struct CrRules {
    /// Rule number -> lowercase hex SHA-256 of the normalized rule text.
    pub hashes: BTreeMap<RuleNum, String>,
    /// How many rule numbers appeared more than once in the region (later entry kept).
    pub duplicates: usize,
}

/// If `line` starts a rule, return its number and whether it is a section heading.
pub fn rule_start(line: &str) -> Option<(RuleNum, bool)> {
    let b = line.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    if !(at(0).is_ascii_digit()
        && at(1).is_ascii_digit()
        && at(2).is_ascii_digit()
        && at(3) == b'.')
    {
        return None;
    }
    if at(4) == b' ' {
        // section heading: `ddd. Title`
        let title = line.get(5..).unwrap_or("").trim();
        return if title.is_empty() {
            None
        } else {
            Some((RuleNum(line[..3].to_string()), true))
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
    let end = i;
    if at(i) == b'.' {
        i += 1;
    }
    if at(i) != b' ' {
        return None;
    }
    RuleNum::parse(&line[..end]).map(|n| (n, false))
}

/// Parse the CR text into rule hashes. Errors describe an unrecognised layout.
pub fn parse_cr(text: &str) -> Result<CrRules, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let last = |marker: &str| lines.iter().rposition(|l| l.trim() == marker);
    let (Some(start), Some(end)) = (last(START_MARKER), last(END_MARKER)) else {
        return Err(format!(
            "unrecognised CR layout: expected lines `{START_MARKER}` and `{END_MARKER}`"
        ));
    };
    if end <= start {
        return Err(format!(
            "unrecognised CR layout: the last `{END_MARKER}` line precedes the last `{START_MARKER}` line"
        ));
    }
    let mut out = CrRules::default();
    let mut current: Option<(RuleNum, bool, Vec<&str>)> = None;
    fn finish(cur: Option<(RuleNum, bool, Vec<&str>)>, out: &mut CrRules) {
        if let Some((num, _, body)) = cur {
            let hash = rule_hash(&body.join("\n"));
            if out.hashes.insert(num, hash).is_some() {
                out.duplicates += 1;
            }
        }
    }
    for line in &lines[start + 1..end] {
        if line.trim().is_empty() {
            finish(current.take(), &mut out);
        } else if let Some((num, heading)) = rule_start(line) {
            finish(current.take(), &mut out);
            current = Some((num, heading, vec![line]));
        } else if let Some((_, false, body)) = current.as_mut() {
            body.push(line);
        }
    }
    finish(current.take(), &mut out);
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a synthetic CR text (invented placeholder prose only) with a BOM, CRLF line
    /// endings, a contents block that repeats the headings, both markers twice, a lettered
    /// sub-rule, an `Example:` continuation, a duplicate number and a glossary that mentions a
    /// rule number mid-line. `extra` adds `(number, prose)` entries to the body.
    pub(crate) fn synthetic_cr(extra: &[(&str, &str)]) -> String {
        let s = |n: u32, rest: &str| format!("{n}{rest}");
        let mut lines: Vec<String> = vec![
            "\u{feff}Synthetic Comprehensive Rules".to_string(),
            String::new(),
            "These rules are effective as of November 6, 2026.".to_string(),
            String::new(),
            "Contents".to_string(),
            String::new(),
            START_MARKER.to_string(),
            s(100, ". Alpha Section"),
            s(101, ". Beta Section"),
            END_MARKER.to_string(),
            String::new(),
            START_MARKER.to_string(),
            String::new(),
            s(100, ". Alpha Section"),
            String::new(),
            s(100, ".1. Synthetic rule alpha zephyr text."),
            String::new(),
            s(100, ".1a Synthetic sub rule"),
            "Example: Synthetic example quokka line.".to_string(),
            String::new(),
            s(100, ".2. First copy of rule two."),
            String::new(),
            s(101, ". Beta Section"),
            s(101, ".1. Rule after heading without blank."),
            String::new(),
            s(100, ".2. Second copy of rule two."),
            String::new(),
        ];
        for (n, prose) in extra {
            let dot = if n.contains('.') { "" } else { "." };
            lines.push(format!("{n}{dot} {prose}"));
            lines.push(String::new());
        }
        lines.push(END_MARKER.to_string());
        lines.push(String::new());
        lines.push(format!("Term One{}", ""));
        lines.push(format!("Mentioned in rule {}.9z of nothing.", 100));
        lines.push(String::new());
        lines.push("Credits".to_string());
        lines.join("\r\n")
    }

    fn hash_of(rules: &CrRules, n: &str) -> Option<String> {
        rules.hashes.get(&RuleNum::parse(n).unwrap()).cloned()
    }

    /// REQ-NFR-007: rule set, extents, duplicates and the glossary boundary.
    #[test]
    fn req_nfr_007_parse_synthetic_cr() {
        let rules = parse_cr(&synthetic_cr(&[])).unwrap();
        let keys: Vec<&str> = rules.hashes.keys().map(RuleNum::as_str).collect();
        assert_eq!(keys, ["100", "100.1", "100.1a", "100.2", "101", "101.1"]);
        assert_eq!(rules.duplicates, 1);
        assert_eq!(
            hash_of(&rules, "100.2").unwrap(),
            rule_hash(&format!("{}.2. Second copy of rule two.", 100))
        );
        assert_eq!(
            hash_of(&rules, "100.1a").unwrap(),
            rule_hash(&format!(
                "{}.1a Synthetic sub rule\nExample: Synthetic example quokka line.",
                100
            ))
        );
        assert_eq!(
            hash_of(&rules, "101").unwrap(),
            rule_hash(&format!("{}. Beta Section", 101))
        );
        assert_eq!(
            hash_of(&rules, "101.1").unwrap(),
            rule_hash(&format!("{}.1. Rule after heading without blank.", 101))
        );
        // the glossary mention after the end marker is not a rule
        assert!(hash_of(&rules, "100.9z").is_none());
    }

    /// REQ-NFR-007: missing markers are an unrecognised layout.
    #[test]
    fn req_nfr_007_parse_missing_markers() {
        let text = synthetic_cr(&[]);
        assert!(parse_cr(&text.replace(END_MARKER, "Glossar")).is_err());
        assert!(parse_cr(&text.replace(START_MARKER, "1. Concepts")).is_err());
        assert!(parse_cr("").is_err());
        let reversed = format!("{END_MARKER}\n{START_MARKER}\n");
        assert!(parse_cr(&reversed).is_err());
    }

    /// REQ-NFR-007: two-letter sub-rules, headings and non-rule lines.
    #[test]
    fn req_nfr_007_rule_start_shapes() {
        let start = |l: String| rule_start(&l).map(|(n, h)| (n.as_str().to_string(), h));
        assert_eq!(
            start(format!("{}.5aa Text", 704)),
            Some((format!("{}.5aa", 704), false))
        );
        assert_eq!(
            start(format!("{}.12. Text", 702)),
            Some((format!("{}.12", 702), false))
        );
        assert_eq!(start(format!("{}. Title", 702)), Some(("702".into(), true)));
        assert_eq!(start(format!("{}. ", 702)), None);
        assert_eq!(start("1. Game Concepts".into()), None);
        assert_eq!(start(format!("{}.5abc Text", 704)), None);
        assert_eq!(start(format!("{}.x Text", 704)), None);
        assert_eq!(start(format!("{}.5", 704)), None);
        assert_eq!(start(format!("{}.5aText", 704)), None);
    }

    /// REQ-NFR-007: whitespace, CRLF, BOM and trailing spaces do not change the hash; a
    /// one-word change does; example lines are part of the hashed text.
    #[test]
    fn req_nfr_007_normalization() {
        let base = "Synthetic  rule\ttext\r\nExample: synthetic example.";
        let h = rule_hash(base);
        assert_eq!(
            rule_hash("\u{feff}Synthetic rule text\nExample: synthetic example.   "),
            h
        );
        assert_eq!(
            rule_hash("  Synthetic\u{a0}rule\r\ntext \r Example:\u{2003}synthetic example."),
            h
        );
        assert_ne!(
            rule_hash("Synthetic rule text\nExample: synthetic sample."),
            h
        );
        assert_ne!(rule_hash("Synthetic rule text"), h);
        assert_eq!(
            normalize("a\u{2014}b \u{201c}q\u{201d}"),
            "a\u{2014}b \u{201c}q\u{201d}"
        );
        assert_eq!(normalize(" \r\n "), "");
        // an edited example inside a parsed CR changes that rule's hash only
        let a = parse_cr(&synthetic_cr(&[])).unwrap();
        let b = parse_cr(&synthetic_cr(&[]).replace("quokka", "wombat")).unwrap();
        for (k, v) in &a.hashes {
            let changed = b.hashes.get(k) != Some(v);
            assert_eq!(changed, k.as_str() == "100.1a", "rule {k}");
        }
    }

    /// REQ-NFR-007: ordering by section, rule number, then letters.
    #[test]
    fn req_nfr_007_rule_order() {
        let mut v: Vec<RuleNum> = [
            "101.2", "100.10", "100.2", "100", "100.2b", "100.2aa", "100.2z", "99", "100.2a",
        ]
        .iter()
        .filter_map(|s| RuleNum::parse(s))
        .collect();
        v.sort();
        let s: Vec<&str> = v.iter().map(RuleNum::as_str).collect();
        assert_eq!(
            s,
            [
                "100", "100.2", "100.2a", "100.2b", "100.2z", "100.2aa", "100.10", "101.2"
            ]
        );
        assert_eq!(RuleNum::parse("116.2a").unwrap().base(), "116.2");
        assert_eq!(RuleNum::parse("116").unwrap().base(), "116");
        assert!(RuleNum::parse("116.").is_none());
        assert!(RuleNum::parse("116x").is_none());
        assert!(RuleNum::parse("116.2A").is_none());
        assert_eq!(RuleNum::parse("116.2").unwrap().to_string(), "116.2");
    }
}
