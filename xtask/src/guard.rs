// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `cargo xtask guard <headers|card-data|card-branch|all>`: repository guards.
//!
//! Every scanner is a pure function over `(path, content)`; only [`tracked_files`] touches the
//! file system and git. Violations print as `path:line: message` on stderr.
//!
//! # headers (REQ-NFR-012)
//!
//! Every tracked file with extension `.rs .cs .c .cpp .h .ts .js .py` must contain the MPL-2.0
//! header sentence within its first 20 lines (after an optional `#!` shebang line). Each line is
//! stripped of a leading comment marker (`//`, `///`, `//!`, `#`, `/*`, `*`, `<!--`) and
//! surrounding whitespace, the lines are joined with single spaces and whitespace is collapsed;
//! the result must contain [`HEADER_SENTENCE`] (case-sensitive containment). Skipped: paths
//! with a `target/`, `node_modules/` or `dist/` directory component. Generated files need the
//! header too.
//!
//! # card-data (REQ-NFR-004, REQ-ARCH-001, REQ-NFR-007)
//!
//! File-name checks apply to every path: a tracked file is rejected when it ends in `.fpdb` or
//! `.cr.txt`, or its name matches a bulk-data shape (case-insensitive, see `BULK_NAMES`).
//! Content checks are skipped under `docs/` and `.github/ISSUE_TEMPLATE`; elsewhere a file is
//! rejected when it is a `.json` file with an `oracle_text` or `oracleText` key, or has five or
//! more lines that start with a rules-number shape (three digits, a dot, digits, optional
//! letter, optional dot, a space) followed by prose of at least three words. Content scans
//! apply to valid UTF-8 files; a file of 5 MiB or more outside the exempt directories is an
//! explicit "scan incomplete" violation. A comment such as `// CR 613.8b` does not match.
//!
//! # card-branch (REQ-DSL-013, REQ-ARCH-001)
//!
//! Only `.rs` files under `crates/fineprint-core/` are scanned. After a lexical pass that blanks
//! comments and string contents, a line is rejected when it compares card identity (an
//! identifier among `card_def_id`, `cardDefId`, `card_name`, `name`, `def_id`, `definition_id`,
//! which also covers `.name()` and `.card_def_id()`) with a string literal:
//!
//! - `x.card_def_id == "..."`, `"..." != card.name` (either operand order, raw strings too);
//! - `matches!(card_name.as_str(), "...")`, and `match card.name.as_str() { "..." => .. }`
//!   (the arms are scanned until the closing brace);
//! - `card.name.eq("...")` and `card.name.eq_ignore_ascii_case("...")` (`.as_str() == "..."`
//!   is covered by the first form).
//!
//! Not caught: names stored in a local alias first, comparisons built across lines (other than
//! `match` arms), non-literal constants, byte strings, `matches!` split over lines, and
//! identifiers hidden by macros. Exempt: files under `crates/fineprint-core/src/support/`, and
//! any line carrying the marker `// guard-allow: card-branch (<reason>)` with a non-empty
//! reason (see [`ALLOW_MARKER`]).

use std::fmt;
use std::process::Command;

/// The sentence every source header must contain after normalisation.
pub const HEADER_SENTENCE: &str = "This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.";

/// Extensions that need the header.
const HEADER_EXTS: [&str; 8] = ["rs", "cs", "c", "cpp", "h", "ts", "js", "py"];

/// Directory components whose files are skipped by the header guard.
const HEADER_SKIP_DIRS: [&str; 3] = ["target", "node_modules", "dist"];

/// Path prefixes exempt from card-data.
const CARD_DATA_EXEMPT: [&str; 2] = ["docs/", ".github/ISSUE_TEMPLATE"];

/// Bulk-data file-name shapes (lower-case prefix, lower-case suffix).
const BULK_NAMES: [(&str, &str); 10] = [
    ("scryfall-", ".json"),
    ("oracle-cards-", ".json"),
    ("default-cards-", ".json"),
    ("all-cards-", ".json"),
    ("unique-artwork-", ".json"),
    ("rulings-", ".json"),
    ("atomiccards", ".json"),
    ("allprintings", ".json"),
    ("allidentifiers", ".json"),
    ("", ".fpdb"),
];

/// Largest file whose content is scanned by card-data.
const MAX_SCAN_BYTES: usize = 5 * 1024 * 1024;

/// Number of CR-shaped lines that make a file look like rules text.
const CR_LINE_THRESHOLD: usize = 5;

/// Card-branch scan root.
const CARD_BRANCH_ROOT: &str = "crates/fineprint-core/";

/// Paths exempt from card-branch (keyed maps live here).
pub const CARD_BRANCH_ALLOWED_PATHS: [&str; 1] = ["crates/fineprint-core/src/support/"];

/// Per-line exemption marker; a non-empty reason in parentheses must follow.
pub const ALLOW_MARKER: &str = "// guard-allow: card-branch (";

