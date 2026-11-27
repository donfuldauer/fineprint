// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `cargo xtask pzl [--cards <dir>] <path>...`: runs `.pzl` scenarios (REQ-TEST-001).
//!
//! Each path is a `.pzl` file or a directory, searched recursively for `*.pzl` files (sorted by
//! path; symbolic links to directories are not followed). Every file is read, parsed with
//! `fineprint_core::scenario::parse_scenario` and run on the reference host `StateHost`, which
//! is loaded with every `*.json` card definition under the card library (`--cards`, default
//! `tests/fixtures/cards` under the workspace root: `git rev-parse --show-toplevel`, else the
//! current directory).
//!
//! Output: `PASS <file>` or `FAIL <file>` per scenario on stdout; for a failure, one line per
//! problem on stderr as `<file>:<line>:<col>: <pointer>: <message>` (line and column of the
//! offending value; `1:1` when the position is unknown). Exit codes: 0 every scenario passed
//! (one summary line on stdout), 1 a scenario failed an assertion, hit a script error or did not
//! parse, 2 usage or I/O error (no path, an unreadable path, a path holding no `.pzl` file, an
//! unreadable or invalid card library).

use std::io::Read;
use std::path::{Path, PathBuf};

use fineprint_core::scenario::{
    MAX_SCENARIO_BYTES, ScenarioError, StateHost, parse_scenario, pointer_position, run_scenario,
};

const USAGE: &str = "usage: cargo xtask pzl [--cards <dir>] <path.pzl|dir>...";

/// Runs the `pzl` command; returns the exit code.
pub fn run(args: &[String]) -> u8 {
    let mut cards: Option<PathBuf> = None;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--cards" {
            match it.next() {
                Some(d) if !d.starts_with("--") => cards = Some(PathBuf::from(d)),
                _ => {
                    eprintln!("pzl: --cards needs a directory\n{USAGE}");
                    return 2;
                }
            }
        } else if a.starts_with("--") {
            eprintln!("pzl: unknown option {a}\n{USAGE}");
            return 2;
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    if paths.is_empty() {
        eprintln!("pzl: no scenario path given\n{USAGE}");
        return 2;
    }
    let files = match discover(&paths) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("pzl: {e}");
            return 2;
        }
    };
    let cards = cards.unwrap_or_else(|| workspace_root().join("tests/fixtures/cards"));
    let library = match load_library(&cards) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pzl: {e}");
            return 2;
        }
    };
    let mut failed = 0usize;
    for file in &files {
        let bytes = match read_bounded(file) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("pzl: cannot read {}: {e}", file.display());
                return 2;
            }
        };
        let problems = match check_file(&bytes, &library) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pzl: {e}");
                return 2;
            }
        };
        if problems.is_empty() {
            println!("PASS {}", file.display());
        } else {
            failed += 1;
            println!("FAIL {}", file.display());
            for (line, column, message) in problems {
                eprintln!("{}:{line}:{column}: {message}", file.display());
            }
        }
    }
    if failed == 0 {
        println!("{}", summary(files.len()));
        0
    } else {
        eprintln!("pzl: {failed} of {} scenarios failed", files.len());
        1
    }
}

/// Reads `file`, at most `MAX_SCENARIO_BYTES + 1` bytes: the extra byte lets the parser reject an
/// oversized file with `TooLarge` without the whole file ever being held in memory (R12).
fn read_bounded(file: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(file)?
        .take(MAX_SCENARIO_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The success summary line.
fn summary(count: usize) -> String {
    let noun = if count == 1 { "scenario" } else { "scenarios" };
    format!("pzl: {count} {noun} passed")
}

/// Workspace root: `git rev-parse --show-toplevel`, else the current directory.
fn workspace_root() -> PathBuf {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Every `.pzl` file named by `paths`, each path's files sorted; an error for an unreadable
/// path or a path that holds no `.pzl` file.
fn discover(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths {
        let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        let mut found = Vec::new();
        if meta.is_dir() {
            walk(p, "pzl", &mut found)?;
            found.sort();
        } else if is_ext(p, "pzl") {
            found.push(p.clone());
        }
        if found.is_empty() {
            return Err(format!("{} holds no .pzl file", p.display()));
        }
        out.extend(found);
    }
    Ok(out)
}

fn is_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e == ext)
}

/// Collects the files with extension `ext` under `dir` (not following directory symlinks).
fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        paths.push(entry.path());
    }
    paths.sort();
    for p in paths {
        let meta = std::fs::symlink_metadata(&p)
            .map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        if meta.is_dir() {
            walk(&p, ext, out)?;
        } else if is_ext(&p, ext) {
            out.push(p);
        }
    }
    Ok(())
}

