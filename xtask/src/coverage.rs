// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `cargo xtask coverage`: per-crate line-coverage floors (REQ-NFR-008).
//!
//! Input is the JSON written by `cargo llvm-cov --json --summary-only`. Files are mapped to
//! workspace crates by path, line counts are summed per crate (never averaged), and each crate
//! is compared with its floor in [`FLOORS`] using integer arithmetic only. The same floors are
//! documented in `docs/DEVELOPMENT.md` "Coverage floors".

use std::collections::BTreeMap;

/// Crate to minimum line-coverage percentage. The order is the order of the printed table.
/// Mirror any change in `docs/DEVELOPMENT.md` "Coverage floors".
pub const FLOORS: [(&str, u64); 9] = [
    ("fineprint-base", 90),
    ("fineprint-dsl", 90),
    ("fineprint-core", 90),
    ("fineprint-ingest", 75),
    ("fineprint-ffi", 75),
    ("fineprint-wasm", 75),
    ("fineprint-bot", 75),
    ("fineprint-cli", 75),
    ("xtask", 75),
];

/// Maximum JSON nesting depth the reader accepts.
const MAX_DEPTH: usize = 64;

/// Summed line coverage of one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateCoverage {
    /// Crate name as it appears in [`FLOORS`] (or an unknown name found in the JSON).
    pub name: String,
    /// Total measured lines.
    pub lines: u64,
    /// Covered lines.
    pub covered: u64,
}

/// Pass or fail for one crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// At or above the floor, or nothing measured.
    Pass,
    /// Below the floor, or not in the floor table.
    Fail,
}

/// One row of the printed table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Crate name.
    pub name: String,
    /// Measured lines, `None` when absent or zero.
    pub lines: Option<u64>,
    /// Covered lines.
    pub covered: u64,
    /// Floor percentage, `None` for a crate missing from the table.
    pub floor: Option<u64>,
    /// Result.
    pub status: Status,
}

/// Result of [`check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// One row per floor-table crate in table order, then unknown crates sorted by name.
    pub rows: Vec<Row>,
    /// Messages for failing crates (sorted by crate name).
    pub failures: Vec<String>,
}

impl Report {
    /// True when no crate failed.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// Render the table: header plus one row per crate.
    pub fn render(&self) -> String {
        let mut out = String::from("crate | lines | covered | percent | floor | status\n");
        for r in &self.rows {
            let (lines, percent) = match r.lines {
                Some(l) => (l.to_string(), percent_text(r.covered, l)),
                None => ("n/a".to_string(), "n/a".to_string()),
            };
            let covered = if r.lines.is_some() {
                r.covered.to_string()
            } else {
                "n/a".to_string()
            };
            let floor = r
                .floor
                .map_or_else(|| "none".to_string(), |f| format!("{f}%"));
            let status = match r.status {
                Status::Pass => "PASS",
                Status::Fail => "FAIL",
            };
            out.push_str(&format!(
                "{} | {lines} | {covered} | {percent} | {floor} | {status}\n",
                r.name
            ));
        }
        out
    }
}

/// `covered / lines` as a percentage with two decimals, using integers only.
fn percent_text(covered: u64, lines: u64) -> String {
    if lines == 0 {
        return "n/a".to_string();
    }
    let hundredths = u128::from(covered) * 10_000 / u128::from(lines);
    format!("{}.{:02}%", hundredths / 100, hundredths % 100)
}

/// Compare summed coverage with the floors. Pure and deterministic.
pub fn check(crates: &[CrateCoverage]) -> Report {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for (name, floor) in FLOORS {
        let found = crates.iter().find(|c| c.name == name);
        match found {
            Some(c) if c.lines > 0 => {
                let ok = u128::from(c.covered) * 100 >= u128::from(floor) * u128::from(c.lines);
                if !ok {
                    failures.push(format!(
                        "{name}: line coverage below the {floor}% floor ({} of {} lines)",
                        c.covered, c.lines
                    ));
                }
                rows.push(Row {
                    name: name.to_string(),
                    lines: Some(c.lines),
                    covered: c.covered,
                    floor: Some(floor),
                    status: if ok { Status::Pass } else { Status::Fail },
                });
            }
            _ => rows.push(Row {
                name: name.to_string(),
                lines: None,
                covered: 0,
                floor: Some(floor),
                status: Status::Pass,
            }),
        }
    }
    let mut unknown: Vec<&CrateCoverage> = crates
        .iter()
        .filter(|c| !FLOORS.iter().any(|(n, _)| *n == c.name))
        .collect();
    unknown.sort_by(|a, b| a.name.cmp(&b.name));
    for c in unknown {
        failures.push(format!(
            "{}: crate is not in the floor table; add it to FLOORS in xtask/src/coverage.rs and to docs/DEVELOPMENT.md",
            c.name
        ));
        rows.push(Row {
            name: c.name.clone(),
            lines: Some(c.lines),
            covered: c.covered,
            floor: None,
            status: Status::Fail,
        });
    }
    failures.sort();
    Report { rows, failures }
}

/// The normalised path of `path` relative to `root` (`\` to `/`, trailing `/` of the root
/// dropped), or `None` when `path` does not lie strictly below `root`. Never panics.
fn rel_to_root(root: &str, path: &str) -> Option<String> {
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let path = path.replace('\\', "/");
    let rel = path.strip_prefix(root)?.strip_prefix('/')?;
    Some(rel.to_string())
}

