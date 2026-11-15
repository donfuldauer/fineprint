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

/// Map a file path from the JSON to a workspace crate name, or `None` for other files.
pub fn crate_of(path: &str) -> Option<String> {
    let norm = path.replace('\\', "/");
    if norm.contains("/registry/src/") || norm.contains("/.rustup/") || norm.contains("/rustc/") {
        return None;
    }
    let parts: Vec<&str> = norm.split('/').filter(|p| !p.is_empty()).collect();
    for (i, part) in parts.iter().enumerate() {
        let next = parts.get(i + 1);
        let after = parts.get(i + 2);
        match (*part, next, after) {
            ("crates", Some(dir), Some(_)) => return Some((*dir).to_string()),
            ("clients", Some(&"cli"), Some(_)) => return Some("fineprint-cli".to_string()),
            ("xtask", Some(_), _) => return Some("xtask".to_string()),
            _ => {}
        }
    }
    None
}

/// Parse `cargo llvm-cov --json --summary-only` output into `(crate, lines, covered)` triples
/// sorted by crate name. Files that map to no workspace crate are ignored.
pub fn parse_summary(text: &str) -> Result<Vec<(String, u64, u64)>, String> {
    let root = parse_json(text)?;
    let data = root
        .get("data")
        .and_then(Json::as_array)
        .ok_or("missing `data` array")?;
    let first = data.first().ok_or("`data` array is empty")?;
    let files = first
        .get("files")
        .and_then(Json::as_array)
        .ok_or("missing `data[0].files` array")?;
    let mut sums: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for file in files {
        let name = file
            .get("filename")
            .and_then(Json::as_str)
            .ok_or("file entry without a string `filename`")?;
        let Some(krate) = crate_of(name) else {
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
    Ok(sums.into_iter().map(|(k, (l, c))| (k, l, c)).collect())
}

/// Entry point for `cargo xtask coverage [--input <file>]`. Returns the process exit code.
pub fn run(args: &[String]) -> u8 {
    let text = match read_input(args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("coverage: {e}");
            return 2;
        }
    };
    let parsed = match parse_summary(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("coverage: cannot parse llvm-cov JSON: {e}");
            return 1;
        }
    };
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

fn read_input(args: &[String]) -> Result<String, String> {
    match args {
        [] => run_llvm_cov(),
        [flag, path] if flag == "--input" => {
            std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
        }
        _ => Err("usage: cargo xtask coverage [--input <llvm-cov-summary.json>]".to_string()),
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
            file("C:\\\\w\\\\xtask\\\\src\\\\main.rs", "40", "30"),
            file(
                "/home/u/.cargo/registry/src/idx/serde-1/src/lib.rs",
                "999",
                "0",
            ),
        ]);
        let parsed = parse_summary(&json).expect("parses");
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

    #[test]
    fn req_nfr_008_crate_of_mapping() {
        assert_eq!(crate_of("/w/crates/x/src/a.rs"), Some("x".to_string()));
        assert_eq!(crate_of("C:\\w\\crates\\x\\a.rs"), Some("x".to_string()));
        assert_eq!(
            crate_of("/w/clients/cli/src/a.rs"),
            Some("fineprint-cli".to_string())
        );
        assert_eq!(crate_of("/w/xtask/src/a.rs"), Some("xtask".to_string()));
        assert_eq!(crate_of("/w/other/a.rs"), None);
        assert_eq!(crate_of("/h/.cargo/registry/src/i/c/crates/y/a.rs"), None);
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
            "{\"data\":[{\"files\":[{\"filename\":\"/w/xtask/a.rs\",\"summary\":{\"lines\":{\"count\":\"x\",\"covered\":1}}}]}]}",
            "{\"data\":[{\"files\":[{\"filename\":\"/w/xtask/a.rs\",\"summary\":{\"lines\":{\"count\":-1,\"covered\":1}}}]}]}",
            "{}",
            "{\"data\":[]}",
            "01x",
            "tru",
            "1 2",
        ];
        for b in bad {
            assert!(parse_summary(b).is_err(), "should fail: {b}");
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
            let _ = parse_summary(&text);
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
        let arg = |p: &std::path::Path| vec!["--input".to_string(), p.display().to_string()];
        assert_eq!(run(&arg(&good)), 0);
        assert_eq!(run(&arg(&bad)), 1);
        assert_eq!(run(&arg(&junk)), 1);
        assert_eq!(run(&arg(&dir.join("missing.json"))), 2);
        assert_eq!(run(&["--bogus".to_string()]), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
