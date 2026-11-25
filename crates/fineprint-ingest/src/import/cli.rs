// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `fetch` and `import` command lines (REQ-ING-001, REQ-ING-002, REQ-NFR-004).
//!
//! - `fineprint-ingest fetch [--cache <dir>] [--force] [--mtgjson]`
//! - `fineprint-ingest import [--cache <dir>] [--out <file>|-] [--mtgjson-report <file>|-]
//!   [--strict]`
//!
//! `--cache` defaults to [`DEFAULT_CACHE`] (relative to the current directory; run from the
//! repository root, where it is git-ignored). `import` writes `<cache>/normalized/cards.json`
//! unless `--out` names another file (`-` is stdout; the summary line then goes to stderr).
//! Files are written atomically (a temporary file next to the destination, then a rename).
//! The records file and the MTGJSON report must be different files (compared after resolving
//! the parent directory), and at most one of them may be stdout.
//! An option never takes another option as its value. Exit codes: 0 success, 1 violations
//! (`import --strict` with a rejection or diagnostic), 2 usage or input error. Messages never
//! quote card data.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fetch::{Curl, FetchOptions, Http, fetch, read_source_meta};
use super::mtgjson::{cross_check, parse_atomic};
use super::normalize::{ImportResult, SourceInfo, import_oracle_cards, import_rulings};
use super::{
    ATOMIC_CARDS_FILE, DEFAULT_CACHE, MAX_INPUT_BYTES, MTGJSON_DIR, NORMALIZED_DIR,
    NORMALIZED_FILE, ORACLE_CARDS_FILE, RULINGS_FILE, SCRYFALL_DIR, read_capped,
};
use crate::vocab::cli::write_atomic;
use fineprint_base::hash::sha256;

/// The `fetch` usage text.
pub const FETCH_USAGE: &str = "usage: fineprint-ingest fetch [--cache <dir>] [--force] [--mtgjson]";
/// The `import` usage text.
pub const IMPORT_USAGE: &str = "usage: fineprint-ingest import [--cache <dir>] [--out <file>|-] \
                                [--mtgjson-report <file>|-] [--strict]";

/// Every option of both subcommands; none of them is accepted as a value.
const OPTIONS: [&str; 7] = [
    "--cache",
    "--force",
    "--mtgjson",
    "--out",
    "--mtgjson-report",
    "--strict",
    "--help",
];

fn value<'a>(
    opt: &str,
    it: &mut impl Iterator<Item = &'a OsString>,
    slot: &mut Option<OsString>,
) -> Result<(), String> {
    let v = match it.next() {
        None => return Err(format!("{opt} needs a value")),
        Some(v) if OPTIONS.iter().any(|o| v == o) => {
            return Err(format!(
                "{opt} needs a value but is followed by the option {}",
                v.to_string_lossy()
            ));
        }
        Some(v) => v,
    };
    if slot.replace(v.clone()).is_some() {
        return Err(format!("{opt} given twice"));
    }
    Ok(())
}

/// Parsed `fetch` arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchArgs {
    /// The cache directory.
    pub cache: PathBuf,
    /// `--force`.
    pub force: bool,
    /// `--mtgjson`.
    pub mtgjson: bool,
}

/// Parses the `fetch` arguments.
///
/// # Errors
/// A usage message.
pub fn parse_fetch_args(args: &[OsString]) -> Result<FetchArgs, String> {
    let mut cache = None;
    let (mut force, mut mtgjson) = (false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--cache") => value("--cache", &mut it, &mut cache)?,
            Some("--force") => force = true,
            Some("--mtgjson") => mtgjson = true,
            _ => return Err(format!("unexpected argument {}", a.to_string_lossy())),
        }
    }
    Ok(FetchArgs {
        cache: PathBuf::from(cache.unwrap_or_else(|| OsString::from(DEFAULT_CACHE))),
        force,
        mtgjson,
    })
}

/// Parsed `import` arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportArgs {
    /// The cache directory.
    pub cache: PathBuf,
    /// The output file, `-` for stdout; `None` is `<cache>/normalized/cards.json`.
    pub out: Option<OsString>,
    /// The MTGJSON report destination, `-` for stdout.
    pub mtgjson_report: Option<OsString>,
    /// `--strict`.
    pub strict: bool,
}