/// True when `path` lies below the workspace `root` (REQ-NFR-008); shares its prefix logic with
/// [`crate_of_rooted`].
pub fn under_root(root: &str, path: &str) -> bool {
    rel_to_root(root, path).is_some()
}

/// Map a file path from the JSON to a workspace crate name, anchored to the workspace `root`
/// (REQ-NFR-008).
///
/// Both arguments are normalised (`\` to `/`, trailing `/` of the root dropped). The path must
/// lie below the root; only the first components of the path relative to the root are examined:
/// `crates/<dir>/<more>` is `<dir>`, `clients/cli/<more>` is `fineprint-cli`, `xtask/<more>` is
/// `xtask`. Everything else (registry, toolchain sources, paths outside the root, nested
/// `xtask`/`crates`/`clients` directories deeper in a crate) is `None`. Never panics.
pub fn crate_of_rooted(root: &str, path: &str) -> Option<String> {
    let rel = rel_to_root(root, path)?;
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        ["crates", dir, _, ..] => Some((*dir).to_string()),
        ["clients", "cli", _, ..] => Some("fineprint-cli".to_string()),
        ["xtask", _, ..] => Some("xtask".to_string()),
        _ => None,
    }
}

/// `(crate, lines, covered)` per crate.
type CrateSums = (String, u64, u64);

/// Parse `cargo llvm-cov --json --summary-only` output into `(crate, lines, covered)` triples
/// sorted by crate name. Files that map to no workspace crate (see [`crate_of_rooted`]) under the
/// workspace `root` are ignored.
#[cfg(test)]
pub fn parse_summary(text: &str, root: &str) -> Result<Vec<CrateSums>, String> {
    parse_summary_counted(text, root).map(|(sums, _, _)| sums)
}

/// Like [`parse_summary`], also returning the number of files in the report and how many of them
/// lie under `root` (see [`under_root`]).
fn parse_summary_counted(text: &str, root: &str) -> Result<(Vec<CrateSums>, usize, usize), String> {
    let doc = parse_json(text)?;
    let data = doc
        .get("data")
        .and_then(Json::as_array)
        .ok_or("missing `data` array")?;
    let first = data.first().ok_or("`data` array is empty")?;
    let files = first
        .get("files")
        .and_then(Json::as_array)
        .ok_or("missing `data[0].files` array")?;
    let mut sums: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut inside = 0usize;
    for file in files {
        let name = file
            .get("filename")
            .and_then(Json::as_str)
            .ok_or("file entry without a string `filename`")?;
        if under_root(root, name) {
            inside += 1;
        }
        let Some(krate) = crate_of_rooted(root, name) else {
            continue;
        };
        let lines = file
            .get("summary")
            .and_then(|s| s.get("lines"))
            .or_else(|| file.get("lines"))
            .ok_or_else(|| format!("{name}: missing `lines` summary"))?;
        let count = lines
            .get("count")
            .and_then(Json::as_count)
            .ok_or_else(|| format!("{name}: `lines.count` is not a non-negative integer"))?;
        let covered = lines
            .get("covered")
            .and_then(Json::as_count)
            .ok_or_else(|| format!("{name}: `lines.covered` is not a non-negative integer"))?;
        let entry = sums.entry(krate).or_insert((0, 0));
        entry.0 = entry.0.checked_add(count).ok_or("line count overflow")?;
        entry.1 = entry.1.checked_add(covered).ok_or("line count overflow")?;
    }
    let sums = sums.into_iter().map(|(k, (l, c))| (k, l, c)).collect();
    Ok((sums, files.len(), inside))
}

/// Parsed command-line options.
struct Options {
    input: Option<String>,
    root: Option<String>,
}

const USAGE: &str =
    "usage: cargo xtask coverage [--input <llvm-cov-summary.json>] [--root <workspace-dir>]";

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        input: None,
        root: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let slot = match flag.as_str() {
            "--input" => &mut opts.input,
            "--root" => &mut opts.root,
            _ => return Err(USAGE.to_string()),
        };
        match it.next() {
            Some(v) if slot.is_none() => *slot = Some(v.clone()),
            _ => return Err(USAGE.to_string()),
        }
    }
    Ok(opts)
}

/// Workspace root: `git rev-parse --show-toplevel`, else the current directory.
fn detect_root() -> Result<String, String> {
    let git = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(root) = git {
        return Ok(root);
    }
    std::env::current_dir()
        .map(|d| d.display().to_string())
        .map_err(|e| {
            format!("cannot determine the workspace root (git and current dir failed): {e}")
        })
}

