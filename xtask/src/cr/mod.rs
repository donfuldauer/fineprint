// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `cargo xtask cr <fetch|index|check>`: the CR citation index (REQ-DOC-001, REQ-NFR-007).
//!
//! - `fetch [--url <url>] [--force] [--cache <dir>]` downloads the official Comprehensive Rules
//!   text into the git-ignored cache (see [`fetch`]).
//! - `index [--cache <dir>] [--root <dir>] [--out <file>]` parses the cached text (see
//!   [`rules`]), scans `docs/SRS.md` and source comments for citations (see [`scan`]) and writes
//!   `docs/cr-citations.json` (see [`index`]): rule number -> SHA-256 of the normalized rule
//!   text, plus citing locations. No rule prose is written. A citation missing from the CR
//!   (exit 1) prevents writing.
//! - `check [--cache <dir>] [--root <dir>] [--index <file>]` re-scans and fails (exit 1) when a
//!   cited rule is missing from the CR, is not in the committed index, or its hash differs from
//!   the committed one, or when the index is missing, unparsable or of an unknown schema
//!   version. Only citation sets and hashes are compared, never locations or line numbers. An
//!   indexed rule that is no longer cited and an effective-date change are warnings only.
//!
//! Defaults: the root is the git top level (else the current directory), the cache is
//! `<root>/cr-cache`, the index is `<root>/docs/cr-citations.json`. Files come from
//! `git ls-files` when the root is a git work tree's top level, otherwise from a directory walk
//! (skipping `.git`, `target`, `cr-cache` and `node_modules`). Exit codes: 0 success (one summary
//! line on stdout), 1 violations (`path:line: message` on stderr), 2 usage or input error.

pub mod fetch;
pub mod index;
pub mod json;
pub mod rules;
pub mod scan;
pub mod sha256;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use index::{Baseline, Index};
use rules::{CrRules, RuleNum};
use scan::{SRS_PATH, ScanFile, ScanResult};

const USAGE: &str = "usage: cargo xtask cr <command>\n  fetch [--url <url>] [--force] [--cache <dir>]\n  index [--cache <dir>] [--root <dir>] [--out <file>]\n  check [--cache <dir>] [--root <dir>] [--index <file>]";

/// The committed index path, relative to the root.
pub const INDEX_PATH: &str = "docs/cr-citations.json";

/// Parsed command-line flags.
#[derive(Debug, Default, PartialEq)]
struct Flags {
    url: Option<String>,
    force: bool,
    cache: Option<String>,
    root: Option<String>,
    file: Option<String>,
}

/// Parse `args` allowing the value flags in `values` (plus `--force` when `force_ok`).
fn parse_flags(args: &[String], values: &[&str], force_ok: bool) -> Result<Flags, String> {
    let mut f = Flags::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--force" && force_ok {
            f.force = true;
            continue;
        }
        if !values.contains(&a.as_str()) {
            return Err(format!("unknown argument `{a}`"));
        }
        let v = it
            .next()
            .ok_or_else(|| format!("missing value for `{a}`"))?
            .clone();
        match a.as_str() {
            "--url" => f.url = Some(v),
            "--cache" => f.cache = Some(v),
            "--root" => f.root = Some(v),
            _ => f.file = Some(v),
        }
    }
    Ok(f)
}