/// Parses the `import` arguments.
///
/// # Errors
/// A usage message.
pub fn parse_import_args(args: &[OsString]) -> Result<ImportArgs, String> {
    let (mut cache, mut out, mut report) = (None, None, None);
    let mut strict = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--cache") => value("--cache", &mut it, &mut cache)?,
            Some("--out") => value("--out", &mut it, &mut out)?,
            Some("--mtgjson-report") => value("--mtgjson-report", &mut it, &mut report)?,
            Some("--strict") => strict = true,
            _ => return Err(format!("unexpected argument {}", a.to_string_lossy())),
        }
    }
    if out.as_deref().is_some_and(|o| o == "-") && report.as_deref().is_some_and(|r| r == "-") {
        return Err("--out - and --mtgjson-report - cannot both use stdout".to_owned());
    }
    Ok(ImportArgs {
        cache: PathBuf::from(cache.unwrap_or_else(|| OsString::from(DEFAULT_CACHE))),
        out,
        mtgjson_report: report,
        strict,
    })
}

/// Runs `fetch` with the system clock and `curl`; returns the exit code.
pub fn run_fetch(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    run_fetch_with(args, &Curl, now, stdout, stderr)
}

/// Runs `fetch` with an injected client and time (seconds since the Unix epoch); returns the
/// exit code (0 success, 2 usage, input or network error).
pub fn run_fetch_with(
    args: &[OsString],
    http: &dyn Http,
    now: u64,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let a = match parse_fetch_args(args) {
        Ok(a) => a,
        Err(m) => {
            let _ = writeln!(stderr, "fetch: {m}\n{FETCH_USAGE}");
            return 2;
        }
    };
    let opts = FetchOptions {
        cache: a.cache,
        force: a.force,
        mtgjson: a.mtgjson,
    };
    match fetch(http, &opts, now) {
        Ok(s) => {
            let _ = writeln!(stdout, "{s}");
            0
        }
        Err(e) => {
            let _ = writeln!(stderr, "fetch: {e}");
            2
        }
    }
}

/// The provenance of `bytes` and whether it differs from the cached digest. The cached
/// `updated_at` belongs to the bytes that were fetched, so it is kept only when the recomputed
/// digest matches the recorded one; otherwise `updatedAt` is unknown (`None`).
fn source_info(bytes: &[u8], cached: Option<&super::fetch::CachedFile>) -> (SourceInfo, bool) {
    let hash = sha256(bytes).to_hex();
    let mismatch = cached.is_some_and(|c| c.sha256 != hash);
    (
        SourceInfo {
            updated_at: cached
                .filter(|c| c.sha256 == hash)
                .and_then(|c| c.updated_at.clone()),
            sha256: hash,
            byte_count: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        },
        mismatch,
    )
}

/// Imports the Scryfall files of `cache` (REQ-ING-001, REQ-ING-002). Reads the clock and the
/// environment nowhere.
///
/// # Errors
/// A message for a missing or unusable Oracle Cards file, or an unusable Rulings file.
pub fn import_cache(cache: &Path) -> Result<ImportResult, String> {
    let sdir = cache.join(SCRYFALL_DIR);
    let oracle_path = sdir.join(ORACLE_CARDS_FILE);
    if !oracle_path.is_file() {
        return Err(format!(
            "{} is missing; run `fineprint-ingest fetch` first",
            oracle_path.display()
        ));
    }
    let bytes = read_capped(&oracle_path, MAX_INPUT_BYTES)
        .map_err(|e| format!("cannot read {}: {e}", oracle_path.display()))?;
    let meta = read_source_meta(&sdir);
    let mut warnings = Vec::new();
    if meta.is_empty() {
        warnings.push("sourceMetadataMissing".to_owned());
    }
    let (oracle_cards, mismatch) = source_info(&bytes, meta.get(ORACLE_CARDS_FILE));
    if mismatch {
        warnings.push("sourceHashMismatch:oracleCards".to_owned());
    }
    let oracle =
        import_oracle_cards(&bytes).map_err(|e| format!("{}: {e}", oracle_path.display()))?;
    drop(bytes);
    let rulings_path = sdir.join(RULINGS_FILE);
    let (rulings_source, rulings) = if rulings_path.is_file() {
        let bytes = read_capped(&rulings_path, MAX_INPUT_BYTES)
            .map_err(|e| format!("cannot read {}: {e}", rulings_path.display()))?;
        let (info, mismatch) = source_info(&bytes, meta.get(RULINGS_FILE));
        if mismatch {
            warnings.push("sourceHashMismatch:rulings".to_owned());
        }
        let r = import_rulings(&bytes, &oracle.records)
            .map_err(|e| format!("{}: {e}", rulings_path.display()))?;
        if r.unknown_oracle_id > 0 {
            warnings.push(format!("rulingsUnknownOracleId:{}", r.unknown_oracle_id));
        }
        if r.invalid > 0 {
            warnings.push(format!("rulingsInvalid:{}", r.invalid));
        }
        (Some(info), r.rulings)
    } else {
        (None, Default::default())
    };
    Ok(ImportResult {
        oracle_cards,
        rulings_source,
        oracle,
        rulings,
        warnings,
    })
}

