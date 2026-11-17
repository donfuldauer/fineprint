// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Citation scanner: finds CR citations in `docs/SRS.md` and in source comments (REQ-DOC-001).
//!
//! # Sources
//!
//! `docs/SRS.md`, plus every listed file with extension `.rs .cs .c .cpp .h .ts .js .py .toml
//! .yml .yaml`, except paths under `xtask/src/cr/` (this module's own synthetic fixtures and
//! examples, which deliberately contain fabricated citations) and paths with a `target/`
//! component. A file that is not valid UTF-8 is read lossily.
//!
//! # Grammar
//!
//! A rule number is a section `ddd` or a rule `ddd.n[x[x]]` (see [`super::rules::RuleNum`]).
//!
//! - **CR lists** (SRS and comments): after the word `CR` or `CRs` followed by one space (the
//!   word not preceded by a letter, digit or `_`), a list of items separated by `,`, `;`, `/`
//!   or `and` (`, and` too). An item is a rule number or a range `A–B` / `A-B` (en dash or
//!   hyphen, no spaces) where `B` is a rule number or, when `A` has a dot, a bare rule number
//!   in A's section (`702.178–179` means `702.178` to `702.179`) or one or two bare letters
//!   (`116.2a–m` means `116.2a` to `116.2m`). Only the two endpoints of a range are cited. A dash with spaces (`CR 113.6 – default`) ends the list, and `CR-2026-11-06` is
//!   not a list. Bare sections (`101`) count only inside such a list.
//! - **Bare dotted tokens** (SRS only): every `ddd.n[x[x]]` anywhere in the SRS, not preceded by
//!   a letter, digit or `.`, and not followed by a letter, digit, `_`, or `.` plus a digit (so a
//!   version such as `192.168.1.1` is not a citation). This picks up table cells such as
//!   `712.3, 712.6e` and parentheses such as `(702.191)`.
//!
//! Source comments use only the CR-list grammar (the R6 citation style `// CR 613.8b`), so
//! numbers in ordinary comments are not mistaken for citations.
//!
//! # Comment extraction (lexical, best effort)
//!
//! `.rs .cs .c .cpp .h .ts .js`: text after `//` (so `///` and `//!` too) and inside `/* */`
//! (nested, as in Rust) outside string and character literals. String literals `"..."` with
//! backslash escapes, Rust raw strings `r"..."`/`r#"..."#`, and backtick template strings in
//! `.ts`/`.js` are skipped; `'x'` and `'\n'` are character literals, any other `'` (a Rust
//! lifetime) is ordinary code. `.py .toml .yml .yaml`: text after a `#` that is outside a
//! single-line `'...'` or `"..."` string.
//!
//! Not handled: Python triple-quoted strings and YAML block scalars (their `#` is read as a
//! comment), TOML multi-line strings, an apostrophe in an unquoted YAML value before a `#`
//! comment (the comment is then missed), C++ raw strings, C# verbatim strings with `""`
//! escapes, and byte or C string prefixes other than Rust's `r`. A citation split across two
//! lines is not joined.
//!
//! # Exclusions
//!
//! [`EXCLUSIONS`] lists deliberate non-citations: the SRS errata table quotes two citations that
//! do not exist in the CR. An exclusion matches only in its own path (`docs/SRS.md`), only for
//! its rule, and only on a line containing its marker text, so it never hides a source comment.

use std::collections::{BTreeMap, BTreeSet};

use super::rules::RuleNum;

/// The SRS path, relative to the repository root.
pub const SRS_PATH: &str = "docs/SRS.md";

/// Extensions whose comments are scanned.
pub const SOURCE_EXTS: [&str; 11] = [
    "rs", "cs", "c", "cpp", "h", "ts", "js", "py", "toml", "yml", "yaml",
];

/// Directory excluded from the scan (this module's own fixtures).
pub const SELF_DIR: &str = "xtask/src/cr/";

/// A deliberate non-citation in the SRS.
#[derive(Debug, Clone, Copy)]
pub struct Exclusion {
    /// Path the exclusion applies to.
    pub path: &'static str,
    /// The excluded rule number.
    pub rule: &'static str,
    /// Text that must appear on the citing line.
    pub line_marker: &'static str,
    /// Why it is excluded (copied into the index).
    pub reason: &'static str,
}