/// The bytes of every card definition under `dir`, sorted by path.
fn load_library(dir: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let meta = std::fs::metadata(dir)
        .map_err(|e| format!("cannot read the card library {}: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!(
            "the card library {} is not a directory",
            dir.display()
        ));
    }
    let mut files = Vec::new();
    walk(dir, "json", &mut files)?;
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let bytes = std::fs::read(&f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        out.push((f, bytes));
    }
    // Every definition must load (R11: no unverified definition is ever run).
    host_for(&out)?;
    Ok(out)
}

/// A reference host with every library card loaded.
fn host_for(library: &[(PathBuf, Vec<u8>)]) -> Result<StateHost, String> {
    let mut host = StateHost::new();
    for (path, bytes) in library {
        host.add_card_json(bytes)
            .map_err(|e| format!("card library {}: {e}", path.display()))?;
    }
    Ok(host)
}

/// The position of a loader error, `1:1` when unknown.
fn error_line(e: &ScenarioError) -> (u32, u32, String) {
    let shown = e.display_path();
    let pointer = if shown.is_empty() {
        "/"
    } else {
        shown.as_str()
    };
    (
        e.line.unwrap_or(1),
        e.column.unwrap_or(1),
        format!("{pointer}: {} ({})", e.message, e.kind),
    )
}