/// True for a root starting with `/` or `\`, or a Windows drive prefix (`C:` then `/` or `\`).
fn is_absolute_root(root: &str) -> bool {
    let b = root.as_bytes();
    matches!(b.first(), Some(b'/' | b'\\'))
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

/// Resolve `root` against `cwd` (REQ-NFR-008). An absolute `root` is returned unchanged. A
/// relative one is joined to `cwd` and normalised lexically (no filesystem access, so symlinks are
/// not resolved and the result matches the paths in the llvm-cov report): `.` and empty components
/// are dropped, `..` pops the previous component and never goes above the root. The separator is
/// `\` when `cwd` uses only backslashes, else `/`. There is no trailing separator, except that the
/// filesystem root itself is returned as `/` (or `C:\`); the result is never empty. A leading UNC
/// prefix (`\\server\share`, `//server/share`) and the Win32 verbatim prefixes (`\\?\C:`,
/// `\\?\UNC\server\share`) are kept as one non-poppable unit: `..` never removes the server or
/// share. Never panics.
pub fn resolve_root(cwd: &str, root: &str) -> String {
    if is_absolute_root(root) {
        return root.to_string();
    }
    let sep = if cwd.contains('\\') && !cwd.contains('/') {
        '\\'
    } else {
        '/'
    };
    let b = cwd.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    let pieces: Vec<&str> = cwd.split(['/', '\\']).collect();
    let unc =
        pieces.len() > 2 && pieces[0].is_empty() && pieces[1].is_empty() && !pieces[2].is_empty();
    // `fixed` is the number of leading pieces that form the non-poppable prefix.
    let (prefix, skip, rooted) = if unc {
        let verbatim = matches!(pieces[2], "?" | ".");
        let n = if verbatim && pieces.get(3).is_some_and(|p| p.eq_ignore_ascii_case("UNC")) {
            // `\\?\UNC\server\share`
            6
        } else {
            // `\\server\share`, `\\?\C:`, `\\?\<device>`
            4
        };
        let n = n.min(pieces.len());
        let drive_like = verbatim
            && n == 4
            && pieces.get(3).is_some_and(|p| {
                p.len() == 2 && p.as_bytes()[0].is_ascii_alphabetic() && p.as_bytes()[1] == b':'
            });
        let head = format!("{sep}{sep}{}", pieces[2..n].join(&sep.to_string()));
        (head, n, drive_like)
    } else if drive {
        (cwd[..2].to_string(), 1, true)
    } else {
        (String::new(), 0, true)
    };
    let rest = &pieces[skip.min(pieces.len())..];
    // For the drive and plain-root forms the prefix is followed by a separator even when empty.
    let mut comps: Vec<&str> = Vec::new();
    let drive_rest: Vec<&str> = if drive && !unc {
        // `C:` was the first piece's head; its remainder is `pieces[0][2..]`, always empty or a
        // component without separators (e.g. `C:w` is treated as the component `w`).
        let tail = &pieces[0][2..];
        std::iter::once(tail).chain(rest.iter().copied()).collect()
    } else {
        rest.to_vec()
    };
    for c in drive_rest.into_iter().chain(root.split(['/', '\\'])) {
        match c {
            "" | "." => {}
            ".." => {
                comps.pop();
            }
            c => comps.push(c),
        }
    }
    let joined = comps.join(&sep.to_string());
    if rooted || !joined.is_empty() {
        format!("{prefix}{sep}{joined}")
    } else {
        prefix
    }
}

/// Entry point for `cargo xtask coverage [--input <file>] [--root <dir>]`. Returns the process
/// exit code.
pub fn run(args: &[String]) -> u8 {
    let opts = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("coverage: {e}");
            return 2;
        }
    };
    let root = match opts.root {
        Some(r) if is_absolute_root(&r) => r,
        Some(r) => match std::env::current_dir() {
            Ok(cwd) => resolve_root(&cwd.display().to_string(), &r),
            Err(e) => {
                eprintln!("coverage: cannot resolve the relative --root {r}: {e}");
                return 2;
            }
        },
        None => match detect_root() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("coverage: {e}");
                return 2;
            }
        },
    };
    let text = match read_input(opts.input.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("coverage: {e}");
            return 2;
        }
    };
    let (parsed, total, inside) = match parse_summary_counted(&text, &root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("coverage: cannot parse llvm-cov JSON: {e}");
            return 1;
        }
    };
    // A root that matches no file would leave every crate n/a: a vacuous PASS (REQ-NFR-008).
    if total > 0 && inside == 0 {
        eprintln!(
            "coverage: none of the {total} files in the coverage report lies under the workspace root {root}; pass --root <dir> if the checkout path differs"
        );
        return 1;
    }
    let crates: Vec<CrateCoverage> = parsed
        .into_iter()
        .map(|(name, lines, covered)| CrateCoverage {
            name,
            lines,
            covered,
        })
        .collect();
    let report = check(&crates);
    print!("{}", report.render());
    if report.passed() {
        println!("coverage: all crates meet their floors");
        0
    } else {
        for f in &report.failures {
            eprintln!("{f}");
        }
        1
    }
}

fn read_input(input: Option<&str>) -> Result<String, String> {
    match input {
        None => run_llvm_cov(),
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}")),
    }
}