/// Identifiers that denote card identity.
const CARD_IDENTS: [&str; 6] = [
    "card_def_id",
    "cardDefId",
    "card_name",
    "name",
    "def_id",
    "definition_id",
];

/// One guard violation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// Repository-relative path with `/` separators.
    pub path: String,
    /// 1-based line, when the violation has one.
    pub line: Option<usize>,
    /// Human-readable message.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{}: {}", self.path, l, self.message),
            None => write!(f, "{}: {}", self.path, self.message),
        }
    }
}

/// Which guard to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// License headers.
    Headers,
    /// Committed card data.
    CardData,
    /// Card-specific branches in the engine.
    CardBranch,
}

impl Guard {
    /// Name used on the command line and in the summary.
    pub fn name(self) -> &'static str {
        match self {
            Guard::Headers => "headers",
            Guard::CardData => "card-data",
            Guard::CardBranch => "card-branch",
        }
    }
}

/// A tracked file with its bytes.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Repository-relative path with `/` separators.
    pub path: String,
    /// Raw content.
    pub bytes: Vec<u8>,
}

fn violation(path: &str, line: Option<usize>, message: impl Into<String>) -> Violation {
    Violation {
        path: path.to_string(),
        line,
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------------------------
// headers
// ---------------------------------------------------------------------------------------------

fn strip_comment_markers(line: &str) -> &str {
    let mut s = line.trim();
    loop {
        let before = s;
        for m in ["///", "//!", "//", "/*", "<!--", "#", "*"] {
            if let Some(rest) = s.strip_prefix(m) {
                s = rest.trim_start();
                break;
            }
        }
        if s == before {
            break;
        }
    }
    s.trim_end_matches("*/").trim_end_matches("-->").trim()
}

/// Header text of a file: first 20 lines (after a shebang), comment markers stripped,
/// whitespace collapsed.
fn normalised_head(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.lines().peekable();
    if lines.peek().is_some_and(|l| l.starts_with("#!")) {
        lines.next();
    }
    let joined: Vec<&str> = lines.take(20).map(strip_comment_markers).collect();
    joined
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// REQ-NFR-012: report a missing MPL-2.0 header in a source file.
pub fn check_headers(path: &str, content: &str) -> Vec<Violation> {
    let (dirs, file) = match path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", path),
    };
    if dirs.split('/').any(|c| HEADER_SKIP_DIRS.contains(&c)) {
        return Vec::new();
    }
    let ext = file.rsplit_once('.').map(|(_, e)| e);
    if !ext.is_some_and(|e| HEADER_EXTS.contains(&e)) {
        return Vec::new();
    }
    if normalised_head(content).contains(HEADER_SENTENCE) {
        Vec::new()
    } else {
        vec![violation(
            path,
            Some(1),
            "missing MPL-2.0 header in the first 20 lines",
        )]
    }
}

// ---------------------------------------------------------------------------------------------
// card-data
// ---------------------------------------------------------------------------------------------

fn card_data_exempt(path: &str) -> bool {
    CARD_DATA_EXEMPT.iter().any(|p| path.starts_with(p))
}

/// REQ-NFR-004: path-only card-data checks (apply to every path, including exempt directories).
pub fn check_card_data_path(path: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    if path.ends_with(".fpdb") {
        out.push(violation(
            path,
            None,
            "card database artifact must not be committed",
        ));
    }
    if path.ends_with(".cr.txt") {
        out.push(violation(
            path,
            None,
            "Comprehensive Rules text must not be committed",
        ));
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    for (prefix, suffix) in BULK_NAMES {
        if name.starts_with(prefix) && name.ends_with(suffix) && !path.ends_with(".fpdb") {
            out.push(violation(
                path,
                None,
                "file name matches a card bulk-data shape",
            ));
            break;
        }
    }
    out
}

fn line_of(content: &str, byte_idx: usize) -> usize {
    content
        .get(..byte_idx)
        .map_or(0, |s| s.matches('\n').count())
        + 1
}

/// True when `line` starts with a rules-number shape followed by prose.
fn is_cr_line(line: &str) -> bool {
    let b = line.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    if !(at(0).is_ascii_digit()
        && at(1).is_ascii_digit()
        && at(2).is_ascii_digit()
        && at(3) == b'.')
    {
        return false;
    }
    let mut i = 4;
    while at(i).is_ascii_digit() {
        i += 1;
    }
    if i == 4 {
        return false;
    }
    if at(i).is_ascii_lowercase() {
        i += 1;
    }
    if at(i) == b'.' {
        i += 1;
    }
    if at(i) != b' ' {
        return false;
    }
    let rest = line.get(i + 1..).unwrap_or("").trim();
    rest.chars().next().is_some_and(char::is_alphabetic) && rest.split_whitespace().count() >= 3
}

/// REQ-NFR-004: content card-data checks. `content` is `None` when the file is too large or
/// not UTF-8 (the scan is then skipped).
pub fn check_card_data_content(path: &str, content: Option<&str>) -> Vec<Violation> {
    let Some(content) = content else {
        return Vec::new();
    };
    if card_data_exempt(path) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if path.ends_with(".json") {
        let keys = [
            concat!("\"oracle", "_text\""),
            concat!("\"oracle", "Text\""),
        ];
        let mut first: Option<usize> = None;
        for key in keys {
            for (idx, _) in content.match_indices(key) {
                let after = content.get(idx + key.len()..).unwrap_or("");
                if after.trim_start().starts_with(':') {
                    first = Some(first.map_or(idx, |f| f.min(idx)));
                }
            }
        }
        if let Some(idx) = first {
            out.push(violation(
                path,
                Some(line_of(content, idx)),
                "JSON carries an Oracle text key (card data must not be committed)",
            ));
        }
    }
    let cr: Vec<usize> = content
        .lines()
        .enumerate()
        .filter(|(_, l)| is_cr_line(l))
        .map(|(i, _)| i + 1)
        .collect();
    if cr.len() >= CR_LINE_THRESHOLD {
        out.push(violation(
            path,
            cr.first().copied(),
            format!(
                "{} lines look like numbered rules text (rules text must not be committed)",
                cr.len()
            ),
        ));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// card-branch
// ---------------------------------------------------------------------------------------------

/// Blank `//` and `/* */` comments and replace string-literal contents by `x`, keeping line
/// structure. Returns one string per line.
fn lex_lines(content: &str) -> Vec<String> {
    let c: Vec<char> = content.chars().collect();
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    let blank = |ch: char| if ch == '\n' { '\n' } else { ' ' };
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let ch = at(i);
        let prev_ident = out
            .chars()
            .last()
            .is_some_and(|p| p.is_alphanumeric() || p == '_');
        if ch == '/' && at(i + 1) == '/' {
            while i < c.len() && at(i) != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == '*' {
            // block comments nest; blank until the depth returns to zero
            out.push_str("  ");
            i += 2;
            let mut depth = 1usize;
            while i < c.len() && depth > 0 {
                if at(i) == '/' && at(i + 1) == '*' {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if at(i) == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                } else {
                    out.push(blank(at(i)));
                    i += 1;
                }
            }
        } else if ch == 'r' && !prev_ident && (at(i + 1) == '"' || at(i + 1) == '#') {
            let mut hashes = 0;
            while at(i + 1 + hashes) == '#' {
                hashes += 1;
            }
            if at(i + 1 + hashes) == '"' {
                out.push('r');
                for _ in 0..hashes {
                    out.push('#');
                }
                out.push('"');
                i += 2 + hashes;
                while i < c.len() {
                    if at(i) == '"' && (0..hashes).all(|k| at(i + 1 + k) == '#') {
                        break;
                    }
                    out.push(if at(i) == '\n' { '\n' } else { 'x' });
                    i += 1;
                }
                if i < c.len() {
                    out.push('"');
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    i += 1 + hashes;
                }
            } else {
                out.push('r');
                i += 1;
            }
        } else if ch == '"' {
            out.push('"');
            i += 1;
            while i < c.len() && at(i) != '"' {
                if at(i) == '\\' {
                    out.push('x');
                    i += 1;
                }
                if i < c.len() {
                    out.push(if at(i) == '\n' { '\n' } else { 'x' });
                    i += 1;
                }
            }
            if i < c.len() {
                out.push('"');
                i += 1;
            }
        } else if ch == '\'' {
            if at(i + 1) == '\\' {
                // escaped char literal: skip to the closing quote (bounded)
                let mut j = i + 2;
                while j < c.len() && j < i + 12 && at(j) != '\'' && at(j) != '\n' {
                    j += 1;
                }
                if at(j) == '\'' {
                    out.push_str("' '");
                    i = j + 1;
                } else {
                    out.push('\'');
                    i += 1;
                }
            } else if at(i + 2) == '\'' && at(i + 1) != '\'' && at(i + 1) != '\n' {
                out.push_str("' '");
                i += 3;
            } else {
                out.push('\'');
                i += 1;
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out.split('\n').map(str::to_string).collect()
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True when the text mentions one of the card-identity identifiers.
fn has_card_ident(text: &str) -> bool {
    text.split(|c: char| !is_ident_char(c))
        .any(|w| CARD_IDENTS.contains(&w))
}

fn starts_with_literal(s: &str) -> bool {
    let t = s.trim_start();
    if t.starts_with('"') {
        return true;
    }
    t.strip_prefix('r')
        .is_some_and(|r| r.trim_start_matches('#').starts_with('"'))
}

fn ends_with_literal(s: &str) -> bool {
    s.trim_end().trim_end_matches('#').ends_with('"')
}

/// The operand expression ending at the end of `text`.
fn back_operand(text: &str) -> &str {
    let t = text.trim_end();
    let mut depth = 0i32;
    let mut start = t.len();
    for (idx, ch) in t.char_indices().rev() {
        match ch {
            ')' | ']' => {
                depth += 1;
                start = idx;
            }
            '(' | '[' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                start = idx;
            }
            _ if depth > 0 => start = idx,
            c if is_ident_char(c) || matches!(c, '.' | '&' | '*' | ':') => start = idx,
            _ => break,
        }
    }
    t.get(start..).unwrap_or("")
}

/// The operand expression starting at the beginning of `text`.
fn fwd_operand(text: &str) -> &str {
    let t = text.trim_start();
    let mut depth = 0i32;
    let mut end = 0;
    for (idx, ch) in t.char_indices() {
        match ch {
            '(' | '[' => {
                depth += 1;
                end = idx + 1;
            }
            ')' | ']' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                end = idx + 1;
            }
            c if depth > 0 => end = idx + c.len_utf8(),
            c if is_ident_char(c) || matches!(c, '.' | '&' | '*' | ':') => end = idx + c.len_utf8(),
            _ => break,
        }
    }
    t.get(..end).unwrap_or("")
}

/// `==` / `!=` with a string literal on one side and a card identifier on the other.
fn compares_literal(line: &str) -> bool {
    for op in ["==", "!="] {
        for (idx, _) in line.match_indices(op) {
            let left = line.get(..idx).unwrap_or("");
            let right = line.get(idx + 2..).unwrap_or("");
            if starts_with_literal(right) && has_card_ident(back_operand(left)) {
                return true;
            }
            if ends_with_literal(left) && has_card_ident(fwd_operand(right)) {
                return true;
            }
        }
    }
    false
}

/// `matches!(<card expr>, "...")`.
fn matches_macro_literal(line: &str) -> bool {
    for (idx, pat) in line.match_indices("matches!(") {
        let rest = line.get(idx + pat.len()..).unwrap_or("");
        let mut depth = 0i32;
        let mut comma = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                ',' if depth == 0 => {
                    comma = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(c) = comma {
            let expr = rest.get(..c).unwrap_or("");
            let after = rest.get(c + 1..).unwrap_or("");
            if has_card_ident(expr) && starts_with_literal(after) {
                return true;
            }
        }
    }
    false
}

/// `<card expr>.eq("...")` and `<card expr>.eq_ignore_ascii_case("...")`.
fn eq_method_literal(line: &str) -> bool {
    for pat in [".eq(", ".eq_ignore_ascii_case("] {
        for (idx, _) in line.match_indices(pat) {
            let recv = back_operand(line.get(..idx).unwrap_or(""));
            let arg = line.get(idx + pat.len()..).unwrap_or("");
            if has_card_ident(recv) && starts_with_literal(arg) {
                return true;
            }
        }
    }
    false
}

fn brace_delta(s: &str) -> i64 {
    s.chars().fold(0i64, |d, c| match c {
        '{' => d + 1,
        '}' => d - 1,
        _ => d,
    })
}

/// `match <card expr> {` whose arms (until the closing brace) include a string-literal pattern.
fn match_block_literal(lines: &[String], i: usize) -> bool {
    let Some(line) = lines.get(i) else {
        return false;
    };
    for (idx, _) in line.match_indices("match ") {
        let prev_ok = line
            .get(..idx)
            .and_then(|s| s.chars().last())
            .is_none_or(|p| !is_ident_char(p));
        if !prev_ok {
            continue;
        }
        let after = line.get(idx + 6..).unwrap_or("");
        let Some(brace) = after.find('{') else {
            continue;
        };
        if !has_card_ident(after.get(..brace).unwrap_or("")) {
            continue;
        }
        let first_rest = after.get(brace + 1..).unwrap_or("");
        if starts_with_literal(first_rest) && first_rest.contains("=>") {
            return true;
        }
        let mut depth = 1 + brace_delta(first_rest);
        let mut j = i + 1;
        while depth > 0 {
            let Some(l) = lines.get(j) else {
                break;
            };
            if depth == 1 && starts_with_literal(l) && l.contains("=>") {
                return true;
            }
            depth += brace_delta(l);
            j += 1;
        }
    }
    false
}

/// True when the raw line carries `// guard-allow: card-branch (<reason>)` with a reason.
fn allowed_by_marker(raw: &str) -> bool {
    let Some(idx) = raw.find(ALLOW_MARKER) else {
        return false;
    };
    let rest = raw.get(idx + ALLOW_MARKER.len()..).unwrap_or("");
    rest.find(')')
        .and_then(|close| rest.get(..close))
        .is_some_and(|reason| !reason.trim().is_empty())
}

/// REQ-DSL-013, REQ-ARCH-001: report card-specific branches in `fineprint-core` sources.
pub fn check_card_branch(path: &str, content: &str) -> Vec<Violation> {
    if !path.starts_with(CARD_BRANCH_ROOT)
        || !path.ends_with(".rs")
        || CARD_BRANCH_ALLOWED_PATHS
            .iter()
            .any(|p| path.starts_with(p))
    {
        return Vec::new();
    }
    let code = lex_lines(content);
    let raw: Vec<&str> = content.split('\n').collect();
    let mut out = Vec::new();
    for (i, line) in code.iter().enumerate() {
        let hit = compares_literal(line)
            || matches_macro_literal(line)
            || eq_method_literal(line)
            || match_block_literal(&code, i);
        if !hit {
            continue;
        }
        let original = raw.get(i).copied().unwrap_or("");
        if allowed_by_marker(original) {
            continue;
        }
        out.push(violation(
            path,
            Some(i + 1),
            format!("card-specific branch: {}", original.trim()),
        ));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// driver
// ---------------------------------------------------------------------------------------------

/// Run one guard over a set of files. Pure; the result is sorted.
pub fn scan(guard: Guard, files: &[SourceFile]) -> Vec<Violation> {
    let mut out = Vec::new();
    for f in files {
        match guard {
            Guard::Headers => {
                out.extend(check_headers(&f.path, &String::from_utf8_lossy(&f.bytes)));
            }
            Guard::CardData => {
                out.extend(check_card_data_path(&f.path));
                let too_large = f.bytes.len() >= MAX_SCAN_BYTES;
                if too_large && !card_data_exempt(&f.path) {
                    out.push(violation(
                        &f.path,
                        None,
                        "file too large for the card-data content scan (limit 5 MiB): scan incomplete",
                    ));
                }
                let text = if too_large {
                    None
                } else {
                    std::str::from_utf8(&f.bytes).ok()
                };
                out.extend(check_card_data_content(&f.path, text));
            }
            Guard::CardBranch => {
                if let Ok(text) = std::str::from_utf8(&f.bytes) {
                    out.extend(check_card_branch(&f.path, text));
                }
            }
        }
    }
    out.sort();
    out
}

/// List tracked files (`git ls-files -z`) and read them; unreadable files are skipped.
pub fn tracked_files() -> Result<Vec<SourceFile>, String> {
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !top.status.success() {
        return Err("not inside a git repository".to_string());
    }
    let top = String::from_utf8_lossy(&top.stdout).trim().to_string();
    let list = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&top)
        .output()
        .map_err(|e| format!("cannot run git ls-files: {e}"))?;
    if !list.status.success() {
        return Err("git ls-files failed".to_string());
    }
    let mut paths: Vec<String> = list
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    paths.sort();
    paths.dedup();
    let mut files = Vec::new();
    for p in paths {
        if let Ok(bytes) = std::fs::read(std::path::Path::new(&top).join(&p)) {
            files.push(SourceFile { path: p, bytes });
        }
    }
    Ok(files)
}

/// Entry point for `cargo xtask guard <name>`. Returns the process exit code.
pub fn run(args: &[String]) -> u8 {
    let guards: Vec<Guard> = match args {
        [n] if n == "headers" => vec![Guard::Headers],
        [n] if n == "card-data" => vec![Guard::CardData],
        [n] if n == "card-branch" => vec![Guard::CardBranch],
        [n] if n == "all" => vec![Guard::Headers, Guard::CardData, Guard::CardBranch],
        _ => {
            eprintln!("usage: cargo xtask guard <headers|card-data|card-branch|all>");
            return 2;
        }
    };
    let files = match tracked_files() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("guard: {e}");
            return 2;
        }
    };
    report(&guards, &files)
}

/// Run guards over files, print violations (stderr) or a summary (stdout), return exit code.
pub fn report(guards: &[Guard], files: &[SourceFile]) -> u8 {
    let mut all = Vec::new();
    for g in guards {
        all.extend(scan(*g, files));
    }
    all.sort();
    let names: Vec<&str> = guards.iter().map(|g| g.name()).collect();
    if all.is_empty() {
        println!(
            "guard {}: ok ({} tracked files checked)",
            names.join("+"),
            files.len()
        );
        0
    } else {
        for v in &all {
            eprintln!("{v}");
        }
        eprintln!("guard {}: {} violation(s)", names.join("+"), all.len());
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str =
        "This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.";
    const H2: &str =
        "If a copy of the MPL was not distributed with this file, You can obtain one at";
    const H3: &str = "https://mozilla.org/MPL/2.0/.";

    fn prefixed(p: &str) -> String {
        format!("{p} {H1}\n{p} {H2}\n{p} {H3}\n")
    }

    fn headers_ok(path: &str, content: &str) -> bool {
        check_headers(path, content).is_empty()
    }

    /// REQ-NFR-012: accepted header shapes.
    #[test]
    fn req_nfr_012_headers_clean() {
        assert!(headers_ok(
            "a/x.rs",
            &format!("{}\nfn main() {{}}\n", prefixed("//"))
        ));
        let wrapped = "# This Source Code Form is subject to the terms of the Mozilla Public\n# License, v. 2.0. If a copy of the MPL was not distributed with this\n# file, You can obtain one at https://mozilla.org/MPL/2.0/.\n";
        assert!(headers_ok("x.py", wrapped));
        assert!(headers_ok(
            "x.py",
            &format!("#!/usr/bin/env python3\n{wrapped}")
        ));
        let block = format!("/*\n * {H1}\n * {H2}\n * {H3}\n */\n");
        assert!(headers_ok("x.c", &block));
        assert!(headers_ok("src/x.ts", &prefixed("//")));
        assert!(headers_ok(
            "x.rs",
            &format!("//! {H1}\n//! {H2}\n//! {H3}\n")
        ));
    }

    /// REQ-NFR-012: violations.
    #[test]
    fn req_nfr_012_headers_violations() {
        assert_eq!(check_headers("x.rs", "fn main() {}\n").len(), 1);
        let changed = prefixed("//").replace("Mozilla", "Mozzilla");
        assert_eq!(check_headers("x.rs", &changed).len(), 1);
        let late = format!("{}{}", "\n".repeat(25), prefixed("//"));
        assert_eq!(check_headers("x.rs", &late).len(), 1);
        let v = check_headers("x.rs", "");
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].to_string(),
            "x.rs:1: missing MPL-2.0 header in the first 20 lines"
        );
        // header on line 20 is still found; on line 21 it is not
        let edge = format!("{}{}", "\n".repeat(17), prefixed("//"));
        assert!(headers_ok("x.rs", &edge));
    }

    /// REQ-NFR-012: extension filter and skips.
    #[test]
    fn req_nfr_012_headers_filter_and_skips() {
        assert!(headers_ok("README.md", "text"));
        assert!(headers_ok("Cargo.toml", "x = 1"));
        for ext in ["rs", "cs", "c", "cpp", "h", "ts", "js", "py"] {
            assert_eq!(
                check_headers(&format!("d/f.{ext}"), "x\n").len(),
                1,
                "{ext}"
            );
        }
        assert!(headers_ok("target/x.rs", "x"));
        assert!(headers_ok("a/node_modules/x.js", "x"));
        assert!(headers_ok("clients/dist/x.js", "x"));
        // generated files need the header too (no `@generated` exemption)
        assert_eq!(
            check_headers("x.rs", "// @generated by tool\nfn x() {}\n").len(),
            1
        );
        let generated = format!("// @generated by tool\n{}fn x() {{}}\n", prefixed("//"));
        assert!(headers_ok("x.rs", &generated));
        assert_eq!(check_headers("x.rs", "\n\n\n\n\n// @generated\n").len(), 1);
    }

    /// REQ-NFR-012: `.cpp` files are header-checked.
    #[test]
    fn req_nfr_012_headers_cpp() {
        assert_eq!(check_headers("example.cpp", "int main() {}\n").len(), 1);
        let with = format!("{}int main() {{}}\n", prefixed("//"));
        assert!(headers_ok("example.cpp", &with));
    }

    fn cr_lines(n: usize) -> String {
        (0..n)
            .map(|i| format!("{}.{} {}", 100 + i, i + 1, "some prose about a rule here"))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn data_violations(path: &str, content: &str) -> usize {
        let mut v = check_card_data_path(path);
        v.extend(check_card_data_content(path, Some(content)));
        v.len()
    }

    /// REQ-NFR-004, REQ-NFR-007: card-data violations.
    #[test]
    fn req_nfr_004_card_data_violations() {
        assert_eq!(data_violations("x.fpdb", ""), 1);
        assert_eq!(data_violations("a/b.cr.txt", ""), 1);
        assert_eq!(
            data_violations(&format!("scryfall-{}-20260101.json", "default-cards"), "{}"),
            1
        );
        assert_eq!(data_violations("AtomicCards.json", "{}"), 1);
        assert_eq!(data_violations("x/AllPrintings.JSON", "{}"), 1);
        for k in ["oracle_text", "oracleText"] {
            let body = format!("{{\n  \"{k}\" : \"x\"\n}}");
            let v = check_card_data_content("data/cards.json", Some(&body));
            assert_eq!(v.len(), 1, "{k}");
            assert_eq!(v[0].line, Some(2));
        }
        let v = check_card_data_content("notes.txt", Some(&cr_lines(5)));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, Some(1));
        assert_eq!(
            check_card_data_content("notes.txt", Some(&cr_lines(7))).len(),
            1
        );
    }

    /// REQ-NFR-004: card-data clean samples and skips.
    #[test]
    fn req_nfr_004_card_data_clean() {
        assert_eq!(
            data_violations("data/cards.json", "{\"name\": \"test.alpha\"}"),
            0
        );
        let key = concat!("\"oracle", "_text\"");
        assert_eq!(
            data_violations("data/cards.json", &format!("{{\"x\": {key}}}")),
            0
        );
        assert_eq!(
            data_violations("x.rs", &format!("// CR {}.8b\nfn f() {{}}\n", 613)),
            0
        );
        assert_eq!(data_violations("notes.txt", &cr_lines(4)), 0);
        // exempt directories skip content checks only; file-name checks still apply
        assert_eq!(data_violations("docs/a.fpdb", &cr_lines(9)), 1);
        assert_eq!(data_violations("docs/cards.fpdb", ""), 1);
        assert_eq!(
            check_card_data_path("docs/scryfall-default-cards.json").len(),
            1
        );
        assert_eq!(check_card_data_path("docs/x.cr.txt").len(), 1);
        assert_eq!(check_card_data_path("docs/cards.fpdb").len(), 1);
        assert!(check_card_data_path("docs/notes.md").is_empty());
        assert!(check_card_data_path("docs/SRS.md").is_empty());
        assert_eq!(
            data_violations("docs/x/data.json", &format!("{{{key}: 1}}")),
            0
        );
        assert_eq!(data_violations("docs/notes.txt", &cr_lines(9)), 0);
        assert_eq!(
            data_violations(".github/ISSUE_TEMPLATE/x.json", &format!("{{{key}: 1}}")),
            0
        );
        // prose requirement: a bare numbered list is not rules text
        let short: String = (0..9).map(|i| format!("{}.{} ok\n", 100 + i, i)).collect();
        assert_eq!(data_violations("n.txt", &short), 0);
        // content skipped when absent (too large or not UTF-8) while paths still apply
        assert!(check_card_data_content("a.json", None).is_empty());
        assert_eq!(check_card_data_path("x.fpdb").len(), 1);
    }

    /// REQ-NFR-004: size and UTF-8 handling through `scan`.
    #[test]
    fn req_nfr_004_scan_skips_big_and_binary() {
        let key = concat!("\"oracle", "_text\"");
        let big = format!("{{{key}: 1}}{}", " ".repeat(MAX_SCAN_BYTES));
        let mut bad = format!("{{{key}: 1}}").into_bytes();
        bad.push(0xff);
        // oversized outside exempt dirs: exactly one explicit incomplete-scan violation
        let v = scan(
            Guard::CardData,
            &[SourceFile {
                path: "data/cards.json".to_string(),
                bytes: big.clone().into_bytes(),
            }],
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("scan incomplete"), "{}", v[0]);
        // same content under docs/: no violation
        let v = scan(
            Guard::CardData,
            &[SourceFile {
                path: "docs/cards.json".to_string(),
                bytes: big.into_bytes(),
            }],
        );
        assert!(v.is_empty());
        // one byte under the limit is still content-scanned
        let pad = MAX_SCAN_BYTES - 1 - format!("{{{key}: 1}}").len();
        let near = format!("{{{key}: 1}}{}", " ".repeat(pad));
        assert_eq!(near.len(), MAX_SCAN_BYTES - 1);
        let v = scan(
            Guard::CardData,
            &[SourceFile {
                path: "data/cards.json".to_string(),
                bytes: near.into_bytes(),
            }],
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("Oracle text key"), "{}", v[0]);
        let files = vec![
            SourceFile {
                path: "bin.json".to_string(),
                bytes: bad,
            },
            SourceFile {
                path: "ok.json".to_string(),
                bytes: format!("{{{key}: 1}}").into_bytes(),
            },
        ];
        let v = scan(Guard::CardData, &files);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path, "ok.json");
    }

    fn branch(src: &str) -> usize {
        check_card_branch("crates/fineprint-core/src/x.rs", src).len()
    }

    /// REQ-DSL-013, REQ-ARCH-001: card-branch violations.
    #[test]
    fn req_dsl_013_card_branch_violations() {
        assert_eq!(branch("    if def.card_def_id == \"test.alpha\" {\n"), 1);
        assert_eq!(branch("    if \"test.alpha\" != card.name {\n"), 1);
        assert_eq!(
            branch("    let b = matches!(card_name.as_str(), \"test.beta\");\n"),
            1
        );
        assert_eq!(
            branch(
                "    match card.name.as_str() {\n        \"test.gamma\" => 1,\n        _ => 2,\n    }\n"
            ),
            1
        );
        assert_eq!(
            branch("    match card.name.as_str() { \"test.gamma\" => 1, _ => 2 }\n"),
            1
        );
        assert_eq!(
            branch("    if card.name.eq_ignore_ascii_case(\"test.delta\") {}\n"),
            1
        );
        assert_eq!(branch("    if def.name().eq(\"test.eps\") {}\n"), 1);
        assert_eq!(
            branch("    if x.card_def_id.as_str() == r#\"test.zeta\"# {}\n"),
            1
        );
        assert_eq!(branch("    if cardDefId != r\"test.eta\" {}\n"), 1);
        let v = check_card_branch(
            "crates/fineprint-core/src/y.rs",
            "fn f() {\n    if def.card_def_id == \"test.alpha\" {\n    }\n}\n",
        );
        assert_eq!(
            v[0].to_string(),
            "crates/fineprint-core/src/y.rs:2: card-specific branch: if def.card_def_id == \"test.alpha\" {"
        );
    }

    /// REQ-DSL-013: block comments nest in the lexical pass.
    #[test]
    fn req_dsl_013_card_branch_nested_block_comments() {
        let cmp = format!("if def.card_def_id {} \"test.alpha\" {{}}", "==");
        let inside = format!("/* outer /* inner */ {cmp} */\nfn f() {{}}\n");
        assert_eq!(branch(&inside), 0);
        let after = format!("/* outer /* inner */ */ {cmp}\n");
        assert_eq!(branch(&after), 1);
        // unterminated nested comment runs to end of file
        let open = format!("/* a /* b */\n{cmp}\n");
        assert_eq!(branch(&open), 0);
    }

    /// REQ-NFR-008: `tracked_files` lists repository files (run from the xtask directory).
    #[test]
    fn req_nfr_008_tracked_files_lists_repository_files() {
        let files = tracked_files().expect("git repository");
        assert!(files.iter().any(|f| f.path == "xtask/src/guard.rs"));
    }

    /// REQ-DSL-013: card-branch clean samples and exemptions.
    #[test]
    fn req_dsl_013_card_branch_clean() {
        assert_eq!(branch("    if a.card_def_id == b.card_def_id {}\n"), 0);
        assert_eq!(branch("    if phase_label == \"main\" {}\n"), 0);
        assert_eq!(branch("    // if def.card_def_id == \"test.alpha\" {\n"), 0);
        assert_eq!(branch("    /* card.name == \"test.alpha\" */\n"), 0);
        assert_eq!(branch("    let s = \"card.name == x\";\n"), 0);
        assert_eq!(
            branch("    match phase {\n        \"main\" => 1,\n        _ => 2,\n    }\n"),
            0
        );
        assert_eq!(branch("    let c = 'a'; if card.name == other {}\n"), 0);
        let src = "if def.card_def_id == \"test.alpha\" {}\n";
        assert!(check_card_branch("crates/fineprint-core/src/support/map.rs", src).is_empty());
        assert!(check_card_branch("crates/fineprint-base/src/x.rs", src).is_empty());
        assert!(check_card_branch("crates/fineprint-core/src/x.txt", src).is_empty());
        let ok =
            "if def.card_def_id == \"test.alpha\" {} // guard-allow: card-branch (keyed lookup)\n";
        assert_eq!(branch(ok), 0);
        let empty = "if def.card_def_id == \"test.alpha\" {} // guard-allow: card-branch ()\n";
        assert_eq!(branch(empty), 1);
        let blank = "if def.card_def_id == \"test.alpha\" {} // guard-allow: card-branch (  )\n";
        assert_eq!(branch(blank), 1);
    }

    /// REQ-DSL-013, REQ-NFR-004: scanners survive random input (fixed seed).
    #[test]
    fn req_dsl_013_scanners_fuzz_no_panic() {
        let alphabet: Vec<char> =
            "ab_ .=!\"'#r\\/*(){}[]:,\n\t01nameqv\u{e9}matches!(card_def_id match ==!=.eq(=> "
                .chars()
                .collect();
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..1000 {
            let mut text = String::new();
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            for _ in 0..(s % 257) {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                text.push(alphabet[(s % alphabet.len() as u64) as usize]);
            }
            let _ = check_card_branch("crates/fineprint-core/src/f.rs", &text);
            let _ = check_card_data_content("a.json", Some(&text));
            let _ = check_card_data_content("a.txt", Some(&text));
            let _ = check_headers("a.rs", &text);
        }
    }

    /// REQ-NFR-012, REQ-NFR-004, REQ-DSL-013: driver sorts, reports and returns exit codes.
    #[test]
    fn req_nfr_012_report_and_exit_codes() {
        let files = vec![
            SourceFile {
                path: "z.rs".to_string(),
                bytes: b"fn x() {}".to_vec(),
            },
            SourceFile {
                path: "a.rs".to_string(),
                bytes: b"fn x() {}".to_vec(),
            },
            SourceFile {
                path: "ok.rs".to_string(),
                bytes: prefixed("//").into_bytes(),
            },
        ];
        let v = scan(Guard::Headers, &files);
        assert_eq!(
            v.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(),
            ["a.rs", "z.rs"]
        );
        assert_eq!(report(&[Guard::Headers], &files), 1);
        assert_eq!(
            report(
                &[Guard::Headers, Guard::CardData, Guard::CardBranch],
                &files[2..]
            ),
            0
        );
        assert_eq!(run(&["bogus".to_string()]), 2);
        assert_eq!(run(&[]), 2);
    }
}