/// The documented exclusion list (SRS errata rows that quote wrong citations).
pub const EXCLUSIONS: [Exclusion; 2] = [
    Exclusion {
        path: SRS_PATH,
        rule: "906",
        line_marker: "Oathbreaker cited as",
        reason: "SRS errata table row 29 quotes this wrong citation; the rule does not exist in the CR",
    },
    Exclusion {
        path: SRS_PATH,
        rule: "729.4",
        line_marker: "Mandatory loops cited",
        reason: "SRS errata table row 31 quotes this wrong citation; the rule does not exist in the CR",
    },
];

/// One excluded citation found by the scan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExcludedCite {
    /// The rule number.
    pub rule: RuleNum,
    /// `path:line`.
    pub location: String,
    /// The exclusion's reason.
    pub reason: String,
}

/// Every citation found: rule -> sorted `path:line` locations, plus the excluded ones.
#[derive(Debug, Default, PartialEq)]
pub struct ScanResult {
    /// Rule number -> citing locations (`path:line`, sorted lexicographically).
    pub cited: BTreeMap<RuleNum, BTreeSet<String>>,
    /// Citations removed by [`EXCLUSIONS`], sorted.
    pub excluded: Vec<ExcludedCite>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Parse a rule number at `p`; returns it and the position after it. The token must end at a
/// boundary: not followed by a letter, digit, `_`, or `.` plus a digit.
fn parse_num(c: &[char], p: usize) -> Option<(RuleNum, usize)> {
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    if !(at(p).is_ascii_digit() && at(p + 1).is_ascii_digit() && at(p + 2).is_ascii_digit()) {
        return None;
    }
    let mut i = p + 3;
    if at(i) == '.' && at(i + 1).is_ascii_digit() {
        i += 1;
        while at(i).is_ascii_digit() {
            i += 1;
        }
        let letters_start = i;
        while at(i).is_ascii_lowercase() {
            i += 1;
        }
        if i - letters_start > 2 {
            return None;
        }
    }
    if is_word(at(i)) || (at(i) == '.' && at(i + 1).is_ascii_digit()) {
        return None;
    }
    let s: String = c[p..i].iter().collect();
    RuleNum::parse(&s).map(|n| (n, i))
}

/// Parse one list item (a rule number or a range) at `p`.
///
/// After a dotted start `A`, the end may be a full rule number, a bare rule number within A's
/// section (`702.178–179` ends at `702.179`, `733.4–6` at `733.6`), or bare letters
/// (`116.2a–m` ends at `116.2m`). After a section start, the end is a rule number or a section.
fn parse_item(c: &[char], p: usize) -> Option<(Vec<RuleNum>, usize)> {
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let (a, end) = parse_num(c, p)?;
    if !matches!(at(end), '\u{2013}' | '-') {
        return Some((vec![a], end));
    }
    let b_start = end + 1;
    if !a.is_dotted() {
        return Some(match parse_num(c, b_start) {
            Some((b, b_end)) => (vec![a, b], b_end),
            None => (vec![a], end),
        });
    }
    if let Some((b, b_end)) = parse_num(c, b_start).filter(|(b, _)| b.is_dotted()) {
        return Some((vec![a, b], b_end));
    }
    let mut i = b_start;
    while at(i).is_ascii_digit() {
        i += 1;
    }
    let digits_end = i;
    while at(i).is_ascii_lowercase() {
        i += 1;
    }
    let boundary = !is_word(at(i)) && !(at(i) == '.' && at(i + 1).is_ascii_digit());
    let tail: String = c[b_start..i].iter().collect();
    let b = if !boundary || tail.is_empty() {
        None
    } else if digits_end > b_start {
        let section = a.as_str().get(..3).unwrap_or("");
        RuleNum::parse(&format!("{section}.{tail}"))
    } else {
        RuleNum::parse(&format!("{}{tail}", a.base()))
    };
    Some(match b {
        Some(b) => (vec![a, b], i),
        None => (vec![a], end),
    })
}

/// Skip one list separator at `p` (`,` `;` `/` or `and`, with spaces); `None` if there is none.
fn skip_separator(c: &[char], p: usize) -> Option<usize> {
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let skip_spaces = |mut i: usize| {
        while at(i) == ' ' {
            i += 1;
        }
        i
    };
    let and_at =
        |i: usize| at(i) == 'a' && at(i + 1) == 'n' && at(i + 2) == 'd' && at(i + 3) == ' ';
    let q = skip_spaces(p);
    if matches!(at(q), ',' | ';' | '/') {
        let r = skip_spaces(q + 1);
        return Some(if and_at(r) { skip_spaces(r + 4) } else { r });
    }
    if q > p && and_at(q) {
        return Some(skip_spaces(q + 4));
    }
    None
}

/// Rule numbers cited by `CR ...` / `CRs ...` lists in `text`.
pub fn cr_lists(text: &str) -> Vec<RuleNum> {
    let c: Vec<char> = text.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let boundary = i == 0 || !is_word(at(i - 1));
        if !(boundary && at(i) == 'C' && at(i + 1) == 'R') {
            i += 1;
            continue;
        }
        let mut p = i + 2;
        if at(p) == 's' {
            p += 1;
        }
        if at(p) != ' ' {
            i += 1;
            continue;
        }
        p += 1;
        while let Some((items, end)) = parse_item(&c, p) {
            out.extend(items);
            match skip_separator(&c, end) {
                Some(next) => p = next,
                None => {
                    p = end;
                    break;
                }
            }
        }
        i = p.max(i + 1);
    }
    out
}