/// Entry point for `cargo xtask cr ...`. Returns the process exit code.
pub fn run(args: &[String]) -> u8 {
    let parsed = match args.split_first() {
        Some((cmd, rest)) if cmd == "fetch" => {
            parse_flags(rest, &["--url", "--cache"], true).map(|f| (0, f))
        }
        Some((cmd, rest)) if cmd == "index" => {
            parse_flags(rest, &["--cache", "--root", "--out"], false).map(|f| (1, f))
        }
        Some((cmd, rest)) if cmd == "check" => {
            parse_flags(rest, &["--cache", "--root", "--index"], false).map(|f| (2, f))
        }
        _ => Err("missing or unknown subcommand".to_string()),
    };
    let (which, flags) = match parsed {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cr: {e}\n{USAGE}");
            return 2;
        }
    };
    let root = match flags.root.clone().map_or_else(detect_root, Ok) {
        Ok(r) => PathBuf::from(r),
        Err(e) => {
            eprintln!("cr: {e}");
            return 2;
        }
    };
    let cache = flags
        .cache
        .clone()
        .map_or_else(|| root.join("cr-cache"), PathBuf::from);
    let file = flags
        .file
        .clone()
        .map_or_else(|| root.join(INDEX_PATH), PathBuf::from);
    let display = flags.file.clone().unwrap_or_else(|| INDEX_PATH.to_string());
    match which {
        0 => {
            let opts = fetch::FetchOptions {
                url: flags.url,
                force: flags.force,
                cache,
            };
            match fetch::fetch(&fetch::Curl, &opts) {
                Ok(msg) => {
                    println!("{msg}");
                    0
                }
                Err(e) => {
                    eprintln!("cr fetch: {e}");
                    2
                }
            }
        }
        1 => cmd_index(&root, &cache, &file, &display),
        _ => cmd_check(&root, &cache, &file, &display),
    }
}

/// Workspace root: `git rev-parse --show-toplevel`, else the current directory.
fn detect_root() -> Result<String, String> {
    let git = Command::new("git")
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
        .map_err(|e| format!("cannot determine the root (git and current dir failed): {e}"))
}