/// Builds the MTGJSON report bytes for `result` from `<cache>/mtgjson/AtomicCards.json`.
///
/// # Errors
/// A message for a missing or unusable `AtomicCards` file.
pub fn mtgjson_report(cache: &Path, result: &ImportResult) -> Result<Vec<u8>, String> {
    let mdir = cache.join(MTGJSON_DIR);
    let path = mdir.join(ATOMIC_CARDS_FILE);
    if !path.is_file() {
        return Err(format!(
            "{} is missing; run `fineprint-ingest fetch --mtgjson` first",
            path.display()
        ));
    }
    let bytes = read_capped(&path, MAX_INPUT_BYTES)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let meta = read_source_meta(&mdir);
    let (info, mismatch) = source_info(&bytes, meta.get(ATOMIC_CARDS_FILE));
    let atomic = parse_atomic(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    drop(bytes);
    let mut source = info.to_json_value();
    if let Value::Object(m) = &mut source {
        m.insert("hashMismatch".to_owned(), Value::Bool(mismatch));
    }
    let report = cross_check(&result.oracle.records, &atomic, &source);
    let mut out = serde_json::to_vec_pretty(&report).unwrap_or_default();
    out.push(b'\n');
    Ok(out)
}

fn emit(
    dest: &OsString,
    bytes: &[u8],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    what: &str,
) -> Result<String, i32> {
    if dest == "-" {
        if stdout.write_all(bytes).is_err() {
            let _ = writeln!(stderr, "import: cannot write the {what} to stdout");
            return Err(2);
        }
        return Ok("stdout".to_owned());
    }
    let path = Path::new(dest);
    write_atomic(path, bytes).map_err(|e| {
        let _ = writeln!(
            stderr,
            "import: cannot write the {what} to {}: {e}",
            path.display()
        );
        2
    })?;
    Ok(path.display().to_string())
}

/// The file a non-stdout destination names, for comparing two destinations: the parent
/// directory canonicalized (symlinks and `.`/`..` resolved) joined with the file name. When the
/// parent cannot be resolved (it does not exist), the path is kept as written; writing to it
/// then fails on its own.
fn resolve_dest(dest: &Path) -> PathBuf {
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    match (dest.file_name(), std::fs::canonicalize(parent)) {
        (Some(name), Ok(dir)) => dir.join(name),
        _ => std::fs::canonicalize(dest).unwrap_or_else(|_| dest.to_path_buf()),
    }
}

/// True when `report` already exists and is the same file as `records` (same device and inode
/// on Unix, the equivalent file identity on Windows): two names that alias on the file system,
/// such as names differing only in case on a case-insensitive file system, which a spelling
/// comparison cannot tell apart. A missing file, or an unreadable one, is not the same file.
/// A symbolic link as the report is never checked: the atomic rename replaces the link itself
/// and cannot reach the records file (REQ-ING-001, REQ-NFR-004).
fn report_aliases_records(records: &Path, report: &Path) -> bool {
    if std::fs::symlink_metadata(report).is_ok_and(|m| m.file_type().is_symlink()) {
        return false;
    }
    same_file::is_same_file(records, report).unwrap_or(false)
}

const SAME_FILE_MESSAGE: &str = "the records file and --mtgjson-report name the same file";

/// Runs `import`; returns the exit code.
pub fn run_import(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let a = match parse_import_args(args) {
        Ok(a) => a,
        Err(m) => {
            let _ = writeln!(stderr, "import: {m}\n{IMPORT_USAGE}");
            return 2;
        }
    };
    let result = match import_cache(&a.cache) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "import: {e}");
            return 2;
        }
    };
    let report = match &a.mtgjson_report {
        None => None,
        Some(dest) => match mtgjson_report(&a.cache, &result) {
            Ok(b) => Some((dest, b)),
            Err(e) => {
                let _ = writeln!(stderr, "import: {e}");
                return 2;
            }
        },
    };
    let out = a.out.clone().unwrap_or_else(|| {
        let dir = a.cache.join(NORMALIZED_DIR);
        dir.join(NORMALIZED_FILE).into_os_string()
    });
    if a.out.is_none() {
        let dir = a.cache.join(NORMALIZED_DIR);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            let _ = writeln!(stderr, "import: cannot create {}: {e}", dir.display());
            return 2;
        }
    }
    // Two file destinations naming one file would let the report replace the records.
    if let Some((dest, _)) = &report
        && out != "-"
        && *dest != "-"
        && resolve_dest(Path::new(&out)) == resolve_dest(Path::new(dest))
    {
        let _ = writeln!(stderr, "import: {SAME_FILE_MESSAGE}\n{IMPORT_USAGE}");
        return 2;
    }
    let to_stdout = out == "-" || report.as_ref().is_some_and(|(d, _)| *d == "-");
    let written = match emit(&out, &result.to_bytes(), stdout, stderr, "records") {
        Ok(w) => w,
        Err(c) => return c,
    };
    let mut report_note = String::new();
    if let Some((dest, bytes)) = &report {
        // Names that alias only on the file system (case-insensitive) have no identity before
        // the records exist: check now, before the report could replace them.
        if out != "-" && *dest != "-" && report_aliases_records(Path::new(&out), Path::new(dest)) {
            let _ = writeln!(stderr, "import: {SAME_FILE_MESSAGE}\n{IMPORT_USAGE}");
            return 2;
        }
        match emit(dest, bytes, stdout, stderr, "MTGJSON report") {
            Ok(w) => report_note = format!("; MTGJSON report to {w}"),
            Err(c) => return c,
        }
    }
    let diagnosed = result.records_with_diagnostics();
    let rejected = result.oracle.rejected.len();
    let summary = format!(
        "import: wrote {written} ({} records, {rejected} rejected, {diagnosed} with diagnostics, \
         {} with rulings, {} warnings){report_note}",
        result.oracle.records.len(),
        result.rulings.len(),
        result.warnings.len(),
    );
    let sink: &mut dyn Write = if to_stdout { stderr } else { stdout };
    let _ = writeln!(sink, "{summary}");
    if a.strict && (rejected > 0 || diagnosed > 0) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::report_aliases_records;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A fresh scratch directory (exclusive `create_dir`; the leaf only needs to be unique).
    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("fp-ingest-cli-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir(&d).expect("create scratch dir");
        d
    }

    /// REQ-ING-001, REQ-NFR-004 (gate review R4): the identity helper. Two names of one file
    /// are the same; two distinct files and a missing report are not; a symbolic link as the
    /// report is not checked.
    #[test]
    fn destination_identity_helper() {
        let d = scratch();
        let records = d.join("records.json");
        std::fs::write(&records, b"{}").unwrap();
        // The same path is the same file.
        assert!(report_aliases_records(&records, &records));
        // A distinct file is not.
        let other = d.join("other.json");
        std::fs::write(&other, b"{}").unwrap();
        assert!(!report_aliases_records(&records, &other));
        // A missing report is not.
        assert!(!report_aliases_records(&records, &d.join("missing.json")));
        #[cfg(unix)]
        {
            // A hard link is another name of the same file.
            let link = d.join("hard.json");
            std::fs::hard_link(&records, &link).unwrap();
            assert!(report_aliases_records(&records, &link));
            // A symbolic link report is never checked, even when it points at the records.
            let sym = d.join("sym.json");
            std::os::unix::fs::symlink(&records, &sym).unwrap();
            assert!(!report_aliases_records(&records, &sym));
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