/// Bare dotted rule tokens anywhere in `text` (SRS grammar).
pub fn dotted_tokens(text: &str) -> Vec<RuleNum> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for i in 0..c.len() {
        if i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '.') {
            continue;
        }
        if c.get(i + 3) != Some(&'.') {
            continue;
        }
        if let Some((n, _)) = parse_num(&c, i).filter(|(n, _)| n.is_dotted()) {
            out.push(n);
        }
    }
    out
}

/// All citations on one SRS line, deduplicated and sorted.
pub fn srs_line(line: &str) -> Vec<RuleNum> {
    let mut v = cr_lists(line);
    v.extend(dotted_tokens(line));
    v.sort();
    v.dedup();
    v
}

fn ext_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    }
}

/// True when `path` is a source file whose comments are scanned.
pub fn is_scanned_source(path: &str) -> bool {
    if path.starts_with(SELF_DIR) || path.split('/').any(|seg| seg == "target") {
        return false;
    }
    SOURCE_EXTS.contains(&ext_of(path))
}

/// Comment text per line (1-based line number, text) for a source file.
pub fn comment_lines(path: &str, content: &str) -> Vec<(usize, String)> {
    match ext_of(path) {
        "py" | "toml" | "yml" | "yaml" => hash_comments(content),
        ext => slash_comments(content, matches!(ext, "ts" | "js")),
    }
}

fn hash_comments(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in content.split('\n').enumerate() {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for (i, ch) in line.char_indices() {
            match quote {
                Some(q) => {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' && q == '"' {
                        escaped = true;
                    } else if ch == q {
                        quote = None;
                    }
                }
                None if ch == '"' || ch == '\'' => quote = Some(ch),
                None if ch == '#' => {
                    out.push((n + 1, line[i + 1..].to_string()));
                    break;
                }
                None => {}
            }
        }
    }
    out
}