/// The problems of one scenario file, as (line, column, message); empty when it passes.
fn check_file(
    bytes: &[u8],
    library: &[(PathBuf, Vec<u8>)],
) -> Result<Vec<(u32, u32, String)>, String> {
    let scenario = match parse_scenario(bytes) {
        Ok(s) => s,
        Err(e) => return Ok(vec![error_line(&e)]),
    };
    let mut host = host_for(library)?;
    let report = run_scenario(&scenario, &mut host);
    if report.passed {
        return Ok(Vec::new());
    }
    Ok(report
        .problems()
        .into_iter()
        .map(|(pointer, message)| {
            let (line, column) = pointer_position(bytes, &pointer).unwrap_or((1, 1));
            let shown = if pointer.is_empty() { "/" } else { &pointer };
            (line, column, format!("{shown}: {message}"))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-pzl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A passing scenario over one library card; `life` is the asserted life total.
    fn scenario(life: i64) -> String {
        let def = std::fs::read_to_string(
            repo().join("tests/fixtures/cards/creatures/vanilla_white_1_1.json"),
        )
        .unwrap();
        let at = def.find("\"definitionHash\": \"").unwrap() + 19;
        let hash = &def[at..at + 64];
        format!(
            r#"{{
  "pzlVersion": 1,
  "title": "test xtask",
  "requirements": ["REQ-TEST-001"],
  "cr": [],
  "format": "testformat",
  "rulesBaseline": "test-baseline",
  "cardDatabase": {{"version": "test", "definitionHashes": {{"test.vanilla_white_1_1": "{hash}"}}}},
  "prngSeed": 1,
  "players": [{{"id": "p1", "life": 20, "zones": {{"hand": ["test.vanilla_white_1_1"]}}}}],
  "turn": {{"number": 1, "activePlayer": "p1", "phase": "PRECOMBAT_MAIN", "step": "MAIN"}},
  "priority": "p1",
  "script": [{{"answer": {{"type": "PASS_PRIORITY"}}}}],
  "assertions": [
    {{"kind": "life", "player": "p1", "equals": {life}}},
    {{"kind": "zone", "owner": "p1", "zone": "hand", "contents": ["p1.hand.0"]}}
  ]
}}
"#
        )
    }

    /// REQ-TEST-001, R12 (F82): a file over the parser limit is read only up to the limit plus
    /// one byte and fails with the parser's `TooLarge` message; a valid file reads unchanged.
    #[test]
    fn req_test_001_pzl_over_limit_file_is_too_large() {
        let dir = scratch("too-large");
        let big = dir.join("big.pzl");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_SCENARIO_BYTES as u64 * 4).unwrap();
        drop(f);
        let bytes = read_bounded(&big).unwrap();
        assert_eq!(bytes.len(), MAX_SCENARIO_BYTES + 1);
        let problems = check_file(&bytes, &[]).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].2.contains("too-large"), "{problems:?}");
        assert_eq!(run(&args(&[big.to_str().unwrap()])), 1);
        let ok = dir.join("ok.pzl");
        std::fs::write(&ok, scenario(20)).unwrap();
        assert_eq!(read_bounded(&ok).unwrap(), scenario(20).into_bytes());
        assert!(read_bounded(&dir.join("missing.pzl")).is_err());
    }

    /// REQ-TEST-001, R12 (F74): a diagnostic never prints a hostile key raw: control characters
    /// become `?`, a long segment is cut to 64 characters, and the line stays one line.
    #[test]
    fn req_test_001_pzl_diagnostic_path_is_sanitised() {
        let key = format!("{}\\u0001{}", "k".repeat(70), "z".repeat(10));
        let text = scenario(20).replace(
            "\"definitionHashes\": {",
            &format!("\"definitionHashes\": {{\"{key}\": \"bad\", "),
        );
        let problems = check_file(text.as_bytes(), &[]).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        let msg = &problems[0].2;
        assert!(!msg.chars().any(char::is_control), "{msg:?}");
        assert!(msg.contains(&format!("{}...", "k".repeat(64))), "{msg}");
        assert!(!msg.contains(&"k".repeat(65)), "{msg}");
    }

    /// REQ-TEST-001: `pzl` without a path, with a bad option and with an unreadable or empty
    /// path exits 2.
    #[test]
    fn req_test_001_pzl_usage_exit_codes() {
        assert_eq!(run(&[]), 2);
        assert_eq!(run(&args(&["--cards"])), 2);
        assert_eq!(run(&args(&["--bogus", "x.pzl"])), 2);
        assert_eq!(run(&args(&["/nonexistent/xtask-pzl/none.pzl"])), 2);
        let dir = scratch("empty");
        assert_eq!(run(&args(&[dir.to_str().unwrap()])), 2);
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(run(&args(&[dir.join("notes.txt").to_str().unwrap()])), 2);
        // A card library that does not exist.
        std::fs::write(dir.join("a.pzl"), scenario(20)).unwrap();
        assert_eq!(
            run(&args(&[
                "--cards",
                "/nonexistent/cards",
                dir.to_str().unwrap()
            ])),
            2
        );
        // An invalid card library.
        let bad = dir.join("cards");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("x.json"), "{}").unwrap();
        assert_eq!(
            run(&args(&[
                "--cards",
                bad.to_str().unwrap(),
                dir.join("a.pzl").to_str().unwrap()
            ])),
            2
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ-TEST-001: recursive discovery, sorted by path, `.pzl` files only.
    #[test]
    fn req_test_001_pzl_discovery_order() {
        let dir = scratch("order");
        std::fs::create_dir_all(dir.join("b/inner")).unwrap();
        std::fs::create_dir_all(dir.join("a")).unwrap();
        for f in [
            "b/inner/z.pzl",
            "b/a.pzl",
            "a/y.pzl",
            "c.pzl",
            "a/skip.json",
            "d.txt",
        ] {
            std::fs::write(dir.join(f), "{}").unwrap();
        }
        let found = discover(std::slice::from_ref(&dir)).unwrap();
        let rel: Vec<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(&dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(rel, ["a/y.pzl", "b/a.pzl", "b/inner/z.pzl", "c.pzl"]);
        // Explicit files keep the given order.
        let two = discover(&[dir.join("c.pzl"), dir.join("a/y.pzl")]).unwrap();
        assert_eq!(two, [dir.join("c.pzl"), dir.join("a/y.pzl")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ-TEST-001: a passing scenario exits 0; a wrong expectation or a malformed file exits
    /// 1 with a located problem.
    #[test]
    fn req_test_001_pzl_runs_scenarios() {
        let dir = scratch("run");
        let cards = repo().join("tests/fixtures/cards");
        let cards = cards.to_str().unwrap();
        std::fs::write(dir.join("pass.pzl"), scenario(20)).unwrap();
        assert_eq!(run(&args(&["--cards", cards, dir.to_str().unwrap()])), 0);
        std::fs::write(dir.join("wrong.pzl"), scenario(19)).unwrap();
        assert_eq!(run(&args(&["--cards", cards, dir.to_str().unwrap()])), 1);
        let library = load_library(Path::new(cards)).unwrap();
        let problems = check_file(scenario(19).as_bytes(), &library).unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!((problems[0].0, problems[0].1), (15, 5));
        assert!(
            problems[0]
                .2
                .starts_with("/assertions/0: assertion 0 (life)"),
            "{}",
            problems[0].2
        );
        let broken = check_file(b"{\n  \"pzlVersion\": 2\n}", &library).unwrap();
        assert_eq!((broken[0].0, broken[0].1), (2, 17));
        assert!(broken[0].2.starts_with("/pzlVersion: "));
        let too_big = check_file(&vec![b' '; 1_048_577], &library).unwrap();
        assert_eq!((too_big[0].0, too_big[0].1), (1, 1));
        assert!(too_big[0].2.starts_with("/: "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REQ-TEST-001: the one-line summary.
    #[test]
    fn req_test_001_pzl_summary_line() {
        assert_eq!(summary(1), "pzl: 1 scenario passed");
        assert_eq!(summary(3), "pzl: 3 scenarios passed");
    }
}