fn run_llvm_cov() -> Result<String, String> {
    let out = std::process::Command::new("cargo")
        .args(["llvm-cov", "--workspace", "--json", "--summary-only"])
        .output()
        .map_err(|e| format!("cannot run cargo llvm-cov: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo llvm-cov failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("llvm-cov output is not UTF-8: {e}"))
}

// ---------------------------------------------------------------------------------------------
// Minimal JSON reader (std only). Never panics; nesting depth capped at MAX_DEPTH.
// ---------------------------------------------------------------------------------------------

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
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A non-negative integer; an integer-valued float form (`12.0`, `1e2`) is accepted.
    fn as_count(&self) -> Option<u64> {
        let Json::Num(text) = self else {
            return None;
        };
        if let Ok(n) = text.parse::<u64>() {
            return Some(n);
        }
        let f: f64 = text.parse().ok()?;
        if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9.0e15 {
            Some(f as u64)
        } else {
            None
        }
    }
}

/// Parse a complete JSON document.
pub fn parse_json(text: &str) -> Result<Json, String> {
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

    fn file(name: &str, count: &str, covered: &str) -> String {
        format!(
            "{{\"filename\":\"{name}\",\"summary\":{{\"lines\":{{\"count\":{count},\"covered\":{covered},\"percent\":0}}}}}}"
        )
    }

    fn doc(files: &[String]) -> String {
        format!(
            "{{\"data\":[{{\"files\":[{}]}}],\"type\":\"llvm.coverage.json.export\"}}",
            files.join(",")
        )
    }

    fn cc(name: &str, lines: u64, covered: u64) -> CrateCoverage {
        CrateCoverage {
            name: name.to_string(),
            lines,
            covered,
        }
    }

    /// REQ-NFR-008: crate mapping, summing (not averaging), ignored paths, rendered table.
    #[test]
    fn req_nfr_008_table_parse_and_render() {
        let json = doc(&[
            file("/w/crates/fineprint-base/src/a.rs", "100", "100"),
            file("/w/crates/fineprint-base/src/b.rs", "900", "800"),
            file("/w/crates/fineprint-dsl/src/lib.rs", "10", "5"),
            file("/w/clients/cli/src/lib.rs", "20", "20.0"),
            file(
                "/home/u/.cargo/registry/src/idx/serde-1/src/lib.rs",
                "999",
                "0",
            ),
        ]);
        let mut parsed = parse_summary(&json, "/w").expect("parses");
        // a Windows-style checkout with its own root maps the same way
        let win = doc(&[file("C:\\\\w\\\\xtask\\\\src\\\\main.rs", "40", "30")]);
        parsed.extend(parse_summary(&win, "C:\\w").expect("parses"));
        // the same file is outside a different root
        assert!(parse_summary(&win, "/w").expect("parses").is_empty());
        parsed.sort();
        assert_eq!(
            parsed,
            vec![
                ("fineprint-base".to_string(), 1000, 900),
                ("fineprint-cli".to_string(), 20, 20),
                ("fineprint-dsl".to_string(), 10, 5),
                ("xtask".to_string(), 40, 30),
            ]
        );
        let crates: Vec<CrateCoverage> = parsed.into_iter().map(|(n, l, c)| cc(&n, l, c)).collect();
        let report = check(&crates);
        let expected = "\
crate | lines | covered | percent | floor | status
fineprint-base | 1000 | 900 | 90.00% | 90% | PASS
fineprint-dsl | 10 | 5 | 50.00% | 90% | FAIL
fineprint-core | n/a | n/a | n/a | 90% | PASS
fineprint-ingest | n/a | n/a | n/a | 75% | PASS
fineprint-ffi | n/a | n/a | n/a | 75% | PASS
fineprint-wasm | n/a | n/a | n/a | 75% | PASS
fineprint-bot | n/a | n/a | n/a | 75% | PASS
fineprint-cli | 20 | 20 | 100.00% | 75% | PASS
xtask | 40 | 30 | 75.00% | 75% | PASS
";
        assert_eq!(report.render(), expected);
        assert!(!report.passed());
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].starts_with("fineprint-dsl"));
    }

    /// REQ-NFR-008: floor boundaries use integer arithmetic.
    #[test]
    fn req_nfr_008_floor_boundaries() {
        let pass = |name: &str, l, c| check(&[cc(name, l, c)]).passed();
        assert!(pass("fineprint-core", 100, 90));
        assert!(pass("xtask", 100, 75));
        assert!(!pass("fineprint-core", 100, 89));
        assert!(!pass("xtask", 100, 74));
        assert!(!pass("fineprint-core", 1000, 899));
        assert!(pass("fineprint-core", 1000, 900));
    }

    /// REQ-NFR-008: zero-line and missing crates are n/a and pass, rows still printed.
    #[test]
    fn req_nfr_008_zero_and_missing_are_na() {
        let report = check(&[cc("fineprint-base", 0, 0)]);
        assert!(report.passed());
        assert_eq!(report.rows.len(), FLOORS.len());
        assert!(report.rows.iter().all(|r| r.status == Status::Pass));
        assert!(
            report
                .render()
                .contains("fineprint-base | n/a | n/a | n/a | 90% | PASS")
        );
    }

    /// REQ-NFR-008: a crate missing from the floor table fails with an instruction.
    #[test]
    fn req_nfr_008_unknown_crate_fails() {
        let report = check(&[cc("fineprint-new", 10, 10)]);
        assert!(!report.passed());
        assert!(report.failures[0].contains("add it to"));
        assert!(report.failures[0].starts_with("fineprint-new"));
        assert!(
            report
                .render()
                .contains("fineprint-new | 10 | 10 | 100.00% | none | FAIL")
        );
    }

    /// REQ-NFR-008: crate mapping relative to an explicit root.
    #[test]
    fn req_nfr_008_crate_of_mapping() {
        let r = crate_of_rooted;
        assert_eq!(r("/w", "/w/crates/x/src/a.rs"), Some("x".to_string()));
        assert_eq!(r("C:\\w", "C:\\w\\crates\\x\\a.rs"), Some("x".to_string()));
        assert_eq!(
            r("/w", "/w/clients/cli/src/a.rs"),
            Some("fineprint-cli".to_string())
        );
        assert_eq!(r("/w", "/w/xtask/src/a.rs"), Some("xtask".to_string()));
        assert_eq!(r("/w", "/w/other/a.rs"), None);
        assert_eq!(r("/h", "/h/.cargo/registry/src/i/c/crates/y/a.rs"), None);
        // registry / rustup / rustc sources are outside the workspace root
        assert_eq!(r("/w", "/h/.cargo/registry/src/i/c/crates/y/a.rs"), None);
        assert_eq!(r("/w", "/h/.rustup/toolchains/t/lib/crates/y/a.rs"), None);
        assert_eq!(r("/w", "/rustc/abc/library/crates/y/a.rs"), None);
        assert_eq!(r("/w", "/w/.rustup/crates/y/a.rs"), None);
    }

    /// REQ-NFR-008: a checkout that itself lives under `crates/` or `xtask/` directories.
    #[test]
    fn req_nfr_008_crate_of_outer_crates_dir() {
        let core = Some("fineprint-core".to_string());
        let root = "/home/dev/crates/fineprint";
        assert_eq!(
            crate_of_rooted(
                root,
                "/home/dev/crates/fineprint/crates/fineprint-core/src/lib.rs"
            ),
            core
        );
        assert_eq!(
            crate_of_rooted(
                "C:\\dev\\crates\\fineprint",
                "C:\\dev\\crates\\fineprint\\crates\\fineprint-core\\src\\lib.rs"
            ),
            core
        );
        assert_eq!(
            crate_of_rooted(root, "/home/dev/crates/fineprint/clients/cli/src/main.rs"),
            Some("fineprint-cli".to_string())
        );
        assert_eq!(
            crate_of_rooted(root, "/home/dev/crates/fineprint/xtask/src/main.rs"),
            Some("xtask".to_string())
        );
        // root equals path
        assert_eq!(crate_of_rooted(root, "/home/dev/crates/fineprint"), None);
        // root with `xtask` in its path
        assert_eq!(
            crate_of_rooted(
                "/home/dev/xtask/fineprint",
                "/home/dev/xtask/fineprint/xtask/src/a.rs"
            ),
            Some("xtask".to_string())
        );
        assert_eq!(
            crate_of_rooted(
                "/home/dev/xtask/fineprint",
                "/home/dev/xtask/fineprint/crates/fineprint-core/src/a.rs"
            ),
            core
        );
    }

    /// REQ-NFR-008: `xtask` / `clients/cli` directories inside another crate's tree do not
    /// hijack the mapping.
    #[test]
    fn req_nfr_008_crate_of_nested_workspace_names() {
        let core = Some("fineprint-core".to_string());
        let r = |p: &str| crate_of_rooted("/w", p);
        let w = |p: &str| crate_of_rooted("C:\\w", p);
        assert_eq!(r("/w/crates/fineprint-core/src/xtask/mod.rs"), core);
        assert_eq!(r("/w/crates/fineprint-core/src/xtask/src/a.rs"), core);
        assert_eq!(w("C:\\w\\crates\\fineprint-core\\src\\xtask\\mod.rs"), core);
        assert_eq!(
            w("C:\\w\\crates\\fineprint-core\\src\\xtask\\src\\a.rs"),
            core
        );
        assert_eq!(r("/w/crates/fineprint-core/src/clients/cli/x.rs"), core);
        assert_eq!(
            w("C:\\w\\crates\\fineprint-core\\src\\clients\\cli\\x.rs"),
            core
        );
        assert_eq!(r("/w/xtask/src/main.rs"), Some("xtask".to_string()));
    }

    /// REQ-NFR-008: regression cases for nested directories, roots, and odd input.
    #[test]
    fn req_nfr_008_crate_of_rooted_nested_dirs() {
        let core = Some("fineprint-core".to_string());
        let r = |p: &str| crate_of_rooted("/w", p);
        assert_eq!(
            r("/w/crates/fineprint-core/src/nested/xtask/src/a.rs"),
            core
        );
        assert_eq!(
            r("/w/crates/fineprint-core/src/nested/crates/other/src/a.rs"),
            core
        );
        assert_eq!(r("/w/crates/fineprint-core/tests/clients/cli/a.rs"), core);
        assert_eq!(
            crate_of_rooted(
                "C:\\w",
                "C:\\w\\crates\\fineprint-core\\src\\nested\\xtask\\src\\a.rs"
            ),
            core
        );
        // outside the root
        assert_eq!(r("/other/crates/fineprint-core/src/a.rs"), None);
        assert_eq!(r("/wx/crates/fineprint-core/src/a.rs"), None);
        // root with trailing slash
        assert_eq!(
            crate_of_rooted("/w/", "/w/crates/fineprint-core/src/a.rs"),
            core
        );
        assert_eq!(
            crate_of_rooted("C:\\w\\", "C:\\w\\xtask\\src\\a.rs"),
            Some("xtask".to_string())
        );
        // no component after the crate directory
        assert_eq!(r("/w/crates/fineprint-core"), None);
        assert_eq!(r("/w/crates/fineprint-core/"), None);
        assert_eq!(r("/w/crates"), None);
        assert_eq!(r("/w/clients/cli"), None);
        assert_eq!(r("/w/xtask"), None);
        // an unknown crate directory keeps its name so the unknown-crate FAIL rule triggers
        assert_eq!(
            r("/w/crates/fineprint-new/src/a.rs"),
            Some("fineprint-new".to_string())
        );
        // odd input never panics
        assert_eq!(crate_of_rooted("", ""), None);
        assert_eq!(crate_of_rooted("", "/crates/x/a.rs"), Some("x".to_string()));
        assert_eq!(crate_of_rooted("/w", ""), None);
        assert_eq!(crate_of_rooted("/w", "/w"), None);
        assert_eq!(crate_of_rooted("/w", "/w/"), None);
        assert_eq!(crate_of_rooted("/", "/"), None);
    }

    /// REQ-NFR-008: malformed input is an error, never a panic.
    #[test]
    fn req_nfr_008_json_errors() {
        let bad = [
            "",
            "{\"data\":[",
            "{\"a\":1,}",
            "[1,]",
            "\"abc",
            "\"a\\qb\"",
            "\"\\u12\"",
            "{\"data\":[{\"files\":[{\"filename\":\"/w/xtask/src/a.rs\",\"summary\":{\"lines\":{\"count\":\"x\",\"covered\":1}}}]}]}",
            "{\"data\":[{\"files\":[{\"filename\":\"/w/xtask/src/a.rs\",\"summary\":{\"lines\":{\"count\":-1,\"covered\":1}}}]}]}",
            "{}",
            "{\"data\":[]}",
            "01x",
            "tru",
            "1 2",
        ];
        for b in bad {
            assert!(parse_summary(b, "/w").is_err(), "should fail: {b}");
        }
        let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
        assert!(parse_json(&deep).is_err());
        let ok_deep = format!("{}{}", "[".repeat(60), "]".repeat(60));
        assert!(parse_json(&ok_deep).is_ok());
    }

    #[test]
    fn req_nfr_008_json_values() {
        let v = parse_json("{\"a\":[true,false,null,-1.5e2,\"x\\n\\u00e9\\ud83d\\ude00\"]}")
            .expect("ok");
        let arr = v.get("a").and_then(Json::as_array).expect("array");
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[4].as_str(), Some("x\n\u{e9}\u{1f600}"));
        assert_eq!(Json::Num("1e2".to_string()).as_count(), Some(100));
        assert_eq!(Json::Num("1.5".to_string()).as_count(), None);
    }

    /// REQ-NFR-008: fixed-seed fuzz of the reader; no panics.
    #[test]
    fn req_nfr_008_json_fuzz_no_panic() {
        let alphabet: Vec<char> = "{}[]\",:\\u0123-.eEtrufalsn \n\u{e9}x".chars().collect();
        let mut s: u64 = 0x2545_F491_4F6C_DD1D;
        for _ in 0..1000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let len = (s % 257) as usize;
            let mut text = String::new();
            for _ in 0..len {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                text.push(alphabet[(s % alphabet.len() as u64) as usize]);
            }
            let _ = parse_json(&text);
            let _ = parse_summary(&text, "/w");
        }
    }

    /// REQ-NFR-008: a relative `--root` is resolved lexically against the current directory.
    #[test]
    fn req_nfr_008_resolve_root() {
        let r = resolve_root;
        assert_eq!(r("/w", "."), "/w");
        assert_eq!(r("/w", ""), "/w");
        assert_eq!(r("/w", "./xtask/.."), "/w");
        assert_eq!(r("/w/xtask", ".."), "/w");
        // never above the filesystem root; the root itself is `/`, never empty
        assert_eq!(r("/w", "../.."), "/");
        assert_eq!(r("/", ".."), "/");
        assert_eq!(r("/w", "sub/dir"), "/w/sub/dir");
        assert_eq!(r("/w/", "sub//dir/"), "/w/sub/dir");
        assert_eq!(r("/w", "./a/./b/../c"), "/w/a/c");
        // absolute roots are unchanged
        assert_eq!(r("/w", "/abs/root"), "/abs/root");
        assert_eq!(r("/w", "/abs/root/"), "/abs/root/");
        assert_eq!(r("/w", "C:\\w\\root"), "C:\\w\\root");
        assert_eq!(r("/w", "\\w"), "\\w");
        // Windows cwd keeps its separator style
        assert_eq!(r("C:\\w", "."), "C:\\w");
        assert_eq!(r("C:\\w", "sub\\dir"), "C:\\w\\sub\\dir");
        assert_eq!(r("C:\\w\\xtask", ".."), "C:\\w");
        assert_eq!(r("C:\\w", "..\\.."), "C:\\");
        assert_eq!(r("C:\\w\\", "."), "C:\\w");
        // UNC cwd keeps both leading separators, server and share; `..` never pops them
        let unc = "\\\\server\\share\\repo";
        assert_eq!(r(unc, "."), unc);
        assert_eq!(r(unc, ".."), "\\\\server\\share");
        assert_eq!(r(unc, "..\\.."), "\\\\server\\share");
        assert_eq!(r(unc, "..\\..\\.."), "\\\\server\\share");
        assert_eq!(r(unc, "sub\\dir"), "\\\\server\\share\\repo\\sub\\dir");
        assert_eq!(r("\\\\server\\share\\repo\\", "."), unc);
        assert_eq!(r("\\\\server\\share\\", "x"), "\\\\server\\share\\x");
        assert_eq!(r("//server/share/repo", "."), "//server/share/repo");
        assert_eq!(r("//server/share/repo/", "../.."), "//server/share");
        assert_eq!(r("\\\\server", ".."), "\\\\server");
        assert_eq!(
            r("\\\\server\\share\\repo", "\\\\other\\s\\r"),
            "\\\\other\\s\\r"
        );
        // verbatim forms
        assert_eq!(r("\\\\?\\C:\\w\\x", ".."), "\\\\?\\C:\\w");
        assert_eq!(r("\\\\?\\C:\\w", "..\\.."), "\\\\?\\C:\\");
        assert_eq!(
            r("\\\\?\\UNC\\server\\share\\repo", "."),
            "\\\\?\\UNC\\server\\share\\repo"
        );
        assert_eq!(
            r("\\\\?\\UNC\\server\\share\\repo", "..\\..\\.."),
            "\\\\?\\UNC\\server\\share"
        );
        // UNC root and UNC report paths agree, in either separator style
        let root = r(unc, ".");
        for p in [
            "\\\\server\\share\\repo\\xtask\\src\\main.rs",
            "//server/share/repo/xtask/src/main.rs",
        ] {
            assert!(under_root(&root, p));
            assert_eq!(crate_of_rooted(&root, p).as_deref(), Some("xtask"));
        }
        assert_eq!(
            crate_of_rooted(
                &root,
                "\\\\server\\share\\repo\\crates\\fineprint-core\\src\\lib.rs"
            )
            .as_deref(),
            Some("fineprint-core")
        );
        assert!(!under_root(
            &root,
            "\\\\server\\share\\other\\xtask\\src\\main.rs"
        ));
        let froot = r("//server/share/repo", ".");
        assert!(under_root(&froot, "//server/share/repo/xtask/src/main.rs"));
        assert_eq!(
            crate_of_rooted(
                &froot,
                "//server/share/repo/crates/fineprint-core/src/lib.rs"
            )
            .as_deref(),
            Some("fineprint-core")
        );
        // end to end: UNC root and UNC filenames keep every measurement
        let report = doc(&[
            file(
                "\\\\\\\\server\\\\share\\\\repo\\\\xtask\\\\src\\\\main.rs",
                "100",
                "90",
            ),
            file(
                "//server/share/repo/crates/fineprint-core/src/lib.rs",
                "20",
                "19",
            ),
        ]);
        assert_eq!(
            parse_summary(&report, &root).expect("parse"),
            vec![
                ("fineprint-core".to_string(), 20, 19),
                ("xtask".to_string(), 100, 90)
            ]
        );
        // a resolved root classifies files as the absolute one does
        assert!(under_root(&r("/w", "."), "/w/a.rs"));
        assert!(under_root(&r("/w/xtask", ".."), "/w/a.rs"));
        // fixed-seed fuzz: arbitrary strings never panic or yield an empty string
        let alphabet: Vec<char> = "/\\.:aC ?\u{e9}".chars().collect();
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            let mut t = String::new();
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            for _ in 0..(s % 12) {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                t.push(alphabet[(s % alphabet.len() as u64) as usize]);
            }
            t
        };
        for _ in 0..2000 {
            let (cwd, root) = (next(), next());
            assert!(!resolve_root(&cwd, &root).is_empty());
        }
    }

    /// REQ-NFR-008: argument handling and file input.
    #[test]
    fn req_nfr_008_run_with_input_file() {
        let dir = std::env::temp_dir().join(format!("xtask-cov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let good = dir.join("good.json");
        std::fs::write(&good, doc(&[file("/w/xtask/src/a.rs", "10", "10")])).expect("write");
        let bad = dir.join("bad.json");
        std::fs::write(&bad, doc(&[file("/w/xtask/src/a.rs", "10", "1")])).expect("write");
        let junk = dir.join("junk.json");
        std::fs::write(&junk, "nope").expect("write");
        let arg = |p: &std::path::Path| {
            vec![
                "--input".to_string(),
                p.display().to_string(),
                "--root".to_string(),
                "/w".to_string(),
            ]
        };
        assert_eq!(run(&arg(&good)), 0);
        assert_eq!(run(&arg(&bad)), 1);
        assert_eq!(run(&arg(&junk)), 1);
        assert_eq!(run(&arg(&dir.join("missing.json"))), 2);
        assert_eq!(run(&["--bogus".to_string()]), 2);
        assert_eq!(run(&["--root".to_string()]), 2);
        assert_eq!(run(&["--input".to_string()]), 2);
        // a root that contains none of the (non-empty) files is a failure, not a vacuous pass
        let mut other = arg(&bad);
        other[3] = "/elsewhere".to_string();
        assert_eq!(run(&other), 1);
        // auto-detected root (git toplevel or current dir) is accepted
        assert!(detect_root().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ-NFR-008: a workspace checked out at a directory literally named `src` keeps all of
    /// its measurements (nothing is discarded, low coverage is not dropped as unmeasured).
    #[test]
    fn req_nfr_008_checkout_named_src() {
        // `sep` is the separator used in the report paths; JSON needs backslashes doubled.
        let build = |root: &str, sep: &str, xtask_covered: &str| {
            let p = |tail: &str| {
                let joined = format!("{root}/{tail}").replace('/', sep);
                joined.replace('\\', "\\\\")
            };
            doc(&[
                file(&p("xtask/src/main.rs"), "100", xtask_covered),
                file(&p("clients/cli/src/main.rs"), "10", "9"),
                file(&p("crates/fineprint-core/src/lib.rs"), "20", "19"),
            ])
        };
        let expect_rows = |parsed: Vec<CrateSums>, xtask_covered: u64, passes: bool| {
            assert_eq!(
                parsed,
                vec![
                    ("fineprint-cli".to_string(), 10, 9),
                    ("fineprint-core".to_string(), 20, 19),
                    ("xtask".to_string(), 100, xtask_covered),
                ]
            );
            let crates: Vec<CrateCoverage> =
                parsed.into_iter().map(|(n, l, c)| cc(&n, l, c)).collect();
            let report = check(&crates);
            for (name, status) in [
                ("xtask", if passes { Status::Pass } else { Status::Fail }),
                ("fineprint-cli", Status::Pass),
                ("fineprint-core", Status::Pass),
            ] {
                let row = report.rows.iter().find(|r| r.name == name).expect("row");
                assert!(row.lines.is_some(), "{name} must be measured, not n/a");
                assert_eq!(row.status, status, "{name}");
            }
            assert_eq!(report.passed(), passes);
            if !passes {
                assert_eq!(report.failures.len(), 1);
                assert!(report.failures[0].starts_with("xtask"));
            }
        };
        for (root, sep) in [("/work/src", "/"), ("C:\\work\\src", "\\")] {
            // the Windows root is spelled with backslashes in the report paths
            let norm = root.replace('\\', "/");
            let good = build(&norm, sep, "90");
            expect_rows(parse_summary(&good, root).expect("parses"), 90, true);
            let bad = build(&norm, sep, "10");
            expect_rows(parse_summary(&bad, root).expect("parses"), 10, false);
        }

        let dir = std::env::temp_dir().join(format!("xtask-cov-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let good = dir.join("good.json");
        std::fs::write(&good, build("/work/src", "/", "90")).expect("write");
        let bad = dir.join("bad.json");
        std::fs::write(&bad, build("/work/src", "/", "10")).expect("write");
        let args = |p: &std::path::Path, root: &str| {
            vec![
                "--input".to_string(),
                p.display().to_string(),
                "--root".to_string(),
                root.to_string(),
            ]
        };
        assert_eq!(run(&args(&good, "/work/src")), 0);
        assert_eq!(run(&args(&bad, "/work/src")), 1);
        let win_good = dir.join("win_good.json");
        std::fs::write(&win_good, build("C:/work/src", "\\", "90")).expect("write");
        let win_bad = dir.join("win_bad.json");
        std::fs::write(&win_bad, build("C:/work/src", "\\", "10")).expect("write");
        assert_eq!(run(&args(&win_good, "C:\\work\\src")), 0);
        assert_eq!(run(&args(&win_bad, "C:\\work\\src")), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ-NFR-008: a root matching no file of a non-empty report fails; other shapes keep working.
    #[test]
    fn req_nfr_008_root_mismatch_is_failure() {
        assert!(under_root("/w", "/w/a.rs"));
        assert!(under_root("C:\\w\\", "C:\\w\\a.rs"));
        assert!(!under_root("/w", "/wx/a.rs"));
        assert!(!under_root("/w", "/w"));
        assert!(!under_root("/w", ""));
        assert!(!under_root("", ""));

        let dir = std::env::temp_dir().join(format!("xtask-cov-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let write = |name: &str, body: String| {
            let p = dir.join(name);
            std::fs::write(&p, body).expect("write");
            p.display().to_string()
        };
        let args = |input: String, root: &str| {
            vec![
                "--input".to_string(),
                input,
                "--root".to_string(),
                root.to_string(),
            ]
        };
        let good = write("good.json", doc(&[file("/w/xtask/src/a.rs", "10", "10")]));
        assert_eq!(run(&args(good.clone(), "/w")), 0);
        assert_eq!(run(&args(good.clone(), "/nonexistent")), 1);
        // empty files list: nothing to attribute, still a pass
        let empty = write("empty.json", doc(&[]));
        assert_eq!(run(&args(empty.clone(), "/w")), 0);
        assert_eq!(run(&args(empty, "/nonexistent")), 0);
        // all files under the root but unmapped (e.g. vendored sources): pass
        let unmapped = write("unmapped.json", doc(&[file("/w/vendor/x/a.rs", "5", "0")]));
        assert_eq!(run(&args(unmapped, "/w")), 0);
        // mixed: one file under the root, one outside; only the mapped crate is measured
        let mixed_doc = doc(&[
            file("/w/xtask/src/a.rs", "10", "10"),
            file("/h/.cargo/registry/src/i/c/src/lib.rs", "99", "0"),
        ]);
        let (parsed, total, inside) = parse_summary_counted(&mixed_doc, "/w").expect("parses");
        assert_eq!(parsed, vec![("xtask".to_string(), 10, 10)]);
        assert_eq!((total, inside), (2, 1));
        let mixed = write("mixed.json", mixed_doc);
        assert_eq!(run(&args(mixed, "/w")), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