fn slash_comments(content: &str, backticks: bool) -> Vec<(usize, String)> {
    let c: Vec<char> = content.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let mut lines: BTreeMap<usize, String> = BTreeMap::new();
    let mut line = 1usize;
    let mut i = 0;
    fn push(lines: &mut BTreeMap<usize, String>, line: usize, ch: char) {
        lines.entry(line).or_default().push(ch);
    }
    while i < c.len() {
        let ch = at(i);
        let prev_ident = i > 0 && is_word(at(i - 1));
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch == '/' && at(i + 1) == '/' {
            i += 2;
            lines.entry(line).or_default();
            while i < c.len() && at(i) != '\n' {
                push(&mut lines, line, at(i));
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == '*' {
            i += 2;
            let mut depth = 1usize;
            while i < c.len() && depth > 0 {
                if at(i) == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if at(i) == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    push(&mut lines, line, ' ');
                } else {
                    if at(i) == '\n' {
                        line += 1;
                    } else {
                        push(&mut lines, line, at(i));
                    }
                    i += 1;
                }
            }
        } else if ch == 'r' && !prev_ident && (at(i + 1) == '"' || at(i + 1) == '#') {
            let mut hashes = 0;
            while at(i + 1 + hashes) == '#' {
                hashes += 1;
            }
            if at(i + 1 + hashes) == '"' {
                i += 2 + hashes;
                while i < c.len() {
                    if at(i) == '"' && (0..hashes).all(|k| at(i + 1 + k) == '#') {
                        i += 1 + hashes;
                        break;
                    }
                    if at(i) == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else if ch == '"' || (backticks && ch == '`') {
            i += 1;
            while i < c.len() && at(i) != ch {
                if at(i) == '\\' {
                    i += 1;
                }
                if at(i) == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
        } else if ch == '\'' && backticks {
            // TypeScript/JavaScript: a single-quoted string ends at its closing
            // quote or at the end of the line; backslash escapes the next char.
            i += 1;
            while i < c.len() && at(i) != '\'' && at(i) != '\n' {
                if at(i) == '\\' && at(i + 1) != '\n' {
                    i += 1;
                }
                i += 1;
            }
            if at(i) == '\'' {
                i += 1;
            }
        } else if ch == '\'' {
            if at(i + 1) == '\\' {
                let mut j = i + 2;
                while j < c.len() && j < i + 12 && at(j) != '\'' && at(j) != '\n' {
                    j += 1;
                }
                i = if at(j) == '\'' { j + 1 } else { i + 1 };
            } else if at(i + 2) == '\'' && at(i + 1) != '\n' {
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    lines.into_iter().collect()
}

/// A file to scan: repository-relative path (forward slashes) and its bytes.
#[derive(Debug, Clone)]
pub struct ScanFile {
    /// Path relative to the root, `/`-separated.
    pub path: String,
    /// File content.
    pub bytes: Vec<u8>,
}

fn exclusion_for(path: &str, rule: &RuleNum, line: &str) -> Option<&'static Exclusion> {
    EXCLUSIONS
        .iter()
        .find(|e| e.path == path && e.rule == rule.as_str() && line.contains(e.line_marker))
}

/// Scan the SRS and every scanned source file among `files`.
pub fn scan_files(files: &[ScanFile]) -> ScanResult {
    let mut res = ScanResult::default();
    fn record(res: &mut ScanResult, path: &str, n: usize, text: &str, rule: RuleNum) {
        let location = format!("{path}:{n}");
        if let Some(e) = exclusion_for(path, &rule, text) {
            res.excluded.push(ExcludedCite {
                rule,
                location,
                reason: e.reason.to_string(),
            });
        } else {
            res.cited.entry(rule).or_default().insert(location);
        }
    }
    for f in files {
        let text = String::from_utf8_lossy(&f.bytes);
        if f.path == SRS_PATH {
            for (i, line) in text.split('\n').enumerate() {
                for rule in srs_line(line) {
                    record(&mut res, &f.path, i + 1, line, rule);
                }
            }
        } else if is_scanned_source(&f.path) {
            for (n, comment) in comment_lines(&f.path, &text) {
                let mut rules = cr_lists(&comment);
                rules.sort();
                rules.dedup();
                for rule in rules {
                    record(&mut res, &f.path, n, &comment, rule);
                }
            }
        }
    }
    res.excluded.sort();
    res.excluded.dedup();
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(v: Vec<RuleNum>) -> Vec<String> {
        v.into_iter().map(|r| r.as_str().to_string()).collect()
    }

    /// REQ-DOC-001: the SRS grammar, table-driven.
    #[test]
    fn req_doc_001_srs_grammar_table() {
        let cases: [(&str, &[&str]); 19] = [
            ("see CR 104.4b, 733 here", &["104.4b", "733"]),
            ("CR 100–104, 800–905", &["100", "104", "800", "905"]),
            ("CR 100-104", &["100", "104"]),
            ("**CR 116.2a–m**.", &["116.2a", "116.2m"]),
            ("(CR 117.3, 704.3)", &["117.3", "704.3"]),
            ("the flag (702.191) is", &["702.191"]),
            ("| 712.3, 712.6e |", &["712.3", "712.6e"]),
            ("CR 113.6 – default", &["113.6"]),
            ("baseline CR-2026-11-06 here", &[]),
            ("deck of 101 cards", &[]),
            ("CR 101 alone", &["101"]),
            ("CRs 500 and 502", &["500", "502"]),
            ("CR 713 and 701.42.", &["701.42", "713"]),
            (
                "CR 601.2a–i; activation 602.2 follows",
                &["601.2a", "601.2i", "602.2"],
            ),
            (
                "CR 400.7; 400.8 / 400.9, and 401",
                &["400.7", "400.8", "400.9", "401"],
            ),
            ("v192.168.1.1 or 192.168.1.1 or 1234.5 or x100.1", &[]),
            ("SCR 101, 733.4–6, 100.1abc", &["733.4"]),
            (
                "CR 702.178–179 and 733.4–6",
                &["702.178", "702.179", "733.4", "733.6"],
            ),
            (
                "CR 702.178–179a, 101–102.1",
                &["101", "102.1", "702.178", "702.179a"],
            ),
        ];
        for (line, want) in cases {
            assert_eq!(strs(srs_line(line)), *want, "line {line:?}");
        }
    }

    /// REQ-DOC-001: CR-list edge cases.
    #[test]
    fn req_doc_001_cr_list_edges() {
        assert_eq!(strs(cr_lists("CR")), Vec::<String>::new());
        assert_eq!(strs(cr_lists("CR 1000")), Vec::<String>::new());
        assert_eq!(strs(cr_lists("CR 101.")), ["101"]);
        assert_eq!(strs(cr_lists("CR 101.1-")), ["101.1"]);
        assert_eq!(strs(cr_lists("CR 101.1a-zzz")), ["101.1a"]);
        assert_eq!(strs(cr_lists("CR 101-x")), ["101"]);
        assert_eq!(strs(cr_lists("CR 101.1-2x3")), ["101.1"]);
        assert_eq!(strs(cr_lists("CR 101.1-2.5")), ["101.1"]);
        assert_eq!(strs(cr_lists("CR 101.1-102.2")), ["101.1", "102.2"]);
        assert_eq!(strs(cr_lists("CR 101, then")), ["101"]);
        assert_eq!(strs(cr_lists("CR 101 andrew")), ["101"]);
        assert_eq!(strs(cr_lists("CR 101; CR 102")), ["101", "102"]);
        assert_eq!(strs(cr_lists("CR (e.g. 101)")), Vec::<String>::new());
        assert_eq!(strs(cr_lists("CRs101")), Vec::<String>::new());
    }

    fn files(list: &[(&str, &str)]) -> Vec<ScanFile> {
        list.iter()
            .map(|(p, c)| ScanFile {
                path: (*p).to_string(),
                bytes: c.as_bytes().to_vec(),
            })
            .collect()
    }

    fn cited(res: &ScanResult) -> Vec<(String, Vec<String>)> {
        res.cited
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.iter().cloned().collect()))
            .collect()
    }

    /// REQ-DOC-001: source-comment scanning, string literals ignored, own module ignored,
    /// locations recorded as `path:line`.
    #[test]
    fn req_doc_001_source_comments() {
        let rs = [
            "fn f() {}",
            "// CR 613.8b",
            "/// CR 704.3, 704.5a",
            "//! module (CR 117.3) note",
            "let s = \"// CR 101.1\"; /* CR 117.4 */",
            "let r = r#\"CR 101.2 \"# ; let c = '\"'; // CR 101.3",
            "/* multi",
            "   line CR 101.4 */ let x = 1;",
            "fn g<'a>(x: &'a str) {} // CR 101.5",
            "let e = '\\n'; let q = \"a\\\"CR 101.6\"; // CR 101.7",
        ]
        .join("\n");
        let py = "x = \"# CR 101.8\"  # CR 101.9\ny = 'it''s' # CR 102.1\n# CR 102.2\n";
        let ts = "const s = `// CR 102.3`; // CR 102.4\n";
        let res = scan_files(&files(&[
            ("crates/a/src/lib.rs", &rs),
            ("scripts/x.py", py),
            ("web/x.ts", ts),
            ("xtask/src/cr/fixture.rs", "// CR 103.1"),
            ("target/debug/x.rs", "// CR 103.2"),
            ("crates/a/src/notes.md", "CR 103.3"),
            (
                "crates/a/src/plain.rs",
                "100.5 not a comment // and 100.6 bare in comment",
            ),
        ]));
        let got = cited(&res);
        let want: Vec<(String, Vec<String>)> = [
            ("101.3", "crates/a/src/lib.rs:6"),
            ("101.4", "crates/a/src/lib.rs:8"),
            ("101.5", "crates/a/src/lib.rs:9"),
            ("101.7", "crates/a/src/lib.rs:10"),
            ("101.9", "scripts/x.py:1"),
            ("102.1", "scripts/x.py:2"),
            ("102.2", "scripts/x.py:3"),
            ("102.4", "web/x.ts:1"),
            ("117.3", "crates/a/src/lib.rs:4"),
            ("117.4", "crates/a/src/lib.rs:5"),
            ("613.8b", "crates/a/src/lib.rs:2"),
            ("704.3", "crates/a/src/lib.rs:3"),
            ("704.5a", "crates/a/src/lib.rs:3"),
        ]
        .iter()
        .map(|(r, l)| ((*r).to_string(), vec![(*l).to_string()]))
        .collect();
        assert_eq!(got, want);
        assert!(res.excluded.is_empty());
    }

    /// REQ-DOC-001: the errata exclusions apply to the marked SRS rows only, never to other
    /// SRS lines or source comments.
    #[test]
    fn req_doc_001_exclusions_scoped() {
        let srs = format!(
            "intro CR {a}\n| 29 | Oathbreaker cited as \"CR {a}\" | x |\n| 31 | Mandatory loops cited {b}; y |\n",
            a = 906,
            b = "729.4"
        );
        let src = format!("// CR {}\n", 906);
        let res = scan_files(&files(&[(SRS_PATH, &srs), ("crates/a/src/x.rs", &src)]));
        assert_eq!(
            cited(&res),
            vec![(
                "906".to_string(),
                vec![
                    "crates/a/src/x.rs:1".to_string(),
                    "docs/SRS.md:1".to_string()
                ]
            )]
        );
        let ex: Vec<(String, String)> = res
            .excluded
            .iter()
            .map(|e| (e.rule.as_str().to_string(), e.location.clone()))
            .collect();
        assert_eq!(
            ex,
            [
                ("729.4".to_string(), "docs/SRS.md:3".to_string()),
                ("906".to_string(), "docs/SRS.md:2".to_string())
            ]
        );
        // a source file with the same marker text is not excluded
        let src2 = format!("// Oathbreaker cited as CR {}\n", 906);
        let res2 = scan_files(&files(&[("crates/a/src/y.rs", &src2)]));
        assert!(res2.excluded.is_empty());
        assert_eq!(res2.cited.len(), 1);
    }

    /// REQ-DOC-001: scanned extensions and skipped paths.
    #[test]
    fn req_doc_001_scanned_paths() {
        for p in [
            "a.rs",
            "a/b.cs",
            "c.c",
            "d.cpp",
            "e.h",
            "f.ts",
            "g.js",
            "h.py",
            "Cargo.toml",
            "w.yml",
            "w.yaml",
        ] {
            assert!(is_scanned_source(p), "{p}");
        }
        for p in [
            "a.md",
            "xtask/src/cr/mod.rs",
            "x/target/y.rs",
            ".rs",
            "noext",
        ] {
            assert!(!is_scanned_source(p), "{p}");
        }
    }
}

#[cfg(test)]
mod single_quote_tests {
    use super::*;

    /// REQ-DOC-001: JS/TS single-quoted strings are skipped like double-quoted ones.
    #[test]
    fn req_doc_001_js_single_quoted_strings() {
        let n = format!("{}.{}z", 999, 9);
        for path in ["a.ts", "a.js"] {
            let src = format!("const s = '/*'; // CR {n}\nconst t = 1;\n");
            let got = comment_lines(path, &src);
            assert_eq!(got, vec![(1, format!(" CR {n}"))], "{path}");
            let src = format!("const s = '// CR {n}'; const e = 'a\\'b'; // k\n");
            let got = comment_lines(path, &src);
            assert_eq!(got, vec![(1, " k".to_string())], "{path}");
            // Unterminated: ends at the end of the line.
            let got = comment_lines(path, "const s = 'oops\n// x\n");
            assert_eq!(got, vec![(2, " x".to_string())], "{path}");
        }
    }

    /// REQ-DOC-001: Rust lifetimes and char literals are unchanged.
    #[test]
    fn req_doc_001_rust_lifetimes_unchanged() {
        let got = comment_lines("a.rs", "fn f<'a>(x: &'a str) { let c = '/'; } // k\n");
        assert_eq!(got, vec![(1, " k".to_string())]);
        // A lifetime does not swallow a following block comment start.
        let got = comment_lines("a.rs", "fn f<'a>() {} /* k */\n");
        assert_eq!(got, vec![(1, " k  ".to_string())]);
    }
}