/// List and read the files under `root` (git-tracked when `root` is a work tree's top level).
pub fn list_files(root: &Path) -> Result<Vec<ScanFile>, String> {
    let canon =
        fs::canonicalize(root).map_err(|e| format!("cannot open {}: {e}", root.display()))?;
    let top = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| fs::canonicalize(s.trim()).ok());
    let mut paths: Vec<String> = Vec::new();
    if top.as_deref() == Some(canon.as_path()) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "-z"])
            .output()
            .map_err(|e| format!("cannot run git ls-files: {e}"))?;
        if !out.status.success() {
            return Err("git ls-files failed".to_string());
        }
        paths = out
            .stdout
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
    } else {
        walk(&canon, "", &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    read_scan_files(root, paths)
}

/// Read the SRS and every eligible source among `paths` (relative to `root`).
///
/// The SRS must exist and be readable: without it the baseline guard and every
/// SRS citation would silently vanish. Any other read error is fatal too, except
/// `NotFound` for a path that `git ls-files` listed but the working tree no
/// longer holds: a deleted file holds no citations, so it is skipped.
/// Files the scanner would ignore (ineligible extension) are not read.
fn read_scan_files(root: &Path, paths: Vec<String>) -> Result<Vec<ScanFile>, String> {
    let srs = root.join(scan::SRS_PATH);
    let srs_bytes = fs::read(&srs).map_err(|e| {
        format!(
            "cannot read {} (the SRS must exist and be readable): {e}",
            srs.display()
        )
    })?;
    let mut files = vec![ScanFile {
        path: scan::SRS_PATH.to_string(),
        bytes: srs_bytes,
    }];
    for p in paths {
        if p == scan::SRS_PATH || !scan::is_scanned_source(&p) {
            continue;
        }
        let full = root.join(&p);
        match fs::read(&full) {
            Ok(bytes) => files.push(ScanFile { path: p, bytes }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot read {}: {e}", full.display())),
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|e| format!("cannot read an entry of {}: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}{name}");
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", dir.join(&name).display()))?;
        if ft.is_dir() {
            if ![".git", "target", "cr-cache", "node_modules"].contains(&name.as_str()) {
                walk(&e.path(), &format!("{rel}/"), out)?;
            }
        } else if ft.is_file() {
            out.push(rel);
        }
    }
    Ok(())
}

/// The parsed cache: rule hashes and the recorded source.
pub struct Cache {
    /// Parsed rules.
    pub rules: CrRules,
    /// `source.json`.
    pub source: fetch::Source,
}

/// Load and parse the cache; every error is an input error (exit 2).
pub fn load_cache(cache: &Path) -> Result<Cache, String> {
    let rules_path = cache.join("rules.txt");
    let source_path = cache.join("source.json");
    if !rules_path.exists() || !source_path.exists() {
        return Err(format!(
            "no fetched CR text in {} (run `cargo xtask cr fetch`)",
            cache.display()
        ));
    }
    let bytes =
        fs::read(&rules_path).map_err(|e| format!("cannot read {}: {e}", rules_path.display()))?;
    let source_text = fs::read_to_string(&source_path)
        .map_err(|e| format!("cannot read {}: {e}", source_path.display()))?;
    let source = fetch::read_source(&source_text).map_err(|e| {
        format!(
            "{}: {e} (run `cargo xtask cr fetch --force`)",
            source_path.display()
        )
    })?;
    if sha256::sha256_hex(&bytes) != source.sha256 {
        return Err(format!(
            "{} does not match the checksum in {} (run `cargo xtask cr fetch --force`)",
            rules_path.display(),
            source_path.display()
        ));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("{} is not valid UTF-8", rules_path.display()))?;
    let rules = rules::parse_cr(text).map_err(|e| format!("{}: {e}", rules_path.display()))?;
    Ok(Cache { rules, source })
}

/// Outcome of a command: exit code plus the lines it prints.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Exit code.
    pub code: u8,
    /// Lines for stdout.
    pub stdout: Vec<String>,
    /// Lines for stderr.
    pub stderr: Vec<String>,
}

impl Outcome {
    fn emit(self) -> u8 {
        for l in &self.stdout {
            println!("{l}");
        }
        for l in &self.stderr {
            eprintln!("{l}");
        }
        self.code
    }

    fn input_error(msg: String) -> Outcome {
        Outcome {
            code: 2,
            stdout: vec![],
            stderr: vec![msg],
        }
    }
}

fn cmd_index(root: &Path, cache: &Path, out: &Path, display: &str) -> u8 {
    index_at(root, cache, out, display).emit()
}

fn cmd_check(root: &Path, cache: &Path, file: &Path, display: &str) -> u8 {
    check_at(root, cache, file, display).emit()
}

/// `cr index` over a root: everything except printing.
pub fn index_at(root: &Path, cache: &Path, out: &Path, display: &str) -> Outcome {
    let c = match load_cache(cache) {
        Ok(c) => c,
        Err(e) => return Outcome::input_error(format!("cr index: {e}")),
    };
    let files = match list_files(root) {
        Ok(f) => f,
        Err(e) => return Outcome::input_error(format!("cr index: {e}")),
    };
    let scan = scan::scan_files(&files);
    let baseline = Baseline::new(&c.source.effective_date, &c.source.source_url);
    let mut stderr = Vec::new();
    if let Some(srs) = files.iter().find(|f| f.path == SRS_PATH) {
        let id = baseline.id.as_bytes();
        if !srs.bytes.windows(id.len()).any(|w| w == id) {
            stderr.push(format!(
                "{SRS_PATH}:1: the fetched CR is baseline {} but the SRS does not name it; update the SRS rules baseline first",
                baseline.id
            ));
        }
    }
    let built = index::build(&scan, &c.rules, baseline);
    if let Err(missing) = &built {
        for m in missing {
            stderr.push(format!(
                "{}: CR {} not found in the fetched CR text",
                m.location, m.rule
            ));
        }
    }
    if !stderr.is_empty() {
        stderr.push(format!(
            "cr index: {} problem(s); {display} not written",
            stderr.len()
        ));
        return Outcome {
            code: 1,
            stdout: vec![],
            stderr,
        };
    }
    let Ok(idx) = built else {
        return Outcome::input_error("cr index: internal error".to_string());
    };
    if let Err(e) = fs::write(out, index::render(&idx)) {
        return Outcome::input_error(format!("cr index: cannot write {}: {e}", out.display()));
    }
    Outcome {
        code: 0,
        stdout: vec![format!(
            "cr index: wrote {display} ({} cited rules, {} excluded citations, {} CR rules parsed, {} duplicate rule numbers, baseline {})",
            idx.citations.len(),
            idx.excluded.len(),
            c.rules.hashes.len(),
            c.rules.duplicates,
            idx.baseline.id
        )],
        stderr: vec![],
    }
}

/// Compare a scan with the CR and the committed index (pure). Returns (errors, warnings).
pub fn compare(
    scan: &ScanResult,
    cr: &CrRules,
    fetched_date: &str,
    committed: Result<&Index, &str>,
    display: &str,
) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for m in index::missing(scan, cr) {
        errors.push(format!(
            "{}: CR {} not found in the fetched CR text",
            m.location, m.rule
        ));
    }
    let idx = match committed {
        Ok(i) => i,
        Err(e) => {
            errors.push(format!("{display}:1: {e}"));
            return (errors, warnings);
        }
    };
    for (rule, locs) in &scan.cited {
        let Some(current) = cr.hashes.get(rule) else {
            continue;
        };
        match idx.citations.get(rule) {
            None => {
                for l in locs {
                    errors.push(format!(
                        "{l}: CR {rule} is cited but not in {display}; re-verify the citation, then run `cargo xtask cr index`"
                    ));
                }
            }
            Some(entry) if &entry.sha256 != current => {
                let first = locs.iter().next().map_or(display, String::as_str);
                errors.push(format!(
                    "{first}: CR {rule}: rule text changed since the verified baseline; re-verify, then run `cargo xtask cr index`"
                ));
            }
            Some(_) => {}
        }
    }
    let uncited: Vec<&RuleNum> = idx
        .citations
        .keys()
        .filter(|r| !scan.cited.contains_key(*r))
        .collect();
    for r in uncited {
        warnings.push(format!(
            "warning: {display}: CR {r} is indexed but no longer cited (run `cargo xtask cr index` to drop it)"
        ));
    }
    if idx.baseline.effective_date != fetched_date {
        warnings.push(format!(
            "warning: the fetched CR is effective {fetched_date} but the index baseline is {} ({}); a new CR release can change rule text",
            idx.baseline.effective_date, idx.baseline.id
        ));
    }
    (errors, warnings)
}

/// `cr check` over a root: everything except printing.
pub fn check_at(root: &Path, cache: &Path, file: &Path, display: &str) -> Outcome {
    let c = match load_cache(cache) {
        Ok(c) => c,
        Err(e) => return Outcome::input_error(format!("cr check: {e}")),
    };
    let files = match list_files(root) {
        Ok(f) => f,
        Err(e) => return Outcome::input_error(format!("cr check: {e}")),
    };
    let scan = scan::scan_files(&files);
    let committed: Result<Index, String> = match fs::read_to_string(file) {
        Ok(t) => index::read(&t),
        Err(e) => Err(format!(
            "cannot read the citation index ({e}); run `cargo xtask cr index`"
        )),
    };
    let (errors, warnings) = compare(
        &scan,
        &c.rules,
        &c.source.effective_date,
        committed.as_ref().map_err(String::as_str),
        display,
    );
    let mut stderr = warnings;
    if errors.is_empty() {
        let id = committed.as_ref().map_or("?", |i| i.baseline.id.as_str());
        Outcome {
            code: 0,
            stdout: vec![format!(
                "cr check: ok ({} cited rules match baseline {id}, {} excluded)",
                scan.cited.len(),
                scan.excluded.len()
            )],
            stderr,
        }
    } else {
        let n = errors.len();
        stderr.extend(errors);
        stderr.push(format!("cr check: {n} problem(s)"));
        Outcome {
            code: 1,
            stdout: vec![],
            stderr,
        }
    }
}

#[cfg(test)]
mod tests;
